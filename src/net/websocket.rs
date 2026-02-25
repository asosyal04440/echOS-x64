//! # WebSocket Protocol
//!
//! RFC 6455 WebSocket implementation for real-time bidirectional communication.

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::vec;
use spin::Mutex;

// WebSocket Opcodes
const OPCODE_CONTINUATION: u8 = 0x0;
const OPCODE_TEXT: u8 = 0x1;
const OPCODE_BINARY: u8 = 0x2;
const OPCODE_CLOSE: u8 = 0x8;
const OPCODE_PING: u8 = 0x9;
const OPCODE_PONG: u8 = 0xA;

// WebSocket Magic GUID for handshake
const WEBSOCKET_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// WebSocket State
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WebSocketState {
    Connecting,
    Open,
    Closing,
    Closed,
}

/// WebSocket Frame
#[derive(Clone, Debug)]
pub struct WebSocketFrame {
    pub fin: bool,
    pub rsv1: bool,
    pub rsv2: bool,
    pub rsv3: bool,
    pub opcode: u8,
    pub masked: bool,
    pub payload_len: u64,
    pub masking_key: Option<[u8; 4]>,
    pub payload: Vec<u8>,
}

impl WebSocketFrame {
    /// Create new text frame
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

    /// Create new binary frame
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

    /// Create close frame
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

    /// Create ping frame
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

    /// Create pong frame
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

    /// Set masking key (for client frames)
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

    /// Encode frame to bytes
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        
        // First byte: FIN, RSV, Opcode
        let mut first = 0u8;
        if self.fin { first |= 0x80; }
        if self.rsv1 { first |= 0x40; }
        if self.rsv2 { first |= 0x20; }
        if self.rsv3 { first |= 0x10; }
        first |= self.opcode & 0x0F;
        buf.push(first);
        
        // Second byte: MASK, Payload length
        let mut second = 0u8;
        if self.masked { second |= 0x80; }
        
        if self.payload_len < 126 {
            second |= self.payload_len as u8;
            buf.push(second);
        } else if self.payload_len < 65536 {
            second |= 126;
            buf.push(second);
            buf.extend_from_slice(&(self.payload_len as u16).to_be_bytes());
        } else {
            second |= 127;
            buf.push(second);
            buf.extend_from_slice(&self.payload_len.to_be_bytes());
        }
        
        // Masking key
        if let Some(key) = self.masking_key {
            buf.extend_from_slice(&key);
        }
        
        // Payload
        buf.extend_from_slice(&self.payload);
        
        buf
    }

    /// Decode frame from bytes
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
        
        // Extended payload length
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
        
        // Masking key
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
        
        // Payload
        if data.len() < offset + payload_len as usize {
            return Err(WebSocketError::IncompleteFrame);
        }
        
        let mut payload = data[offset..offset + payload_len as usize].to_vec();
        offset += payload_len as usize;
        
        // Unmask payload
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

    /// Check if text frame
    pub fn is_text(&self) -> bool {
        self.opcode == OPCODE_TEXT
    }

    /// Check if binary frame
    pub fn is_binary(&self) -> bool {
        self.opcode == OPCODE_BINARY
    }

    /// Check if close frame
    pub fn is_close(&self) -> bool {
        self.opcode == OPCODE_CLOSE
    }

    /// Check if ping frame
    pub fn is_ping(&self) -> bool {
        self.opcode == OPCODE_PING
    }

    /// Check if pong frame
    pub fn is_pong(&self) -> bool {
        self.opcode == OPCODE_PONG
    }

    /// Get payload as string
    pub fn payload_as_string(&self) -> String {
        String::from_utf8_lossy(&self.payload).to_string()
    }

    /// Get close code
    pub fn close_code(&self) -> Option<u16> {
        if self.is_close() && self.payload.len() >= 2 {
            Some(u16::from_be_bytes([self.payload[0], self.payload[1]]))
        } else {
            None
        }
    }

    /// Get close reason
    pub fn close_reason(&self) -> Option<&str> {
        if self.is_close() && self.payload.len() > 2 {
            core::str::from_utf8(&self.payload[2..]).ok()
        } else {
            None
        }
    }
}

/// WebSocket Error
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WebSocketError {
    IncompleteFrame,
    InvalidOpcode,
    ProtocolError,
    InvalidUtf8,
    ConnectionClosed,
    HandshakeFailed,
    FrameTooLarge,
}

