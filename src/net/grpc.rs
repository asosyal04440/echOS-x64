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

use super::http2::{Http2Connection, Http2Error, Http2Frame};

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GrpcError {
    Http2Error(Http2Error),
    InvalidMessage,
    SerializationError,
    DeserializationError,
    StatusError(i32),
    StatusMessage(i32, String),
    HttpStatus(u16),
    ResetStream(u32),
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
#[derive(Clone, Debug, PartialEq, Eq)]
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrpcStreamingResponse {
    pub headers: BTreeMap<String, String>,
    pub trailers: BTreeMap<String, String>,
    pub messages: Vec<ProtoMessage>,
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

        let mut response_headers = BTreeMap::new();
        response_headers.insert(":status".to_string(), "200".to_string());
        response_headers.insert("content-type".to_string(), GRPC_CONTENT_TYPE.to_string());

        let mut response_trailers = BTreeMap::new();
        response_trailers.insert("grpc-status".to_string(), GRPC_STATUS_OK.to_string());

        let response = self.process_grpc_response(
            &response_headers,
            Some(&response_trailers),
            &response_body,
        )?;
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
                        if let Some(reset_error) = stream.reset_error {
                            return Err(GrpcError::ResetStream(reset_error));
                        }
                        return self.process_grpc_response(
                            &stream.headers,
                            Some(&stream.trailers),
                            &stream.data,
                        );
                    }
                }
            }

            Err(GrpcError::Unavailable)
        })();

        let _ = close(sock_id);
        send_result
    }

    pub fn call_server_streaming(
        &mut self,
        service_name: &str,
        method_name: &str,
        request: &ProtoMessage,
    ) -> Result<Vec<ProtoMessage>, GrpcError> {
        let method = self
            .services
            .get(service_name)
            .and_then(|service| service.get_method(method_name))
            .cloned()
            .ok_or(GrpcError::Unavailable)?;

        if method.method_type != GrpcMethodType::ServerStreaming {
            return Err(GrpcError::InvalidMessage);
        }

        let messages =
            self.dispatch_builtin_server_streaming(service_name, method_name, request)?;
        let body = Self::encode_grpc_message_stream(&messages);

        let mut response_headers = BTreeMap::new();
        response_headers.insert(":status".to_string(), "200".to_string());
        response_headers.insert("content-type".to_string(), GRPC_CONTENT_TYPE.to_string());

        let mut response_trailers = BTreeMap::new();
        response_trailers.insert("grpc-status".to_string(), GRPC_STATUS_OK.to_string());

        let response =
            self.process_grpc_stream_response(&response_headers, Some(&response_trailers), &body)?;
        Ok(response.messages)
    }

    pub fn call_server_streaming_remote(
        &mut self,
        server_ip: super::Ipv4Addr,
        port: u16,
        authority: &str,
        service_name: &str,
        method_name: &str,
        request: &ProtoMessage,
    ) -> Result<GrpcStreamingResponse, GrpcError> {
        let method = self
            .services
            .get(service_name)
            .and_then(|service| service.get_method(method_name))
            .cloned()
            .ok_or(GrpcError::Unavailable)?;

        if method.method_type != GrpcMethodType::ServerStreaming {
            return Err(GrpcError::InvalidMessage);
        }

        self.execute_remote_call(
            server_ip,
            port,
            authority,
            service_name,
            &method,
            &Self::encode_grpc_message_stream(core::slice::from_ref(request)),
            true,
        )
    }

    pub fn call_client_streaming(
        &mut self,
        service_name: &str,
        method_name: &str,
        requests: &[ProtoMessage],
    ) -> Result<ProtoMessage, GrpcError> {
        let method = self
            .services
            .get(service_name)
            .and_then(|service| service.get_method(method_name))
            .cloned()
            .ok_or(GrpcError::Unavailable)?;

        if method.method_type != GrpcMethodType::ClientStreaming {
            return Err(GrpcError::InvalidMessage);
        }

        self.dispatch_builtin_client_streaming(service_name, method_name, requests)
    }

    pub fn call_bidi_streaming(
        &mut self,
        service_name: &str,
        method_name: &str,
        requests: &[ProtoMessage],
    ) -> Result<Vec<ProtoMessage>, GrpcError> {
        let method = self
            .services
            .get(service_name)
            .and_then(|service| service.get_method(method_name))
            .cloned()
            .ok_or(GrpcError::Unavailable)?;

        if method.method_type != GrpcMethodType::BidiStreaming {
            return Err(GrpcError::InvalidMessage);
        }

        self.dispatch_builtin_bidi_streaming(service_name, method_name, requests)
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
        headers: &BTreeMap<String, String>,
        trailers: Option<&BTreeMap<String, String>>,
        body: &[u8],
    ) -> Result<ProtoMessage, GrpcError> {
        let content_type = headers
            .get("content-type")
            .map(String::as_str)
            .unwrap_or("");
        if !Self::is_grpc_content_type(content_type) {
            return Err(GrpcError::InvalidMessage);
        }

        if let Some(http_status) = Self::parse_http_status(headers) {
            if http_status != 200 {
                return Err(GrpcError::HttpStatus(http_status));
            }
        }

        if let Some((grpc_status, grpc_message)) = Self::parse_grpc_status(headers, trailers)? {
            if grpc_status != GRPC_STATUS_OK {
                return if let Some(message) = grpc_message {
                    Err(GrpcError::StatusMessage(grpc_status, message))
                } else {
                    Err(GrpcError::StatusError(grpc_status))
                };
            }
        }

        if body.is_empty() {
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
            return Err(GrpcError::InvalidMessage);
        }

        if body.len() < GRPC_MESSAGE_HEADER_SIZE + length as usize {
            return Err(GrpcError::InvalidMessage);
        }

        let message_data =
            &body[GRPC_MESSAGE_HEADER_SIZE..GRPC_MESSAGE_HEADER_SIZE + length as usize];

        // Protocol Buffer mesajını deserialize et
        ProtoMessage::deserialize(message_data).map_err(|_| GrpcError::DeserializationError)
    }

    fn process_grpc_stream_response(
        &self,
        headers: &BTreeMap<String, String>,
        trailers: Option<&BTreeMap<String, String>>,
        body: &[u8],
    ) -> Result<GrpcStreamingResponse, GrpcError> {
        let content_type = headers
            .get("content-type")
            .map(String::as_str)
            .unwrap_or("");
        if !Self::is_grpc_content_type(content_type) {
            return Err(GrpcError::InvalidMessage);
        }

        if let Some(http_status) = Self::parse_http_status(headers) {
            if http_status != 200 {
                return Err(GrpcError::HttpStatus(http_status));
            }
        }

        if let Some((grpc_status, grpc_message)) = Self::parse_grpc_status(headers, trailers)? {
            if grpc_status != GRPC_STATUS_OK {
                return if let Some(message) = grpc_message {
                    Err(GrpcError::StatusMessage(grpc_status, message))
                } else {
                    Err(GrpcError::StatusError(grpc_status))
                };
            }
        }

        let messages = Self::decode_grpc_message_stream(body)?;
        Ok(GrpcStreamingResponse {
            headers: headers.clone(),
            trailers: trailers.cloned().unwrap_or_default(),
            messages,
        })
    }

    fn encode_grpc_message_stream(messages: &[ProtoMessage]) -> Vec<u8> {
        let mut body = Vec::new();
        for message in messages {
            let payload = message.serialize();
            body.push(0);
            body.extend_from_slice(&(payload.len() as u32).to_be_bytes());
            body.extend_from_slice(&payload);
        }
        body
    }

    fn decode_grpc_message_stream(body: &[u8]) -> Result<Vec<ProtoMessage>, GrpcError> {
        let mut offset = 0usize;
        let mut messages = Vec::new();

        while offset < body.len() {
            if body.len() - offset < GRPC_MESSAGE_HEADER_SIZE {
                return Err(GrpcError::InvalidMessage);
            }

            let flags = body[offset];
            let length = ((body[offset + 1] as u32) << 24)
                | ((body[offset + 2] as u32) << 16)
                | ((body[offset + 3] as u32) << 8)
                | (body[offset + 4] as u32);
            offset += GRPC_MESSAGE_HEADER_SIZE;

            if (flags & GRPC_FLAG_COMPRESSED) != 0 {
                return Err(GrpcError::InvalidMessage);
            }

            if offset + length as usize > body.len() {
                return Err(GrpcError::InvalidMessage);
            }

            messages.push(
                ProtoMessage::deserialize(&body[offset..offset + length as usize])
                    .map_err(|_| GrpcError::DeserializationError)?,
            );
            offset += length as usize;
        }

        Ok(messages)
    }

    fn is_grpc_content_type(content_type: &str) -> bool {
        let lower = content_type.to_ascii_lowercase();
        lower == GRPC_CONTENT_TYPE
            || lower.starts_with("application/grpc+")
            || lower.starts_with("application/grpc;")
    }

    fn parse_http_status(headers: &BTreeMap<String, String>) -> Option<u16> {
        headers
            .get(":status")
            .and_then(|status| status.parse::<u16>().ok())
    }

    fn parse_grpc_status(
        headers: &BTreeMap<String, String>,
        trailers: Option<&BTreeMap<String, String>>,
    ) -> Result<Option<(i32, Option<String>)>, GrpcError> {
        let trailer_status = trailers.and_then(|map| map.get("grpc-status"));
        let header_status = headers.get("grpc-status");
        let status_value = trailer_status.or(header_status);
        let status = match status_value {
            Some(value) => value
                .parse::<i32>()
                .map_err(|_| GrpcError::InvalidMessage)?,
            None => return Ok(None),
        };

        let message = trailers
            .and_then(|map| map.get("grpc-message"))
            .or_else(|| headers.get("grpc-message"))
            .map(|value| Self::decode_grpc_status_message(value));

        Ok(Some((status, message)))
    }

    fn decode_grpc_status_message(raw: &str) -> String {
        let bytes = raw.as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0usize;
        while i < bytes.len() {
            if bytes[i] == b'%' && i + 2 < bytes.len() {
                let hi = bytes[i + 1];
                let lo = bytes[i + 2];
                if let (Some(hi), Some(lo)) =
                    (Self::decode_hex_nibble(hi), Self::decode_hex_nibble(lo))
                {
                    out.push((hi << 4) | lo);
                    i += 3;
                    continue;
                }
            }
            out.push(bytes[i]);
            i += 1;
        }
        String::from_utf8(out).unwrap_or_else(|_| raw.to_string())
    }

    fn decode_hex_nibble(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            _ => None,
        }
    }

    fn execute_remote_call(
        &mut self,
        server_ip: super::Ipv4Addr,
        port: u16,
        authority: &str,
        service_name: &str,
        method: &GrpcMethod,
        request_body: &[u8],
        expect_stream: bool,
    ) -> Result<GrpcStreamingResponse, GrpcError> {
        #[cfg(all(test, target_os = "windows"))]
        {
            return self.execute_remote_call_host(
                server_ip,
                port,
                authority,
                service_name,
                method,
                request_body,
                expect_stream,
            );
        }

        use super::socket::{
            close, connect, recv, send, socket, AddressFamily, Protocol, SocketType,
        };
        use super::{Port, SocketAddr};

        let sock_id = socket(AddressFamily::IPV4, SocketType::STREAM, Protocol::TCP)
            .map_err(|_| GrpcError::Unavailable)?;
        connect(sock_id, SocketAddr::new(server_ip, Port(port)))
            .map_err(|_| GrpcError::Unavailable)?;

        let send_result = (|| -> Result<GrpcStreamingResponse, GrpcError> {
            send(sock_id, super::http2::connection_preface(), 0)
                .map_err(|_| GrpcError::Unavailable)?;

            let settings = super::http2::Http2Frame::settings(&self.http2_client.settings).encode();
            send(sock_id, &settings, 0).map_err(|_| GrpcError::Unavailable)?;

            let stream_id = self.http2_client.create_stream();
            let mut grpc_request =
                self.create_grpc_request(service_name, method, authority, &ProtoMessage::new())?;
            grpc_request.body = request_body.to_vec();

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
                        if let Some(reset_error) = stream.reset_error {
                            return Err(GrpcError::ResetStream(reset_error));
                        }
                        let response = self.process_grpc_stream_response(
                            &stream.headers,
                            Some(&stream.trailers),
                            &stream.data,
                        )?;
                        if !expect_stream && response.messages.len() != 1 {
                            return Err(GrpcError::InvalidMessage);
                        }
                        return Ok(response);
                    }
                }
            }

            Err(GrpcError::Unavailable)
        })();

        let _ = close(sock_id);
        send_result
    }

    #[cfg(all(test, target_os = "windows"))]
    fn execute_remote_call_host(
        &mut self,
        server_ip: super::Ipv4Addr,
        port: u16,
        authority: &str,
        service_name: &str,
        method: &GrpcMethod,
        request_body: &[u8],
        expect_stream: bool,
    ) -> Result<GrpcStreamingResponse, GrpcError> {
        use std::io::{Read, Write};
        use std::net::{Shutdown, SocketAddrV4, TcpStream};
        use std::time::Duration;

        let octets = server_ip.0;
        let server_ip = std::net::Ipv4Addr::new(octets[0], octets[1], octets[2], octets[3]);
        let mut stream = TcpStream::connect(SocketAddrV4::new(server_ip, port))
            .map_err(|_| GrpcError::Unavailable)?;
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .map_err(|_| GrpcError::Unavailable)?;
        stream
            .write_all(super::http2::connection_preface())
            .map_err(|_| GrpcError::Unavailable)?;

        let settings = super::http2::Http2Frame::settings(&self.http2_client.settings).encode();
        stream
            .write_all(&settings)
            .map_err(|_| GrpcError::Unavailable)?;

        let stream_id = self.http2_client.create_stream();
        let mut grpc_request =
            self.create_grpc_request(service_name, method, authority, &ProtoMessage::new())?;
        grpc_request.body = request_body.to_vec();

        let mut request_header_map = BTreeMap::new();
        for (key, value) in &grpc_request.headers {
            request_header_map.insert(key.clone(), value.clone());
        }
        let encoded_headers = self.http2_client.encoder.encode(&request_header_map);
        let headers_frame = Http2Frame::headers(stream_id, encoded_headers, false).encode();
        let data_frame = Http2Frame::data(stream_id, grpc_request.body.clone(), true).encode();

        stream
            .write_all(&headers_frame)
            .map_err(|_| GrpcError::Unavailable)?;
        stream
            .write_all(&data_frame)
            .map_err(|_| GrpcError::Unavailable)?;
        stream.flush().map_err(|_| GrpcError::Unavailable)?;

        let mut wire = Vec::new();
        let mut recv_buf = [0u8; 8192];
        loop {
            let recv_len = match stream.read(&mut recv_buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(err)
                    if matches!(
                        err.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    break;
                }
                Err(_) => return Err(GrpcError::Unavailable),
            };
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

            if let Some(stream_state) = self.http2_client.get_stream(stream_id) {
                if stream_state.end_stream {
                    if let Some(reset_error) = stream_state.reset_error {
                        return Err(GrpcError::ResetStream(reset_error));
                    }
                    let response = self.process_grpc_stream_response(
                        &stream_state.headers,
                        Some(&stream_state.trailers),
                        &stream_state.data,
                    )?;
                    if !expect_stream && response.messages.len() != 1 {
                        return Err(GrpcError::InvalidMessage);
                    }
                    let _ = stream.shutdown(Shutdown::Both);
                    return Ok(response);
                }
            }
        }

        let _ = stream.shutdown(Shutdown::Both);
        Err(GrpcError::Unavailable)
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

    fn dispatch_builtin_server_streaming(
        &self,
        service_name: &str,
        method_name: &str,
        request: &ProtoMessage,
    ) -> Result<Vec<ProtoMessage>, GrpcError> {
        match (service_name, method_name) {
            ("Greeter", "SayHelloStream") => {
                let name = request.get_string(1).unwrap_or_else(|| "World".to_string());
                let mut first = ProtoMessage::new();
                first.add_string(1, &format!("Hello, {}!", name));
                let mut second = ProtoMessage::new();
                second.add_string(1, &format!("Still here, {}.", name));
                Ok(vec![first, second])
            }
            _ => Err(GrpcError::Unavailable),
        }
    }

    fn dispatch_builtin_client_streaming(
        &self,
        service_name: &str,
        method_name: &str,
        requests: &[ProtoMessage],
    ) -> Result<ProtoMessage, GrpcError> {
        match (service_name, method_name) {
            ("Greeter", "CollectHello") => {
                let mut names = Vec::new();
                for request in requests {
                    if let Some(name) = request.get_string(1) {
                        names.push(name);
                    }
                }
                let mut reply = ProtoMessage::new();
                reply.add_string(1, &format!("Collected {} names", names.len()));
                Ok(reply)
            }
            _ => Err(GrpcError::Unavailable),
        }
    }

    fn dispatch_builtin_bidi_streaming(
        &self,
        service_name: &str,
        method_name: &str,
        requests: &[ProtoMessage],
    ) -> Result<Vec<ProtoMessage>, GrpcError> {
        match (service_name, method_name) {
            ("Greeter", "EchoHelloBidi") => {
                let mut replies = Vec::new();
                for request in requests {
                    let mut reply = ProtoMessage::new();
                    let name = request.get_string(1).unwrap_or_else(|| "World".to_string());
                    reply.add_string(1, &format!("Echo {}", name));
                    replies.push(reply);
                }
                Ok(replies)
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
    let say_hello_stream_method = GrpcMethod::new(
        "SayHelloStream",
        GrpcMethodType::ServerStreaming,
        "HelloRequest",
        "HelloReply",
    );
    let collect_hello_method = GrpcMethod::new(
        "CollectHello",
        GrpcMethodType::ClientStreaming,
        "HelloRequest",
        "HelloReply",
    );
    let echo_hello_bidi_method = GrpcMethod::new(
        "EchoHelloBidi",
        GrpcMethodType::BidiStreaming,
        "HelloRequest",
        "HelloReply",
    );

    service.add_method(say_hello_method);
    service.add_method(say_hello_stream_method);
    service.add_method(collect_hello_method);
    service.add_method(echo_hello_bidi_method);

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::http2::{connection_preface, HpackEncoder, Http2Connection, Http2Frame};
    use std::io::{Read, Write};
    use std::net::{Ipv4Addr, Shutdown, SocketAddrV4, TcpListener};
    use std::thread;
    use std::time::Duration;

    fn encode_grpc_body(message: &ProtoMessage) -> Vec<u8> {
        let payload = message.serialize();
        let mut body = Vec::with_capacity(GRPC_MESSAGE_HEADER_SIZE + payload.len());
        body.push(0);
        body.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        body.extend_from_slice(&payload);
        body
    }

    #[test]
    fn grpc_response_prefers_trailer_status_and_message() {
        let client = GrpcClient::new();
        let mut headers = BTreeMap::new();
        headers.insert(":status".to_string(), "200".to_string());
        headers.insert(
            "content-type".to_string(),
            "application/grpc+proto".to_string(),
        );
        headers.insert("grpc-status".to_string(), "0".to_string());

        let mut trailers = BTreeMap::new();
        trailers.insert(
            "grpc-status".to_string(),
            GRPC_STATUS_UNAVAILABLE.to_string(),
        );
        trailers.insert("grpc-message".to_string(), "backend%20draining".to_string());

        let err = client
            .process_grpc_response(&headers, Some(&trailers), &[])
            .unwrap_err();
        assert_eq!(
            err,
            GrpcError::StatusMessage(GRPC_STATUS_UNAVAILABLE, "backend draining".to_string())
        );
    }

    #[test]
    fn grpc_response_accepts_header_status_and_trailer_ok() {
        let client = GrpcClient::new();
        let mut headers = BTreeMap::new();
        headers.insert(":status".to_string(), "200".to_string());
        headers.insert("content-type".to_string(), "application/grpc".to_string());

        let mut trailers = BTreeMap::new();
        trailers.insert("grpc-status".to_string(), GRPC_STATUS_OK.to_string());

        let mut message = ProtoMessage::new();
        message.add_string(1, "hello");
        let body = encode_grpc_body(&message);

        let decoded = client
            .process_grpc_response(&headers, Some(&trailers), &body)
            .unwrap();
        assert_eq!(decoded.get_string(1).as_deref(), Some("hello"));
    }

    #[test]
    fn grpc_remote_transport_retains_http2_trailers_and_reset_reason() {
        let mut client = GrpcClient::new();
        let stream_id = client.http2_client.create_stream();

        let mut headers = BTreeMap::new();
        headers.insert(":status".to_string(), "200".to_string());
        headers.insert("content-type".to_string(), "application/grpc".to_string());
        let headers_frame = Http2Frame::headers(
            stream_id,
            client.http2_client.encoder.encode(&headers),
            false,
        );
        client.http2_client.process_frame(&headers_frame).unwrap();

        let rst = Http2Frame::rst_stream(stream_id, 0x07);
        client.http2_client.process_frame(&rst).unwrap();

        let stream = client.http2_client.get_stream(stream_id).unwrap();
        assert_eq!(stream.reset_error, Some(0x07));
        assert!(stream.end_stream);
    }

    #[test]
    fn grpc_server_streaming_builtin_returns_multiple_messages() {
        let mut client = GrpcClient::new();
        client.add_service(create_greeter_service());

        let mut request = ProtoMessage::new();
        request.add_string(1, "Titan");
        let replies = client
            .call_server_streaming("Greeter", "SayHelloStream", &request)
            .unwrap();

        assert_eq!(replies.len(), 2);
        assert_eq!(replies[0].get_string(1).as_deref(), Some("Hello, Titan!"));
        assert_eq!(
            replies[1].get_string(1).as_deref(),
            Some("Still here, Titan.")
        );
    }

    #[test]
    fn grpc_client_streaming_builtin_aggregates_requests() {
        let mut client = GrpcClient::new();
        client.add_service(create_greeter_service());

        let mut first = ProtoMessage::new();
        first.add_string(1, "A");
        let mut second = ProtoMessage::new();
        second.add_string(1, "B");

        let reply = client
            .call_client_streaming("Greeter", "CollectHello", &[first, second])
            .unwrap();
        assert_eq!(reply.get_string(1).as_deref(), Some("Collected 2 names"));
    }

    #[test]
    fn grpc_bidi_streaming_builtin_echoes_each_message() {
        let mut client = GrpcClient::new();
        client.add_service(create_greeter_service());

        let mut first = ProtoMessage::new();
        first.add_string(1, "A");
        let mut second = ProtoMessage::new();
        second.add_string(1, "B");

        let replies = client
            .call_bidi_streaming("Greeter", "EchoHelloBidi", &[first, second])
            .unwrap();
        assert_eq!(replies.len(), 2);
        assert_eq!(replies[0].get_string(1).as_deref(), Some("Echo A"));
        assert_eq!(replies[1].get_string(1).as_deref(), Some("Echo B"));
    }

    #[test]
    fn grpc_stream_response_decodes_multiple_messages() {
        let client = GrpcClient::new();
        let mut headers = BTreeMap::new();
        headers.insert(":status".to_string(), "200".to_string());
        headers.insert("content-type".to_string(), "application/grpc".to_string());
        let mut trailers = BTreeMap::new();
        trailers.insert("grpc-status".to_string(), GRPC_STATUS_OK.to_string());

        let mut first = ProtoMessage::new();
        first.add_string(1, "one");
        let mut second = ProtoMessage::new();
        second.add_string(1, "two");
        let body = GrpcClient::encode_grpc_message_stream(&[first, second]);

        let response = client
            .process_grpc_stream_response(&headers, Some(&trailers), &body)
            .unwrap();
        assert_eq!(response.messages.len(), 2);
        assert_eq!(response.messages[0].get_string(1).as_deref(), Some("one"));
        assert_eq!(response.messages[1].get_string(1).as_deref(), Some("two"));
    }

    #[test]
    fn grpc_remote_server_streaming_loopback_preserves_messages_and_trailers() {
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();

            let mut request_wire = Vec::new();
            let mut recv_buf = [0u8; 4096];
            while request_wire.len() < connection_preface().len() {
                let read_len = stream.read(&mut recv_buf).unwrap();
                if read_len == 0 {
                    break;
                }
                request_wire.extend_from_slice(&recv_buf[..read_len]);
            }
            assert_eq!(
                &request_wire[..connection_preface().len()],
                connection_preface()
            );

            let mut request_conn = Http2Connection::new();
            let request_stream_id = request_conn.create_stream();
            let mut frame_wire = request_wire.split_off(connection_preface().len());
            loop {
                let mut consumed = 0usize;
                while consumed < frame_wire.len() {
                    let Some((frame, used)) = Http2Frame::decode(&frame_wire[consumed..]) else {
                        break;
                    };
                    request_conn.process_frame(&frame).unwrap();
                    consumed += used;
                }
                if consumed > 0 {
                    frame_wire.drain(..consumed);
                }
                if request_conn
                    .get_stream(request_stream_id)
                    .is_some_and(|state| state.end_stream && !state.data.is_empty())
                {
                    break;
                }
                let read_len = stream.read(&mut recv_buf).unwrap();
                if read_len == 0 {
                    break;
                }
                frame_wire.extend_from_slice(&recv_buf[..read_len]);
            }

            let settings =
                Http2Frame::settings(&crate::net::http2::Http2Settings::default()).encode();
            stream.write_all(&settings).unwrap();

            let mut headers = BTreeMap::new();
            headers.insert(":status".to_string(), "200".to_string());
            headers.insert("content-type".to_string(), "application/grpc".to_string());
            let mut encoder = HpackEncoder::new(4096);
            let headers_frame =
                Http2Frame::headers(request_stream_id, encoder.encode(&headers), false).encode();

            let mut first = ProtoMessage::new();
            first.add_string(1, "Hello, remote stream!");
            let mut second = ProtoMessage::new();
            second.add_string(1, "Still streaming.");
            let data_frame = Http2Frame::data(
                request_stream_id,
                GrpcClient::encode_grpc_message_stream(&[first, second]),
                false,
            )
            .encode();

            let mut trailers = BTreeMap::new();
            trailers.insert("grpc-status".to_string(), GRPC_STATUS_OK.to_string());
            trailers.insert("grpc-message".to_string(), "all%20good".to_string());
            let trailer_frame =
                Http2Frame::headers(request_stream_id, encoder.encode(&trailers), true).encode();

            stream.write_all(&headers_frame).unwrap();
            stream.write_all(&data_frame).unwrap();
            stream.write_all(&trailer_frame).unwrap();
            stream.flush().unwrap();
            let _ = stream.shutdown(Shutdown::Write);
        });

        thread::sleep(Duration::from_millis(50));

        let mut client = GrpcClient::new();
        client.add_service(create_greeter_service());
        let mut request = ProtoMessage::new();
        request.add_string(1, "Titan");
        let response = client
            .call_server_streaming_remote(
                crate::net::Ipv4Addr::new(127, 0, 0, 1),
                port,
                &format!("127.0.0.1:{port}"),
                "Greeter",
                "SayHelloStream",
                &request,
            )
            .unwrap();

        server.join().unwrap();

        assert_eq!(response.messages.len(), 2);
        assert_eq!(
            response.messages[0].get_string(1).as_deref(),
            Some("Hello, remote stream!")
        );
        assert_eq!(
            response.messages[1].get_string(1).as_deref(),
            Some("Still streaming.")
        );
        assert_eq!(
            response.trailers.get("grpc-status").map(String::as_str),
            Some("0")
        );
        assert_eq!(
            response.trailers.get("grpc-message").map(String::as_str),
            Some("all%20good")
        );
    }
}
