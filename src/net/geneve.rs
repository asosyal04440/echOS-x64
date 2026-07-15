use super::NetError;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;

pub const GENEVE_UDP_PORT: u16 = 6081;
pub const GENEVE_VERSION: u8 = 0;
pub const GENEVE_VNI_MAX: u32 = 0x00FF_FFFF;
pub const GENEVE_BASE_HEADER_LEN: usize = 8;
pub const GENEVE_TLV_HEADER_LEN: usize = 4;
pub const GENEVE_OPT_LEN_UNIT: usize = 4;
pub const GENEVE_PROTOCOL_ETHERNET: u16 = 0x6558;

pub const GENEVE_OPT_CLASS_IANA: u16 = 0x0000;
pub const GENEVE_OPT_TYPE_PORT_MAPPING: u8 = 0x01;
pub const GENEVE_OPT_TYPE_OUTER_ETH_MAC: u8 = 0x02;
pub const GENEVE_OPT_TYPE_TUNNEL_ID: u8 = 0x03;

const VERSION_MASK: u8 = 0x03;
const VERSION_SHIFT: u8 = 6;
const FLAG_CRITICAL: u8 = 0x20;
const FLAG_OAM: u8 = 0x10;
const FLAG_CRITICAL_OPTS: u8 = 0x08;
const FLAG_MAX_LENGTH: u8 = 0x04;
const TYPE_MASK: u8 = 0x3F;
const TYPE_SHIFT: u8 = 2;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneveOption {
    pub class: u16,
    pub opt_type: u8,
    pub data: Vec<u8>,
}

impl GeneveOption {
    pub fn new(class: u16, opt_type: u8, data: Vec<u8>) -> Self {
        Self { class, opt_type, data }
    }

    pub fn port_mapping(port: u16) -> Self {
        Self {
            class: GENEVE_OPT_CLASS_IANA,
            opt_type: GENEVE_OPT_TYPE_PORT_MAPPING,
            data: port.to_be_bytes().to_vec(),
        }
    }

    pub fn outer_ethernet_mac(mac: [u8; 6]) -> Self {
        Self {
            class: GENEVE_OPT_CLASS_IANA,
            opt_type: GENEVE_OPT_TYPE_OUTER_ETH_MAC,
            data: mac.to_vec(),
        }
    }

    pub fn tunnel_id(id: u64) -> Self {
        Self {
            class: GENEVE_OPT_CLASS_IANA,
            opt_type: GENEVE_OPT_TYPE_TUNNEL_ID,
            data: id.to_be_bytes().to_vec(),
        }
    }

    fn padded_data_len(&self) -> usize {
        (self.data.len() + GENEVE_OPT_LEN_UNIT - 1) & !(GENEVE_OPT_LEN_UNIT - 1)
    }

    pub fn total_len(&self) -> usize {
        GENEVE_TLV_HEADER_LEN + self.padded_data_len()
    }

    pub fn serialize(&self, buf: &mut [u8]) -> Result<(), NetError> {
        let total = self.total_len();
        if buf.len() < total {
            return Err(NetError::BufferFull);
        }
        buf[0..2].copy_from_slice(&self.class.to_be_bytes());
        buf[2] = (self.opt_type & TYPE_MASK) << TYPE_SHIFT;
        let data_words = self.padded_data_len() / GENEVE_OPT_LEN_UNIT;
        buf[3] = data_words as u8;
        let padded = self.padded_data_len();
        buf[4..4 + padded].fill(0);
        buf[4..4 + self.data.len()].copy_from_slice(&self.data);
        Ok(())
    }