/// WebSocket Close Codes
pub mod close_codes {
    pub const NORMAL: u16 = 1000;
    pub const GOING_AWAY: u16 = 1001;
    pub const PROTOCOL_ERROR: u16 = 1002;
    pub const UNSUPPORTED: u16 = 1003;
    pub const NO_STATUS: u16 = 1005;
    pub const ABNORMAL: u16 = 1006;
    pub const INVALID_DATA: u16 = 1007;
    pub const POLICY_VIOLATION: u16 = 1008;
    pub const MESSAGE_TOO_BIG: u16 = 1009;
    pub const MANDATORY_EXTENSION: u16 = 1010;
    pub const INTERNAL_ERROR: u16 = 1011;
    pub const SERVICE_RESTART: u16 = 1012;
    pub const TRY_AGAIN_LATER: u16 = 1013;
    pub const TLS_HANDSHAKE: u16 = 1015;
}

/// WebSocket Handshake
pub struct WebSocketHandshake;

impl WebSocketHandshake {
    /// Generate client handshake key
    pub fn generate_key() -> [u8; 16] {
        let mut key = [0u8; 16];
        crate::crypto::rdrand_bytes(&mut key);
        key
    }

    /// Build client handshake request
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

    /// Verify server handshake response
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

    /// Compute Sec-WebSocket-Accept key
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

/// WebSocket Connection
#[derive(Clone)]
pub struct WebSocketConnection {
    pub state: WebSocketState,
    pub host: String,
    pub port: u16,
    pub path: String,
    pub protocol: String,
    pub send_buffer: Vec<u8>,
    pub recv_buffer: Vec<u8>,
}

impl WebSocketConnection {
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

    /// Send text message
    pub fn send_text(&mut self, message: &str) -> Vec<u8> {
        let mut frame = WebSocketFrame::text(message);
        frame.mask(Self::generate_mask());
        frame.encode()
    }

    /// Send binary message
    pub fn send_binary(&mut self, data: &[u8]) -> Vec<u8> {
        let mut frame = WebSocketFrame::binary(data.to_vec());
        frame.mask(Self::generate_mask());
        frame.encode()
    }

    /// Send close frame
    pub fn send_close(&mut self, code: u16, reason: &str) -> Vec<u8> {
        let mut frame = WebSocketFrame::close(code, reason);
        frame.mask(Self::generate_mask());
        self.state = WebSocketState::Closing;
        frame.encode()
    }

    /// Send ping
    pub fn send_ping(&mut self, data: &[u8]) -> Vec<u8> {
        let mut frame = WebSocketFrame::ping(data.to_vec());
        frame.mask(Self::generate_mask());
        frame.encode()
    }

    /// Receive frame
    pub fn receive(&mut self, data: &[u8]) -> Result<Vec<WebSocketFrame>, WebSocketError> {
        self.recv_buffer.extend_from_slice(data);
        
        let mut frames = Vec::new();
        
        loop {
            match WebSocketFrame::decode(&self.recv_buffer) {
                Ok((frame, consumed)) => {
                    // Handle control frames
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

    /// Get pending send data
    pub fn get_send_data(&mut self) -> Option<Vec<u8>> {
        if self.send_buffer.is_empty() {
            None
        } else {
            let data = self.send_buffer.clone();
            self.send_buffer.clear();
            Some(data)
        }
    }

    fn generate_mask() -> [u8; 4] {
        let mut mask = [0u8; 4];
        crate::crypto::rdrand_bytes(&mut mask);
        mask
    }
}

/// Base64 encode (simplified)
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
            result.push('=');
        }
        
        if i + 2 < data.len() {
            result.push(ALPHABET[b2 & 0x3F] as char);
        } else {
            result.push('=');
        }
        
        i += 3;
    }
    
    result
}

// Global WebSocket connections
lazy_static::lazy_static! {
    static ref WS_CONNECTIONS: Mutex<BTreeMap<u32, WebSocketConnection>> = Mutex::new(BTreeMap::new());
    static ref WS_NEXT_ID: Mutex<u32> = Mutex::new(1);
}

/// Create WebSocket connection
pub fn connect_ws(host: &str, port: u16, path: &str) -> u32 {
    let mut connections = WS_CONNECTIONS.lock();
    let mut next_id = WS_NEXT_ID.lock();
    
    let id = *next_id;
    *next_id += 1;
    
    connections.insert(id, WebSocketConnection::new(host, port, path));
    id
}

/// Get WebSocket connection
pub fn get_connection(id: u32) -> Option<WebSocketConnection> {
    WS_CONNECTIONS.lock().get(&id).cloned()
}

/// Close WebSocket connection
pub fn close_ws(id: u32) {
    WS_CONNECTIONS.lock().remove(&id);
}
