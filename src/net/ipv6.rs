//! # IPv6 Protocol
//!
//! IPv6 header and address handling

use alloc::string::String;
use alloc::format;
use core::str::FromStr;

// ============================================================================
// IPv6 ADDRESS
// ============================================================================

/// IPv6 address (128-bit)
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ipv6Addr(pub [u8; 16]);

impl Ipv6Addr {
    /// Unspecified address (::)
    pub const UNSPECIFIED: Self = Ipv6Addr([0; 16]);
    
    /// Loopback address (::1)
    pub const LOOPBACK: Self = Ipv6Addr([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
    
    /// Link-local prefix (fe80::)
    pub const LINK_LOCAL_PREFIX: [u8; 8] = [0xfe, 0x80, 0, 0, 0, 0, 0, 0];
    
    /// Create new IPv6 address from bytes
    pub const fn new(bytes: [u8; 16]) -> Self {
        Ipv6Addr(bytes)
    }
    
    /// Create from 8 16-bit segments
    pub const fn from_segments(segments: [u16; 8]) -> Self {
        Ipv6Addr([
            (segments[0] >> 8) as u8, (segments[0] & 0xFF) as u8,
            (segments[1] >> 8) as u8, (segments[1] & 0xFF) as u8,
            (segments[2] >> 8) as u8, (segments[2] & 0xFF) as u8,
            (segments[3] >> 8) as u8, (segments[3] & 0xFF) as u8,
            (segments[4] >> 8) as u8, (segments[4] & 0xFF) as u8,
            (segments[5] >> 8) as u8, (segments[5] & 0xFF) as u8,
            (segments[6] >> 8) as u8, (segments[6] & 0xFF) as u8,
            (segments[7] >> 8) as u8, (segments[7] & 0xFF) as u8,
        ])
    }
    
    /// Get as bytes
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
    
    /// Get as 16-bit segments
    pub fn segments(&self) -> [u16; 8] {
        [
            u16::from_be_bytes([self.0[0], self.0[1]]),
            u16::from_be_bytes([self.0[2], self.0[3]]),
            u16::from_be_bytes([self.0[4], self.0[5]]),
            u16::from_be_bytes([self.0[6], self.0[7]]),
            u16::from_be_bytes([self.0[8], self.0[9]]),
            u16::from_be_bytes([self.0[10], self.0[11]]),
            u16::from_be_bytes([self.0[12], self.0[13]]),
            u16::from_be_bytes([self.0[14], self.0[15]]),
        ]
    }
    
    /// Check if unspecified (::)
    pub fn is_unspecified(&self) -> bool {
        self.0 == [0; 16]
    }
    
    /// Check if loopback (::1)
    pub fn is_loopback(&self) -> bool {
        *self == Self::LOOPBACK
    }
    
    /// Check if link-local (fe80::/10)
    pub fn is_link_local(&self) -> bool {
        (self.0[0] & 0xFF) == 0xFE && (self.0[1] & 0xC0) == 0x80
    }
    
    /// Check if unique local (fc00::/7)
    pub fn is_unique_local(&self) -> bool {
        (self.0[0] & 0xFE) == 0xFC
    }
    
    /// Check if global unicast
    pub fn is_global(&self) -> bool {
        // Global unicast: 2000::/3
        (self.0[0] & 0xE0) == 0x20
    }
    
    /// Check if multicast (ff00::/8)
    pub fn is_multicast(&self) -> bool {
        self.0[0] == 0xFF
    }
    
    /// Check if IPv4-mapped (::ffff:0:0/96)
    pub fn is_ipv4_mapped(&self) -> bool {
        self.0[0..10] == [0; 10] && self.0[10..12] == [0xFF, 0xFF]
    }
    
    /// Convert to IPv4 if mapped
    pub fn to_ipv4_mapped(&self) -> Option<super::Ipv4Addr> {
        if self.is_ipv4_mapped() {
            Some(super::Ipv4Addr::from_bytes([self.0[12], self.0[13], self.0[14], self.0[15]]))
        } else {
            None
        }
    }
    
    /// Create IPv4-mapped IPv6 address
    pub fn from_ipv4_mapped(ipv4: super::Ipv4Addr) -> Self {
        let bytes = ipv4.as_bytes();
        Ipv6Addr([
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xFF, 0xFF,
            bytes[0], bytes[1], bytes[2], bytes[3],
        ])
    }
    
    /// Get scope ID for link-local addresses
    pub fn scope_id(&self) -> Option<u32> {
        if self.is_link_local() {
            Some(u32::from_be_bytes([self.0[12], self.0[13], self.0[14], self.0[15]]))
        } else {
            None
        }
    }
    
    /// Format as string
    pub fn to_string(&self) -> String {
        if self.is_ipv4_mapped() {
            if let Some(ipv4) = self.to_ipv4_mapped() {
                return format!("::ffff:{}.{}.{}.{}", ipv4.0[0], ipv4.0[1], ipv4.0[2], ipv4.0[3]);
            }
        }
        
        if self.is_loopback() {
            return String::from("::1");
        }
        
        if self.is_unspecified() {
            return String::from("::");
        }
        
        let segments = self.segments();
        
        // Find longest run of zeros for ::
        let mut longest_start = 0;
        let mut longest_len = 0;
        let mut current_start = 0;
        let mut current_len = 0;
        
        for i in 0..8 {
            if segments[i] == 0 {
                if current_len == 0 {
                    current_start = i;
                }
                current_len += 1;
                if current_len > longest_len {
                    longest_len = current_len;
                    longest_start = current_start;
                }
            } else {
                current_len = 0;
            }
        }
        
        let mut result = String::new();
        let mut i = 0;
        
        while i < 8 {
            if i == longest_start && longest_len > 1 {
                if i == 0 {
                    result.push(':');
                }
                result.push(':');
                i += longest_len;
            } else {
                if i > 0 {
                    result.push(':');
                }
                result.push_str(&format!("{:x}", segments[i]));
                i += 1;
            }
        }
        
        result
    }
}

impl FromStr for Ipv6Addr {
    type Err = ();
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut segments = [0u16; 8];
        let mut seg_idx = 0;
        let mut double_colon_pos = None;
        let mut parts = s.split(':');
        let mut before_double_colon = alloc::vec::Vec::new();
        let mut after_double_colon = alloc::vec::Vec::new();
        let mut found_double_colon = false;
        
        // Handle special case of leading ::
        let chars: alloc::vec::Vec<char> = s.chars().collect();
        let mut idx = 0;
        
        while idx < chars.len() {
            if chars[idx] == ':' {
                if idx + 1 < chars.len() && chars[idx + 1] == ':' {
                    found_double_colon = true;
                    double_colon_pos = Some(before_double_colon.len());
                    idx += 2;
                    // Parse rest after ::
                    while idx < chars.len() {
                        let mut hex_str = String::new();
                        while idx < chars.len() && chars[idx] != ':' {
                            hex_str.push(chars[idx]);
                            idx += 1;
                        }
                        if !hex_str.is_empty() {
                            if let Ok(val) = u16::from_str_radix(&hex_str, 16) {
                                after_double_colon.push(val);
                            }
                        }
                        idx += 1;
                    }
                    break;
                } else {
                    idx += 1;
                }
            } else {
                let mut hex_str = String::new();
                while idx < chars.len() && chars[idx] != ':' {
                    hex_str.push(chars[idx]);
                    idx += 1;
                }
                if let Ok(val) = u16::from_str_radix(&hex_str, 16) {
                    before_double_colon.push(val);
                }
                idx += 1;
            }
        }
        
        if !found_double_colon {
            // No ::, must have exactly 8 segments
            let all_parts: alloc::vec::Vec<u16> = s.split(':')
                .filter(|p| !p.is_empty())
                .filter_map(|p| u16::from_str_radix(p, 16).ok())
                .collect();
            
            if all_parts.len() != 8 {
                return Err(());
            }
            segments.copy_from_slice(&all_parts);
        } else {
            let before_len = before_double_colon.len();
            let after_len = after_double_colon.len();
            let zero_count = 8 - before_len - after_len;
            
            if zero_count == 0 && before_len + after_len != 8 {
                return Err(());
            }
            
            for (i, &val) in before_double_colon.iter().enumerate() {
                segments[i] = val;
            }
            for (i, &val) in after_double_colon.iter().enumerate() {
                segments[before_len + zero_count + i] = val;
            }
        }
        
        Ok(Ipv6Addr::from_segments(segments))
    }
}

impl Default for Ipv6Addr {
    fn default() -> Self {
        Self::UNSPECIFIED
    }
}

// ============================================================================
// IPv6 HEADER
// ============================================================================

/// IPv6 header (40 bytes fixed)
#[derive(Clone, Copy, Debug)]
pub struct Ipv6Header {
    pub version: u8,           // 4 bits, always 6
    pub traffic_class: u8,     // 8 bits
    pub flow_label: u32,       // 20 bits
    pub payload_len: u16,      // Payload length (excluding header)
    pub next_header: u8,       // Next header type (like IPv4 protocol)
    pub hop_limit: u8,         // Like TTL
    pub src: Ipv6Addr,
    pub dst: Ipv6Addr,
}

impl Ipv6Header {
    pub const SIZE: usize = 40;
    
