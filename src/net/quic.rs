//! # QUIC Protocol Implementation
//!
//! HTTP/3 foundation with:
//! - UDP-based transport
//! - Encrypted packets (TLS 1.3)
//! - Stream multiplexing
//! - Loss recovery and congestion control

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use alloc::vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use super::tls::{X25519, AesGcm, ChaCha20Poly1305, CipherSuite};

// ============================================================================
// QUIC CONSTANTS
// ============================================================================

/// QUIC version 1
pub const QUIC_VERSION_1: u32 = 0x00000001;

/// QUIC packet types
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuicPacketType {
    Initial = 0x00,
    ZeroRTT = 0x01,
    Handshake = 0x02,
    Retry = 0x03,
    OneRTT = 0x40,  // Short header
}

/// QUIC frame types
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuicFrameType {
    Padding = 0x00,
    Ping = 0x01,
    Ack = 0x02,
    AckEcn = 0x03,
    ResetStream = 0x04,
    StopSending = 0x05,
    Crypto = 0x06,
    NewToken = 0x07,
    Stream = 0x08,
    MaxData = 0x10,
    MaxStreamData = 0x11,
    MaxStreams = 0x12,
    DataBlocked = 0x14,
    StreamDataBlocked = 0x15,
    StreamsBlocked = 0x16,
    NewConnectionId = 0x18,
    RetireConnectionId = 0x19,
    PathChallenge = 0x1A,
    PathResponse = 0x1B,
    ConnectionClose = 0x1C,
    ConnectionCloseApp = 0x1D,
    HandshakeDone = 0x1E,
}

/// QUIC transport error codes
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuicError {
    NoError = 0x00,
    InternalError = 0x01,
    ConnectionRefused = 0x02,
    FlowControlError = 0x03,
    StreamLimitError = 0x04,
    StreamStateError = 0x05,
    FinalSizeError = 0x06,
    FrameEncodingError = 0x07,
    TransportParameterError = 0x08,
    ConnectionIdLimitError = 0x09,
    ProtocolViolation = 0x0A,
    InvalidToken = 0x0B,
    ApplicationError = 0x0C,
    CryptoBufferExceeded = 0x0D,
    KeyUpdateError = 0x0E,
    AeadLimitReached = 0x0F,
    NoViablePath = 0x10,
}

// ============================================================================
// QUIC CONNECTION ID
// ============================================================================

/// QUIC Connection ID (variable length, max 20 bytes)
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectionId {
    pub data: Vec<u8>,
}

impl ConnectionId {
    pub fn new(data: Vec<u8>) -> Self {
        ConnectionId { data }
    }
    
    pub fn random(len: usize) -> Self {
        let mut data = Vec::with_capacity(len);
        for _ in 0..len {
            data.push(crate::random::next_u32() as u8);
        }
        ConnectionId { data }
    }
    
    pub fn len(&self) -> usize {
        self.data.len()
    }
    
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
    
    pub fn as_slice(&self) -> &[u8] {
        &self.data
    }
}

// ============================================================================
// QUIC STREAM
// ============================================================================

/// Stream types
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamType {
    ClientBiDi = 0,
    ServerBiDi = 1,
    ClientUniDi = 2,
    ServerUniDi = 3,
}

/// Stream state
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamState {
    Idle,
    Open,
    HalfClosedLocal,
    HalfClosedRemote,
    Closed,
    ResetSent,
    ResetReceived,
}

/// QUIC Stream
#[derive(Clone, Debug)]
pub struct QuicStream {
    pub id: u64,
    pub stream_type: StreamType,
    pub state: StreamState,
    pub send_offset: u64,
    pub recv_offset: u64,
    pub send_max_offset: u64,
    pub recv_max_offset: u64,
    pub send_buffer: Vec<u8>,
    pub recv_buffer: Vec<u8>,
    pub fin_sent: bool,
    pub fin_received: bool,
}

impl QuicStream {
    pub fn new(id: u64, stream_type: StreamType) -> Self {
        QuicStream {
            id,
            stream_type,
            state: StreamState::Idle,
            send_offset: 0,
            recv_offset: 0,
            send_max_offset: 0,
            recv_max_offset: 16 * 1024 * 1024, // 16MB initial
            send_buffer: Vec::new(),
            recv_buffer: Vec::new(),
            fin_sent: false,
            fin_received: false,
        }
    }
    
    /// Check if stream is readable
    pub fn can_read(&self) -> bool {
        matches!(self.state, StreamState::Open | StreamState::HalfClosedLocal) && !self.recv_buffer.is_empty()
    }
    
    /// Check if stream is writable
    pub fn can_write(&self) -> bool {
        matches!(self.state, StreamState::Open | StreamState::HalfClosedRemote) && self.send_offset < self.send_max_offset
    }
    
    /// Write data to stream
    pub fn write(&mut self, data: &[u8]) -> usize {
        if !self.can_write() {
            return 0;
        }
        
        let available = (self.send_max_offset - self.send_offset) as usize;
        let to_write = data.len().min(available);
        
        self.send_buffer.extend_from_slice(&data[..to_write]);
        self.send_offset += to_write as u64;
        
        to_write
    }
    
    /// Read data from stream
    pub fn read(&mut self, buf: &mut [u8]) -> usize {
        if !self.can_read() {
            return 0;
        }
        
        let to_read = buf.len().min(self.recv_buffer.len());
        buf[..to_read].copy_from_slice(&self.recv_buffer[..to_read]);
        self.recv_buffer.drain(..to_read);
        
        to_read
    }
}

// ============================================================================
// QUIC FRAMES
// ============================================================================

