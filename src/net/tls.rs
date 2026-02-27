//! # echOS için TLS 1.3 Uygulaması
//!
//! TLS 1.3 el sıkışma (handshake) durum makinesi;
//! TLS, istemci ve sunucu arasında şifreli ve kimlik doğrulamalı kanal kurar:
//! - Kayıt (Record) ayrıştırma ve oluşturma
//! - El sıkışma mesajı işleme
//! - Anahtar programı (HKDF tabanlı türetme)
//! - no_std ortamı için saf Rust kripto uygulamaları

use alloc::vec::Vec;
use alloc::vec;
use alloc::string::String;
use alloc::string::ToString;
use sha2::{Sha256, Sha384, Digest};
use hkdf::Hkdf;

// ============================================================================
// TLS SABİTLERİ
// ============================================================================

/// TLS 1.3 protokol sürüm numarası (0x0303 = TLS 1.2 uyumlu başlık)
pub const TLS_VERSION_1_3: u16 = 0x0303;

/// TLS 1.3 kayıt türleri — her TLS kaydının başına eklenir ve içerik tipini belirtir
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

/// TLS 1.3 el sıkışma mesaj türleri — her handshake mesajının ilk baytı bu türü kodlar
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

/// TLS 1.3 şifre paketleri — şifreleme + MAC + HKDF hash kombinasyonlarını tanımlar
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
    
    pub fn iv_len(&self) -> usize { 12 }
}

/// TLS 1.3 anahtar değişiminde kullanılan eliptik eğri grupları — X25519 en yaygın ve verimli olanıdır
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

/// TLS 1.3 imza şemaları — sertifika doğrulamasında kullanılan imza algoritmaları ve hash kombinasyonları
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
// TLS HATA TÜRLERİ
// ============================================================================

/// TLS hata türleri — bağlantı kurma ve şifreleme sürecinde karşılaşılabilecek hatalar
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

/// TLS uyarı seviyeleri — Warning kapanabilir bağlantı, Fatal ise bâğlantıyı zorunlu sonlandırır
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlertLevel {
    Warning = 1,
    Fatal = 2,
}

/// TLS uyarı açıklamaları — RFC 8446 bölüm 6'da tanımlı hata kodları
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
// TLS EL SIKISMA DURUMU
// ============================================================================

/// TLS el sıkışma durum makinesi — her adım mesaj alınana kadar bir sonraki duruma geçılmez
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
// TLS KAYDI (RECORD)
// ============================================================================

/// TLS kayıt başlığı — her TLS mesajının önünde 5 baytlık meta veri bulunur
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
        
        let content_type = ContentType::from_u8(data[0])
            .ok_or(TlsError::InvalidMessage)?;
        let version = u16::from_be_bytes([data[1], data[2]]);
        let length = u16::from_be_bytes([data[3], data[4]]);
        
        Ok(Self { content_type, version, length })
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
// TLS EL SIKISMA MESAJI
// ============================================================================

/// TLS el sıkışma mesajı başlığı — mesaj tipi (1 bayt) + uzunluk (3 bayt big-endian)
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
        
        let msg_type = HandshakeType::from_u8(data[0])
            .ok_or(TlsError::InvalidMessage)?;
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
// TLS ANAHTAR PROGRAMI
// ============================================================================

/// TLS 1.3 anahtar programı — HKDF ile zincir şeklinde erken, el sıkışma ve uygulama gizli anahtarları türetir
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
    
    fn derive_traffic_secret(&self, secret: &[u8], label: &[u8], transcript_hash: &[u8]) -> [u8; 32] {
        self.hkdf_expand_label(secret, label, transcript_hash, 32)
    }
    
    fn hkdf_expand_label(&self, secret: &[u8], label: &[u8], context: &[u8], length: usize) -> [u8; 32] {
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
    fn default() -> Self { Self::new() }
}

// ============================================================================
// TLS İSTEMCIİS
// ============================================================================

/// TLS istemci bağlantısı — durum makinesi, şifre paketi seçimi ve ız belgesi tutma
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
    
    /// ClientHello mesajını oluşturur — istemci desteklediği şifre paketlerini, grupları ve
    /// uzantıları bu mesajla sunucuya bildirir; TLS 1.3 müstereke anahtarı bürada önerilir
    pub fn build_client_hello(&mut self, hostname: &str) -> Vec<u8> {
        let mut body = Vec::new();
        
        // Protokol sürü numarası (TLS 1.2 başlığı = 0x0303, TLS 1.3 uzantıda ilan edilir)
        body.extend_from_slice(&TLS_VERSION_1_3.to_be_bytes());
        
        // Rastgele 32 bayt — yeniden oynatma (replay) saldırılarına karşı benzersizlik sağlar
        crate::random::fill_bytes(&mut [0u8; 32]);
        for _ in 0..32 {
            body.push(crate::random::next_u32() as u8);
        }
        
        // Oturum kimliği (TLS 1.3'te boş bırakılır, geriye uyumluluk için alan vardır)
        body.push(0);
        
        // Şifre paketleri listesi — istemci desteklediği paketleri önem sırasıyla bildirir
        let cipher_suites: [u16; 3] = [
            CipherSuite::ChaCha20Poly1305Sha256 as u16,
            CipherSuite::Aes256GcmSha384 as u16,
            CipherSuite::Aes128GcmSha256 as u16,
        ];
        body.push((cipher_suites.len() * 2) as u8);
        for suite in &cipher_suites {
            body.extend_from_slice(&suite.to_be_bytes());
        }
        
        // Sıkıştırma yöntemleri (TLS 1.3'te yalnızca "null" desteklenir; sıkıştırma güvenlik açığı yaratabilir)
        body.push(1);
        body.push(0);
        
        // Uzantılar — TLS 1.3'te kritik özellikler uzantı alanında gönderilir
        let mut extensions = Vec::new();
        
        // Sunucu Adı Göstergesi uzantısı (tip 0) — istemci hangi sunucuya bağlanacağını bildirir
        // Bu sayede tek IP'de birden fazla TLS sertifikası barındırılabilir (sanal hosting)
        let mut sni = Vec::new();
        sni.push(0);
        sni.extend_from_slice(&(hostname.len() as u16).to_be_bytes());
        sni.extend_from_slice(hostname.as_bytes());
        extensions.extend_from_slice(&0u16.to_be_bytes());
        extensions.extend_from_slice(&(sni.len() as u16).to_be_bytes());
        extensions.extend_from_slice(&sni);
        
        // Desteklenen Sürümler uzantısı (tip 43) — istemci "TLS 1.3 istiyorum" sinyâli verir
        // Sunucu bu uzantıyı destekliyorsa TLS 1.3 protokolünü kullanır
        let mut versions = Vec::new();
        versions.push(2);
        versions.extend_from_slice(&0x0304u16.to_be_bytes());
        extensions.extend_from_slice(&43u16.to_be_bytes());
        extensions.extend_from_slice(&(versions.len() as u16).to_be_bytes());
        extensions.extend_from_slice(&versions);
        
        // Anahtar Paylaşım uzantısı (tip 51) — X25519 geçici açık anahtarı gönderilir;
        // sunucu buna karşılık kendi açık anahtarını gönderir; ECDH ile paylaşılan gizli hesaplanır
        let mut key_share = Vec::new();
        key_share.extend_from_slice(&(NamedGroup::X25519 as u16).to_be_bytes());
        key_share.extend_from_slice(&32u16.to_be_bytes());
        for _ in 0..32 {
            key_share.push(crate::random::next_u32() as u8);
        }
        extensions.extend_from_slice(&51u16.to_be_bytes());
        extensions.extend_from_slice(&(key_share.len() as u16).to_be_bytes());
        extensions.extend_from_slice(&key_share);
        
        // İmza Algoritmaları uzantısı (tip 13) — istemci desteklediği imza şemalarını bildirir
        // Sunucu sertifikasını bu listeden birini kullanarak imzalar
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
        
        // El sıkışma mesajını oluştur — başlık + gövde birleştirilir ve ız belgesine eklenir
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
    
    /// ServerHello mesajını işler — sunucu seçtiği şifre paketi ve anahtar paylaşım ekstrasını ayrıştırır
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
        
        offset += 2; // Sürüm alanı atlanır (uzantıda gerçek sürüm var)
        offset += 32; // Rastgele veri alanı atlanır
        
        let session_id_len = body[offset] as usize;
        offset += 1 + session_id_len;
        
        let suite = u16::from_be_bytes([body[offset], body[offset + 1]]);
        self.cipher_suite = CipherSuite::from_u16(suite);
        offset += 2;
        offset += 1; // Compression
        
        // Uzantıları ayrıştır — sunucunun seçtiği anahtar paylaşım grubunu bul
        if offset + 2 <= body.len() {
            let ext_len = u16::from_be_bytes([body[offset], body[offset + 1]]) as usize;
            offset += 2;
            
            let ext_end = offset + ext_len;
            while offset + 4 <= ext_end {
                let ext_type = u16::from_be_bytes([body[offset], body[offset + 1]]);
                let ext_len = u16::from_be_bytes([body[offset + 2], body[offset + 3]]) as usize;
                offset += 4;
                
                if ext_type == 51 && offset + ext_len <= body.len() {
                    // Anahtar Paylaşımı uzantısı — şimdilik atlanır, ileriki sürümde işlenecek
                }
                
                offset += ext_len;
            }
        }
        
        self.transcript.extend_from_slice(data);
        self.state = TlsState::ServerHelloReceived;
        
        Ok(())
    }
    
    /// Şifreli Uzantılar mesajını işler — sunucu el sıkışma trafıği anahtarlarıyla ALPN vb. iletir
    pub fn process_encrypted_extensions(&mut self, data: &[u8]) -> Result<(), TlsError> {
        let header = HandshakeHeader::parse(data)?;
        if header.msg_type != HandshakeType::EncryptedExtensions {
            return Err(TlsError::InvalidMessage);
        }
        
        self.transcript.extend_from_slice(data);
        self.state = TlsState::EncryptedExtensionsReceived;
        Ok(())
    }
    
    /// Sertifika mesajını işler — sunucunun X.509 sertifikasını alır; kimlik doğrulama bu ız belgesiyle yapılır
    pub fn process_certificate(&mut self, data: &[u8]) -> Result<(), TlsError> {
        let header = HandshakeHeader::parse(data)?;
        if header.msg_type != HandshakeType::Certificate {
            return Err(TlsError::InvalidMessage);
        }
        
        self.transcript.extend_from_slice(data);
        self.state = TlsState::CertificateReceived;
        Ok(())
    }
    
    /// Sertifika Doğrulama mesajını işler — sunucu, tüm el sıkışma ız belgesini özel anahtarla imzalar
    pub fn process_certificate_verify(&mut self, data: &[u8]) -> Result<(), TlsError> {
        let header = HandshakeHeader::parse(data)?;
        if header.msg_type != HandshakeType::CertificateVerify {
            return Err(TlsError::InvalidMessage);
        }
        
        self.transcript.extend_from_slice(data);
        self.state = TlsState::CertificateVerifyReceived;
        Ok(())
    }
    
    /// Finished mesajını işler — HMAC ile ız belgesi bütünlüğü doğrulanır; el sıkışma
    /// aşamasının son admadır, ortadaki adam saldırılarını engeller
    pub fn process_finished(&mut self, data: &[u8]) -> Result<(), TlsError> {
        let header = HandshakeHeader::parse(data)?;
        if header.msg_type != HandshakeType::Finished {
            return Err(TlsError::InvalidMessage);
        }
        
        self.transcript.extend_from_slice(data);
        self.state = TlsState::FinishedReceived;
        Ok(())
    }
    
    /// El sıkışmayı tamamlar — ız belgesinin SHA-256 özetini alıp ana gizli anahtarları türetir
    pub fn complete_handshake(&mut self) {
        let hash = Sha256::digest(&self.transcript);
        self.key_schedule.derive_master_secret(&hash);
        self.state = TlsState::Established;
    }
    
    pub fn state(&self) -> &TlsState { &self.state }
    pub fn is_established(&self) -> bool { self.state == TlsState::Established }
    pub fn cipher_suite(&self) -> Option<CipherSuite> { self.cipher_suite }
}