    pub fn new(src: Ipv6Addr, dst: Ipv6Addr, next_header: u8, payload_len: u16) -> Self {
        Ipv6Header {
            version: 6,
            traffic_class: 0,
            flow_label: 0,
            payload_len,
            next_header,
            hop_limit: 64,
            src,
            dst,
        }
    }
    
    pub fn parse(data: &[u8]) -> Result<Self, super::NetError> {
        if data.len() < Self::SIZE {
            return Err(super::NetError::InvalidPacket);
        }
        
        let version = (data[0] >> 4) & 0x0F;
        if version != 6 {
            return Err(super::NetError::InvalidPacket);
        }
        
        let traffic_class = ((data[0] & 0x0F) << 4) | ((data[1] >> 4) & 0x0F);
        let flow_label = ((data[1] as u32 & 0x0F) << 16) | 
                         ((data[2] as u32) << 8) | 
                         (data[3] as u32);
        
        let payload_len = u16::from_be_bytes([data[4], data[5]]);
        let next_header = data[6];
        let hop_limit = data[7];
        
        let mut src_bytes = [0u8; 16];
        src_bytes.copy_from_slice(&data[8..24]);
        let src = Ipv6Addr(src_bytes);
        
        let mut dst_bytes = [0u8; 16];
        dst_bytes.copy_from_slice(&data[24..40]);
        let dst = Ipv6Addr(dst_bytes);
        
        Ok(Ipv6Header {
            version,
            traffic_class,
            flow_label,
            payload_len,
            next_header,
            hop_limit,
            src,
            dst,
        })
    }
    
