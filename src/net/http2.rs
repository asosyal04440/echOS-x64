//! # HTTP/2 Protokolü
//!
//! HTTP/2, multiplexing (çoklama), HPACK başlık sıkıştırma ve akış önceliklendirme sunar.
//!
//! ## HTTP/1.1 vs HTTP/2 Temel Fark
//!
//! ```
//! HTTP/1.1 (Sıralı - Head-of-Line Blocking var):
//! ┌──────────────────────────────────────────────────┐
//! │ İstek 1 → Yanıt 1 → İstek 2 → Yanıt 2 → ...   │
//! └──────────────────────────────────────────────────┘
//!
//! HTTP/2 (Eşzamanlı - Multiplexing):
//! ┌──────────────────────────────────────────────────┐
//! │                  TEK TCP Bağlantısı               │
//! │  Akış 1: ██████████░░░░████████                  │
//! │  Akış 2: ░░░░████████████░░░░░░                  │
//! │  Akış 3: ████░░░░████░░░░████                    │
//! └──────────────────────────────────────────────────┘
//! ```
//!
//! ## HTTP/2 Çerçeve Yapısı (9 Bayt Başlık)
//!
//! ```
//! 0       8       16      24      32
//! ┌───────────────────────┬────────┐
//! │    Uzunluk (24 bit)   │ Tür    │
//! ├───────────────────────┴────────┤
//! │ Bayraklar (8 bit)              │
//! ├────────────────────────────────┤
//! │ R │    Akış ID (31 bit)        │
//! ├───┴────────────────────────────┤
//! │        Yük (Payload)           │
//! └────────────────────────────────┘
//! ```
//!
//! ## Bağlantı Katmanları
//!
//! ```
//! Uygulama
//!    │
//!    ▼
//! HTTP/2 Çerçeveleme ◄── HPACK (başlık sıkıştırma)
//!    │
//!    ▼
//! TLS (isteğe bağlı)
//!    │
//!    ▼
//! TCP
//! ```

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use spin::Mutex;

#[path = "http2_huffman.rs"]
mod http2_huffman;

// HTTP/2 Çerçeve Türleri
// Her çerçeve türü farklı bir amaca hizmet eder:
// - DATA: Asıl uygulama verisi taşır
// - HEADERS: HTTP başlıklarını iletir (HPACK ile sıkıştırılmış)
// - PRIORITY: Akış önceliğini değiştirir
// - RST_STREAM: Akışı hemen sonlandırır
// - SETTINGS: Bağlantı parametrelerini müzakere eder
// - PUSH_PROMISE: Sunucu push'unu bildirir
// - PING: Keep-alive ve RTT ölçümü için
// - GOAWAY: Bağlantıyı düzgünce kapatır
// - WINDOW_UPDATE: Akış kontrolü penceresi günceller
// - CONTINUATION: Büyük başlık bloklarını devam ettirir
const FRAME_DATA: u8 = 0x00;
const FRAME_HEADERS: u8 = 0x01;
const FRAME_PRIORITY: u8 = 0x02;
const FRAME_RST_STREAM: u8 = 0x03;
const FRAME_SETTINGS: u8 = 0x04;
const FRAME_PUSH_PROMISE: u8 = 0x05;
const FRAME_PING: u8 = 0x06;
const FRAME_GOAWAY: u8 = 0x07;
const FRAME_WINDOW_UPDATE: u8 = 0x08;
const FRAME_CONTINUATION: u8 = 0x09;

// HTTP/2 Ayarları (Settings)
// Bağlantı kurulurken her iki taraf da kendi kapasitesini bildirir.
// Örneğin: kaç eş zamanlı akış desteklenir, başlık tablosu ne kadar büyük olabilir.
const SETTINGS_HEADER_TABLE_SIZE: u16 = 0x01;
const SETTINGS_ENABLE_PUSH: u16 = 0x02;
const SETTINGS_MAX_CONCURRENT_STREAMS: u16 = 0x03;
const SETTINGS_INITIAL_WINDOW_SIZE: u16 = 0x04;
const SETTINGS_MAX_FRAME_SIZE: u16 = 0x05;
const SETTINGS_MAX_HEADER_LIST_SIZE: u16 = 0x06;

// HTTP/2 Hata Kodları
// Bir sorun oluştuğunda RST_STREAM veya GOAWAY çerçevesiyle gönderilir.
// NO_ERROR bağlantının temiz kapandığını ifade eder.
const NO_ERROR: u32 = 0x00;
const PROTOCOL_ERROR: u32 = 0x01;
const INTERNAL_ERROR: u32 = 0x02;
const FLOW_CONTROL_ERROR: u32 = 0x03;
const SETTINGS_TIMEOUT: u32 = 0x04;
const STREAM_CLOSED: u32 = 0x05;
const FRAME_SIZE_ERROR: u32 = 0x06;
const REFUSED_STREAM: u32 = 0x07;
const CANCEL: u32 = 0x08;
const COMPRESSION_ERROR: u32 = 0x09;
const CONNECT_ERROR: u32 = 0x0a;
const ENHANCE_YOUR_CALM: u32 = 0x0b;
const INADEQUATE_SECURITY: u32 = 0x0c;
const HTTP_1_1_REQUIRED: u32 = 0x0d;

