//! # TCP Protocol
//!
//! TCP state machine and connection handling with SACK support

use super::{Ipv4Addr, Port, SocketAddr, NetError, allocate_socket_id};
use super::ip::{IpProtocol, Ipv4Packet};
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use alloc::vec;
use alloc::boxed::Box;
use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;

// ============================================================================
// TCP OPTIONS
// ============================================================================

/// TCP option kinds
pub const TCPOPT_EOL: u8 = 0;
pub const TCPOPT_NOP: u8 = 1;
pub const TCPOPT_MSS: u8 = 2;
pub const TCPOPT_WINDOW_SCALE: u8 = 3;
pub const TCPOPT_SACK_PERMITTED: u8 = 4;
pub const TCPOPT_SACK: u8 = 5;
pub const TCPOPT_TIMESTAMP: u8 = 8;

// ============================================================================
// TCP SACK (Selective Acknowledgment)
// ============================================================================

/// SACK block: [start, end) sequence numbers
#[derive(Clone, Copy, Debug, Default)]
pub struct SackBlock {
    pub start: u32,
    pub end: u32,
}

impl SackBlock {
    pub fn new(start: u32, end: u32) -> Self {
        SackBlock { start, end }
    }
    
    /// Check if sequence number is within this block
    pub fn contains(&self, seq: u32) -> bool {
        // Handle wraparound
        if self.start <= self.end {
            seq >= self.start && seq < self.end
        } else {
            seq >= self.start || seq < self.end
        }
    }
    
    /// Get block length
    pub fn len(&self) -> u32 {
        self.end.wrapping_sub(self.start)
    }
}

/// SACK scoreboard - tracks received segments for selective retransmission
#[derive(Clone, Debug)]
pub struct SackScoreboard {
    /// Received SACK blocks (max 4 per RFC 2018)
    pub blocks: Vec<SackBlock>,
    /// Maximum blocks we can store
    pub max_blocks: usize,
    /// Highest SACKed sequence number
    pub high_sack: u32,
    /// Number of bytes SACKed
    pub sacked_bytes: u32,
}

impl Default for SackScoreboard {
    fn default() -> Self {
        Self::new()
    }
}

impl SackScoreboard {
    pub fn new() -> Self {
        SackScoreboard {
            blocks: Vec::with_capacity(4),
            max_blocks: 4,
            high_sack: 0,
            sacked_bytes: 0,
        }
    }
    
    /// Add a SACK block, merging overlaps
    pub fn add_block(&mut self, block: SackBlock) {
        // Check for overlap with existing blocks and merge
        let mut merged = false;
        for existing in &mut self.blocks {
            // Check if blocks overlap or are adjacent
            if Self::blocks_overlap_or_adjacent(existing, &block) {
                // Merge: expand existing block
                existing.start = existing.start.min(block.start);
                existing.end = existing.end.max(block.end);
                merged = true;
                break;
            }
        }
        
        if !merged {
            // Add new block if we have space
            if self.blocks.len() < self.max_blocks {
                self.blocks.push(block);
            } else {
                // Remove oldest block and add new one
                self.blocks.remove(0);
                self.blocks.push(block);
            }
        }
        
        // Sort blocks by start sequence
        self.blocks.sort_by_key(|b| b.start);
        
        // Update high_sack
        for block in &self.blocks {
            if block.end.wrapping_sub(self.high_sack) as i32 > 0 {
                self.high_sack = block.end;
            }
        }
        
        // Recalculate sacked bytes
        self.sacked_bytes = self.blocks.iter().map(|b| b.len()).sum();
    }
    
    /// Check if two blocks overlap or are adjacent
    fn blocks_overlap_or_adjacent(a: &SackBlock, b: &SackBlock) -> bool {
        // Handle wraparound
        let a_before_b = a.end.wrapping_sub(b.start) as i32 >= 0;
        let b_before_a = b.end.wrapping_sub(a.start) as i32 >= 0;
        
        // Adjacent: a.end == b.start or b.end == a.start
        let adjacent = a.end.wrapping_sub(b.start) == 0 || b.end.wrapping_sub(a.start) == 0;
        
        // Overlap: intervals intersect
        let overlap = (a.start <= b.start && a.end > b.start) ||
                      (b.start <= a.start && b.end > a.start);
        
        overlap || adjacent
    }
    
    /// Check if a sequence range is covered by SACK blocks
    pub fn is_sacked(&self, start: u32, end: u32) -> bool {
        for block in &self.blocks {
            if block.start <= start && block.end >= end {
                return true;
            }
        }
        false
    }
    
    /// Get gaps in received data (for retransmission)
    pub fn get_gaps(&self, snd_una: u32, snd_nxt: u32) -> Vec<SackBlock> {
        let mut gaps = Vec::new();
        
        if self.blocks.is_empty() {
            // No SACK info, entire window is a gap
            gaps.push(SackBlock::new(snd_una, snd_nxt));
            return gaps;
        }
        
        // Start from SND.UNA
        let mut current = snd_una;
        
        for block in &self.blocks {
            if current < block.start {
                // Gap between current and block.start
                gaps.push(SackBlock::new(current, block.start));
            }
            current = current.max(block.end);
        }
        
        // Gap after last block to SND.NXT
        if current < snd_nxt {
            gaps.push(SackBlock::new(current, snd_nxt));
        }
        
        gaps
    }
    
    /// Clear scoreboard
    pub fn clear(&mut self) {
        self.blocks.clear();
        self.high_sack = 0;
        self.sacked_bytes = 0;
    }
    
    /// Serialize SACK blocks to TCP option format
    pub fn serialize(&self) -> Vec<u8> {
        let mut data = Vec::new();
        
        // Option type
        data.push(TCPOPT_SACK);
        // Length: 2 (header) + 8 * num_blocks
        let len = 2 + 8 * self.blocks.len();
        data.push(len as u8);
        
        // Each block: 4 bytes start + 4 bytes end
        for block in &self.blocks {
            data.extend_from_slice(&block.start.to_be_bytes());
            data.extend_from_slice(&block.end.to_be_bytes());
        }
        
        // Pad to 4-byte boundary
        while data.len() % 4 != 0 {
            data.push(TCPOPT_NOP);
        }
        
        data
    }
    
    /// Parse SACK blocks from TCP option data
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 2 || data[0] != TCPOPT_SACK {
            return None;
        }
        
        let len = data[1] as usize;
        if len < 2 || (len - 2) % 8 != 0 {
            return None;
        }
        
        let num_blocks = (len - 2) / 8;
        let mut scoreboard = SackScoreboard::new();
        
        for i in 0..num_blocks {
            let offset = 2 + i * 8;
            if offset + 8 > data.len() {
                break;
            }
            
            let start = u32::from_be_bytes([data[offset], data[offset+1], data[offset+2], data[offset+3]]);
            let end = u32::from_be_bytes([data[offset+4], data[offset+5], data[offset+6], data[offset+7]]);
            
            scoreboard.add_block(SackBlock::new(start, end));
        }
        
        Some(scoreboard)
    }
}