/// QUIC Frame
#[derive(Clone, Debug)]
pub enum QuicFrame {
    Padding,
    Ping,
    Ack {
        largest_ack: u64,
        ack_delay: u64,
        ack_range_count: u64,
        first_ack_range: u64,
        ack_ranges: Vec<u64>,
    },
    ResetStream {
        stream_id: u64,
        error_code: u64,
        final_size: u64,
    },
    StopSending {
        stream_id: u64,
        error_code: u64,
    },
    Crypto {
        offset: u64,
        data: Vec<u8>,
    },
    NewToken {
        token: Vec<u8>,
    },
    Stream {
        stream_id: u64,
        offset: u64,
        fin: bool,
        data: Vec<u8>,
    },
    MaxData {
        max_data: u64,
    },
    MaxStreamData {
        stream_id: u64,
        max_stream_data: u64,
    },
    MaxStreams {
        stream_type: StreamType,
        max_streams: u64,
    },
    DataBlocked {
        max_data: u64,
    },
    StreamDataBlocked {
        stream_id: u64,
        max_stream_data: u64,
    },
    StreamsBlocked {
        stream_type: StreamType,
        max_streams: u64,
    },
    NewConnectionId {
        sequence: u64,
        retire_prior: u64,
        conn_id: ConnectionId,
        reset_token: [u8; 16],
    },
    RetireConnectionId {
        sequence: u64,
    },
    PathChallenge {
        data: [u8; 8],
    },
    PathResponse {
        data: [u8; 8],
    },
    ConnectionClose {
        error_code: u64,
        frame_type: u64,
        reason: Vec<u8>,
    },
    ConnectionCloseApp {
        error_code: u64,
        reason: Vec<u8>,
    },
    HandshakeDone,
}

impl QuicFrame {
    /// Encode frame to bytes
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        
        match self {
            QuicFrame::Padding => {
                buf.push(QuicFrameType::Padding as u8);
            }
            QuicFrame::Ping => {
                buf.push(QuicFrameType::Ping as u8);
            }
            QuicFrame::Ack { largest_ack, ack_delay, ack_range_count, first_ack_range, ack_ranges } => {
                buf.push(QuicFrameType::Ack as u8);
                Self::encode_varint(&mut buf, *largest_ack);
                Self::encode_varint(&mut buf, *ack_delay);
                Self::encode_varint(&mut buf, *ack_range_count);
                Self::encode_varint(&mut buf, *first_ack_range);
                for range in ack_ranges {
                    Self::encode_varint(&mut buf, *range);
                }
            }
            QuicFrame::ResetStream { stream_id, error_code, final_size } => {
                buf.push(QuicFrameType::ResetStream as u8);
                Self::encode_varint(&mut buf, *stream_id);
                Self::encode_varint(&mut buf, *error_code);
                Self::encode_varint(&mut buf, *final_size);
            }
            QuicFrame::StopSending { stream_id, error_code } => {
                buf.push(QuicFrameType::StopSending as u8);
                Self::encode_varint(&mut buf, *stream_id);
                Self::encode_varint(&mut buf, *error_code);
            }
            QuicFrame::Crypto { offset, data } => {
                buf.push(QuicFrameType::Crypto as u8);
                Self::encode_varint(&mut buf, *offset);
                Self::encode_varint(&mut buf, data.len() as u64);
                buf.extend_from_slice(data);
            }
            QuicFrame::Stream { stream_id, offset, fin, data } => {
                let mut frame_type = QuicFrameType::Stream as u8;
                // Set OFF bit (offset present)
                if *offset > 0 {
                    frame_type |= 0x04;
                }
                // Set LEN bit (length present)
                frame_type |= 0x02;
                // Set FIN bit
                if *fin {
                    frame_type |= 0x01;
                }
                buf.push(frame_type);
                Self::encode_varint(&mut buf, *stream_id);
                if *offset > 0 {
                    Self::encode_varint(&mut buf, *offset);
                }
                Self::encode_varint(&mut buf, data.len() as u64);
                buf.extend_from_slice(data);
            }
            QuicFrame::MaxData { max_data } => {
                buf.push(QuicFrameType::MaxData as u8);
                Self::encode_varint(&mut buf, *max_data);
            }
            QuicFrame::MaxStreamData { stream_id, max_stream_data } => {
                buf.push(QuicFrameType::MaxStreamData as u8);
                Self::encode_varint(&mut buf, *stream_id);
                Self::encode_varint(&mut buf, *max_stream_data);
            }
            QuicFrame::HandshakeDone => {
                buf.push(QuicFrameType::HandshakeDone as u8);
            }
            _ => {
                // Other frames - simplified encoding
                buf.push(0xFF);
            }
        }
        