// HTTP/2 Bağlantı Ön Söz (Connection Preface)
// İstemci bağlantı açar açmaz bu sabit diziyi gönderir.
// Sunucu da bu diziyi alınca HTTP/2 konuşulduğunu anlar.
// "PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n" (24 bayt)
const CONNECTION_PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

/// HTTP/2 Çerçeve (Frame)
///
/// Tüm HTTP/2 iletişimi çerçeveler üzerinden yürütülür.
/// Her çerçeve 9 bayt sabit başlık + değişken uzunluklu yükten oluşur.
///
/// ```
/// ┌──────────────────────────────────┐
/// │ length  (24 bit): Yük bayt sayısı│
/// │ type     (8 bit): Çerçeve türü   │
/// │ flags    (8 bit): Durum bayrakları│
/// │ stream_id(31bit): Akış tanımlayıcı│
/// │ payload        : Asıl veri       │
/// └──────────────────────────────────┘
/// ```
#[derive(Clone, Debug)]
pub struct Http2Frame {
    pub length: u32,
    pub frame_type: u8,
    pub flags: u8,
    pub stream_id: u32,
    pub payload: Vec<u8>,
}

impl Http2Frame {
    /// Yeni bir HTTP/2 çerçevesi oluşturur.
    /// Uzunluk otomatik olarak yük boyutundan hesaplanır.
    pub fn new(frame_type: u8, stream_id: u32, payload: Vec<u8>) -> Self {
        Http2Frame {
            length: payload.len() as u32,
            frame_type,
            flags: 0,
            stream_id,
            payload,
        }
    }

    /// SETTINGS çerçevesi oluşturur.
    ///
    /// Bağlantı parametrelerini karşı tarafa bildirir.
    /// Her ayar 6 bayt: 2 bayt ID + 4 bayt değer.
    ///
    /// ```
    /// ┌─────────┬──────────────────────┐
    /// │ ID (2B) │    Değer (4B)        │
    /// ├─────────┼──────────────────────┤
    /// │ 0x0001  │ Başlık tablo boyutu  │
    /// │ 0x0002  │ Push etkin mi?       │
    /// │ 0x0003  │ Max eş zamanlı akış  │
    /// │ 0x0004  │ Başlangıç penceresi  │
    /// │ 0x0005  │ Max çerçeve boyutu   │
    /// └─────────┴──────────────────────┘
    /// ```
    pub fn settings(settings: &Http2Settings) -> Self {
        let mut payload = Vec::new();

        // Header table size (başlık tablosu boyutu - HPACK için)
        payload.extend_from_slice(&SETTINGS_HEADER_TABLE_SIZE.to_be_bytes()[2..]);
        payload.extend_from_slice(&settings.header_table_size.to_be_bytes());

        // Enable push (sunucu push'u etkinleştir/devre dışı bırak)
        payload.extend_from_slice(&SETTINGS_ENABLE_PUSH.to_be_bytes()[2..]);
        payload.extend_from_slice(&(settings.enable_push as u32).to_be_bytes());

        // Max concurrent streams (aynı anda kaç akış açık olabilir)
        payload.extend_from_slice(&SETTINGS_MAX_CONCURRENT_STREAMS.to_be_bytes()[2..]);
        payload.extend_from_slice(&settings.max_concurrent_streams.to_be_bytes());

        // Initial window size (akış kontrolü başlangıç penceresi)
        payload.extend_from_slice(&SETTINGS_INITIAL_WINDOW_SIZE.to_be_bytes()[2..]);
        payload.extend_from_slice(&settings.initial_window_size.to_be_bytes());

        // Max frame size (tek bir çerçevenin maksimum yük boyutu)
        payload.extend_from_slice(&SETTINGS_MAX_FRAME_SIZE.to_be_bytes()[2..]);
        payload.extend_from_slice(&settings.max_frame_size.to_be_bytes());

        Http2Frame::new(FRAME_SETTINGS, 0, payload)
    }

    /// HEADERS çerçevesi oluşturur.
    ///
    /// HTTP başlıklarını HPACK ile sıkıştırılmış biçimde taşır.
    /// `end_stream` bayrağı set edilirse bu akış için daha veri gelmeyecek demektir.
    ///
    /// Bayraklar:
    /// - 0x01 = END_STREAM: Bu akışın son verisi
    /// - 0x04 = END_HEADERS: Başlık bloğu tamamlandı
    pub fn headers(stream_id: u32, header_block: Vec<u8>, end_stream: bool) -> Self {
        let mut frame = Http2Frame::new(FRAME_HEADERS, stream_id, header_block);
        if end_stream {
            frame.flags |= 0x01; // END_STREAM
        }
        frame.flags |= 0x04; // END_HEADERS
        frame
    }

    /// DATA çerçevesi oluşturur.
    ///
    /// Asıl uygulama verisini (örn. HTTP yanıt gövdesi) taşır.
    /// Akış kontrolüne tabidir: alıcı hazır değilse veri gönderilemez.
    pub fn data(stream_id: u32, data: Vec<u8>, end_stream: bool) -> Self {
        let mut frame = Http2Frame::new(FRAME_DATA, stream_id, data);
        if end_stream {
            frame.flags |= 0x01; // END_STREAM
        }
        frame
    }

