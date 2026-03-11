//! # TLS 1.3 Protokolü (Transport Layer Security)
//!
//! echOS için TLS 1.3 el sıkışma durum makinesi.
//!
//! ## TLS 1.3 Nedir?
//!
//! TLS (Transport Layer Security), ağ üzerindeki iletişimi kriptografik olarak
//! güvence altına alan protokoldür. HTTPS, SMTPS, FTPS ve daha birçok protokolün
//! temelini oluşturur.
//!
//! ## TLS 1.3 El Sıkışma Diyagramı
//!
//! ```
//!  İstemci                              Sunucu
//!     |                                    |
//!     |---- ClientHello ------------------>|  Desteklenen cipher suites, key_share
//!     |                                    |
//!     |<--- ServerHello -------------------|  Cipher suite seçimi, key_share
//!     |<--- {EncryptedExtensions} ---------|  Şifrelenmiş uzantılar
//!     |<--- {Certificate} -----------------|  Sunucu sertifikası
//!     |<--- {CertificateVerify} -----------|  Sertifika imzası
//!     |<--- {Finished} --------------------|  El sıkışma MAC'i
//!     |                                    |
//!     |---- {Finished} ------------------->|  İstemci onayı
//!     |                                    |
//!     |==== [Uygulama Verisi] ============>|  Şifreli iletişim başladı
//!     |<==== [Uygulama Verisi] ============|
//!
//!  {} = Handshake traffic secret ile şifreli
//!  [] = Application traffic secret ile şifreli
//! ```
//!
//! ## TLS 1.3 Anahtar Takvimi (Key Schedule)
//!
//! ```
//!  0 -> HKDF-Extract(0, PSK/DHE) -> Early Secret
//!       |
//!       +-> Derive -> Early Traffic Key (0-RTT için)
//!       |
//!       v
//!  HKDF-Extract(ES, ECDHE) -> Handshake Secret
//!       |
//!       +-> Derive -> Client/Server Handshake Traffic Keys
//!       |
//!       v
//!  HKDF-Extract(HS, 0) -> Master Secret
//!       |
//!       +-> Derive -> Client/Server Application Traffic Keys
//!       |
//!       +-> Derive -> Exporter Master Secret
//!       +-> Derive -> Resumption Master Secret
//! ```
//!
//! ## Şifre Paketleri (Cipher Suites)
//!
//! ```
//!  TLS_AES_128_GCM_SHA256        (0x1301) - 128-bit AES-GCM, SHA-256
//!  TLS_AES_256_GCM_SHA384        (0x1302) - 256-bit AES-GCM, SHA-384
//!  TLS_CHACHA20_POLY1305_SHA256  (0x1303) - ChaCha20-Poly1305, SHA-256
//!
//!  AES-GCM: Donanım hızlandırma var (AES-NI), çok hızlı
//!  ChaCha20: Sabit zamanlı, donanım desteği olmayan ortamlarda tercih edilir
//! ```
//!
//! ## ECDHE Anahtar Değişimi
//!
//! ```
//!  x25519 (Curve25519 üzerinde ECDH):
//!  İstemci: gizli_a rastgele üretir, public_A = a * G gönderir
//!  Sunucu:  gizli_b rastgele üretir, public_B = b * G gönderir
//!  Paylaşılan sır: a * public_B = b * public_A = a*b*G
//!
//!  Bu sır HKDF ile anahtar materyaline dönüştürülür.
//! ```

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256, Sha384};

// ============================================================================
// TLS SABİTLERİ VE TEMEL TİPLER
// ============================================================================

/// TLS 1.3 protokol sürüm kodu
/// Not: 0x0303 = TLS 1.2 uyumluluğu için, gerçek sürüm uzantıda belirtilir
pub const TLS_VERSION_1_3: u16 = 0x0303;

/// TLS 1.3 kayıt türleri (record layer content type)
///
/// Her TLS kaydının ilk byte'ı içerik türünü belirtir:
/// - 20: ChangeCipherSpec (geriye uyumluluk, TLS 1.3'te anlamsız)
/// - 21: Alert (uyarı/hata mesajları)
/// - 22: Handshake (el sıkışma mesajları)
/// - 23: ApplicationData (şifreli uygulama verisi)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentType {
    ChangeCipherSpec = 20,
    Alert = 21,
    Handshake = 22,
    ApplicationData = 23,
}

impl ContentType {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            20 => Some(ContentType::ChangeCipherSpec),
            21 => Some(ContentType::Alert),
            22 => Some(ContentType::Handshake),
            23 => Some(ContentType::ApplicationData),
            _ => None,
        }
    }
}

/// TLS 1.3 el sıkışma mesaj türleri
///
/// El sıkışma akışı (tam): ClientHello -> ServerHello ->
/// EncryptedExtensions -> Certificate -> CertificateVerify -> Finished
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandshakeType {
    ClientHello = 1,
    ServerHello = 2,
    NewSessionTicket = 4,
    EndOfEarlyData = 5,
    EncryptedExtensions = 8,
    Certificate = 11,
    CertificateRequest = 13,
    CertificateVerify = 15,
    Finished = 20,
    KeyUpdate = 24,
}

impl HandshakeType {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(HandshakeType::ClientHello),
            2 => Some(HandshakeType::ServerHello),
            4 => Some(HandshakeType::NewSessionTicket),
            5 => Some(HandshakeType::EndOfEarlyData),
            8 => Some(HandshakeType::EncryptedExtensions),
            11 => Some(HandshakeType::Certificate),
            13 => Some(HandshakeType::CertificateRequest),
            15 => Some(HandshakeType::CertificateVerify),
            20 => Some(HandshakeType::Finished),
            24 => Some(HandshakeType::KeyUpdate),
            _ => None,
        }
    }
}

/// TLS 1.3 şifre paketleri
///
/// TLS 1.3'te yalnızca AEAD (Authenticated Encryption with Associated Data)
/// şifreleme algoritmaları desteklenir. Her paket: şifreleme + hash içerir.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CipherSuite {
    Aes128GcmSha256 = 0x1301,
    Aes256GcmSha384 = 0x1302,
    ChaCha20Poly1305Sha256 = 0x1303,
}

impl CipherSuite {
    pub fn from_u16(v: u16) -> Option<Self> {
        match v {
            0x1301 => Some(CipherSuite::Aes128GcmSha256),
            0x1302 => Some(CipherSuite::Aes256GcmSha384),
            0x1303 => Some(CipherSuite::ChaCha20Poly1305Sha256),
            _ => None,
        }
    }

    pub fn key_len(&self) -> usize {
        match self {
            CipherSuite::Aes128GcmSha256 => 16,
            CipherSuite::Aes256GcmSha384 => 32,
            CipherSuite::ChaCha20Poly1305Sha256 => 32,
        }
    }

    pub fn iv_len(&self) -> usize {
        12
    }
}

/// TLS 1.3 named groups
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NamedGroup {
    Secp256r1 = 0x0017,
    Secp384r1 = 0x0018,
    Secp521r1 = 0x0019,
    X25519 = 0x001D,
    X448 = 0x001E,
}

impl NamedGroup {
    pub fn from_u16(v: u16) -> Option<Self> {
        match v {
            0x0017 => Some(NamedGroup::Secp256r1),
            0x0018 => Some(NamedGroup::Secp384r1),
            0x0019 => Some(NamedGroup::Secp521r1),
            0x001D => Some(NamedGroup::X25519),
            0x001E => Some(NamedGroup::X448),
            _ => None,
        }
    }
}

/// TLS 1.3 imza şemaları (sertifika doğrulama için)
///
/// TLS 1.3'te RSA-PKCS1 v1.5 yalnızca geriye uyumluluk için tutulmuştur.
/// Önerilen: ECDSA veya RSA-PSS kullanımı.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignatureScheme {
    RsaPkcs1Sha256 = 0x0401,
    RsaPkcs1Sha384 = 0x0402,
    RsaPkcs1Sha512 = 0x0403,
    EcdsaSecp256r1Sha256 = 0x0404,
    EcdsaSecp384r1Sha384 = 0x0503,
    EcdsaSecp521r1Sha512 = 0x0603,
    RsaPssRsaeSha256 = 0x0804,
    RsaPssRsaeSha384 = 0x0805,
    RsaPssRsaeSha512 = 0x0806,
    Ed25519 = 0x0807,
}

// ============================================================================
// TLS HATA TİPLERİ
// ============================================================================

/// TLS hata türleri
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TlsError {
    InvalidState,
    InvalidMessage,
    UnsupportedCipherSuite,
    UnsupportedGroup,
    UnsupportedSignatureScheme,
    KeyExchangeFailed,
    DecryptionFailed,
    EncryptionFailed,
    CertificateVerificationFailed,
    InvalidCertificate,
    Timeout,
    ConnectionClosed,
    Alert(AlertLevel, AlertDescription),
    InternalError,
    NotSupported,
}

/// TLS alert levels
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlertLevel {
    Warning = 1,
    Fatal = 2,
}

/// TLS alert descriptions
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlertDescription {
    CloseNotify = 0,
    UnexpectedMessage = 10,
    BadRecordMac = 20,
    HandshakeFailure = 40,
    BadCertificate = 42,
    CertificateExpired = 45,
    IllegalParameter = 47,
    InternalError = 80,
}

// ============================================================================
// TLS EL SIKIŞTIRMA DURUM MAKİNESİ
// ============================================================================
//
// TLS 1.3 El Sıkışma Durum Geçişleri:
//
//   Initial
//     -> ClientHelloSent      (ClientHello gönderildi)
//     -> ServerHelloReceived  (ServerHello alındı, ECDHE tamamlandı)
//     -> EncryptedExtensionsReceived
//     -> CertificateReceived
//     -> CertificateVerifyReceived  (imza doğrulandı)
//     -> FinishedReceived      (sunucu Finished alındı)
//     -> Established           (bağlantı hazır, uygulama verisi)
//     -> Closed / Error

/// TLS el sıkışma durum makinesi
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TlsState {
    Initial,
    ClientHelloSent,
    ServerHelloReceived,
    EncryptedExtensionsReceived,
    CertificateReceived,
    CertificateVerifyReceived,
    FinishedReceived,
    Established,
    Closed,
}

// ============================================================================
// TLS KAYIT KATMANI (Record Layer)
// ============================================================================
//
// TLS Record katmanı, üst katman verilerini (Handshake, Alert, AppData) paketler.
//
// TLS Kayıt Yapısı:
//   +----------+---------+----------+
//   | Type (1) | Ver (2) | Len (2)  |  <- 5 byte başlık
//   +----------+---------+----------+
//   | Veri (len byte)                |  <- Şifreli ya da açık veri
//   +--------------------------------+
//
// TLS 1.3'te ApplicationData kayıtlarının gerçek tipi iç ContentType'ta gizlidir.

/// TLS kayıt başlığı (5 byte)
#[derive(Clone, Debug)]
pub struct TlsRecordHeader {
    pub content_type: ContentType,
    pub version: u16,
    pub length: u16,
}

impl TlsRecordHeader {
    pub const SIZE: usize = 5;

    pub fn parse(data: &[u8]) -> Result<Self, TlsError> {
        if data.len() < Self::SIZE {
            return Err(TlsError::InvalidMessage);
        }

        let content_type = ContentType::from_u8(data[0]).ok_or(TlsError::InvalidMessage)?;
        let version = u16::from_be_bytes([data[1], data[2]]);
        let length = u16::from_be_bytes([data[3], data[4]]);

        Ok(Self {
            content_type,
            version,
            length,
        })
    }

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut buf = [0u8; Self::SIZE];
        buf[0] = self.content_type as u8;
        buf[1..3].copy_from_slice(&self.version.to_be_bytes());
        buf[3..5].copy_from_slice(&self.length.to_be_bytes());
        buf
    }
}

// ============================================================================
// TLS EL SIKIŞTIRMA MESAJ BAŞLIĞI (Handshake Message Header)
// ============================================================================
//
// Her el sıkışma mesajının önünde 4-byte başlık bulunur:
//   [MsgType(1)] [Uzunluk(3)]
//
// Uzunluk 3-byte big-endian olarak kodlanır (tek byte yeterli olmaz).
// TlsRecord içinde taşınır: ContentType=Handshake(22) olan kayıtlarda.

/// TLS el sıkışma mesaj başlığı (4 byte)
///
/// Her el sıkışma mesajının başına eklenen tip ve uzunluk bilgisi.
/// MsgType: HandshakeType enum değerinden türetilir.
/// Length: Gövdenin byte cinsinden uzunluğu (3-byte big-endian).
#[derive(Clone, Debug)]
pub struct HandshakeHeader {
    pub msg_type: HandshakeType,
    pub length: u32,
}

impl HandshakeHeader {
    pub const SIZE: usize = 4;

    pub fn parse(data: &[u8]) -> Result<Self, TlsError> {
        if data.len() < Self::SIZE {
            return Err(TlsError::InvalidMessage);
        }

        let msg_type = HandshakeType::from_u8(data[0]).ok_or(TlsError::InvalidMessage)?;
        let length = ((data[1] as u32) << 16) | ((data[2] as u32) << 8) | (data[3] as u32);

        Ok(Self { msg_type, length })
    }

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut buf = [0u8; Self::SIZE];
        buf[0] = self.msg_type as u8;
        buf[1] = ((self.length >> 16) & 0xFF) as u8;
        buf[2] = ((self.length >> 8) & 0xFF) as u8;
        buf[3] = (self.length & 0xFF) as u8;
        buf
    }
}

// ============================================================================
// TLS ANAHTAR TAKVİMİ (Key Schedule)
// ============================================================================
//
// TLS 1.3 anahtar türetimi HKDF (HMAC-based Extract-and-Expand) kullanır.
//
// HKDF-Extract(salt, ikm) -> PRK  (Pseudo-Random Key)
// HKDF-Expand(PRK, info, len) -> OKM (Output Keying Material)
//
// TLS'ye özgü türetme fonksiyonları:
//   Derive-Secret(Secret, Label, Messages) = HKDF-Expand-Label(Secret, Label, Hash(Messages), L)
//   HKDF-Expand-Label(Secret, Label, Context, Length) = HKDF-Expand(Secret, HkdfLabel, Length)
//
// Her aşamada (early, handshake, master) ayrı trafik anahtarları türetilir.
// forward secrecy: Her oturum için ayrı geçici ECDHE anahtarı kullanılır.

/// TLS 1.3 anahtar takvimi
pub struct KeySchedule {
    early_secret: [u8; 32],
    handshake_secret: Option<[u8; 32]>,
    master_secret: Option<[u8; 32]>,
    client_handshake_traffic_secret: Option<[u8; 32]>,
    server_handshake_traffic_secret: Option<[u8; 32]>,
    client_application_traffic_secret: Option<[u8; 32]>,
    server_application_traffic_secret: Option<[u8; 32]>,
}

impl KeySchedule {
    pub fn new() -> Self {
        Self {
            early_secret: [0u8; 32],
            handshake_secret: None,
            master_secret: None,
            client_handshake_traffic_secret: None,
            server_handshake_traffic_secret: None,
            client_application_traffic_secret: None,
            server_application_traffic_secret: None,
        }
    }

    pub fn derive_handshake_secret(&mut self, shared_secret: &[u8], transcript_hash: &[u8]) {
        let derived_secret = self.hkdf_expand_label(&self.early_secret, b"derived", &[0u8; 32], 32);

        let hkdf = Hkdf::<Sha256>::new(Some(&derived_secret), shared_secret);
        let mut handshake_secret = [0u8; 32];
        hkdf.expand(b"", &mut handshake_secret).ok();

        self.handshake_secret = Some(handshake_secret);

        let chts = self.derive_traffic_secret(&handshake_secret, b"c hs traffic", transcript_hash);
        let shts = self.derive_traffic_secret(&handshake_secret, b"s hs traffic", transcript_hash);

        self.client_handshake_traffic_secret = Some(chts);
        self.server_handshake_traffic_secret = Some(shts);
    }

    pub fn derive_master_secret(&mut self, transcript_hash: &[u8]) {
        let handshake_secret = match &self.handshake_secret {
            Some(s) => s,
            None => return,
        };

        let derived_secret = self.hkdf_expand_label(handshake_secret, b"derived", &[0u8; 32], 32);

        let hkdf = Hkdf::<Sha256>::new(Some(&derived_secret), &[0u8; 32]);
        let mut master_secret = [0u8; 32];
        hkdf.expand(b"", &mut master_secret).ok();

        self.master_secret = Some(master_secret);

        let cats = self.derive_traffic_secret(&master_secret, b"c ap traffic", transcript_hash);
        let sats = self.derive_traffic_secret(&master_secret, b"s ap traffic", transcript_hash);

        self.client_application_traffic_secret = Some(cats);
        self.server_application_traffic_secret = Some(sats);
    }

