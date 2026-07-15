use super::{checksum, Ipv4Addr, Mutex, NetError};
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

const GRE_PROTO_IPV4: u16 = 0x0800;
const GRE_PROTO_IPV6: u16 = 0x86DD;
const GRE_PROTO_ETHERNET: u16 = 0x6558;
const GRE_PROTO_MPLS: u16 = 0x8847;

const GRE_BASE_HEADER_LEN: usize = 4;
const GRE_CHECKSUM_HEADER_LEN: usize = 8;
const GRE_KEY_HEADER_LEN: usize = 8;
const GRE_SEQ_HEADER_LEN: usize = 8;
const GRE_KEY_SEQ_HEADER_LEN: usize = 12;
const GRE_CHECKSUM_KEY_HEADER_LEN: usize = 12;
const GRE_CHECKSUM_SEQ_HEADER_LEN: usize = 12;
const GRE_FULL_HEADER_LEN: usize = 16;

const GRE_FLAG_C: u16 = 0x8000;
const GRE_FLAG_K: u16 = 0x2000;
const GRE_FLAG_S: u16 = 0x1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GreHeader {
    pub flags: u16,
    pub protocol: u16,
    pub checksum: Option<u16>,
    pub key: Option<u32>,
    pub seq: Option<u32>,
}

impl GreHeader {
    pub fn new(protocol: u16) -> Self {
        GreHeader {
            flags: 0,
            protocol,
            checksum: None,
            key: None,
            seq: None,
        }
    }

    pub fn with_key(protocol: u16, key: u32) -> Self {
        GreHeader {
            flags: GRE_FLAG_K,
            protocol,
            checksum: None,
            key: Some(key),
            seq: None,
        }
    }

    pub fn with_checksum(protocol: u16) -> Self {
        GreHeader {
            flags: GRE_FLAG_C,
            protocol,
            checksum: Some(0),
            key: None,
            seq: None,
        }
    }

    pub fn with_key_seq(protocol: u16, key: u32, seq: u32) -> Self {
        GreHeader {
            flags: GRE_FLAG_K | GRE_FLAG_S,
            protocol,
            checksum: None,
            key: Some(key),
            seq: Some(seq),
        }
    }

    pub fn header_len(&self) -> usize {
        let mut len = GRE_BASE_HEADER_LEN;
        if self.checksum.is_some() {
            len += 4;
        } else if self.key.is_some() || self.seq.is_some() {
            len += 4;
        }
        if self.key.is_some() {
            len += 4;
        }
        if self.seq.is_some() {
            len += 4;
        }
        if self.flags & (GRE_FLAG_C | GRE_FLAG_K | GRE_FLAG_S) != 0
            && len == GRE_BASE_HEADER_LEN
        {
            len = GRE_BASE_HEADER_LEN + 4;
        }
        len
    }