    /// WINDOW_UPDATE çerçevesi oluşturur.
    ///
    /// Akış kontrolü: alıcı daha fazla veri almaya hazır olduğunu bildirir.
    /// `stream_id == 0` ise bağlantı düzeyinde güncelleme yapılır.
    ///
    /// ```
    /// Gönderen          Alıcı
    ///    │                │
    ///    │── Veri ───────►│ (pencere azalır)
    ///    │                │
    ///    │◄── WINDOW_UPDATE (pencere büyür)
    ///    │                │
    ///    │── Daha fazla veri ──►│
    /// ```
    pub fn window_update(stream_id: u32, increment: u32) -> Self {
        let mut payload = Vec::new();
        payload.extend_from_slice(&increment.to_be_bytes()[..]);
        Http2Frame::new(FRAME_WINDOW_UPDATE, stream_id, payload)
    }

    /// PING çerçevesi oluşturur.
    ///
    /// Bağlantının canlı olup olmadığını kontrol eder (keep-alive).
    /// 8 bayt opaque veri içerir; karşı taraf aynı veriyle ACK döner.
    pub fn ping(opaque: [u8; 8], ack: bool) -> Self {
        let mut frame = Http2Frame::new(FRAME_PING, 0, opaque.to_vec());
        if ack {
            frame.flags |= 0x01; // ACK
        }
        frame
    }

    /// GOAWAY çerçevesi oluşturur.
    ///
    /// Bağlantıyı düzgünce kapatır.
    /// `last_stream_id` son işlenen akış ID'sini, `error_code` neden kapandığını belirtir.
    ///
    /// ```
    /// İstemci          Sunucu
    ///    │                │
    ///    │◄── GOAWAY ────│ (son_akış_id, hata_kodu)
    ///    │                │
    ///    │ (last_stream_id'den büyük akışlar yeniden denenebilir)
    /// ```
    pub fn goaway(last_stream_id: u32, error_code: u32, debug_data: Vec<u8>) -> Self {
        let mut payload = Vec::new();
        payload.extend_from_slice(&last_stream_id.to_be_bytes()[..]);
        payload.extend_from_slice(&error_code.to_be_bytes()[..]);
        payload.extend_from_slice(&debug_data);
        Http2Frame::new(FRAME_GOAWAY, 0, payload)
    }

    /// RST_STREAM çerçevesi oluşturur.
    ///
    /// Tek bir akışı anında sonlandırır (tüm bağlantıyı değil).
    /// GOAWAY'den farkı: sadece belirtilen akışı etkiler.
    pub fn rst_stream(stream_id: u32, error_code: u32) -> Self {
        let mut payload = Vec::new();
        payload.extend_from_slice(&error_code.to_be_bytes()[..]);
        Http2Frame::new(FRAME_RST_STREAM, stream_id, payload)
    }

    /// Çerçeveyi bayt dizisine dönüştürür (serileştirme).
    ///
    /// HTTP/2 çerçeve wire formatı:
    /// ```
    /// ┌────────────────────────────────────────────┐
    /// │ [0..3) Uzunluk: 3 bayt, büyük-endian       │
    /// │ [3]    Tür: 1 bayt                         │
    /// │ [4]    Bayraklar: 1 bayt                   │
    /// │ [5..9) Akış ID: 4 bayt (MSB sıfır/reserved)│
    /// │ [9..]  Yük: length bayt                    │
    /// └────────────────────────────────────────────┘
    /// ```
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(9 + self.payload.len());

        // Length (24 bits) - büyük-endian, 3 bayt
        buf.push(((self.length >> 16) & 0xFF) as u8);
        buf.push(((self.length >> 8) & 0xFF) as u8);
        buf.push((self.length & 0xFF) as u8);

        // Type (8 bits) - çerçeve türü
        buf.push(self.frame_type);

        // Flags (8 bits) - bayraklar
        buf.push(self.flags);

        // Stream ID (32 bits, R bit reserved) - en yüksek bit her zaman 0
        buf.extend_from_slice(&self.stream_id.to_be_bytes()[..]);

        // Payload - asıl veri
        buf.extend_from_slice(&self.payload);

        buf
    }

    /// Bayt dizisinden çerçeve ayrıştırır (çözümleme).
    ///
    /// Başarılı olursa `(çerçeve, tüketilen_bayt_sayısı)` döner.
    /// Veri yetersizse veya bozuksa `None` döner.
    pub fn decode(data: &[u8]) -> Option<(Self, usize)> {
        if data.len() < 9 {
            return None;
        }

        let length = ((data[0] as u32) << 16) | ((data[1] as u32) << 8) | (data[2] as u32);
        let frame_type = data[3];
        let flags = data[4];
        // Akış ID'nin en yüksek biti (R biti) RFC'de tanımsız; maskelenerek sıfırlanır
        let stream_id = u32::from_be_bytes([data[5], data[6], data[7], data[8]]) & 0x7FFFFFFF;

        if data.len() < 9 + length as usize {
            return None;
        }

        let payload = data[9..9 + length as usize].to_vec();
        let frame = Http2Frame {
            length,
            frame_type,
            flags,
            stream_id,
            payload,
        };

        Some((frame, 9 + length as usize))
    }

    /// END_STREAM bayrağını kontrol eder.
    /// Bu bayrak set edilmişse akışta daha fazla veri gelmeyecek demektir.
    pub fn is_end_stream(&self) -> bool {
        (self.flags & 0x01) != 0
    }

    /// END_HEADERS bayrağını kontrol eder.
    /// Bu bayrak set edilmişse başlık bloğu tamamlanmıştır.
    pub fn is_end_headers(&self) -> bool {
        (self.flags & 0x04) != 0
    }

    /// ACK bayrağını kontrol eder (PING ve SETTINGS için).
    pub fn is_ack(&self) -> bool {
        (self.flags & 0x01) != 0
    }
}

