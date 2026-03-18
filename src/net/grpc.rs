//! # gRPC (Google Remote Procedure Call)
//!
//! HTTP/2 tabanlı modern RPC framework'ü.
//! Protocol Buffers ile yüksek performanslı servisler arası iletişim sağlar.
//!
//! ## gRPC Nedir?
//!
//! gRPC, Google tarafından geliştirilen, HTTP/2 ve Protocol Buffers kullanan
//! modern bir RPC framework'üdür. Mikroservis mimarilerinde popülerdir.
//!
//! ## gRPC Mimarisi
//!
//! ```text
//!  İstemci                           Sunucu
//!     |                                |
//!     |--- HTTP/2 Request ------------>|  (gRPC over HTTP/2)
//!     |  Content-Type: application/grpc|
//!     |  Protocol Buffer Message       |
//!     |                                |
//!     |<-- HTTP/2 Response -----------|
//!     |  Content-Type: application/grpc|
//!     |  Protocol Buffer Message       |
//! ```
//!
//! ## gRPC Servis Tanımı (.proto)
//!
//! ```protobuf
//! service Greeter {
//!   rpc SayHello (HelloRequest) returns (HelloReply) {}
//!   rpc SayHelloStream (HelloRequest) returns (stream HelloReply) {}
//! }
//!
//! message HelloRequest {
//!   string name = 1;
//! }
//!
//! message HelloReply {
//!   string message = 1;
//! }
//! ```

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use super::http2::{Http2Connection, Http2Error, Http2Frame, Http2Stream};

// ============================================================================
// gRPC SABİTLERİ
// ============================================================================

/// gRPC content-type
pub const GRPC_CONTENT_TYPE: &str = "application/grpc";

/// gRPC mesaj başlığı (5-byte)
/// - 1 byte: flags (compression)
/// - 4 bytes: message length (big-endian)
const GRPC_MESSAGE_HEADER_SIZE: usize = 5;

/// gRPC flags
const GRPC_FLAG_COMPRESSED: u8 = 0x01;

/// gRPC durum kodları
pub const GRPC_STATUS_OK: i32 = 0;
pub const GRPC_STATUS_CANCELLED: i32 = 1;
pub const GRPC_STATUS_UNKNOWN: i32 = 2;
pub const GRPC_STATUS_INVALID_ARGUMENT: i32 = 3;
pub const GRPC_STATUS_DEADLINE_EXCEEDED: i32 = 4;
pub const GRPC_STATUS_NOT_FOUND: i32 = 5;
pub const GRPC_STATUS_ALREADY_EXISTS: i32 = 6;
pub const GRPC_STATUS_PERMISSION_DENIED: i32 = 7;
pub const GRPC_STATUS_UNAUTHENTICATED: i32 = 16;
pub const GRPC_STATUS_UNAVAILABLE: i32 = 14;
pub const GRPC_STATUS_INTERNAL: i32 = 13;

// ============================================================================
// gRPC HATASI
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrpcError {
    Http2Error(Http2Error),
    InvalidMessage,
    SerializationError,
    DeserializationError,
    StatusError(i32),
    DeadlineExceeded,
    Unavailable,
}

impl From<Http2Error> for GrpcError {
    fn from(err: Http2Error) -> Self {
        GrpcError::Http2Error(err)
    }
}

// ============================================================================
// PROTOCOL BUFFER MESSAGE MODEL
// ============================================================================

/// Length-delimited Protocol Buffer field map used by the in-tree unary examples.
#[derive(Clone, Debug)]
pub struct ProtoMessage {
    fields: BTreeMap<u32, Vec<u8>>,
}

impl ProtoMessage {
    /// Yeni ProtoMessage oluştur
    pub fn new() -> Self {
        ProtoMessage {
            fields: BTreeMap::new(),
        }
    }

    /// String alanı ekle
    pub fn add_string(&mut self, field_number: u32, value: &str) {
        let mut data = Vec::new();
        data.extend_from_slice(value.as_bytes());
        self.fields.insert(field_number, data);
    }

    /// String alanı oku
    pub fn get_string(&self, field_number: u32) -> Option<String> {
        self.fields
            .get(&field_number)
            .and_then(|data| String::from_utf8(data.clone()).ok())
    }

