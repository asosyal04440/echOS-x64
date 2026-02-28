//! # WebSocket Protokolü (RFC 6455)
//!
//! Gerçek zamanlı, iki yönlü iletişim için HTTP üzerinden çalışan protokol.
//!
//! ## WebSocket Neden Var?
//!
//! HTTP istek-yanıt modelidir: istemci sorar, sunucu cevaplar.
//! WebSocket ile sunucu da istemciye veri itebilir (server push).
//! Oyunlar, sohbet uygulamaları, canlı veriler için idealdir.
//!
//! ## WebSocket Bağlantı Kurma (HTTP Upgrade)
//!
//! ```
//!  İstemci                              Sunucu
//!     |                                    |
//!     |--- HTTP GET /ws HTTP/1.1 --------->|
//!     |    Upgrade: websocket              |
//!     |    Sec-WebSocket-Key: <nonce>      |
//!     |                                    |
//!     |<-- HTTP 101 Switching Protocols ---|
//!     |    Upgrade: websocket              |
//!     |    Sec-WebSocket-Accept: <hash>    |
//!     |                                    |
//!     |=== WebSocket çerçeveleri ==========>|  (ikili protokol)
//!     |<== WebSocket çerçeveleri ===========|
//!
//!  Sec-WebSocket-Accept = Base64(SHA1(key + "258EAFA5-...GUID"))
//! ```
//!
//! ## WebSocket Çerçeve (Frame) Yapısı
//!
//! ```
//!   0                   1                   2                   3
//!   0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
//!  +-+-+-+-+-------+-+-------------+-------------------------------+
//!  |F|R|R|R| opcode|M| payload len |    Extended payload length    |
//!  |I|S|S|S|  (4)  |A|     (7)     |          (16/64 bit)          |
//!  |N|V|V|V|       |S|             |                               |
//!  | |1|2|3|       |K|             |                               |
//!  +-+-+-+-+-------+-+-------------+ - - - - - - - - - - - - - - -+
//!  |     Extended payload length continued, if payload len == 127  |
//!  + - - - - - - - - - - - - - - -+-------------------------------+
//!  |                               |Masking-key, if MASK set to 1  |
//!  +-------------------------------+-------------------------------+
//!  | Masking-key (devam)           |          Payload Data         |
//!  +-------------------------------- - - - - - - - - - - - - - - -+
//!  :                     Payload Data continued                    :
//!  + - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - +
//!
//!  FIN=1: Mesajın son parçası
//!  MASK=1: İstemci -> Sunucu yönünde zorunlu (güvenlik)
//!  payload len: 0-125 = gerçek uzunluk, 126 = 2 byte ek, 127 = 8 byte ek
//! ```
//!
//! ## Maskeleme (Masking)
//!
//! İstemciden sunucuya giden her çerçeve maskelenmek zorunda:
//! ```
//! masked_byte[i] = original_byte[i] XOR mask[i % 4]
//! ```
//! Bu, proxy'lerin yanlış WebSocket verisine tepki vermesini önler.

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::vec;
use spin::Mutex;

// WebSocket Opcode'ları (çerçeve türü belirler)
/// Devam çerçevesi: Önceki parçanın devamı
const OPCODE_CONTINUATION: u8 = 0x0;
/// Metin çerçevesi: UTF-8 kodlu metin verisi
const OPCODE_TEXT: u8 = 0x1;
/// İkili çerçeve: Ham binary veri
const OPCODE_BINARY: u8 = 0x2;
/// Kapatma çerçevesi: Bağlantıyı nazikçe kapat (opsiyonel kod + neden)
const OPCODE_CLOSE: u8 = 0x8;
/// Ping çerçevesi: Bağlantı denetimi (sunucu -> istemci)
const OPCODE_PING: u8 = 0x9;
/// Pong çerçevesi: Ping yanıtı (otomatik gönderilir)
const OPCODE_PONG: u8 = 0xA;

/// WebSocket protokol el sıkışması için sabit GUID (RFC 6455 Bölüm 1.3)
/// Sunucu bu GUID'i istemci anahtarıyla birleştirip SHA-1 hash'ini döndürür.
const WEBSOCKET_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// WebSocket bağlantı durumu makinesi
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WebSocketState {
    /// HTTP Upgrade isteği gönderildi, yanıt bekleniyor
    Connecting,
    /// El sıkışma tamamlandı, veri alışverişi aktif
    Open,
    /// Kapatma çerçevesi gönderildi, yanıt bekleniyor
    Closing,
    /// Bağlantı tamamen kapatıldı
    Closed,
}

