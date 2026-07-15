//! IPv6 transition mechanisms: 6to4, Teredo, and ISATAP.

use super::ethernet::EtherType;
use super::ip::{IpProtocol, Ipv4Packet};
use super::ipv6::{Ipv6Addr, Ipv6Header, Ipv6Packet, Ipv6NextHeader};
use super::{IpAddr, Ipv4Addr, NetError, Port};
use alloc::vec;
use alloc::vec::Vec;

pub const TEREDO_PORT: u16 = 3544;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SixToFourAddr {
    pub relay_ipv4: Ipv4Addr,
    pub prefix: [u16; 3],
}

impl SixToFourAddr {
    pub fn from_ipv4(ip: Ipv4Addr) -> Self {
        let octets = ip.0;
        let p1 = 0x2002;
        let p2 = ((octets[0] as u16) << 8) | octets[1] as u16;
        let p3 = ((octets[2] as u16) << 8) | octets[3] as u16;
        Self {
            relay_ipv4: ip,
            prefix: [p1, p2, p3],
        }
    }

    pub fn to_ipv6(&self, subnet: u16, iface_id: [u16; 4]) -> Ipv6Addr {
        Ipv6Addr::from_segments([
            self.prefix[0],
            self.prefix[1],
            self.prefix[2],
            subnet,
            iface_id[0],
            iface_id[1],
            iface_id[2],
            iface_id[3],
        ])
    }

    pub fn extract_ipv4(addr: Ipv6Addr) -> Option<Ipv4Addr> {
        let seg = addr.segments();
        if seg[0] != 0x2002 {
            return None;
        }
        Some(Ipv4Addr([
            (seg[1] >> 8) as u8,
            seg[1] as u8,
            (seg[2] >> 8) as u8,
            seg[2] as u8,
        ]))
    }
}

pub fn encapsulate_6to4(
    outer_src: Ipv4Addr,
    outer_dst: Ipv4Addr,
    inner: &Ipv6Packet,
) -> Vec<u8> {
    let payload = inner.serialize();
    let packet = Ipv4Packet::new(outer_src, outer_dst, IpProtocol::UNKNOWN, &payload);
    let mut buf = vec![0u8; payload.len() + 20];
    let len = packet.serialize(&mut buf).unwrap_or(0);
    if len >= 20 {
        buf[9] = 41;
        buf[10] = 0;
        buf[11] = 0;
        let checksum = ipv4_checksum(&buf[..20]);
        buf[10..12].copy_from_slice(&checksum.to_be_bytes());
    }
    buf.truncate(len);
    buf
}