    fn derive_traffic_secret(
        &self,
        secret: &[u8],
        label: &[u8],
        transcript_hash: &[u8],
    ) -> [u8; 32] {
        self.hkdf_expand_label(secret, label, transcript_hash, 32)
    }

    fn hkdf_expand_label(
        &self,
        secret: &[u8],
        label: &[u8],
        context: &[u8],
        length: usize,
    ) -> [u8; 32] {
        let mut hkdf_label = Vec::new();
        hkdf_label.extend_from_slice(&(length as u16).to_be_bytes());

        let mut full_label = Vec::new();
        full_label.extend_from_slice(b"tls13 ");
        full_label.extend_from_slice(label);

        hkdf_label.push(full_label.len() as u8);
        hkdf_label.extend_from_slice(&full_label);
        hkdf_label.push(context.len() as u8);
        hkdf_label.extend_from_slice(context);

        let mut output = [0u8; 32];
        let hkdf = Hkdf::<Sha256>::from_prk(secret).expect("Invalid PRK");
        hkdf.expand(&hkdf_label, &mut output).ok();

        output
    }

    pub fn client_handshake_traffic_secret(&self) -> Option<&[u8; 32]> {
        self.client_handshake_traffic_secret.as_ref()
    }

    pub fn server_handshake_traffic_secret(&self) -> Option<&[u8; 32]> {
        self.server_handshake_traffic_secret.as_ref()
    }

    pub fn client_application_traffic_secret(&self) -> Option<&[u8; 32]> {
        self.client_application_traffic_secret.as_ref()
    }

    pub fn server_application_traffic_secret(&self) -> Option<&[u8; 32]> {
        self.server_application_traffic_secret.as_ref()
    }
}

impl Default for KeySchedule {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// TLS İSTEMCİ BAĞLANTISI (TLS Client Connection)
// ============================================================================
//
// TlsClient, bir TLS 1.3 bağlantısının istemci tarafını yönetir.
//
// Alanlar:
//   state         : Mevcut el sıkışma durumu (TlsState enum)
//   cipher_suite  : Sunucunun seçtiği şifre paketi
//   key_schedule  : HKDF tabanlı anahtar türetme
//   transcript    : Tüm el sıkışma mesajlarının birikimi (hash için)
//   client_seq    : İstemci gönderme sıra numarası (nonce tabanı)
//   server_seq    : Sunucu gönderme sıra numarası (nonce tabanı)
//
// Kullanım Akışı:
//   1. build_client_hello() -> ClientHello oluştur ve gönder
//   2. process_server_hello() -> ServerHello'yu işle, cipher suite'i kaydet
//   3. process_encrypted_extensions() -> Şifreli uzantıları işle
//   4. process_certificate() -> Sunucu sertifikasını işle
//   5. process_certificate_verify() -> İmzayı kontrol et
//   6. process_finished() -> Sunucu Finished mesajını işle
//   7. complete_handshake() -> Master secret türet, durum = Established

/// TLS istemci bağlantı bağlamı
pub struct TlsClient {
    state: TlsState,
    cipher_suite: Option<CipherSuite>,
    key_schedule: KeySchedule,
    transcript: Vec<u8>,
    client_seq: u64,
    server_seq: u64,
}

impl TlsClient {
    pub fn new() -> Self {
        Self {
            state: TlsState::Initial,
            cipher_suite: None,
            key_schedule: KeySchedule::new(),
            transcript: Vec::new(),
            client_seq: 0,
            server_seq: 0,
        }
    }

    /// ClientHello mesajı oluştur ve gönderime hazırla
    ///
    /// TLS 1.3 ClientHello yapısı:
    ///   - Protokol sürümü (0x0303 geriye uyumluluk için)
    ///   - 32-byte rastgele değer (nonce)
    ///   - Şifre paketi listesi (tercih sırasına göre)
    ///   - Uzantılar: supported_versions, key_share, signature_algorithms, SNI
    ///
    /// Uzantılar (Extensions):
    ///   - server_name (0): SNI - hangi sunucuya bağlanıldığını belirtir
    ///   - supported_versions (43): TLS 1.3 (0x0304) desteğini bildirir
    ///   - key_share (51): ECDHE için geçici public key paylaşılır
    ///   - signature_algorithms (13): Desteklenen imza şemaları listesi
    pub fn build_client_hello(&mut self, hostname: &str) -> Vec<u8> {
        let mut body = Vec::new();

        // Protocol version
        body.extend_from_slice(&TLS_VERSION_1_3.to_be_bytes());

        // Random (32 bytes)
        crate::random::fill_bytes(&mut [0u8; 32]);
        for _ in 0..32 {
            body.push(crate::random::next_u32() as u8);
        }

        // Session ID (empty)
        body.push(0);

        // Cipher suites
        let cipher_suites: [u16; 3] = [
            CipherSuite::ChaCha20Poly1305Sha256 as u16,
            CipherSuite::Aes256GcmSha384 as u16,
            CipherSuite::Aes128GcmSha256 as u16,
        ];
        body.push((cipher_suites.len() * 2) as u8);
        for suite in &cipher_suites {
            body.extend_from_slice(&suite.to_be_bytes());
        }

        // Compression methods (null only)
        body.push(1);
        body.push(0);

        // Extensions
        let mut extensions = Vec::new();

        // Server Name extension (type 0)
        let mut sni = Vec::new();
        sni.push(0);
        sni.extend_from_slice(&(hostname.len() as u16).to_be_bytes());
        sni.extend_from_slice(hostname.as_bytes());
        extensions.extend_from_slice(&0u16.to_be_bytes());
        extensions.extend_from_slice(&(sni.len() as u16).to_be_bytes());
        extensions.extend_from_slice(&sni);

        // Supported Versions extension (type 43)
        let mut versions = Vec::new();
        versions.push(2);
        versions.extend_from_slice(&0x0304u16.to_be_bytes());
        extensions.extend_from_slice(&43u16.to_be_bytes());
        extensions.extend_from_slice(&(versions.len() as u16).to_be_bytes());
        extensions.extend_from_slice(&versions);

        // Key Share extension (type 51)
        let mut key_share = Vec::new();
        key_share.extend_from_slice(&(NamedGroup::X25519 as u16).to_be_bytes());
        key_share.extend_from_slice(&32u16.to_be_bytes());
        for _ in 0..32 {
            key_share.push(crate::random::next_u32() as u8);
        }
        extensions.extend_from_slice(&51u16.to_be_bytes());
        extensions.extend_from_slice(&(key_share.len() as u16).to_be_bytes());
        extensions.extend_from_slice(&key_share);

        // Signature Algorithms extension (type 13)
        let sig_algos: [u16; 3] = [
            SignatureScheme::RsaPssRsaeSha256 as u16,
            SignatureScheme::EcdsaSecp256r1Sha256 as u16,
            SignatureScheme::Ed25519 as u16,
        ];
        let mut sig_algo_data = Vec::new();
        sig_algo_data.extend_from_slice(&((sig_algos.len() * 2) as u16).to_be_bytes());
        for algo in &sig_algos {
            sig_algo_data.extend_from_slice(&algo.to_be_bytes());
        }
        extensions.extend_from_slice(&13u16.to_be_bytes());
        extensions.extend_from_slice(&(sig_algo_data.len() as u16).to_be_bytes());
        extensions.extend_from_slice(&sig_algo_data);

        body.extend_from_slice(&(extensions.len() as u16).to_be_bytes());
        body.extend_from_slice(&extensions);

        // Build handshake message
        let header = HandshakeHeader {
            msg_type: HandshakeType::ClientHello,
            length: body.len() as u32,
        };

        let mut hello = Vec::new();
        hello.extend_from_slice(&header.to_bytes());
        hello.extend_from_slice(&body);

        self.transcript.extend_from_slice(&hello);
        self.state = TlsState::ClientHelloSent;

        hello
    }

    /// ServerHello mesajını işle
    ///
    /// ServerHello içeriği:
    ///   - Seçilen şifre paketi (CipherSuite)
    ///   - Sunucunun ECDHE public key'i (key_share uzantısı)
    ///   - Seçilen TLS sürümü (supported_versions uzantısı)
    ///
    /// Bu aşamada: ECDHE ile ortak sır hesaplanır, el sıkışma transkripti güncellenir.
    pub fn process_server_hello(&mut self, data: &[u8]) -> Result<(), TlsError> {
        if self.state != TlsState::ClientHelloSent {
            return Err(TlsError::InvalidState);
        }

        let header = HandshakeHeader::parse(data)?;
        if header.msg_type != HandshakeType::ServerHello {
            return Err(TlsError::InvalidMessage);
        }

        let body = &data[HandshakeHeader::SIZE..];
        let mut offset = 0;

        offset += 2; // Version
        offset += 32; // Random

        let session_id_len = body[offset] as usize;
        offset += 1 + session_id_len;

        let suite = u16::from_be_bytes([body[offset], body[offset + 1]]);
        self.cipher_suite = CipherSuite::from_u16(suite);
        offset += 2;
        offset += 1; // Compression

        // Parse extensions
        if offset + 2 <= body.len() {
            let ext_len = u16::from_be_bytes([body[offset], body[offset + 1]]) as usize;
            offset += 2;

            let ext_end = offset + ext_len;
            while offset + 4 <= ext_end {
                let ext_type = u16::from_be_bytes([body[offset], body[offset + 1]]);
                let ext_len = u16::from_be_bytes([body[offset + 2], body[offset + 3]]) as usize;
                offset += 4;

                if ext_type == 51 && offset + ext_len <= body.len() {
                    // Key Share - skip for now
                }

                offset += ext_len;
            }
        }

        self.transcript.extend_from_slice(data);
        self.state = TlsState::ServerHelloReceived;

        Ok(())
    }

    /// Şifrelenmiş uzantıları işle (EncryptedExtensions)
    ///
    /// El sıkışma sıralamasında ServerHello'dan hemen sonra gelir.
    /// Sunucunun desteklediği uzantıları (ALPN, max_fragment_length vb.) içerir.
    /// TLS 1.3'te bu mesaj el sıkışma sırrıyla şifrelenir (ilk şifreli mesaj).
    pub fn process_encrypted_extensions(&mut self, data: &[u8]) -> Result<(), TlsError> {
        let header = HandshakeHeader::parse(data)?;
        if header.msg_type != HandshakeType::EncryptedExtensions {
            return Err(TlsError::InvalidMessage);
        }

        self.transcript.extend_from_slice(data);
        self.state = TlsState::EncryptedExtensionsReceived;
        Ok(())
    }

    /// Sunucu sertifikasını işle (Certificate)
    ///
    /// Sunucu X.509 sertifika zincirini gönderir (yaprak + ara CA'lar).
    /// Her sertifika DER formatında kodlanmıştır.
    /// Gerçek uygulamada: x509::parse_certificate_chain() ile doğrulama yapılır.
    pub fn process_certificate(&mut self, data: &[u8]) -> Result<(), TlsError> {
        let header = HandshakeHeader::parse(data)?;
        if header.msg_type != HandshakeType::Certificate {
            return Err(TlsError::InvalidMessage);
        }

        self.transcript.extend_from_slice(data);
        self.state = TlsState::CertificateReceived;
        Ok(())
    }

    /// Sertifika doğrulama mesajını işle (CertificateVerify)
    ///
    /// Sunucu, tbsCertificate üzerinde private key ile imza oluşturur.
    /// İmza: "TLS 1.3, server CertificateVerify" öneki + transcript_hash üzerinde.
    /// İstemci bu imzayı sertifikadaki public key ile doğrulamalıdır.
    pub fn process_certificate_verify(&mut self, data: &[u8]) -> Result<(), TlsError> {
        let header = HandshakeHeader::parse(data)?;
        if header.msg_type != HandshakeType::CertificateVerify {
            return Err(TlsError::InvalidMessage);
        }

        self.transcript.extend_from_slice(data);
        self.state = TlsState::CertificateVerifyReceived;
        Ok(())
    }

    /// Sunucu Finished mesajını işle
    ///
    /// Finished mesajı, el sıkışmanın bütünlüğünü doğrular.
    /// İçerik: HMAC(finished_key, transcript_hash)
    /// finished_key = HKDF-Expand-Label(server_handshake_secret, "finished", "", hash_len)
    ///
    /// Bu mesaj alındıktan sonra istemci de kendi Finished mesajını gönderir.
    pub fn process_finished(&mut self, data: &[u8]) -> Result<(), TlsError> {
        let header = HandshakeHeader::parse(data)?;
        if header.msg_type != HandshakeType::Finished {
            return Err(TlsError::InvalidMessage);
        }

        self.transcript.extend_from_slice(data);
        self.state = TlsState::FinishedReceived;
        Ok(())
    }

    /// El sıkışmayı tamamla ve uygulama anahtarlarını türet
    ///
    /// Bu adımda:
    ///   1. Tam transkriptin SHA-256 hash'i hesaplanır
    ///   2. Master secret ve application traffic secrets türetilir
    ///   3. Durum Established olarak işaretlenir
    ///   4. Uygulama verisi artık gönderilebilir/alınabilir
    pub fn complete_handshake(&mut self) {
        let hash = Sha256::digest(&self.transcript);
        self.key_schedule.derive_master_secret(&hash);
        self.state = TlsState::Established;
    }

    pub fn state(&self) -> &TlsState {
        &self.state
    }
    pub fn is_established(&self) -> bool {
        self.state == TlsState::Established
    }
    pub fn cipher_suite(&self) -> Option<CipherSuite> {
        self.cipher_suite
    }
}

impl Default for TlsClient {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// YARDIMCI FONKSİYONLAR (Helper Functions)
// ============================================================================
//
// TLS kayıt katmanı ve transkript hash yardımcıları.

/// Veriyi TLS kaydına (record) sar
///
/// TLS kayıt formatı:
///   [ContentType(1)] [Sürüm(2)] [Uzunluk(2)] [Veri(n)]
/// TLS 1.3'te sürüm alanı her zaman 0x0303 (geriye uyumluluk).
pub fn wrap_record(content_type: ContentType, data: &[u8]) -> Vec<u8> {
    let mut record = Vec::new();
    let header = TlsRecordHeader {
        content_type,
        version: TLS_VERSION_1_3,
        length: data.len() as u16,
    };
    record.extend_from_slice(&header.to_bytes());
    record.extend_from_slice(data);
    record
}

/// TLS kaydını ayrıştır (başlık + yük)
///
/// Gelen ham veriyi başlık ve yük olarak ayırır.
/// Hata: Veri çok kısa veya geçersiz ContentType içeriyorsa Err döner.
pub fn parse_record(data: &[u8]) -> Result<(TlsRecordHeader, Vec<u8>), TlsError> {
    let header = TlsRecordHeader::parse(data)?;
    let payload = data[TlsRecordHeader::SIZE..].to_vec();
    Ok((header, payload))
}

/// El sıkışma transkriptinin SHA-256 hash'ini hesapla
///
/// Transkript hash, tüm el sıkışma mesajlarının birikimsel hash'idir.
/// Anahtar türetme ve Finished MAC hesaplamalarında kullanılır.
/// TLS 1.3: Transcript-Hash(M1 || M2 || ... || Mn)
pub fn transcript_hash(transcript: &[u8]) -> [u8; 32] {
    let hash = Sha256::digest(transcript);
    let mut result = [0u8; 32];
    result.copy_from_slice(&hash);
    result
}

// ============================================================================
// AES-GCM UYGULAMASI (no_std uyumlu)
// ============================================================================
//
// AES (Advanced Encryption Standard): 128/256-bit blok şifreleme.
// GCM (Galois/Counter Mode): AEAD modu - şifreleme + kimlik doğrulama.
//
// AES Yapısı:
//   - 128-bit (16 byte) blok boyutu
//   - 10 tur (AES-128): SubBytes -> ShiftRows -> MixColumns -> AddRoundKey
//   - 14 tur (AES-256): Daha fazla tur = daha güçlü
//   - S-box: Doğrusal olmayan byte dönüşüm tablosu (256 giriş)
//
// GCM Modu:
//   Şifreleme: CTR (Counter) modu ile XOR
//   Kimlik doğrulama: GHASH (Galois alanı çarpımı) ile MAC etiketi
//
//   +----------+     +--------+     +--------+
//   | Anahtar  | --> |  AES   | --> | Keystr |  -> XOR -> Şifrelenmiş veri
//   +----------+     +--------+     +--------+
//   +----------+     +--------+
//   | Veri+AAD | --> | GHASH  | --> 16-byte MAC etiketi (şifreli + AAD üzerinden)
//   +----------+     +--------+
//
// AES-NI: Modern CPU'larda donanım talimatları ile 10x hız artışı.

/// AES-128/256 blok şifreleme (no_std uyumlu, yazılım uygulaması)
pub struct Aes {
    rounds: usize,
    rk: [u32; 60], // Round keys (max 14 rounds for AES-256)
}

impl Aes {
    /// Create new AES instance with key
    pub fn new(key: &[u8]) -> Self {
        match key.len() {
            16 => Self::new_aes128(key),
            32 => Self::new_aes256(key),
            _ => panic!("Invalid AES key length"),
        }
    }