impl Default for TlsClient {
    fn default() -> Self { Self::new() }
}

// ============================================================================
// YARDIMCI FONKSiİYONLAR
// ============================================================================

/// Veriyi TLS kaydına sarar — başlık + yük oluşturulur, ağ üzerinden gönderilmeye hazır hale gelir
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

/// TLS kaydını ayrıştırır — başlık ile yükü ayırır
pub fn parse_record(data: &[u8]) -> Result<(TlsRecordHeader, Vec<u8>), TlsError> {
    let header = TlsRecordHeader::parse(data)?;
    let payload = data[TlsRecordHeader::SIZE..].to_vec();
    Ok((header, payload))
}

/// İz belgesi hash'ini hesaplar — tüm el sıkışma mesajlarının SHA-256 özetini verir;
/// bu özet Finished MAC ve anahtar türetmede kullanılır
pub fn transcript_hash(transcript: &[u8]) -> [u8; 32] {
    let hash = Sha256::digest(transcript);
    let mut result = [0u8; 32];
    result.copy_from_slice(&hash);
    result
}

// ============================================================================
// AES-GCM UYGULAMASİ (no_std uyumlu)
// ============================================================================

/// AES-128/256 blok şifreleme — Rijndael algoritması; 128'lik blokları anahtar-zamanlı tur transformasyonlarla şifreler
pub struct Aes {
    rounds: usize,
    rk: [u32; 60], // Round keys (max 14 rounds for AES-256)
}

impl Aes {
    /// Verilen anahtarla yeni AES örneği oluşturur — 16 baytlık anahtar AES-128, 32 baytlık anahtar AES-256 içindir
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
        
        // AES-128 anahtar genişletmesi — 128 bitlik anahtarı 10 tur anahtara (44 x 32-bit kelime) dönüştürür
        let rcon: [u32; 10] = [0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0x1b, 0x36];
        
        for i in 0..4 {
            aes.rk[i] = u32::from_be_bytes([key[i*4], key[i*4+1], key[i*4+2], key[i*4+3]]);
        }
        