/// SACK Permitted option for SYN packets
#[derive(Clone, Copy, Debug)]
pub struct SackPermitted;

impl SackPermitted {
    pub fn serialize() -> [u8; 2] {
        [TCPOPT_SACK_PERMITTED, 2]
    }
}

// ============================================================================
// TCP FAST RETRANSMIT
// ============================================================================

/// Fast retransmit state
#[derive(Clone, Debug)]
pub struct FastRetransmitState {
    /// Duplicate ACK counter
    pub dup_ack_count: u32,
    /// Last ACK number received
    pub last_ack: u32,
    /// Sequence number at last fast retransmit
    pub recover: u32,
    /// Is fast recovery in progress?
    pub in_recovery: bool,
    /// Threshold for fast retransmit (typically 3)
    pub threshold: u32,
}

impl Default for FastRetransmitState {
    fn default() -> Self {
        Self::new()
    }
}

impl FastRetransmitState {
    pub fn new() -> Self {
        FastRetransmitState {
            dup_ack_count: 0,
            last_ack: 0,
            recover: 0,
            in_recovery: false,
            threshold: 3,
        }
    }
    
    /// Process incoming ACK, returns true if fast retransmit needed
    pub fn on_ack(&mut self, ack: u32, sack_blocks: &[SackBlock], snd_una: u32) -> bool {
        if ack == self.last_ack && ack != snd_una {
            // Duplicate ACK
            // Check if it carries new SACK info
            let has_new_sack = !sack_blocks.is_empty();
            
            self.dup_ack_count += 1;
            
            // Fast retransmit when dup_ack_count reaches threshold
            if self.dup_ack_count >= self.threshold && !self.in_recovery {
                self.in_recovery = true;
                self.recover = snd_una;
                return true;
            }
        } else if ack > self.last_ack {
            // New ACK - reset duplicate counter
            self.dup_ack_count = 0;
            self.last_ack = ack;
            
            // Exit fast recovery if ACK covers recover point
            if self.in_recovery && ack >= self.recover {
                self.in_recovery = false;
            }
        }
        
        false
    }
    
    /// Reset state
    pub fn reset(&mut self) {
        self.dup_ack_count = 0;
        self.in_recovery = false;
    }
}

// ============================================================================
// TCP FAST OPEN (TFO)
// ============================================================================

/// TFO Cookie (8 bytes)
#[derive(Clone, Copy, Debug, Default)]
pub struct TfoCookie(pub [u8; 8]);

impl TfoCookie {
    pub fn new() -> Self {
        let mut cookie = [0u8; 8];
        for i in 0..8 {
            cookie[i] = crate::random::next_u32() as u8;
        }
        TfoCookie(cookie)
    }
    
    pub fn generate(server_ip: Ipv4Addr, time_ms: u64) -> Self {
        // Simple cookie: hash of IP + time
        let mut cookie = [0u8; 8];
        cookie[..4].copy_from_slice(&server_ip.0);
        cookie[4..8].copy_from_slice(&(time_ms as u32).to_be_bytes());
        
        // XOR with random
        let rand = crate::random::next_u32();
        for i in 0..4 {
            cookie[i] ^= (rand >> (i * 8)) as u8;
        }
        
        TfoCookie(cookie)
    }
    
    pub fn verify(&self, server_ip: Ipv4Addr, time_window: u64) -> bool {
        // Simplified verification - in production would use crypto
        let expected = TfoCookie::generate(server_ip, time_window);
        self.0 == expected.0
    }
}

/// TFO state for connection
#[derive(Clone, Debug)]
pub struct TfoState {
    /// Cookies per server IP
    pub cookies: BTreeMap<u32, TfoCookie>,
    /// Pending data to send in SYN
    pub pending_data: Vec<u8>,
    /// TFO enabled
    pub enabled: bool,
    /// TFO cookie request sent
    pub cookie_requested: bool,
    /// TFO data sent in SYN
    pub data_in_syn: bool,
}

impl Default for TfoState {
    fn default() -> Self {
        Self::new()
    }
}

impl TfoState {
    pub fn new() -> Self {
        TfoState {
            cookies: BTreeMap::new(),
            pending_data: Vec::new(),
            enabled: true,
            cookie_requested: false,
            data_in_syn: false,
        }
    }
    
    /// Get cookie for server
    pub fn get_cookie(&self, server_ip: Ipv4Addr) -> Option<TfoCookie> {
        let ip_key = u32::from_be_bytes(server_ip.0);
        self.cookies.get(&ip_key).copied()
    }
    
    /// Store cookie from server
    pub fn store_cookie(&mut self, server_ip: Ipv4Addr, cookie: TfoCookie) {
        let ip_key = u32::from_be_bytes(server_ip.0);
        self.cookies.insert(ip_key, cookie);
    }
    
    /// Serialize TFO cookie option
    pub fn serialize_cookie_option(cookie: &TfoCookie) -> Vec<u8> {
        let mut data = Vec::new();
        data.push(TCPOPT_FAST_OPEN); // 34
        data.push(10); // Length: 2 + 8
        data.extend_from_slice(&cookie.0);
        data
    }
}

/// TFO option kind
pub const TCPOPT_FAST_OPEN: u8 = 34;

// ============================================================================
// TCP WINDOW SCALING
// ============================================================================

/// Window scaling option
#[derive(Clone, Copy, Debug)]
pub struct WindowScaleOption {
    pub scale: u8,
}

impl WindowScaleOption {
    pub fn new(scale: u8) -> Self {
        WindowScaleOption { scale: scale.min(14) }
    }
    
    /// Serialize window scale option
    pub fn serialize(&self) -> [u8; 3] {
        [TCPOPT_WINDOW_SCALE, 3, self.scale]
    }
    
    /// Parse from option data
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 3 || data[0] != TCPOPT_WINDOW_SCALE {
            return None;
        }
        Some(WindowScaleOption { scale: data[2] })
    }
    
    /// Calculate effective window size
    pub fn effective_window(base: u16, scale: u8) -> u32 {
        (base as u32) << (scale as u32)
    }
}

// ============================================================================
// TCP TIMESTAMPS
// ============================================================================

/// TCP Timestamp option
#[derive(Clone, Copy, Debug, Default)]
pub struct TimestampOption {
    pub ts_val: u32,  // Timestamp value (sender's)
    pub ts_ecr: u32,  // Timestamp echo reply
}

impl TimestampOption {
    pub fn new(ts_val: u32, ts_ecr: u32) -> Self {
        TimestampOption { ts_val, ts_ecr }
    }
    
    /// Create with current time
    pub fn now(ts_ecr: u32) -> Self {
        // Use random as pseudo-timestamp for now
        let ts_val = crate::random::next_u32();
        TimestampOption { ts_val, ts_ecr }
    }
    
    /// Serialize timestamp option
    pub fn serialize(&self) -> [u8; 10] {
        let mut data = [0u8; 10];
        data[0] = TCPOPT_TIMESTAMP;
        data[1] = 10; // Length
        data[2..6].copy_from_slice(&self.ts_val.to_be_bytes());
        data[6..10].copy_from_slice(&self.ts_ecr.to_be_bytes());
        data
    }
    