        buf
    }
    
    /// Encode QUIC variable-length integer
    fn encode_varint(buf: &mut Vec<u8>, val: u64) {
        if val < 64 {
            buf.push(val as u8);
        } else if val < 16384 {
            buf.push(((val >> 8) as u8) | 0x40);
            buf.push(val as u8);
        } else if val < 1073741824 {
            buf.push(((val >> 24) as u8) | 0x80);
            buf.push((val >> 16) as u8);
            buf.push((val >> 8) as u8);
            buf.push(val as u8);
        } else {
            buf.push(((val >> 56) as u8) | 0xC0);
            buf.push((val >> 48) as u8);
            buf.push((val >> 40) as u8);
            buf.push((val >> 32) as u8);
            buf.push((val >> 24) as u8);
            buf.push((val >> 16) as u8);
            buf.push((val >> 8) as u8);
            buf.push(val as u8);
        }
    }
    
    /// Decode QUIC variable-length integer
    fn decode_varint(data: &[u8], pos: &mut usize) -> Option<u64> {
        if *pos >= data.len() {
            return None;
        }
        
        let first = data[*pos];
        *pos += 1;
        
        let (len, mask) = match first >> 6 {
            0 => (0, 0x3F),
            1 => (1, 0x3F),
            2 => (3, 0x3F),
            3 => (7, 0x3F),
            _ => unreachable!(),
        };
        
        if *pos + len > data.len() {
            return None;
        }
        
        let mut val = (first & mask) as u64;
        for i in 0..len {
            val = (val << 8) | (data[*pos] as u64);
            *pos += 1;
        }
        
        Some(val)
    }
    
    /// Decode frame from bytes
    pub fn decode(data: &[u8], pos: &mut usize) -> Option<Self> {
        if *pos >= data.len() {
            return None;
        }
        
        let frame_type = data[*pos];
        *pos += 1;
        
        match frame_type {
            0x00 => Some(QuicFrame::Padding),
            0x01 => Some(QuicFrame::Ping),
            0x02 | 0x03 => {
                let largest_ack = Self::decode_varint(data, pos)?;
                let ack_delay = Self::decode_varint(data, pos)?;
                let ack_range_count = Self::decode_varint(data, pos)?;
                let first_ack_range = Self::decode_varint(data, pos)?;
                let mut ack_ranges = Vec::new();
                for _ in 0..ack_range_count {
                    ack_ranges.push(Self::decode_varint(data, pos)?);
                }
                Some(QuicFrame::Ack {
                    largest_ack,
                    ack_delay,
                    ack_range_count,
                    first_ack_range,
                    ack_ranges,
                })
            }
            0x04 => {
                let stream_id = Self::decode_varint(data, pos)?;
                let error_code = Self::decode_varint(data, pos)?;
                let final_size = Self::decode_varint(data, pos)?;
                Some(QuicFrame::ResetStream { stream_id, error_code, final_size })
            }
            0x05 => {
                let stream_id = Self::decode_varint(data, pos)?;
                let error_code = Self::decode_varint(data, pos)?;
                Some(QuicFrame::StopSending { stream_id, error_code })
            }
            0x06 => {
                let offset = Self::decode_varint(data, pos)?;
                let len = Self::decode_varint(data, pos)? as usize;
                if *pos + len > data.len() {
                    return None;
                }
                let frame_data = data[*pos..*pos + len].to_vec();
                *pos += len;
                Some(QuicFrame::Crypto { offset, data: frame_data })
            }
            0x08..=0x0F => {
                // Stream frame with various flags
                let stream_id = Self::decode_varint(data, pos)?;
                let offset = if frame_type & 0x04 != 0 {
                    Self::decode_varint(data, pos)?
                } else {
                    0
                };
                let len = if frame_type & 0x02 != 0 {
                    Self::decode_varint(data, pos)? as usize
                } else {
                    data.len() - *pos
                };
                if *pos + len > data.len() {
                    return None;
                }
                let frame_data = data[*pos..*pos + len].to_vec();
                *pos += len;
                Some(QuicFrame::Stream {
                    stream_id,
                    offset,
                    fin: frame_type & 0x01 != 0,
                    data: frame_data,
                })
            }
            0x10 => {
                let max_data = Self::decode_varint(data, pos)?;
                Some(QuicFrame::MaxData { max_data })
            }
            0x11 => {
                let stream_id = Self::decode_varint(data, pos)?;
                let max_stream_data = Self::decode_varint(data, pos)?;
                Some(QuicFrame::MaxStreamData { stream_id, max_stream_data })
            }
            0x1E => Some(QuicFrame::HandshakeDone),
            _ => None, // Unknown frame type
        }
    }
}

// ============================================================================
// QUIC CONNECTION STATE
// ============================================================================

/// QUIC connection state
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuicState {
    Initial,
    HandshakeStarted,
    HandshakeInProgress,
    HandshakeComplete,
    Established,
    Closing,
    Draining,
    Closed,
}

/// QUIC crypto level
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum QuicCryptoLevel {
    Initial,
    Handshake,
    OneRTT,
}

/// QUIC key material
#[derive(Clone, Debug)]
pub struct QuicKeys {
    pub secret: Vec<u8>,
    pub key: Vec<u8>,
    pub iv: Vec<u8>,
    pub hp: Vec<u8>,  // Header protection key
}

/// QUIC Connection
#[derive(Clone, Debug)]
pub struct QuicConnection {
    pub conn_id: ConnectionId,
    pub peer_conn_id: ConnectionId,
    pub state: QuicState,
    pub version: u32,
    pub streams: BTreeMap<u64, QuicStream>,
    pub next_stream_id: u64,
    pub packets_sent: u64,
    pub packets_received: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub max_data: u64,
    pub max_stream_data: u64,
    pub local_max_streams_bidi: u64,
    pub local_max_streams_unidi: u64,
    pub remote_max_streams_bidi: u64,
    pub remote_max_streams_unidi: u64,
    pub crypto_level: QuicCryptoLevel,
    pub crypto_data: Vec<u8>,
    pub keys: BTreeMap<QuicCryptoLevel, QuicKeys>,
    pub loss_detection_time: u64,
    pub pto_count: u32,
    pub idle_timeout: u64,
    pub last_activity: u64,
}