    pub fn serialize(&self, buf: &mut [u8]) -> Result<(), super::NetError> {
        if buf.len() < Self::SIZE {
            return Err(super::NetError::BufferFull);
        }
        
        // Version (4 bits) + Traffic class (8 bits) + Flow label (20 bits)
        buf[0] = (self.version << 4) | ((self.traffic_class >> 4) & 0x0F);
        buf[1] = ((self.traffic_class & 0x0F) << 4) | ((self.flow_label >> 16) as u8 & 0x0F);
        buf[2] = (self.flow_label >> 8) as u8;
        buf[3] = self.flow_label as u8;
        
        buf[4..6].copy_from_slice(&self.payload_len.to_be_bytes());
        buf[6] = self.next_header;
        buf[7] = self.hop_limit;
        
        buf[8..24].copy_from_slice(&self.src.0);
        buf[24..40].copy_from_slice(&self.dst.0);
        
        Ok(())
    }
}

/// IPv6 next header types
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ipv6NextHeader {
    HopByHop = 0,
    Tcp = 6,
    Udp = 17,
    Icmpv6 = 58,
    NoNextHeader = 59,
    DestinationOptions = 60,
    Fragment = 44,
    Authentication = 51,
    EncapsulatingSecurityPayload = 50,
}

impl Ipv6NextHeader {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => Ipv6NextHeader::HopByHop,
            6 => Ipv6NextHeader::Tcp,
            17 => Ipv6NextHeader::Udp,
            58 => Ipv6NextHeader::Icmpv6,
            59 => Ipv6NextHeader::NoNextHeader,
            60 => Ipv6NextHeader::DestinationOptions,
            44 => Ipv6NextHeader::Fragment,
            51 => Ipv6NextHeader::Authentication,
            50 => Ipv6NextHeader::EncapsulatingSecurityPayload,
            _ => Ipv6NextHeader::HopByHop,
        }
    }
}

// ============================================================================
// IPv6 PACKET
// ============================================================================

/// IPv6 packet
#[derive(Clone, Debug)]
pub struct Ipv6Packet {
    pub header: Ipv6Header,
    pub payload: alloc::vec::Vec<u8>,
}

impl Ipv6Packet {
    pub fn new(header: Ipv6Header, payload: &[u8]) -> Self {
        Ipv6Packet {
            header,
            payload: alloc::vec::Vec::from(payload),
        }
    }
    
    pub fn parse(data: &[u8]) -> Result<Self, super::NetError> {
        let header = Ipv6Header::parse(data)?;
        let payload_start = Ipv6Header::SIZE;
        let payload_end = payload_start + header.payload_len as usize;
        
        if payload_end > data.len() {
            return Err(super::NetError::InvalidPacket);
        }
        
        Ok(Ipv6Packet {
            header,
            payload: alloc::vec::Vec::from(&data[payload_start..payload_end]),
        })
    }
    
    pub fn serialize(&self) -> alloc::vec::Vec<u8> {
        let mut buf = alloc::vec![0u8; Ipv6Header::SIZE + self.payload.len()];
        self.header.serialize(&mut buf).ok();
        buf[Ipv6Header::SIZE..].copy_from_slice(&self.payload);
        buf
    }
    
    pub fn total_len(&self) -> usize {
        Ipv6Header::SIZE + self.payload.len()
    }
}

// ============================================================================
// IPv6 EXTENSION HEADERS (placeholder)
// ============================================================================

/// Hop-by-Hop Options header
#[derive(Clone, Debug)]
pub struct HopByHopHeader {
    pub next_header: u8,
    pub hdr_ext_len: u8,
    pub options: alloc::vec::Vec<u8>,
}

/// Fragment header
#[derive(Clone, Debug)]
pub struct FragmentHeader {
    pub next_header: u8,
    pub fragment_offset: u16,  // 13 bits
    pub more_fragments: bool,
    pub identification: u32,
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Generate IPv6 link-local address from MAC (EUI-64)
pub fn link_local_from_mac(mac: super::MacAddr) -> Ipv6Addr {
    let bytes = mac.as_bytes();
    
    // EUI-64: Insert FF:FE in middle
    let mut addr = [0u8; 16];
    addr[0] = 0xFE;
    addr[1] = 0x80;
    // Bytes 2-7 are zero
    addr[8] = bytes[0] ^ 0x02;  // Flip universal/local bit
    addr[9] = bytes[1];
    addr[10] = bytes[2];
    addr[11] = 0xFF;
    addr[12] = 0xFE;
    addr[13] = bytes[3];
    addr[14] = bytes[4];
    addr[15] = bytes[5];
    
    Ipv6Addr(addr)
}

/// Generate solicited-node multicast address
pub fn solicited_node_multicast(addr: &Ipv6Addr) -> Ipv6Addr {
    Ipv6Addr([
        0xFF, 0x02, 0, 0,
        0, 0, 0, 0,
        0, 0, 0, 1,
        0xFF,
        addr.0[13],
        addr.0[14],
        addr.0[15],
    ])
}

// ============================================================================
// ICMPv6 (Internet Control Message Protocol for IPv6)
// ============================================================================

/// ICMPv6 message types
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icmpv6Type {
    DestinationUnreachable = 1,
    PacketTooBig = 2,
    TimeExceeded = 3,
    ParameterProblem = 4,
    EchoRequest = 128,
    EchoReply = 129,
    RouterSolicitation = 133,
    RouterAdvertisement = 134,
    NeighborSolicitation = 135,
    NeighborAdvertisement = 136,
    Redirect = 137,
}

impl Icmpv6Type {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(Icmpv6Type::DestinationUnreachable),
            2 => Some(Icmpv6Type::PacketTooBig),
            3 => Some(Icmpv6Type::TimeExceeded),
            4 => Some(Icmpv6Type::ParameterProblem),
            128 => Some(Icmpv6Type::EchoRequest),
            129 => Some(Icmpv6Type::EchoReply),
            133 => Some(Icmpv6Type::RouterSolicitation),
            134 => Some(Icmpv6Type::RouterAdvertisement),
            135 => Some(Icmpv6Type::NeighborSolicitation),
            136 => Some(Icmpv6Type::NeighborAdvertisement),
            137 => Some(Icmpv6Type::Redirect),
            _ => None,
        }
    }
}

