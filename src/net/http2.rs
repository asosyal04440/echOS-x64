//! # HTTP/2 Protocol
//!
//! HTTP/2 with multiplexing, HPACK header compression, and stream prioritization.

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::vec;
use spin::Mutex;

// HTTP/2 Frame Types
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

// HTTP/2 Settings
const SETTINGS_HEADER_TABLE_SIZE: u16 = 0x01;
const SETTINGS_ENABLE_PUSH: u16 = 0x02;
const SETTINGS_MAX_CONCURRENT_STREAMS: u16 = 0x03;
const SETTINGS_INITIAL_WINDOW_SIZE: u16 = 0x04;
const SETTINGS_MAX_FRAME_SIZE: u16 = 0x05;
const SETTINGS_MAX_HEADER_LIST_SIZE: u16 = 0x06;

// HTTP/2 Error Codes
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

// HTTP/2 Connection Preface
const CONNECTION_PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

/// HTTP/2 Frame
#[derive(Clone, Debug)]
pub struct Http2Frame {
    pub length: u32,
    pub frame_type: u8,
    pub flags: u8,
    pub stream_id: u32,
    pub payload: Vec<u8>,
}

impl Http2Frame {
    /// Create new frame
    pub fn new(frame_type: u8, stream_id: u32, payload: Vec<u8>) -> Self {
        Http2Frame {
            length: payload.len() as u32,
            frame_type,
            flags: 0,
            stream_id,
            payload,
        }
    }

    /// Create SETTINGS frame
    pub fn settings(settings: &Http2Settings) -> Self {
        let mut payload = Vec::new();
        
        // Header table size
        payload.extend_from_slice(&SETTINGS_HEADER_TABLE_SIZE.to_be_bytes()[2..]);
        payload.extend_from_slice(&settings.header_table_size.to_be_bytes());
        
        // Enable push
        payload.extend_from_slice(&SETTINGS_ENABLE_PUSH.to_be_bytes()[2..]);
        payload.extend_from_slice(&(settings.enable_push as u32).to_be_bytes());
        
        // Max concurrent streams
        payload.extend_from_slice(&SETTINGS_MAX_CONCURRENT_STREAMS.to_be_bytes()[2..]);
        payload.extend_from_slice(&settings.max_concurrent_streams.to_be_bytes());
        
        // Initial window size
        payload.extend_from_slice(&SETTINGS_INITIAL_WINDOW_SIZE.to_be_bytes()[2..]);
        payload.extend_from_slice(&settings.initial_window_size.to_be_bytes());
        
        // Max frame size
        payload.extend_from_slice(&SETTINGS_MAX_FRAME_SIZE.to_be_bytes()[2..]);
        payload.extend_from_slice(&settings.max_frame_size.to_be_bytes());
        
        Http2Frame::new(FRAME_SETTINGS, 0, payload)
    }

    /// Create HEADERS frame
    pub fn headers(stream_id: u32, header_block: Vec<u8>, end_stream: bool) -> Self {
        let mut frame = Http2Frame::new(FRAME_HEADERS, stream_id, header_block);
        if end_stream {
            frame.flags |= 0x01; // END_STREAM
        }
        frame.flags |= 0x04; // END_HEADERS
        frame
    }

    /// Create DATA frame
    pub fn data(stream_id: u32, data: Vec<u8>, end_stream: bool) -> Self {
        let mut frame = Http2Frame::new(FRAME_DATA, stream_id, data);
        if end_stream {
            frame.flags |= 0x01; // END_STREAM
        }
        frame
    }

    /// Create WINDOW_UPDATE frame
    pub fn window_update(stream_id: u32, increment: u32) -> Self {
        let mut payload = Vec::new();
        payload.extend_from_slice(&increment.to_be_bytes()[..]);
        Http2Frame::new(FRAME_WINDOW_UPDATE, stream_id, payload)
    }

    /// Create PING frame
    pub fn ping(opaque: [u8; 8], ack: bool) -> Self {
        let mut frame = Http2Frame::new(FRAME_PING, 0, opaque.to_vec());
        if ack {
            frame.flags |= 0x01; // ACK
        }
        frame
    }