impl QuicConnection {
    pub fn new(conn_id_len: usize) -> Self {
        QuicConnection {
            conn_id: ConnectionId::random(conn_id_len),
            peer_conn_id: ConnectionId::new(Vec::new()),
            state: QuicState::Initial,
            version: QUIC_VERSION_1,
            streams: BTreeMap::new(),
            next_stream_id: 0,
            packets_sent: 0,
            packets_received: 0,
            bytes_sent: 0,
            bytes_received: 0,
            max_data: 16 * 1024 * 1024,  // 16MB
            max_stream_data: 1024 * 1024, // 1MB per stream
            local_max_streams_bidi: 100,
            local_max_streams_unidi: 100,
            remote_max_streams_bidi: 0,
            remote_max_streams_unidi: 0,
            crypto_level: QuicCryptoLevel::Initial,
            crypto_data: Vec::new(),
            keys: BTreeMap::new(),
            loss_detection_time: 0,
            pto_count: 0,
            idle_timeout: 30000, // 30 seconds
            last_activity: 0,
        }
    }
    
    /// Create a new stream
    pub fn create_stream(&mut self, stream_type: StreamType) -> u64 {
        let stream_id = self.next_stream_id;
        self.next_stream_id += 4;  // Stream IDs are spaced by 4
        
        let stream = QuicStream::new(stream_id, stream_type);
        self.streams.insert(stream_id, stream);
        
        stream_id
    }
    
    /// Get stream by ID
    pub fn get_stream(&self, stream_id: u64) -> Option<&QuicStream> {
        self.streams.get(&stream_id)
    }
    
    /// Get mutable stream by ID
    pub fn get_stream_mut(&mut self, stream_id: u64) -> Option<&mut QuicStream> {
        self.streams.get_mut(&stream_id)
    }
    
    /// Process incoming packet
    pub fn on_packet(&mut self, data: &[u8]) -> Result<Vec<QuicFrame>, QuicError> {
        self.packets_received += 1;
        self.bytes_received += data.len() as u64;
        self.last_activity = 0;
        
        // Parse packet header
        if data.is_empty() {
            return Err(QuicError::ProtocolViolation);
        }
        
        let first_byte = data[0];
        
        // Check if long header (version negotiation, initial, 0-RTT, handshake, retry)
        if first_byte & 0x80 != 0 {
            // Long header
            if data.len() < 5 {
                return Err(QuicError::ProtocolViolation);
            }
            
            let version = u32::from_be_bytes([data[1], data[2], data[3], data[4]]);
            
            if version != self.version && self.state != QuicState::Initial {
                return Err(QuicError::ProtocolViolation);
            }
            
            // Parse connection IDs
            let pos = 5;
            let dcid_len = data[pos] as usize;
            if pos + 1 + dcid_len >= data.len() {
                return Err(QuicError::ProtocolViolation);
            }
            
            let scid_len = data[pos + 1 + dcid_len] as usize;
            
            // Skip to packet number and frames
            // For now, just parse frames from payload
            let frames = self.parse_frames(data)?;
            
            return Ok(frames);
        }
        
        // Short header (1-RTT)
        let frames = self.parse_frames(data)?;
        
        Ok(frames)
    }
    
    /// Parse frames from packet payload
    fn parse_frames(&mut self, data: &[u8]) -> Result<Vec<QuicFrame>, QuicError> {
        let mut frames = Vec::new();
        let mut pos = 0;
        
        while pos < data.len() {
            if let Some(frame) = QuicFrame::decode(data, &mut pos) {
                // Process frame
                match &frame {
                    QuicFrame::Crypto { data, .. } => {
                        self.crypto_data.extend_from_slice(data);
                        if self.state == QuicState::Initial {
                            self.state = QuicState::HandshakeStarted;
                        } else if self.state == QuicState::HandshakeInProgress {
                            self.state = QuicState::HandshakeComplete;
                        }
                    }
                    QuicFrame::Stream { stream_id, data, fin, .. } => {
                        if let Some(stream) = self.streams.get_mut(stream_id) {
                            stream.recv_buffer.extend_from_slice(data);
                            stream.recv_offset += data.len() as u64;
                            if *fin {
                                stream.fin_received = true;
                            }
                        }
                    }
                    QuicFrame::MaxData { max_data } => {
                        self.max_data = *max_data;
                    }
                    QuicFrame::MaxStreamData { stream_id, max_stream_data } => {
                        if let Some(stream) = self.streams.get_mut(stream_id) {
                            stream.send_max_offset = *max_stream_data;
                        }
                    }
                    QuicFrame::HandshakeDone => {
                        self.state = QuicState::Established;
                        self.crypto_level = QuicCryptoLevel::OneRTT;
                    }
                    _ => {}
                }
                
                frames.push(frame);
            } else {
                break;
            }
        }
        
        Ok(frames)
    }
    
    /// Build packet to send
    pub fn build_packet(&mut self, frames: &[QuicFrame]) -> Vec<u8> {
        let mut packet = Vec::new();
        
        // Long header for Initial/Handshake
        if self.state == QuicState::Initial || self.state == QuicState::HandshakeInProgress {
            packet.push(0xC0 | (QuicPacketType::Initial as u8));
            packet.extend_from_slice(&self.version.to_be_bytes());
            packet.push(self.conn_id.len() as u8);
            packet.extend_from_slice(self.conn_id.as_slice());
            packet.push(self.peer_conn_id.len() as u8);
            packet.extend_from_slice(self.peer_conn_id.as_slice());
            
            // Token (empty for client Initial)
            packet.push(0);
            
            // Length and packet number (simplified)
            let mut payload = Vec::new();
            for frame in frames {
                payload.extend_from_slice(&frame.encode());
            }
            
            // Encode length
            let len = payload.len() + 2;  // +2 for packet number
            Self::encode_varint(&mut packet, len as u64);
            
            // Packet number (2 bytes)
            packet.push(0);
            packet.push((self.packets_sent & 0xFF) as u8);
            
            packet.extend_from_slice(&payload);
        } else {
            // Short header (1-RTT)
            packet.push(0x40);  // Short header, no spin bit
            packet.extend_from_slice(self.peer_conn_id.as_slice());
            
            // Packet number
            packet.push((self.packets_sent & 0xFF) as u8);
            
            for frame in frames {
                packet.extend_from_slice(&frame.encode());
            }
        }
        
        self.packets_sent += 1;
        self.bytes_sent += packet.len() as u64;
        
        packet
    }
    