/// ICMPv6 header
#[derive(Clone, Debug)]
pub struct Icmpv6Header {
    pub msg_type: Icmpv6Type,
    pub code: u8,
    pub checksum: u16,
}

/// ICMPv6 Router Solicitation
#[derive(Clone, Debug)]
pub struct RouterSolicitation {
    pub header: Icmpv6Header,
    /// Source link-layer address option (optional)
    pub source_link_addr: Option<[u8; 6]>,
}

impl RouterSolicitation {
    pub fn new(source_mac: Option<super::MacAddr>) -> Self {
        RouterSolicitation {
            header: Icmpv6Header {
                msg_type: Icmpv6Type::RouterSolicitation,
                code: 0,
                checksum: 0,
            },
            source_link_addr: source_mac.map(|m| *m.as_bytes()),
        }
    }
    
    pub fn serialize(&self) -> alloc::vec::Vec<u8> {
        let mut buf = alloc::vec::Vec::new();
        
        // ICMPv6 header
        buf.push(self.header.msg_type as u8);
        buf.push(self.header.code);
        buf.extend_from_slice(&self.header.checksum.to_be_bytes());
        
        // Reserved (4 bytes)
        buf.extend_from_slice(&[0u8; 4]);
        
        // Source link-layer address option (type 1)
        if let Some(mac) = &self.source_link_addr {
            buf.push(1); // Option type
            buf.push(1); // Option length (in 8-byte units)
            buf.extend_from_slice(mac);
            buf.extend_from_slice(&[0u8; 2]); // Padding
        }
        
        buf
    }
}

/// ICMPv6 Router Advertisement
#[derive(Clone, Debug)]
pub struct RouterAdvertisement {
    pub header: Icmpv6Header,
    /// Current hop limit
    pub hop_limit: u8,
    /// Flags (M=Managed, O=Other, H=HomeAgent, P=Proxy, A=Autonomous)
    pub flags: u8,
    /// Router lifetime (seconds)
    pub router_lifetime: u16,
    /// Reachable time (milliseconds)
    pub reachable_time: u32,
    /// Retransmit timer (milliseconds)
    pub retransmit_timer: u32,
    /// Prefix options
    pub prefixes: alloc::vec::Vec<PrefixInfo>,
    /// DNS servers (RDNSS)
    pub dns_servers: alloc::vec::Vec<Ipv6Addr>,
    /// MTU
    pub mtu: Option<u32>,
}

/// Prefix Information option
#[derive(Clone, Debug)]
pub struct PrefixInfo {
    pub prefix: Ipv6Addr,
    pub prefix_len: u8,
    pub on_link: bool,
    pub autonomous: bool,
    pub valid_lifetime: u32,
    pub preferred_lifetime: u32,
}

impl RouterAdvertisement {
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 16 {
            return None;
        }
        
        let msg_type = Icmpv6Type::from_u8(data[0])?;
        if msg_type != Icmpv6Type::RouterAdvertisement {
            return None;
        }
        
        let mut ra = RouterAdvertisement {
            header: Icmpv6Header {
                msg_type,
                code: data[1],
                checksum: u16::from_be_bytes([data[2], data[3]]),
            },
            hop_limit: data[4],
            flags: data[5],
            router_lifetime: u16::from_be_bytes([data[6], data[7]]),
            reachable_time: u32::from_be_bytes([data[8], data[9], data[10], data[11]]),
            retransmit_timer: u32::from_be_bytes([data[12], data[13], data[14], data[15]]),
            prefixes: alloc::vec::Vec::new(),
            dns_servers: alloc::vec::Vec::new(),
            mtu: None,
        };
        
        // Parse options
        let mut offset = 16;
        while offset + 2 <= data.len() {
            let opt_type = data[offset];
            let opt_len = data[offset + 1] as usize * 8;
            
            if offset + opt_len > data.len() {
                break;
            }
            
            match opt_type {
                1 => {
                    // Source link-layer address
                }
                3 => {
                    // Prefix Information
                    if opt_len >= 32 {
                        let prefix_len = data[offset + 2];
                        let flags = data[offset + 3];
                        let valid_lifetime = u32::from_be_bytes([
                            data[offset + 4], data[offset + 5], data[offset + 6], data[offset + 7]
                        ]);
                        let preferred_lifetime = u32::from_be_bytes([
                            data[offset + 8], data[offset + 9], data[offset + 10], data[offset + 11]
                        ]);
                        
                        let mut prefix = [0u8; 16];
                        prefix.copy_from_slice(&data[offset + 16..offset + 32]);
                        
                        ra.prefixes.push(PrefixInfo {
                            prefix: Ipv6Addr(prefix),
                            prefix_len,
                            on_link: (flags & 0x80) != 0,
                            autonomous: (flags & 0x40) != 0,
                            valid_lifetime,
                            preferred_lifetime,
                        });
                    }
                }
                5 => {
                    // MTU option
                    if opt_len >= 8 {
                        ra.mtu = Some(u32::from_be_bytes([
                            data[offset + 4], data[offset + 5], data[offset + 6], data[offset + 7]
                        ]));
                    }
                }
                25 => {
                    // Recursive DNS Server (RDNSS)
                    if opt_len >= 24 {
                        let num_servers = (opt_len - 8) / 16;
                        for i in 0..num_servers {
                            let start = offset + 8 + i * 16;
                            if start + 16 <= data.len() {
                                let mut addr = [0u8; 16];
                                addr.copy_from_slice(&data[start..start + 16]);
                                ra.dns_servers.push(Ipv6Addr(addr));
                            }
                        }
                    }
                }
                _ => {}
            }
            
            offset += opt_len;
        }
        