    /// Parse from option data
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 10 || data[0] != TCPOPT_TIMESTAMP {
            return None;
        }
        let ts_val = u32::from_be_bytes([data[2], data[3], data[4], data[5]]);
        let ts_ecr = u32::from_be_bytes([data[6], data[7], data[8], data[9]]);
        Some(TimestampOption { ts_val, ts_ecr })
    }
    
    /// Calculate RTT from timestamp
    pub fn calculate_rtt(&self) -> u32 {
        // Simplified RTT calculation
        let now = crate::random::next_u32();
        now.wrapping_sub(self.ts_ecr)
    }
}

// ============================================================================
// TCP OPTIONS PARSER
// ============================================================================

/// Parsed TCP options
#[derive(Clone, Debug, Default)]
pub struct TcpOptions {
    pub mss: Option<u16>,
    pub window_scale: Option<WindowScaleOption>,
    pub sack_permitted: bool,
    pub sack_blocks: Vec<SackBlock>,
    pub timestamps: Option<TimestampOption>,
    pub tfo_cookie: Option<TfoCookie>,
}

impl TcpOptions {
    /// Parse options from TCP header
    pub fn parse(header_data: &[u8], header_len: usize) -> Self {
        let mut options = TcpOptions::default();
        
        if header_len <= 20 {
            return options;
        }
        
        let opts_data = &header_data[20..header_len];
        let mut i = 0;
        
        while i < opts_data.len() {
            let opt_kind = opts_data[i];
            
            match opt_kind {
                TCPOPT_EOL => break,
                TCPOPT_NOP => {
                    i += 1;
                    continue;
                }
                _ => {
                    if i + 1 >= opts_data.len() {
                        break;
                    }
                    let opt_len = opts_data[i + 1] as usize;
                    if opt_len < 2 || i + opt_len > opts_data.len() {
                        break;
                    }
                    
                    let opt_data = &opts_data[i..i + opt_len];
                    
                    match opt_kind {
                        TCPOPT_MSS => {
                            if opt_len >= 4 {
                                options.mss = Some(u16::from_be_bytes([opt_data[2], opt_data[3]]));
                            }
                        }
                        TCPOPT_WINDOW_SCALE => {
                            if let Some(ws) = WindowScaleOption::parse(opt_data) {
                                options.window_scale = Some(ws);
                            }
                        }
                        TCPOPT_SACK_PERMITTED => {
                            options.sack_permitted = true;
                        }
                        TCPOPT_SACK => {
                            if let Some(scoreboard) = SackScoreboard::parse(opt_data) {
                                options.sack_blocks = scoreboard.blocks;
                            }
                        }
                        TCPOPT_TIMESTAMP => {
                            options.timestamps = TimestampOption::parse(opt_data);
                        }
                        TCPOPT_FAST_OPEN => {
                            if opt_len >= 10 {
                                let mut cookie = [0u8; 8];
                                cookie.copy_from_slice(&opt_data[2..10]);
                                options.tfo_cookie = Some(TfoCookie(cookie));
                            }
                        }
                        _ => {}
                    }
                    
                    i += opt_len;
                }
            }
        }
        
        options
    }
    
    /// Build options for SYN packet
    pub fn build_syn_options(mss: u16, ws_scale: u8, enable_sack: bool, enable_tfo: bool) -> Vec<u8> {
        let mut opts = Vec::new();
        
        // MSS option
        opts.push(TCPOPT_MSS);
        opts.push(4);
        opts.extend_from_slice(&mss.to_be_bytes());
        
        // Window scale
        opts.extend_from_slice(&WindowScaleOption::new(ws_scale).serialize());
        
        // SACK permitted
        if enable_sack {
            opts.extend_from_slice(&SackPermitted::serialize());
        }
        
        // Timestamps
        let ts = TimestampOption::now(0);
        opts.extend_from_slice(&ts.serialize());
        
        // Pad to 4-byte boundary
        while opts.len() % 4 != 0 {
            opts.push(TCPOPT_NOP);
        }
        
        opts
    }
    
    /// Build options for data packet
    pub fn build_data_options(ts_echo: u32, sack_blocks: &[SackBlock]) -> Vec<u8> {
        let mut opts = Vec::new();
        
        // Timestamps
        let ts = TimestampOption::now(ts_echo);
        opts.extend_from_slice(&ts.serialize());
        
        // SACK blocks if any
        if !sack_blocks.is_empty() {
            let scoreboard = SackScoreboard {
                blocks: sack_blocks.to_vec(),
                max_blocks: 4,
                high_sack: 0,
                sacked_bytes: 0,
            };
            opts.extend_from_slice(&scoreboard.serialize());
        }
        
        // Pad to 4-byte boundary
        while opts.len() % 4 != 0 {
            opts.push(TCPOPT_NOP);
        }
        
        opts
    }
}

/// TCP header (20 bytes minimum)
#[derive(Clone, Copy, Debug)]
pub struct TcpHeader {
    pub src_port: Port,
    pub dst_port: Port,
    pub seq_num: u32,
    pub ack_num: u32,
    pub data_offset: u8,        // 4 bits, header length in 32-bit words
    pub flags: TcpFlags,
    pub window_size: u16,
    pub checksum: u16,
    pub urgent_ptr: u16,
}

/// TCP flags
#[derive(Clone, Copy, Debug, Default)]
pub struct TcpFlags {
    pub fin: bool,
    pub syn: bool,
    pub rst: bool,
    pub psh: bool,
    pub ack: bool,
    pub urg: bool,
}

impl TcpFlags {
    pub fn new() -> Self {
        TcpFlags::default()
    }
    
    pub fn syn() -> Self {
        TcpFlags { syn: true, ..Default::default() }
    }
    
    pub fn syn_ack() -> Self {
        TcpFlags { syn: true, ack: true, ..Default::default() }
    }
    
    pub fn ack() -> Self {
        TcpFlags { ack: true, ..Default::default() }
    }
    
    pub fn fin() -> Self {
        TcpFlags { fin: true, ..Default::default() }
    }
    
    pub fn fin_ack() -> Self {
        TcpFlags { fin: true, ack: true, ..Default::default() }
    }
    
    pub fn rst() -> Self {
        TcpFlags { rst: true, ..Default::default() }
    }
    
    pub fn to_u8(self) -> u8 {
        let mut val = 0u8;
        if self.fin { val |= 0x01; }
        if self.syn { val |= 0x02; }
        if self.rst { val |= 0x04; }
        if self.psh { val |= 0x08; }
        if self.ack { val |= 0x10; }
        if self.urg { val |= 0x20; }
        val
    }
    
    pub fn from_u8(val: u8) -> Self {
        TcpFlags {
            fin: val & 0x01 != 0,
            syn: val & 0x02 != 0,
            rst: val & 0x04 != 0,
            psh: val & 0x08 != 0,
            ack: val & 0x10 != 0,
            urg: val & 0x20 != 0,
        }
    }
}