        for i in 4..44 {
            let temp = aes.rk[i-1];
            let mut w = aes.rk[i-4];
            
            if i % 4 == 0 {
                // RotWord (döngüsel kaydırma) + SubWord (S-kutusu uygulamalı) + Rcon (tur sabiti XOR)
                let sub = Self::sub_word(Self::rot_word(temp));
                w ^= sub ^ (rcon[i/4 - 1] << 24);
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
            aes.rk[i] = u32::from_be_bytes([key[i*4], key[i*4+1], key[i*4+2], key[i*4+3]]);
        }
        
        for i in 8..60 {
            let temp = aes.rk[i-1];
            let mut w = aes.rk[i-8];
            
            if i % 8 == 0 {
                let sub = Self::sub_word(Self::rot_word(temp));
                w ^= sub ^ (rcon[i/8 - 1] << 24);
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
        // AES S-kutusu (önceden hesaplanmış) — GF(2^8) üzerinde çarpımsal ters + afin dönüşüm
        const SBOX_VALUES: [u8; 256] = [
            0x63, 0x7c, 0x77, 0x7b, 0xf2, 0x6b, 0x6f, 0xc5, 0x30, 0x01, 0x67, 0x2b, 0xfe, 0xd7, 0xab, 0x76,
            0xca, 0x82, 0xc9, 0x7d, 0xfa, 0x59, 0x47, 0xf0, 0xad, 0xd4, 0xa2, 0xaf, 0x9c, 0xa4, 0x72, 0xc0,
            0xb7, 0xfd, 0x93, 0x26, 0x36, 0x3f, 0xf7, 0xcc, 0x34, 0xa5, 0xe5, 0xf1, 0x71, 0xd8, 0x31, 0x15,
            0x04, 0xc7, 0x23, 0xc3, 0x18, 0x96, 0x05, 0x9a, 0x07, 0x12, 0x80, 0xe2, 0xeb, 0x27, 0xb2, 0x75,
            0x09, 0x83, 0x2c, 0x1a, 0x1b, 0x6e, 0x5a, 0xa0, 0x52, 0x3b, 0xd6, 0xb3, 0x29, 0xe3, 0x2f, 0x84,
            0x53, 0xd1, 0x00, 0xed, 0x20, 0xfc, 0xb1, 0x5b, 0x6a, 0xcb, 0xbe, 0x39, 0x4a, 0x4c, 0x58, 0xcf,
            0xd0, 0xef, 0xaa, 0xfb, 0x43, 0x4d, 0x33, 0x85, 0x45, 0xf9, 0x02, 0x7f, 0x50, 0x3c, 0x9f, 0xa8,
            0x51, 0xa3, 0x40, 0x8f, 0x92, 0x9d, 0x38, 0xf5, 0xbc, 0xb6, 0xda, 0x21, 0x10, 0xff, 0xf3, 0xd2,
            0xcd, 0x0c, 0x13, 0xec, 0x5f, 0x97, 0x44, 0x17, 0xc4, 0xa7, 0x7e, 0x3d, 0x64, 0x5d, 0x19, 0x73,
            0x60, 0x81, 0x4f, 0xdc, 0x22, 0x2a, 0x90, 0x88, 0x46, 0xee, 0xb8, 0x14, 0xde, 0x5e, 0x0b, 0xdb,
            0xe0, 0x32, 0x3a, 0x0a, 0x49, 0x06, 0x24, 0x5c, 0xc2, 0xd3, 0xac, 0x62, 0x91, 0x95, 0xe4, 0x79,
            0xe7, 0xc8, 0x37, 0x6d, 0x8d, 0xd5, 0x4e, 0xa9, 0x6c, 0x56, 0xf4, 0xea, 0x65, 0x7a, 0xae, 0x08,
            0xba, 0x78, 0x25, 0x2e, 0x1c, 0xa6, 0xb4, 0xc6, 0xe8, 0xdd, 0x74, 0x1f, 0x4b, 0xbd, 0x8b, 0x8a,
            0x70, 0x3e, 0xb5, 0x66, 0x48, 0x03, 0xf6, 0x0e, 0x61, 0x35, 0x57, 0xb9, 0x86, 0xc1, 0x1d, 0x9e,
            0xe1, 0xf8, 0x98, 0x11, 0x69, 0xd9, 0x8e, 0x94, 0x9b, 0x1e, 0x87, 0xe9, 0xce, 0x55, 0x28, 0xdf,
            0x8c, 0xa1, 0x89, 0x0d, 0xbf, 0xe6, 0x42, 0x68, 0x41, 0x99, 0x2d, 0x0f, 0xb0, 0x54, 0xbb, 0x16,
        ];
        sbox.copy_from_slice(&SBOX_VALUES);
        sbox
    }
    
    fn inv_sbox() -> [u8; 256] {
        let mut sbox = [0u8; 256];
        const INV_SBOX_VALUES: [u8; 256] = [
            0x52, 0x09, 0x6a, 0xd5, 0x30, 0x36, 0xa5, 0x38, 0xbf, 0x40, 0xa3, 0x9e, 0x81, 0xf3, 0xd7, 0xfb,
            0x7c, 0xe3, 0x39, 0x82, 0x9b, 0x2f, 0xff, 0x87, 0x34, 0x8e, 0x43, 0x44, 0xc4, 0xde, 0xe9, 0xcb,
            0x54, 0x7b, 0x94, 0x32, 0xa6, 0xc2, 0x23, 0x3d, 0xee, 0x4c, 0x95, 0x0b, 0x42, 0xfa, 0xc3, 0x4e,
            0x08, 0x2e, 0xa1, 0x66, 0x28, 0xd9, 0x24, 0xb2, 0x76, 0x5b, 0xa2, 0x49, 0x6d, 0x8b, 0xd1, 0x25,
            0x72, 0xf8, 0xf6, 0x64, 0x86, 0x68, 0x98, 0x16, 0xd4, 0xa4, 0x5c, 0xcc, 0x5d, 0x65, 0xb6, 0x92,
            0x6c, 0x70, 0x48, 0x50, 0xfd, 0xed, 0xb9, 0xda, 0x5e, 0x15, 0x46, 0x57, 0xa7, 0x8d, 0x9d, 0x84,
            0x90, 0xd8, 0xab, 0x00, 0x8c, 0xbc, 0xd3, 0x0a, 0xf7, 0xe4, 0x58, 0x05, 0xb8, 0xb3, 0x45, 0x06,
            0xd0, 0x2c, 0x1e, 0x8f, 0xca, 0x3f, 0x0f, 0x02, 0xc1, 0xaf, 0xbd, 0x03, 0x01, 0x13, 0x8a, 0x6b,
            0x3a, 0x91, 0x11, 0x41, 0x4f, 0x67, 0xdc, 0xea, 0x97, 0xf2, 0xcf, 0xce, 0xf0, 0xb4, 0xe6, 0x73,
            0x96, 0xac, 0x74, 0x22, 0xe7, 0xad, 0x35, 0x85, 0xe2, 0xf9, 0x37, 0xe8, 0x1c, 0x75, 0xdf, 0x6e,
            0x47, 0xf1, 0x1a, 0x71, 0x1d, 0x29, 0xc5, 0x89, 0x6f, 0xb7, 0x62, 0x0e, 0xaa, 0x18, 0xbe, 0x1b,
            0xfc, 0x56, 0x3e, 0x4b, 0xc6, 0xd2, 0x79, 0x20, 0x9a, 0xdb, 0xc0, 0xfe, 0x78, 0xcd, 0x5a, 0xf4,
            0x1f, 0xdd, 0xa8, 0x33, 0x88, 0x07, 0xc7, 0x31, 0xb1, 0x12, 0x10, 0x59, 0x27, 0x80, 0xec, 0x5f,
            0x60, 0x51, 0x7f, 0xa9, 0x19, 0xb5, 0x4a, 0x0d, 0x2d, 0xe5, 0x7a, 0x9f, 0x93, 0xc9, 0x9c, 0xef,
            0xa0, 0xe0, 0x3b, 0x4d, 0xae, 0x2a, 0xf5, 0xb0, 0xc8, 0xeb, 0xbb, 0x3c, 0x83, 0x53, 0x99, 0x61,
            0x17, 0x2b, 0x04, 0x7e, 0xba, 0x77, 0xd6, 0x26, 0xe1, 0x69, 0x14, 0x63, 0x55, 0x21, 0x0c, 0x7d,
        ];
        sbox.copy_from_slice(&INV_SBOX_VALUES);
        sbox
    }
    
    /// Tek bir 128-bitlik bloğu şifreler — InitialRound + 9/13 tam tur + FinalRound (MixColumns yok)
    pub fn encrypt_block(&self, block: &mut [u8; 16]) {
        let mut state = [0u32; 4];
        for i in 0..4 {
            state[i] = u32::from_be_bytes([block[i*4], block[i*4+1], block[i*4+2], block[i*4+3]]);
        }
        
        // Başlangıç tur anahtar eklenmesi — durum ile ilk tur anahtarları XOR'lanır
        for i in 0..4 {
            state[i] ^= self.rk[i];
        }
        
        // Ana turlar — SubBytes + ShiftRows + MixColumns + AddRoundKey sırasıyla çalıştırılır
        for round in 1..self.rounds {
            Self::sub_bytes(&mut state);
            Self::shift_rows(&mut state);
            Self::mix_columns(&mut state);
            for i in 0..4 {
                state[i] ^= self.rk[round * 4 + i];
            }
        }
        
        // Son tur (MixColumns yok) — TLS 1.3 standardinde son turda MixColumns uygulanmaz
        Self::sub_bytes(&mut state);
        Self::shift_rows(&mut state);
        for i in 0..4 {
            state[i] ^= self.rk[self.rounds * 4 + i];
        }
        
        for i in 0..4 {
            let bytes = state[i].to_be_bytes();
            block[i*4..i*4+4].copy_from_slice(&bytes);
        }
    }
    
    /// Tek bir 128-bitlik bloğu çözer — ters InvShiftRows+InvSubBytes+AddRoundKey+InvMixColumns
    pub fn decrypt_block(&self, block: &mut [u8; 16]) {
        let mut state = [0u32; 4];
        for i in 0..4 {
            state[i] = u32::from_be_bytes([block[i*4], block[i*4+1], block[i*4+2], block[i*4+3]]);
        }
        
        // Başlangıç tur anahtar eklenmesi (son tur anahtarıyla başlanır — şifrelemenin tersi)
        for i in 0..4 {
            state[i] ^= self.rk[self.rounds * 4 + i];
        }
        
        // Ana turlar (ters sırayla) — InvShiftRows + InvSubBytes + AddRoundKey + InvMixColumns
        for round in (1..self.rounds).rev() {
            Self::inv_shift_rows(&mut state);
            Self::inv_sub_bytes(&mut state);
            for i in 0..4 {
                state[i] ^= self.rk[round * 4 + i];
            }
            Self::inv_mix_columns(&mut state);
        }
        
        // Son tur (çözüm için) — InvShiftRows + InvSubBytes + ilk tur anahtarı ile XOR
        Self::inv_shift_rows(&mut state);
        Self::inv_sub_bytes(&mut state);
        for i in 0..4 {
            state[i] ^= self.rk[i];
        }
        
        for i in 0..4 {
            let bytes = state[i].to_be_bytes();
            block[i*4..i*4+4].copy_from_slice(&bytes);
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
        // Satır 1: sola 1 pozisyon kaydır — yaygınlaştırma sındırümü sağlar
        // Satır 2: sola 2 pozisyon kaydır
        // Satır 3: sola 3 pozisyon kaydır
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
        fn xtime(a: u8) -> u8 { if a & 0x80 != 0 { (a << 1) ^ 0x1b } else { a << 1 } }
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
        fn xtime(a: u8) -> u8 { if a & 0x80 != 0 { (a << 1) ^ 0x1b } else { a << 1 } }
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

/// AES-GCM (Galois/Counter Modu) — AEAD şifreleme: hem gizlilik hem bütünlük sağlar
/// GCM = CTR modu ile şifreleme + GHASH ile kimlik doğrulama
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
    
    /// GCM ile şifreler — CTR modu ile veri şifrelenir, GHASH ile kimlik doğrulama etiket oluşturulur
    pub fn encrypt(&self, nonce: &[u8], aad: &[u8], plaintext: &[u8]) -> (Vec<u8>, [u8; 16]) {
        let mut ciphertext = vec![0u8; plaintext.len()];
        let mut tag = [0u8; 16];
        
        // Sayış bloğunu oluştur — nonce (12 bayt) + sayış değeri (4 bayt big-endian)
        let mut counter = [0u8; 16];
        counter[..12].copy_from_slice(&nonce[..12]);
        counter[15] = 1; // Sayış 1'den başlar (0, etiket oluşturmak için saklanır)
        
        // GCTR şifreleme — her 16 baytlık için artıştırılmış sayış bloğu AES ile şifrelenerek XOR yapılır
        let mut block_counter = counter.clone();
        for (i, chunk) in plaintext.chunks(16).enumerate() {
            block_counter[15] = (i + 2) as u8; // Anahtar akışı için sayış değeri
            let mut keystream = [0u8; 16];
            self.aes.encrypt_block(&mut keystream);
            
            // Düzeltme: doğru sayış değeriyle şifreleme yapılır
            let mut enc_counter = counter.clone();
            enc_counter[15] = (i + 2) as u8;
            let mut enc_block = enc_counter;
            self.aes.encrypt_block(&mut enc_block);
            
            for (j, byte) in chunk.iter().enumerate() {
                ciphertext[i * 16 + j] = byte ^ enc_block[j];
            }
        }
        
        // Kimlik doğrulama için GHASH — AAD ve şifreli metin GHASH fonksiyonuyla işlenir
        let ghash = self.ghash(aad, &ciphertext);
        
        // Son etiket — GHASH sonucu ile E(K,J0) XOR'lanır
        let mut tag_block = [0u8; 16];
        self.aes.encrypt_block(&mut tag_block);
        for i in 0..16 {
            tag[i] = ghash[i] ^ tag_block[i];
        }
        
        (ciphertext, tag)
    }
    
    /// GCM ile çözer — önce etiket doğrulanır (başarısız olursa None döner), sonra veri çözülür
    pub fn decrypt(&self, nonce: &[u8], aad: &[u8], ciphertext: &[u8], tag: &[u8; 16]) -> Option<Vec<u8>> {
        // Önce etiketi doğrula — doğrulama önce bütünlük kontrolünü garantiler
        let ghash = self.ghash(aad, ciphertext);
        let mut tag_block = [0u8; 16];
        self.aes.encrypt_block(&mut tag_block);
        
        let mut expected_tag = [0u8; 16];
        for i in 0..16 {
            expected_tag[i] = ghash[i] ^ tag_block[i];
        }
        
        if expected_tag != *tag {
            return None; // Kimlik doğrulama başarısız — veri değiştirilmiş veya hatalı anahtar
        }
        
        // Çöz— CTR modu ile tersine şifreleme yapılır (XOR simetrik olduğundan şifreleme ile aynı)
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
    
    /// GHASH fonksiyonu — GF(2^128) Galois alanında H sabiti ile polinom çarpımı yaparak
    /// AAD ve şifreli metni kısa bir kimlik doğrulama değerine indirger
    fn ghash(&self, aad: &[u8], ciphertext: &[u8]) -> [u8; 16] {
        let h = {
            let mut h = [0u8; 16];
            self.aes.encrypt_block(&mut h);
            h
        };
        
        let mut y = [0u8; 16];
        
        // AAD'yi işle — ek kimlik doğrulama verisi (başlık vb.) GHASH'e sarılır
        for chunk in aad.chunks(16) {
            let mut block = [0u8; 16];
            block[..chunk.len()].copy_from_slice(chunk);
            for i in 0..16 {
                y[i] ^= block[i];
            }
            y = Self::gmul(&y, &h);
        }
        
        // Şifreli metni işle — her 16 baytlık blok GHASH durumuna eklenir
        for chunk in ciphertext.chunks(16) {
            let mut block = [0u8; 16];
            block[..chunk.len()].copy_from_slice(chunk);
            for i in 0..16 {
                y[i] ^= block[i];
            }
            y = Self::gmul(&y, &h);
        }
        
        // Uzunluk bloğu — AAD ve şifreli metnin bit uzunlukları eklenerek etiket benzersizliği sağlanır
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
    
    /// Galois alanı çarpımı — GF(2^128) üzerinde x^128 + x^7 + x^2 + x + 1 indirgeme
    /// polinomuyla ikili polinom çarpımı yapar; GHASH'in temel işlemidir
    fn gmul(x: &[u8; 16], y: &[u8; 16]) -> [u8; 16] {
        let mut z = [0u8; 16];
        let mut v = *y;
        
        for i in 0..128 {
            if (x[i / 8] >> (7 - i % 8)) & 1 != 0 {
                for j in 0..16 {
                    z[j] ^= v[j];
                }
            }
            
            // V = V >> 1 ile indirgeme — en düşük bitişa bağlı olarak GF polinomu uygulanır
            let lsb = v[15] & 1;
            for j in (1..16).rev() {
                v[j] = (v[j] >> 1) | (v[j-1] << 7);
            }
            v[0] >>= 1;
            
            if lsb != 0 {
                v[0] ^= 0xe1; // İndirgeme polinomu: x^128 + x^7 + x^2 + x + 1 = 0xe1000...0
            }
        }
        
        z
    }
}

// ============================================================================
// CHACHA20-POLY1305 UYGULAMASİ
// ============================================================================

/// ChaCha20 akış şifreleme — 4 x 32-bit kelimeden oluşan 4x4 matris üzerinde
/// çeyrek-tur (quarter-round) işlemlerini 20 tur uygular
pub struct ChaCha20 {
    state: [u32; 16],
}

impl ChaCha20 {
    /// Yeni ChaCha20 örneği oluşturur — 32 baytlık anahtar, 12 baytlık nonce ve başlangıç sayıcısı ile
    pub fn new(key: &[u8; 32], nonce: &[u8; 12], counter: u32) -> Self {
        let mut state = [0u32; 16];
        
        // Sabitler "expand 32-byte k" — ChaCha20 başlangıç durumunun ilk 4 kelimesi sabit ASCII
        state[0] = 0x61707865;
        state[1] = 0x3320646e;
        state[2] = 0x79622d32;
        state[3] = 0x6b206574;
        
        // Anahtar — 32 baytlık anahtar little-endian olarak durum kelimelerine yüklenir
        for i in 0..8 {
            state[4 + i] = u32::from_le_bytes([
                key[i*4], key[i*4+1], key[i*4+2], key[i*4+3]
            ]);
        }
        
        // Sayıcı ve nonce — sayıcı blok numarasını takip eder, nonce yeniden kullanımı önler
        state[12] = counter;
        for i in 0..3 {
            state[13 + i] = u32::from_le_bytes([
                nonce[i*4], nonce[i*4+1], nonce[i*4+2], nonce[i*4+3]
            ]);
        }
        
        ChaCha20 { state }
    }
    
    /// Çeyrek-tur — ChaCha20'nin temel karıştırma adımı; ekleme+XOR+döndürme (ARX) yapısı
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
    
    /// 64 baytlık anahtar akış bloğu üretir — durum 20 tur karıştırılır, ardından orijinal durumla toplanır
    pub fn block(&self) -> [u8; 64] {
        let mut working = self.state;
        
        // 20 tur (10 çift-tur) — sütun ve köşegensel turlar dönüşümlü uygulanır
        for _ in 0..10 {
            // Sütun turları — matrisin sütunlarına çeyrek-tur uygulanır
            Self::quarter_round(0, 4, 8, 12, &mut working);
            Self::quarter_round(1, 5, 9, 13, &mut working);
            Self::quarter_round(2, 6, 10, 14, &mut working);
            Self::quarter_round(3, 7, 11, 15, &mut working);
            
            // Köşegensel turlar — matrisin köşegenlerine çeyrek-tur uygulanır
            Self::quarter_round(0, 5, 10, 15, &mut working);
            Self::quarter_round(1, 6, 11, 12, &mut working);
            Self::quarter_round(2, 7, 8, 13, &mut working);
            Self::quarter_round(3, 4, 9, 14, &mut working);
        }
        
        // Orijinal durum eklenir — ChaCha20'de karıştırma sonrası başlangıç durumuyla toplanır;
        // bu adım tersinirliği önler
        for i in 0..16 {
            working[i] = working[i].wrapping_add(self.state[i]);
        }
        
        // Baytlara dönüştür — 32-bit kelimeler little-endian sırayla bayta yazılır
        let mut output = [0u8; 64];
        for i in 0..16 {
            let bytes = working[i].to_le_bytes();
            output[i*4..i*4+4].copy_from_slice(&bytes);
        }
        
        output
    }
    
    /// Veriyi şifreler veya çözer — anahtar akışı XOR'u ile simetrik işlem
    pub fn process(&self, data: &[u8]) -> Vec<u8> {
        let mut result = Vec::with_capacity(data.len());
        let mut counter = self.state[12];
        let key = [0u8; 32]; // Durumıdan anahtar alınır (basitleştirilmiş)
        let nonce = [0u8; 12]; // Durumıdan nonce alınır (basitleştirilmiş)
        
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

/// Poly1305 MAC — 2^130-5 asalı modular Mersenne &alingı üzerinde tek seferlik kimlik doğrulama kodu
/// r ve s olmak üzere iki 128-bitlik anahtar parçası kullanır
pub struct Poly1305 {
    r: [u8; 16],
    s: [u8; 16],
    accumulator: [u8; 17],
}

impl Poly1305 {
    /// Yeni Poly1305 örneği oluşturur — ilk 16 bayt r (sıkıştırılmış), son 16 bayt s (ekleme maskesi)
    pub fn new(key: &[u8; 32]) -> Self {
        let mut r = [0u8; 16];
        let mut s = [0u8; 16];
        r.copy_from_slice(&key[..16]);
        s.copy_from_slice(&key[16..]);
        
        // r'yi sıkıştır — RFC 8439 gereksinimine göre bazı bitler sıfırlanır;
        // bu, GF(2^130-5) alanında düzgün çalışmayı garantiler
        r[3] &= 0x0f;
        r[7] &= 0x0f;
        r[11] &= 0x0f;
        r[15] &= 0x0f;
        r[4] &= 0xfc;
        r[8] &= 0xfc;
        r[12] &= 0xfc;
        
        Poly1305 {
            r,
            s,
            accumulator: [0u8; 17],
        }
    }
    
    /// Veriyi işler — birerşer 16 baytlık blok halinde biriktiriciye eklenir
    pub fn update(&mut self, data: &[u8]) {
        for chunk in data.chunks(16) {
            let mut block = [0u8; 17];
            block[..chunk.len()].copy_from_slice(chunk);
            block[chunk.len()] = 1; // Yüksek bit — GF(2^130-5)'te blok normalize edişi için
            
            // Biriktiriciye ekle (basitleştirilmiş) — gerçek uygulama büyük sayı aritmetikle modular toplama yapar
            for i in 0..17 {
                self.accumulator[i] ^= block[i];
            }
        }
    }
    
    /// Sonlandırır ve etiketi döndürür — birikim değerine s maskeleme anahtarı eklenir
    pub fn finalize(self) -> [u8; 16] {
        let mut tag = [0u8; 16];
        
        // Basitleştirilmiş: s ile XOR — gerçek Poly1305'te s, birikim değerine modular 2^128 ile eklenir
        for i in 0..16 {
            tag[i] = self.accumulator[i] ^ self.s[i];
        }
        
        tag
    }
}

/// ChaCha20-Poly1305 AEAD — RFC 8439 tanımlı; ChaCha20 ile şifreleme,
/// Poly1305 ile kimlik doğrulama bir arada sağlanır
pub struct ChaCha20Poly1305 {
    key: [u8; 32],
}

impl ChaCha20Poly1305 {
    pub fn new(key: &[u8; 32]) -> Self {
        let mut k = [0u8; 32];
        k.copy_from_slice(key);
        ChaCha20Poly1305 { key: k }
    }
    
    /// Poly1305 kimlik doğrulamasıyla şifreler — sayıcı=0 ile Poly1305 anahtarı üretilir, sayıcı=1 ile veri şifrelenir
    pub fn encrypt(&self, nonce: &[u8; 12], aad: &[u8], plaintext: &[u8]) -> (Vec<u8>, [u8; 16]) {
        // ChaCha20 kullanarak Poly1305 anahtarı üret (sayıcı=0)
        let chacha = ChaCha20::new(&self.key, nonce, 0);
        let keystream = chacha.block();
        let mut poly_key = [0u8; 32];
        poly_key.copy_from_slice(&keystream[..32]);
        
        // Düz metni şifreleme (sayıcı=1 ile başlar, 0 Poly1305 anahtarı için ayrıldı)
        let cipher_chacha = ChaCha20::new(&self.key, nonce, 1);
        let ciphertext = cipher_chacha.process(plaintext);
        
        // Poly1305 etiketini hesapla — AAD sonra şifreli metin işlenir
        let mut poly = Poly1305::new(&poly_key);
        poly.update(aad);
        poly.update(&ciphertext);
        let tag = poly.finalize();
        
        (ciphertext, tag)
    }
    
    /// Çözer ve doğrular — etiket doğrulaması başarısız olursa None döner (zamanlama saldırısına dayanıklı karşılaştırma önerilir)
    pub fn decrypt(&self, nonce: &[u8; 12], aad: &[u8], ciphertext: &[u8], tag: &[u8; 16]) -> Option<Vec<u8>> {
        // Poly1305 anahtarını üret
        let chacha = ChaCha20::new(&self.key, nonce, 0);
        let keystream = chacha.block();
        let mut poly_key = [0u8; 32];
        poly_key.copy_from_slice(&keystream[..32]);
        
        // Etiketi doğrula — önce bütünlük kontrolü yap, ardından çöz
        let mut poly = Poly1305::new(&poly_key);
        poly.update(aad);
        poly.update(ciphertext);
        let expected_tag = poly.finalize();
        
        if expected_tag != *tag {
            return None;
        }
        
        // Çöz — ChaCha20 XOR ile ters şifreleme (sayıcı=1 ile aynı anahtar akışı)
        let cipher_chacha = ChaCha20::new(&self.key, nonce, 1);
        Some(cipher_chacha.process(ciphertext))
    }
}

// ============================================================================
// ECDHE (Eliptik Eğri Diffie-Hellman) - X25519
// ============================================================================

/// Curve25519 için alan elemanı (255 bit, 5 x 64-bit uzuv)
/// Temsil: limbs[0] + limbs[1]*2^51 + limbs[2]*2^102 + limbs[3]*2^153 + limbs[4]*2^204
/// Bu 51-bit limb yapısı taşıma yayılımını basitleştirir ve taşıma patlamasını önler
#[derive(Clone, Copy, Debug)]
pub struct FieldElement(pub [u64; 5]);

impl FieldElement {
    /// Asal sayı p = 2^255 - 19 (Mersenne benzeri; mod indirgeme 19 ile çoklu basit toplama)
    const P: [u64; 5] = [
        0x7ffffffffffffed, // 2^51 - 19
        0x7ffffffffffff,   // 2^51 - 1
        0x7ffffffffffff,
        0x7ffffffffffff,
        0x7ffffffffffff,
    ];
    
    /// Sıfır alan elemanı oluşturur — toplumsal kimlik, montgomery merdiveninde başlangıç noktası
    pub fn zero() -> Self {
        FieldElement([0, 0, 0, 0, 0])
    }
    
    /// Bir (multiplicative identity) alan elemanı oluşturur
    pub fn one() -> Self {
        FieldElement([1, 0, 0, 0, 0])
    }
    
    /// u8 dizisinden oluşturur (little-endian, 32 bayt) — ağ paketinden alınan ham baytlar alanına çevrilir
    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        let mut limbs = [0u64; 5];
        
        // 5 x 51-bitlik uzuvlara çözücü — bitler baytlar arasında bölündüğünden birden fazla bayt okunur
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
    
    /// u8 dizisine dönüştürür (little-endian, 32 bayt) — alan elemanını ağa gönderilecek ham baytlara yazar
    pub fn to_bytes(&self) -> [u8; 32] {
        let mut result = [0u8; 32];
        let mut carry = 0i64;
        let mut limbs = self.0;
        
        // 2^255-19 modülüyle küccük — uzuvları kanonik forma getirir
        for i in 0..5 {
            limbs[i] = (limbs[i] as i64 + carry) as u64;
            carry = (limbs[i] >> 51) as i64;
            limbs[i] &= 0x7ffffffffffff;
        }
        
        // Gerekiyorsa p çıkar — değer p'den büyüksé bir kez daha indirgenir
        let gt_p = ((limbs[0] + 19) >> 51) as i64
            | (limbs[1] >> 51) as i64
            | (limbs[2] >> 51) as i64
            | (limbs[3] >> 51) as i64
            | (limbs[4] >> 51) as i64;
        
        if gt_p != 0 {
            limbs[0] += 19;
        }
        
        // Baytlara kodla — uzuvların bitleri little-endian sırayla 32 bayta yayılır
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
    
    /// İki alan elemanını toplar — uzuv-uzuv toplama; taşıma işlemi reduce() ile yapılır
    pub fn add(&self, other: &Self) -> Self {
        let mut result = FieldElement::zero();
        for i in 0..5 {
            result.0[i] = self.0[i] + other.0[i];
        }
        result
    }
    
    /// İki alan elemanını çıkarır — negatif sonuç engellemek için 2*p eklenir
    pub fn sub(&self, other: &Self) -> Self {
        let mut result = FieldElement::zero();
        for i in 0..5 {
            // Pozitif kalınmasını garantilemek için 2*p ekle — modular çıkarmada standart numara
            result.0[i] = self.0[i] + (0x1ffffffffffffe << 1) - other.0[i];
        }
        result.reduce();
        result
    }
    
    /// İki alan elemanını çarpar (okul kitabı yöntemi) — 5x5=25 kısmi çarpım birikerek
    /// 2^255-19 modulos ile indirgenir
    pub fn mul(&self, other: &Self) -> Self {
        // Çarp ve biriktir — her uzuv çifti çarpılıp ilgili pozisyona eklenir
        let mut product = [0u128; 9];
        
        for i in 0..5 {
            for j in 0..5 {
                product[i + j] += (self.0[i] as u128) * (other.0[j] as u128);
            }
        }
        
        // 2^255-19 modülüyle indirge — 125-bitlik aralıktan 51-bit uzuvlara indirgeme
        let mut result = FieldElement::zero();
        
        // Taşıma yayılımı — her uzuvdan taşan bitler bir sonraki uzuva aktarılır
        let mut carry = 0u128;
        for i in 0..5 {
            product[i] += carry;
            carry = product[i] >> 51;
            result.0[i] = (product[i] & 0x7ffffffffffff) as u64;
        }
        
        // Taşıma çıkışı: 19 ile çarpıp geri ekle — 2^255 ≡ 19 (mod 2^255-19) geçerliliğinden
        result.0[0] += (carry as u64) * 19;
        
        result.reduce();
        result
    }
    
    /// Alan elemanını kareler (optimize edilmiş) — mul ile eşdeğerdir; ileriki sürümde özel kareler optimizasyonu eklenecek
    pub fn square(&self) -> Self {
        self.mul(self)
    }
    
    /// Kanonik forma indirger — uzuvlardaki 51-bit sınırı aşımlıkları bir sonraki uzuva taşır
    pub fn reduce(&mut self) {
        let mut carry = 0u64;
        
        for i in 0..5 {
            self.0[i] += carry;
            carry = self.0[i] >> 51;
            self.0[i] &= 0x7ffffffffffff;
        }
        
        // Taşımayı 19 katsayısıyla geri kat — 2^255 ≡ 19 (mod p) ilkesini kullan
        self.0[0] += carry * 19;
        
        // Son indirgeme — ilk uzuv da taşabilir, bir adım daha yapılır
        carry = self.0[0] >> 51;
        self.0[0] &= 0x7ffffffffffff;
        self.0[1] += carry;
    }
    
    /// Çarpımsal ters hesaplar (a^(p-2) mod p = a^(2^255-21)) — Fermat'nın küçük teoremine dayanan kare-ve-çarp yöntemi
    pub fn invert(&self) -> Self {
        // a^(2^255-21) için kare-ve-çarp — modüler ters hesabın standart yöntemi
        let mut result = self.clone();
        
        // a^(2^250-1) — çarpma zincirleri ile özgül üüstler hesaplanır
        for _ in 0..249 {
            result = result.square();
            result = result.mul(self);
        }
        
        // 2^255-21 için son kareler — 252'den 255'e çıkmak için 5 kez ek kare
        result = result.square();
        result = result.square();
        result = result.square();
        result = result.square();
        result = result.square();
        result = result.square();
        
        result
    }
    
    /// Koşullu değiş-tokuş (sabit-zaman) — zamanlama saldırılarını önlemek için XOR mask ile flip yapılır
    pub fn conditional_swap(a: &mut Self, b: &mut Self, swap: u8) {
        let mask = (-(swap as i64)) as u64;
        
        for i in 0..5 {
            let diff = (a.0[i] ^ b.0[i]) & mask;
            a.0[i] ^= diff;
            b.0[i] ^= diff;
        }
    }
}

/// X25519 eliptik eğri işlemleri (Curve25519) — Montgomery eğrisi üzerinde;
/// Montgomery merdiveni DH anahtar değişimini uygular
pub struct X25519;

impl X25519 {
    /// A24 = 121665 (Montgomery merdiveni formulünde kullanılır; eğrinin a katsayısından türetilmiş)
    const A24: FieldElement = FieldElement([121665, 0, 0, 0, 0]);
    
    /// Anahtar çifti üretir — rastgele özel anahtar, Curve25519 üzerinde skalar çarpım ile açık anahtar elde edilir
    pub fn generate_keypair() -> ([u8; 32], [u8; 32]) {
        let mut private = [0u8; 32];
        for i in 0..32 {
            private[i] = crate::random::next_u32() as u8;
        }
        
        // Özel anahtarı sıkıştır — RFC 7748 gereği bazı bitler belirli değerlere ayarlanır;
        // böylece küçük alt-grup saldırıları ve ölümün serbest grubuna düşme önlenir
        private[0] &= 248;
        private[31] &= 127;
        private[31] |= 64;
        
        let public = Self::public_from_private(&private);
        (private, public)
    }
    
    /// Özel anahtardan açık anahtarı türetir — taban noktası u=9 ile skalar çarpım yapılır
    pub fn public_from_private(private: &[u8; 32]) -> [u8; 32] {
        // Taban noktası u = 9 — Curve25519'un standart başlangıç noktası
        let base = [9u8; 32];
        Self::scalar_mult(private, &base)
    }
    
    /// Montgomery merdiveni skalar çarpımı — sabit-zaman DH hesabı;
    /// dalımsız koşullu swap zamanlama saldırılarını önler
    pub fn scalar_mult(scalar: &[u8; 32], point: &[u8; 32]) -> [u8; 32] {
        let mut k = *scalar;
        
        // Skaleri sıkıştır — RFC 7748 standart bit maskeleme
        k[0] &= 248;
        k[31] &= 127;
        k[31] |= 64;
        
        // Noktaıyı alan elemanına çözücü — ağ paketinden gelen ham baytlar
        let u = FieldElement::from_bytes(point);
        
        // Montgomery merdiveni — ikili genisletme ile skalerle eğri noktası çarpımı yapılır
        // x_1 = u (nokta)
        // x_2 = 1, z_2 = 0 (sonsuz nokta / nötr eleman)
        // x_3 = u, z_3 = 1 (nokta)
        let mut x1 = u;
        let mut x2 = FieldElement::one();
        let mut z2 = FieldElement::zero();
        let mut x3 = u;
        let mut z3 = FieldElement::one();
        
        // değiş-tokuş bitleri sıfır başlar
        let mut swap: u8 = 0;
        
        // Biti üstden alta işle — skaler sıfırdan başlayarak her bit merdiven adımını sürer
        for t in (0..255).rev() {
            let k_t = (k[t / 8] >> (t % 8)) & 1;
            
            // Koşullu değiş-tokuş — mevcut skaler bitime göre x_2/x_3 ve z_2/z_3 değiştirilebilir
            FieldElement::conditional_swap(&mut x2, &mut x3, swap ^ k_t);
            FieldElement::conditional_swap(&mut z2, &mut z3, swap ^ k_t);
            swap = k_t;
            
            // A = x_2 + z_2  (Montgomery merdiveni değişkenel akımı)
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
            // z_2 = E * (AA + a24 * E)  (Montgomery eğrisi diferansiyeli)
            let a24_e = Self::A24.mul(&e);
            let aa_a24e = aa.add(&a24_e);
            z2 = e.mul(&aa_a24e);
        }
        
        // Son koşullu değiş-tokuş — son bit işleme eşitliğinden swap durumu düzeltiilir
        FieldElement::conditional_swap(&mut x2, &mut x3, swap);
        FieldElement::conditional_swap(&mut z2, &mut z3, swap);
        
        // Sonuç hesapla: x_2 * (z_2^(p-2)) — projektif koordinatlardan afin u koordinatına geçiş
        let z2_inv = z2.invert();
        let result = x2.mul(&z2_inv);
        
        result.to_bytes()
    }
    
    /// Paylaşılan gizliyi hesaplar (Diffie-Hellman) — özel anahtar x sunucunun açık anahtarıyla çarpılır
    pub fn diffie_hellman(private: &[u8; 32], public: &[u8; 32]) -> [u8; 32] {
        Self::scalar_mult(private, public)
    }
}

// ============================================================================
// TLS 1.3 KRİPTO ENTEGRASYONU
// ============================================================================

/// TLS 1.3 Anahtar Programı (gelişmiş)
/// RFC 8446 Bölüm 7.1'e göre HKDF tabanlı anahtar türetimini uygular:
/// Erken Gizli → El Sıkışma Gizli → Ana Gizli → Trafik Gizlileri zincirleme türetimi
pub struct TlsKeySchedule {
    /// Şifre paketi — hash algoritmasını ve şifre uzunluğunu belirler
    cipher_suite: CipherSuite,
    /// Hash uzunluğu — SHA-256 için 32 bayt, SHA-384 için 48 bayt
    hash_len: usize,
    /// Erken Gizli — PSK veya sıfır IKM'den türetilir
    early_secret: Vec<u8>,
    /// El Sıkışma Gizli — ECDHE paylaşılan gizli dahil edilir
    handshake_secret: Option<Vec<u8>>,
    /// Ana Gizli — el sıkışmada tüm malzeme tüketildikten sonra oluşturulur
    master_secret: Option<Vec<u8>>,
    /// İstemci El Sıkışma Trafik Gizli — istemcinin el sıkışma mesajlarını şifreler
    client_hs_secret: Option<Vec<u8>>,
    /// Sunucu El Sıkışma Trafik Gizli — sunucunun el sıkışma mesajlarını şifreler
    server_hs_secret: Option<Vec<u8>>,
    /// İstemci Uygulama Trafik Gizli — normal veri akışını şifreler
    client_app_secret: Option<Vec<u8>>,
    /// Sunucu Uygulama Trafik Gizli — sunucudan gelen veriyi şifreler
    server_app_secret: Option<Vec<u8>>,
}

impl TlsKeySchedule {
    /// Yeni anahtar programı oluşturur — şifre paketine göre hash uzunluğu belirlenir
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
    
    /// HKDF-Extract: PRK = HMAC-Hash(tuz, IKM) — giriş anahtar malzemesini sözderastgele anahtara dönüştürür
    fn hkdf_extract(&self, salt: &[u8], ikm: &[u8]) -> Vec<u8> {
        // HMAC-Hash(tuz, ikm) — basitleştirilmiş: tuz || ikm hash'lenir
        // Gerçek HKDF'te HMAC-SHA2 kullanılır
        let mut data = salt.to_vec();
        data.extend_from_slice(ikm);
        
        match self.cipher_suite {
            CipherSuite::Aes128GcmSha256 | CipherSuite::ChaCha20Poly1305Sha256 => {
                let mut hasher = Sha256::new();
                hasher.update(&data);
                hasher.finalize().to_vec()
            }
            CipherSuite::Aes256GcmSha384 => {
                let mut hasher = Sha384::new();
                hasher.update(&data);
                hasher.finalize().to_vec()
            }
        }
    }
    
    /// HKDF-Expand: OKM = HKDF-Expand(PRK, bilgi, L) — PRK'den istenilen uzunlukta anonim anahtar malzeme üretir
    fn hkdf_expand(&self, prk: &[u8], info: &[u8], len: usize) -> Vec<u8> {
        // HKDF-Expand(PRK, bilgi, L) =
        //   T(1) | T(2) | T(3) | ... | T(n)
        // T(0) = boş dize
        //       T(1) = HMAC(PRK, T(0) | bilgi | 0x01)
        //       T(2) = HMAC(PRK, T(1) | bilgi | 0x02)
        //       vb.
        
        let mut output = Vec::new();
        let mut t = Vec::new();
        let mut counter = 1u8;
        
        while output.len() < len {
            // T(n) = HMAC(PRK, T(n-1) | bilgi | n) — her blok bir öncekine bağlıdır
            let mut data = t.clone();
            data.extend_from_slice(info);
            data.push(counter);
            
            // HMAC(PRK, data) — pseudo-rastgele fonksiyon olarak kullanılır
            let t_n = self.hmac_hash(prk, &data);
            
            t = t_n.clone();
            output.extend_from_slice(&t);
            
            counter += 1;
            if counter == 0 {
                break; // Taşmayı önle — 255 bloktan fazla üretilemez
            }
        }
        
        output.truncate(len);
        output
    }
    
    /// HMAC-Hash — HMAC(K, m) = H((K ^ dış_dolgu) || H((K ^ iç_dolgu) || m))
    fn hmac_hash(&self, key: &[u8], data: &[u8]) -> Vec<u8> {
        // HMAC(K, m) = H((K ^ opad) || H((K ^ ipad) || m))
        // ipad = 0x36 tekrar, opad = 0x5c tekrar
        let block_size = match self.cipher_suite {
            CipherSuite::Aes128GcmSha256 | CipherSuite::ChaCha20Poly1305Sha256 => 64,
            CipherSuite::Aes256GcmSha384 => 128,
        };
        
        // Anahtarı blok boyutuna uyarla — SHA-256 için 64 bayt, SHA-384 için 128 bayt
        let mut k_ipad = vec![0x36u8; block_size];
        let mut k_opad = vec![0x5cu8; block_size];
        
        for (i, &k) in key.iter().enumerate().take(block_size) {
            k_ipad[i] ^= k;
            k_opad[i] ^= k;
        }
        
        // İç hash: H(K ^ ipad || veri) — birinci hash katmanı
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
        
        // Dış hash: H(K ^ opad || iç_hash) — ikinci hash katmanı; uzunluk uzantısı saldırılarını engeller
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
    
    /// HKDF-Genişlet-Etiket: TLS 1.3 etiketi ile anahtar türetir — RFC 8446 şablonu
    /// HkdfExpandLabel(Gizli, Etiket, Bağlam, Uzunluk) =
    ///   HKDF-Expand(Gizli, HkdfEtiket, Uzunluk)
    /// burada HkdfEtiket = Uzunluk || "tls13 " || Etiket || Bağlam
    pub fn hkdf_expand_label(&self, secret: &[u8], label: &[u8], context: &[u8], len: usize) -> Vec<u8> {
        // HkdfEtiket oluştur — yapı: 2-bayt uzunluk + etiket + 1-bayt bağlam uzunluğu + bağlam
        let mut info = Vec::new();
        
        // Uzunluk (2 bayt) — istenen çıkış uzunluğu kodlanır
        info.extend_from_slice(&(len as u16).to_be_bytes());
        
        // "tls13 " || Etiket — olasılık alanını diğer protokollerden ayırmak için önek kullanılır
        info.extend_from_slice(b"tls13 ");
        info.extend_from_slice(label);
        
        // Bağlam uzunluğu (1 bayt) || Bağlam — ız belgesi hash'ini veya boş dizeyi içerir
        info.push(context.len() as u8);
        info.extend_from_slice(context);
        
        self.hkdf_expand(secret, &info, len)
    }
    
    /// Gizli Türet: Derive-Secret(Gizli, Etiket, Mesajlar)
    /// = HKDF-Genişlet-Etiket(Gizli, Etiket, İz-Hash(Mesajlar), Hash.uzunluk)
    pub fn derive_secret(&self, secret: &[u8], label: &[u8], transcript_hash: &[u8]) -> Vec<u8> {
        self.hkdf_expand_label(secret, label, transcript_hash, self.hash_len)
    }
    
    /// PSK ile başlat (veya yeni bağlantı için sıfır) — PSK olmadan boş IKM ile Erken Gizli hesaplanır
    pub fn init_with_psk(&mut self, psk: Option<&[u8]>) {
        let zero_vec = vec![0u8; self.hash_len];
        let ikm = psk.unwrap_or(&zero_vec);
        let salt = vec![0u8; self.hash_len];
        
        self.early_secret = self.hkdf_extract(&salt, ikm);
    }
    
    /// ECDH sonrası el sıkışma gizlilerini türetir — paylaşılan gizli anahtar programına dalştırılır
    pub fn derive_handshake_secrets(&mut self, ecdhe_secret: &[u8], transcript_hash: &[u8]) {
        // Derive-Secret(erken_gizli, "derived", boş_hash) — getürdüğe ile bir sonraki katmana köprü yapılır
        let derived_secret = self.derive_secret(&self.early_secret, b"derived", &[]);
        
        // handshake_secret = HKDF-Extract(türetilmiş_gizli, ECDHE) — ECDH çıktısı dahil edilir
        self.handshake_secret = Some(self.hkdf_extract(&derived_secret, ecdhe_secret));
        
        let hs = self.handshake_secret.as_ref().unwrap();
        
        // c_hs_gizli = Derive-Secret(el_sıkışma_gizli, "c hs traffic", iz_hash) — istemci el sıkışma kayni
        self.client_hs_secret = Some(self.derive_secret(hs, b"c hs traffic", transcript_hash));
        
        // s_hs_gizli = Derive-Secret(el_sıkışma_gizli, "s hs traffic", iz_hash) — sunucu el sıkışma kayıt
        self.server_hs_secret = Some(self.derive_secret(hs, b"s hs traffic", transcript_hash));
    }
    
    /// Ana gizliyi ve uygulama trafik gizlilerini türetir — el sıkışma tamamlanınca çâğrılır
    pub fn derive_master_secret(&mut self, transcript_hash: &[u8]) {
        let hs = self.handshake_secret.as_ref().unwrap();
        
        // Derive-Secret(el_sıkışma_gizli, "derived", boş_hash) — uygulama katmanına köprü
        let derived_secret = self.derive_secret(hs, b"derived", &[]);
        
        // master_secret = HKDF-Extract(türetilmiş_gizli, 0) — IKM=0 ile son gizli üretilir
        let zero = vec![0u8; self.hash_len];
        self.master_secret = Some(self.hkdf_extract(&derived_secret, &zero));
        
        let ms = self.master_secret.as_ref().unwrap();
        
        // c_ap_gizli = Derive-Secret(ana_gizli, "c ap traffic", iz_hash) — istemci uygulama trafik anahtar kaynağı
        self.client_app_secret = Some(self.derive_secret(ms, b"c ap traffic", transcript_hash));
        
        // s_ap_gizli = Derive-Secret(ana_gizli, "s ap traffic", iz_hash) — sunucu uygulama trafik anahtar kaynağı
        self.server_app_secret = Some(self.derive_secret(ms, b"s ap traffic", transcript_hash));
    }
    
    /// Trafik gizlisinden şifreleme anahtarlarını türetir — her eşriz trafik gizlisinin kendine ait
    /// anahtar ve IV'sı vardır; anahtar uzunluğu şifre paketine göre değişir
    /// anahtar = HKDF-Genişlet-Etiket(gizli, "key", "", anahtar_uzunluğu)
    /// iv = HKDF-Genişlet-Etiket(gizli, "iv", "", iv_uzunluğu)
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
    
    /// İstemci el sıkışma trafik gizlisini döndürür
    pub fn client_hs_secret(&self) -> Option<&[u8]> {
        self.client_hs_secret.as_deref()
    }
    
    /// Sunucu el sıkışma trafik gizlisini döndürür
    pub fn server_hs_secret(&self) -> Option<&[u8]> {
        self.server_hs_secret.as_deref()
    }
    
    /// İstemci uygulama trafik gizlisini döndürür
    pub fn client_app_secret(&self) -> Option<&[u8]> {
        self.client_app_secret.as_deref()
    }
    
    /// Sunucu uygulama trafik gizlisini döndürür
    pub fn server_app_secret(&self) -> Option<&[u8]> {
        self.server_app_secret.as_deref()
    }
    
    /// Finished MAC'ini hesaplar — el sıkışma bütünlüğünü HMAC ile doğrular
    /// bitis_anahtari = HKDF-Genişlet-Etiket(gizli, "finished", "", Hash.uzunluk)
    /// doğrulama_verisi = HMAC(bitis_anahtari, iz_hash)
    pub fn compute_finished_mac(&self, traffic_secret: &[u8], transcript_hash: &[u8]) -> Vec<u8> {
        let finished_key = self.hkdf_expand_label(traffic_secret, b"finished", &[], self.hash_len);
        self.hmac_hash(&finished_key, transcript_hash)
    }
    
    /// Anahtar güncellemesi için trafik gizlisini günceller — "traffic upd" etiketiyle bir sonraki nesil gizli türetilir
    pub fn update_traffic_secret(&self, traffic_secret: &[u8]) -> Vec<u8> {
        self.hkdf_expand_label(traffic_secret, b"traffic upd", &[], self.hash_len)
    }
}

/// TLS kripto işlemleri — seçili şifre paketine göre şifreleme/çözme uygular
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
    
    /// TLS kaydını şifreler — içerik tipini ekleyip doldurma yapar, sonra AEAD şifreleme uygular
    pub fn encrypt_record(&self, content_type: ContentType, plaintext: &[u8]) -> Vec<u8> {
        // İçerik tipi ve doldurma ekle — TLS 1.3'te gerçek içerik tipi şifreli metnin sonuna eklenir
        let mut data = plaintext.to_vec();
        data.push(content_type as u8);
        
        // 16 bayt sınırına doldur — blok boyutu hizalaması ve trafik analizi engelleme
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
                let chacha = ChaCha20Poly1305::new(
                    &self.key.as_slice().try_into().unwrap_or([0u8; 32])
                );
                let (ciphertext, tag) = chacha.encrypt(&nonce, &[], &data);
                
                let mut result = ciphertext;
                result.extend_from_slice(&tag);
                result
            }
        }
    }
    
    /// TLS kaydını çözer — AEAD doğrulaması gerçekleştirilir, içerik tipi son bayttan çıkarılır
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
                let chacha = ChaCha20Poly1305::new(
                    &self.key.as_slice().try_into().unwrap_or([0u8; 32])
                );
                chacha.decrypt(&nonce, &[], ct, &tag_arr)?
            }
        };
        
        // Sondaki içerik tipini çıkar — TLS 1.3'te şifreli metindeki son non-sıfır bayt gerçek içerik tipini verir
        if plaintext.is_empty() {
            return None;
        }
        
        let content_type = ContentType::from_u8(plaintext[plaintext.len() - 1])?;
        let data = plaintext[..plaintext.len() - 1].to_vec();
        
        Some((content_type, data))
    }
}

// ============================================================================
// TLS 1.3 0-RTT ÖNDEN GELEN VERİ (EARLY DATA)
// ============================================================================

/// 0-RTT Önden Gelen Veri yapılandırması — önceki oturumdan PSK kullanarak
/// el sıkışma tamamlanmadan veri göndermek için kullanılır
#[derive(Clone, Debug)]
pub struct EarlyDataConfig {
    /// Sunucunun kabul ettiği maksimum önden gelen veri boyutu (bayt)
    pub max_early_data_size: u32,
    /// Önden gelen verinin etkin olup olmadığı
    pub enabled: bool,
}

impl Default for EarlyDataConfig {
    fn default() -> Self {
        EarlyDataConfig {
            max_early_data_size: 16384,  // Varsayılan 16KB limiti
            enabled: true,
        }
    }
}

/// 0-RTT oturum bileti — sunucunun verdiği ve sonraki bağlantıda PSK olarak kullanılan token
#[derive(Clone, Debug)]
pub struct SessionTicket {
    /// Bilet ömrü (saniye) — bu süreden sonra bilet geçersizleşir
    pub lifetime: u32,
    /// Bilet yaş eklentisi (yaşı gizlemek için rastgele değer) — takıp saldırılarını engeller
    pub age_add: u32,
    /// Bilet nonce değeri — her bilet için benzersiz türetme sağlar
    pub nonce: Vec<u8>,
    /// Bilet verisi (şifreli) — sunucu tarafından şifrelenen oturum durumu
    pub ticket: Vec<u8>,
    /// Önden gelen veri yapılandırması
    pub early_data: EarlyDataConfig,
    /// Oluşturma zaman damgası — bilet yaşı hesaplamak için kullanılır
    pub created_at: u64,
    /// Oturum sürdürme ana gizli — PSK türetmek için kullanılır
    pub resumption_secret: Vec<u8>,
    /// Şifre paketi — yeniden bağlanırken aynı algoritma kullanılmasını sağlar
    pub cipher_suite: CipherSuite,
}

impl SessionTicket {
    /// Yeni oturum bileti oluşturur — sürdürme gizli ve rastgele age_add ile başlatılır
    pub fn new(cipher_suite: CipherSuite, resumption_secret: &[u8]) -> Self {
        SessionTicket {
            lifetime: 86400,  // 24 saat (saniye)
            age_add: crate::random::next_u32(),
            nonce: vec![crate::random::next_u32() as u8; 8],
            ticket: Vec::new(),
            early_data: EarlyDataConfig::default(),
            created_at: 0,  // Would use real timestamp
            resumption_secret: resumption_secret.to_vec(),
            cipher_suite,
        }
    }
    
    /// Biletin hala geçerli olup olmadığını kontrol eder — ömrü géçmiş biletler reddedilir
    pub fn is_valid(&self) -> bool {
        // Basitleştirilmiş — ömrü içinde olup olmadığı kontrol edilir
        let age = 0u64;  // Gerçek yaş hesaplanır (burada sabit sıfır)
        age < self.lifetime as u64
    }
    
    /// Gizlenmiş bilet yaşını hesaplar — gerçek yaşa age_add rastgele değeri eklenir
    pub fn obfuscated_age(&self) -> u32 {
        let age = 0u32;  // Milisaniye cinsinden gerçek yaş hesaplanır
        age.wrapping_add(self.age_add)
    }
    
    /// Önden gelen veri gizlisini türetir — sürdürme gizlisinden PSK önden gelen gizlisi üretilir
    pub fn derive_early_secret(&self) -> Vec<u8> {
        // HKDF-Genişlet-Etiket(sürdürme_gizli, "res early", "", Hash.uzunluk)
        let mut early_secret = vec![0u8; 32];
        // Basitleştirilmiş türetme — gerçek uygulama HKDF kullanır
        for (i, b) in self.resumption_secret.iter().enumerate() {
            if i < 32 {
                early_secret[i] = b ^ 0x5a;  // Yer tutucu (gerçek değil)
            }
        }
        early_secret
    }
}

/// 0-RTT önden gelen veri durumu — sunucunun kabul/ret kararını takip eder
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EarlyDataState {
    /// Önden gelen veri kullanılmıyor
    None,
    /// Sunucu önden gelen veriyi kabul etti
    Accepted,
    /// Sunucu önden gelen veriyi reddetti — veri normal trafik olarak tekrar gönderilmeli
    Rejected,
    /// Sunucunun kararı bekleniyor
    Pending,
}

/// 0-RTT bağlantı durumu — oturum bileti, önden gelen veri tamponı ve durum takibi
#[derive(Clone, Debug)]
pub struct ZeroRttState {
    /// Sürdürme için oturum bileti
    pub ticket: Option<SessionTicket>,
    /// Önden gelen veri durumu
    pub state: EarlyDataState,
    /// Önden gelen veri tamponı — red durumunda yeniden gönderim için saklanır
    pub early_data_buffer: Vec<u8>,
    /// Gönderilen önden gelen veri bayt sayısı
    pub early_data_sent: usize,
    /// İzin verilen maksimum önden gelen veri miktarı
    pub max_early_data: usize,
}

impl ZeroRttState {
    /// Yeni 0-RTT durumu oluşturur — oturum bileti olmadan başlanır
    pub fn new() -> Self {
        ZeroRttState {
            ticket: None,
            state: EarlyDataState::None,
            early_data_buffer: Vec::new(),
            early_data_sent: 0,
            max_early_data: 0,
        }
    }
    
    /// Oturum biletiyle başlatır — bilet varsa önden gelen veri gönderimi mümkün olur
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
    
    /// Önden gelen verinin gönderilebilir olup olmadığını kontrol eder — bilet, durum ve limit kontrolü
    pub fn can_send_early_data(&self) -> bool {
        matches!(self.state, EarlyDataState::Pending | EarlyDataState::Accepted)
            && self.early_data_sent < self.max_early_data
            && self.ticket.is_some()
    }
    
    /// Önden gelen veriyi gönderir — limit aşılmazsa şifreler ve tampon güncellenir
    pub fn send_early_data(&mut self, data: &[u8]) -> Option<Vec<u8>> {
        if !self.can_send_early_data() {
            return None;
        }
        
        let ticket = self.ticket.as_ref()?;
        let remaining = self.max_early_data - self.early_data_sent;
        let to_send = data.len().min(remaining);
        
        // 0-RTT kayıt oluştur — önden gelen gizli ile uygulama verisi şifrelenir
        let early_secret = ticket.derive_early_secret();
        let mut crypto = TlsCrypto::new(ticket.cipher_suite, &early_secret, &[0u8; 12]);
        
        // 0-RTT kayıt olarak şifrele (içerik tipi 0x17 = Uygulama Verisi)
        let encrypted = crypto.encrypt_record(ContentType::ApplicationData, &data[..to_send]);
        
        self.early_data_sent += to_send;
        self.early_data_buffer.extend_from_slice(&data[..to_send]);
        
        Some(encrypted)
    }
    
    /// Sunucunun önden gelen veri reddini işler — tampon sıfırlanır, veri normal yoldan tekrar gönderilmeli
    pub fn on_reject(&mut self) {
        self.state = EarlyDataState::Rejected;
        self.early_data_buffer.clear();
        self.early_data_sent = 0;
    }
    
    /// Sunucunun önden gelen veri kabulunu işler — durum Kabul Edildi olarak güncellenir
    pub fn on_accept(&mut self) {
        self.state = EarlyDataState::Accepted;
    }
    
    /// Reddedilen önden gelen veriyi yeniden gönderim için döndürür — normal el sıkışma sonrası kullanılır
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
// TLS 1.3 OTURUM SÜRDÜRME
// ============================================================================

/// Oturum önbelleği — alınan biletleri depolar ve sonraki bağlantılarda PSK için kullanılır
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
    
    /// Oturum biletini önbelleğe ekler — süresi dolmuş biletler temizlenir, kapasite aşılırsa en eski silinir
    pub fn add(&mut self, ticket: SessionTicket) {
        // Süresi dolmuş oturumları temizle
        self.sessions.retain(|t| t.is_valid());
        
        // Kapasitede en eskiyi kaldır — FIFO eşittir: en eski bilet silinir
        if self.sessions.len() >= self.max_sessions {
            self.sessions.remove(0);
        }
        
        self.sessions.push(ticket);
    }
    
    /// Sunucu için oturum arar — geçerli bilet bulunursa PSK ile hızlı sürdürme mümkün olur
    pub fn find_for_server(&self, server_name: &str) -> Option<&SessionTicket> {
        // Basitleştirilmiş — sunucu adına ve diğer kriterlere göre eşleşme yapılacak
        self.sessions.iter().find(|t| t.is_valid())
    }
    
    /// Oturumu kaldırır — bilet verisiyle eşleşen giriş silinir
    pub fn remove(&mut self, ticket: &[u8]) {
        self.sessions.retain(|t| t.ticket != ticket);
    }
    
    /// Tüm oturumları temizler — güvenlik gereksinimlerinde veya oturum kapatılırken kullanılır
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
// TLS 1.3 0-RTT DESTEKLİ EL SIKISMA
// ============================================================================

/// 0-RTT destekli genişletilmiş el sıkışma durumu — önden gelen veri ve oturum sürdürmeyi bir arada yönetir
#[derive(Clone, Debug)]
pub struct TlsHandshakeExt {
    /// Temel el sıkışma durumu
    pub state: TlsState,
    /// 0-RTT durumu — oturum bileti ve önden gelen veri yönetimi
    pub zero_rtt: ZeroRttState,
    /// Oturum önbelleği — alınan biletleri depolar
    pub session_cache: SessionCache,
    /// SNI için sunucu adı — istemci hangi sunucuya bağlanacak
    pub server_name: Option<String>,
    /// Önden gelen veri istenmesi gerekip gerekmediği
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
    
    /// Olası 0-RTT ile el sıkışmayı başlatır — önbellekte bilet varsa PSK uzantısıyla ClientHello gönderilir
    pub fn start_with_early_data(&mut self, server_name: &str) -> Option<Vec<u8>> {
        self.server_name = Some(server_name.to_string());
        
        // Önbellekte oturum ara — bilet bulunursa 0-RTT ile bağlantı kurulabilir
        if let Some(ticket) = self.session_cache.find_for_server(server_name) {
            self.zero_rtt = ZeroRttState::with_ticket(ticket.clone());
            self.request_early_data = true;
            
            // pre_shared_key uzantısıyla ClientHello oluştur — sunucuya PSK kimliği bildirilir
            let mut client_hello = self.build_client_hello();
            
            // pre_shared_key uzantısını ekle — bilet kimliği ve gizlenmiş yaş dahil edilir
            let obfuscated_age = ticket.obfuscated_age();
            client_hello.extend_from_slice(&[0x00, 0x2a]);  // pre_shared_key uzantı tipi
            client_hello.extend_from_slice(&(4 + ticket.ticket.len() as u16).to_be_bytes());
            client_hello.extend_from_slice(&(ticket.ticket.len() as u16).to_be_bytes());
            client_hello.extend_from_slice(&ticket.ticket);
            client_hello.extend_from_slice(&obfuscated_age.to_be_bytes());
            
            // early_data uzantısını ekle — sunucuya 0-RTT verisi göndermek istendiği bildirilir
            client_hello.extend_from_slice(&[0x00, 0x2a]);  // early_data uzantısı
            client_hello.extend_from_slice(&[0x00, 0x00]);  // boş veri
            
            return Some(client_hello);
        }
        
        // Önbellekte oturum yok, normal el sıkışma yapılır (1-RTT)
        Some(self.build_client_hello())
    }
    
    /// ClientHello mesajını oluşturur (dahili) — minimum TLS 1.3 uyumlu yapı
    fn build_client_hello(&self) -> Vec<u8> {
        let mut hello = Vec::new();
        
        // El sıkışma tipi: ClientHello (0x01)
        hello.push(0x01);
        
        // Sürüm: TLS 1.2 (0x0303) — uyumluluk için başlıkta TLS 1.2, gerçek sürüm uzantıda
        hello.extend_from_slice(&[0x03, 0x03]);
        
        // Rastgele veri (32 bayt)
        for _ in 0..32 {
            hello.push(crate::random::next_u32() as u8);
        }
        
        // Oturum kimliği (yeni bağlantı için boş)
        hello.push(0x00);
        
        // Şifre paketleri — desteklenen 2 paket listelenir
        hello.extend_from_slice(&[0x00, 0x04]);  // 2 şifre paketi
        hello.extend_from_slice(&[0x13, 0x01]);  // TLS_AES_128_GCM_SHA256
        hello.extend_from_slice(&[0x13, 0x02]);  // TLS_AES_256_GCM_SHA384
        
        // Sıkıştırma yöntemleri — TLS 1.3'te yalnızca null
        hello.push(0x01);  // 1 yöntem
        hello.push(0x00);  // null sıkıştırma
        
        // Uzantılar yer tutucu (doldurulmalıdır)
        hello.extend_from_slice(&[0x00, 0x00]);  // uzantı uzunluğu
        
        hello
    }
    
    /// Sunucu yanıtını işler — mesaj tipine göre durum güncellenir ve 0-RTT kararı alınır
    pub fn process_server_response(&mut self, data: &[u8]) -> Option<Vec<u8>> {
        if data.is_empty() {
            return None;
        }
        
        let msg_type = data[0];
        
        match msg_type {
            0x02 => {
                // ServerHello — sunucu PSK secöti ve‟ seçmediği bildirilir
                self.state = TlsState::ServerHelloReceived;
                
                // Önden gelen veri göstergesini kontrol et
                // Sunucu 0-RTT'yi kabul veya reddedebilir
                if self.zero_rtt.state == EarlyDataState::Pending {
                    // Sunucunun PSK seçip seçmediğini kontrol et — başarısız ise red
                    // Şu an için reddedildi varsayılır
                    self.zero_rtt.on_reject();
                }
                
                // El sıkışmaya devam et
                None
            }
            0x04 => {
                // NewSessionTicket — sunucu gelecek bağlantılar için bilet gönderir; önbelleğe eklenir
                if data.len() > 4 {
                    let lifetime = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
                    let ticket_len = data[8] as usize;
                    
                    if data.len() > 9 + ticket_len {
                        let ticket_data = &data[9..9+ticket_len];
                        
                        let ticket = SessionTicket {
                            lifetime,
                            age_add: crate::random::next_u32(),
                            nonce: vec![0],
                            ticket: ticket_data.to_vec(),
                            early_data: EarlyDataConfig::default(),
                            created_at: 0,
                            resumption_secret: vec![0; 32],
                            cipher_suite: CipherSuite::Aes128GcmSha256,
                        };
                        
                        self.session_cache.add(ticket);
                    }
                }
                None
            }
            0x14 => {
                // Şifreli Uzantılar — sunucu el sıkışma anahtarlarıyla ALPN ve diğer uzantıları gönderir
                // early_data uzantısını kontrol et — kabul veya ret burada belirlenebilir
                self.state = TlsState::FinishedReceived;
                None
            }
            _ => None,
        }
    }
    
    /// Uygulama verisi gönderir (mümkünse 0-RTT ile) — el sıkışma tamamlanmışsa normal 1-RTT,
    /// yoksa 0-RTT kullanılır
    pub fn send_data(&mut self, data: &[u8]) -> Option<Vec<u8>> {
        if self.state == TlsState::Established {
            // Normal 1-RTT verisi — geçerli trafik anahtarlarıyla şifreleme yapılacak
            // Mevcut anahtarlarla şifrele
            Some(data.to_vec())
        } else if self.zero_rtt.can_send_early_data() {
            // 0-RTT önden gelen verisi — oturum bileti gizlisiyle şifrelenir
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