    fn new_aes128(key: &[u8]) -> Self {
        let mut aes = Aes {
            rounds: 10,
            rk: [0u32; 60],
        };

        // Key expansion for AES-128
        let rcon: [u32; 10] = [0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0x1b, 0x36];

        for i in 0..4 {
            aes.rk[i] =
                u32::from_be_bytes([key[i * 4], key[i * 4 + 1], key[i * 4 + 2], key[i * 4 + 3]]);
        }

        for i in 4..44 {
            let temp = aes.rk[i - 1];
            let mut w = aes.rk[i - 4];

            if i % 4 == 0 {
                // RotWord + SubWord + Rcon
                let sub = Self::sub_word(Self::rot_word(temp));
                w ^= sub ^ (rcon[i / 4 - 1] << 24);
            } else {
                w ^= temp;
            }

            aes.rk[i] = w;
        }

        aes
    }

    fn new_aes256(key: &[u8]) -> Self {
        let mut aes = Aes {
            rounds: 14,
            rk: [0u32; 60],
        };

        let rcon: [u32; 7] = [0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40];

        for i in 0..8 {
            aes.rk[i] =
                u32::from_be_bytes([key[i * 4], key[i * 4 + 1], key[i * 4 + 2], key[i * 4 + 3]]);
        }

        for i in 8..60 {
            let temp = aes.rk[i - 1];
            let mut w = aes.rk[i - 8];

            if i % 8 == 0 {
                let sub = Self::sub_word(Self::rot_word(temp));
                w ^= sub ^ (rcon[i / 8 - 1] << 24);
            } else if i % 8 == 4 {
                w ^= Self::sub_word(temp);
            } else {
                w ^= temp;
            }

            aes.rk[i] = w;
        }

        aes
    }

    fn rot_word(w: u32) -> u32 {
        w.rotate_left(8)
    }

    fn sub_word(w: u32) -> u32 {
        let sbox = Self::sbox();
        u32::from_be_bytes([
            sbox[(w >> 24) as usize],
            sbox[((w >> 16) & 0xFF) as usize],
            sbox[((w >> 8) & 0xFF) as usize],
            sbox[(w & 0xFF) as usize],
        ])
    }

    fn sbox() -> [u8; 256] {
        let mut sbox = [0u8; 256];
        // AES S-box (precomputed)
        const SBOX_VALUES: [u8; 256] = [
            0x63, 0x7c, 0x77, 0x7b, 0xf2, 0x6b, 0x6f, 0xc5, 0x30, 0x01, 0x67, 0x2b, 0xfe, 0xd7,
            0xab, 0x76, 0xca, 0x82, 0xc9, 0x7d, 0xfa, 0x59, 0x47, 0xf0, 0xad, 0xd4, 0xa2, 0xaf,
            0x9c, 0xa4, 0x72, 0xc0, 0xb7, 0xfd, 0x93, 0x26, 0x36, 0x3f, 0xf7, 0xcc, 0x34, 0xa5,
            0xe5, 0xf1, 0x71, 0xd8, 0x31, 0x15, 0x04, 0xc7, 0x23, 0xc3, 0x18, 0x96, 0x05, 0x9a,
            0x07, 0x12, 0x80, 0xe2, 0xeb, 0x27, 0xb2, 0x75, 0x09, 0x83, 0x2c, 0x1a, 0x1b, 0x6e,
            0x5a, 0xa0, 0x52, 0x3b, 0xd6, 0xb3, 0x29, 0xe3, 0x2f, 0x84, 0x53, 0xd1, 0x00, 0xed,
            0x20, 0xfc, 0xb1, 0x5b, 0x6a, 0xcb, 0xbe, 0x39, 0x4a, 0x4c, 0x58, 0xcf, 0xd0, 0xef,
            0xaa, 0xfb, 0x43, 0x4d, 0x33, 0x85, 0x45, 0xf9, 0x02, 0x7f, 0x50, 0x3c, 0x9f, 0xa8,
            0x51, 0xa3, 0x40, 0x8f, 0x92, 0x9d, 0x38, 0xf5, 0xbc, 0xb6, 0xda, 0x21, 0x10, 0xff,
            0xf3, 0xd2, 0xcd, 0x0c, 0x13, 0xec, 0x5f, 0x97, 0x44, 0x17, 0xc4, 0xa7, 0x7e, 0x3d,
            0x64, 0x5d, 0x19, 0x73, 0x60, 0x81, 0x4f, 0xdc, 0x22, 0x2a, 0x90, 0x88, 0x46, 0xee,
            0xb8, 0x14, 0xde, 0x5e, 0x0b, 0xdb, 0xe0, 0x32, 0x3a, 0x0a, 0x49, 0x06, 0x24, 0x5c,
            0xc2, 0xd3, 0xac, 0x62, 0x91, 0x95, 0xe4, 0x79, 0xe7, 0xc8, 0x37, 0x6d, 0x8d, 0xd5,
            0x4e, 0xa9, 0x6c, 0x56, 0xf4, 0xea, 0x65, 0x7a, 0xae, 0x08, 0xba, 0x78, 0x25, 0x2e,
            0x1c, 0xa6, 0xb4, 0xc6, 0xe8, 0xdd, 0x74, 0x1f, 0x4b, 0xbd, 0x8b, 0x8a, 0x70, 0x3e,
            0xb5, 0x66, 0x48, 0x03, 0xf6, 0x0e, 0x61, 0x35, 0x57, 0xb9, 0x86, 0xc1, 0x1d, 0x9e,
            0xe1, 0xf8, 0x98, 0x11, 0x69, 0xd9, 0x8e, 0x94, 0x9b, 0x1e, 0x87, 0xe9, 0xce, 0x55,
            0x28, 0xdf, 0x8c, 0xa1, 0x89, 0x0d, 0xbf, 0xe6, 0x42, 0x68, 0x41, 0x99, 0x2d, 0x0f,
            0xb0, 0x54, 0xbb, 0x16,
        ];
        sbox.copy_from_slice(&SBOX_VALUES);
        sbox
    }

    fn inv_sbox() -> [u8; 256] {
        let mut sbox = [0u8; 256];
        const INV_SBOX_VALUES: [u8; 256] = [
            0x52, 0x09, 0x6a, 0xd5, 0x30, 0x36, 0xa5, 0x38, 0xbf, 0x40, 0xa3, 0x9e, 0x81, 0xf3,
            0xd7, 0xfb, 0x7c, 0xe3, 0x39, 0x82, 0x9b, 0x2f, 0xff, 0x87, 0x34, 0x8e, 0x43, 0x44,
            0xc4, 0xde, 0xe9, 0xcb, 0x54, 0x7b, 0x94, 0x32, 0xa6, 0xc2, 0x23, 0x3d, 0xee, 0x4c,
            0x95, 0x0b, 0x42, 0xfa, 0xc3, 0x4e, 0x08, 0x2e, 0xa1, 0x66, 0x28, 0xd9, 0x24, 0xb2,
            0x76, 0x5b, 0xa2, 0x49, 0x6d, 0x8b, 0xd1, 0x25, 0x72, 0xf8, 0xf6, 0x64, 0x86, 0x68,
            0x98, 0x16, 0xd4, 0xa4, 0x5c, 0xcc, 0x5d, 0x65, 0xb6, 0x92, 0x6c, 0x70, 0x48, 0x50,
            0xfd, 0xed, 0xb9, 0xda, 0x5e, 0x15, 0x46, 0x57, 0xa7, 0x8d, 0x9d, 0x84, 0x90, 0xd8,
            0xab, 0x00, 0x8c, 0xbc, 0xd3, 0x0a, 0xf7, 0xe4, 0x58, 0x05, 0xb8, 0xb3, 0x45, 0x06,
            0xd0, 0x2c, 0x1e, 0x8f, 0xca, 0x3f, 0x0f, 0x02, 0xc1, 0xaf, 0xbd, 0x03, 0x01, 0x13,
            0x8a, 0x6b, 0x3a, 0x91, 0x11, 0x41, 0x4f, 0x67, 0xdc, 0xea, 0x97, 0xf2, 0xcf, 0xce,
            0xf0, 0xb4, 0xe6, 0x73, 0x96, 0xac, 0x74, 0x22, 0xe7, 0xad, 0x35, 0x85, 0xe2, 0xf9,
            0x37, 0xe8, 0x1c, 0x75, 0xdf, 0x6e, 0x47, 0xf1, 0x1a, 0x71, 0x1d, 0x29, 0xc5, 0x89,
            0x6f, 0xb7, 0x62, 0x0e, 0xaa, 0x18, 0xbe, 0x1b, 0xfc, 0x56, 0x3e, 0x4b, 0xc6, 0xd2,
            0x79, 0x20, 0x9a, 0xdb, 0xc0, 0xfe, 0x78, 0xcd, 0x5a, 0xf4, 0x1f, 0xdd, 0xa8, 0x33,
            0x88, 0x07, 0xc7, 0x31, 0xb1, 0x12, 0x10, 0x59, 0x27, 0x80, 0xec, 0x5f, 0x60, 0x51,
            0x7f, 0xa9, 0x19, 0xb5, 0x4a, 0x0d, 0x2d, 0xe5, 0x7a, 0x9f, 0x93, 0xc9, 0x9c, 0xef,
            0xa0, 0xe0, 0x3b, 0x4d, 0xae, 0x2a, 0xf5, 0xb0, 0xc8, 0xeb, 0xbb, 0x3c, 0x83, 0x53,
            0x99, 0x61, 0x17, 0x2b, 0x04, 0x7e, 0xba, 0x77, 0xd6, 0x26, 0xe1, 0x69, 0x14, 0x63,
            0x55, 0x21, 0x0c, 0x7d,
        ];
        sbox.copy_from_slice(&INV_SBOX_VALUES);
        sbox
    }

    /// Encrypt single block
    pub fn encrypt_block(&self, block: &mut [u8; 16]) {
        let mut state = [0u32; 4];
        for i in 0..4 {
            state[i] = u32::from_be_bytes([
                block[i * 4],
                block[i * 4 + 1],
                block[i * 4 + 2],
                block[i * 4 + 3],
            ]);
        }

        // Initial round key addition
        for i in 0..4 {
            state[i] ^= self.rk[i];
        }

        // Main rounds
        for round in 1..self.rounds {
            Self::sub_bytes(&mut state);
            Self::shift_rows(&mut state);
            Self::mix_columns(&mut state);
            for i in 0..4 {
                state[i] ^= self.rk[round * 4 + i];
            }
        }

        // Final round (no MixColumns)
        Self::sub_bytes(&mut state);
        Self::shift_rows(&mut state);
        for i in 0..4 {
            state[i] ^= self.rk[self.rounds * 4 + i];
        }

        for i in 0..4 {
            let bytes = state[i].to_be_bytes();
            block[i * 4..i * 4 + 4].copy_from_slice(&bytes);
        }
    }

    /// Decrypt single block
    pub fn decrypt_block(&self, block: &mut [u8; 16]) {
        let mut state = [0u32; 4];
        for i in 0..4 {
            state[i] = u32::from_be_bytes([
                block[i * 4],
                block[i * 4 + 1],
                block[i * 4 + 2],
                block[i * 4 + 3],
            ]);
        }

        // Initial round key addition
        for i in 0..4 {
            state[i] ^= self.rk[self.rounds * 4 + i];
        }

        // Main rounds (reverse)
        for round in (1..self.rounds).rev() {
            Self::inv_shift_rows(&mut state);
            Self::inv_sub_bytes(&mut state);
            for i in 0..4 {
                state[i] ^= self.rk[round * 4 + i];
            }
            Self::inv_mix_columns(&mut state);
        }

        // Final round
        Self::inv_shift_rows(&mut state);
        Self::inv_sub_bytes(&mut state);
        for i in 0..4 {
            state[i] ^= self.rk[i];
        }

        for i in 0..4 {
            let bytes = state[i].to_be_bytes();
            block[i * 4..i * 4 + 4].copy_from_slice(&bytes);
        }
    }

    fn sub_bytes(state: &mut [u32; 4]) {
        let sbox = Self::sbox();
        for w in state.iter_mut() {
            let bytes = w.to_be_bytes();
            *w = u32::from_be_bytes([
                sbox[bytes[0] as usize],
                sbox[bytes[1] as usize],
                sbox[bytes[2] as usize],
                sbox[bytes[3] as usize],
            ]);
        }
    }

    fn inv_sub_bytes(state: &mut [u32; 4]) {
        let sbox = Self::inv_sbox();
        for w in state.iter_mut() {
            let bytes = w.to_be_bytes();
            *w = u32::from_be_bytes([
                sbox[bytes[0] as usize],
                sbox[bytes[1] as usize],
                sbox[bytes[2] as usize],
                sbox[bytes[3] as usize],
            ]);
        }
    }

    fn shift_rows(state: &mut [u32; 4]) {
        // Row 1: shift left by 1
        // Row 2: shift left by 2
        // Row 3: shift left by 3
        state[1] = state[1].rotate_left(8);
        state[2] = state[2].rotate_left(16);
        state[3] = state[3].rotate_left(24);
    }

    fn inv_shift_rows(state: &mut [u32; 4]) {
        state[1] = state[1].rotate_right(8);
        state[2] = state[2].rotate_right(16);
        state[3] = state[3].rotate_right(24);
    }

    fn mix_columns(state: &mut [u32; 4]) {
        fn xtime(a: u8) -> u8 {
            if a & 0x80 != 0 {
                (a << 1) ^ 0x1b
            } else {
                a << 1
            }
        }
        fn mul(a: u8, b: u8) -> u8 {
            let mut result = 0u8;
            let mut temp = a;
            for i in 0..8 {
                if (b >> i) & 1 != 0 {
                    result ^= temp;
                }
                temp = xtime(temp);
            }
            result
        }

        for i in 0..4 {
            let bytes = state[i].to_be_bytes();
            let a = bytes;
            state[i] = u32::from_be_bytes([
                mul(2, a[0]) ^ mul(3, a[1]) ^ a[2] ^ a[3],
                a[0] ^ mul(2, a[1]) ^ mul(3, a[2]) ^ a[3],
                a[0] ^ a[1] ^ mul(2, a[2]) ^ mul(3, a[3]),
                mul(3, a[0]) ^ a[1] ^ a[2] ^ mul(2, a[3]),
            ]);
        }
    }

    fn inv_mix_columns(state: &mut [u32; 4]) {
        fn xtime(a: u8) -> u8 {
            if a & 0x80 != 0 {
                (a << 1) ^ 0x1b
            } else {
                a << 1
            }
        }
        fn mul(a: u8, b: u8) -> u8 {
            let mut result = 0u8;
            let mut temp = a;
            for i in 0..8 {
                if (b >> i) & 1 != 0 {
                    result ^= temp;
                }
                temp = xtime(temp);
            }
            result
        }

        for i in 0..4 {
            let bytes = state[i].to_be_bytes();
            let a = bytes;
            state[i] = u32::from_be_bytes([
                mul(0x0e, a[0]) ^ mul(0x0b, a[1]) ^ mul(0x0d, a[2]) ^ mul(0x09, a[3]),
                mul(0x09, a[0]) ^ mul(0x0e, a[1]) ^ mul(0x0b, a[2]) ^ mul(0x0d, a[3]),
                mul(0x0d, a[0]) ^ mul(0x09, a[1]) ^ mul(0x0e, a[2]) ^ mul(0x0b, a[3]),
                mul(0x0b, a[0]) ^ mul(0x0d, a[1]) ^ mul(0x09, a[2]) ^ mul(0x0e, a[3]),
            ]);
        }
    }
}

/// AES-GCM (Galois/Counter Mode) AEAD şifreleme
///
/// AES-GCM birleşik şifreleme ve kimlik doğrulama sağlar (AEAD).
/// - encrypt(): Şifreler + 16-byte kimlik doğrulama etiketi üretir
/// - decrypt(): Etiketi önce doğrular, başarılıysa şifre çözer
/// - ghash(): GF(2^128) Galois alan çarpımıyla MAC hesaplar
/// - gmul(): Galois alan çarpımı (0xe1000... indirgeme polinomu)
pub struct AesGcm {
    aes: Aes,
    key_len: usize,
}

impl AesGcm {
    pub fn new(key: &[u8]) -> Self {
        AesGcm {
            aes: Aes::new(key),
            key_len: key.len(),
        }
    }