/// HTTP/2 Ayarları (Settings)
///
/// Her iki taraf da bağlantı başlangıcında bir SETTINGS çerçevesi gönderir.
/// Varsayılan değerler RFC 7540 tarafından tanımlanmıştır.
///
/// | Ayar                    | Varsayılan | Açıklama                          |
/// |-------------------------|------------|-----------------------------------|
/// | header_table_size       | 4096       | HPACK dinamik tablo boyutu (bayt) |
/// | enable_push             | true       | Sunucu push etkin?                |
/// | max_concurrent_streams  | 100        | Aynı anda max açık akış           |
/// | initial_window_size     | 65535      | Akış kontrolü başlangıç penceresi |
/// | max_frame_size          | 16384      | En büyük çerçeve yükü (bayt)      |
/// | max_header_list_size    | 65536      | Max başlık listesi boyutu         |
#[derive(Clone, Debug)]
pub struct Http2Settings {
    pub header_table_size: u32,
    pub enable_push: bool,
    pub max_concurrent_streams: u32,
    pub initial_window_size: u32,
    pub max_frame_size: u32,
    pub max_header_list_size: u32,
}

impl Default for Http2Settings {
    fn default() -> Self {
        Http2Settings {
            header_table_size: 4096,
            enable_push: true,
            max_concurrent_streams: 100,
            initial_window_size: 65535,
            max_frame_size: 16384,
            max_header_list_size: 65536,
        }
    }
}

/// HTTP/2 Akış Durumu (Stream State)
///
/// Her HTTP/2 akışı bir durum makinesinden geçer:
///
/// ```
///              ┌─────────┐
///              │  Idle   │
///              └────┬────┘
///                   │ open / reserved
///          ┌────────┴────────┐
///          ▼                 ▼
/// ┌──────────────┐    ┌─────────────────┐
/// │ReservedLocal │    │ ReservedRemote  │
/// └──────┬───────┘    └────────┬────────┘
///        │ open                │ open
///        ▼                     ▼
///     ┌──────────────────────────┐
///     │           Open           │
///     └─────────────┬────────────┘
///              END_STREAM│
///       ┌───────────┴──────────┐
///       ▼                      ▼
/// HalfClosedLocal       HalfClosedRemote
///       │                      │
///       └──────────┬───────────┘
///                  ▼
///               Closed
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamState {
    Idle,
    ReservedLocal,
    ReservedRemote,
    Open,
    HalfClosedLocal,
    HalfClosedRemote,
    Closed,
}

/// HTTP/2 Akışı (Stream)
///
/// HTTP/2'de her istek-yanıt çifti tek bir "akış" üzerinde taşınır.
/// Birden fazla akış aynı TCP bağlantısını paylaşır (multiplexing).
///
/// Akış ID kuralları:
/// - İstemci başlatır: tek sayılar (1, 3, 5, ...)
/// - Sunucu başlatır: çift sayılar (2, 4, 6, ...)
/// - Akış 0: bağlantı düzeyinde kontrol mesajları için ayrılmıştır
#[derive(Clone, Debug)]
pub struct Http2Stream {
    pub stream_id: u32,
    pub state: StreamState,
    pub headers: BTreeMap<String, String>,
    pub trailers: BTreeMap<String, String>,
    pub data: Vec<u8>,
    pub window_size: u32,
    pub end_stream: bool,
    pub received_headers: bool,
    pub reset_error: Option<u32>,
}

impl Http2Stream {
    pub fn new(stream_id: u32) -> Self {
        Http2Stream {
            stream_id,
            state: StreamState::Idle,
            headers: BTreeMap::new(),
            trailers: BTreeMap::new(),
            data: Vec::new(),
            window_size: 65535,
            end_stream: false,
            received_headers: false,
            reset_error: None,
        }
    }
}

// ============================================================================
// HPACK Başlık Sıkıştırma
// ============================================================================
//
// HPACK (RFC 7541), HTTP/2 başlıklarını sıkıştırmak için tasarlanmıştır.
// İki tablo kullanır:
//
// 1. Statik Tablo: Yaygın başlıkları sabit indekslerle içerir (":method: GET" = 2 gibi).
// 2. Dinamik Tablo: Oturum boyunca öğrenilen başlıklar eklenir ve önbelleklenir.
//
// Kodlama stratejileri:
//
// a) İndeksli başlık (tam eşleşme):
//    ┌────────────────────────────┐
//    │ 1 │     İndeks (7 bit)     │
//    └────────────────────────────┘
//
// b) Artan indeksleme (isim tabloda, değer yeni):
//    ┌──────┬──────────────────────┐
//    │ 01   │   İsim İndeksi       │
//    ├──────┴──────────────────────┤
//    │     Değer (string)          │
//    └─────────────────────────────┘
//
// c) Yeni başlık (hem isim hem değer yeni):
//    ┌──────┬──────┐
//    │ 0100 0000   │
//    ├─────────────┤
//    │ İsim (str)  │
//    ├─────────────┤
//    │ Değer (str) │
//    └─────────────┘