    /// Serialize et
    pub fn serialize(&self) -> Vec<u8> {
        let mut result = Vec::new();

        for (&field_number, data) in &self.fields {
            // Wire type 2 (length-delimited) kullan
            let wire_type = 2u8;
            let field_header = (field_number << 3) | (wire_type as u32);

            // Varint olarak field header'ı kodla
            Self::encode_varint(&mut result, field_header as u64);

            // Length'i kodla
            Self::encode_varint(&mut result, data.len() as u64);

            // Veriyi ekle
            result.extend_from_slice(data);
        }

        result
    }

    /// Deserialize et
    pub fn deserialize(data: &[u8]) -> Result<Self, GrpcError> {
        let mut message = ProtoMessage::new();
        let mut offset = 0;

        while offset < data.len() {
            // Field header oku
            let (field_header, header_offset) =
                Self::decode_varint(&data[offset..]).ok_or(GrpcError::InvalidMessage)?;
            offset += header_offset;

            let field_number = (field_header >> 3) as u32;
            let wire_type = (field_header & 0x07) as u8;

            if wire_type != 2 {
                return Err(GrpcError::InvalidMessage);
            }

            // Length oku
            let (length, new_offset) =
                Self::decode_varint(&data[offset..]).ok_or(GrpcError::InvalidMessage)?;
            offset += new_offset;

            if offset + length as usize > data.len() {
                return Err(GrpcError::InvalidMessage);
            }

            let field_data = data[offset..offset + length as usize].to_vec();
            message.fields.insert(field_number, field_data);

            offset += length as usize;
        }

        Ok(message)
    }

    /// Varint kodla
    fn encode_varint(buf: &mut Vec<u8>, mut value: u64) {
        while value >= 0x80 {
            buf.push((value & 0x7F) as u8 | 0x80);
            value >>= 7;
        }
        buf.push(value as u8);
    }

    /// Varint çöz
    fn decode_varint(data: &[u8]) -> Option<(u64, usize)> {
        let mut result = 0u64;
        let mut shift = 0;
        let mut bytes_read = 0;

        for &byte in data {
            bytes_read += 1;
            result |= ((byte & 0x7F) as u64) << shift;

            if byte & 0x80 == 0 {
                return Some((result, bytes_read));
            }

            shift += 7;
            if shift >= 64 {
                return None;
            }
        }

        None
    }
}

impl Default for ProtoMessage {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// gRPC METODU
// ============================================================================

/// gRPC metodu tipi
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrpcMethodType {
    /// Unary: tek istek, tek yanıt
    Unary,
    /// Server streaming: tek istek, stream yanıt
    ServerStreaming,
    /// Client streaming: stream istek, tek yanıt
    ClientStreaming,
    /// Bidirectional streaming: stream istek, stream yanıt
    BidiStreaming,
}

/// gRPC metodu tanımı
#[derive(Clone, Debug)]
pub struct GrpcMethod {
    /// Metod adı
    pub name: String,
    /// Metod tipi
    pub method_type: GrpcMethodType,
    /// İstek tipi (protobuf mesaj adı)
    pub request_type: String,
    /// Yanıt tipi (protobuf mesaj adı)
    pub response_type: String,
}

impl GrpcMethod {
    /// Yeni metod oluştur
    pub fn new(
        name: &str,
        method_type: GrpcMethodType,
        request_type: &str,
        response_type: &str,
    ) -> Self {
        Self {
            name: name.to_string(),
            method_type,
            request_type: request_type.to_string(),
            response_type: response_type.to_string(),
        }
    }
}

// ============================================================================
// gRPC SERVİSİ
// ============================================================================

/// gRPC servisi
#[derive(Clone, Debug)]
pub struct GrpcService {
    /// Servis adı
    pub name: String,
    /// Metodlar
    pub methods: BTreeMap<String, GrpcMethod>,
}

impl GrpcService {
    /// Yeni servis oluştur
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            methods: BTreeMap::new(),
        }
    }

    /// Metod ekle
    pub fn add_method(&mut self, method: GrpcMethod) {
        self.methods.insert(method.name.clone(), method);
    }

    /// Metod al
    pub fn get_method(&self, name: &str) -> Option<&GrpcMethod> {
        self.methods.get(name)
    }
}