impl TcpHeader {
    pub const MIN_SIZE: usize = 20;
    
    /// Parse TCP header from bytes
    pub fn parse(data: &[u8]) -> Result<Self, NetError> {
        if data.len() < Self::MIN_SIZE {
            return Err(NetError::InvalidPacket);
        }
        
        let src_port = Port(u16::from_be_bytes([data[0], data[1]]));
        let dst_port = Port(u16::from_be_bytes([data[2], data[3]]));
        let seq_num = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
        let ack_num = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
        let data_offset = (data[12] >> 4) & 0x0F;
        let flags = TcpFlags::from_u8(data[13]);
        let window_size = u16::from_be_bytes([data[14], data[15]]);
        let checksum = u16::from_be_bytes([data[16], data[17]]);
        let urgent_ptr = u16::from_be_bytes([data[18], data[19]]);
        
        Ok(TcpHeader {
            src_port,
            dst_port,
            seq_num,
            ack_num,
            data_offset,
            flags,
            window_size,
            checksum,
            urgent_ptr,
        })
    }
    
    /// Serialize header to bytes
    pub fn serialize(&self, buf: &mut [u8]) -> Result<(), NetError> {
        if buf.len() < Self::MIN_SIZE {
            return Err(NetError::BufferFull);
        }
        
        buf[0..2].copy_from_slice(&self.src_port.0.to_be_bytes());
        buf[2..4].copy_from_slice(&self.dst_port.0.to_be_bytes());
        buf[4..8].copy_from_slice(&self.seq_num.to_be_bytes());
        buf[8..12].copy_from_slice(&self.ack_num.to_be_bytes());
        buf[12] = (self.data_offset << 4) | 0x00; // Reserved bits
        buf[13] = self.flags.to_u8();
        buf[14..16].copy_from_slice(&self.window_size.to_be_bytes());
        buf[16..18].copy_from_slice(&self.checksum.to_be_bytes());
        buf[18..20].copy_from_slice(&self.urgent_ptr.to_be_bytes());
        
        Ok(())
    }
    
    /// Get header length in bytes
    pub fn header_len(&self) -> usize {
        (self.data_offset as usize) * 4
    }
}

/// Compute TCP checksum
pub fn compute_checksum(src_ip: Ipv4Addr, dst_ip: Ipv4Addr, segment: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    
    // Pseudo-header
    sum += u16::from_be_bytes([src_ip.0[0], src_ip.0[1]]) as u32;
    sum += u16::from_be_bytes([src_ip.0[2], src_ip.0[3]]) as u32;
    sum += u16::from_be_bytes([dst_ip.0[0], dst_ip.0[1]]) as u32;
    sum += u16::from_be_bytes([dst_ip.0[2], dst_ip.0[3]]) as u32;
    sum += 6u32; // TCP protocol number
    sum += segment.len() as u32;
    
    // TCP segment
    let mut i = 0;
    while i + 1 < segment.len() {
        sum += u16::from_be_bytes([segment[i], segment[i + 1]]) as u32;
        i += 2;
    }
    
    // Odd byte
    if i < segment.len() {
        sum += (segment[i] as u32) << 8;
    }
    
    // Fold carries
    while (sum >> 16) != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    
    // One's complement
    !(sum as u16)
}

/// Verify TCP checksum
pub fn verify_checksum(src_ip: Ipv4Addr, dst_ip: Ipv4Addr, segment: &[u8]) -> bool {
    compute_checksum(src_ip, dst_ip, segment) == 0
}

/// TCP connection state
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TcpState {
    Closed,
    Listen,
    SynSent,
    SynReceived,
    Established,
    FinWait1,
    FinWait2,
    CloseWait,
    Closing,
    LastAck,
    TimeWait,
}

/// TCP connection
#[derive(Clone, Debug)]
pub struct TcpConnection {
    pub id: u32,
    pub local: SocketAddr,
    pub remote: SocketAddr,
    pub state: TcpState,
    pub seq_num: u32,
    pub ack_num: u32,
    pub window_size: u16,
    pub rx_buffer: Vec<u8>,
    pub tx_buffer: Vec<u8>,
    pub listen_backlog: usize,
    // Congestion control
    pub cwnd: u32,           // Congestion window
    pub ssthresh: u32,       // Slow start threshold
    pub rtt: u32,            // Round-trip time (ms)
    pub rtt_var: u32,        // RTT variance
    pub rto: u32,            // Retransmission timeout (ms)
    pub retransmit_count: u8, // Retransmission counter
    // Window scaling
    pub ws_scale: u8,        // Window scale factor
    pub peer_ws_scale: u8,   // Peer's window scale
    // SACK support
    pub sack_permitted: bool,     // SACK negotiated
    pub sack_scoreboard: SackScoreboard,  // Received SACK blocks
    pub rx_sack_blocks: Vec<SackBlock>,   // SACK blocks to send
    // Fast retransmit
    pub fast_retx: FastRetransmitState,
    // Send state
    pub snd_una: u32,        // Oldest unacknowledged sequence
    pub snd_nxt: u32,        // Next sequence to send
    pub snd_wnd: u32,        // Send window
    // Timestamps
    pub ts_recent: u32,      // Recent timestamp from peer
    pub ts_echo: u32,        // Timestamp to echo
    pub ts_val: u32,         // Our timestamp value
}

impl TcpConnection {
    pub fn new(local: SocketAddr) -> Self {
        TcpConnection {
            id: allocate_socket_id(),
            local,
            remote: SocketAddr::default(),
            state: TcpState::Closed,
            seq_num: 0,
            ack_num: 0,
            window_size: 65535,
            rx_buffer: Vec::new(),
            tx_buffer: Vec::new(),
            listen_backlog: 0,
            // Congestion control defaults
            cwnd: 10 * 1460,        // Initial window (10 MSS)
            ssthresh: 65535,        // High threshold
            rtt: 100,               // Initial RTT estimate (100ms)
            rtt_var: 50,            // Initial RTT variance
            rto: 200,               // Initial RTO (200ms)
            retransmit_count: 0,
            ws_scale: 0,
            peer_ws_scale: 0,
            // SACK
            sack_permitted: false,
            sack_scoreboard: SackScoreboard::new(),
            rx_sack_blocks: Vec::new(),
            // Fast retransmit
            fast_retx: FastRetransmitState::new(),
            // Send state
            snd_una: 0,
            snd_nxt: 0,
            snd_wnd: 65535,
            // Timestamps
            ts_recent: 0,
            ts_echo: 0,
            ts_val: 0,
        }
    }
    
    pub fn connect(&mut self, remote: SocketAddr) -> Result<(), NetError> {
        if self.state != TcpState::Closed {
            return Err(NetError::ProtocolError);
        }
        
        self.remote = remote;
        self.seq_num = crate::random::rand_u64() as u32;
        self.state = TcpState::SynSent;
        
        // Send SYN
        self.send_packet(TcpFlags::syn(), &[])?;
        
        Ok(())
    }
    