/// HPACK Statik Tablo (ilk 20 giriş)
///
/// Tüm HTTP/2 uygulamalarında önceden tanımlıdır.
/// Sık kullanılan `:method: GET` gibi başlıklar kısa indekslerle ifade edilir.
/// Tam tablo 61 giriş içerir (burada sadece ilk 20'si).
const STATIC_TABLE: [(&str, &str); 20] = [
    (":authority", ""),
    (":method", "GET"),
    (":method", "POST"),
    (":path", "/"),
    (":path", "/index.html"),
    (":scheme", "http"),
    (":scheme", "https"),
    (":status", "200"),
    (":status", "204"),
    (":status", "206"),
    (":status", "304"),
    (":status", "400"),
    (":status", "404"),
    (":status", "500"),
    ("accept-charset", ""),
    ("accept-encoding", "gzip, deflate"),
    ("accept-language", ""),
    ("accept-ranges", ""),
    ("accept", ""),
    ("access-control-allow-origin", ""),
];

/// HPACK Kodlayıcı (Encoder)
///
/// HTTP başlıklarını ikili forma sıkıştırır.
/// Sıkıştırma oranını artırmak için dinamik tablo yönetir.
pub struct HpackEncoder {
    dynamic_table: Vec<(String, String)>,
    dynamic_table_size: usize,
    max_table_size: usize,
}

impl HpackEncoder {
    pub fn new(max_table_size: usize) -> Self {
        HpackEncoder {
            dynamic_table: Vec::new(),
            dynamic_table_size: 0,
            max_table_size,
        }
    }

    /// Başlık haritasını HPACK ikili formatına kodlar.
    ///
    /// Her başlık için önce statik tablo kontrol edilir.
    /// Bulunamazsa dinamik tabloya yeni giriş eklenir.
    pub fn encode(&mut self, headers: &BTreeMap<String, String>) -> Vec<u8> {
        let mut encoded = Vec::new();

        for (name, value) in headers {
            // Önce statik tabloyu tara: tam eşleşme veya sadece isim eşleşmesi ara
            let mut found_static = false;
            for (i, (s_name, s_value)) in STATIC_TABLE.iter().enumerate() {
                if name == s_name && value == s_value {
                    // Tam eşleşme: sadece indeks gönder (çok verimli)
                    encoded.push(0x80 | (i as u8 + 1));
                    found_static = true;
                    break;
                } else if name == s_name {
                    // İsim eşleşmesi: indeks + yeni değer gönder, dinamik tabloya ekle
                    encoded.push(0x40 | (i as u8 + 1));
                    self.encode_string(value, &mut encoded);
                    found_static = true;
                    break;
                }
            }

            if !found_static {
                // Hiç eşleşme yok: hem isim hem değer olduğu gibi gönderilecek
                encoded.push(0x40);
                self.encode_string(name, &mut encoded);
                self.encode_string(value, &mut encoded);

                // Yeterli yer varsa dinamik tabloya ekle
                if self.dynamic_table_size + name.len() + value.len() + 32 <= self.max_table_size {
                    self.dynamic_table.push((name.clone(), value.clone()));
                    self.dynamic_table_size += name.len() + value.len() + 32;
                }
            }
        }

        encoded
    }

    fn encode_string(&self, s: &str, buf: &mut Vec<u8>) {
        let bytes = s.as_bytes();

        // Emit RFC-valid literal strings. Huffman coding is optional on the
        // wire, so the encoder keeps literal form unless a future corpus
        // requires compressed output.
        if bytes.len() < 127 {
            buf.push(bytes.len() as u8);
        } else {
            // Length > 127: use multi-byte length
            buf.push(0x7F);
            let len = bytes.len();
            let mut remaining = len - 127;
            while remaining >= 128 {
                buf.push(0x80 | (remaining as u8 & 0x7F));
                remaining >>= 7;
            }
            buf.push(remaining as u8);
        }
        buf.extend_from_slice(bytes);
    }
}

/// HPACK Çözücü (Decoder)
///
/// HPACK ikili formatını HTTP başlık haritasına çevirir.
/// Kodlayıcıyla senkronize dinamik tablo yönetir.
pub struct HpackDecoder {
    dynamic_table: Vec<(String, String)>,
    max_table_size: usize,
}

impl HpackDecoder {
    pub fn new(max_table_size: usize) -> Self {
        HpackDecoder {
            dynamic_table: Vec::new(),
            max_table_size,
        }
    }