// ============================================================================
// gRPC İSTEMCİSİ
// ============================================================================

/// gRPC istemcisi
pub struct GrpcClient {
    /// HTTP/2 istemcisi
    http2_client: Http2Connection,
    /// Servisler
    services: BTreeMap<String, GrpcService>,
    /// Sonraki stream ID
    next_stream_id: AtomicU64,
}

impl GrpcClient {
    /// Yeni gRPC istemcisi oluştur
    pub fn new() -> Self {
        GrpcClient {
            services: BTreeMap::new(),
            http2_client: Http2Connection::new(),
            next_stream_id: AtomicU64::new(1),
        }
    }

    /// Servis ekle
    pub fn add_service(&mut self, service: GrpcService) {
        self.services.insert(service.name.clone(), service);
    }

    /// Unary çağrı yap
    pub fn call_unary(
        &mut self,
        service_name: &str,
        method_name: &str,
        request: &ProtoMessage,
    ) -> Result<ProtoMessage, GrpcError> {
        // Servis ve metodu bul
        let method = self
            .services
            .get(service_name)
            .and_then(|service| service.get_method(method_name))
            .cloned()
            .ok_or(GrpcError::Unavailable)?;

        if method.method_type != GrpcMethodType::Unary {
            return Err(GrpcError::InvalidMessage);
        }

        let stream_id = self.next_stream_id.fetch_add(2, Ordering::SeqCst);
        self.http2_client.create_stream();

        let grpc_request =
            self.create_grpc_request(service_name, &method, "builtin.local", request)?;
        let mut request_header_map = BTreeMap::new();
        for (key, value) in &grpc_request.headers {
            request_header_map.insert(key.clone(), value.clone());
        }
        let encoded_headers = self.http2_client.encoder.encode(&request_header_map);
        let request_headers_frame = Http2Frame::headers(stream_id as u32, encoded_headers, false);
        let request_data_frame =
            Http2Frame::data(stream_id as u32, grpc_request.body.clone(), true);
        self.http2_client.process_frame(&request_headers_frame)?;
        self.http2_client.process_frame(&request_data_frame)?;

        let response_message = self.dispatch_builtin_unary(service_name, method_name, request)?;
        let serialized_response = response_message.serialize();
        let mut response_body =
            Vec::with_capacity(GRPC_MESSAGE_HEADER_SIZE + serialized_response.len());
        response_body.push(0);
        response_body.extend_from_slice(&(serialized_response.len() as u32).to_be_bytes());
        response_body.extend_from_slice(&serialized_response);

        let response_headers = vec![
            (":status".to_string(), "200".to_string()),
            ("content-type".to_string(), GRPC_CONTENT_TYPE.to_string()),
            ("grpc-status".to_string(), GRPC_STATUS_OK.to_string()),
        ];
        let response = self.process_grpc_response(&response_headers, &response_body)?;
        Ok(response)
    }