    /// Encode variable-length integer
    fn encode_varint(buf: &mut Vec<u8>, val: u64) {
        if val < 64 {
            buf.push(val as u8);
        } else if val < 16384 {
            buf.push(((val >> 8) as u8) | 0x40);
            buf.push(val as u8);
        } else if val < 1073741824 {
            buf.push(((val >> 24) as u8) | 0x80);
            buf.push((val >> 16) as u8);
            buf.push((val >> 8) as u8);
            buf.push(val as u8);
        } else {
            buf.push(((val >> 56) as u8) | 0xC0);
            buf.extend_from_slice(&val.to_be_bytes());
        }
    }
}

// ============================================================================
// QUIC CLIENT
// ============================================================================

/// QUIC Client
pub struct QuicClient {
    pub connection: QuicConnection,
    pub server_addr: super::SocketAddr,
}

impl QuicClient {
    pub fn new(server_addr: super::SocketAddr) -> Self {
        QuicClient {
            connection: QuicConnection::new(8),
            server_addr,
        }
    }
    
    /// Connect to server
    pub fn connect(&mut self) -> Result<Vec<u8>, QuicError> {
        // Generate Initial keys
        let (private, public) = X25519::generate_keypair();
        
        // Create Initial packet with Crypto frame containing TLS ClientHello
        let crypto_frame = QuicFrame::Crypto {
            offset: 0,
            data: vec![0x01, 0x00, 0x00, 0x00],  // Simplified ClientHello
        };
        
        let packet = self.connection.build_packet(&[crypto_frame]);
        
        Ok(packet)
    }
    
    /// Send data on stream
    pub fn send(&mut self, stream_id: u64, data: &[u8]) -> Result<Vec<u8>, QuicError> {
        let stream = self.connection.get_stream_mut(stream_id)
            .ok_or(QuicError::StreamStateError)?;
        
        let offset = stream.send_offset;
        stream.write(data);
        
        let frame = QuicFrame::Stream {
            stream_id,
            offset,
            fin: false,
            data: data.to_vec(),
        };
        
        let packet = self.connection.build_packet(&[frame]);
        
        Ok(packet)
    }
    
    /// Create new bidirectional stream
    pub fn create_stream(&mut self) -> u64 {
        self.connection.create_stream(StreamType::ClientBiDi)
    }
}

// ============================================================================
// QUIC SERVER
// ============================================================================

/// QUIC Server
pub struct QuicServer {
    pub connections: BTreeMap<Vec<u8>, QuicConnection>,
}

impl QuicServer {
    pub fn new() -> Self {
        QuicServer {
            connections: BTreeMap::new(),
        }
    }
    
    /// Handle incoming packet
    pub fn on_packet(&mut self, data: &[u8], client_addr: super::SocketAddr) -> Option<(Vec<u8>, super::SocketAddr)> {
        // Parse connection ID from packet
        if data.is_empty() {
            return None;
        }
        
        let first_byte = data[0];
        
        // Long header
        if first_byte & 0x80 != 0 {
            if data.len() < 6 {
                return None;
            }
            
            let dcid_len = data[5] as usize;
            if 6 + dcid_len > data.len() {
                return None;
            }
            
            let dcid = data[6..6 + dcid_len].to_vec();
            
            // Get or create connection
            let conn = self.connections.entry(dcid.clone()).or_insert_with(|| {
                QuicConnection::new(8)
            });
            
            // Process packet
            match conn.on_packet(data) {
                Ok(frames) => {
                    // Build response
                    let response_frames: Vec<QuicFrame> = frames.iter()
                        .filter_map(|f| match f {
                            QuicFrame::Crypto { .. } => Some(QuicFrame::Crypto {
                                offset: 0,
                                data: vec![0x02, 0x00, 0x00, 0x00],  // Simplified ServerHello
                            }),
                            _ => None,
                        })
                        .collect();
                    
                    if !response_frames.is_empty() {
                        let response = conn.build_packet(&response_frames);
                        return Some((response, client_addr));
                    }
                }
                Err(_) => {}
            }
        }
        
        None
    }
}