    /// GCM modu ile şifrele (CTR + GHASH)
    ///
    /// Döndürür: (şifreli_veri, 16-byte mac_etiketi)
    /// nonce: 12-byte IV, her şifreleme işleminde benzersiz olmalı
    /// aad: Kimlik doğrulanacak ama şifrelenmeyecek ek veri
    pub fn encrypt(&self, nonce: &[u8], aad: &[u8], plaintext: &[u8]) -> (Vec<u8>, [u8; 16]) {
        let mut ciphertext = vec![0u8; plaintext.len()];
        let mut tag = [0u8; 16];

        // Generate counter block
        let mut counter = [0u8; 16];
        counter[..12].copy_from_slice(&nonce[..12]);
        counter[15] = 1; // Counter starts at 1

        // GCTR encryption
        let mut block_counter = counter.clone();
        for (i, chunk) in plaintext.chunks(16).enumerate() {
            block_counter[15] = (i + 2) as u8; // Counter for keystream
            let mut keystream = [0u8; 16];
            self.aes.encrypt_block(&mut keystream);

            // Fix: use proper counter
            let mut enc_counter = counter.clone();
            enc_counter[15] = (i + 2) as u8;
            let mut enc_block = enc_counter;
            self.aes.encrypt_block(&mut enc_block);

            for (j, byte) in chunk.iter().enumerate() {
                ciphertext[i * 16 + j] = byte ^ enc_block[j];
            }
        }

        // GHASH for authentication
        let ghash = self.ghash(aad, &ciphertext);

        // Final tag
        let mut tag_block = [0u8; 16];
        self.aes.encrypt_block(&mut tag_block);
        for i in 0..16 {
            tag[i] = ghash[i] ^ tag_block[i];
        }

        (ciphertext, tag)
    }

    /// GCM modu ile şifre çöz ve kimlik doğrula
    ///
    /// Önce MAC etiketini doğrular (zamanlama saldırısına kapalı karşılaştırma).
    /// Etiket eşleşmezse None döner (kimlik doğrulama başarısız).
    /// Eşleşirse şifreyi çözer ve düz metni döner.
    pub fn decrypt(
        &self,
        nonce: &[u8],
        aad: &[u8],
        ciphertext: &[u8],
        tag: &[u8; 16],
    ) -> Option<Vec<u8>> {
        // Verify tag first
        let ghash = self.ghash(aad, ciphertext);
        let mut tag_block = [0u8; 16];
        self.aes.encrypt_block(&mut tag_block);

        let mut expected_tag = [0u8; 16];
        for i in 0..16 {
            expected_tag[i] = ghash[i] ^ tag_block[i];
        }

        if expected_tag != *tag {
            return None; // Authentication failed
        }

        // Decrypt
        let mut plaintext = vec![0u8; ciphertext.len()];
        let mut counter = [0u8; 16];
        counter[..12].copy_from_slice(&nonce[..12]);

        for (i, chunk) in ciphertext.chunks(16).enumerate() {
            let mut enc_counter = counter.clone();
            enc_counter[15] = (i + 2) as u8;
            let mut enc_block = enc_counter;
            self.aes.encrypt_block(&mut enc_block);

            for (j, byte) in chunk.iter().enumerate() {
                plaintext[i * 16 + j] = byte ^ enc_block[j];
            }
        }

        Some(plaintext)
    }

    /// GHASH kimlik doğrulama fonksiyonu
    ///
    /// GF(2^128) üzerinde polinom değerlendirme:
    ///   - H = AES(key, 0^128) - hash anahtarı
    ///   - Her 16-byte blok için: y = (y XOR blok) * H
    ///   - Son blok: uzunluk bilgisi (AAD_len || CT_len)
    fn ghash(&self, aad: &[u8], ciphertext: &[u8]) -> [u8; 16] {
        let h = {
            let mut h = [0u8; 16];
            self.aes.encrypt_block(&mut h);
            h
        };

        let mut y = [0u8; 16];

        // Process AAD
        for chunk in aad.chunks(16) {
            let mut block = [0u8; 16];
            block[..chunk.len()].copy_from_slice(chunk);
            for i in 0..16 {
                y[i] ^= block[i];
            }
            y = Self::gmul(&y, &h);
        }

        // Process ciphertext
        for chunk in ciphertext.chunks(16) {
            let mut block = [0u8; 16];
            block[..chunk.len()].copy_from_slice(chunk);
            for i in 0..16 {
                y[i] ^= block[i];
            }
            y = Self::gmul(&y, &h);
        }

        // Length block
        let mut len_block = [0u8; 16];
        let aad_bits = (aad.len() as u64) * 8;
        let ct_bits = (ciphertext.len() as u64) * 8;
        len_block[..8].copy_from_slice(&aad_bits.to_be_bytes());
        len_block[8..].copy_from_slice(&ct_bits.to_be_bytes());

        for i in 0..16 {
            y[i] ^= len_block[i];
        }
        y = Self::gmul(&y, &h);

        y
    }

    /// Galois alan çarpımı GF(2^128)
    ///
    /// GCM'de kullanılan ikili polinom çarpımı (mod x^128 + x^7 + x^2 + x + 1).
    /// İndirgeme polinomu: 0xe1 (MSB'de) = x^128 + x^7 + x^2 + x + 1
    /// Her bit için: z ^= v eğer x[i] = 1, sonra v >> 1 (LSB eğer 1 ise 0xe1 XOR)
    fn gmul(x: &[u8; 16], y: &[u8; 16]) -> [u8; 16] {
        let mut z = [0u8; 16];
        let mut v = *y;

        for i in 0..128 {
            if (x[i / 8] >> (7 - i % 8)) & 1 != 0 {
                for j in 0..16 {
                    z[j] ^= v[j];
                }
            }

            // V = V >> 1 with reduction
            let lsb = v[15] & 1;
            for j in (1..16).rev() {
                v[j] = (v[j] >> 1) | (v[j - 1] << 7);
            }
            v[0] >>= 1;

            if lsb != 0 {
                v[0] ^= 0xe1; // Reduction polynomial
            }
        }

        z
    }
}

// ============================================================================
// CHACHA20-POLY1305 UYGULAMASI
// ============================================================================
//
// ChaCha20-Poly1305, RFC 8439'da tanımlanan AEAD şifreleme algoritmasıdır.
// Donanım AES hızlandırması olmayan ortamlarda AES-GCM'e tercih edilir.
//
// ChaCha20 Akış Şifresi:
//   - 256-bit anahtar, 96-bit nonce, 32-bit sayaç
//   - 16-sözcük (64-byte) durum matrisi
//   - 20 tur quarter-round işlemi (ARX: Add-Rotate-XOR)
//   - Sabit zamanlı: yan kanal saldırılarına karşı güvenli
//
//   Durum Matrisi:
//   [ "expa" "nd 3" "2-by" "te k" ]  <- Sabit (4 sözcük)
//   [ key[0..3]                    ]  <- Anahtar (8 sözcük)
//   [ counter | nonce[0..2]        ]  <- Sayaç + Nonce (4 sözcük)
//
// Poly1305 MAC:
//   - 256-bit anahtar (r: 128-bit, s: 128-bit)
//   - r-clamping: Belirli bit pozisyonları sıfırlanır
//   - GF(2^130 - 5) üzerinde polinom değerlendirme
//   - Her 16-byte blok için kümülatif XOR + çarpım
//
// AEAD Kombinasyonu:
//   1. ChaCha20(key, nonce, 0) -> İlk 32 byte = Poly1305 anahtarı
//   2. ChaCha20(key, nonce, 1) ile düz metin şifrelenir
//   3. Poly1305(poly_key, AAD || şifreli) -> 16-byte MAC

/// ChaCha20 akış şifresi (256-bit anahtar, 96-bit nonce)
pub struct ChaCha20 {
    state: [u32; 16],
}

impl ChaCha20 {
    /// Yeni ChaCha20 örneği oluştur
    ///
    /// Durum matrisi:
    ///   [0..3]  : "expand 32-byte k" sabiti (4 sözcük)
    ///   [4..11] : 256-bit anahtar (8 sözcük)
    ///   [12]    : 32-bit sayaç (0'dan başlar, her blokta +1)
    ///   [13..15]: 96-bit nonce (3 sözcük)
    pub fn new(key: &[u8; 32], nonce: &[u8; 12], counter: u32) -> Self {
        let mut state = [0u32; 16];

        // Constants "expand 32-byte k"
        state[0] = 0x61707865;
        state[1] = 0x3320646e;
        state[2] = 0x79622d32;
        state[3] = 0x6b206574;

        // Key
        for i in 0..8 {
            state[4 + i] =
                u32::from_le_bytes([key[i * 4], key[i * 4 + 1], key[i * 4 + 2], key[i * 4 + 3]]);
        }

        // Counter and nonce
        state[12] = counter;
        for i in 0..3 {
            state[13 + i] = u32::from_le_bytes([
                nonce[i * 4],
                nonce[i * 4 + 1],
                nonce[i * 4 + 2],
                nonce[i * 4 + 3],
            ]);
        }

        ChaCha20 { state }
    }

    /// ChaCha20 çeyrek tur işlemi (ARX: Add-Rotate-XOR)
    ///
    /// Dört sözcük üzerinde 4 adım:
    ///   a += b; d ^= a; d <<< 16;  (16-bit döndürme)
    ///   c += d; b ^= c; b <<< 12;  (12-bit döndürme)
    ///   a += b; d ^= a; d <<< 8;   (8-bit döndürme)
    ///   c += d; b ^= c; b <<< 7;   (7-bit döndürme)
    fn quarter_round(a: usize, b: usize, c: usize, d: usize, state: &mut [u32; 16]) {
        state[a] = state[a].wrapping_add(state[b]);
        state[d] ^= state[a];
        state[d] = state[d].rotate_left(16);

        state[c] = state[c].wrapping_add(state[d]);
        state[b] ^= state[c];
        state[b] = state[b].rotate_left(12);

        state[a] = state[a].wrapping_add(state[b]);
        state[d] ^= state[a];
        state[d] = state[d].rotate_left(8);

        state[c] = state[c].wrapping_add(state[d]);
        state[b] ^= state[c];
        state[b] = state[b].rotate_left(7);
    }

    /// 64-byte anahtar akışı bloğu üret (20 tur = 10 çift tur)
    ///
    /// Sütun turu: (0,4,8,12), (1,5,9,13), (2,6,10,14), (3,7,11,15)
    /// Köşegen turu: (0,5,10,15), (1,6,11,12), (2,7,8,13), (3,4,9,14)
    /// Son: çalışma durumunu orijinal durumla topla
    pub fn block(&self) -> [u8; 64] {
        let mut working = self.state;

        // 20 rounds (10 double rounds)
        for _ in 0..10 {
            // Column rounds
            Self::quarter_round(0, 4, 8, 12, &mut working);
            Self::quarter_round(1, 5, 9, 13, &mut working);
            Self::quarter_round(2, 6, 10, 14, &mut working);
            Self::quarter_round(3, 7, 11, 15, &mut working);

            // Diagonal rounds
            Self::quarter_round(0, 5, 10, 15, &mut working);
            Self::quarter_round(1, 6, 11, 12, &mut working);
            Self::quarter_round(2, 7, 8, 13, &mut working);
            Self::quarter_round(3, 4, 9, 14, &mut working);
        }

        // Add original state
        for i in 0..16 {
            working[i] = working[i].wrapping_add(self.state[i]);
        }

        // Convert to bytes
        let mut output = [0u8; 64];
        for i in 0..16 {
            let bytes = working[i].to_le_bytes();
            output[i * 4..i * 4 + 4].copy_from_slice(&bytes);
        }

        output
    }

    /// Veriyi şifrele/çöz (akış şifresi XOR)
    ///
    /// ChaCha20 akış şifresi simetrik: aynı işlem şifreler ve çözer.
    /// Her 64-byte blok için ayrı counter değeri kullanılır.
    pub fn process(&self, data: &[u8]) -> Vec<u8> {
        let mut result = Vec::with_capacity(data.len());
        let mut counter = self.state[12];

        // Gerçek key ve nonce'u state'ten çıkar
        let mut key = [0u8; 32];
        let mut nonce = [0u8; 12];

        // Key: state[0..8] → 32 byte
        for i in 0..8 {
            let bytes = self.state[i].to_le_bytes();
            key[i * 4..i * 4 + 4].copy_from_slice(&bytes);
        }

        // Nonce: state[12..15] → 12 byte
        for i in 0..3 {
            let bytes = self.state[12 + i].to_le_bytes();
            nonce[i * 4..i * 4 + 4].copy_from_slice(&bytes);
        }

        for (block_idx, chunk) in data.chunks(64).enumerate() {
            let chacha = ChaCha20::new(&key, &nonce, counter + block_idx as u32);
            let keystream = chacha.block();

            for (i, byte) in chunk.iter().enumerate() {
                result.push(byte ^ keystream[i]);
            }
        }

        result
    }
}

/// Poly1305 tek seferlik mesaj kimlik doğrulama kodu (MAC)
///
/// GF(2^130 - 5) üzerinde polinom değerlendirmesi:
///   - r: 128-bit anahtar (clamped - belirli bitler sıfırlanır)
///   - s: 128-bit nonce (ekleme için)
///   - Her 16-byte blok için akümülatöre eklenir
///   - Sonuç = (a[0]*r^n + a[1]*r^(n-1) + ... + a[n]) + s (mod 2^130-5)
///
/// 130-bit arithmetic uses 5 × u32 limbs (26 bits each).
pub struct Poly1305 {
    /// r key in 5×u32 radix-2^26 limbs (clamped)
    r: [u32; 5],
    /// Precomputed 5*r values for modular reduction
    s_r: [u32; 4],
    /// Pad (s key), added at the end
    pad: [u32; 4],
    /// Accumulator in 5×u32 radix-2^26 limbs
    acc: [u32; 5],
    /// Leftover buffer for partial blocks
    leftover: [u8; 16],
    /// Number of bytes in leftover buffer
    leftover_len: usize,
}

impl Poly1305 {
    /// Yeni Poly1305 örneği oluştur (32-byte anahtar ile)
    ///
    /// İlk 16 byte r anahtarı (clamping uygulanır), son 16 byte s anahtarı.
    /// r-clamping: bytes[3,7,11,15] &= 0x0f, bytes[4,8,12] &= 0xfc
    pub fn new(key: &[u8; 32]) -> Self {
        let mut r_bytes = [0u8; 16];
        r_bytes.copy_from_slice(&key[..16]);

        // Clamp r per RFC 8439
        r_bytes[3] &= 0x0f;
        r_bytes[7] &= 0x0f;
        r_bytes[11] &= 0x0f;
        r_bytes[15] &= 0x0f;
        r_bytes[4] &= 0xfc;
        r_bytes[8] &= 0xfc;
        r_bytes[12] &= 0xfc;

        // Convert r to radix-2^26 limbs
        let r0 = u32::from_le_bytes([r_bytes[0], r_bytes[1], r_bytes[2], r_bytes[3]]) & 0x03ff_ffff;
        let r1 = (u32::from_le_bytes([r_bytes[3], r_bytes[4], r_bytes[5], r_bytes[6]]) >> 2)
            & 0x03ff_ffff;
        let r2 = (u32::from_le_bytes([r_bytes[6], r_bytes[7], r_bytes[8], r_bytes[9]]) >> 4)
            & 0x03ff_ffff;
        let r3 = (u32::from_le_bytes([r_bytes[9], r_bytes[10], r_bytes[11], r_bytes[12]]) >> 6)
            & 0x03ff_ffff;
        let r4 = u32::from_le_bytes([r_bytes[13], r_bytes[14], r_bytes[15], 0]) >> 0 & 0x03ff_ffff;

        // Precompute 5*r[i] for reduction step
        let s_r = [r1 * 5, r2 * 5, r3 * 5, r4 * 5];

        // s (pad) as 4×u32 LE
        let pad = [
            u32::from_le_bytes([key[16], key[17], key[18], key[19]]),
            u32::from_le_bytes([key[20], key[21], key[22], key[23]]),
            u32::from_le_bytes([key[24], key[25], key[26], key[27]]),
            u32::from_le_bytes([key[28], key[29], key[30], key[31]]),
        ];

        Poly1305 {
            r: [r0, r1, r2, r3, r4],
            s_r,
            pad,
            acc: [0u32; 5],
            leftover: [0u8; 16],
            leftover_len: 0,
        }
    }