    pub fn call_unary_remote(
        &mut self,
        server_ip: super::Ipv4Addr,
        port: u16,
        authority: &str,
        service_name: &str,
        method_name: &str,
        request: &ProtoMessage,
    ) -> Result<ProtoMessage, GrpcError> {
        use super::socket::{
            close, connect, recv, send, socket, AddressFamily, Protocol, SocketType,
        };
        use super::{Port, SocketAddr};

        let method = self
            .services
            .get(service_name)
            .and_then(|service| service.get_method(method_name))
            .cloned()
            .ok_or(GrpcError::Unavailable)?;

        if method.method_type != GrpcMethodType::Unary {
            return Err(GrpcError::InvalidMessage);
        }

        let sock_id = socket(AddressFamily::IPV4, SocketType::STREAM, Protocol::TCP)
            .map_err(|_| GrpcError::Unavailable)?;
        connect(sock_id, SocketAddr::new(server_ip, Port(port)))
            .map_err(|_| GrpcError::Unavailable)?;

        let send_result = (|| -> Result<ProtoMessage, GrpcError> {
            send(sock_id, super::http2::connection_preface(), 0)
                .map_err(|_| GrpcError::Unavailable)?;

            let settings = super::http2::Http2Frame::settings(&self.http2_client.settings).encode();
            send(sock_id, &settings, 0).map_err(|_| GrpcError::Unavailable)?;

            let stream_id = self.http2_client.create_stream();
            let grpc_request =
                self.create_grpc_request(service_name, &method, authority, request)?;
            let mut request_header_map = BTreeMap::new();
            for (key, value) in &grpc_request.headers {
                request_header_map.insert(key.clone(), value.clone());
            }
            let encoded_headers = self.http2_client.encoder.encode(&request_header_map);
            let headers_frame = Http2Frame::headers(stream_id, encoded_headers, false).encode();
            let data_frame = Http2Frame::data(stream_id, grpc_request.body.clone(), true).encode();

            send(sock_id, &headers_frame, 0).map_err(|_| GrpcError::Unavailable)?;
            send(sock_id, &data_frame, 0).map_err(|_| GrpcError::Unavailable)?;

            let mut wire = Vec::new();
            let mut recv_buf = [0u8; 8192];
            loop {
                let recv_len =
                    recv(sock_id, &mut recv_buf, 0).map_err(|_| GrpcError::Unavailable)?;
                if recv_len == 0 {
                    break;
                }
                wire.extend_from_slice(&recv_buf[..recv_len]);

                let mut consumed = 0usize;
                while consumed < wire.len() {
                    let Some((frame, used)) = Http2Frame::decode(&wire[consumed..]) else {
                        break;
                    };
                    consumed += used;
                    self.http2_client.process_frame(&frame)?;
                }

                if consumed > 0 {
                    wire.drain(..consumed);
                }

                if let Some(stream) = self.http2_client.get_stream(stream_id) {
                    if stream.end_stream {
                        let headers: Vec<(String, String)> = stream
                            .headers
                            .iter()
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect();
                        return self.process_grpc_response(&headers, &stream.data);
                    }
                }
            }

            Err(GrpcError::Unavailable)
        })();

        let _ = close(sock_id);
        send_result
    }

    /// gRPC isteği oluştur
    fn create_grpc_request(
        &self,
        service_name: &str,
        method: &GrpcMethod,
        authority: &str,
        request: &ProtoMessage,
    ) -> Result<GrpcMessage, GrpcError> {
        let mut headers = Vec::new();
        headers.push((":method".to_string(), "POST".to_string()));
        headers.push((
            ":path".to_string(),
            format!("/{}/{}", service_name, method.name),
        ));
        headers.push((":authority".to_string(), authority.to_string()));
        headers.push(("content-type".to_string(), GRPC_CONTENT_TYPE.to_string()));
        headers.push(("te".to_string(), "trailers".to_string()));

        // Protocol Buffer mesajını serialize et
        let serialized_request = request.serialize();

        // gRPC mesaj başlığı ekle
        let mut grpc_body = Vec::new();
        grpc_body.push(0); // Flags (no compression)
        grpc_body.extend_from_slice(&[
            (serialized_request.len() >> 24) as u8,
            (serialized_request.len() >> 16) as u8,
            (serialized_request.len() >> 8) as u8,
            serialized_request.len() as u8,
        ]);
        grpc_body.extend_from_slice(&serialized_request);

        Ok(GrpcMessage {
            headers,
            body: grpc_body,
        })
    }

    /// gRPC yanıtını işle
    fn process_grpc_response(
        &self,
        headers: &[(String, String)],
        body: &[u8],
    ) -> Result<ProtoMessage, GrpcError> {
        // Content-type kontrolü
        let content_type = headers
            .iter()
            .find(|(k, _)| k == "content-type")
            .map(|(_, v)| v.as_str())
            .unwrap_or("");

        if content_type != GRPC_CONTENT_TYPE {
            return Err(GrpcError::InvalidMessage);
        }

        // gRPC mesaj başlığını oku
        if body.len() < GRPC_MESSAGE_HEADER_SIZE {
            return Err(GrpcError::InvalidMessage);
        }

        let flags = body[0];
        let length = ((body[1] as u32) << 24)
            | ((body[2] as u32) << 16)
            | ((body[3] as u32) << 8)
            | (body[4] as u32);

        if (flags & GRPC_FLAG_COMPRESSED) != 0 {
            return Err(GrpcError::InvalidMessage); // Compression desteklenmiyor
        }

        if body.len() < GRPC_MESSAGE_HEADER_SIZE + length as usize {
            return Err(GrpcError::InvalidMessage);
        }

        let message_data =
            &body[GRPC_MESSAGE_HEADER_SIZE..GRPC_MESSAGE_HEADER_SIZE + length as usize];

        // Protocol Buffer mesajını deserialize et
        ProtoMessage::deserialize(message_data).map_err(|_| GrpcError::DeserializationError)
    }