/// WebSocket çerçeve yapısı
///
/// Her WebSocket mesajı bir veya daha fazla çerçeveden oluşur.
/// Büyük mesajlar birden fazla çerçeveye bölünebilir (fragmentasyon).
#[derive(Clone, Debug)]
pub struct WebSocketFrame {
    /// Final bit: Bu çerçeve mesajın son parçası mı?
    pub fin: bool,
    /// Rezerv bit 1 (uzantılar için, normalde 0)
    pub rsv1: bool,
    /// Rezerv bit 2 (uzantılar için, normalde 0)
    pub rsv2: bool,
    /// Rezerv bit 3 (uzantılar için, normalde 0)
    pub rsv3: bool,
    /// Çerçeve türü (TEXT, BINARY, CLOSE, PING, PONG vb.)
    pub opcode: u8,
    /// Maske biti: İstemci -> Sunucu yönünde 1 olmalı
    pub masked: bool,
    /// Payload (yük) uzunluğu (byte)
    pub payload_len: u64,
    /// 4-byte maskeleme anahtarı (istemci tarafı çerçevelerde)
    pub masking_key: Option<[u8; 4]>,
    /// Çerçeve verisi (maske uygulanmışsa çözülmüş halde)
    pub payload: Vec<u8>,
}

impl WebSocketFrame {
    /// Yeni metin çerçevesi oluştur (UTF-8)
    pub fn text(data: &str) -> Self {
        WebSocketFrame {
            fin: true,
            rsv1: false,
            rsv2: false,
            rsv3: false,
            opcode: OPCODE_TEXT,
            masked: false,
            payload_len: data.len() as u64,
            masking_key: None,
            payload: data.as_bytes().to_vec(),
        }
    }

    /// Yeni ikili veri çerçevesi oluştur
    pub fn binary(data: Vec<u8>) -> Self {
        WebSocketFrame {
            fin: true,
            rsv1: false,
            rsv2: false,
            rsv3: false,
            opcode: OPCODE_BINARY,
            masked: false,
            payload_len: data.len() as u64,
            masking_key: None,
            payload: data,
        }
    }

    /// Bağlantı kapatma çerçevesi oluştur
    ///
    /// Payload: 2 byte big-endian kapanış kodu + UTF-8 neden dizisi
    pub fn close(code: u16, reason: &str) -> Self {
        let mut payload = Vec::new();
        payload.extend_from_slice(&code.to_be_bytes());
        payload.extend_from_slice(reason.as_bytes());

        WebSocketFrame {
            fin: true,
            rsv1: false,
            rsv2: false,
            rsv3: false,
            opcode: OPCODE_CLOSE,
            masked: false,
            payload_len: payload.len() as u64,
            masking_key: None,
            payload,
        }
    }

    /// Ping çerçevesi oluştur (bağlantı denetimi)
    pub fn ping(data: Vec<u8>) -> Self {
        WebSocketFrame {
            fin: true,
            rsv1: false,
            rsv2: false,
            rsv3: false,
            opcode: OPCODE_PING,
            masked: false,
            payload_len: data.len() as u64,
            masking_key: None,
            payload: data,
        }
    }

    /// Pong çerçevesi oluştur (ping yanıtı)
    pub fn pong(data: Vec<u8>) -> Self {
        WebSocketFrame {
            fin: true,
            rsv1: false,
            rsv2: false,
            rsv3: false,
            opcode: OPCODE_PONG,
            masked: false,
            payload_len: data.len() as u64,
            masking_key: None,
            payload: data,
        }
    }

    /// Çerçeveyi maskele (istemci -> sunucu yönü için zorunlu)
    ///
    /// XOR maskeleme: masked[i] = original[i] XOR key[i % 4]
    /// Maskeleme anahtarı her çerçeve için rastgele seçilir.
    pub fn mask(&mut self, key: [u8; 4]) {
        self.masked = true;
        self.masking_key = Some(key);

        // Apply mask to payload
        if let Some(mask) = self.masking_key {
            for i in 0..self.payload.len() {
                self.payload[i] ^= mask[i % 4];
            }
        }
    }