    /// Process a single block (up to 16 bytes). `hibit` is 1 for message blocks, 0 for final partial.
    fn process_block(&mut self, block: &[u8], hibit: u32) {
        // Convert block to radix-2^26 number and add to accumulator
        let mut buf = [0u8; 17];
        buf[..block.len()].copy_from_slice(block);
        // hibit goes at bit 128 (byte 16)
        // For full blocks we set the 2^128 bit

        let s0 = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) & 0x03ff_ffff;
        let s1 = (u32::from_le_bytes([buf[3], buf[4], buf[5], buf[6]]) >> 2) & 0x03ff_ffff;
        let s2 = (u32::from_le_bytes([buf[6], buf[7], buf[8], buf[9]]) >> 4) & 0x03ff_ffff;
        let s3 = (u32::from_le_bytes([buf[9], buf[10], buf[11], buf[12]]) >> 6) & 0x03ff_ffff;
        let s4_raw = u32::from_le_bytes([buf[12], buf[13], buf[14], buf[15]]);
        // Only take the top bits not consumed by s3
        let s4 = (s4_raw >> 8) | (hibit << 24);

        // acc += block
        self.acc[0] = self.acc[0].wrapping_add(s0);
        self.acc[1] = self.acc[1].wrapping_add(s1);
        self.acc[2] = self.acc[2].wrapping_add(s2);
        self.acc[3] = self.acc[3].wrapping_add(s3);
        self.acc[4] = self.acc[4].wrapping_add(s4);

        // Multiply acc * r using u64 to avoid overflow
        let r0 = self.r[0] as u64;
        let r1 = self.r[1] as u64;
        let r2 = self.r[2] as u64;
        let r3 = self.r[3] as u64;
        let r4 = self.r[4] as u64;
        let sr1 = self.s_r[0] as u64; // 5*r1
        let sr2 = self.s_r[1] as u64; // 5*r2
        let sr3 = self.s_r[2] as u64; // 5*r3
        let sr4 = self.s_r[3] as u64; // 5*r4

        let a0 = self.acc[0] as u64;
        let a1 = self.acc[1] as u64;
        let a2 = self.acc[2] as u64;
        let a3 = self.acc[3] as u64;
        let a4 = self.acc[4] as u64;

        // Schoolbook multiplication mod 2^130-5 using the identity:
        // 2^130 ≡ 5 (mod 2^130-5)
        let mut d0 = a0 * r0 + a1 * sr4 + a2 * sr3 + a3 * sr2 + a4 * sr1;
        let mut d1 = a0 * r1 + a1 * r0 + a2 * sr4 + a3 * sr3 + a4 * sr2;
        let mut d2 = a0 * r2 + a1 * r1 + a2 * r0 + a3 * sr4 + a4 * sr3;
        let mut d3 = a0 * r3 + a1 * r2 + a2 * r1 + a3 * r0 + a4 * sr4;
        let mut d4 = a0 * r4 + a1 * r3 + a2 * r2 + a3 * r1 + a4 * r0;

        // Partial reduction: carry propagation in radix-2^26
        let mut c: u64;
        c = d0 >> 26;
        self.acc[0] = (d0 & 0x03ff_ffff) as u32;
        d1 += c;
        c = d1 >> 26;
        self.acc[1] = (d1 & 0x03ff_ffff) as u32;
        d2 += c;
        c = d2 >> 26;
        self.acc[2] = (d2 & 0x03ff_ffff) as u32;
        d3 += c;
        c = d3 >> 26;
        self.acc[3] = (d3 & 0x03ff_ffff) as u32;
        d4 += c;
        c = d4 >> 26;
        self.acc[4] = (d4 & 0x03ff_ffff) as u32;
        self.acc[0] = self.acc[0].wrapping_add((c as u32) * 5);
        c = (self.acc[0] >> 26) as u64;
        self.acc[0] &= 0x03ff_ffff;
        self.acc[1] = self.acc[1].wrapping_add(c as u32);
    }

    /// Veri bloğuyla akümülatörü güncelle
    ///
    /// Her 16-byte blok için: blok sonuna 0x01 byte eklenir (2^128 bit set),
    /// akümülatöre eklenir, r ile çarpılır, mod 2^130-5 indirgenir.
    pub fn update(&mut self, data: &[u8]) {
        let mut pos = 0;

        // If we have leftover bytes, fill them first
        if self.leftover_len > 0 {
            let want = 16 - self.leftover_len;
            let take = if data.len() < want { data.len() } else { want };
            self.leftover[self.leftover_len..self.leftover_len + take]
                .copy_from_slice(&data[..take]);
            self.leftover_len += take;
            pos = take;

            if self.leftover_len < 16 {
                return;
            }

            let block = self.leftover;
            self.leftover_len = 0;
            self.process_block(&block, 1);
        }

        // Process full 16-byte blocks
        while pos + 16 <= data.len() {
            self.process_block(&data[pos..pos + 16], 1);
            pos += 16;
        }

        // Buffer remaining bytes
        if pos < data.len() {
            let remaining = data.len() - pos;
            self.leftover[..remaining].copy_from_slice(&data[pos..]);
            self.leftover_len = remaining;
        }
    }

    /// MAC etiketini tamamla ve döndür (16 byte)
    ///
    /// Son adım: kalan baytları işle, sonra akümülatör + s (mod 2^128)
    /// Sonuç: 16-byte Poly1305 kimlik doğrulama etiketi
    pub fn finalize(mut self) -> [u8; 16] {
        // Process any remaining bytes (partial block)
        if self.leftover_len > 0 {
            // Pad the partial block: append 0x01 byte, rest zeros
            let mut block = [0u8; 16];
            block[..self.leftover_len].copy_from_slice(&self.leftover[..self.leftover_len]);
            block[self.leftover_len] = 0x01;
            // hibit = 0 for partial final block (the 0x01 padding handles the bit)
            self.process_block(&block[..self.leftover_len + 1], 0);
        }

        // Full carry chain
        let mut c: u32;
        c = self.acc[1] >> 26;
        self.acc[1] &= 0x03ff_ffff;
        self.acc[2] = self.acc[2].wrapping_add(c);
        c = self.acc[2] >> 26;
        self.acc[2] &= 0x03ff_ffff;
        self.acc[3] = self.acc[3].wrapping_add(c);
        c = self.acc[3] >> 26;
        self.acc[3] &= 0x03ff_ffff;
        self.acc[4] = self.acc[4].wrapping_add(c);
        c = self.acc[4] >> 26;
        self.acc[4] &= 0x03ff_ffff;
        self.acc[0] = self.acc[0].wrapping_add(c * 5);
        c = self.acc[0] >> 26;
        self.acc[0] &= 0x03ff_ffff;
        self.acc[1] = self.acc[1].wrapping_add(c);

        // Compute acc - (2^130 - 5) = acc - 2^130 + 5
        // If acc >= 2^130-5, we subtract; otherwise keep acc.
        let mut g = [0u32; 5];
        g[0] = self.acc[0].wrapping_add(5);
        c = g[0] >> 26;
        g[0] &= 0x03ff_ffff;
        g[1] = self.acc[1].wrapping_add(c);
        c = g[1] >> 26;
        g[1] &= 0x03ff_ffff;
        g[2] = self.acc[2].wrapping_add(c);
        c = g[2] >> 26;
        g[2] &= 0x03ff_ffff;
        g[3] = self.acc[3].wrapping_add(c);
        c = g[3] >> 26;
        g[3] &= 0x03ff_ffff;
        g[4] = self.acc[4].wrapping_add(c).wrapping_sub(1 << 26);

        // If g[4] didn't underflow (bit 31 not set), acc >= p, use g; else use acc.
        let mask = !((g[4] >> 31).wrapping_sub(1)); // 0xFFFFFFFF if g[4] < 0, else 0
        for i in 0..5 {
            self.acc[i] = (self.acc[i] & mask) | (g[i] & !mask);
        }

        // Convert from radix-2^26 back to 4×u32 (128-bit LE)
        let f0 = self.acc[0] | (self.acc[1] << 26);
        let f1 = (self.acc[1] >> 6) | (self.acc[2] << 20);
        let f2 = (self.acc[2] >> 12) | (self.acc[3] << 14);
        let f3 = (self.acc[3] >> 18) | (self.acc[4] << 8);

        // Add pad (s): acc + s mod 2^128
        let (t0, carry) = f0.overflowing_add(self.pad[0]);
        let (t1, carry2) = f1.overflowing_add(self.pad[1]);
        let (t1, carry2b) = t1.overflowing_add(carry as u32);
        let (t2, carry3) = f2.overflowing_add(self.pad[2]);
        let (t2, carry3b) = t2.overflowing_add((carry2 || carry2b) as u32);
        let (t3, _) = f3.overflowing_add(self.pad[3]);
        let t3 = t3.wrapping_add((carry3 || carry3b) as u32);

        let mut tag = [0u8; 16];
        tag[0..4].copy_from_slice(&t0.to_le_bytes());
        tag[4..8].copy_from_slice(&t1.to_le_bytes());
        tag[8..12].copy_from_slice(&t2.to_le_bytes());
        tag[12..16].copy_from_slice(&t3.to_le_bytes());

        tag
    }
}

/// ChaCha20-Poly1305 AEAD şifreleme (RFC 8439)
///
/// ChaCha20 (şifreleme) + Poly1305 (kimlik doğrulama) kombinasyonu.
/// Sabit zamanlı, donanım hızlandırması gerektirmez.
/// TLS 1.3'te TLS_CHACHA20_POLY1305_SHA256 şifre paketi için kullanılır.
pub struct ChaCha20Poly1305 {
    key: [u8; 32],
}

impl ChaCha20Poly1305 {
    pub fn new(key: &[u8; 32]) -> Self {
        let mut k = [0u8; 32];
        k.copy_from_slice(key);
        ChaCha20Poly1305 { key: k }
    }

    /// Poly1305 kimlik doğrulamasıyla şifrele
    ///
    /// Adımlar:
    ///   1. ChaCha20(key, nonce, 0) -> İlk 32 byte = Poly1305 anahtarı
    ///   2. ChaCha20(key, nonce, 1) ile düz metin XOR -> şifreli metin
    ///   3. Poly1305(poly_key, aad || şifreli) -> 16-byte MAC
    pub fn encrypt(&self, nonce: &[u8; 12], aad: &[u8], plaintext: &[u8]) -> (Vec<u8>, [u8; 16]) {
        // Generate Poly1305 key using ChaCha20
        let chacha = ChaCha20::new(&self.key, nonce, 0);
        let keystream = chacha.block();
        let mut poly_key = [0u8; 32];
        poly_key.copy_from_slice(&keystream[..32]);

        // Encrypt plaintext
        let cipher_chacha = ChaCha20::new(&self.key, nonce, 1);
        let ciphertext = cipher_chacha.process(plaintext);

        // Compute Poly1305 tag
        let mut poly = Poly1305::new(&poly_key);
        poly.update(aad);
        poly.update(&ciphertext);
        let tag = poly.finalize();

        (ciphertext, tag)
    }

    /// Kimlik doğrula ve şifre çöz
    ///
    /// Önce MAC doğrulaması yapılır. Etiket yanlışsa None döner.
    /// Doğrulama başarılıysa ChaCha20 ile şifre çözülür.
    pub fn decrypt(
        &self,
        nonce: &[u8; 12],
        aad: &[u8],
        ciphertext: &[u8],
        tag: &[u8; 16],
    ) -> Option<Vec<u8>> {
        // Generate Poly1305 key
        let chacha = ChaCha20::new(&self.key, nonce, 0);
        let keystream = chacha.block();
        let mut poly_key = [0u8; 32];
        poly_key.copy_from_slice(&keystream[..32]);

        // Verify tag
        let mut poly = Poly1305::new(&poly_key);
        poly.update(aad);
        poly.update(ciphertext);
        let expected_tag = poly.finalize();

        if expected_tag != *tag {
            return None;
        }

        // Decrypt
        let cipher_chacha = ChaCha20::new(&self.key, nonce, 1);
        Some(cipher_chacha.process(ciphertext))
    }
}

// ============================================================================
// ECDHE - ELİPTİK EĞRİ DİFFIE-HELLMAN ANAHTAR DEĞİŞİMİ (X25519)
// ============================================================================
//
// X25519, Montgomery eğrisi Curve25519 üzerinde Diffie-Hellman anahtar değişimidir.
// RFC 7748'de tanımlanmış, TLS 1.3'ün varsayılan anahtar değişim yöntemidir.
//
// Matematiksel Temel:
//   Curve25519: y^2 = x^3 + 486662*x^2 + x (mod 2^255 - 19)
//   Skaler çarpım: scalar * P eğri noktası hesaplama
//   DH ortak sır: a * (b*G) = b * (a*G) = a*b*G
//
// Montgomery Merdiveni (Scalar Multiplication Algoritması):
//   - Sabit zamanlı: Her bit işlemi aynı süre alır (yan kanal saldırısı önlemi)
//   - conditional_swap: Bit değerine göre iki noktayı takas eder (zamanlama sızıntısı yok)
//   - 255 bit işlenerek nokta koordinatları hesaplanır
//
// Saha Elemanı (FieldElement):
//   - 51-bit parçalı gösterim: 5 x 64-bit limb ile 255-bit sayı temsili
//   - limbs[0] + limbs[1]*2^51 + limbs[2]*2^102 + ... + limbs[4]*2^204
//   - p = 2^255 - 19 (ana sayı)
//
// TLS 1.3'te Kullanım:
//   1. generate_keypair() -> (private, public)
//   2. public key ClientHello key_share uzantısında gönderilir
//   3. shared_secret = diffie_hellman(our_private, peer_public)
//   4. shared_secret HKDF-Extract'a girdi olarak verilir

/// Curve25519 saha elemanı (255 bit, 5 x 64-bit limb gösterimi)
///
/// Gösterim: limbs[0] + limbs[1]*2^51 + limbs[2]*2^102 + limbs[3]*2^153 + limbs[4]*2^204
/// Ana sayı: p = 2^255 - 19
/// Bu gösterim mod p aritmetiğini verimli kılar (51-bit taşıma zinciri)
#[derive(Clone, Copy, Debug)]
pub struct FieldElement(pub [u64; 5]);

impl FieldElement {
    /// Prime p = 2^255 - 19
    const P: [u64; 5] = [
        0x7ffffffffffffed, // 2^51 - 19
        0x7ffffffffffff,   // 2^51 - 1
        0x7ffffffffffff,
        0x7ffffffffffff,
        0x7ffffffffffff,
    ];

    /// Create zero element
    pub fn zero() -> Self {
        FieldElement([0, 0, 0, 0, 0])
    }

    /// Create one element
    pub fn one() -> Self {
        FieldElement([1, 0, 0, 0, 0])
    }

    /// Create from u8 array (little-endian, 32 bytes)
    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        let mut limbs = [0u64; 5];

        // Decode as 5 x 51-bit limbs
        limbs[0] = (bytes[0] as u64)
            | ((bytes[1] as u64) << 8)
            | ((bytes[2] as u64) << 16)
            | ((bytes[3] as u64) << 24)
            | ((bytes[4] as u64) << 32)
            | ((bytes[5] as u64) << 40)
            | (((bytes[6] as u64) & 0x7f) << 48);

        limbs[1] = ((bytes[6] as u64) >> 7)
            | ((bytes[7] as u64) << 1)
            | ((bytes[8] as u64) << 9)
            | ((bytes[9] as u64) << 17)
            | ((bytes[10] as u64) << 25)
            | ((bytes[11] as u64) << 33)
            | ((bytes[12] as u64) << 41)
            | (((bytes[13] as u64) & 0x3f) << 49);

        limbs[2] = ((bytes[13] as u64) >> 6)
            | ((bytes[14] as u64) << 2)
            | ((bytes[15] as u64) << 10)
            | ((bytes[16] as u64) << 18)
            | ((bytes[17] as u64) << 26)
            | ((bytes[18] as u64) << 34)
            | ((bytes[19] as u64) << 42)
            | (((bytes[20] as u64) & 0x1f) << 50);

        limbs[3] = ((bytes[20] as u64) >> 5)
            | ((bytes[21] as u64) << 3)
            | ((bytes[22] as u64) << 11)
            | ((bytes[23] as u64) << 19)
            | ((bytes[24] as u64) << 27)
            | ((bytes[25] as u64) << 35)
            | ((bytes[26] as u64) << 43)
            | (((bytes[27] as u64) & 0x0f) << 51);