    fn dispatch_builtin_unary(
        &self,
        service_name: &str,
        method_name: &str,
        request: &ProtoMessage,
    ) -> Result<ProtoMessage, GrpcError> {
        match (service_name, method_name) {
            ("Greeter", "SayHello") => {
                let name = request.get_string(1).unwrap_or_else(|| "World".to_string());
                let mut reply = ProtoMessage::new();
                reply.add_string(1, &format!("Hello, {}!", name));
                Ok(reply)
            }
            _ => Err(GrpcError::Unavailable),
        }
    }
}

impl Default for GrpcClient {
    fn default() -> Self {
        Self::new()
    }
}

/// gRPC mesajı
#[derive(Clone, Debug)]
struct GrpcMessage {
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

// ============================================================================
// gRPC SUNUCUSU
// ============================================================================

/// gRPC sunucusu
pub struct GrpcServer {
    /// HTTP/2 sunucusu
    http2_server: Http2Server,
    /// Servisler
    services: BTreeMap<String, GrpcService>,
}

/// HTTP/2 sunucusu.
///
/// Remote transport kapanana kadar server yolu basari taklidi yapmaz.
pub struct Http2Server {}

impl Http2Server {
    pub fn new() -> Self {
        Self {}
    }

    pub fn listen(&mut self, port: u16) -> Result<(), GrpcError> {
        crate::serial_println!(
            "[gRPC] Remote HTTP/2 server transport unavailable on port {}",
            port
        );
        Err(GrpcError::Unavailable)
    }

    pub fn accept(&mut self) -> Result<GrpcConnection, GrpcError> {
        // Remote HTTP/2 accept path is not wired yet.
        Err(GrpcError::Unavailable)
    }
}

/// gRPC bağlantısı
pub struct GrpcConnection {}

impl GrpcConnection {
    pub fn new() -> Self {
        Self {}
    }

    pub fn handle_request(
        &mut self,
        services: &BTreeMap<String, GrpcService>,
    ) -> Result<(), GrpcError> {
        // Remote HTTP/2 request handling is not wired yet.
        let _ = services;
        Err(GrpcError::Unavailable)
    }
}

impl GrpcServer {
    /// Yeni gRPC sunucusu oluştur
    pub fn new() -> Self {
        Self {
            http2_server: Http2Server::new(),
            services: BTreeMap::new(),
        }
    }

    /// Servis ekle
    pub fn add_service(&mut self, service: GrpcService) {
        self.services.insert(service.name.clone(), service);
    }

    /// Sunucuyu başlat
    pub fn serve(&mut self, port: u16) -> Result<(), GrpcError> {
        self.http2_server.listen(port)?;

        loop {
            let mut connection = self.http2_server.accept()?;
            connection.handle_request(&self.services)?;
        }
    }
}

impl Default for GrpcServer {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// MODÜL BAŞLATMA
// ============================================================================

/// gRPC modülünü başlat
pub fn init() {
    crate::serial_println!("[gRPC] gRPC module initialized");
}

/// In-tree Greeter servisini oluştur
pub fn create_greeter_service() -> GrpcService {
    let mut service = GrpcService::new("Greeter");

    let say_hello_method = GrpcMethod::new(
        "SayHello",
        GrpcMethodType::Unary,
        "HelloRequest",
        "HelloReply",
    );

    service.add_method(say_hello_method);

    service
}

/// In-tree Greeter istemci yolu
pub fn test_greeter_client() -> Result<(), GrpcError> {
    let mut client = GrpcClient::new();

    // Servis ekle
    client.add_service(create_greeter_service());

    // İstek oluştur
    let mut request = ProtoMessage::new();
    request.add_string(1, "World");

    // Çağrı yap
    let response = client.call_unary("Greeter", "SayHello", &request)?;

    // Yanıtı kontrol et
    if let Some(message) = response.get_string(1) {
        crate::serial_println!("[gRPC] Response: {}", message);
    }

    Ok(())
}