    pub fn listen(&mut self, backlog: usize) -> Result<(), NetError> {
        if self.state != TcpState::Closed {
            return Err(NetError::ProtocolError);
        }
        
        self.state = TcpState::Listen;
        self.listen_backlog = backlog;
        
        Ok(())
    }
    
    pub fn accept(&mut self) -> Result<SocketAddr, NetError> {
        if self.state != TcpState::Listen {
            return Err(NetError::ProtocolError);
        }
        
        // Check for pending connections
        // TODO: Implement accept queue
        
        Err(NetError::WouldBlock)
    }
    
    pub fn send(&mut self, data: &[u8]) -> Result<usize, NetError> {
        if self.state != TcpState::Established {
            return Err(NetError::ConnectionClosed);
        }
        
        self.send_packet(TcpFlags::ack(), data)?;
        self.seq_num = self.seq_num.wrapping_add(data.len() as u32);
        
        Ok(data.len())
    }
    
    pub fn recv(&mut self, buf: &mut [u8]) -> Result<usize, NetError> {
        if self.rx_buffer.is_empty() {
            if self.state == TcpState::CloseWait || self.state == TcpState::Closed {
                return Err(NetError::ConnectionClosed);
            }
            return Err(NetError::WouldBlock);
        }
        
        let len = buf.len().min(self.rx_buffer.len());
        buf[..len].copy_from_slice(&self.rx_buffer[..len]);
        self.rx_buffer.drain(..len);
        
        Ok(len)
    }
    
    pub fn close(&mut self) -> Result<(), NetError> {
        match self.state {
            TcpState::Established => {
                self.state = TcpState::FinWait1;
                self.send_packet(TcpFlags::fin_ack(), &[])?;
            }
            TcpState::CloseWait => {
                self.state = TcpState::LastAck;
                self.send_packet(TcpFlags::fin_ack(), &[])?;
            }
            _ => {
                self.state = TcpState::Closed;
            }
        }
        
        Ok(())
    }
    
    fn send_packet(&mut self, flags: TcpFlags, data: &[u8]) -> Result<(), NetError> {
        let mut header = TcpHeader {
            src_port: self.local.port,
            dst_port: self.remote.port,
            seq_num: self.seq_num,
            ack_num: self.ack_num,
            data_offset: 5,
            flags,
            window_size: self.window_size,
            checksum: 0,
            urgent_ptr: 0,
        };
        
        // Build TCP segment
        let mut segment = vec![0u8; TcpHeader::MIN_SIZE + data.len()];
        header.serialize(&mut segment)?;
        segment[TcpHeader::MIN_SIZE..].copy_from_slice(data);
        
        // Compute checksum with pseudo-header
        let src_ip = super::local_ip();
        header.checksum = compute_checksum(src_ip, self.remote.ip, &segment);
        header.serialize(&mut segment)?;
        
        // Send via IP layer
        let mut ip_buf = vec![0u8; 1500];
        let len = super::ip::build_packet(
            self.remote.ip,
            IpProtocol::TCP,
            &segment,
            &mut ip_buf,
        )?;
        
        // Build Ethernet frame and send
        super::send_packet(&ip_buf[..len])?;
        
        Ok(())
    }
    
    fn on_packet(&mut self, header: &TcpHeader, data: &[u8]) -> Result<(), NetError> {
        // Update ACK number
        if header.flags.ack {
            self.ack_num = header.ack_num;
        }
        
        // State machine
        match self.state {
            TcpState::Listen => {
                if header.flags.syn {
                    // New connection attempt
                    self.remote = SocketAddr::new(
                        // IP would come from IP layer
                        Ipv4Addr::UNSPECIFIED,
                        header.src_port,
                    );
                    self.seq_num = crate::random::rand_u64() as u32;
                    self.ack_num = header.seq_num.wrapping_add(1);
                    self.state = TcpState::SynReceived;
                    
                    // Send SYN-ACK
                    self.send_packet(TcpFlags::syn_ack(), &[])?;
                }
            }
            TcpState::SynSent => {
                if header.flags.syn && header.flags.ack {
                    // SYN-ACK received
                    self.ack_num = header.seq_num.wrapping_add(1);
                    self.state = TcpState::Established;
                    
                    // Send ACK
                    self.send_packet(TcpFlags::ack(), &[])?;
                }
            }
            TcpState::SynReceived => {
                if header.flags.ack {
                    // Connection established
                    self.state = TcpState::Established;
                }
            }
            TcpState::Established => {
                // Receive data
                if !data.is_empty() {
                    self.rx_buffer.extend_from_slice(data);
                    self.ack_num = self.ack_num.wrapping_add(data.len() as u32);
                    
                    // Send ACK
                    self.send_packet(TcpFlags::ack(), &[])?;
                }
                
                // FIN received
                if header.flags.fin {
                    self.state = TcpState::CloseWait;
                    self.ack_num = self.ack_num.wrapping_add(1);
                    self.send_packet(TcpFlags::ack(), &[])?;
                }
            }
            TcpState::FinWait1 => {
                if header.flags.ack {
                    self.state = TcpState::FinWait2;
                }
            }
            TcpState::FinWait2 => {
                if header.flags.fin {
                    self.ack_num = self.ack_num.wrapping_add(1);
                    self.send_packet(TcpFlags::ack(), &[])?;
                    self.state = TcpState::TimeWait;
                }
            }
            TcpState::LastAck => {
                if header.flags.ack {
                    self.state = TcpState::Closed;
                }
            }
            _ => {}
        }
        
        Ok(())
    }
    
    /// Update RTT estimate (Jacobson/Karels algorithm)
    pub fn update_rtt(&mut self, measured_rtt: u32) {
        let delta = if measured_rtt > self.rtt {
            measured_rtt - self.rtt
        } else {
            self.rtt - measured_rtt
        };
        
        self.rtt_var = (3 * self.rtt_var + delta) / 4;
        self.rtt = (7 * self.rtt + measured_rtt) / 8;
        self.rto = self.rtt + 4 * self.rtt_var;
        
        // Clamp RTO
        if self.rto < 200 { self.rto = 200; }
        if self.rto > 60000 { self.rto = 60000; }
    }
}

// ============================================================================
// TCP CUBIC CONGESTION CONTROL
// ============================================================================

/// CUBIC congestion control state
#[derive(Clone, Debug)]
pub struct CubicState {
    /// CUBIC window increase constant (beta_cubic)
    pub beta: f64,
    /// CUBIC window decrease constant (C)
    pub c: f64,
    /// Window maximum before last congestion event (W_max)
    pub w_max: f64,
    /// Time of last congestion event (in ms)
    pub t_last: u64,
    /// Current cwnd in bytes
    pub cwnd: u32,
    /// Slow start threshold
    pub ssthresh: u32,
    /// Minimum RTT observed
    pub min_rtt: u32,
    /// TCP-friendly window estimate
    pub tcp_cwnd: f64,
}

impl CubicState {
    /// CUBIC beta = 0.7 (multiplicative decrease factor)
    const BETA: f64 = 0.7;
    /// CUBIC C = 0.4 (window growth factor)
    const C: f64 = 0.4;
    /// MSS for calculations
    const MSS: u32 = 1460;
    