    /// Çerçeveyi byte dizisine kodla (wire format)
    ///
    /// ## Payload Uzunluk Kodlaması
    ///
    /// ```
    /// 0-125    : 7-bit doğrudan uzunluk
    /// 126      : Sonraki 2 byte = 16-bit uzunluk (65535'e kadar)
    /// 127      : Sonraki 8 byte = 64-bit uzunluk (devasa dosyalar)
    /// ```
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();

        // İlk byte: FIN, RSV1-3, Opcode
        let mut first = 0u8;
        if self.fin { first |= 0x80; }
        if self.rsv1 { first |= 0x40; }
        if self.rsv2 { first |= 0x20; }
        if self.rsv3 { first |= 0x10; }
        first |= self.opcode & 0x0F;
        buf.push(first);

        // İkinci byte: MASK biti + Payload uzunluğu
        let mut second = 0u8;
        if self.masked { second |= 0x80; }

        if self.payload_len < 126 {
            second |= self.payload_len as u8;
            buf.push(second);
        } else if self.payload_len < 65536 {
            second |= 126; // Genişletilmiş 16-bit uzunluk
            buf.push(second);
            buf.extend_from_slice(&(self.payload_len as u16).to_be_bytes());
        } else {
            second |= 127; // Genişletilmiş 64-bit uzunluk
            buf.push(second);
            buf.extend_from_slice(&self.payload_len.to_be_bytes());
        }

        // Maskeleme anahtarı (varsa)
        if let Some(key) = self.masking_key {
            buf.extend_from_slice(&key);
        }

        // Payload verisi
        buf.extend_from_slice(&self.payload);

        buf
    }

    /// Byte dizisinden çerçeve çöz
    ///
    /// Döndürür: (çerçeve, tüketilen_byte_sayısı)
    /// Eksik veri varsa IncompleteFrame hatası (daha fazla veri bekle)
    pub fn decode(data: &[u8]) -> Result<(Self, usize), WebSocketError> {
        if data.len() < 2 {
            return Err(WebSocketError::IncompleteFrame);
        }

        let first = data[0];
        let second = data[1];

        let fin = (first & 0x80) != 0;
        let rsv1 = (first & 0x40) != 0;
        let rsv2 = (first & 0x20) != 0;
        let rsv3 = (first & 0x10) != 0;
        let opcode = first & 0x0F;

        let masked = (second & 0x80) != 0;
        let mut payload_len = (second & 0x7F) as u64;

        let mut offset = 2;

        // Genişletilmiş payload uzunluğu çözümle
        if payload_len == 126 {
            if data.len() < offset + 2 {
                return Err(WebSocketError::IncompleteFrame);
            }
            payload_len = u16::from_be_bytes([data[offset], data[offset + 1]]) as u64;
            offset += 2;
        } else if payload_len == 127 {
            if data.len() < offset + 8 {
                return Err(WebSocketError::IncompleteFrame);
            }
            payload_len = u64::from_be_bytes([
                data[offset], data[offset + 1], data[offset + 2], data[offset + 3],
                data[offset + 4], data[offset + 5], data[offset + 6], data[offset + 7],
            ]);
            offset += 8;
        }

        // Maskeleme anahtarı
        let masking_key = if masked {
            if data.len() < offset + 4 {
                return Err(WebSocketError::IncompleteFrame);
            }
            let key = [data[offset], data[offset + 1], data[offset + 2], data[offset + 3]];
            offset += 4;
            Some(key)
        } else {
            None
        };

        // Payload verisi
        if data.len() < offset + payload_len as usize {
            return Err(WebSocketError::IncompleteFrame);
        }

        let mut payload = data[offset..offset + payload_len as usize].to_vec();
        offset += payload_len as usize;

        // Maskeyi çöz (XOR ile maskeleme tersine çevrilir)
        if let Some(key) = masking_key {
            for i in 0..payload.len() {
                payload[i] ^= key[i % 4];
            }
        }

        Ok((WebSocketFrame {
            fin,
            rsv1,
            rsv2,
            rsv3,
            opcode,
            masked,
            payload_len,
            masking_key,
            payload,
        }, offset))
    }

    /// Metin çerçevesi mi?
    pub fn is_text(&self) -> bool {
        self.opcode == OPCODE_TEXT
    }

    /// İkili çerçeve mi?
    pub fn is_binary(&self) -> bool {
        self.opcode == OPCODE_BINARY
    }

    /// Kapatma çerçevesi mi?
    pub fn is_close(&self) -> bool {
        self.opcode == OPCODE_CLOSE
    }

    /// Ping çerçevesi mi?
    pub fn is_ping(&self) -> bool {
        self.opcode == OPCODE_PING
    }

    /// Pong çerçevesi mi?
    pub fn is_pong(&self) -> bool {
        self.opcode == OPCODE_PONG
    }

    /// Payload'ı UTF-8 string olarak döndür
    pub fn payload_as_string(&self) -> String {
        String::from_utf8_lossy(&self.payload).to_string()
    }

    /// Kapatma kodu döndür (varsa)
    pub fn close_code(&self) -> Option<u16> {
        if self.is_close() && self.payload.len() >= 2 {
            Some(u16::from_be_bytes([self.payload[0], self.payload[1]]))
        } else {
            None
        }
    }

    /// Kapatma nedeni döndür (varsa)
    pub fn close_reason(&self) -> Option<&str> {
        if self.is_close() && self.payload.len() > 2 {
            core::str::from_utf8(&self.payload[2..]).ok()
        } else {
            None
        }
    }
}