    pub fn parse(data: &[u8]) -> Result<Self, NetError> {
        if data.len() < GRE_BASE_HEADER_LEN {
            return Err(NetError::InvalidPacket);
        }
        let flags = u16::from_be_bytes([data[0], data[1]]);
        let protocol = u16::from_be_bytes([data[2], data[3]]);

        let c_set = (flags & GRE_FLAG_C) != 0;
        let k_set = (flags & GRE_FLAG_K) != 0;
        let s_set = (flags & GRE_FLAG_S) != 0;
        let reserved_bits = flags & 0x0FFF;
        if reserved_bits != 0 {
            return Err(NetError::InvalidPacket);
        }

        let mut offset = GRE_BASE_HEADER_LEN;
        let mut checksum = None;
        let mut reserved_field = None;

        if c_set {
            if data.len() < offset + 4 {
                return Err(NetError::InvalidPacket);
            }
            let csum = u16::from_be_bytes([data[offset], data[offset + 1]]);
            let rsvd = u16::from_be_bytes([data[offset + 2], data[offset + 3]]);
            if rsvd != 0 {
                return Err(NetError::InvalidPacket);
            }
            checksum = Some(csum);
            reserved_field = Some(rsvd);
            offset += 4;
        } else if k_set || s_set {
            if data.len() < offset + 4 {
                return Err(NetError::InvalidPacket);
            }
            let rsvd = u16::from_be_bytes([data[offset], data[offset + 1]]);
            let rsvd2 = u16::from_be_bytes([data[offset + 2], data[offset + 3]]);
            if rsvd != 0 || rsvd2 != 0 {
                return Err(NetError::InvalidPacket);
            }
            offset += 4;
        }

        let mut key = None;
        if k_set {
            if data.len() < offset + 4 {
                return Err(NetError::InvalidPacket);
            }
            key = Some(u32::from_be_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ]));
            offset += 4;
        }

        let mut seq = None;
        if s_set {
            if data.len() < offset + 4 {
                return Err(NetError::InvalidPacket);
            }
            seq = Some(u32::from_be_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ]));
            offset += 4;
        }

        let mut hdr = GreHeader {
            flags: flags & (GRE_FLAG_C | GRE_FLAG_K | GRE_FLAG_S),
            protocol,
            checksum,
            key,
            seq,
        };

        if let Some(_csum) = hdr.checksum {
            let computed = checksum::internet_checksum(&data[..offset]);
            if computed != 0 {
                return Err(NetError::ChecksumError);
            }
        }

        Ok(hdr)
    }

    pub fn serialize(&self, buf: &mut [u8]) -> Result<usize, NetError> {
        let hdr_len = self.header_len();
        if buf.len() < hdr_len {
            return Err(NetError::BufferFull);
        }

        let mut flags = self.flags;
        if self.checksum.is_some() {
            flags |= GRE_FLAG_C;
        }
        if self.key.is_some() {
            flags |= GRE_FLAG_K;
        }
        if self.seq.is_some() {
            flags |= GRE_FLAG_S;
        }

        buf[0..2].copy_from_slice(&flags.to_be_bytes());
        buf[2..4].copy_from_slice(&self.protocol.to_be_bytes());
        let mut offset = GRE_BASE_HEADER_LEN;

        if self.checksum.is_some() {
            buf[offset] = 0;
            buf[offset + 1] = 0;
            buf[offset + 2..offset + 4].copy_from_slice(&0u16.to_be_bytes());
            offset += 4;
        } else if self.key.is_some() || self.seq.is_some() {
            buf[offset..offset + 4].copy_from_slice(&[0, 0, 0, 0]);
            offset += 4;
        }

        if let Some(key) = self.key {
            buf[offset..offset + 4].copy_from_slice(&key.to_be_bytes());
            offset += 4;
        }

        if let Some(seq) = self.seq {
            buf[offset..offset + 4].copy_from_slice(&seq.to_be_bytes());
            offset += 4;
        }

        if let Some(_csum) = self.checksum {
            let computed = checksum::internet_checksum(&buf[..offset]);
            buf[GRE_BASE_HEADER_LEN] = (computed >> 8) as u8;
            buf[GRE_BASE_HEADER_LEN + 1] = (computed & 0xFF) as u8;
        }

        Ok(offset)
    }
}

pub fn gre_encapsulate(
    inner_packet: &[u8],
    tunnel_key: u32,
    proto: u16,
) -> Vec<u8> {
    let hdr = GreHeader::with_key(proto, tunnel_key);
    let hdr_len = hdr.header_len();
    let mut buf = vec![0u8; hdr_len + inner_packet.len()];
    let _ = hdr.serialize(&mut buf);
    buf[hdr_len..].copy_from_slice(inner_packet);
    buf
}

pub fn gre_decapsulate(
    gre_packet: &[u8],
) -> Result<(&[u8], u16, Option<u32>), NetError> {
    let hdr = GreHeader::parse(gre_packet)?;
    let hdr_len = hdr.header_len();
    if gre_packet.len() < hdr_len {
        return Err(NetError::InvalidPacket);
    }
    let payload = &gre_packet[hdr_len..];
    Ok((payload, hdr.protocol, hdr.key))
}

#[derive(Clone, Debug)]
pub struct GreTunnel {
    pub name: String,
    pub src_ip: Ipv4Addr,
    pub dst_ip: Ipv4Addr,
    pub tunnel_key: u32,
    pub key_present: bool,
    pub csum_enabled: bool,
    pub seq_enabled: bool,
    pub ttl: u8,
    pub up: bool,
    pub seq_counter: u32,
    pub keepalive_interval: Option<u32>,
    pub keepalive_timeout: Option<u32>,
    pub last_rx_tick: u64,
    pub last_tx_tick: u64,
    pub rx_packets: u64,
    pub tx_packets: u64,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_errors: u64,
    pub tx_errors: u64,
}