    pub fn new() -> Self {
        CubicState {
            beta: Self::BETA,
            c: Self::C,
            w_max: 0.0,
            t_last: 0,
            cwnd: Self::MSS * 10, // Initial 10 MSS
            ssthresh: Self::MSS * 100,
            min_rtt: 1000,
            tcp_cwnd: Self::MSS as f64 * 10.0,
        }
    }
    
    /// Cube root approximation (Newton-Raphson)
    fn cbrt(x: f64) -> f64 {
        if x <= 0.0 {
            return 0.0;
        }
        // Initial guess using bit manipulation
        let mut y = x;
        for _ in 0..10 {
            let y3 = y * y * y;
            if y3 == 0.0 {
                break;
            }
            y = y - (y3 - x) / (3.0 * y * y);
        }
        y
    }
    
    /// Integer power for f64
    fn powi(base: f64, exp: i32) -> f64 {
        if exp == 0 {
            return 1.0;
        }
        let abs_exp = if exp < 0 { -exp } else { exp };
        let mut result = 1.0;
        for _ in 0..abs_exp {
            result *= base;
        }
        if exp < 0 {
            1.0 / result
        } else {
            result
        }
    }
    
    /// Calculate CUBIC window at time t
    /// W(t) = C*(t-K)^3 + W_max
    /// where K = cubic_root(W_max*beta/C)
    pub fn cubic_window(&self, t_ms: u64) -> f64 {
        if self.w_max == 0.0 {
            return self.cwnd as f64;
        }
        
        // K = cubic_root(W_max * beta / C)
        let k = Self::cbrt(self.w_max * self.beta / self.c);
        
        // Time since last congestion event
        let t = (t_ms as f64 - self.t_last as f64) / 1000.0; // Convert to seconds
        
        // W(t) = C * (t - K)^3 + W_max
        let w = self.c * Self::powi(t - k, 3) + self.w_max;
        
        w.max(Self::MSS as f64)
    }
    
    /// Calculate TCP-friendly window (for TCP friendliness)
    /// W_tcp(t) = W_max * (1 - beta) + 3 * beta / (2 - beta) * (t / RTT)
    pub fn tcp_friendly_window(&self, t_ms: u64, rtt_ms: u32) -> f64 {
        let t = (t_ms as f64 - self.t_last as f64) / 1000.0;
        let rtt = rtt_ms as f64 / 1000.0;
        
        if rtt == 0.0 {
            return self.tcp_cwnd;
        }
        
        // TCP-friendly increase rate
        let alpha = 3.0 * self.beta / (2.0 - self.beta);
        let w_tcp = self.w_max * (1.0 - self.beta) + alpha * t / rtt;
        
        w_tcp.max(Self::MSS as f64)
    }
    
    /// Update cwnd on ACK (CUBIC algorithm)
    pub fn on_ack(&mut self, acked_bytes: u32, current_time_ms: u64, rtt_ms: u32) {
        // Update min RTT
        if rtt_ms < self.min_rtt && rtt_ms > 0 {
            self.min_rtt = rtt_ms;
        }
        
        // Slow start phase
        if self.cwnd < self.ssthresh {
            self.cwnd += acked_bytes;
            return;
        }
        
        // CUBIC congestion avoidance
        let cubic_w = self.cubic_window(current_time_ms);
        let tcp_w = self.tcp_friendly_window(current_time_ms, rtt_ms);
        
        // Take the maximum of CUBIC and TCP-friendly
        let target_w = cubic_w.max(tcp_w);
        
        // Convert to bytes and update
        let current_w = self.cwnd as f64;
        let new_w = current_w + (target_w - current_w) * (acked_bytes as f64 / self.cwnd as f64);
        self.cwnd = new_w.max(Self::MSS as f64) as u32;
    }
    
    /// Handle congestion event (packet loss)
    pub fn on_loss(&mut self, current_time_ms: u64) {
        // Save current window as W_max
        self.w_max = self.cwnd as f64;
        
        // Multiplicative decrease: W_max = W_max * beta
        self.w_max *= self.beta;
        
        // Set new cwnd
        self.cwnd = (self.cwnd as f64 * self.beta).max(Self::MSS as f64) as u32;
        self.ssthresh = self.cwnd;
        
        // Record time of congestion event
        self.t_last = current_time_ms;
    }
    
    /// Handle timeout
    pub fn on_timeout(&mut self, current_time_ms: u64) {
        // Save current window
        self.w_max = self.cwnd as f64;
        
        // Reset to 1 MSS
        self.cwnd = Self::MSS;
        self.ssthresh = Self::MSS * 2;
        
        // Record time
        self.t_last = current_time_ms;
    }
}

// ============================================================================
// TCP BBR CONGESTION CONTROL
// ============================================================================

/// BBR congestion control state
#[derive(Clone, Debug, Default)]
pub struct BbrState {
    /// BBR mode
    pub mode: BbrMode,
    /// Estimated bandwidth (bytes/second)
    pub bw: u64,
    /// Minimum RTT observed (microseconds)
    pub min_rtt: u64,
    /// Round trip time counter
    pub rtprop: u64,
    /// Time of last RTprop update
    pub rtprop_stamp: u64,
    /// Pacing rate (bytes/second)
    pub pacing_rate: u64,
    /// Send quantum (bytes)
    pub send_quantum: u32,
    /// Cwnd gain
    pub cwnd_gain: f64,
    /// Pacing gain
    pub pacing_gain: f64,
    /// BBR round counter
    pub round_count: u64,
    /// Next round delimiter
    pub next_round_delivered: u64,
    /// Bandwidth filter
    pub bw_filter: BbrBwFilter,
    /// RTT filter
    pub rtt_filter: BbrRttFilter,
    /// ProbeRTT state
    pub probe_rtt_done: bool,
    /// ProbeRTT round stamp
    pub probe_rtt_round_stamp: u64,
}

/// BBR modes
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BbrMode {
    #[default]
    Startup,
    Drain,
    ProbeBW,
    ProbeRTT,
}

/// BBR bandwidth filter (max bandwidth over last 10 rounds)
#[derive(Clone, Debug)]
pub struct BbrBwFilter {
    pub samples: [u64; 10],
    pub count: usize,
}

impl Default for BbrBwFilter {
    fn default() -> Self {
        BbrBwFilter {
            samples: [0; 10],
            count: 0,
        }
    }
}

impl BbrBwFilter {
    pub fn update(&mut self, bw: u64) {
        self.samples[self.count % 10] = bw;
        self.count += 1;
    }
    
    pub fn max(&self) -> u64 {
        self.samples.iter().max().copied().unwrap_or(0)
    }
}

/// BBR RTT filter (min RTT over 10-second window)
#[derive(Clone, Debug)]
pub struct BbrRttFilter {
    pub min_rtt: u64,
    pub stamp: u64,
}

impl Default for BbrRttFilter {
    fn default() -> Self {
        BbrRttFilter {
            min_rtt: u64::MAX,
            stamp: 0,
        }
    }
}