    /// HPACK başlık bloğunu çözer ve başlık haritası döner.
    ///
    /// İlk baytın yüksek bitlerine bakarak kodlama türü belirlenir:
    /// - 1xxxxxxx: İndeksli başlık (tam tablo araması)
    /// - 01xxxxxx: Artan indeksleme (dinamik tabloya ekle)
    /// - 0000xxxx: İndeksleme yok (tabloya ekleme)
    /// - 0001xxxx: Asla indeksleme (hassas veriler için)
    pub fn decode(&mut self, data: &[u8]) -> Result<BTreeMap<String, String>, HpackError> {
        let mut headers = BTreeMap::new();
        let mut pos = 0;

        while pos < data.len() {
            let byte = data[pos];

            if byte & 0x80 != 0 {
                // İndeksli başlık alanı: tüm başlık tabloda
                let index = (byte & 0x7F) as usize;
                if let Some((name, value)) = self.get_header(index) {
                    headers.insert(name.to_string(), value.to_string());
                }
                pos += 1;
            } else if byte & 0xC0 == 0x40 {
                // Artan indeksleme ile literal: dinamik tabloya ekle
                let index = (byte & 0x3F) as usize;
                pos += 1;

                let (name, value) = if index == 0 {
                    // New name
                    let name = self.decode_string(data, &mut pos)?;
                    let value = self.decode_string(data, &mut pos)?;
                    (name, value)
                } else if let Some((n, _)) = self.get_header(index) {
                    let value = self.decode_string(data, &mut pos)?;
                    (n.to_string(), value)
                } else {
                    return Err(HpackError::InvalidIndex);
                };

                headers.insert(name.clone(), value.clone());
                self.dynamic_table.insert(0, (name, value));
            } else if byte & 0xF0 == 0x00 {
                // İndeksleme olmadan literal: dinamik tabloya eklenmez
                let index = (byte & 0x0F) as usize;
                pos += 1;

                let (name, value) = if index == 0 {
                    let name = self.decode_string(data, &mut pos)?;
                    let value = self.decode_string(data, &mut pos)?;
                    (name, value)
                } else if let Some((n, _)) = self.get_header(index) {
                    let value = self.decode_string(data, &mut pos)?;
                    (n.to_string(), value)
                } else {
                    return Err(HpackError::InvalidIndex);
                };

                headers.insert(name, value);
            } else if byte & 0xF0 == 0x10 {
                // Asla indeksleme: ara proxy'ler bu başlığı tabloya ekleyemez
                let index = (byte & 0x0F) as usize;
                pos += 1;

                let (name, value) = if index == 0 {
                    let name = self.decode_string(data, &mut pos)?;
                    let value = self.decode_string(data, &mut pos)?;
                    (name, value)
                } else if let Some((n, _)) = self.get_header(index) {
                    let value = self.decode_string(data, &mut pos)?;
                    (n.to_string(), value)
                } else {
                    return Err(HpackError::InvalidIndex);
                };

                headers.insert(name, value);
            } else if byte == 0x20 {
                // Dinamik tablo boyutu güncelleme: yeni max boyutu uygula
                pos += 1;
                let _size = self.decode_integer(data, &mut pos, 5)?;
            } else {
                return Err(HpackError::InvalidPrefix);
            }
        }

        Ok(headers)
    }

    fn get_header(&self, index: usize) -> Option<(&str, &str)> {
        if index == 0 {
            return None;
        }

        if index <= STATIC_TABLE.len() {
            Some(STATIC_TABLE[index - 1])
        } else {
            let dynamic_index = index - STATIC_TABLE.len() - 1;
            self.dynamic_table
                .get(dynamic_index)
                .map(|(n, v)| (n.as_str(), v.as_str()))
        }
    }

    fn decode_string(&self, data: &[u8], pos: &mut usize) -> Result<String, HpackError> {
        if *pos >= data.len() {
            return Err(HpackError::UnexpectedEnd);
        }

        let first = data[*pos];
        let huffman = (first & 0x80) != 0;
        let len = self.decode_integer(data, pos, 7)? as usize;

        if *pos + len > data.len() {
            return Err(HpackError::UnexpectedEnd);
        }

        if huffman {
            let decoded = http2_huffman::decode_huffman(&data[*pos..*pos + len])
                .map_err(|_| HpackError::InvalidHuffman)?;
            let s = core::str::from_utf8(&decoded)
                .map_err(|_| HpackError::InvalidUtf8)?
                .to_string();
            *pos += len;
            return Ok(s);
        }

        let s = core::str::from_utf8(&data[*pos..*pos + len])
            .map_err(|_| HpackError::InvalidUtf8)?
            .to_string();
        *pos += len;

        Ok(s)
    }

    fn decode_integer(
        &self,
        data: &[u8],
        pos: &mut usize,
        prefix_bits: u8,
    ) -> Result<u32, HpackError> {
        if *pos >= data.len() {
            return Err(HpackError::UnexpectedEnd);
        }

        let mask = (1u8 << prefix_bits) - 1;
        let mut value = (data[*pos] & mask) as u32;
        *pos += 1;

        if value < mask as u32 {
            return Ok(value);
        }

        let mut shift = 0;
        loop {
            if *pos >= data.len() {
                return Err(HpackError::UnexpectedEnd);
            }

            let byte = data[*pos];
            *pos += 1;

            value += ((byte & 0x7F) as u32) << shift;
            shift += 7;

            if byte & 0x80 == 0 {
                break;
            }
        }

        Ok(value)
    }
}

/// HPACK Hata Türleri
///
/// Başlık sıkıştırma/çözme sırasında oluşabilecek hatalar.
/// `CompressionError` HTTP/2 bağlantısını sonlandırabilecek ciddi bir hata kodu taşır.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HpackError {
    InvalidIndex,
    InvalidPrefix,
    UnexpectedEnd,
    InvalidUtf8,
    InvalidHuffman,
}