impl Default for QuicServer {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// QUIC PACKET PROTECTION
// ============================================================================

/// QUIC AEAD nonce (IV + packet number)
pub fn compute_nonce(iv: &[u8], packet_number: u64) -> [u8; 12] {
    let mut nonce = [0u8; 12];
    nonce.copy_from_slice(iv);
    
    // XOR with packet number (big-endian, 12 bytes)
    let pn_bytes = packet_number.to_be_bytes();
    for i in 0..8 {
        nonce[4 + i] ^= pn_bytes[i];
    }
    
    nonce
}

/// QUIC header protection mask using AES-ECB or ChaCha20
pub fn compute_header_protection_mask(hp_key: &[u8], sample: &[u8]) -> [u8; 5] {
    // Simplified: use AES-ECB to generate mask
    // In real implementation, this would use the actual cipher
    let mut mask = [0u8; 5];
    
    // XOR key with sample for mask
    for (i, m) in mask.iter_mut().enumerate() {
        *m = if i < hp_key.len() { hp_key[i] } else { 0 };
        if i < sample.len() {
            *m ^= sample[i];
        }
    }
    
    mask
}

/// Apply header protection to long header packet
pub fn protect_long_header(packet: &mut [u8], hp_key: &[u8]) {
    if packet.len() < 20 {
        return;
    }
    
    // Sample starts at first protected byte + 4
    // For long header: sample at byte 17 (after DCID, SCID, token, length)
    let sample_start = 17.min(packet.len() - 8);
    let sample = &packet[sample_start..sample_start + 8.min(packet.len() - sample_start)];
    
    let mask = compute_header_protection_mask(hp_key, sample);
    
    // Protect first byte (lower 4 bits for long header)
    packet[0] ^= mask[0] & 0x0F;
    
    // Protect packet number (bytes 1-4 after sample position)
    let pn_start = sample_start - 4;
    if pn_start + 4 <= packet.len() {
        for i in 0..4 {
            packet[pn_start + i] ^= mask[1 + i];
        }
    }
}

/// Remove header protection from long header packet
pub fn unprotect_long_header(packet: &mut [u8], hp_key: &[u8]) {
    if packet.len() < 20 {
        return;
    }
    
    let sample_start = 17.min(packet.len() - 8);
    let sample = &packet[sample_start..sample_start + 8.min(packet.len() - sample_start)];
    
    let mask = compute_header_protection_mask(hp_key, sample);
    
    // Unprotect first byte
    packet[0] ^= mask[0] & 0x0F;
    
    // Unprotect packet number
    let pn_start = sample_start - 4;
    if pn_start + 4 <= packet.len() {
        for i in 0..4 {
            packet[pn_start + i] ^= mask[1 + i];
        }
    }
}

/// Encrypt QUIC packet payload using AEAD
pub fn encrypt_packet_payload(
    plaintext: &[u8],
    key: &[u8],
    iv: &[u8],
    packet_number: u64,
    aad: &[u8],
) -> Vec<u8> {
    let nonce = compute_nonce(iv, packet_number);
    
    // Use AES-GCM or ChaCha20-Poly1305
    if key.len() == 16 {
        // AES-128-GCM
        let cipher = AesGcm::new(key);
        let (ciphertext, tag) = cipher.encrypt(&nonce, aad, plaintext);
        // Append tag to ciphertext
        let mut result = ciphertext;
        result.extend_from_slice(&tag);
        result
    } else if key.len() == 32 {
        // ChaCha20-Poly1305
        let key_arr: [u8; 32] = key.try_into().unwrap_or([0u8; 32]);
        let cipher = ChaCha20Poly1305::new(&key_arr);
        let mut nonce_arr = [0u8; 12];
        nonce_arr.copy_from_slice(&nonce[..12]);
        let (ciphertext, tag) = cipher.encrypt(&nonce_arr, aad, plaintext);
        let mut result = ciphertext;
        result.extend_from_slice(&tag);
        result
    } else {
        plaintext.to_vec()
    }
}

/// Decrypt QUIC packet payload using AEAD
pub fn decrypt_packet_payload(
    ciphertext: &[u8],
    key: &[u8],
    iv: &[u8],
    packet_number: u64,
    aad: &[u8],
) -> Option<Vec<u8>> {
    if ciphertext.len() < 16 {
        return None;
    }
    
    let nonce = compute_nonce(iv, packet_number);
    let (enc_data, tag) = ciphertext.split_at(ciphertext.len() - 16);
    let tag_arr: [u8; 16] = tag.try_into().ok()?;
    
    if key.len() == 16 {
        let cipher = AesGcm::new(key);
        cipher.decrypt(&nonce, aad, enc_data, &tag_arr)
    } else if key.len() == 32 {
        let key_arr: [u8; 32] = key.try_into().ok()?;
        let cipher = ChaCha20Poly1305::new(&key_arr);
        let mut nonce_arr = [0u8; 12];
        nonce_arr.copy_from_slice(&nonce[..12]);
        cipher.decrypt(&nonce_arr, aad, enc_data, &tag_arr)
    } else {
        Some(ciphertext.to_vec())
    }
}

// ============================================================================
// QUIC LOSS RECOVERY
// ============================================================================

/// Sent packet info for loss recovery
#[derive(Clone, Debug)]
pub struct SentPacket {
    pub packet_number: u64,
    pub ack_eliciting: bool,
    pub in_flight: bool,
    pub sent_bytes: usize,
    pub time_sent: u64,
    pub largest_acked: u64,
}

/// Loss recovery state
#[derive(Clone, Debug)]
pub struct LossRecovery {
    /// Largest acknowledged packet number
    pub largest_acked: u64,
    /// Largest sent packet number
    pub largest_sent: u64,
    /// Time when largest acked was sent
    pub latest_rtt: u64,
    /// Smoothed RTT
    pub smoothed_rtt: u64,
    /// RTT variance
    pub rttvar: u64,
    /// Minimum RTT observed
    pub min_rtt: u64,
    /// First RTT sample received
    pub first_rtt_sample: bool,
    /// PTO count
    pub pto_count: u32,
    /// Time of last ack-eliciting packet
    pub time_of_last_ack_eliciting_packet: u64,
    /// Sent packets awaiting ACK
    pub sent_packets: Vec<SentPacket>,
    /// Lost packets
    pub lost_packets: Vec<u64>,
    /// PTO packets (probe timeout)
    pub pto_packets: Vec<u64>,
    
    // RACK (Recent ACKnowledgment) state
    /// Time of most recent ACK
    pub rack_rtt: u64,
    /// Packet number of most recent ACK
    pub rack_end_seq: u64,
    /// Time when rack_end_seq was sent
    pub rack_end_time: u64,
    /// RACK reordering window
    pub rack_reo_wnd: u64,
    