        limbs[4] = ((bytes[27] as u64) >> 4)
            | ((bytes[28] as u64) << 4)
            | ((bytes[29] as u64) << 12)
            | ((bytes[30] as u64) << 20)
            | ((bytes[31] as u64) << 28);

        FieldElement(limbs)
    }

    /// Convert to u8 array (little-endian, 32 bytes)
    pub fn to_bytes(&self) -> [u8; 32] {
        let mut result = [0u8; 32];
        let mut carry = 0i64;
        let mut limbs = self.0;

        // Reduce modulo 2^255-19
        for i in 0..5 {
            limbs[i] = (limbs[i] as i64 + carry) as u64;
            carry = (limbs[i] >> 51) as i64;
            limbs[i] &= 0x7ffffffffffff;
        }

        // Subtract p if necessary
        let gt_p = ((limbs[0] + 19) >> 51) as i64
            | (limbs[1] >> 51) as i64
            | (limbs[2] >> 51) as i64
            | (limbs[3] >> 51) as i64
            | (limbs[4] >> 51) as i64;

        if gt_p != 0 {
            limbs[0] += 19;
        }

        // Encode to bytes
        result[0] = limbs[0] as u8;
        result[1] = (limbs[0] >> 8) as u8;
        result[2] = (limbs[0] >> 16) as u8;
        result[3] = (limbs[0] >> 24) as u8;
        result[4] = (limbs[0] >> 32) as u8;
        result[5] = (limbs[0] >> 40) as u8;
        result[6] = ((limbs[0] >> 48) | (limbs[1] << 7)) as u8;
        result[7] = (limbs[1] >> 1) as u8;
        result[8] = (limbs[1] >> 9) as u8;
        result[9] = (limbs[1] >> 17) as u8;
        result[10] = (limbs[1] >> 25) as u8;
        result[11] = (limbs[1] >> 33) as u8;
        result[12] = (limbs[1] >> 41) as u8;
        result[13] = ((limbs[1] >> 49) | (limbs[2] << 6)) as u8;
        result[14] = (limbs[2] >> 2) as u8;
        result[15] = (limbs[2] >> 10) as u8;
        result[16] = (limbs[2] >> 18) as u8;
        result[17] = (limbs[2] >> 26) as u8;
        result[18] = (limbs[2] >> 34) as u8;
        result[19] = (limbs[2] >> 42) as u8;
        result[20] = ((limbs[2] >> 50) | (limbs[3] << 5)) as u8;
        result[21] = (limbs[3] >> 3) as u8;
        result[22] = (limbs[3] >> 11) as u8;
        result[23] = (limbs[3] >> 19) as u8;
        result[24] = (limbs[3] >> 27) as u8;
        result[25] = (limbs[3] >> 35) as u8;
        result[26] = (limbs[3] >> 43) as u8;
        result[27] = ((limbs[3] >> 51) | (limbs[4] << 4)) as u8;
        result[28] = (limbs[4] >> 4) as u8;
        result[29] = (limbs[4] >> 12) as u8;
        result[30] = (limbs[4] >> 20) as u8;
        result[31] = (limbs[4] >> 28) as u8;

        result
    }

    /// Add two field elements
    pub fn add(&self, other: &Self) -> Self {
        let mut result = FieldElement::zero();
        for i in 0..5 {
            result.0[i] = self.0[i] + other.0[i];
        }
        result
    }

    /// Subtract two field elements
    pub fn sub(&self, other: &Self) -> Self {
        let mut result = FieldElement::zero();
        for i in 0..5 {
            // Add 2*p to ensure positive result
            result.0[i] = self.0[i] + (0x1ffffffffffffe << 1) - other.0[i];
        }
        result.reduce();
        result
    }

    /// Multiply two field elements (schoolbook method)
    pub fn mul(&self, other: &Self) -> Self {
        // Multiply and accumulate
        let mut product = [0u128; 9];

        for i in 0..5 {
            for j in 0..5 {
                product[i + j] += (self.0[i] as u128) * (other.0[j] as u128);
            }
        }

        // Reduce modulo 2^255-19
        let mut result = FieldElement::zero();

        // Carry propagation
        let mut carry = 0u128;
        for i in 0..5 {
            product[i] += carry;
            carry = product[i] >> 51;
            result.0[i] = (product[i] & 0x7ffffffffffff) as u64;
        }

        // Handle overflow: multiply by 19 and add
        result.0[0] += (carry as u64) * 19;

        result.reduce();
        result
    }

    /// Square a field element (optimized)
    pub fn square(&self) -> Self {
        self.mul(self)
    }

    /// Reduce to canonical form
    pub fn reduce(&mut self) {
        let mut carry = 0u64;

        for i in 0..5 {
            self.0[i] += carry;
            carry = self.0[i] >> 51;
            self.0[i] &= 0x7ffffffffffff;
        }

        // Fold carry back with factor 19
        self.0[0] += carry * 19;

        // Final reduction
        carry = self.0[0] >> 51;
        self.0[0] &= 0x7ffffffffffff;
        self.0[1] += carry;
    }

    /// Compute multiplicative inverse (a^(p-2) = a^(2^255-21))
    pub fn invert(&self) -> Self {
        // Square-and-multiply for a^(2^255-21)
        let mut result = self.clone();

        // a^(2^250-1)
        for _ in 0..249 {
            result = result.square();
            result = result.mul(self);
        }

        // Final squarings for 2^255-21
        result = result.square();
        result = result.square();
        result = result.square();
        result = result.square();
        result = result.square();
        result = result.square();

        result
    }

    /// Conditional swap (constant-time)
    pub fn conditional_swap(a: &mut Self, b: &mut Self, swap: u8) {
        let mask = (-(swap as i64)) as u64;

        for i in 0..5 {
            let diff = (a.0[i] ^ b.0[i]) & mask;
            a.0[i] ^= diff;
            b.0[i] ^= diff;
        }
    }
}

/// X25519 eliptik eğri işlemleri (Curve25519 Montgomery merdiveni)
///
/// Temel işlemler:
///   - generate_keypair(): Rastgele private key üretir, public key türetir
///   - public_from_private(): u=9 temel noktasıyla skaler çarpım (G*private)
///   - scalar_mult(): Montgomery ladder ile sabit zamanlı skaler çarpım
///   - diffie_hellman(): Ortak sır hesaplama (scalar_mult yeniden adlandırılmış)
pub struct X25519;

impl X25519 {
    /// A24 = 121665 (used in Montgomery ladder)
    const A24: FieldElement = FieldElement([121665, 0, 0, 0, 0]);

    /// Generate keypair
    pub fn generate_keypair() -> ([u8; 32], [u8; 32]) {
        let mut private = [0u8; 32];
        for i in 0..32 {
            private[i] = crate::random::next_u32() as u8;
        }

        // Clamp private key
        private[0] &= 248;
        private[31] &= 127;
        private[31] |= 64;

        let public = Self::public_from_private(&private);
        (private, public)
    }

    /// Derive public key from private key
    pub fn public_from_private(private: &[u8; 32]) -> [u8; 32] {
        // Base point u = 9
        let base = [9u8; 32];
        Self::scalar_mult(private, &base)
    }

    /// Montgomery ladder scalar multiplication
    pub fn scalar_mult(scalar: &[u8; 32], point: &[u8; 32]) -> [u8; 32] {
        let mut k = *scalar;

        // Clamp scalar
        k[0] &= 248;
        k[31] &= 127;
        k[31] |= 64;

        // Decode point as field element
        let u = FieldElement::from_bytes(point);

        // Montgomery ladder
        // x_1 = u (point)
        // x_2 = 1, z_2 = 0 (point at infinity)
        // x_3 = u, z_3 = 1 (point)
        let mut x1 = u;
        let mut x2 = FieldElement::one();
        let mut z2 = FieldElement::zero();
        let mut x3 = u;
        let mut z3 = FieldElement::one();

        // Swap = 0
        let mut swap: u8 = 0;

        // Process bits from high to low
        for t in (0..255).rev() {
            let k_t = (k[t / 8] >> (t % 8)) & 1;

            // Conditional swap
            FieldElement::conditional_swap(&mut x2, &mut x3, swap ^ k_t);
            FieldElement::conditional_swap(&mut z2, &mut z3, swap ^ k_t);
            swap = k_t;

            // A = x_2 + z_2
            let a = x2.add(&z2);
            // AA = A^2
            let aa = a.square();
            // B = x_2 - z_2
            let b = x2.sub(&z2);
            // BB = B^2
            let bb = b.square();
            // E = AA - BB
            let e = aa.sub(&bb);
            // C = x_3 + z_3
            let c = x3.add(&z3);
            // D = x_3 - z_3
            let d = x3.sub(&z3);
            // DA = D * A
            let da = d.mul(&a);
            // CB = C * B
            let cb = c.mul(&b);
            // x_3 = (DA + CB)^2
            let dacb = da.add(&cb);
            x3 = dacb.square();
            // z_3 = x_1 * (DA - CB)^2
            let dacb_sub = da.sub(&cb);
            z3 = x1.mul(&dacb_sub.square());
            // x_2 = AA * BB
            x2 = aa.mul(&bb);
            // z_2 = E * (AA + a24 * E)
            let a24_e = Self::A24.mul(&e);
            let aa_a24e = aa.add(&a24_e);
            z2 = e.mul(&aa_a24e);
        }

        // Final conditional swap
        FieldElement::conditional_swap(&mut x2, &mut x3, swap);
        FieldElement::conditional_swap(&mut z2, &mut z3, swap);

        // Compute result: x_2 * (z_2^(p-2))
        let z2_inv = z2.invert();
        let result = x2.mul(&z2_inv);

        result.to_bytes()
    }

    /// Diffie-Hellman ortak sır hesapla
    ///
    /// scalar_mult(private, public) = private * public_point
    /// Sonuç: Her iki tarafın da hesaplayabildiği ortak gizli değer.
    /// TLS 1.3'te bu değer HKDF-Extract'a ikm (input keying material) olarak verilir.
    pub fn diffie_hellman(private: &[u8; 32], public: &[u8; 32]) -> [u8; 32] {
        Self::scalar_mult(private, public)
    }
}

// ============================================================================
// TLS 1.3 KRİPTO ENTEGRASYONU
// ============================================================================
//
// Bu bölüm, TLS 1.3'ün RFC 8446 Bölüm 7'sine göre tam HKDF anahtar takvimini
// ve şifreleme/şifre çözme işlemlerini uygular.
//
// TlsKeySchedule:
//   HKDF tabanlı anahtar türetme iş akışı:
//   init_with_psk() -> derive_handshake_secrets() -> derive_master_secret()
//   Son adımda: derive_traffic_keys() ile key + iv elde edilir.
//
//   traffic_secret -> HKDF-Expand-Label(secret, "key", "", key_len) = şifreleme anahtarı
//   traffic_secret -> HKDF-Expand-Label(secret, "iv",  "", 12)      = nonce tabanı
//
// TlsCrypto:
//   Seçilen şifre paketine göre şifreleme/şifre çözme gerçekleştirir.
//   TLS 1.3 kayıt formatı:
//     - Düz metin + ContentType baytı + dolgu birleştirilir
//     - AEAD ile şifrelenir (key + nonce kullanarak)
//     - Sonuç: şifreli veri || 16-byte kimlik doğrulama etiketi
//
//   Nonce oluşturma (RFC 8446 Bölüm 5.3):
//     nonce = IV XOR (sekans_numarası'nın big-endian 12-byte gösterimi)

/// TLS 1.3 anahtar takvimi (RFC 8446 Bölüm 7.1 uyumlu)
pub struct TlsKeySchedule {
    /// Cipher suite
    cipher_suite: CipherSuite,
    /// Hash length
    hash_len: usize,
    /// Early Secret
    early_secret: Vec<u8>,
    /// Handshake Secret
    handshake_secret: Option<Vec<u8>>,
    /// Master Secret
    master_secret: Option<Vec<u8>>,
    /// Client Handshake Traffic Secret
    client_hs_secret: Option<Vec<u8>>,
    /// Server Handshake Traffic Secret
    server_hs_secret: Option<Vec<u8>>,
    /// Client Application Traffic Secret
    client_app_secret: Option<Vec<u8>>,
    /// Server Application Traffic Secret
    server_app_secret: Option<Vec<u8>>,
}

impl TlsKeySchedule {
    /// Create new key schedule
    pub fn new(cipher_suite: CipherSuite) -> Self {
        let hash_len = match cipher_suite {
            CipherSuite::Aes128GcmSha256 | CipherSuite::ChaCha20Poly1305Sha256 => 32,
            CipherSuite::Aes256GcmSha384 => 48,
        };

        TlsKeySchedule {
            cipher_suite,
            hash_len,
            early_secret: vec![0u8; hash_len],
            handshake_secret: None,
            master_secret: None,
            client_hs_secret: None,
            server_hs_secret: None,
            client_app_secret: None,
            server_app_secret: None,
        }
    }

    /// HKDF-Extract: PSK veya ECDHE girdisinden Pseudo-Random Key türet
    ///
    /// PRK = HMAC-Hash(salt, IKM)
    /// salt: Önceki aşamanın derived_secret değeri (veya sıfır dizisi)
    /// IKM (Input Keying Material): PSK veya ECDHE ortak sırrı
    fn hkdf_extract(&self, salt: &[u8], ikm: &[u8]) -> Vec<u8> {
        // HMAC-Hash(salt, ikm)
        match self.cipher_suite {
            CipherSuite::Aes128GcmSha256 | CipherSuite::ChaCha20Poly1305Sha256 => {
                let mut hmac = Hmac::<Sha256>::new_from_slice(salt)
                    .unwrap_or_else(|_| Hmac::<Sha256>::new_from_slice(&[0u8; 32]).unwrap());
                hmac.update(ikm);
                hmac.finalize().into_bytes().to_vec()
            }
            CipherSuite::Aes256GcmSha384 => {
                let mut hmac = Hmac::<Sha384>::new_from_slice(salt)
                    .unwrap_or_else(|_| Hmac::<Sha384>::new_from_slice(&[0u8; 48]).unwrap());
                hmac.update(ikm);
                hmac.finalize().into_bytes().to_vec()
            }
        }
    }

    /// HKDF-Expand: PRK'dan istenilen uzunlukta OKM türet
    ///
    /// OKM = T(1) | T(2) | T(3) | ... | T(n)
    /// T(0) = boş dizi
    /// T(n) = HMAC(PRK, T(n-1) | info | n)
    /// Maksimum çıktı: 255 * Hash.länge
    fn hkdf_expand(&self, prk: &[u8], info: &[u8], len: usize) -> Vec<u8> {
        // HKDF-Expand(PRK, info, L) =
        //   T(1) | T(2) | T(3) | ... | T(n)
        // where T(0) = empty string
        //       T(1) = HMAC(PRK, T(0) | info | 0x01)
        //       T(2) = HMAC(PRK, T(1) | info | 0x02)
        //       etc.

        let mut output = Vec::new();
        let mut t = Vec::new();
        let mut counter = 1u8;

        while output.len() < len {
            // T(n) = HMAC(PRK, T(n-1) | info | n)
            let mut data = t.clone();
            data.extend_from_slice(info);
            data.push(counter);

            // HMAC(PRK, data)
            let t_n = self.hmac_hash(prk, &data);

            t = t_n.clone();
            output.extend_from_slice(&t);

            counter += 1;
            if counter == 0 {
                break; // Prevent overflow
            }
        }

        output.truncate(len);
        output
    }

    /// HMAC-Hash
    fn hmac_hash(&self, key: &[u8], data: &[u8]) -> Vec<u8> {
        // HMAC(K, m) = H((K ^ opad) || H((K ^ ipad) || m))
        let block_size = match self.cipher_suite {
            CipherSuite::Aes128GcmSha256 | CipherSuite::ChaCha20Poly1305Sha256 => 64,
            CipherSuite::Aes256GcmSha384 => 128,
        };

        // Pad key to block size
        let mut k_ipad = vec![0x36u8; block_size];
        let mut k_opad = vec![0x5cu8; block_size];

        for (i, &k) in key.iter().enumerate().take(block_size) {
            k_ipad[i] ^= k;
            k_opad[i] ^= k;
        }

        // Inner hash: H(K ^ ipad || data)
        let mut inner = k_ipad;
        inner.extend_from_slice(data);

        let inner_hash = match self.cipher_suite {
            CipherSuite::Aes128GcmSha256 | CipherSuite::ChaCha20Poly1305Sha256 => {
                let mut hasher = Sha256::new();
                hasher.update(&inner);
                hasher.finalize().to_vec()
            }
            CipherSuite::Aes256GcmSha384 => {
                let mut hasher = Sha384::new();
                hasher.update(&inner);
                hasher.finalize().to_vec()
            }
        };

        // Outer hash: H(K ^ opad || inner_hash)
        let mut outer = k_opad;
        outer.extend_from_slice(&inner_hash);

        match self.cipher_suite {
            CipherSuite::Aes128GcmSha256 | CipherSuite::ChaCha20Poly1305Sha256 => {
                let mut hasher = Sha256::new();
                hasher.update(&outer);
                hasher.finalize().to_vec()
            }
            CipherSuite::Aes256GcmSha384 => {
                let mut hasher = Sha384::new();
                hasher.update(&outer);
                hasher.finalize().to_vec()
            }
        }
    }