impl GreTunnel {
    pub fn new(
        name: &str,
        src_ip: Ipv4Addr,
        dst_ip: Ipv4Addr,
        tunnel_key: u32,
    ) -> Self {
        GreTunnel {
            name: String::from(name),
            src_ip,
            dst_ip,
            tunnel_key,
            key_present: true,
            csum_enabled: false,
            seq_enabled: false,
            ttl: 64,
            up: false,
            seq_counter: 0,
            keepalive_interval: None,
            keepalive_timeout: None,
            last_rx_tick: 0,
            last_tx_tick: 0,
            rx_packets: 0,
            tx_packets: 0,
            rx_bytes: 0,
            tx_bytes: 0,
            rx_errors: 0,
            tx_errors: 0,
        }
    }

    pub fn with_checksum(mut self, enabled: bool) -> Self {
        self.csum_enabled = enabled;
        self
    }

    pub fn with_seq(mut self, enabled: bool) -> Self {
        self.seq_enabled = enabled;
        self
    }

    pub fn with_keepalive(mut self, interval_ticks: u32, timeout_ticks: u32) -> Self {
        self.keepalive_interval = Some(interval_ticks);
        self.keepalive_timeout = Some(timeout_ticks);
        self
    }

    fn build_header(&mut self) -> GreHeader {
        let proto = GRE_PROTO_IPV4;
        let mut flags = 0u16;
        let mut checksum = None;
        let mut key = None;
        let mut seq = None;

        if self.csum_enabled {
            checksum = Some(0);
        }
        if self.key_present {
            key = Some(self.tunnel_key);
        }
        if self.seq_enabled {
            self.seq_counter = self.seq_counter.wrapping_add(1);
            seq = Some(self.seq_counter);
        }

        if checksum.is_some() {
            flags |= GRE_FLAG_C;
        }
        if key.is_some() {
            flags |= GRE_FLAG_K;
        }
        if seq.is_some() {
            flags |= GRE_FLAG_S;
        }

        GreHeader {
            flags,
            protocol: proto,
            checksum,
            key,
            seq,
        }
    }
}

pub fn gre_tunnel_encapsulate(
    tunnel: &mut GreTunnel,
    payload: &[u8],
) -> Result<Vec<u8>, NetError> {
    if !tunnel.up {
        return Err(NetError::NotUp);
    }

    let hdr = tunnel.build_header();
    let hdr_len = hdr.header_len();
    let total_len = hdr_len + payload.len();
    let mut buf = vec![0u8; hdr_len + payload.len()];
    let _ = hdr.serialize(&mut buf);

    let ip_total_len = 20 + total_len;
    let mut ip_buf = vec![0u8; 20 + hdr_len + payload.len()];
    ip_buf[0] = 0x45;
    ip_buf[2..4].copy_from_slice(&(ip_total_len as u16).to_be_bytes());
    ip_buf[8] = tunnel.ttl;
    ip_buf[9] = 47;
    ip_buf[12..16].copy_from_slice(tunnel.src_ip.as_bytes());
    ip_buf[16..20].copy_from_slice(tunnel.dst_ip.as_bytes());
    let ip_csum = checksum::internet_checksum(&ip_buf[..20]);
    ip_buf[10..12].copy_from_slice(&ip_csum.to_be_bytes());

    let gre_offset = 20;
    ip_buf[gre_offset..gre_offset + hdr_len].copy_from_slice(&buf[..hdr_len]);
    ip_buf[gre_offset + hdr_len..].copy_from_slice(payload);

    tunnel.tx_packets += 1;
    tunnel.tx_bytes += ip_buf.len() as u64;
    Ok(ip_buf)
}

pub fn gre_tunnel_decapsulate(
    tunnel: &mut GreTunnel,
    ip_packet: &[u8],
) -> Result<Vec<u8>, NetError> {
    if !tunnel.up {
        return Err(NetError::NotUp);
    }

    if ip_packet.len() < 20 {
        return Err(NetError::InvalidPacket);
    }

    let ihl = (ip_packet[0] & 0x0F) as usize * 4;
    if ip_packet.len() < ihl {
        return Err(NetError::InvalidPacket);
    }

    let total_len = u16::from_be_bytes([ip_packet[2], ip_packet[3]]) as usize;
    if ip_packet.len() < total_len {
        return Err(NetError::InvalidPacket);
    }

    let gre_data = &ip_packet[ihl..total_len];
    let (payload, proto, key) = gre_decapsulate(gre_data)?;

    if tunnel.key_present {
        match key {
            Some(k) if k == tunnel.tunnel_key => {}
            _ => {
                tunnel.rx_errors += 1;
                return Err(NetError::ProtocolError);
            }
        }
    }

    if proto != GRE_PROTO_IPV4 {
        return Err(NetError::NotSupported);
    }

    tunnel.rx_packets += 1;
    tunnel.rx_bytes += ip_packet.len() as u64;

    Ok(payload.to_vec())
}