/// HTTP/2 Bağlantısı
///
/// Tek bir TCP bağlantısı üzerinde birden fazla akış yönetir.
/// HPACK encoder/decoder'ı bağlantı ömrü boyunca paylaşır.
///
/// ```
/// HTTP/2 Connection
/// ├── Settings (yerel + uzak)
/// ├── Akışlar BTreeMap<u32, Http2Stream>
/// │   ├── Akış 1: GET /index.html
/// │   ├── Akış 3: GET /style.css
/// │   └── Akış 5: GET /script.js
/// ├── HPACK Encoder (başlık sıkıştırma)
/// ├── HPACK Decoder (başlık çözme)
/// └── Bağlantı penceresi (akış kontrolü)
/// ```
pub struct Http2Connection {
    pub settings: Http2Settings,
    pub streams: BTreeMap<u32, Http2Stream>,
    pub next_stream_id: u32,
    pub window_size: u32,
    pub encoder: HpackEncoder,
    pub decoder: HpackDecoder,
}

impl Http2Connection {
    pub fn new() -> Self {
        Http2Connection {
            settings: Http2Settings::default(),
            streams: BTreeMap::new(),
            next_stream_id: 1, // İstemci tek sayılarla başlar
            window_size: 65535,
            encoder: HpackEncoder::new(4096),
            decoder: HpackDecoder::new(4096),
        }
    }

    /// Yeni bir HTTP/2 akışı oluşturur ve akış ID'sini döner.
    ///
    /// İstemci tarafı tek sayı ID'ler kullanır (1, 3, 5...).
    /// Her yeni akışta ID 2 artırılır.
    pub fn create_stream(&mut self) -> u32 {
        let stream_id = self.next_stream_id;
        self.next_stream_id += 2; // Client-initiated streams use odd numbers
        self.streams.insert(stream_id, Http2Stream::new(stream_id));
        stream_id
    }

    /// Akışı salt okunur olarak döner.
    pub fn get_stream(&self, stream_id: u32) -> Option<&Http2Stream> {
        self.streams.get(&stream_id)
    }

    /// Akışı değiştirilebilir olarak döner.
    pub fn get_stream_mut(&mut self, stream_id: u32) -> Option<&mut Http2Stream> {
        self.streams.get_mut(&stream_id)
    }

    /// Verilen akış üzerinde HTTP isteği için başlık bloğu oluşturur.
    ///
    /// HTTP/2 sözde-başlıkları (pseudo-headers) ':' ile başlar:
    /// - `:method` → HTTP metodu (GET, POST...)
    /// - `:path`   → İstek yolu (/index.html)
    /// - `:scheme` → http veya https
    /// - `:authority` → Host başlığına karşılık gelir
    pub fn build_request(
        &mut self,
        stream_id: u32,
        method: &str,
        path: &str,
        host: &str,
    ) -> Vec<u8> {
        let mut headers = BTreeMap::new();
        headers.insert(":method".to_string(), method.to_string());
        headers.insert(":path".to_string(), path.to_string());
        headers.insert(":scheme".to_string(), "https".to_string());
        headers.insert(":authority".to_string(), host.to_string());
        headers.insert("user-agent".to_string(), "echOS/2.0".to_string());

        self.encoder.encode(&headers)
    }

    /// Gelen HTTP/2 çerçevesini işler.
    ///
    /// Çerçeve türüne göre uygun işlem yapılır:
    /// - SETTINGS: yerel ayarlar güncellenir
    /// - HEADERS: akış başlıkları HPACK ile çözülür
    /// - DATA: akış veri tamponu güncellenir
    /// - WINDOW_UPDATE: akış kontrolü penceresi büyütülür
    /// - RST_STREAM: akış kaldırılır
    /// - GOAWAY: bağlantı kapatma hatası
    pub fn process_frame(&mut self, frame: &Http2Frame) -> Result<(), Http2Error> {
        match frame.frame_type {
            FRAME_SETTINGS => {
                self.process_settings(&frame.payload)?;
            }
            FRAME_HEADERS => {
                let headers = self
                    .decoder
                    .decode(&frame.payload)
                    .map_err(|_| Http2Error::CompressionError)?;
                if let Some(stream) = self.streams.get_mut(&frame.stream_id) {
                    if !stream.received_headers {
                        stream.headers = headers;
                        stream.received_headers = true;
                    } else {
                        for (name, value) in headers {
                            stream.trailers.insert(name, value);
                        }
                    }
                    if frame.is_end_stream() {
                        stream.end_stream = true;
                    }
                }
            }
            FRAME_DATA => {
                if let Some(stream) = self.streams.get_mut(&frame.stream_id) {
                    stream.data.extend_from_slice(&frame.payload);
                    if frame.is_end_stream() {
                        stream.end_stream = true;
                    }
                }
            }
            FRAME_WINDOW_UPDATE => {
                let increment = u32::from_be_bytes([
                    frame.payload[0],
                    frame.payload[1],
                    frame.payload[2],
                    frame.payload[3],
                ]);
                if frame.stream_id == 0 {
                    self.window_size += increment;
                } else if let Some(stream) = self.streams.get_mut(&frame.stream_id) {
                    stream.window_size += increment;
                }
            }
            FRAME_RST_STREAM => {
                if let Some(stream) = self.streams.get_mut(&frame.stream_id) {
                    if frame.payload.len() >= 4 {
                        stream.reset_error = Some(u32::from_be_bytes([
                            frame.payload[0],
                            frame.payload[1],
                            frame.payload[2],
                            frame.payload[3],
                        ]));
                    }
                    stream.end_stream = true;
                    stream.state = StreamState::Closed;
                } else {
                    self.streams.remove(&frame.stream_id);
                }
            }
            FRAME_GOAWAY => {
                return Err(Http2Error::GoAway);
            }
            _ => {}
        }
        Ok(())
    }