        Some(ra)
    }
    
    /// Check if Managed flag is set (use DHCPv6 for addresses)
    pub fn use_dhcpv6(&self) -> bool {
        (self.flags & 0x80) != 0
    }
    
    /// Check if Other flag is set (use DHCPv6 for other config)
    pub fn use_dhcpv6_other(&self) -> bool {
        (self.flags & 0x40) != 0
    }
}

// ============================================================================
// SLAAC (Stateless Address Autoconfiguration)
// ============================================================================

/// SLAAC state
#[derive(Clone, Debug)]
pub struct SlaacState {
    /// Link-local address
    pub link_local: Ipv6Addr,
    /// Global addresses (prefix + EUI-64)
    pub global_addresses: alloc::vec::Vec<SlaacAddress>,
    /// Default gateway
    pub default_gateway: Option<Ipv6Addr>,
    /// DNS servers
    pub dns_servers: alloc::vec::Vec<Ipv6Addr>,
    /// MTU
    pub mtu: u32,
    /// Router lifetime
    pub router_lifetime: u32,
}

/// SLAAC address
#[derive(Clone, Debug)]
pub struct SlaacAddress {
    pub address: Ipv6Addr,
    pub prefix_len: u8,
    pub valid_lifetime: u32,
    pub preferred_lifetime: u32,
    pub created_at: u64,
}

impl SlaacState {
    pub fn new(mac: super::MacAddr) -> Self {
        SlaacState {
            link_local: link_local_from_mac(mac),
            global_addresses: alloc::vec::Vec::new(),
            default_gateway: None,
            dns_servers: alloc::vec::Vec::new(),
            mtu: 1500,
            router_lifetime: 0,
        }
    }
    
    /// Generate SLAAC address from prefix and MAC
    pub fn generate_address(prefix: &Ipv6Addr, prefix_len: u8, mac: super::MacAddr) -> Ipv6Addr {
        let mac_bytes = mac.as_bytes();
        
        // EUI-64 interface identifier
        let mut interface_id = [0u8; 8];
        interface_id[0] = mac_bytes[0] ^ 0x02; // Flip universal/local bit
        interface_id[1] = mac_bytes[1];
        interface_id[2] = mac_bytes[2];
        interface_id[3] = 0xFF;
        interface_id[4] = 0xFE;
        interface_id[5] = mac_bytes[3];
        interface_id[6] = mac_bytes[4];
        interface_id[7] = mac_bytes[5];
        
        // Combine prefix and interface ID
        let mut addr = [0u8; 16];
        
        // Copy prefix (up to prefix_len bits)
        let prefix_bytes = (prefix_len as usize + 7) / 8;
        for i in 0..prefix_bytes.min(8) {
            addr[i] = prefix.0[i];
        }
        
        // Append interface ID
        for i in 0..8 {
            addr[8 + i] = interface_id[i];
        }
        
        Ipv6Addr(addr)
    }
    
    /// Process Router Advertisement
    pub fn process_ra(&mut self, ra: &RouterAdvertisement, mac: super::MacAddr, current_time: u64) {
        // Update default gateway (source of RA)
        if ra.router_lifetime > 0 {
            // Gateway is the source of the RA (we'd need the source IP from IPv6 header)
            self.router_lifetime = ra.router_lifetime as u32;
        }
        
        // Update MTU
        if let Some(mtu) = ra.mtu {
            self.mtu = mtu;
        }
        
        // Update DNS servers
        self.dns_servers = ra.dns_servers.clone();
        
        // Process prefixes for SLAAC
        for prefix in &ra.prefixes {
            if prefix.autonomous {
                // Generate SLAAC address
                let addr = Self::generate_address(&prefix.prefix, prefix.prefix_len, mac);
                
                // Check if we already have this address
                let existing = self.global_addresses.iter_mut()
                    .find(|a| a.address == addr);
                
                if let Some(existing) = existing {
                    // Update lifetimes
                    existing.valid_lifetime = prefix.valid_lifetime;
                    existing.preferred_lifetime = prefix.preferred_lifetime;
                } else {
                    // Add new address
                    self.global_addresses.push(SlaacAddress {
                        address: addr,
                        prefix_len: prefix.prefix_len,
                        valid_lifetime: prefix.valid_lifetime,
                        preferred_lifetime: prefix.preferred_lifetime,
                        created_at: current_time,
                    });
                }
            }
        }
    }
    
    /// Check if address is preferred
    pub fn is_preferred(&self, addr: &Ipv6Addr, current_time: u64) -> bool {
        for slaac_addr in &self.global_addresses {
            if &slaac_addr.address == addr {
                let elapsed = current_time - slaac_addr.created_at;
                return elapsed < slaac_addr.preferred_lifetime as u64;
            }
        }
        false
    }
    
    /// Check if address is valid
    pub fn is_valid(&self, addr: &Ipv6Addr, current_time: u64) -> bool {
        for slaac_addr in &self.global_addresses {
            if &slaac_addr.address == addr {
                let elapsed = current_time - slaac_addr.created_at;
                return elapsed < slaac_addr.valid_lifetime as u64;
            }
        }
        false
    }
    