    pub fn parse(data: &[u8]) -> Result<(Self, usize), NetError> {
        if data.len() < GENEVE_TLV_HEADER_LEN {
            return Err(NetError::InvalidPacket);
        }
        let class = u16::from_be_bytes([data[0], data[1]]);
        let opt_type = (data[2] >> TYPE_SHIFT) & TYPE_MASK;
        let data_words = data[3] as usize;
        let data_len = data_words * GENEVE_OPT_LEN_UNIT;
        let total = GENEVE_TLV_HEADER_LEN + data_len;
        if data.len() < total {
            return Err(NetError::InvalidPacket);
        }
        let opt_data = data[4..total].to_vec();
        Ok((Self { class, opt_type, data: opt_data }, total))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GeneveHeader {
    pub version: u8,
    pub critical: bool,
    pub oam: bool,
    pub critical_opts: bool,
    pub max_length: bool,
    pub protocol: u16,
    pub vni: u32,
    pub opt_len: u8,
}

impl GeneveHeader {
    pub fn new(vni: u32, protocol: u16) -> Self {
        Self {
            version: GENEVE_VERSION,
            critical: false,
            oam: false,
            critical_opts: false,
            max_length: false,
            protocol,
            vni: vni & GENEVE_VNI_MAX,
            opt_len: 0,
        }
    }

    pub fn serialize(&self) -> [u8; 8] {
        let mut buf = [0u8; 8];
        let mut b0 = (self.version & VERSION_MASK) << VERSION_SHIFT;
        if self.critical {
            b0 |= FLAG_CRITICAL;
        }
        if self.oam {
            b0 |= FLAG_OAM;
        }
        if self.critical_opts {
            b0 |= FLAG_CRITICAL_OPTS;
        }
        if self.max_length {
            b0 |= FLAG_MAX_LENGTH;
        }
        buf[0] = b0;
        buf[1] = self.opt_len;
        buf[2..4].copy_from_slice(&self.protocol.to_be_bytes());
        let v = self.vni.to_be_bytes();
        buf[4] = v[1];
        buf[5] = v[2];
        buf[6] = v[3];
        buf[7] = 0;
        buf
    }

    pub fn parse(data: &[u8]) -> Result<Self, NetError> {
        if data.len() < GENEVE_BASE_HEADER_LEN {
            return Err(NetError::InvalidPacket);
        }
        let b0 = data[0];
        Ok(Self {
            version: (b0 >> VERSION_SHIFT) & VERSION_MASK,
            critical: (b0 & FLAG_CRITICAL) != 0,
            oam: (b0 & FLAG_OAM) != 0,
            critical_opts: (b0 & FLAG_CRITICAL_OPTS) != 0,
            max_length: (b0 & FLAG_MAX_LENGTH) != 0,
            protocol: u16::from_be_bytes([data[2], data[3]]),
            vni: ((data[4] as u32) << 16) | ((data[5] as u32) << 8) | (data[6] as u32),
            opt_len: data[1],
        })
    }

    fn options_len(&self) -> usize {
        (self.opt_len as usize) * GENEVE_OPT_LEN_UNIT
    }
}

pub fn geneve_encapsulate(inner: &[u8], vni: u32, dst_port: u16) -> Result<Vec<u8>, NetError> {
    let _ = dst_port;
    let hdr = GeneveHeader::new(vni, GENEVE_PROTOCOL_ETHERNET);
    let mut packet = Vec::with_capacity(GENEVE_BASE_HEADER_LEN + inner.len());
    packet.extend_from_slice(&hdr.serialize());
    packet.extend_from_slice(inner);
    Ok(packet)
}

pub fn geneve_encapsulate_with_opts(
    inner: &[u8],
    vni: u32,
    options: &[GeneveOption],
) -> Result<Vec<u8>, NetError> {
    let mut total_opts_len = 0usize;
    for opt in options {
        total_opts_len += opt.total_len();
    }
    if total_opts_len > 255 * GENEVE_OPT_LEN_UNIT {
        return Err(NetError::InvalidParam);
    }
    let mut hdr = GeneveHeader::new(vni, GENEVE_PROTOCOL_ETHERNET);
    hdr.opt_len = (total_opts_len / GENEVE_OPT_LEN_UNIT) as u8;
    let mut packet = Vec::with_capacity(GENEVE_BASE_HEADER_LEN + total_opts_len + inner.len());
    packet.extend_from_slice(&hdr.serialize());
    let mut opt_buf = vec![0u8; total_opts_len];
    let mut offset = 0;
    for opt in options {
        opt.serialize(&mut opt_buf[offset..])?;
        offset += opt.total_len();
    }
    packet.extend_from_slice(&opt_buf);
    packet.extend_from_slice(inner);
    Ok(packet)
}

pub fn geneve_decapsulate(packet: &[u8]) -> Result<(&[u8], u32, Vec<GeneveOption>), NetError> {
    if packet.len() < GENEVE_BASE_HEADER_LEN {
        return Err(NetError::InvalidPacket);
    }
    let hdr = GeneveHeader::parse(packet)?;
    let opts_len = hdr.options_len();
    let total_hdr_len = GENEVE_BASE_HEADER_LEN + opts_len;
    if packet.len() < total_hdr_len {
        return Err(NetError::InvalidPacket);
    }
    let mut options = Vec::new();
    let mut offset = GENEVE_BASE_HEADER_LEN;
    while offset < total_hdr_len {
        let (opt, consumed) = GeneveOption::parse(&packet[offset..])?;
        options.push(opt);
        offset += consumed;
    }
    Ok((&packet[total_hdr_len..], hdr.vni, options))
}

#[derive(Clone, Debug)]
pub struct GeneveTunnel {
    pub vni: u32,
    pub local_port: u16,
    pub remote_port: u16,
    pub options: Vec<GeneveOption>,
    pub up: bool,
    pub rx_packets: u64,
    pub tx_packets: u64,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

impl GeneveTunnel {
    pub fn new(vni: u32, local_port: u16, remote_port: u16) -> Self {
        Self {
            vni: vni & GENEVE_VNI_MAX,
            local_port,
            remote_port,
            options: Vec::new(),
            up: false,
            rx_packets: 0,
            tx_packets: 0,
            rx_bytes: 0,
            tx_bytes: 0,
        }
    }

    pub fn with_options(
        vni: u32,
        local_port: u16,
        remote_port: u16,
        options: Vec<GeneveOption>,
    ) -> Self {
        Self {
            vni: vni & GENEVE_VNI_MAX,
            local_port,
            remote_port,
            options,
            up: false,
            rx_packets: 0,
            tx_packets: 0,
            rx_bytes: 0,
            tx_bytes: 0,
        }
    }
}

pub fn geneve_tunnel_encapsulate(
    tunnel: &mut GeneveTunnel,
    payload: &[u8],
) -> Result<Vec<u8>, NetError> {
    if !tunnel.up {
        return Err(NetError::NotUp);
    }
    let result = if tunnel.options.is_empty() {
        geneve_encapsulate(payload, tunnel.vni, tunnel.remote_port)?
    } else {
        geneve_encapsulate_with_opts(payload, tunnel.vni, &tunnel.options)?
    };
    tunnel.tx_packets += 1;
    tunnel.tx_bytes += result.len() as u64;
    Ok(result)
}

pub fn geneve_tunnel_decapsulate(
    tunnel: &mut GeneveTunnel,
    packet: &[u8],
) -> Result<Vec<u8>, NetError> {
    if !tunnel.up {
        return Err(NetError::NotUp);
    }
    let (payload, vni, _opts) = geneve_decapsulate(packet)?;
    if vni != tunnel.vni {
        return Err(NetError::InvalidPacket);
    }
    let result = payload.to_vec();
    tunnel.rx_packets += 1;
    tunnel.rx_bytes += result.len() as u64;
    Ok(result)
}

static GENEVE_TUNNELS: Mutex<BTreeMap<String, GeneveTunnel>> = Mutex::new(BTreeMap::new());

static GENEVE_STATS: GeneveStats = GeneveStats::new();
struct GeneveStats {
    tunnels: AtomicU32,
    encap_ok: AtomicU32,
    decap_ok: AtomicU32,
}
impl GeneveStats {
    const fn new() -> Self {
        Self {
            tunnels: AtomicU32::new(0),
            encap_ok: AtomicU32::new(0),
            decap_ok: AtomicU32::new(0),
        }
    }
}

pub fn create_tunnel(
    name: &str,
    vni: u32,
    local_port: u16,
    remote_port: u16,
) -> Result<(), NetError> {
    let mut tunnels = GENEVE_TUNNELS.lock();
    if tunnels.contains_key(name) {
        return Err(NetError::AddrInUse);
    }
    if vni == 0 || vni > GENEVE_VNI_MAX {
        return Err(NetError::InvalidParam);
    }
    tunnels.insert(
        String::from(name),
        GeneveTunnel::new(vni, local_port, remote_port),
    );
    GENEVE_STATS.tunnels.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

pub fn set_tunnel_up(name: &str, up: bool) -> Result<(), NetError> {
    let mut tunnels = GENEVE_TUNNELS.lock();
    let tunnel = tunnels.get_mut(name).ok_or(NetError::InvalidParam)?;
    tunnel.up = up;
    Ok(())
}

pub fn tunnel_encapsulate(name: &str, payload: &[u8]) -> Result<Vec<u8>, NetError> {
    let mut tunnels = GENEVE_TUNNELS.lock();
    let tunnel = tunnels.get_mut(name).ok_or(NetError::InvalidParam)?;
    let result = geneve_tunnel_encapsulate(tunnel, payload)?;
    GENEVE_STATS.encap_ok.fetch_add(1, Ordering::Relaxed);
    Ok(result)
}

pub fn tunnel_decapsulate(name: &str, packet: &[u8]) -> Result<Vec<u8>, NetError> {
    let mut tunnels = GENEVE_TUNNELS.lock();
    let tunnel = tunnels.get_mut(name).ok_or(NetError::InvalidParam)?;
    let result = geneve_tunnel_decapsulate(tunnel, packet)?;
    GENEVE_STATS.decap_ok.fetch_add(1, Ordering::Relaxed);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_roundtrip() {
        let hdr = GeneveHeader::new(100, GENEVE_PROTOCOL_ETHERNET);
        let bytes = hdr.serialize();
        let parsed = GeneveHeader::parse(&bytes).unwrap();
        assert_eq!(parsed.vni, 100);
        assert_eq!(parsed.protocol, GENEVE_PROTOCOL_ETHERNET);
        assert_eq!(parsed.version, 0);
        assert!(!parsed.critical);
        assert!(!parsed.oam);
        assert!(!parsed.critical_opts);
        assert!(!parsed.max_length);
        assert_eq!(parsed.opt_len, 0);
    }

    #[test]
    fn header_wire_layout() {
        let mut hdr = GeneveHeader::new(0x00AB_CDEF, 0x0800);
        hdr.critical = true;
        hdr.oam = true;
        let b = hdr.serialize();
        assert_eq!(b[0], 0x30);
        assert_eq!(b[1], 0);
        assert_eq!(b[2], 0x08);
        assert_eq!(b[3], 0x00);
        assert_eq!(b[4], 0xAB);
        assert_eq!(b[5], 0xCD);
        assert_eq!(b[6], 0xEF);
        assert_eq!(b[7], 0);
    }

    #[test]
    fn header_flags_roundtrip() {
        let mut hdr = GeneveHeader::new(42, 0x6558);
        hdr.critical = true;
        hdr.critical_opts = true;
        hdr.max_length = true;
        let b = hdr.serialize();
        let parsed = GeneveHeader::parse(&b).unwrap();
        assert!(parsed.critical);
        assert!(!parsed.oam);
        assert!(parsed.critical_opts);
        assert!(parsed.max_length);
        assert_eq!(parsed.vni, 42);
    }

    #[test]
    fn vni_truncated_to_24_bits() {
        let hdr = GeneveHeader::new(0x01FF_FFFF, 0x6558);
        assert_eq!(hdr.vni, 0x00FF_FFFF);
    }

    #[test]
    fn parse_rejects_short_data() {
        assert!(GeneveHeader::parse(&[]).is_err());
        assert!(GeneveHeader::parse(&[0u8; 7]).is_err());
    }

    #[test]
    fn option_port_mapping_roundtrip() {
        let opt = GeneveOption::port_mapping(8080);
        assert_eq!(opt.class, GENEVE_OPT_CLASS_IANA);
        assert_eq!(opt.opt_type, GENEVE_OPT_TYPE_PORT_MAPPING);
        assert_eq!(opt.data.len(), 2);
        let mut buf = vec![0u8; opt.total_len()];
        opt.serialize(&mut buf).unwrap();
        let (parsed, consumed) = GeneveOption::parse(&buf).unwrap();
        assert_eq!(consumed, opt.total_len());
        assert_eq!(parsed.class, GENEVE_OPT_CLASS_IANA);
        assert_eq!(parsed.opt_type, GENEVE_OPT_TYPE_PORT_MAPPING);
        let port = u16::from_be_bytes([parsed.data[0], parsed.data[1]]);
        assert_eq!(port, 8080);
    }

    #[test]
    fn option_outer_mac_roundtrip() {
        let mac = [0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x01];
        let opt = GeneveOption::outer_ethernet_mac(mac);
        let mut buf = vec![0u8; opt.total_len()];
        opt.serialize(&mut buf).unwrap();
        let (parsed, consumed) = GeneveOption::parse(&buf).unwrap();
        assert_eq!(consumed, 12);
        assert_eq!(parsed.data[..6], mac);
    }

    #[test]
    fn option_tunnel_id_roundtrip() {
        let opt = GeneveOption::tunnel_id(0x0000_0000_0000_002A);
        let mut buf = vec![0u8; opt.total_len()];
        opt.serialize(&mut buf).unwrap();
        let (parsed, _) = GeneveOption::parse(&buf).unwrap();
        assert_eq!(parsed.data.len(), 8);
        let id = u64::from_be_bytes(parsed.data.try_into().unwrap());
        assert_eq!(id, 42);
    }

    #[test]
    fn option_tlv_wire_layout() {
        let opt = GeneveOption::new(0x0100, 0x05, vec![0xAA, 0xBB]);
        let mut buf = vec![0u8; opt.total_len()];
        opt.serialize(&mut buf).unwrap();
        assert_eq!(buf[0], 0x01);
        assert_eq!(buf[1], 0x00);
        assert_eq!(buf[2], 0x05 << 2);
        assert_eq!(buf[3], 1);
        assert_eq!(buf[4], 0xAA);
        assert_eq!(buf[5], 0xBB);
        assert_eq!(buf[6], 0);
        assert_eq!(buf[7], 0);
    }

    #[test]
    fn encap_decap_roundtrip() {
        let inner: Vec<u8> = [0xFF; 6]
            .iter()
            .chain([0xAA; 6].iter())
            .chain([0x08, 0x00].iter())
            .chain([1, 2, 3, 4].iter())
            .copied()
            .collect();
        let packet = geneve_encapsulate(&inner, 100, GENEVE_UDP_PORT).unwrap();
        assert_eq!(packet.len(), GENEVE_BASE_HEADER_LEN + inner.len());
        let (payload, vni, opts) = geneve_decapsulate(&packet).unwrap();
        assert_eq!(payload, inner.as_slice());
        assert_eq!(vni, 100);
        assert!(opts.is_empty());
    }

    #[test]
    fn encap_with_no_options() {
        let inner = vec![0x01, 0x02, 0x03];
        let packet = geneve_encapsulate(&inner, 50, 9000).unwrap();
        let hdr = GeneveHeader::parse(&packet).unwrap();
        assert_eq!(hdr.opt_len, 0);
        assert_eq!(hdr.vni, 50);
        assert_eq!(&packet[GENEVE_BASE_HEADER_LEN..], inner.as_slice());
    }

    #[test]
    fn encap_with_options_roundtrip() {
        let inner = vec![0x01, 0x02, 0x03, 0x04];
        let opts = vec![GeneveOption::port_mapping(80), GeneveOption::tunnel_id(42)];
        let packet = geneve_encapsulate_with_opts(&inner, 200, &opts).unwrap();
        let (payload, vni, parsed_opts) = geneve_decapsulate(&packet).unwrap();
        assert_eq!(payload, inner.as_slice());
        assert_eq!(vni, 200);
        assert_eq!(parsed_opts.len(), 2);
        assert_eq!(parsed_opts[0].opt_type, GENEVE_OPT_TYPE_PORT_MAPPING);
        assert_eq!(parsed_opts[1].opt_type, GENEVE_OPT_TYPE_TUNNEL_ID);
        let port = u16::from_be_bytes([parsed_opts[0].data[0], parsed_opts[0].data[1]]);
        assert_eq!(port, 80);
        let id = u64::from_be_bytes(parsed_opts[1].data.clone().try_into().unwrap());
        assert_eq!(id, 42);
    }

    #[test]
    fn multi_option_packet() {
        let inner = vec![0xAA];
        let opts = vec![
            GeneveOption::port_mapping(443),
            GeneveOption::outer_ethernet_mac([1, 2, 3, 4, 5, 6]),
            GeneveOption::tunnel_id(999),
        ];
        let packet = geneve_encapsulate_with_opts(&inner, 777, &opts).unwrap();
        let (_, vni, parsed_opts) = geneve_decapsulate(&packet).unwrap();
        assert_eq!(vni, 777);
        assert_eq!(parsed_opts.len(), 3);
        assert_eq!(parsed_opts[0].opt_type, GENEVE_OPT_TYPE_PORT_MAPPING);
        assert_eq!(parsed_opts[1].opt_type, GENEVE_OPT_TYPE_OUTER_ETH_MAC);
        assert_eq!(parsed_opts[2].opt_type, GENEVE_OPT_TYPE_TUNNEL_ID);
    }

    #[test]
    fn decap_invalid_packet_too_short() {
        assert!(geneve_decapsulate(&[]).is_err());
        assert!(geneve_decapsulate(&[0u8; 7]).is_err());
    }

    #[test]
    fn decap_invalid_opts_length() {
        let mut hdr = GeneveHeader::new(1, GENEVE_PROTOCOL_ETHERNET);
        hdr.opt_len = 10;
        let mut packet = hdr.serialize().to_vec();
        packet.extend_from_slice(&[0u8; 4]);
        assert!(geneve_decapsulate(&packet).is_err());
    }

    #[test]
    fn vni_max_value() {
        let packet = geneve_encapsulate(&[0x01], GENEVE_VNI_MAX, GENEVE_UDP_PORT).unwrap();
        let (_, vni, _) = geneve_decapsulate(&packet).unwrap();
        assert_eq!(vni, GENEVE_VNI_MAX);
    }

    #[test]
    fn vni_overflow_truncated() {
        let packet = geneve_encapsulate(&[0x01], 0x01FF_FFFF, GENEVE_UDP_PORT).unwrap();
        let (_, vni, _) = geneve_decapsulate(&packet).unwrap();
        assert_eq!(vni, GENEVE_VNI_MAX);
    }

    #[test]
    fn tunnel_encap_decap_roundtrip() {
        let mut tunnel = GeneveTunnel::new(100, 6081, 6081);
        tunnel.up = true;
        let payload = vec![0xAA; 100];
        let packet = geneve_tunnel_encapsulate(&mut tunnel, &payload).unwrap();
        assert_eq!(tunnel.tx_packets, 1);
        assert_eq!(tunnel.tx_bytes, packet.len() as u64);
        let result = geneve_tunnel_decapsulate(&mut tunnel, &packet).unwrap();
        assert_eq!(result, payload);
        assert_eq!(tunnel.rx_packets, 1);
        assert_eq!(tunnel.rx_bytes, payload.len() as u64);
    }

    #[test]
    fn tunnel_vni_mismatch_rejected() {
        let mut tunnel = GeneveTunnel::new(100, 6081, 6081);
        tunnel.up = true;
        let packet = geneve_encapsulate(&[0x01], 200, GENEVE_UDP_PORT).unwrap();
        assert_eq!(
            geneve_tunnel_decapsulate(&mut tunnel, &packet),
            Err(NetError::InvalidPacket)
        );
    }

    #[test]
    fn tunnel_not_up_rejected() {
        let mut tunnel = GeneveTunnel::new(100, 6081, 6081);
        assert_eq!(
            geneve_tunnel_encapsulate(&mut tunnel, &[0x01]),
            Err(NetError::NotUp)
        );
        assert_eq!(
            geneve_tunnel_decapsulate(&mut tunnel, &[0u8; 8]),
            Err(NetError::NotUp)
        );
    }

    #[test]
    fn tunnel_with_options() {
        let opts = vec![GeneveOption::tunnel_id(55)];
        let mut tunnel = GeneveTunnel::with_options(300, 6081, 6081, opts);
        tunnel.up = true;
        let payload = vec![0xBB; 50];
        let packet = geneve_tunnel_encapsulate(&mut tunnel, &payload).unwrap();
        let (_, vni, parsed_opts) = geneve_decapsulate(&packet).unwrap();
        assert_eq!(vni, 300);
        assert_eq!(parsed_opts.len(), 1);
        assert_eq!(parsed_opts[0].opt_type, GENEVE_OPT_TYPE_TUNNEL_ID);
    }

    #[test]
    fn global_tunnel_create_and_encap() {
        create_tunnel("gt0", 100, 6081, 6081).unwrap();
        set_tunnel_up("gt0", true).unwrap();
        let pkt = tunnel_encapsulate("gt0", &[0x01, 0x02]).unwrap();
        let (payload, vni, _) = geneve_decapsulate(&pkt).unwrap();
        assert_eq!(payload, &[0x01, 0x02]);
        assert_eq!(vni, 100);
    }

    #[test]
    fn global_tunnel_duplicate_name() {
        create_tunnel("dup", 1, 6081, 6081).unwrap();
        assert_eq!(
            create_tunnel("dup", 2, 6081, 6081),
            Err(NetError::AddrInUse)
        );
    }

    #[test]
    fn global_tunnel_not_found() {
        assert_eq!(
            tunnel_encapsulate("nonexistent", &[0x01]),
            Err(NetError::InvalidParam)
        );
    }

    #[test]
    fn global_tunnel_decap() {
        create_tunnel("gt1", 200, 6081, 6081).unwrap();
        set_tunnel_up("gt1", true).unwrap();
        let orig = vec![0xCC; 20];
        let pkt = tunnel_encapsulate("gt1", &orig).unwrap();
        let result = tunnel_decapsulate("gt1", &pkt).unwrap();
        assert_eq!(result, orig);
    }

    #[test]
    fn zero_vni_rejected() {
        assert_eq!(
            create_tunnel("bad", 0, 6081, 6081),
            Err(NetError::InvalidParam)
        );
    }

    #[test]
    fn vni_overflow_rejected() {
        assert_eq!(
            create_tunnel("bad", GENEVE_VNI_MAX + 1, 6081, 6081),
            Err(NetError::InvalidParam)
        );
    }
}