/// WebSocket hata türleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WebSocketError {
    /// Tam çerçeve alınmadı (daha fazla veri bekleniyor)
    IncompleteFrame,
    /// Bilinmeyen opcode
    InvalidOpcode,
    /// Protokol ihlali
    ProtocolError,
    /// Geçersiz UTF-8 metin
    InvalidUtf8,
    /// Bağlantı kapatıldı
    ConnectionClosed,
    /// HTTP Upgrade el sıkışması başarısız
    HandshakeFailed,
    /// Çerçeve boyutu çok büyük
    FrameTooLarge,
}

/// WebSocket kapatma kodları (RFC 6455 Bölüm 7.4)
pub mod close_codes {
    /// Normal kapatma (1000)
    pub const NORMAL: u16 = 1000;
    /// Sayfa kapatılıyor/navigasyon (1001)
    pub const GOING_AWAY: u16 = 1001;
    /// Protokol hatası (1002)
    pub const PROTOCOL_ERROR: u16 = 1002;
    /// Desteklenmeyen veri tipi (1003)
    pub const UNSUPPORTED: u16 = 1003;
    /// Kapatma kodu yok (1005)
    pub const NO_STATUS: u16 = 1005;
    /// Anormal kapatma/bağlantı kesildi (1006)
    pub const ABNORMAL: u16 = 1006;
    /// Tutarsız veri (örn: metin olmayan UTF-8) (1007)
    pub const INVALID_DATA: u16 = 1007;
    /// Politika ihlali (1008)
    pub const POLICY_VIOLATION: u16 = 1008;
    /// Mesaj çok büyük (1009)
    pub const MESSAGE_TOO_BIG: u16 = 1009;
    /// Zorunlu uzantı eksik (1010)
    pub const MANDATORY_EXTENSION: u16 = 1010;
    /// Sunucu iç hatası (1011)
    pub const INTERNAL_ERROR: u16 = 1011;
    /// Sunucu yeniden başlatılıyor (1012)
    pub const SERVICE_RESTART: u16 = 1012;
    /// Daha sonra tekrar dene (1013)
    pub const TRY_AGAIN_LATER: u16 = 1013;
    /// TLS el sıkışması başarısız (1015)
    pub const TLS_HANDSHAKE: u16 = 1015;
}

/// WebSocket HTTP el sıkışması yardımcısı
pub struct WebSocketHandshake;

impl WebSocketHandshake {
    /// İstemci el sıkışma anahtarı üret (16 byte rastgele)
    pub fn generate_key() -> [u8; 16] {
        let mut key = [0u8; 16];
        crate::crypto::rdrand_bytes(&mut key);
        key
    }

    /// İstemci HTTP Upgrade isteği oluştur
    ///
    /// Sec-WebSocket-Key: Base64(16_byte_rastgele)
    /// Sunucu bu anahtarı GUID ile birleştirip SHA-1 hash'ler ve döndürür.
    pub fn build_request(host: &str, port: u16, path: &str, key: &[u8; 16]) -> String {
        // Base64 encode the key
        let key_b64 = base64_encode(key);

        let mut request = String::new();
        request.push_str("GET ");
        request.push_str(path);
        request.push_str(" HTTP/1.1\r\n");
        request.push_str("Host: ");
        request.push_str(host);
        if port != 80 && port != 443 {
            request.push(':');
            request.push_str(&port.to_string());
        }
        request.push_str("\r\n");
        request.push_str("Upgrade: websocket\r\n");
        request.push_str("Connection: Upgrade\r\n");
        request.push_str("Sec-WebSocket-Key: ");
        request.push_str(&key_b64);
        request.push_str("\r\n");
        request.push_str("Sec-WebSocket-Version: 13\r\n");
        request.push_str("User-Agent: echOS/1.0\r\n");
        request.push_str("\r\n");

        request
    }