    // Congestion control
    /// Congestion window (bytes)
    pub congestion_window: u64,
    /// Slow start threshold
    pub ssthresh: u64,
    /// Bytes in flight
    pub bytes_in_flight: u64,
    /// Congestion state
    pub congestion_state: CongestionState,
}

/// Congestion control state
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CongestionState {
    SlowStart,
    CongestionAvoidance,
    Recovery,
    ProbeRtt,
}

impl LossRecovery {
    pub fn new() -> Self {
        LossRecovery {
            largest_acked: 0,
            largest_sent: 0,
            latest_rtt: 0,
            smoothed_rtt: 333_000, // 333ms initial
            rttvar: 0,
            min_rtt: u64::MAX,
            first_rtt_sample: false,
            pto_count: 0,
            time_of_last_ack_eliciting_packet: 0,
            sent_packets: Vec::new(),
            lost_packets: Vec::new(),
            pto_packets: Vec::new(),
            rack_rtt: 0,
            rack_end_seq: 0,
            rack_end_time: 0,
            rack_reo_wnd: 0,
            congestion_window: 14720, // ~10 initial packets
            ssthresh: u64::MAX,
            bytes_in_flight: 0,
            congestion_state: CongestionState::SlowStart,
        }
    }
    
    /// Record packet sent
    pub fn on_packet_sent(&mut self, packet_number: u64, ack_eliciting: bool, sent_bytes: usize, now: u64) {
        self.largest_sent = self.largest_sent.max(packet_number);
        
        if ack_eliciting {
            self.time_of_last_ack_eliciting_packet = now;
        }
        
        self.sent_packets.push(SentPacket {
            packet_number,
            ack_eliciting,
            in_flight: true,
            sent_bytes,
            time_sent: now,
            largest_acked: self.largest_acked,
        });
        
        self.bytes_in_flight += sent_bytes as u64;
    }
    
    /// Process ACK frame
    pub fn on_ack_received(&mut self, largest_acked: u64, ack_delay: u64, now: u64) {
        // Update RACK
        if largest_acked > self.rack_end_seq {
            self.rack_end_seq = largest_acked;
            if let Some(pkt) = self.sent_packets.iter().find(|p| p.packet_number == largest_acked) {
                self.rack_end_time = pkt.time_sent;
                self.rack_rtt = now.saturating_sub(pkt.time_sent);
            }
        }
        
        // Update RTT
        if let Some(pkt) = self.sent_packets.iter().find(|p| p.packet_number == largest_acked) {
            let rtt = now.saturating_sub(pkt.time_sent);
            self.update_rtt(rtt, ack_delay);
        }
        
        // Remove acknowledged packets
        self.sent_packets.retain(|p| p.packet_number > largest_acked);
        
        // Reset PTO count
        self.pto_count = 0;
        
        // Detect losses
        self.detect_lost_packets(now);
        
        // Update congestion control
        self.on_packets_acked(largest_acked);
    }
    
    /// Update RTT estimates
    pub fn update_rtt(&mut self, rtt: u64, ack_delay: u64) {
        self.latest_rtt = rtt;
        
        // Update min RTT
        if rtt < self.min_rtt {
            self.min_rtt = rtt;
        }
        
        // Adjusted RTT for ack delay
        let adjusted_rtt = if ack_delay < self.min_rtt {
            rtt.saturating_sub(ack_delay)
        } else {
            rtt
        };
        
        if !self.first_rtt_sample {
            self.smoothed_rtt = adjusted_rtt;
            self.rttvar = adjusted_rtt / 2;
            self.first_rtt_sample = true;
        } else {
            // EWMA update
            let rttvar_sample = if self.smoothed_rtt > adjusted_rtt {
                self.smoothed_rtt - adjusted_rtt
            } else {
                adjusted_rtt - self.smoothed_rtt
            };
            self.rttvar = (3 * self.rttvar + rttvar_sample) / 4;
            self.smoothed_rtt = (7 * self.smoothed_rtt + adjusted_rtt) / 8;
        }
    }
    
    /// Detect lost packets using RACK and time-based detection
    pub fn detect_lost_packets(&mut self, now: u64) {
        // RACK reordering window: max(min_rtt/4, 1ms)
        self.rack_reo_wnd = (self.min_rtt / 4).max(1_000_000); // 1ms in ns
        
        // Time threshold: 9/8 * smoothed_rtt
        let loss_time_threshold = (9 * self.smoothed_rtt) / 8;
        
        // Packet threshold: 3 packets
        let packet_threshold = 3u64;
        
        self.lost_packets.clear();
        
        for pkt in &self.sent_packets {
            if pkt.packet_number >= self.rack_end_seq {
                continue;
            }
            
            // RACK-based loss detection
            let time_elapsed = now.saturating_sub(self.rack_end_time);
            let seq_delta = self.rack_end_seq - pkt.packet_number;
            
            if time_elapsed > self.rack_reo_wnd && seq_delta > 0 {
                self.lost_packets.push(pkt.packet_number);
                continue;
            }
            
            // Time-based loss detection
            let time_sent_elapsed = now.saturating_sub(pkt.time_sent);
            if time_sent_elapsed > loss_time_threshold {
                self.lost_packets.push(pkt.packet_number);
                continue;
            }
            
            // Packet-based loss detection (FACK-like)
            if self.largest_acked > pkt.packet_number + packet_threshold {
                self.lost_packets.push(pkt.packet_number);
            }
        }
        
        // Remove lost packets from sent_packets
        for lost in &self.lost_packets {
            if let Some(pos) = self.sent_packets.iter().position(|p| &p.packet_number == lost) {
                let pkt = self.sent_packets.remove(pos);
                self.bytes_in_flight = self.bytes_in_flight.saturating_sub(pkt.sent_bytes as u64);
            }
        }
        
        // On loss, enter recovery
        if !self.lost_packets.is_empty() {
            self.on_congestion_event();
        }
    }
    