    /// Expire old addresses
    pub fn expire_addresses(&mut self, current_time: u64) {
        self.global_addresses.retain(|addr| {
            let elapsed = current_time - addr.created_at;
            elapsed < addr.valid_lifetime as u64
        });
    }
}

// ============================================================================
// DHCPv6 (Dynamic Host Configuration Protocol for IPv6)
// ============================================================================

/// DHCPv6 message types
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dhcpv6MessageType {
    Solicit = 1,
    Advertise = 2,
    Request = 3,
    Confirm = 4,
    Renew = 5,
    Rebind = 6,
    Reply = 7,
    Release = 8,
    Decline = 9,
    Reconfigure = 10,
    InformationRequest = 11,
    RelayForw = 12,
    RelayRepl = 13,
}

/// DHCPv6 option codes
pub const DHCPV6_OPT_CLIENTID: u16 = 1;
pub const DHCPV6_OPT_SERVERID: u16 = 2;
pub const DHCPV6_OPT_IA_NA: u16 = 3;
pub const DHCPV6_OPT_IA_TA: u16 = 4;
pub const DHCPV6_OPT_IAADDR: u16 = 5;
pub const DHCPV6_OPT_ORO: u16 = 6;
pub const DHCPV6_OPT_PREFERENCE: u16 = 7;
pub const DHCPV6_OPT_ELAPSED_TIME: u16 = 8;
pub const DHCPV6_OPT_RELAY_MSG: u16 = 9;
pub const DHCPV6_OPT_STATUS_CODE: u16 = 13;
pub const DHCPV6_OPT_RAPID_COMMIT: u16 = 14;
pub const DHCPV6_OPT_USER_CLASS: u16 = 15;
pub const DHCPV6_OPT_VENDOR_CLASS: u16 = 16;
pub const DHCPV6_OPT_DNS_SERVERS: u16 = 23;
pub const DHCPV6_OPT_DOMAIN_LIST: u16 = 24;
pub const DHCPV6_OPT_IA_PD: u16 = 25;
pub const DHCPV6_OPT_IA_PREFIX: u16 = 26;

/// DHCPv6 client state
#[derive(Clone, Debug)]
pub struct Dhcpv6Client {
    /// Client DUID (DHCP Unique Identifier)
    pub duid: [u8; 14],
    /// Transaction ID
    pub transaction_id: u32,
    /// Server DUID
    pub server_duid: Option<alloc::vec::Vec<u8>>,
    /// Assigned addresses
    pub addresses: alloc::vec::Vec<Dhcpv6Address>,
    /// DNS servers
    pub dns_servers: alloc::vec::Vec<Ipv6Addr>,
    /// Domain search list
    pub domains: alloc::vec::Vec<String>,
    /// State
    pub state: Dhcpv6State,
    /// Renew timer (T1)
    pub t1: u32,
    /// Rebind timer (T2)
    pub t2: u32,
    /// Preferred lifetime
    pub preferred_lifetime: u32,
    /// Valid lifetime
    pub valid_lifetime: u32,
}

/// DHCPv6 address
#[derive(Clone, Debug)]
pub struct Dhcpv6Address {
    pub address: Ipv6Addr,
    pub prefix_len: u8,
    pub preferred_lifetime: u32,
    pub valid_lifetime: u32,
}

/// DHCPv6 state
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dhcpv6State {
    Init,
    Selecting,
    Requesting,
    Bound,
    Renewing,
    Rebinding,
    Released,
}

impl Dhcpv6Client {
    pub fn new(mac: super::MacAddr) -> Self {
        // Generate DUID-LL (Link-Layer) based on MAC
        let mut duid = [0u8; 14];
        duid[0] = 0; // DUID type: Link-Layer
        duid[1] = 1;
        duid[2] = 0; // Hardware type: Ethernet
        duid[3] = 1;
        duid[4..10].copy_from_slice(mac.as_bytes());
        duid[10..14].copy_from_slice(&[0, 0, 0, 1]); // Time
        
        Dhcpv6Client {
            duid,
            transaction_id: 0,
            server_duid: None,
            addresses: alloc::vec::Vec::new(),
            dns_servers: alloc::vec::Vec::new(),
            domains: alloc::vec::Vec::new(),
            state: Dhcpv6State::Init,
            t1: 0,
            t2: 0,
            preferred_lifetime: 0,
            valid_lifetime: 0,
        }
    }
    
    /// Generate new transaction ID
    pub fn new_transaction_id(&mut self) -> u32 {
        // Use random or timestamp
        self.transaction_id = crate::interrupts::get_ticks() as u32 & 0xFFFFFF;
        self.transaction_id
    }
    