    /// Create GOAWAY frame
    pub fn goaway(last_stream_id: u32, error_code: u32, debug_data: Vec<u8>) -> Self {
        let mut payload = Vec::new();
        payload.extend_from_slice(&last_stream_id.to_be_bytes()[..]);
        payload.extend_from_slice(&error_code.to_be_bytes()[..]);
        payload.extend_from_slice(&debug_data);
        Http2Frame::new(FRAME_GOAWAY, 0, payload)
    }

    /// Create RST_STREAM frame
    pub fn rst_stream(stream_id: u32, error_code: u32) -> Self {
        let mut payload = Vec::new();
        payload.extend_from_slice(&error_code.to_be_bytes()[..]);
        Http2Frame::new(FRAME_RST_STREAM, stream_id, payload)
    }

    /// Encode frame to bytes
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(9 + self.payload.len());
        
        // Length (24 bits)
        buf.push(((self.length >> 16) & 0xFF) as u8);
        buf.push(((self.length >> 8) & 0xFF) as u8);
        buf.push((self.length & 0xFF) as u8);
        
        // Type (8 bits)
        buf.push(self.frame_type);
        
        // Flags (8 bits)
        buf.push(self.flags);
        
        // Stream ID (32 bits, R bit reserved)
        buf.extend_from_slice(&self.stream_id.to_be_bytes()[..]);
        
        // Payload
        buf.extend_from_slice(&self.payload);
        
        buf
    }

    /// Decode frame from bytes
    pub fn decode(data: &[u8]) -> Option<(Self, usize)> {
        if data.len() < 9 {
            return None;
        }

        let length = ((data[0] as u32) << 16) | ((data[1] as u32) << 8) | (data[2] as u32);
        let frame_type = data[3];
        let flags = data[4];
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

    /// Check END_STREAM flag
    pub fn is_end_stream(&self) -> bool {
        (self.flags & 0x01) != 0
    }

    /// Check END_HEADERS flag
    pub fn is_end_headers(&self) -> bool {
        (self.flags & 0x04) != 0
    }

    /// Check ACK flag
    pub fn is_ack(&self) -> bool {
        (self.flags & 0x01) != 0
    }
}

/// HTTP/2 Settings
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

/// HTTP/2 Stream State
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

/// HTTP/2 Stream
#[derive(Clone, Debug)]
pub struct Http2Stream {
    pub stream_id: u32,
    pub state: StreamState,
    pub headers: BTreeMap<String, String>,
    pub data: Vec<u8>,
    pub window_size: u32,
    pub end_stream: bool,
}

impl Http2Stream {
    pub fn new(stream_id: u32) -> Self {
        Http2Stream {
            stream_id,
            state: StreamState::Idle,
            headers: BTreeMap::new(),
            data: Vec::new(),
            window_size: 65535,
            end_stream: false,
        }
    }
}

// ============================================================================
// HPACK Header Compression
// ============================================================================

/// HPACK Static Table (partial - first 20 entries)
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