impl BbrRttFilter {
    pub fn update(&mut self, rtt: u64, now: u64) {
        // 10-second window
        if now - self.stamp > 10_000_000 {
            self.min_rtt = rtt;
            self.stamp = now;
        } else if rtt < self.min_rtt {
            self.min_rtt = rtt;
        }
    }
}

impl BbrState {
    /// BBR constants
    const BBR_HIGH_GAIN: f64 = 2.89;      // 2/ln(2) for startup
    const BBR_DRAIN_GAIN: f64 = 0.35;     // 1/2.89 for drain
    const BBR_CWND_GAIN_TARGET: f64 = 2.0;
    const BBR_PROBE_RTT_CWND_GAIN: f64 = 0.5;
    const BBR_PROBE_RTT_MODE_DURATION_MS: u64 = 200;
    const BBR_MIN_RTT_WIN_SEC: u64 = 10;
    
    pub fn new() -> Self {
        BbrState {
            mode: BbrMode::Startup,
            bw: 0,
            min_rtt: 0,
            rtprop: 0,
            rtprop_stamp: 0,
            pacing_rate: 0,
            send_quantum: 1460,
            cwnd_gain: Self::BBR_HIGH_GAIN,
            pacing_gain: Self::BBR_HIGH_GAIN,
            round_count: 0,
            next_round_delivered: 0,
            bw_filter: BbrBwFilter::default(),
            rtt_filter: BbrRttFilter::default(),
            probe_rtt_done: false,
            probe_rtt_round_stamp: 0,
        }
    }
    
    /// Calculate pacing rate
    pub fn set_pacing_rate(&mut self) {
        let bw = self.bw as f64;
        let gain = self.pacing_gain;
        self.pacing_rate = (bw * gain) as u64;
    }
    
    /// Calculate send quantum
    pub fn set_send_quantum(&mut self) {
        // Send quantum = min(64KB, pacing_rate / 1000)
        let quantum = (self.pacing_rate / 1000).min(65536) as u32;
        self.send_quantum = quantum.max(1460);
    }
    
    /// Calculate target cwnd
    pub fn target_cwnd(&self) -> u32 {
        // BDP = bw * min_rtt (in bytes)
        let bdp = if self.min_rtt > 0 {
            (self.bw * self.min_rtt / 1_000_000) as u32 // Convert us to ms
        } else {
            1460
        };
        
        // target_cwnd = cwnd_gain * BDP
        let target = (self.cwnd_gain * bdp as f64) as u32;
        target.max(1460)
    }
    
    /// BBR on ACK processing
    pub fn on_ack(&mut self, delivered: u32, rtt_us: u64, now_us: u64) {
        // Update bandwidth estimate
        if rtt_us > 0 {
            let bw_sample = (delivered as u64 * 1_000_000) / rtt_us;
            self.bw_filter.update(bw_sample);
            self.bw = self.bw_filter.max();
        }
        
        // Update RTT estimate
        self.rtt_filter.update(rtt_us, now_us);
        self.min_rtt = self.rtt_filter.min_rtt;
        
        // Check for round trip
        self.round_count += 1;
        
        // Mode-specific processing
        match self.mode {
            BbrMode::Startup => {
                // Check if we've found the bottleneck
                if self.is_full_bw_reached() {
                    self.mode = BbrMode::Drain;
                    self.pacing_gain = Self::BBR_DRAIN_GAIN;
                    self.cwnd_gain = Self::BBR_CWND_GAIN_TARGET;
                }
            }
            BbrMode::Drain => {
                // Wait for queue to drain
                if self.target_cwnd() <= 1460 {
                    self.mode = BbrMode::ProbeBW;
                    self.pacing_gain = 1.0;
                    self.cwnd_gain = Self::BBR_CWND_GAIN_TARGET;
                }
            }
            BbrMode::ProbeBW => {
                // Cycle through gains: 1.25, 0.75, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0
                let cycle_idx = (self.round_count / 8) % 8;
                self.pacing_gain = match cycle_idx {
                    0 => 1.25,
                    1 => 0.75,
                    _ => 1.0,
                };
                
                // Check if we need to probe RTT
                if now_us - self.rtprop_stamp > Self::BBR_MIN_RTT_WIN_SEC * 1_000_000 {
                    self.mode = BbrMode::ProbeRTT;
                    self.cwnd_gain = Self::BBR_PROBE_RTT_CWND_GAIN;
                }
            }
            BbrMode::ProbeRTT => {
                // Stay in ProbeRTT for 200ms
                if now_us - self.rtprop_stamp > Self::BBR_PROBE_RTT_MODE_DURATION_MS * 1000 {
                    self.mode = BbrMode::ProbeBW;
                    self.cwnd_gain = Self::BBR_CWND_GAIN_TARGET;
                }
            }
        }
        
        // Update pacing rate
        self.set_pacing_rate();
        self.set_send_quantum();
    }
    
    /// Check if full bandwidth is reached (startup phase)
    fn is_full_bw_reached(&self) -> bool {
        // Simplified: check if bandwidth hasn't increased significantly
        // In real BBR, this tracks bandwidth growth rate
        self.bw > 0 && self.round_count > 3
    }
    
    /// Handle loss (BBR doesn't reduce on loss, uses bandwidth estimation)
    pub fn on_loss(&mut self) {
        // BBR doesn't react to loss directly
        // It relies on bandwidth estimation
    }
    
    /// Handle timeout
    pub fn on_timeout(&mut self, now_us: u64) {
        // Reset to startup mode
        self.mode = BbrMode::Startup;
        self.cwnd_gain = Self::BBR_HIGH_GAIN;
        self.pacing_gain = Self::BBR_HIGH_GAIN;
        self.rtprop_stamp = now_us;
    }
    
    /// Get current cwnd
    pub fn cwnd(&self) -> u32 {
        match self.mode {
            BbrMode::ProbeRTT => 1460, // 4 segments minimum
            _ => self.target_cwnd(),
        }
    }
}

// ============================================================================
// CONGESTION CONTROL ALGORITHM SELECTION
// ============================================================================

/// Congestion control algorithm
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CcAlgorithm {
    #[default]
    Reno,
    Cubic,
    Bbr,
}

/// Congestion control state
#[derive(Clone, Debug)]
pub struct CcState {
    pub algorithm: CcAlgorithm,
    pub reno: RenoState,
    pub cubic: CubicState,
    pub bbr: BbrState,
}

/// Reno state (basic TCP congestion control)
#[derive(Clone, Debug)]
pub struct RenoState {
    pub cwnd: u32,
    pub ssthresh: u32,
    pub rtt: u32,
    pub rtt_var: u32,
    pub rto: u32,
}

impl Default for RenoState {
    fn default() -> Self {
        RenoState {
            cwnd: 1460 * 10,
            ssthresh: 1460 * 100,
            rtt: 1000,
            rtt_var: 500,
            rto: 1000,
        }
    }
}

impl CcState {
    pub fn new(algorithm: CcAlgorithm) -> Self {
        CcState {
            algorithm,
            reno: RenoState::default(),
            cubic: CubicState::new(),
            bbr: BbrState::new(),
        }
    }
    