    /// Build Solicit message
    pub fn build_solicit(&mut self) -> alloc::vec::Vec<u8> {
        let mut buf = alloc::vec::Vec::new();
        
        // Message type
        buf.push(Dhcpv6MessageType::Solicit as u8);
        
        // Transaction ID (24 bits)
        let tid = self.new_transaction_id();
        buf.push(((tid >> 16) & 0xFF) as u8);
        buf.push(((tid >> 8) & 0xFF) as u8);
        buf.push((tid & 0xFF) as u8);
        
        // Client ID option
        buf.extend_from_slice(&DHCPV6_OPT_CLIENTID.to_be_bytes());
        buf.extend_from_slice(&(self.duid.len() as u16).to_be_bytes());
        buf.extend_from_slice(&self.duid);
        
        // Rapid Commit option (empty)
        buf.extend_from_slice(&DHCPV6_OPT_RAPID_COMMIT.to_be_bytes());
        buf.extend_from_slice(&0u16.to_be_bytes());
        
        // Option Request Option (ORO) - request DNS servers
        buf.extend_from_slice(&DHCPV6_OPT_ORO.to_be_bytes());
        buf.extend_from_slice(&4u16.to_be_bytes());
        buf.extend_from_slice(&DHCPV6_OPT_DNS_SERVERS.to_be_bytes());
        buf.extend_from_slice(&DHCPV6_OPT_DOMAIN_LIST.to_be_bytes());
        
        // Elapsed time option
        buf.extend_from_slice(&DHCPV6_OPT_ELAPSED_TIME.to_be_bytes());
        buf.extend_from_slice(&2u16.to_be_bytes());
        buf.extend_from_slice(&0u16.to_be_bytes()); // 0 ms elapsed
        
        buf
    }
    
    /// Build Request message
    pub fn build_request(&mut self) -> alloc::vec::Vec<u8> {
        let mut buf = alloc::vec::Vec::new();
        
        // Message type
        buf.push(Dhcpv6MessageType::Request as u8);
        
        // Transaction ID
        let tid = self.new_transaction_id();
        buf.push(((tid >> 16) & 0xFF) as u8);
        buf.push(((tid >> 8) & 0xFF) as u8);
        buf.push((tid & 0xFF) as u8);
        
        // Client ID option
        buf.extend_from_slice(&DHCPV6_OPT_CLIENTID.to_be_bytes());
        buf.extend_from_slice(&(self.duid.len() as u16).to_be_bytes());
        buf.extend_from_slice(&self.duid);
        
        // Server ID option
        if let Some(server_duid) = &self.server_duid {
            buf.extend_from_slice(&DHCPV6_OPT_SERVERID.to_be_bytes());
            buf.extend_from_slice(&(server_duid.len() as u16).to_be_bytes());
            buf.extend_from_slice(server_duid);
        }
        
        // ORO
        buf.extend_from_slice(&DHCPV6_OPT_ORO.to_be_bytes());
        buf.extend_from_slice(&4u16.to_be_bytes());
        buf.extend_from_slice(&DHCPV6_OPT_DNS_SERVERS.to_be_bytes());
        buf.extend_from_slice(&DHCPV6_OPT_DOMAIN_LIST.to_be_bytes());
        
        // Elapsed time
        buf.extend_from_slice(&DHCPV6_OPT_ELAPSED_TIME.to_be_bytes());
        buf.extend_from_slice(&2u16.to_be_bytes());
        buf.extend_from_slice(&0u16.to_be_bytes());
        
        buf
    }
    
    /// Build Renew message
    pub fn build_renew(&mut self) -> alloc::vec::Vec<u8> {
        let mut buf = alloc::vec::Vec::new();
        
        buf.push(Dhcpv6MessageType::Renew as u8);
        
        let tid = self.new_transaction_id();
        buf.push(((tid >> 16) & 0xFF) as u8);
        buf.push(((tid >> 8) & 0xFF) as u8);
        buf.push((tid & 0xFF) as u8);
        
        // Client ID
        buf.extend_from_slice(&DHCPV6_OPT_CLIENTID.to_be_bytes());
        buf.extend_from_slice(&(self.duid.len() as u16).to_be_bytes());
        buf.extend_from_slice(&self.duid);
        
        // Server ID
        if let Some(server_duid) = &self.server_duid {
            buf.extend_from_slice(&DHCPV6_OPT_SERVERID.to_be_bytes());
            buf.extend_from_slice(&(server_duid.len() as u16).to_be_bytes());
            buf.extend_from_slice(server_duid);
        }
        
        buf
    }
    