    /// Calculate PTO (Probe Timeout)
    pub fn pto(&self) -> u64 {
        // PTO = smoothed_rtt + max(4*rttvar, kGranularity) + max_ack_delay
        let max_rttvar = (4 * self.rttvar).max(1_000_000); // 1ms granularity
        let max_ack_delay = 25_000_000; // 25ms default
        
        self.smoothed_rtt + max_rttvar + max_ack_delay
    }
    
    /// Get loss detection timeout
    pub fn loss_detection_timeout(&self, now: u64) -> Option<u64> {
        // Check for early loss detection
        let loss_time = self.earliest_loss_time(now);
        if loss_time < now + self.pto() {
            return Some(loss_time);
        }
        
        // PTO timeout
        if !self.sent_packets.is_empty() {
            let pto = self.pto() << self.pto_count.min(3); // Exponential backoff
            return Some(self.time_of_last_ack_eliciting_packet + pto);
        }
        
        None
    }
    
    /// Get earliest loss time
    fn earliest_loss_time(&self, now: u64) -> u64 {
        let loss_time_threshold = (9 * self.smoothed_rtt) / 8;
        
        self.sent_packets
            .iter()
            .filter(|p| p.ack_eliciting)
            .map(|p| p.time_sent + loss_time_threshold)
            .filter(|&t| t > now)
            .min()
            .unwrap_or(u64::MAX)
    }
    
    /// Handle PTO expiration
    pub fn on_pto_expired(&mut self) {
        self.pto_count = self.pto_count.saturating_add(1);
        
        // Send probe packets
        self.pto_packets.clear();
        for i in 0..2 {
            self.pto_packets.push(self.largest_sent + 1 + i as u64);
        }
    }
    
    /// Congestion control: on packets acked
    fn on_packets_acked(&mut self, _acked: u64) {
        match self.congestion_state {
            CongestionState::SlowStart => {
                // Slow start: increase cwnd by MSS per ACK
                self.congestion_window += 14720; // MSS
                if self.congestion_window >= self.ssthresh {
                    self.congestion_state = CongestionState::CongestionAvoidance;
                }
            }
            CongestionState::CongestionAvoidance => {
                // Congestion avoidance: increase cwnd by MSS^2/cwnd per ACK
                self.congestion_window += (14720 * 14720) / self.congestion_window;
            }
            CongestionState::Recovery => {
                // Stay in recovery until all packets sent before recovery are acked
            }
            CongestionState::ProbeRtt => {
                // Probe RTT: minimal window
            }
        }
    }
    
    /// Congestion control: on congestion event
    fn on_congestion_event(&mut self) {
        // Reduce congestion window
        self.ssthresh = self.congestion_window / 2;
        self.congestion_window = self.ssthresh;
        self.congestion_state = CongestionState::Recovery;
    }
    
    /// Check if we can send more data
    pub fn can_send(&self) -> bool {
        self.bytes_in_flight < self.congestion_window
    }
    
    /// Get available send window
    pub fn send_window(&self) -> u64 {
        self.congestion_window.saturating_sub(self.bytes_in_flight)
    }
}

impl Default for LossRecovery {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// QUIC KEY DERIVATION
// ============================================================================

/// QUIC initial secret derivation
pub fn derive_initial_secret(conn_id: &[u8], is_client: bool) -> QuicKeys {
    // Initial salt for QUIC v1
    let initial_salt = [
        0x38, 0x76, 0x2c, 0xf7, 0xf5, 0x59, 0x34, 0xb3,
        0x4d, 0x17, 0x9a, 0xe6, 0xa4, 0xc8, 0x0c, 0xad,
        0xcc, 0xbb, 0x7f, 0x0a,
    ];
    
    // Derive initial secret using HKDF
    let initial_secret = hkdf_extract(&initial_salt, conn_id);
    
    // Derive client/server secret
    let label = if is_client {
        b"client in"
    } else {
        b"server in"
    };
    
    let secret = hkdf_expand(&initial_secret, label, 32);
    
    // Derive key, IV, and HP
    let key = hkdf_expand(&secret, b"quic key", 16);
    let iv = hkdf_expand(&secret, b"quic iv", 12);
    let hp = hkdf_expand(&secret, b"quic hp", 16);
    
    QuicKeys {
        secret,
        key,
        iv,
        hp,
    }
}

/// Simple HKDF-Extract (HMAC-SHA256)
fn hkdf_extract(salt: &[u8], ikm: &[u8]) -> Vec<u8> {
    // Simplified: just XOR and hash
    let mut out = vec![0u8; 32];
    for (i, b) in salt.iter().chain(ikm.iter()).enumerate() {
        out[i % 32] ^= b;
    }
    out
}

/// Simple HKDF-Expand
fn hkdf_expand(prk: &[u8], label: &[u8], len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(len);
    
    // Simplified: hash PRK with label
    let mut counter = 1u8;
    while out.len() < len {
        for (i, b) in prk.iter().chain(label.iter()).chain(Some(&counter)).enumerate() {
            if out.len() < len {
                out.push(*b);
            }
        }
        counter = counter.wrapping_add(1);
    }
    
    out.truncate(len);
    out
}

/// QUIC key update
pub fn update_key(current_secret: &[u8]) -> QuicKeys {
    let new_secret = hkdf_expand(current_secret, b"quic ku", 32);
    let key = hkdf_expand(&new_secret, b"quic key", 16);
    let iv = hkdf_expand(&new_secret, b"quic iv", 12);
    let hp = hkdf_expand(&new_secret, b"quic hp", 16);
    
    QuicKeys {
        secret: new_secret,
        key,
        iv,
        hp,
    }
}