    fn process_settings(&mut self, payload: &[u8]) -> Result<(), Http2Error> {
        let mut pos = 0;
        while pos + 6 <= payload.len() {
            let id = u16::from_be_bytes([payload[pos], payload[pos + 1]]);
            let value = u32::from_be_bytes([
                payload[pos + 2],
                payload[pos + 3],
                payload[pos + 4],
                payload[pos + 5],
            ]);
            pos += 6;

            match id {
                SETTINGS_HEADER_TABLE_SIZE => self.settings.header_table_size = value,
                SETTINGS_ENABLE_PUSH => self.settings.enable_push = value != 0,
                SETTINGS_MAX_CONCURRENT_STREAMS => self.settings.max_concurrent_streams = value,
                SETTINGS_INITIAL_WINDOW_SIZE => self.settings.initial_window_size = value,
                SETTINGS_MAX_FRAME_SIZE => self.settings.max_frame_size = value,
                SETTINGS_MAX_HEADER_LIST_SIZE => self.settings.max_header_list_size = value,
                _ => {}
            }
        }
        Ok(())
    }
}

impl Default for Http2Connection {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headers_and_trailers_are_retained_separately() {
        let mut connection = Http2Connection::new();
        let stream_id = connection.create_stream();

        let mut response_headers = BTreeMap::new();
        response_headers.insert(":status".to_string(), "200".to_string());
        response_headers.insert("content-type".to_string(), "application/grpc".to_string());
        let headers_frame = Http2Frame::headers(
            stream_id,
            connection.encoder.encode(&response_headers),
            false,
        );
        connection.process_frame(&headers_frame).unwrap();

        let mut trailer_headers = BTreeMap::new();
        trailer_headers.insert("grpc-status".to_string(), "0".to_string());
        trailer_headers.insert("grpc-message".to_string(), "ok".to_string());
        let trailer_frame =
            Http2Frame::headers(stream_id, connection.encoder.encode(&trailer_headers), true);
        connection.process_frame(&trailer_frame).unwrap();

        let stream = connection.get_stream(stream_id).unwrap();
        assert_eq!(
            stream.headers.get(":status").map(String::as_str),
            Some("200")
        );
        assert_eq!(
            stream.headers.get("content-type").map(String::as_str),
            Some("application/grpc")
        );
        assert_eq!(
            stream.trailers.get("grpc-status").map(String::as_str),
            Some("0")
        );
        assert_eq!(
            stream.trailers.get("grpc-message").map(String::as_str),
            Some("ok")
        );
        assert!(stream.end_stream);
    }

    #[test]
    fn rst_stream_retains_error_code_for_callers() {
        let mut connection = Http2Connection::new();
        let stream_id = connection.create_stream();

        let rst = Http2Frame::rst_stream(stream_id, REFUSED_STREAM);
        connection.process_frame(&rst).unwrap();

        let stream = connection.get_stream(stream_id).unwrap();
        assert_eq!(stream.reset_error, Some(REFUSED_STREAM));
        assert!(stream.end_stream);
        assert_eq!(stream.state, StreamState::Closed);
    }

    #[test]
    fn huffman_literal_string_decodes() {
        let decoder = HpackDecoder::new(4096);
        let mut pos = 0usize;
        let encoded = [0x88, 0x25, 0xa8, 0x49, 0xe9, 0x5b, 0xa9, 0x7d, 0x7f];
        let decoded = decoder.decode_string(&encoded, &mut pos).unwrap();
        assert_eq!(decoded, "custom-key");
        assert_eq!(pos, encoded.len());
    }
}

/// HTTP/2 Hata Türleri
///
/// RFC 7540 bölüm 7'deki tüm hata kodlarını kapsar.
/// `GoAway` tüm bağlantının sonlandığını, diğerleri akış düzeyindeki hataları ifade eder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Http2Error {
    ProtocolError,
    InternalError,
    FlowControlError,
    SettingsTimeout,
    StreamClosed,
    FrameSizeError,
    RefusedStream,
    Cancel,
    CompressionError,
    ConnectError,
    EnhanceYourCalm,
    InadequateSecurity,
    Http11Required,
    GoAway,
}

/// HTTP/2 bağlantı ön sözünü döner.
///
/// İstemci ilk bağlandığında tam olarak bu 24 baytı göndermelidir.
/// Bu, sunucunun HTTP/2 konuşulduğunu anlamasını sağlar.
pub fn connection_preface() -> &'static [u8] {
    CONNECTION_PREFACE
}

/// HTTP/2 istemci - Http2Connection'ın istemci tarafı için alias
pub type Http2Client = Http2Connection;