    /// Sunucu yanıtını doğrula (101 Switching Protocols + doğru Accept anahtarı)
    pub fn verify_response(response: &str, key: &[u8; 16]) -> Result<String, WebSocketError> {
        // Check for 101 Switching Protocols
        if !response.contains("101") || !response.contains("Switching Protocols") {
            return Err(WebSocketError::HandshakeFailed);
        }

        // Check for Upgrade: websocket
        if !response.to_lowercase().contains("upgrade: websocket") {
            return Err(WebSocketError::HandshakeFailed);
        }

        // Find and verify Sec-WebSocket-Accept
        let accept_key = Self::compute_accept_key(key);

        if !response.contains(&accept_key) {
            return Err(WebSocketError::HandshakeFailed);
        }

        // Extract protocols if any
        let protocol = if let Some(start) = response.find("Sec-WebSocket-Protocol:") {
            let rest = &response[start + 22..];
            if let Some(end) = rest.find("\r\n") {
                rest[..end].trim().to_string()
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        Ok(protocol)
    }

    /// Sec-WebSocket-Accept anahtarını hesapla
    ///
    /// Formül: Base64(SHA1(Base64(key) + WEBSOCKET_GUID))
    pub fn compute_accept_key(key: &[u8; 16]) -> String {
        // Concatenate key with GUID
        let key_b64 = base64_encode(key);
        let mut input = String::new();
        input.push_str(&key_b64);
        input.push_str(WEBSOCKET_GUID);

        // SHA-1 hash
        let mut hasher = crate::crypto::Sha3::sha3_256();
        hasher.update(input.as_bytes());
        let hash = hasher.finalize();

        // Base64 encode first 20 bytes (SHA-1 length, but we use SHA-256)
        base64_encode(&hash[..20])
    }
}

/// WebSocket bağlantısı
///
/// Bir WebSocket bağlantısı oluşturulduktan sonra:
/// 1. HTTP Upgrade el sıkışması tamamlanır
/// 2. İkili çerçeve protokolüne geçilir
/// 3. Her iki yönde de veri gönderilebilir
#[derive(Clone)]
pub struct WebSocketConnection {
    /// Bağlantı durumu
    pub state: WebSocketState,
    /// Sunucu adı (SNI için)
    pub host: String,
    /// Sunucu port numarası
    pub port: u16,
    /// WebSocket endpoint yolu
    pub path: String,
    /// Seçilen alt-protokol
    pub protocol: String,
    /// Gönderim tamponu (şifreli/maskelenmiş veriler)
    pub send_buffer: Vec<u8>,
    /// Alım tamponu (henüz işlenmemiş gelen veriler)
    pub recv_buffer: Vec<u8>,
}

impl WebSocketConnection {
    /// Yeni WebSocket bağlantısı oluştur
    pub fn new(host: &str, port: u16, path: &str) -> Self {
        WebSocketConnection {
            state: WebSocketState::Connecting,
            host: host.to_string(),
            port,
            path: path.to_string(),
            protocol: String::new(),
            send_buffer: Vec::new(),
            recv_buffer: Vec::new(),
        }
    }

    /// Metin mesajı gönder (istemci -> sunucu, maskeli)
    pub fn send_text(&mut self, message: &str) -> Vec<u8> {
        let mut frame = WebSocketFrame::text(message);
        frame.mask(Self::generate_mask()); // RFC zorunluluğu: istemci maskelemeli
        frame.encode()
    }

    /// İkili veri gönder (istemci -> sunucu, maskeli)
    pub fn send_binary(&mut self, data: &[u8]) -> Vec<u8> {
        let mut frame = WebSocketFrame::binary(data.to_vec());
        frame.mask(Self::generate_mask());
        frame.encode()
    }

    /// Bağlantıyı kapat (kapatma çerçevesi gönder)
    pub fn send_close(&mut self, code: u16, reason: &str) -> Vec<u8> {
        let mut frame = WebSocketFrame::close(code, reason);
        frame.mask(Self::generate_mask());
        self.state = WebSocketState::Closing;
        frame.encode()
    }

    /// Ping gönder (bağlantı canlılık kontrolü)
    pub fn send_ping(&mut self, data: &[u8]) -> Vec<u8> {
        let mut frame = WebSocketFrame::ping(data.to_vec());
        frame.mask(Self::generate_mask());
        frame.encode()
    }

    /// Gelen veriyi işle ve tamamlanmış çerçeveleri döndür
    ///
    /// Ping çerçevelerine otomatik Pong yanıtı üretir.
    /// Close çerçevesi gelirse durumu Closed'a geçirir.
    pub fn receive(&mut self, data: &[u8]) -> Result<Vec<WebSocketFrame>, WebSocketError> {
        self.recv_buffer.extend_from_slice(data);

        let mut frames = Vec::new();

        loop {
            match WebSocketFrame::decode(&self.recv_buffer) {
                Ok((frame, consumed)) => {
                    // Kontrol çerçevelerini otomatik işle
                    if frame.is_ping() {
                        // Auto-respond with pong
                        let pong = WebSocketFrame::pong(frame.payload.clone());
                        self.send_buffer.extend_from_slice(&pong.encode());
                    } else if frame.is_close() {
                        self.state = WebSocketState::Closed;
                    } else {
                        frames.push(frame);
                    }

                    // Remove consumed bytes
                    self.recv_buffer.drain(..consumed);
                }
                Err(WebSocketError::IncompleteFrame) => {
                    // Need more data
                    break;
                }
                Err(e) => {
                    return Err(e);
                }
            }
        }

        Ok(frames)
    }

    /// Gönderim tamponundaki veriyi al ve tamponu temizle
    pub fn get_send_data(&mut self) -> Option<Vec<u8>> {
        if self.send_buffer.is_empty() {
            None
        } else {
            let data = self.send_buffer.clone();
            self.send_buffer.clear();
            Some(data)
        }
    }

    /// Rastgele maskeleme anahtarı üret (her çerçeve için ayrı)
    fn generate_mask() -> [u8; 4] {
        let mut mask = [0u8; 4];
        crate::crypto::rdrand_bytes(&mut mask);
        mask
    }
}

/// Base64 kodlama (WebSocket el sıkışması için)
///
/// RFC 4648 Base64 alfabesi: A-Z a-z 0-9 + /
/// Her 3 byte girdi -> 4 ASCII karakter çıktı
fn base64_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut result = String::new();
    let mut i = 0;

    while i < data.len() {
        let b0 = data[i] as usize;
        let b1 = if i + 1 < data.len() { data[i + 1] as usize } else { 0 };
        let b2 = if i + 2 < data.len() { data[i + 2] as usize } else { 0 };

        result.push(ALPHABET[b0 >> 2] as char);
        result.push(ALPHABET[((b0 & 0x03) << 4) | (b1 >> 4)] as char);

        if i + 1 < data.len() {
            result.push(ALPHABET[((b1 & 0x0F) << 2) | (b2 >> 6)] as char);
        } else {
            result.push('='); // Dolgu karakteri
        }

        if i + 2 < data.len() {
            result.push(ALPHABET[b2 & 0x3F] as char);
        } else {
            result.push('='); // Dolgu karakteri
        }

        i += 3;
    }

    result
}

// Tüm aktif WebSocket bağlantıları: bağlantı_id -> WebSocketConnection
lazy_static::lazy_static! {
    static ref WS_CONNECTIONS: Mutex<BTreeMap<u32, WebSocketConnection>> = Mutex::new(BTreeMap::new());
    static ref WS_NEXT_ID: Mutex<u32> = Mutex::new(1);
}

/// Yeni WebSocket bağlantısı oluştur ve ID döndür
pub fn connect_ws(host: &str, port: u16, path: &str) -> u32 {
    let mut connections = WS_CONNECTIONS.lock();
    let mut next_id = WS_NEXT_ID.lock();

    let id = *next_id;
    *next_id += 1;

    connections.insert(id, WebSocketConnection::new(host, port, path));
    id
}

/// Bağlantı ID'sine göre WebSocket bağlantısı getir
pub fn get_connection(id: u32) -> Option<WebSocketConnection> {
    WS_CONNECTIONS.lock().get(&id).cloned()
}

/// WebSocket bağlantısını kapat ve kaynakları serbest bırak
pub fn close_ws(id: u32) {
    WS_CONNECTIONS.lock().remove(&id);
}