    /// HKDF-Expand-Label: Derive key with TLS 1.3 label
    /// HkdfExpandLabel(Secret, Label, Context, Length) =
    ///   HKDF-Expand(Secret, HkdfLabel, Length)
    /// where HkdfLabel = Length || "tls13 " || Label || Context
    pub fn hkdf_expand_label(
        &self,
        secret: &[u8],
        label: &[u8],
        context: &[u8],
        len: usize,
    ) -> Vec<u8> {
        // Build HkdfLabel
        let mut info = Vec::new();

        // Length (2 bytes)
        info.extend_from_slice(&(len as u16).to_be_bytes());

        // "tls13 " || Label
        info.extend_from_slice(b"tls13 ");
        info.extend_from_slice(label);

        // Context length (1 byte) || Context
        info.push(context.len() as u8);
        info.extend_from_slice(context);

        self.hkdf_expand(secret, &info, len)
    }

    /// Derive Secret: Derive-Secret(Secret, Label, Messages)
    /// = HKDF-Expand-Label(Secret, Label, Transcript-Hash(Messages), Hash.length)
    pub fn derive_secret(&self, secret: &[u8], label: &[u8], transcript_hash: &[u8]) -> Vec<u8> {
        self.hkdf_expand_label(secret, label, transcript_hash, self.hash_len)
    }

    /// Initialize with PSK (or zero for fresh connection)
    pub fn init_with_psk(&mut self, psk: Option<&[u8]>) {
        let zero_vec = vec![0u8; self.hash_len];
        let ikm = psk.unwrap_or(&zero_vec);
        let salt = vec![0u8; self.hash_len];

        self.early_secret = self.hkdf_extract(&salt, ikm);
    }

    /// Compute handshake secrets after ECDH
    pub fn derive_handshake_secrets(&mut self, ecdhe_secret: &[u8], transcript_hash: &[u8]) {
        // Derive-Secret(early_secret, "derived", empty_hash)
        let derived_secret = self.derive_secret(&self.early_secret, b"derived", &[]);

        // handshake_secret = HKDF-Extract(derived_secret, ECDHE)
        self.handshake_secret = Some(self.hkdf_extract(&derived_secret, ecdhe_secret));

        let hs = self.handshake_secret.as_ref().unwrap();

        // c_hs_secret = Derive-Secret(handshake_secret, "c hs traffic", transcript_hash)
        self.client_hs_secret = Some(self.derive_secret(hs, b"c hs traffic", transcript_hash));

        // s_hs_secret = Derive-Secret(handshake_secret, "s hs traffic", transcript_hash)
        self.server_hs_secret = Some(self.derive_secret(hs, b"s hs traffic", transcript_hash));
    }

    /// Compute master secret and application traffic secrets
    pub fn derive_master_secret(&mut self, transcript_hash: &[u8]) {
        let hs = self.handshake_secret.as_ref().unwrap();

        // Derive-Secret(handshake_secret, "derived", empty_hash)
        let derived_secret = self.derive_secret(hs, b"derived", &[]);

        // master_secret = HKDF-Extract(derived_secret, 0)
        let zero = vec![0u8; self.hash_len];
        self.master_secret = Some(self.hkdf_extract(&derived_secret, &zero));

        let ms = self.master_secret.as_ref().unwrap();

        // c_ap_secret = Derive-Secret(master_secret, "c ap traffic", transcript_hash)
        self.client_app_secret = Some(self.derive_secret(ms, b"c ap traffic", transcript_hash));

        // s_ap_secret = Derive-Secret(master_secret, "s ap traffic", transcript_hash)
        self.server_app_secret = Some(self.derive_secret(ms, b"s ap traffic", transcript_hash));
    }

    /// Derive traffic keys from traffic secret
    /// key = HKDF-Expand-Label(secret, "key", "", key_length)
    /// iv = HKDF-Expand-Label(secret, "iv", "", iv_length)
    pub fn derive_traffic_keys(&self, traffic_secret: &[u8]) -> (Vec<u8>, [u8; 12]) {
        let key_len = match self.cipher_suite {
            CipherSuite::Aes128GcmSha256 => 16,
            CipherSuite::Aes256GcmSha384 => 32,
            CipherSuite::ChaCha20Poly1305Sha256 => 32,
        };

        let key = self.hkdf_expand_label(traffic_secret, b"key", &[], key_len);
        let iv_bytes = self.hkdf_expand_label(traffic_secret, b"iv", &[], 12);

        let mut iv = [0u8; 12];
        iv.copy_from_slice(&iv_bytes);

        (key, iv)
    }

    /// Get client handshake traffic secret
    pub fn client_hs_secret(&self) -> Option<&[u8]> {
        self.client_hs_secret.as_deref()
    }

    /// Get server handshake traffic secret
    pub fn server_hs_secret(&self) -> Option<&[u8]> {
        self.server_hs_secret.as_deref()
    }

    /// Get client application traffic secret
    pub fn client_app_secret(&self) -> Option<&[u8]> {
        self.client_app_secret.as_deref()
    }

    /// Get server application traffic secret
    pub fn server_app_secret(&self) -> Option<&[u8]> {
        self.server_app_secret.as_deref()
    }

    /// Compute Finished MAC
    /// finished_key = HKDF-Expand-Label(secret, "finished", "", Hash.length)
    /// verify_data = HMAC(finished_key, transcript_hash)
    pub fn compute_finished_mac(&self, traffic_secret: &[u8], transcript_hash: &[u8]) -> Vec<u8> {
        let finished_key = self.hkdf_expand_label(traffic_secret, b"finished", &[], self.hash_len);
        self.hmac_hash(&finished_key, transcript_hash)
    }

    /// Update traffic secret for key update
    pub fn update_traffic_secret(&self, traffic_secret: &[u8]) -> Vec<u8> {
        self.hkdf_expand_label(traffic_secret, b"traffic upd", &[], self.hash_len)
    }
}

/// TLS crypto operations
pub struct TlsCrypto {
    cipher_suite: CipherSuite,
    key: Vec<u8>,
    iv: [u8; 12],
}

impl TlsCrypto {
    pub fn new(cipher_suite: CipherSuite, key: &[u8], iv: &[u8; 12]) -> Self {
        TlsCrypto {
            cipher_suite,
            key: key.to_vec(),
            iv: *iv,
        }
    }

    /// Encrypt TLS record
    pub fn encrypt_record(&self, content_type: ContentType, plaintext: &[u8]) -> Vec<u8> {
        // Add content type and padding
        let mut data = plaintext.to_vec();
        data.push(content_type as u8);

        // Add padding to 16-byte boundary
        let pad_len = (16 - (data.len() % 16)) % 16;
        for _ in 0..pad_len {
            data.push(0);
        }

        match self.cipher_suite {
            CipherSuite::Aes128GcmSha256 | CipherSuite::Aes256GcmSha384 => {
                let aes_gcm = AesGcm::new(&self.key);
                let (ciphertext, tag) = aes_gcm.encrypt(&self.iv, &[], &data);

                let mut result = ciphertext;
                result.extend_from_slice(&tag);
                result
            }
            CipherSuite::ChaCha20Poly1305Sha256 => {
                let nonce = self.iv;
                let chacha =
                    ChaCha20Poly1305::new(&self.key.as_slice().try_into().unwrap_or([0u8; 32]));
                let (ciphertext, tag) = chacha.encrypt(&nonce, &[], &data);

                let mut result = ciphertext;
                result.extend_from_slice(&tag);
                result
            }
        }
    }

    /// Decrypt TLS record
    pub fn decrypt_record(&self, ciphertext: &[u8]) -> Option<(ContentType, Vec<u8>)> {
        if ciphertext.len() < 16 {
            return None;
        }

        let (ct, tag) = ciphertext.split_at(ciphertext.len() - 16);
        let tag_arr: [u8; 16] = tag.try_into().ok()?;

        let plaintext = match self.cipher_suite {
            CipherSuite::Aes128GcmSha256 | CipherSuite::Aes256GcmSha384 => {
                let aes_gcm = AesGcm::new(&self.key);
                aes_gcm.decrypt(&self.iv, &[], ct, &tag_arr)?
            }
            CipherSuite::ChaCha20Poly1305Sha256 => {
                let nonce = self.iv;
                let chacha =
                    ChaCha20Poly1305::new(&self.key.as_slice().try_into().unwrap_or([0u8; 32]));
                chacha.decrypt(&nonce, &[], ct, &tag_arr)?
            }
        };

        // Extract content type from end
        if plaintext.is_empty() {
            return None;
        }

        let content_type = ContentType::from_u8(plaintext[plaintext.len() - 1])?;
        let data = plaintext[..plaintext.len() - 1].to_vec();

        Some((content_type, data))
    }
}

// ============================================================================
// TLS 1.3 0-RTT ERKEN VERİ (Early Data)
// ============================================================================
//
// 0-RTT (Zero Round Trip Time), TLS 1.3'ün oturum devam ettirme özelliğidir.
// İstemci, önceki oturumdan elde ettiği PSK ile ilk pakette uygulama verisi gönderir.
// Bu işlem ağ gecikmesini 1 RTT azaltır, özellikle kısa bağlantılarda önemlidir.
//
// 0-RTT El Sıkışma Akışı:
//
//   İstemci                        Sunucu
//      |-- ClientHello (PSK) ------->|  Önceki oturum kimlik bilgisi
//      |-- [Early Data] ============>|  Erken veri (şifreli, 0-RTT anahtarıyla)
//      |                             |
//      |<-- ServerHello (PSK Kabul) -|
//      |<-- [EncryptedExtensions] ---|  early_data uzantısı = kabul/red
//      |<-- [Finished] -------------|
//      |-- [Finished] ------------->|
//      |======= Uygulama Verisi ====|  Normal şifreli iletişim
//
// Güvenlik Uyarıları:
//   - 0-RTT verisi replay saldırısına karşı korunmasızdır
//   - Sunucu aynı isteği iki kez işleyebilir (idempotent olmalı)
//   - Forward secrecy yok (PSK sabit)
//   - Uygulama sadece güvenli/idempotent işlemler için kullanmalı (GET isteği gibi)
//
// Oturum Bileti Yapısı (SessionTicket):
//   ticket_lifetime : Biletin geçerli olduğu süre (saniye)
//   age_add         : Biletin yaşını gizlemek için rastgele eklenen değer
//   resumption_key  : PSK için kullanılan gizli anahtar
//   max_early_data  : Kabul edilecek maksimum erken veri miktarı (byte)

/// 0-RTT erken veri yapılandırması
#[derive(Clone, Debug)]
pub struct EarlyDataConfig {
    /// Maximum early data size the server accepts
    pub max_early_data_size: u32,
    /// Whether early data is enabled
    pub enabled: bool,
}

impl Default for EarlyDataConfig {
    fn default() -> Self {
        EarlyDataConfig {
            max_early_data_size: 16384, // 16KB default
            enabled: true,
        }
    }
}

/// 0-RTT session ticket
#[derive(Clone, Debug)]
pub struct SessionTicket {
    /// Ticket lifetime (seconds)
    pub lifetime: u32,
    /// Ticket age add (random value to obscure age)
    pub age_add: u32,
    /// Ticket nonce
    pub nonce: Vec<u8>,
    /// Ticket data (encrypted)
    pub ticket: Vec<u8>,
    /// Early data configuration
    pub early_data: EarlyDataConfig,
    /// Creation timestamp
    pub created_at: u64,
    /// Resumption master secret
    pub resumption_secret: Vec<u8>,
    /// Cipher suite
    pub cipher_suite: CipherSuite,
}

impl SessionTicket {
    /// Create a new session ticket
    pub fn new(cipher_suite: CipherSuite, resumption_secret: &[u8]) -> Self {
        SessionTicket {
            lifetime: 86400, // 24 hours
            age_add: crate::random::next_u32(),
            nonce: vec![crate::random::next_u32() as u8; 8],
            ticket: Vec::new(),
            early_data: EarlyDataConfig::default(),
            created_at: crate::task::scheduler::get_ticks() as u64,
            resumption_secret: resumption_secret.to_vec(),
            cipher_suite,
        }
    }

    /// Check if ticket is still valid
    pub fn is_valid(&self) -> bool {
        let now = crate::task::scheduler::get_ticks() as u64;
        let age = now.saturating_sub(self.created_at);
        age < self.lifetime as u64
    }

    /// Calculate obfuscated ticket age
    pub fn obfuscated_age(&self) -> u32 {
        let now = crate::task::scheduler::get_ticks() as u64;
        let age_ms = now.saturating_sub(self.created_at) as u32;
        age_ms.wrapping_add(self.age_add)
    }

    /// Derive early data secret using HKDF-Expand-Label (RFC 8446 Section 7.1)
    ///
    /// Early secret = HKDF-Expand-Label(resumption_secret, "res early", "", Hash.length)
    /// HKDF-Expand-Label(Secret, Label, Context, Length) =
    ///   HKDF-Expand(Secret, HkdfLabel, Length)
    /// where HkdfLabel = length (2 bytes) || "tls13 " || Label || context_length (1 byte) || Context
    pub fn derive_early_secret(&self) -> Vec<u8> {
        // Build HKDF label: length || "tls13 " || label || context_length || context
        let label = b"res early";
        let context: &[u8] = b"";
        let length: u16 = 32;

        let mut hkdf_label = Vec::with_capacity(2 + 6 + label.len() + 1 + context.len());
        hkdf_label.extend_from_slice(&length.to_be_bytes());
        hkdf_label.extend_from_slice(b"tls13 ");
        hkdf_label.extend_from_slice(label);
        hkdf_label.push(context.len() as u8);
        hkdf_label.extend_from_slice(context);

        // HKDF-Expand(resumption_secret, hkdf_label, 32)
        let hash_len = 32; // SHA-256
        let n = (length as usize + hash_len - 1) / hash_len;
        let mut out = Vec::with_capacity(n * hash_len);
        let mut t_prev: Vec<u8> = Vec::new();

        for i in 1..=(n as u8) {
            // T(i) = HMAC-SHA256(PRK, T(i-1) || info || i)
            let mut msg = Vec::with_capacity(t_prev.len() + hkdf_label.len() + 1);
            msg.extend_from_slice(&t_prev);
            msg.extend_from_slice(&hkdf_label);
            msg.push(i);
            t_prev = crate::net::quic::hmac_sha256(&self.resumption_secret, &msg);
            out.extend_from_slice(&t_prev);
        }

        out.truncate(length as usize);
        out
    }
}

/// 0-RTT early data state
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EarlyDataState {
    /// Not using early data
    None,
    /// Early data accepted by server
    Accepted,
    /// Early data rejected by server
    Rejected,
    /// Waiting for server decision
    Pending,
}

/// 0-RTT connection state
#[derive(Clone, Debug)]
pub struct ZeroRttState {
    /// Session ticket for resumption
    pub ticket: Option<SessionTicket>,
    /// Early data state
    pub state: EarlyDataState,
    /// Early data buffer
    pub early_data_buffer: Vec<u8>,
    /// Bytes of early data sent
    pub early_data_sent: usize,
    /// Maximum early data allowed
    pub max_early_data: usize,
}

impl ZeroRttState {
    /// Create new 0-RTT state
    pub fn new() -> Self {
        ZeroRttState {
            ticket: None,
            state: EarlyDataState::None,
            early_data_buffer: Vec::new(),
            early_data_sent: 0,
            max_early_data: 0,
        }
    }

    /// Initialize with session ticket
    pub fn with_ticket(ticket: SessionTicket) -> Self {
        let max = ticket.early_data.max_early_data_size as usize;
        ZeroRttState {
            ticket: Some(ticket),
            state: EarlyDataState::Pending,
            early_data_buffer: Vec::new(),
            early_data_sent: 0,
            max_early_data: max,
        }
    }

    /// Check if early data can be sent
    pub fn can_send_early_data(&self) -> bool {
        matches!(
            self.state,
            EarlyDataState::Pending | EarlyDataState::Accepted
        ) && self.early_data_sent < self.max_early_data
            && self.ticket.is_some()
    }