    /// Parse Reply message
    pub fn parse_reply(&mut self, data: &[u8]) -> bool {
        if data.is_empty() {
            return false;
        }
        
        // Check message type
        if data[0] != Dhcpv6MessageType::Reply as u8 {
            return false;
        }
        
        // Transaction ID
        if data.len() < 4 {
            return false;
        }
        
        let tid = ((data[1] as u32) << 16) | ((data[2] as u32) << 8) | (data[3] as u32);
        if tid != self.transaction_id {
            return false;
        }
        
        // Parse options
        let mut offset = 4;
        while offset + 4 <= data.len() {
            let opt_code = u16::from_be_bytes([data[offset], data[offset + 1]]);
            let opt_len = u16::from_be_bytes([data[offset + 2], data[offset + 3]]) as usize;
            
            if offset + 4 + opt_len > data.len() {
                break;
            }
            
            let opt_data = &data[offset + 4..offset + 4 + opt_len];
            
            match opt_code {
                DHCPV6_OPT_SERVERID => {
                    self.server_duid = Some(opt_data.to_vec());
                }
                DHCPV6_OPT_IA_NA => {
                    // IA_NA (Identity Association for Non-temporary Addresses)
                    if opt_len >= 12 {
                        let t1 = u32::from_be_bytes([opt_data[4], opt_data[5], opt_data[6], opt_data[7]]);
                        let t2 = u32::from_be_bytes([opt_data[8], opt_data[9], opt_data[10], opt_data[11]]);
                        self.t1 = t1;
                        self.t2 = t2;
                        
                        // Parse IAADDR options inside
                        let mut ia_offset = 12;
                        while ia_offset + 4 <= opt_data.len() {
                            let ia_opt_code = u16::from_be_bytes([opt_data[ia_offset], opt_data[ia_offset + 1]]);
                            let ia_opt_len = u16::from_be_bytes([opt_data[ia_offset + 2], opt_data[ia_offset + 3]]) as usize;
                            
                            if ia_opt_code == DHCPV6_OPT_IAADDR && ia_opt_len >= 24 {
                                let mut addr = [0u8; 16];
                                addr.copy_from_slice(&opt_data[ia_offset + 4..ia_offset + 20]);
                                let preferred = u32::from_be_bytes([
                                    opt_data[ia_offset + 20], opt_data[ia_offset + 21],
                                    opt_data[ia_offset + 22], opt_data[ia_offset + 23]
                                ]);
                                let valid = u32::from_be_bytes([
                                    opt_data[ia_offset + 24], opt_data[ia_offset + 25],
                                    opt_data[ia_offset + 26], opt_data[ia_offset + 27]
                                ]);
                                
                                self.addresses.push(Dhcpv6Address {
                                    address: Ipv6Addr(addr),
                                    prefix_len: 64, // Default
                                    preferred_lifetime: preferred,
                                    valid_lifetime: valid,
                                });
                            }
                            
                            ia_offset += 4 + ia_opt_len;
                        }
                    }
                }
                DHCPV6_OPT_DNS_SERVERS => {
                    // DNS servers (16 bytes each)
                    for i in (0..opt_len).step_by(16) {
                        if i + 16 <= opt_len {
                            let mut addr = [0u8; 16];
                            addr.copy_from_slice(&opt_data[i..i + 16]);
                            self.dns_servers.push(Ipv6Addr(addr));
                        }
                    }
                }
                DHCPV6_OPT_DOMAIN_LIST => {
                    // Domain search list
                    // Simplified: just store as bytes
                }
                _ => {}
            }
            
            offset += 4 + opt_len;
        }
        
        self.state = Dhcpv6State::Bound;
        true
    }
}

// ============================================================================
// IPv6 NEIGHBOR DISCOVERY
// ============================================================================

/// Neighbor cache entry
#[derive(Clone, Debug)]
pub struct NeighborEntry {
    pub ip: Ipv6Addr,
    pub mac: super::MacAddr,
    pub is_router: bool,
    pub state: NeighborState,
    pub created_at: u64,
    pub last_used: u64,
}

/// Neighbor state
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NeighborState {
    Incomplete,
    Reachable,
    Stale,
    Delay,
    Probe,
}

/// Neighbor Solicitation
#[derive(Clone, Debug)]
pub struct NeighborSolicitation {
    pub header: Icmpv6Header,
    pub target_addr: Ipv6Addr,
    pub source_link_addr: Option<[u8; 6]>,
}

impl NeighborSolicitation {
    pub fn new(target: Ipv6Addr, source_mac: Option<super::MacAddr>) -> Self {
        NeighborSolicitation {
            header: Icmpv6Header {
                msg_type: Icmpv6Type::NeighborSolicitation,
                code: 0,
                checksum: 0,
            },
            target_addr: target,
            source_link_addr: source_mac.map(|m| *m.as_bytes()),
        }
    }
    
    pub fn serialize(&self) -> alloc::vec::Vec<u8> {
        let mut buf = alloc::vec::Vec::new();
        
        buf.push(self.header.msg_type as u8);
        buf.push(self.header.code);
        buf.extend_from_slice(&self.header.checksum.to_be_bytes());
        
        // Reserved
        buf.extend_from_slice(&[0u8; 4]);
        
        // Target address
        buf.extend_from_slice(&self.target_addr.0);
        
        // Source link-layer address option
        if let Some(mac) = &self.source_link_addr {
            buf.push(1); // Option type
            buf.push(1); // Option length
            buf.extend_from_slice(mac);
            buf.extend_from_slice(&[0u8; 2]);
        }
        
        buf
    }
}

/// Neighbor Advertisement
#[derive(Clone, Debug)]
pub struct NeighborAdvertisement {
    pub header: Icmpv6Header,
    pub target_addr: Ipv6Addr,
    pub target_link_addr: [u8; 6],
    pub router: bool,
    pub solicited: bool,
    pub override_flag: bool,
}

impl NeighborAdvertisement {
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 24 {
            return None;
        }
        
        let msg_type = Icmpv6Type::from_u8(data[0])?;
        if msg_type != Icmpv6Type::NeighborAdvertisement {
            return None;
        }
        
        let flags = data[4];
        
        let mut target_addr = [0u8; 16];
        target_addr.copy_from_slice(&data[8..24]);
        
        let mut target_link_addr = None;
        
        // Parse options
        let mut offset = 24;
        while offset + 2 <= data.len() {
            let opt_type = data[offset];
            let opt_len = data[offset + 1] as usize * 8;
            
            if opt_type == 2 && opt_len >= 8 && offset + 8 <= data.len() {
                target_link_addr = Some([
                    data[offset + 2], data[offset + 3],
                    data[offset + 4], data[offset + 5],
                    data[offset + 6], data[offset + 7],
                ]);
            }
            
            offset += opt_len;
        }
        
        Some(NeighborAdvertisement {
            header: Icmpv6Header {
                msg_type,
                code: data[1],
                checksum: u16::from_be_bytes([data[2], data[3]]),
            },
            target_addr: Ipv6Addr(target_addr),
            target_link_addr: target_link_addr?,
            router: (flags & 0x80) != 0,
            solicited: (flags & 0x40) != 0,
            override_flag: (flags & 0x20) != 0,
        })
    }
}

/// Initialize IPv6
pub fn init() {
    crate::serial_println!("[IPv6] Module initialized");
}