    pub fn cwnd(&self) -> u32 {
        match self.algorithm {
            CcAlgorithm::Reno => self.reno.cwnd,
            CcAlgorithm::Cubic => self.cubic.cwnd,
            CcAlgorithm::Bbr => self.bbr.cwnd(),
        }
    }
    
    pub fn on_ack(&mut self, acked_bytes: u32, current_time_ms: u64, rtt_ms: u32) {
        match self.algorithm {
            CcAlgorithm::Reno => {
                if self.reno.cwnd < self.reno.ssthresh {
                    self.reno.cwnd += acked_bytes;
                } else {
                    self.reno.cwnd += (1460 * acked_bytes) / self.reno.cwnd;
                }
            }
            CcAlgorithm::Cubic => {
                self.cubic.on_ack(acked_bytes, current_time_ms, rtt_ms);
            }
            CcAlgorithm::Bbr => {
                self.bbr.on_ack(acked_bytes, rtt_ms as u64 * 1000, current_time_ms * 1000);
            }
        }
    }
    
    pub fn on_loss(&mut self, current_time_ms: u64) {
        match self.algorithm {
            CcAlgorithm::Reno => {
                self.reno.ssthresh = self.reno.cwnd / 2;
                self.reno.cwnd = self.reno.ssthresh;
            }
            CcAlgorithm::Cubic => {
                self.cubic.on_loss(current_time_ms);
            }
            CcAlgorithm::Bbr => {
                self.bbr.on_loss();
            }
        }
    }
    
    pub fn on_timeout(&mut self, current_time_ms: u64) {
        match self.algorithm {
            CcAlgorithm::Reno => {
                self.reno.ssthresh = self.reno.cwnd / 2;
                self.reno.cwnd = 1460;
            }
            CcAlgorithm::Cubic => {
                self.cubic.on_timeout(current_time_ms);
            }
            CcAlgorithm::Bbr => {
                self.bbr.on_timeout(current_time_ms * 1000);
            }
        }
    }
}

// ============================================================================
// TCP MANAGER
// ============================================================================

static TCP_CONNECTIONS: Mutex<BTreeMap<u32, Box<TcpConnection>>> = Mutex::new(BTreeMap::new());
static TCP_LISTENERS: Mutex<BTreeMap<Port, u32>> = Mutex::new(BTreeMap::new());

/// Initialize TCP subsystem
pub fn init() {
    crate::serial_println!("[TCP] Initialized");
}

/// Create TCP socket
pub fn create_socket() -> u32 {
    let conn = TcpConnection::new(SocketAddr::default());
    let id = conn.id;
    TCP_CONNECTIONS.lock().insert(id, Box::new(conn));
    id
}

/// Bind TCP socket
pub fn bind(socket_id: u32, addr: SocketAddr) -> Result<(), NetError> {
    let mut conns = TCP_CONNECTIONS.lock();
    let conn = conns.get_mut(&socket_id).ok_or(NetError::ProtocolError)?;
    conn.local = addr;
    Ok(())
}

/// Connect TCP socket
pub fn connect(socket_id: u32, remote: SocketAddr) -> Result<(), NetError> {
    let mut conns = TCP_CONNECTIONS.lock();
    let conn = conns.get_mut(&socket_id).ok_or(NetError::ProtocolError)?;
    conn.connect(remote)
}

/// Listen on TCP socket
pub fn listen(socket_id: u32, backlog: usize) -> Result<(), NetError> {
    let mut conns = TCP_CONNECTIONS.lock();
    let conn = conns.get_mut(&socket_id).ok_or(NetError::ProtocolError)?;
    conn.listen(backlog)?;
    
    // Register listener
    let mut listeners = TCP_LISTENERS.lock();
    listeners.insert(conn.local.port, socket_id);
    
    Ok(())
}

/// Accept connection
pub fn accept(socket_id: u32) -> Result<(u32, SocketAddr), NetError> {
    let conns = TCP_CONNECTIONS.lock();
    let conn = conns.get(&socket_id).ok_or(NetError::ProtocolError)?;
    
    if conn.state != TcpState::Listen {
        return Err(NetError::ProtocolError);
    }
    
    // TODO: Check accept queue
    Err(NetError::WouldBlock)
}

/// Send data
pub fn send(socket_id: u32, data: &[u8]) -> Result<usize, NetError> {
    let mut conns = TCP_CONNECTIONS.lock();
    let conn = conns.get_mut(&socket_id).ok_or(NetError::ProtocolError)?;
    conn.send(data)
}

/// Receive data
pub fn recv(socket_id: u32, buf: &mut [u8]) -> Result<usize, NetError> {
    let mut conns = TCP_CONNECTIONS.lock();
    let conn = conns.get_mut(&socket_id).ok_or(NetError::ProtocolError)?;
    conn.recv(buf)
}

/// Close socket
pub fn close(socket_id: u32) -> Result<(), NetError> {
    let mut conns = TCP_CONNECTIONS.lock();
    if let Some(conn) = conns.get_mut(&socket_id) {
        conn.close()?;
    }
    Ok(())
}

/// Get connection by ID (for event checking)
pub fn get_connection(socket_id: u32) -> Option<TcpConnection> {
    let conns = TCP_CONNECTIONS.lock();
    conns.get(&socket_id).map(|c| (**c).clone())
}

/// Get all connections (for ss utility)
pub fn get_all_connections() -> Vec<TcpConnection> {
    let conns = TCP_CONNECTIONS.lock();
    conns.values().map(|c| (**c).clone()).collect()
}

/// Process incoming TCP packet
pub fn process_packet(ip_packet: &Ipv4Packet) -> Result<(), NetError> {
    let tcp_header = TcpHeader::parse(ip_packet.payload)?;
    let data = &ip_packet.payload[tcp_header.header_len()..];
    
    // Find connection
    let conns = TCP_CONNECTIONS.lock();
    
    // Look for established connection
    let mut found_id = None;
    for (_, conn) in conns.iter() {
        if conn.local.port == tcp_header.dst_port && 
           conn.remote.port == tcp_header.src_port {
            found_id = Some(conn.id);
            break;
        }
    }
    drop(conns);
    
    if let Some(id) = found_id {
        let mut conns = TCP_CONNECTIONS.lock();
        if let Some(conn) = conns.get_mut(&id) {
            conn.remote.ip = ip_packet.header.src;
            return conn.on_packet(&tcp_header, data);
        }
    }
    
    // Check for listener
    let listeners = TCP_LISTENERS.lock();
    if let Some(&_socket_id) = listeners.get(&tcp_header.dst_port) {
        drop(listeners);
        
        let mut conns = TCP_CONNECTIONS.lock();
        let port_as_key = tcp_header.dst_port.0 as u32;
        if let Some(conn) = conns.get_mut(&port_as_key) {
            conn.remote.ip = ip_packet.header.src;
            return conn.on_packet(&tcp_header, data);
        }
    }
    
    // No matching connection - send RST
    Ok(())
}