/// HPACK Encoder
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

    /// Encode headers
    pub fn encode(&mut self, headers: &BTreeMap<String, String>) -> Vec<u8> {
        let mut encoded = Vec::new();

        for (name, value) in headers {
            // Check static table first
            let mut found_static = false;
            for (i, (s_name, s_value)) in STATIC_TABLE.iter().enumerate() {
                if name == s_name && value == s_value {
                    // Indexed header field
                    encoded.push(0x80 | (i as u8 + 1));
                    found_static = true;
                    break;
                } else if name == s_name {
                    // Literal with incremental indexing
                    encoded.push(0x40 | (i as u8 + 1));
                    self.encode_string(value, &mut encoded);
                    found_static = true;
                    break;
                }
            }

            if !found_static {
                // Literal with incremental indexing (new name)
                encoded.push(0x40);
                self.encode_string(name, &mut encoded);
                self.encode_string(value, &mut encoded);

                // Add to dynamic table
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
        
        // Use Huffman encoding for efficiency (simplified - just use literal)
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

/// HPACK Decoder
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

    /// Decode header block
    pub fn decode(&mut self, data: &[u8]) -> Result<BTreeMap<String, String>, HpackError> {
        let mut headers = BTreeMap::new();
        let mut pos = 0;

        while pos < data.len() {
            let byte = data[pos];

            if byte & 0x80 != 0 {
                // Indexed header field
                let index = (byte & 0x7F) as usize;
                if let Some((name, value)) = self.get_header(index) {
                    headers.insert(name.to_string(), value.to_string());
                }
                pos += 1;
            } else if byte & 0xC0 == 0x40 {
                // Literal with incremental indexing
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
                // Literal without indexing
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
                // Literal never indexed
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
                // Dynamic table size update
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
            self.dynamic_table.get(dynamic_index)
                .map(|(n, v)| (n.as_str(), v.as_str()))
        }
    }

    fn decode_string(&self, data: &[u8], pos: &mut usize) -> Result<String, HpackError> {
        if *pos >= data.len() {
            return Err(HpackError::UnexpectedEnd);
        }

        let first = data[*pos];
        let _huffman = (first & 0x80) != 0;
        let len = self.decode_integer(data, pos, 7)? as usize;

        if *pos + len > data.len() {
            return Err(HpackError::UnexpectedEnd);
        }

        let s = core::str::from_utf8(&data[*pos..*pos + len])
            .map_err(|_| HpackError::InvalidUtf8)?
            .to_string();
        *pos += len;

        Ok(s)
    }

    fn decode_integer(&self, data: &[u8], pos: &mut usize, prefix_bits: u8) -> Result<u32, HpackError> {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HpackError {
    InvalidIndex,
    InvalidPrefix,
    UnexpectedEnd,
    InvalidUtf8,
}

/// HTTP/2 Connection
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
            next_stream_id: 1,
            window_size: 65535,
            encoder: HpackEncoder::new(4096),
            decoder: HpackDecoder::new(4096),
        }
    }

    /// Create new stream
    pub fn create_stream(&mut self) -> u32 {
        let stream_id = self.next_stream_id;
        self.next_stream_id += 2; // Client-initiated streams use odd numbers
        self.streams.insert(stream_id, Http2Stream::new(stream_id));
        stream_id
    }

    /// Get stream
    pub fn get_stream(&self, stream_id: u32) -> Option<&Http2Stream> {
        self.streams.get(&stream_id)
    }

    /// Get stream mutable
    pub fn get_stream_mut(&mut self, stream_id: u32) -> Option<&mut Http2Stream> {
        self.streams.get_mut(&stream_id)
    }

    /// Build request headers
    pub fn build_request(&mut self, stream_id: u32, method: &str, path: &str, host: &str) -> Vec<u8> {
        let mut headers = BTreeMap::new();
        headers.insert(":method".to_string(), method.to_string());
        headers.insert(":path".to_string(), path.to_string());
        headers.insert(":scheme".to_string(), "https".to_string());
        headers.insert(":authority".to_string(), host.to_string());
        headers.insert("user-agent".to_string(), "echOS/2.0".to_string());

        self.encoder.encode(&headers)
    }

    /// Process received frame
    pub fn process_frame(&mut self, frame: &Http2Frame) -> Result<(), Http2Error> {
        match frame.frame_type {
            FRAME_SETTINGS => {
                self.process_settings(&frame.payload)?;
            }
            FRAME_HEADERS => {
                let headers = self.decoder.decode(&frame.payload)
                    .map_err(|_| Http2Error::CompressionError)?;
                if let Some(stream) = self.streams.get_mut(&frame.stream_id) {
                    stream.headers = headers;
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
                    frame.payload[0], frame.payload[1], frame.payload[2], frame.payload[3],
                ]);
                if frame.stream_id == 0 {
                    self.window_size += increment;
                } else if let Some(stream) = self.streams.get_mut(&frame.stream_id) {
                    stream.window_size += increment;
                }
            }
            FRAME_RST_STREAM => {
                self.streams.remove(&frame.stream_id);
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
            let value = u32::from_be_bytes([payload[pos + 2], payload[pos + 3], payload[pos + 4], payload[pos + 5]]);
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

/// Get connection preface
pub fn connection_preface() -> &'static [u8] {
    CONNECTION_PREFACE
}