pub fn decapsulate_6to4(packet: &[u8]) -> Result<Ipv6Packet, NetError> {
    let ipv4 = Ipv4Packet::parse(packet)?;
    if packet.get(9).copied() != Some(41) {
        return Err(NetError::ProtocolError);
    }
    Ipv6Packet::parse(ipv4.payload)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TeredoAddr {
    pub server_ipv4: Ipv4Addr,
    pub client_ipv4: Ipv4Addr,
    pub flags: u16,
    pub udp_port: u16,
}

impl TeredoAddr {
    pub fn encode(&self) -> Ipv6Addr {
        let server = self.server_ipv4.0;
        let client = self.client_ipv4.0;
        let obf_port = !self.udp_port;
        let client_hi = (((!client[0]) as u16) << 8) | (!client[1]) as u16;
        let client_lo = (((!client[2]) as u16) << 8) | (!client[3]) as u16;
        Ipv6Addr::from_segments([
            0x2001,
            0x0000,
            ((server[0] as u16) << 8) | server[1] as u16,
            ((server[2] as u16) << 8) | server[3] as u16,
            self.flags,
            obf_port,
            client_hi,
            client_lo,
        ])
    }

    pub fn decode(addr: Ipv6Addr) -> Option<Self> {
        let seg = addr.segments();
        if seg[0] != 0x2001 {
            return None;
        }
        let server_ipv4 = Ipv4Addr([
            (seg[2] >> 8) as u8,
            seg[2] as u8,
            (seg[3] >> 8) as u8,
            seg[3] as u8,
        ]);
        let udp_port = !seg[5];
        let client_ipv4 = Ipv4Addr([
            !(seg[6] >> 8) as u8,
            !(seg[6] as u8),
            !(seg[7] >> 8) as u8,
            !(seg[7] as u8),
        ]);
        Some(Self {
            server_ipv4,
            client_ipv4,
            flags: seg[4],
            udp_port,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TeredoHeader {
    pub nonce: u64,
    pub origin_port: u16,
    pub origin_addr: Ipv4Addr,
}

pub fn encapsulate_teredo(
    outer_src: Ipv4Addr,
    outer_dst: Ipv4Addr,
    hdr: TeredoHeader,
    inner: &Ipv6Packet,
) -> Vec<u8> {
    let inner_payload = inner.serialize();
    let mut udp_payload = Vec::with_capacity(14 + inner_payload.len());
    udp_payload.extend_from_slice(&hdr.nonce.to_be_bytes());
    udp_payload.extend_from_slice(&hdr.origin_port.to_be_bytes());
    udp_payload.extend_from_slice(&hdr.origin_addr.0);
    udp_payload.extend_from_slice(&inner_payload);
    let mut payload = Vec::with_capacity(8 + udp_payload.len());
    payload.extend_from_slice(&TEREDO_PORT.to_be_bytes());
    payload.extend_from_slice(&TEREDO_PORT.to_be_bytes());
    payload.extend_from_slice(&((8 + udp_payload.len()) as u16).to_be_bytes());
    payload.extend_from_slice(&0u16.to_be_bytes());
    payload.extend_from_slice(&udp_payload);
    let checksum = udp_checksum_ipv4(outer_src, outer_dst, &payload);
    payload[6..8].copy_from_slice(&checksum.to_be_bytes());
    let packet = Ipv4Packet::new(outer_src, outer_dst, IpProtocol::UDP, &payload);
    let mut buf = vec![0u8; payload.len() + 20];
    let len = packet.serialize(&mut buf).unwrap_or(0);
    buf.truncate(len);
    buf
}

pub fn decapsulate_teredo(packet: &[u8]) -> Result<(TeredoHeader, Ipv6Packet), NetError> {
    let ipv4 = Ipv4Packet::parse(packet)?;
    if ipv4.header.protocol != IpProtocol::UDP || ipv4.payload.len() < 22 {
        return Err(NetError::ProtocolError);
    }
    let src_port = u16::from_be_bytes([ipv4.payload[0], ipv4.payload[1]]);
    let dst_port = u16::from_be_bytes([ipv4.payload[2], ipv4.payload[3]]);
    let udp_len = u16::from_be_bytes([ipv4.payload[4], ipv4.payload[5]]) as usize;
    if src_port != TEREDO_PORT || dst_port != TEREDO_PORT || udp_len < 22 || udp_len > ipv4.payload.len() {
        return Err(NetError::ProtocolError);
    }
    let payload = &ipv4.payload[8..udp_len];
    let nonce = u64::from_be_bytes(payload[0..8].try_into().unwrap());
    let origin_port = u16::from_be_bytes([payload[8], payload[9]]);
    let origin_addr = Ipv4Addr([
        payload[10],
        payload[11],
        payload[12],
        payload[13],
    ]);
    let inner = Ipv6Packet::parse(&payload[14..])?;
    Ok((
        TeredoHeader {
            nonce,
            origin_port,
            origin_addr,
        },
        inner,
    ))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IsatapAddr {
    pub prefix: [u16; 4],
    pub ipv4: Ipv4Addr,
}

impl IsatapAddr {
    pub fn new(prefix: [u16; 4], ipv4: Ipv4Addr) -> Self {
        Self { prefix, ipv4 }
    }

    pub fn to_ipv6(&self) -> Ipv6Addr {
        let ip = self.ipv4.0;
        Ipv6Addr::from_segments([
            self.prefix[0],
            self.prefix[1],
            self.prefix[2],
            self.prefix[3],
            0,
            0x5efe,
            ((ip[0] as u16) << 8) | ip[1] as u16,
            ((ip[2] as u16) << 8) | ip[3] as u16,
        ])
    }

    pub fn extract_ipv4(addr: Ipv6Addr) -> Option<Ipv4Addr> {
        let seg = addr.segments();
        if seg[5] != 0x5efe {
            return None;
        }
        Some(Ipv4Addr([
            (seg[6] >> 8) as u8,
            seg[6] as u8,
            (seg[7] >> 8) as u8,
            seg[7] as u8,
        ]))
    }
}

pub fn transition_route_for(ip: IpAddr) -> Option<EtherType> {
    match ip {
        IpAddr::V4(_) => Some(EtherType::IPV4),
        IpAddr::V6(_) => Some(EtherType::IPV6),
    }
}

fn ipv4_checksum(header: &[u8]) -> u16 {
    let mut sum = 0u32;
    let mut i = 0usize;
    while i + 1 < header.len() {
        sum += u16::from_be_bytes([header[i], header[i + 1]]) as u32;
        i += 2;
    }
    while (sum >> 16) != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

fn udp_checksum_ipv4(src: Ipv4Addr, dst: Ipv4Addr, udp_segment: &[u8]) -> u16 {
    let mut sum = 0u32;
    for chunk in src.0.chunks(2) {
        sum += u16::from_be_bytes([chunk[0], chunk[1]]) as u32;
    }
    for chunk in dst.0.chunks(2) {
        sum += u16::from_be_bytes([chunk[0], chunk[1]]) as u32;
    }
    sum += IpProtocol::UDP as u32;
    sum += udp_segment.len() as u32;
    let mut i = 0usize;
    while i + 1 < udp_segment.len() {
        sum += u16::from_be_bytes([udp_segment[i], udp_segment[i + 1]]) as u32;
        i += 2;
    }
    if i < udp_segment.len() {
        sum += (udp_segment[i] as u32) << 8;
    }
    while (sum >> 16) != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    let checksum = !(sum as u16);
    if checksum == 0 { 0xFFFF } else { checksum }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn six_to_four_roundtrip() {
        let relay = Ipv4Addr([192, 0, 2, 10]);
        let addr = SixToFourAddr::from_ipv4(relay).to_ipv6(0x1234, [0, 0, 0, 1]);
        assert_eq!(SixToFourAddr::extract_ipv4(addr), Some(relay));
    }

    #[test]
    fn teredo_address_roundtrip() {
        let teredo = TeredoAddr {
            server_ipv4: Ipv4Addr([65, 54, 227, 120]),
            client_ipv4: Ipv4Addr([203, 0, 113, 9]),
            flags: 0x8000,
            udp_port: 40000,
        };
        assert_eq!(TeredoAddr::decode(teredo.encode()), Some(teredo));
    }

    #[test]
    fn isatap_roundtrip() {
        let addr = IsatapAddr::new([0x2001, 0xdb8, 0, 1], Ipv4Addr([10, 1, 2, 3])).to_ipv6();
        assert_eq!(IsatapAddr::extract_ipv4(addr), Some(Ipv4Addr([10, 1, 2, 3])));
    }
}