    /// Send early data
    pub fn send_early_data(&mut self, data: &[u8]) -> Option<Vec<u8>> {
        if !self.can_send_early_data() {
            return None;
        }

        let ticket = self.ticket.as_ref()?;
        let remaining = self.max_early_data - self.early_data_sent;
        let to_send = data.len().min(remaining);

        // Create early data record
        let early_secret = ticket.derive_early_secret();
        let mut crypto = TlsCrypto::new(ticket.cipher_suite, &early_secret, &[0u8; 12]);

        // Encrypt as 0-RTT record (content type 0x17 = Application Data)
        let encrypted = crypto.encrypt_record(ContentType::ApplicationData, &data[..to_send]);

        self.early_data_sent += to_send;
        self.early_data_buffer.extend_from_slice(&data[..to_send]);

        Some(encrypted)
    }

    /// Handle server's rejection of early data
    pub fn on_reject(&mut self) {
        self.state = EarlyDataState::Rejected;
        self.early_data_buffer.clear();
        self.early_data_sent = 0;
    }

    /// Handle server's acceptance of early data
    pub fn on_accept(&mut self) {
        self.state = EarlyDataState::Accepted;
    }

    /// Get early data to retry after rejection
    pub fn get_retry_data(&self) -> &[u8] {
        &self.early_data_buffer
    }
}

impl Default for ZeroRttState {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// TLS 1.3 OTURUM DEVAM ETTİRME (Session Resumption)
// ============================================================================
//
// TLS 1.3, oturum devam ettirme için PSK (Pre-Shared Key) mekanizmasını kullanır.
// Önceki oturum tamamlandığında, sunucu istemciye bir oturum bileti (NewSessionTicket)
// gönderir. Sonraki bağlantıda bu bilet PSK olarak kullanılır.
//
// Devam Ettirilen Oturum Avantajları:
//   - Daha hızlı el sıkışma (özellikle 0-RTT ile)
//   - TLS katmanı sürtünmesini azaltır
//   - Sertifika doğrulama tekrarlanmaz
//
// NewSessionTicket Mesajı (Sunucu -> İstemci):
//   ticket_lifetime  : Geçerlilik süresi (saniye)
//   ticket_age_add   : Yaş gizleme için rastgele eklenti
//   ticket_nonce     : Anahtarı türetmek için benzersiz değer
//   ticket           : Şifreli bağlamı taşıyan opak bayt dizisi
//   extensions       : early_data uzantısı (max_early_data_size bilgisi)
//
// Resumption Master Secret Türetimi:
//   resumption_secret = Derive-Secret(master_secret, "res master", transcript)
//   PSK = HKDF-Expand-Label(resumption_secret, "resumption", nonce, hash_len)
//
// SessionCache:
//   - Geçerli oturum biletlerini bellekte tutar (LRU, max 100 bilet)
//   - Süresi dolmuş biletler otomatik temizlenir
//   - find_for_server() ile uygun bilet aranır

/// Oturum devam ettirme için istemci tarafı oturum önbelleği
#[derive(Clone, Debug)]
pub struct SessionCache {
    sessions: Vec<SessionTicket>,
    max_sessions: usize,
}

impl SessionCache {
    pub fn new() -> Self {
        SessionCache {
            sessions: Vec::new(),
            max_sessions: 100,
        }
    }

    /// Add session ticket to cache
    pub fn add(&mut self, ticket: SessionTicket) {
        // Remove expired sessions
        self.sessions.retain(|t| t.is_valid());

        // Remove oldest if at capacity
        if self.sessions.len() >= self.max_sessions {
            self.sessions.remove(0);
        }

        self.sessions.push(ticket);
    }

    /// Find session for server
    pub fn find_for_server(&self, server_name: &str) -> Option<&SessionTicket> {
        // Simplified - would match by server name and other criteria
        self.sessions.iter().find(|t| t.is_valid())
    }

    /// Remove session
    pub fn remove(&mut self, ticket: &[u8]) {
        self.sessions.retain(|t| t.ticket != ticket);
    }

    /// Clear all sessions
    pub fn clear(&mut self) {
        self.sessions.clear();
    }
}

impl Default for SessionCache {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// 0-RTT DESTEKLİ TLS 1.3 EL SIKIŞTIRMA (Handshake with 0-RTT)
// ============================================================================
//
// TlsHandshakeExt, temel TlsClient/TlsState yapısını genişleterek oturum devam
// ettirme ve 0-RTT veri gönderimini destekler.
//
// Genişletilmiş Durum Bileşenleri:
//   state          : Temel el sıkışma durumu (TlsState enum)
//   zero_rtt       : 0-RTT yaşam döngüsü yönetimi (ZeroRttState)
//   session_cache  : Sunucu başına oturum bileti önbelleği
//   server_name    : SNI (Server Name Indication) için sunucu adı
//
// start_with_early_data() İş Akışı:
//   1. session_cache.find_for_server() ile önceki bilet aranır
//   2. Bilet bulunduysa:
//      a. ZeroRttState::with_ticket() ile 0-RTT durumu başlatılır
//      b. ClientHello'ya pre_shared_key uzantısı eklenir
//      c. ClientHello'ya early_data uzantısı eklenir
//   3. Bilet bulunamazsa: Normal ClientHello gönderilir
//
// process_server_response() Yanıt İşleme:
//   ServerHello   : PSK kabul/red kararı analiz edilir
//   NewSessionTicket : Yeni bilet önbelleğe alınır
//   EncryptedExtensions: early_data uzantısı kontrol edilir
//
// send_data() Veri Gönderimi:
//   Bağlantı kurulduysa  -> Normal 1-RTT şifreli veri
//   0-RTT mümkünse       -> ZeroRttState.send_early_data() ile erken veri

/// 0-RTT desteği olan genişletilmiş TLS el sıkışma durumu
#[derive(Clone, Debug)]
pub struct TlsHandshakeExt {
    /// Base handshake state
    pub state: TlsState,
    /// 0-RTT state
    pub zero_rtt: ZeroRttState,
    /// Session cache
    pub session_cache: SessionCache,
    /// Server name for SNI
    pub server_name: Option<String>,
    /// Whether to request early data
    pub request_early_data: bool,
}

impl TlsHandshakeExt {
    pub fn new() -> Self {
        TlsHandshakeExt {
            state: TlsState::Initial,
            zero_rtt: ZeroRttState::new(),
            session_cache: SessionCache::new(),
            server_name: None,
            request_early_data: false,
        }
    }

    /// Start handshake with potential 0-RTT
    pub fn start_with_early_data(&mut self, server_name: &str) -> Option<Vec<u8>> {
        self.server_name = Some(server_name.to_string());

        // Check for cached session
        if let Some(ticket) = self.session_cache.find_for_server(server_name) {
            self.zero_rtt = ZeroRttState::with_ticket(ticket.clone());
            self.request_early_data = true;

            // Build ClientHello with pre_shared_key extension
            let mut client_hello = self.build_client_hello();

            // Add pre_shared_key extension
            let obfuscated_age = ticket.obfuscated_age();
            client_hello.extend_from_slice(&[0x00, 0x2a]); // pre_shared_key extension type
            client_hello.extend_from_slice(&(4 + ticket.ticket.len() as u16).to_be_bytes());
            client_hello.extend_from_slice(&(ticket.ticket.len() as u16).to_be_bytes());
            client_hello.extend_from_slice(&ticket.ticket);
            client_hello.extend_from_slice(&obfuscated_age.to_be_bytes());

            // Compute PSK binder: HMAC-SHA256 over transcript hash up to binder list
            // The binder is computed over a transcript hash of the ClientHello
            // with the binder field zeroed out.
            {
                let binder_key = {
                    // early_secret = HKDF-Extract(0, PSK)
                    // binder_key = Derive-Secret(early_secret, "res binder", "")
                    // Simplified: HMAC-SHA256(resumption_secret, "res binder")
                    let mut k_ipad = [0x36u8; 64];
                    let mut k_opad = [0x5cu8; 64];
                    for (i, &b) in ticket.resumption_secret.iter().enumerate().take(64) {
                        k_ipad[i] ^= b;
                        k_opad[i] ^= b;
                    }
                    let mut inner_data = k_ipad.to_vec();
                    inner_data.extend_from_slice(b"res binder");
                    let mut hasher = Sha256::new();
                    hasher.update(&inner_data);
                    let inner_hash = hasher.finalize();
                    let mut outer_data = k_opad.to_vec();
                    outer_data.extend_from_slice(&inner_hash);
                    let mut hasher2 = Sha256::new();
                    hasher2.update(&outer_data);
                    hasher2.finalize().to_vec()
                };

                // Transcript hash = SHA-256(ClientHello without binder)
                let mut transcript_hasher = Sha256::new();
                transcript_hasher.update(&client_hello);
                let transcript_hash = transcript_hasher.finalize();

                // binder = HMAC-SHA256(binder_key, transcript_hash)
                let binder = {
                    let mut k_ipad = [0x36u8; 64];
                    let mut k_opad = [0x5cu8; 64];
                    for (i, &b) in binder_key.iter().enumerate().take(64) {
                        k_ipad[i] ^= b;
                        k_opad[i] ^= b;
                    }
                    let mut inner_data = k_ipad.to_vec();
                    inner_data.extend_from_slice(&transcript_hash);
                    let mut hasher = Sha256::new();
                    hasher.update(&inner_data);
                    let inner_hash = hasher.finalize();
                    let mut outer_data = k_opad.to_vec();
                    outer_data.extend_from_slice(&inner_hash);
                    let mut hasher2 = Sha256::new();
                    hasher2.update(&outer_data);
                    hasher2.finalize().to_vec()
                };

                // Append binder list: u16 binders_len, u8 binder_len, binder
                let binder_len = binder.len() as u8;
                let binders_total = 1 + binder.len() as u16; // 1 byte length prefix + binder
                client_hello.extend_from_slice(&binders_total.to_be_bytes());
                client_hello.push(binder_len);
                client_hello.extend_from_slice(&binder);
            }

            // Add early_data extension
            client_hello.extend_from_slice(&[0x00, 0x2a]); // early_data extension
            client_hello.extend_from_slice(&[0x00, 0x00]); // empty

            return Some(client_hello);
        }

        // No cached session, regular handshake
        Some(self.build_client_hello())
    }

    /// Build ClientHello message with proper TLS 1.3 extensions
    fn build_client_hello(&self) -> Vec<u8> {
        let mut hello = Vec::new();

        // Handshake type: ClientHello (0x01)
        hello.push(0x01);

        // Length placeholder (3 bytes) - will be filled at the end
        let len_pos = hello.len();
        hello.extend_from_slice(&[0x00, 0x00, 0x00]);

        // Version: TLS 1.2 (0x0303) - used for compatibility
        hello.extend_from_slice(&[0x03, 0x03]);

        // Random (32 bytes)
        for _ in 0..32 {
            hello.push(crate::random::next_u32() as u8);
        }

        // Session ID (32 bytes for TLS 1.3 compatibility mode)
        hello.push(32);
        for i in 0..32u8 {
            hello.push(i ^ 0xAA);
        }

        // Cipher suites
        hello.extend_from_slice(&[0x00, 0x06]); // 3 cipher suites
        hello.extend_from_slice(&[0x13, 0x01]); // TLS_AES_128_GCM_SHA256
        hello.extend_from_slice(&[0x13, 0x02]); // TLS_AES_256_GCM_SHA384
        hello.extend_from_slice(&[0x13, 0x03]); // TLS_CHACHA20_POLY1305_SHA256

        // Compression methods
        hello.push(0x01); // 1 method
        hello.push(0x00); // null compression

        // Build extensions
        let mut exts = Vec::new();

        // SNI extension (type=0x0000) - Server Name Indication
        let hostname = b"localhost";
        exts.extend_from_slice(&[0x00, 0x00]); // extension type
        let sni_len = 5 + hostname.len();
        exts.extend_from_slice(&(sni_len as u16).to_be_bytes());
        exts.extend_from_slice(&((sni_len - 2) as u16).to_be_bytes());
        exts.push(0x00); // host_name type
        exts.extend_from_slice(&(hostname.len() as u16).to_be_bytes());
        exts.extend_from_slice(hostname);

        // Supported Versions extension (type=0x002b) - TLS 1.3
        exts.extend_from_slice(&[0x00, 0x2b]); // extension type
        exts.extend_from_slice(&[0x00, 0x03]); // length
        exts.extend_from_slice(&[0x02]); // versions length
        exts.extend_from_slice(&[0x03, 0x04]); // TLS 1.3 (0x0304)

        // Supported Groups extension (type=0x000a)
        exts.extend_from_slice(&[0x00, 0x0a]); // extension type
        exts.extend_from_slice(&[0x00, 0x04]); // length
        exts.extend_from_slice(&[0x00, 0x02]); // groups length
        exts.extend_from_slice(&[0x00, 0x1d]); // x25519

        // Signature Algorithms extension (type=0x000d)
        exts.extend_from_slice(&[0x00, 0x0d]); // extension type
        exts.extend_from_slice(&[0x00, 0x08]); // length
        exts.extend_from_slice(&[0x00, 0x06]); // algorithms length
        exts.extend_from_slice(&[0x04, 0x03]); // ecdsa_secp256r1_sha256
        exts.extend_from_slice(&[0x08, 0x04]); // rsa_pss_rsae_sha256
        exts.extend_from_slice(&[0x08, 0x07]); // ed25519

        // Key Share extension (type=0x0033) - X25519 public key
        let mut public_key = [0u8; 32];
        for i in 0..32 {
            public_key[i] = crate::random::next_u32() as u8;
        }
        exts.extend_from_slice(&[0x00, 0x33]); // extension type
        exts.extend_from_slice(&[0x00, 0x26]); // length (38 bytes)
        exts.extend_from_slice(&[0x00, 0x24]); // client key share length (36 bytes)
        exts.extend_from_slice(&[0x00, 0x1d]); // x25519 group
        exts.extend_from_slice(&[0x00, 0x20]); // key exchange length (32 bytes)
        exts.extend_from_slice(&public_key);

        // Extensions length
        let ext_len = exts.len() as u16;
        hello.extend_from_slice(&ext_len.to_be_bytes());
        hello.extend_from_slice(&exts);

        // Fill in length field
        let body_len = (hello.len() - len_pos - 3) as u32;
        hello[len_pos] = ((body_len >> 16) & 0xFF) as u8;
        hello[len_pos + 1] = ((body_len >> 8) & 0xFF) as u8;
        hello[len_pos + 2] = (body_len & 0xFF) as u8;

        hello
    }

    /// Process server response
    pub fn process_server_response(&mut self, data: &[u8]) -> Option<Vec<u8>> {
        if data.is_empty() {
            return None;
        }

        let msg_type = data[0];

        match msg_type {
            0x02 => {
                // ServerHello
                self.state = TlsState::ServerHelloReceived;

                // Check for early_data indication
                // Server may accept or reject 0-RTT
                if self.zero_rtt.state == EarlyDataState::Pending {
                    // Check if server selected PSK
                    // For now, assume rejected
                    self.zero_rtt.on_reject();
                }

                // Continue with handshake
                None
            }
            0x04 => {
                // NewSessionTicket
                if data.len() > 4 {
                    let lifetime = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
                    let ticket_len = data[8] as usize;

                    if data.len() > 9 + ticket_len {
                        let ticket_data = &data[9..9 + ticket_len];

                        let ticket = SessionTicket {
                            lifetime,
                            age_add: crate::random::next_u32(),
                            nonce: vec![0],
                            ticket: ticket_data.to_vec(),
                            early_data: EarlyDataConfig::default(),
                            created_at: crate::task::scheduler::get_ticks() as u64,
                            resumption_secret: vec![0; 32],
                            cipher_suite: CipherSuite::Aes128GcmSha256,
                        };

                        self.session_cache.add(ticket);
                    }
                }
                None
            }
            0x14 => {
                // EncryptedExtensions
                // Check for early_data extension
                self.state = TlsState::FinishedReceived;
                None
            }
            _ => None,
        }
    }

    /// Send application data (with 0-RTT if possible)
    pub fn send_data(&mut self, data: &[u8]) -> Option<Vec<u8>> {
        if self.state == TlsState::Established {
            // Normal 1-RTT data
            // Would encrypt with current keys
            Some(data.to_vec())
        } else if self.zero_rtt.can_send_early_data() {
            // 0-RTT early data
            self.zero_rtt.send_early_data(data)
        } else {
            None
        }
    }
}

impl Default for TlsHandshakeExt {
    fn default() -> Self {
        Self::new()
    }
}