pub fn gre_tunnel_keepalive_check(
    tunnel: &GreTunnel,
    current_tick: u64,
) -> GreKeepaliveAction {
    let interval_u32 = match tunnel.keepalive_interval {
        Some(i) => i,
        None => return GreKeepaliveAction::None,
    };
    let interval = interval_u32 as u64;
    let timeout = tunnel.keepalive_timeout.unwrap_or(interval_u32.wrapping_mul(3)) as u64;

    if tunnel.last_rx_tick > 0 {
        let elapsed = current_tick.wrapping_sub(tunnel.last_rx_tick);
        if elapsed >= timeout {
            return GreKeepaliveAction::Timeout;
        }
    }

    let since_tx = current_tick.wrapping_sub(tunnel.last_tx_tick);
    if since_tx >= interval {
        GreKeepaliveAction::SendKeepalive
    } else {
        GreKeepaliveAction::None
    }
}

pub fn gre_build_keepalive(tunnel_key: u32) -> Vec<u8> {
    let hdr = GreHeader {
        flags: GRE_FLAG_K,
        protocol: 0,
        checksum: None,
        key: Some(tunnel_key),
        seq: None,
    };
    let hdr_len = hdr.header_len();
    let mut buf = vec![0u8; hdr_len];
    let _ = hdr.serialize(&mut buf);
    buf
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GreKeepaliveAction {
    None,
    SendKeepalive,
    Timeout,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GreProtocolType {
    IPv4,
    IPv6,
    Ethernet,
    MPLS,
}

impl GreProtocolType {
    pub fn as_ethertype(&self) -> u16 {
        match self {
            GreProtocolType::IPv4 => GRE_PROTO_IPV4,
            GreProtocolType::IPv6 => GRE_PROTO_IPV6,
            GreProtocolType::Ethernet => GRE_PROTO_ETHERNET,
            GreProtocolType::MPLS => GRE_PROTO_MPLS,
        }
    }

    pub fn from_ethertype(val: u16) -> Option<Self> {
        match val {
            GRE_PROTO_IPV4 => Some(GreProtocolType::IPv4),
            GRE_PROTO_IPV6 => Some(GreProtocolType::IPv6),
            GRE_PROTO_ETHERNET => Some(GreProtocolType::Ethernet),
            GRE_PROTO_MPLS => Some(GreProtocolType::MPLS),
            _ => None,
        }
    }
}

static GRE_TUNNELS: Mutex<alloc::collections::BTreeMap<String, GreTunnel>> =
    Mutex::new(alloc::collections::BTreeMap::new());

static GRE_STATS: GreStats = GreStats::new();

struct GreStats {
    tunnels_created: AtomicU32,
    encaps: AtomicU64,
    decaps: AtomicU64,
    errors: AtomicU64,
    keepalive_timeouts: AtomicU64,
}

impl GreStats {
    const fn new() -> Self {
        GreStats {
            tunnels_created: AtomicU32::new(0),
            encaps: AtomicU64::new(0),
            decaps: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            keepalive_timeouts: AtomicU64::new(0),
        }
    }
}

pub fn create_tunnel(
    name: &str,
    src_ip: Ipv4Addr,
    dst_ip: Ipv4Addr,
    tunnel_key: u32,
) -> Result<(), NetError> {
    let mut tunnels = GRE_TUNNELS.lock();
    if tunnels.contains_key(name) {
        return Err(NetError::AddrInUse);
    }
    let tunnel = GreTunnel::new(name, src_ip, dst_ip, tunnel_key);
    tunnels.insert(String::from(name), tunnel);
    GRE_STATS.tunnels_created.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

pub fn set_tunnel_up(name: &str, up: bool) -> Result<(), NetError> {
    let mut tunnels = GRE_TUNNELS.lock();
    let tunnel = tunnels.get_mut(name).ok_or(NetError::InvalidParam)?;
    tunnel.up = up;
    Ok(())
}

pub fn set_tunnel_checksum(name: &str, enabled: bool) -> Result<(), NetError> {
    let mut tunnels = GRE_TUNNELS.lock();
    let tunnel = tunnels.get_mut(name).ok_or(NetError::InvalidParam)?;
    tunnel.csum_enabled = enabled;
    Ok(())
}

pub fn set_tunnel_seq(name: &str, enabled: bool) -> Result<(), NetError> {
    let mut tunnels = GRE_TUNNELS.lock();
    let tunnel = tunnels.get_mut(name).ok_or(NetError::InvalidParam)?;
    tunnel.seq_enabled = enabled;
    Ok(())
}

pub fn set_tunnel_keepalive(
    name: &str,
    interval: u32,
    timeout: u32,
) -> Result<(), NetError> {
    let mut tunnels = GRE_TUNNELS.lock();
    let tunnel = tunnels.get_mut(name).ok_or(NetError::InvalidParam)?;
    tunnel.keepalive_interval = Some(interval);
    tunnel.keepalive_timeout = Some(timeout);
    Ok(())
}

pub fn tunnel_encapsulate(name: &str, payload: &[u8]) -> Result<Vec<u8>, NetError> {
    let mut tunnels = GRE_TUNNELS.lock();
    let tunnel = tunnels.get_mut(name).ok_or(NetError::InvalidParam)?;
    let result = gre_tunnel_encapsulate(tunnel, payload);
    if result.is_ok() {
        GRE_STATS.encaps.fetch_add(1, Ordering::Relaxed);
    } else {
        GRE_STATS.errors.fetch_add(1, Ordering::Relaxed);
    }
    result
}

pub fn tunnel_decapsulate(name: &str, ip_packet: &[u8]) -> Result<Vec<u8>, NetError> {
    let mut tunnels = GRE_TUNNELS.lock();
    let tunnel = tunnels.get_mut(name).ok_or(NetError::InvalidParam)?;
    let result = gre_tunnel_decapsulate(tunnel, ip_packet);
    if result.is_ok() {
        GRE_STATS.decaps.fetch_add(1, Ordering::Relaxed);
    } else {
        GRE_STATS.errors.fetch_add(1, Ordering::Relaxed);
    }
    result
}

pub fn check_keepalives(current_tick: u64) -> Vec<(String, GreKeepaliveAction)> {
    let tunnels = GRE_TUNNELS.lock();
    let mut actions = Vec::new();
    for (name, tunnel) in tunnels.iter() {
        let action = gre_tunnel_keepalive_check(tunnel, current_tick);
        if action != GreKeepaliveAction::None {
            if action == GreKeepaliveAction::Timeout {
                GRE_STATS.keepalive_timeouts.fetch_add(1, Ordering::Relaxed);
            }
            actions.push((String::from(name), action));
        }
    }
    actions
}

pub fn get_tunnel_stats(name: &str) -> Option<(u64, u64, u64, u64, u64, u64)> {
    let tunnels = GRE_TUNNELS.lock();
    let t = tunnels.get(name)?;
    Some((
        t.rx_packets,
        t.tx_packets,
        t.rx_bytes,
        t.tx_bytes,
        t.rx_errors,
        t.tx_errors,
    ))
}

pub fn get_global_stats() -> (u32, u64, u64, u64, u64) {
    let ord = Ordering::Relaxed;
    (
        GRE_STATS.tunnels_created.load(ord),
        GRE_STATS.encaps.load(ord),
        GRE_STATS.decaps.load(ord),
        GRE_STATS.errors.load(ord),
        GRE_STATS.keepalive_timeouts.load(ord),
    )
}

pub fn init() {
    crate::serial_println!("[GRE] GRE tunnel module initialized");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gre_header_base_only_roundtrip() {
        let hdr = GreHeader::new(0x0800);
        let mut buf = [0u8; 16];
        let written = hdr.serialize(&mut buf).unwrap();
        assert_eq!(written, GRE_BASE_HEADER_LEN);

        let parsed = GreHeader::parse(&buf[..written]).unwrap();
        assert_eq!(parsed.protocol, 0x0800);
        assert!(parsed.checksum.is_none());
        assert!(parsed.key.is_none());
        assert!(parsed.seq.is_none());
    }

    #[test]
    fn gre_header_with_key_roundtrip() {
        let hdr = GreHeader::with_key(0x0800, 0xDEADBEEF);
        let mut buf = [0u8; 16];
        let written = hdr.serialize(&mut buf).unwrap();
        assert_eq!(written, GRE_KEY_SEQ_HEADER_LEN);

        let parsed = GreHeader::parse(&buf[..written]).unwrap();
        assert_eq!(parsed.protocol, 0x0800);
        assert_eq!(parsed.key, Some(0xDEADBEEF));
        assert!(parsed.checksum.is_none());
        assert!(parsed.seq.is_none());
    }

    #[test]
    fn gre_header_with_checksum_roundtrip() {
        let hdr = GreHeader::with_checksum(0x0800);
        let mut buf = [0u8; 16];
        let written = hdr.serialize(&mut buf).unwrap();
        assert_eq!(written, GRE_CHECKSUM_HEADER_LEN);

        let parsed = GreHeader::parse(&buf[..written]).unwrap();
        assert_eq!(parsed.protocol, 0x0800);
        assert!(parsed.checksum.is_some());
    }

    #[test]
    fn gre_header_with_key_seq_roundtrip() {
        let hdr = GreHeader::with_key_seq(0x0800, 0x12345678, 42);
        let mut buf = [0u8; 16];
        let written = hdr.serialize(&mut buf).unwrap();
        assert_eq!(written, GRE_FULL_HEADER_LEN);

        let parsed = GreHeader::parse(&buf[..written]).unwrap();
        assert_eq!(parsed.protocol, 0x0800);
        assert_eq!(parsed.key, Some(0x12345678));
        assert_eq!(parsed.seq, Some(42));
    }

    #[test]
    fn gre_header_checksum_validation() {
        let hdr = GreHeader::with_checksum(0x0800);
        let mut buf = [0u8; 16];
        let written = hdr.serialize(&mut buf).unwrap();

        let parsed = GreHeader::parse(&buf[..written]).unwrap();
        assert!(parsed.checksum.is_some());
    }

    #[test]
    fn gre_header_checksum_corruption_detected() {
        let hdr = GreHeader::with_checksum(0x0800);
        let mut buf = [0u8; 16];
        let written = hdr.serialize(&mut buf).unwrap();

        buf[5] ^= 0xFF;

        let result = GreHeader::parse(&buf[..written]);
        assert_eq!(result, Err(NetError::ChecksumError));
    }

    #[test]
    fn gre_encapsulate_decap_roundtrip() {
        let inner = [0x45, 0x00, 0x00, 0x1C, 0x00, 0x01, 0x00, 0x00, 0x40, 0x11];
        let gre_pkt = gre_encapsulate(&inner, 0xAABBCCDD, GRE_PROTO_IPV4);
        assert!(gre_pkt.len() > inner.len());

        let (payload, proto, key) = gre_decapsulate(&gre_pkt).unwrap();
        assert_eq!(payload, inner);
        assert_eq!(proto, GRE_PROTO_IPV4);
        assert_eq!(key, Some(0xAABBCCDD));
    }

    #[test]
    fn gre_decapsulate_too_short() {
        let result = gre_decapsulate(&[0x00]);
        assert_eq!(result, Err(NetError::InvalidPacket));
    }

    #[test]
    fn gre_key_based_tunneling() {
        let inner = [0xDE, 0xAD, 0xBE, 0xEF];
        let gre_pkt = gre_encapsulate(&inner, 0x00000042, GRE_PROTO_IPV4);

        let (payload, proto, key) = gre_decapsulate(&gre_pkt).unwrap();
        assert_eq!(payload, inner);
        assert_eq!(proto, GRE_PROTO_IPV4);
        assert_eq!(key, Some(0x00000042));
    }

    #[test]
    fn gre_protocol_types() {
        assert_eq!(GreProtocolType::IPv4.as_ethertype(), 0x0800);
        assert_eq!(GreProtocolType::IPv6.as_ethertype(), 0x86DD);
        assert_eq!(GreProtocolType::Ethernet.as_ethertype(), 0x6558);
        assert_eq!(GreProtocolType::MPLS.as_ethertype(), 0x8847);

        assert_eq!(GreProtocolType::from_ethertype(0x0800), Some(GreProtocolType::IPv4));
        assert_eq!(GreProtocolType::from_ethertype(0x86DD), Some(GreProtocolType::IPv6));
        assert_eq!(GreProtocolType::from_ethertype(0x6558), Some(GreProtocolType::Ethernet));
        assert_eq!(GreProtocolType::from_ethertype(0x8847), Some(GreProtocolType::MPLS));
        assert_eq!(GreProtocolType::from_ethertype(0xFFFF), None);
    }

    #[test]
    fn gre_tunnel_encap_decap_roundtrip() {
        let src = Ipv4Addr::new(10, 0, 0, 1);
        let dst = Ipv4Addr::new(10, 0, 0, 2);
        let mut tunnel = GreTunnel::new("gre0", src, dst, 0x00000001);
        tunnel.up = true;

        let payload = [0x45, 0x00, 0x00, 0x1C, 0x00, 0x01, 0x00, 0x00, 0x40, 0x06];
        let encapped = gre_tunnel_encapsulate(&mut tunnel, &payload).unwrap();

        assert!(encapped.len() >= 20 + GRE_KEY_SEQ_HEADER_LEN + payload.len());
        assert_eq!(tunnel.tx_packets, 1);
        assert!(tunnel.tx_bytes > 0);

        let mut rx_tunnel = GreTunnel::new("gre0", src, dst, 0x00000001);
        rx_tunnel.up = true;
        let decapped = gre_tunnel_decapsulate(&mut rx_tunnel, &encapped).unwrap();
        assert_eq!(decapped, payload);
        assert_eq!(rx_tunnel.rx_packets, 1);
    }

    #[test]
    fn gre_tunnel_wrong_key_rejected() {
        let src = Ipv4Addr::new(10, 0, 0, 1);
        let dst = Ipv4Addr::new(10, 0, 0, 2);
        let mut tx_tunnel = GreTunnel::new("gre0", src, dst, 0x00000001);
        tx_tunnel.up = true;

        let payload = [0x01, 0x02, 0x03, 0x04];
        let encapped = gre_tunnel_encapsulate(&mut tx_tunnel, &payload).unwrap();

        let mut rx_tunnel = GreTunnel::new("gre0", src, dst, 0x00000099);
        rx_tunnel.up = true;
        let result = gre_tunnel_decapsulate(&mut rx_tunnel, &encapped);
        assert_eq!(result, Err(NetError::ProtocolError));
        assert_eq!(rx_tunnel.rx_errors, 1);
    }

    #[test]
    fn gre_tunnel_down_rejects() {
        let src = Ipv4Addr::new(10, 0, 0, 1);
        let dst = Ipv4Addr::new(10, 0, 0, 2);
        let mut tunnel = GreTunnel::new("gre0", src, dst, 1);
        tunnel.up = false;

        let result = gre_tunnel_encapsulate(&mut tunnel, &[0x01]);
        assert_eq!(result, Err(NetError::NotUp));
    }

    #[test]
    fn gre_tunnel_checksum_encap_decap() {
        let src = Ipv4Addr::new(10, 0, 0, 1);
        let dst = Ipv4Addr::new(10, 0, 0, 2);
        let mut tx_tunnel = GreTunnel::new("gre0", src, dst, 0x00000001)
            .with_checksum(true);
        tx_tunnel.up = true;
        tx_tunnel.key_present = false;

        let payload = [0xAA, 0xBB, 0xCC, 0xDD];
        let encapped = gre_tunnel_encapsulate(&mut tx_tunnel, &payload).unwrap();

        let gre_data = &encapped[20..];
        let hdr = GreHeader::parse(gre_data).unwrap();
        assert!(hdr.checksum.is_some());

        let mut rx_tunnel = GreTunnel::new("gre0", src, dst, 0x00000001)
            .with_checksum(true);
        rx_tunnel.up = true;
        rx_tunnel.key_present = false;
        let decapped = gre_tunnel_decapsulate(&mut rx_tunnel, &encapped).unwrap();
        assert_eq!(decapped, payload);
    }

    #[test]
    fn gre_tunnel_seq_counter() {
        let src = Ipv4Addr::new(10, 0, 0, 1);
        let dst = Ipv4Addr::new(10, 0, 0, 2);
        let mut tunnel = GreTunnel::new("gre0", src, dst, 1)
            .with_seq(true);
        tunnel.up = true;
        tunnel.key_present = false;

        let _ = gre_tunnel_encapsulate(&mut tunnel, &[0x01]).unwrap();
        assert_eq!(tunnel.seq_counter, 1);
        let _ = gre_tunnel_encapsulate(&mut tunnel, &[0x02]).unwrap();
        assert_eq!(tunnel.seq_counter, 2);
        let _ = gre_tunnel_encapsulate(&mut tunnel, &[0x03]).unwrap();
        assert_eq!(tunnel.seq_counter, 3);
    }

    #[test]
    fn gre_keepalive_timeout_detected() {
        let src = Ipv4Addr::new(10, 0, 0, 1);
        let dst = Ipv4Addr::new(10, 0, 0, 2);
        let tunnel = GreTunnel::new("gre0", src, dst, 1)
            .with_keepalive(10, 30);

        let mut t = tunnel;
        t.last_rx_tick = 100;
        t.last_tx_tick = 100;
        let action = gre_tunnel_keepalive_check(&t, 105);
        assert_eq!(action, GreKeepaliveAction::None);

        let action = gre_tunnel_keepalive_check(&t, 131);
        assert_eq!(action, GreKeepaliveAction::Timeout);
    }

    #[test]
    fn gre_keepalive_send_needed() {
        let src = Ipv4Addr::new(10, 0, 0, 1);
        let dst = Ipv4Addr::new(10, 0, 0, 2);
        let mut tunnel = GreTunnel::new("gre0", src, dst, 1)
            .with_keepalive(10, 30);
        tunnel.last_tx_tick = 0;
        tunnel.last_rx_tick = 0;

        let action = gre_tunnel_keepalive_check(&tunnel, 11);
        assert_eq!(action, GreKeepaliveAction::SendKeepalive);
    }

    #[test]
    fn gre_keepalive_disabled() {
        let src = Ipv4Addr::new(10, 0, 0, 1);
        let dst = Ipv4Addr::new(10, 0, 0, 2);
        let tunnel = GreTunnel::new("gre0", src, dst, 1);

        let action = gre_tunnel_keepalive_check(&tunnel, 1000);
        assert_eq!(action, GreKeepaliveAction::None);
    }

    #[test]
    fn gre_build_keepalive_packet() {
        let ka = gre_build_keepalive(0x00000042);
        let (payload, proto, key) = gre_decapsulate(&ka).unwrap();
        assert!(payload.is_empty());
        assert_eq!(proto, 0);
        assert_eq!(key, Some(0x00000042));
    }

    #[test]
    fn gre_reserved_bits_rejected() {
        let mut buf = [0u8; 8];
        buf[0] = 0x00;
        buf[1] = 0x01;
        buf[3] = 0x08;
        let result = GreHeader::parse(&buf);
        assert_eq!(result, Err(NetError::InvalidPacket));
    }

    #[test]
    fn gre_key_only_header() {
        let mut buf = [0u8; 12];
        buf[0] = (GRE_FLAG_K >> 8) as u8;
        buf[1] = GRE_FLAG_K as u8;
        buf[3] = 0x08;
        buf[8] = 0xAB;
        buf[9] = 0xCD;
        buf[10] = 0xEF;
        buf[11] = 0x01;

        let parsed = GreHeader::parse(&buf).unwrap();
        assert_eq!(parsed.key, Some(0xABCDEF01));
        assert!(parsed.seq.is_none());
        assert!(parsed.checksum.is_none());
    }

    #[test]
    fn gre_protocol_ethernet() {
        let inner = [0xFF; 64];
        let gre_pkt = gre_encapsulate(&inner, 0x01, GRE_PROTO_ETHERNET);
        let (_, proto, _) = gre_decapsulate(&gre_pkt).unwrap();
        assert_eq!(proto, GRE_PROTO_ETHERNET);
    }

    #[test]
    fn gre_protocol_mpls() {
        let inner = [0x00; 32];
        let gre_pkt = gre_encapsulate(&inner, 0x01, GRE_PROTO_MPLS);
        let (_, proto, _) = gre_decapsulate(&gre_pkt).unwrap();
        assert_eq!(proto, GRE_PROTO_MPLS);
    }

    #[test]
    fn gre_protocol_ipv6() {
        let inner = [0x60; 40];
        let gre_pkt = gre_encapsulate(&inner, 0x01, GRE_PROTO_IPV6);
        let (_, proto, _) = gre_decapsulate(&gre_pkt).unwrap();
        assert_eq!(proto, GRE_PROTO_IPV6);
    }
}
