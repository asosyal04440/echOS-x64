use super::{IpAddr, Ipv4Addr, NetError};
use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;
use core::cmp::Ordering;

const MPTCP_VERSION_0: u8 = 0;
const MPTCP_VERSION_1: u8 = 1;
const MPTCP_CAPABLE_SYN: u8 = 0x00;
const MPTCP_CAPABLE_SYNACK: u8 = 0x01;
const MPTCP_CAPABLE_ACK: u8 = 0x02;
const MPTCP_SUBFLOW_ADD: u8 = 0x00;
const MPTCP_SUBFLOW_REMOVE: u8 = 0x80;
const MP_CAPABLE_OPT: u8 = 0x00;
const MP_JOIN_OPT: u8 = 0x01;
const MP_ADD_ADDR_OPT: u8 = 0x03;
const MP_REMOVE_ADDR_OPT: u8 = 0x04;
const MPTCP_DSN_LEN: usize = 8;
const MAX_SUBFLOWS: usize = 8;
const MPTCP_SUBOPTION_LEN: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MptcpVersion {
    V0 = 0,
    V1 = 1,
}

impl MptcpVersion {
    pub fn from_u8(val: u8) -> Option<Self> {
        match val {
            0 => Some(MptcpVersion::V0),
            1 => Some(MptcpVersion::V1),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MptcpSubflowState {
    Closed,
    Listens,
    SynSent,
    SynRecv,
    Established,
    FinWait1,
    FinWait2,
    Closing,
    CloseWait,
    LastAck,
    TimeWait,
    Close,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathManagerMode {
    Kernel,
    Userspace,
}

#[derive(Clone, Debug)]
pub struct MptcpSubflow {
    pub id: u8,
    pub local_addr: IpAddr,
    pub remote_addr: IpAddr,
    pub local_port: u16,
    pub remote_port: u16,
    pub snd_nxt: u32,
    pub rcv_nxt: u32,
    pub snd_wnd: u32,
    pub cwnd: u32,
    pub ssthresh: u32,
    pub state: MptcpSubflowState,
    pub rtt: u32,
    pub rto: u32,
    pub retransmits: u8,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub packets_sent: u64,
    pub packets_received: u64,
    pub snd_una: u32,
    pub mss: u16,
    pub srtt: u32,
    pub rttvar: u32,
}

impl MptcpSubflow {
    pub fn new(
        id: u8,
        local_addr: IpAddr,
        remote_addr: IpAddr,
        local_port: u16,
        remote_port: u16,
    ) -> Self {
        MptcpSubflow {
            id,
            local_addr,
            remote_addr,
            local_port,
            remote_port,
            snd_nxt: 0,
            rcv_nxt: 0,
            snd_wnd: 65535,
            cwnd: 10,
            ssthresh: 65535,
            state: MptcpSubflowState::Closed,
            rtt: 0,
            rto: 3000,
            retransmits: 0,
            bytes_sent: 0,
            bytes_received: 0,
            packets_sent: 0,
            packets_received: 0,
            snd_una: 0,
            mss: 1460,
            srtt: 0,
            rttvar: 0,
        }
    }

    pub fn congestion_window_bytes(&self) -> u32 {
        self.cwnd * self.mss as u32
    }

    pub fn in_flight(&self) -> u32 {
        self.snd_nxt.saturating_sub(self.snd_una)
    }

    pub fn flight_size(&self) -> u32 {
        let in_flight = self.in_flight();
        let cwnd = self.congestion_window_bytes();
        if in_flight < cwnd {
            cwnd - in_flight
        } else {
            0
        }
    }
}

#[derive(Clone, Debug)]
pub struct MptcpToken {
    pub token: u32,
    pub local_addr: IpAddr,
}

#[derive(Clone, Debug)]
pub struct MptcpAddr {
    pub addr_id: u8,
    pub ip: IpAddr,
    pub port: u16,
    pub is_backup: bool,
}

#[derive(Clone, Debug)]
pub struct MpCapable {
    pub version: u8,
    pub flags: u8,
    pub snd_auth_key: u64,
    pub rcv_auth_key: u64,
}

#[derive(Clone, Debug)]
pub struct MpJoin {
    pub subflow_token: u32,
    pub truncated_mac: [u8; 8],
    pub nonce: u32,
}

#[derive(Clone, Debug)]
pub struct AddAddr {
    pub addr_id: u8,
    pub ip: IpAddr,
    pub port: u16,
}

#[derive(Clone, Debug)]
pub struct RemoveAddr {
    pub addr_id: u8,
}

#[derive(Clone, Debug)]
pub struct MptcpConnection {
    pub local_token: u32,
    pub remote_token: u32,
    pub subflows: Vec<MptcpSubflow>,
    pub snd_keys: Vec<u64>,
    pub rcv_keys: Vec<u64>,
    pub pm_mode: PathManagerMode,
    pub version: MptcpVersion,
    pub local_addrs: Vec<MptcpAddr>,
    pub remote_addrs: Vec<MptcpAddr>,
    pub dsn_next: u32,
    pub dsn_map: BTreeMap<u32, DsnMapEntry>,
    pub data_avail: u64,
}

#[derive(Clone, Debug)]
pub struct DsnMapEntry {
    pub subflow_id: u8,
    pub subflow_seq: u32,
    pub data_seq: u32,
}

impl MptcpConnection {
    pub fn new(local_token: u32) -> Self {
        MptcpConnection {
            local_token,
            remote_token: 0,
            subflows: Vec::new(),
            snd_keys: Vec::new(),
            rcv_keys: Vec::new(),
            pm_mode: PathManagerMode::Kernel,
            version: MptcpVersion::V1,
            local_addrs: Vec::new(),
            remote_addrs: Vec::new(),
            dsn_next: 0,
            dsn_map: BTreeMap::new(),
            data_avail: 0,
        }
    }

    pub fn subflow_count(&self) -> usize {
        self.subflows.len()
    }

    pub fn total_bytes_sent(&self) -> u64 {
        self.subflows.iter().map(|sf| sf.bytes_sent).sum()
    }

    pub fn total_bytes_received(&self) -> u64 {
        self.subflows.iter().map(|sf| sf.bytes_received).sum()
    }

    pub fn established_count(&self) -> usize {
        self.subflows
            .iter()
            .filter(|sf| sf.state == MptcpSubflowState::Established)
            .count()
    }

    pub fn find_subflow(&self, id: u8) -> Option<&MptcpSubflow> {
        self.subflows.iter().find(|sf| sf.id == id)
    }

    pub fn find_subflow_mut(&mut self, id: u8) -> Option<&mut MptcpSubflow> {
        self.subflows.iter_mut().find(|sf| sf.id == id)
    }
}

pub fn mptcp_connection_new(local_token: u32) -> MptcpConnection {
    MptcpConnection::new(local_token)
}

pub fn mptcp_add_subflow(
    conn: &mut MptcpConnection,
    local_addr: IpAddr,
    remote_addr: IpAddr,
) -> Result<u8, NetError> {
    if conn.subflows.len() >= MAX_SUBFLOWS {
        return Err(NetError::BufferFull);
    }
    let id = conn.subflows.len() as u8;
    let local_port = 10000 + id as u16;
    let remote_port = 5000;
    let mut sf = MptcpSubflow::new(id, local_addr, remote_addr, local_port, remote_port);
    sf.state = MptcpSubflowState::SynSent;
    sf.snd_nxt = 1;
    sf.rcv_nxt = 0;
    conn.subflows.push(sf);
    Ok(id)
}

pub fn mptcp_remove_subflow(conn: &mut MptcpConnection, subflow_id: u8) -> Result<(), NetError> {
    let idx = conn
        .subflows
        .iter()
        .position(|sf| sf.id == subflow_id)
        .ok_or(NetError::InvalidParam)?;
    conn.subflows[idx].state = MptcpSubflowState::Closing;
    conn.subflows.remove(idx);
    Ok(())
}

pub fn mptcp_select_subflow(conn: &MptcpConnection) -> Option<u8> {
    conn.subflows
        .iter()
        .filter(|sf| sf.state == MptcpSubflowState::Established)
        .min_by(|a, b| {
            let score_a = a.rtt + a.in_flight() * 2;
            let score_b = b.rtt + b.in_flight() * 2;
            score_a.cmp(&score_b)
        })
        .map(|sf| sf.id)
}

pub fn mptcp_select_subflow_by_loss(conn: &MptcpConnection) -> Option<u8> {
    conn.subflows
        .iter()
        .filter(|sf| sf.state == MptcpSubflowState::Established)
        .min_by(|a, b| {
            let loss_a = a.retransmits as u32 * 1000 + a.rtt;
            let loss_b = b.retransmits as u32 * 1000 + b.rtt;
            loss_a.cmp(&loss_b)
        })
        .map(|sf| sf.id)
}

pub fn mptcp_build_mp_capable(snd_key: u64) -> [u8; 12] {
    let mut buf = [0u8; 12];
    buf[0] = MP_CAPABLE_OPT;
    buf[1] = 12;
    buf[2] = (MPTCP_VERSION_1 << 4) | MPTCP_CAPABLE_SYN;
    buf[3] = 0;
    buf[4..12].copy_from_slice(&snd_key.to_be_bytes());
    buf
}

pub fn mptcp_parse_mp_capable(data: &[u8]) -> Option<MpCapable> {
    if data.len() < 12 {
        return None;
    }
    let kind = data[0];
    if kind != MP_CAPABLE_OPT {
        return None;
    }
    let version = (data[2] >> 4) & 0x0F;
    let flags = data[2] & 0x0F;
    let snd_auth_key = u64::from_be_bytes([
        data[4], data[5], data[6], data[7], data[8], data[9], data[10], data[11],
    ]);
    let rcv_auth_key = if data.len() >= 20 {
        u64::from_be_bytes([
            data[12], data[13], data[14], data[15], data[16], data[17], data[18], data[19],
        ])
    } else {
        0
    };
    Some(MpCapable {
        version,
        flags,
        snd_auth_key,
        rcv_auth_key,
    })
}

pub fn mptcp_build_mp_join(subflow_token: u32, mac: &[u8; 8], nonce: u32) -> [u8; 20] {
    let mut buf = [0u8; 20];
    buf[0] = MP_JOIN_OPT;
    buf[1] = 20;
    buf[2] = 0;
    buf[3] = 0;
    buf[4..8].copy_from_slice(&subflow_token.to_be_bytes());
    buf[8..16].copy_from_slice(mac);
    buf[16..20].copy_from_slice(&nonce.to_be_bytes());
    buf
}

pub fn mptcp_parse_mp_join(data: &[u8]) -> Option<MpJoin> {
    if data.len() < 16 {
        return None;
    }
    let kind = data[0];
    if kind != MP_JOIN_OPT {
        return None;
    }
    let subflow_token = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
    let mut truncated_mac = [0u8; 8];
    truncated_mac.copy_from_slice(&data[8..16]);
    let nonce = if data.len() >= 20 {
        u32::from_be_bytes([data[16], data[17], data[18], data[19]])
    } else {
        0
    };
    Some(MpJoin {
        subflow_token,
        truncated_mac,
        nonce,
    })
}

pub fn mptcp_build_add_addr(addr_id: u8, addr: IpAddr) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.push(MP_ADD_ADDR_OPT);
    let ip_bytes = match addr {
        IpAddr::V4(ip) => {
            buf.push(8);
            buf.push(addr_id);
            buf.push(0);
            let b = ip.as_bytes();
            buf.extend_from_slice(b);
            buf
        }
        IpAddr::V6(_) => {
            buf.push(20);
            buf.push(addr_id);
            buf.push(0);
            match addr {
                IpAddr::V6(ip6) => {
                    let b = ip6.as_bytes();
                    buf.extend_from_slice(b);
                }
                _ => {}
            }
            buf
        }
    };
    ip_bytes
}

pub fn mptcp_parse_add_addr(data: &[u8]) -> Option<AddAddr> {
    if data.len() < 4 {
        return None;
    }
    let kind = data[0];
    if kind != MP_ADD_ADDR_OPT {
        return None;
    }
    let addr_id = data[2];
    let addr_len = data[1];
    let port = if addr_len == 8 && data.len() >= 10 {
        u16::from_be_bytes([data[8], data[9]])
    } else if addr_len == 20 && data.len() >= 22 {
        u16::from_be_bytes([data[20], data[21]])
    } else {
        0
    };
    let ip = if addr_len == 8 && data.len() >= 8 {
        IpAddr::V4(Ipv4Addr::new(data[3], data[4], data[5], data[6]))
    } else if addr_len == 20 && data.len() >= 20 {
        let mut ip6_bytes = [0u8; 16];
        ip6_bytes.copy_from_slice(&data[3..19]);
        IpAddr::V6(super::ipv6::Ipv6Addr::new(ip6_bytes))
    } else {
        return None;
    };
    Some(AddAddr {
        addr_id,
        ip,
        port,
    })
}

pub fn mptcp_build_remove_addr(addr_id: u8) -> [u8; 4] {
    let mut buf = [0u8; 4];
    buf[0] = MP_REMOVE_ADDR_OPT;
    buf[1] = 4;
    buf[2] = 0;
    buf[3] = addr_id;
    buf
}

pub fn mptcp_parse_remove_addr(data: &[u8]) -> Option<RemoveAddr> {
    if data.len() < 4 {
        return None;
    }
    let kind = data[0];
    if kind != MP_REMOVE_ADDR_OPT {
        return None;
    }
    Some(RemoveAddr { addr_id: data[3] })
}

pub fn mptcp_generate_token(key: u64) -> u32 {
    let mut hash: u32 = 0x811c9dc5;
    let bytes = key.to_be_bytes();
    for &b in bytes.iter() {
        hash ^= b as u32;
        hash = hash.wrapping_mul(0x01000193);
    }
    hash
}

pub fn mptcp_compute_mac(key: u64, nonce: u32) -> [u8; 8] {
    let mut mac = [0u8; 8];
    let key_bytes = key.to_be_bytes();
    let nonce_bytes = nonce.to_be_bytes();
    let mut state: u64 = 0;
    for i in 0..8 {
        state = state.wrapping_add(key_bytes[i] as u64);
        state = state.wrapping_add(nonce_bytes[i % 4] as u64);
        state ^= state << 13;
        state ^= state >> 7;
        state = state.wrapping_mul(0x9E3779B9);
        mac[i] = (state & 0xFF) as u8;
    }
    mac
}

pub fn mptcp_data_seq_offset(conn: &MptcpConnection) -> u32 {
    conn.dsn_next
}

pub fn mptcp_subflow_seq_to_data_seq(
    conn: &MptcpConnection,
    subflow_id: u8,
    subflow_seq: u32,
) -> Option<u32> {
    conn.dsn_map
        .iter()
        .find(|(_, entry)| entry.subflow_id == subflow_id && entry.subflow_seq == subflow_seq)
        .map(|(data_seq, _)| *data_seq)
}

pub fn mptcp_register_dsn(conn: &mut MptcpConnection, subflow_id: u8, subflow_seq: u32) -> u32 {
    let data_seq = conn.dsn_next;
    conn.dsn_map.insert(
        data_seq,
        DsnMapEntry {
            subflow_id,
            subflow_seq,
            data_seq,
        },
    );
    conn.dsn_next = conn.dsn_next.wrapping_add(1);
    data_seq
}

pub fn mptcp_calc_rto(srtt: u32, rttvar: u32) -> u32 {
    let rto = srtt + 4 * rttvar;
    rto.clamp(200, 60000)
}

pub fn mptcp_update_rtt(sf: &mut MptcpSubflow, sample: u32) {
    if sf.srtt == 0 {
        sf.srtt = sample;
        sf.rttvar = sample / 2;
    } else {
        let diff = if sample > sf.srtt {
            sample - sf.srtt
        } else {
            sf.srtt - sample
        };
        sf.rttvar = (3 * sf.rttvar + diff) / 4;
        sf.srtt = (7 * sf.srtt + sample) / 8;
    }
    sf.rtt = sf.srtt;
    sf.rto = mptcp_calc_rto(sf.srtt, sf.rttvar);
}

pub fn mptcp_congestion_avoidance(sf: &mut MptcpSubflow) {
    if sf.cwnd < sf.ssthresh {
        sf.cwnd += 1;
    } else {
        sf.cwnd = sf.cwnd.saturating_add(1);
    }
}

pub fn mptcp_fast_retransmit(sf: &mut MptcpSubflow) {
    sf.ssthresh = (sf.cwnd / 2).max(2);
    sf.cwnd = sf.ssthresh + 3;
    sf.retransmits += 1;
}

pub const MPTCP_ADDR_IP_LEN: usize = 4;

#[cfg(test)]
mod tests {
    use super::*;

    fn test_ip(a: u8, b: u8, c: u8, d: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(a, b, c, d))
    }

    #[test]
    fn test_mptcp_connection_new() {
        let conn = mptcp_connection_new(0x12345678);
        assert_eq!(conn.local_token, 0x12345678);
        assert_eq!(conn.subflow_count(), 0);
        assert_eq!(conn.version, MptcpVersion::V1);
    }

    #[test]
    fn test_add_remove_subflow() {
        let mut conn = mptcp_connection_new(100);
        let id = mptcp_add_subflow(
            &mut conn,
            test_ip(10, 0, 0, 1),
            test_ip(10, 0, 0, 2),
        )
        .unwrap();
        assert_eq!(id, 0);
        assert_eq!(conn.subflow_count(), 1);
        mptcp_remove_subflow(&mut conn, 0).unwrap();
        assert_eq!(conn.subflow_count(), 0);
    }

    #[test]
    fn test_max_subflows() {
        let mut conn = mptcp_connection_new(100);
        for _ in 0..MAX_SUBFLOWS {
            let _ = mptcp_add_subflow(
                &mut conn,
                test_ip(10, 0, 0, 1),
                test_ip(10, 0, 0, 2),
            );
        }
        assert!(mptcp_add_subflow(
            &mut conn,
            test_ip(10, 0, 0, 1),
            test_ip(10, 0, 0, 2),
        )
        .is_err());
    }

    #[test]
    fn test_select_subflow() {
        let mut conn = mptcp_connection_new(100);
        let id0 = mptcp_add_subflow(
            &mut conn,
            test_ip(10, 0, 0, 1),
            test_ip(10, 0, 0, 2),
        )
        .unwrap();
        let id1 = mptcp_add_subflow(
            &mut conn,
            test_ip(10, 0, 0, 3),
            test_ip(10, 0, 0, 4),
        )
        .unwrap();
        conn.find_subflow_mut(id0).unwrap().rtt = 100;
        conn.find_subflow_mut(id1).unwrap().rtt = 50;
        conn.find_subflow_mut(id0).unwrap().state = MptcpSubflowState::Established;
        conn.find_subflow_mut(id1).unwrap().state = MptcpSubflowState::Established;
        assert_eq!(mptcp_select_subflow(&conn), Some(1));
    }

    #[test]
    fn test_mp_capable_build_parse() {
        let key = 0xAABBCCDDEEFF0011u64;
        let built = mptcp_build_mp_capable(key);
        assert_eq!(built[0], MP_CAPABLE_OPT);
        assert_eq!(built[1], 12);
        let parsed = mptcp_parse_mp_capable(&built).unwrap();
        assert_eq!(parsed.snd_auth_key, key);
        assert_eq!(parsed.version, MPTCP_VERSION_1);
    }

    #[test]
    fn test_mp_join_build_parse() {
        let mac = [1, 2, 3, 4, 5, 6, 7, 8];
        let built = mptcp_build_mp_join(0xDEADBEEF, &mac, 42);
        let parsed = mptcp_parse_mp_join(&built).unwrap();
        assert_eq!(parsed.subflow_token, 0xDEADBEEF);
        assert_eq!(parsed.truncated_mac, mac);
        assert_eq!(parsed.nonce, 42);
    }

    #[test]
    fn test_add_addr_build_parse() {
        let addr = test_ip(192, 168, 1, 100);
        let built = mptcp_build_add_addr(1, addr);
        assert_eq!(built[0], MP_ADD_ADDR_OPT);
        let parsed = mptcp_parse_add_addr(&built).unwrap();
        assert_eq!(parsed.addr_id, 1);
    }

    #[test]
    fn test_remove_addr_build_parse() {
        let built = mptcp_build_remove_addr(5);
        let parsed = mptcp_parse_remove_addr(&built).unwrap();
        assert_eq!(parsed.addr_id, 5);
    }

    #[test]
    fn test_token_generation() {
        let t1 = mptcp_generate_token(0x12345678);
        let t2 = mptcp_generate_token(0x12345678);
        assert_eq!(t1, t2);
        let t3 = mptcp_generate_token(0x87654321);
        assert_ne!(t1, t3);
    }

    #[test]
    fn test_dsn_registration() {
        let mut conn = mptcp_connection_new(100);
        let dsn0 = mptcp_register_dsn(&mut conn, 0, 100);
        let dsn1 = mptcp_register_dsn(&mut conn, 0, 101);
        assert_eq!(dsn0, 0);
        assert_eq!(dsn1, 1);
        let found = mptcp_subflow_seq_to_data_seq(&conn, 0, 100);
        assert_eq!(found, Some(0));
    }

    #[test]
    fn test_rto_calculation() {
        let rto = mptcp_calc_rto(100, 25);
        assert_eq!(rto, 200);
        let rto2 = mptcp_calc_rto(5000, 500);
        assert_eq!(rto2, 7000);
    }

    #[test]
    fn test_rtt_update() {
        let mut sf = MptcpSubflow::new(0, test_ip(10, 0, 0, 1), test_ip(10, 0, 0, 2), 1000, 5000);
        mptcp_update_rtt(&mut sf, 100);
        assert_eq!(sf.srtt, 100);
        mptcp_update_rtt(&mut sf, 120);
        assert!(sf.srtt > 0);
        assert!(sf.rttvar > 0);
        assert!(sf.rto >= 200);
    }

    #[test]
    fn test_congestion_avoidance() {
        let mut sf = MptcpSubflow::new(0, test_ip(10, 0, 0, 1), test_ip(10, 0, 0, 2), 1000, 5000);
        sf.cwnd = 5;
        sf.ssthresh = 20;
        mptcp_congestion_avoidance(&mut sf);
        assert_eq!(sf.cwnd, 6);
        sf.cwnd = 25;
        sf.ssthresh = 20;
        mptcp_congestion_avoidance(&mut sf);
        assert_eq!(sf.cwnd, 26);
    }

    #[test]
    fn test_fast_retransmit() {
        let mut sf = MptcpSubflow::new(0, test_ip(10, 0, 0, 1), test_ip(10, 0, 0, 2), 1000, 5000);
        sf.cwnd = 30;
        mptcp_fast_retransmit(&mut sf);
        assert_eq!(sf.ssthresh, 15);
        assert_eq!(sf.cwnd, 18);
        assert_eq!(sf.retransmits, 1);
    }

    #[test]
    fn test_subflow_flight_size() {
        let mut sf = MptcpSubflow::new(0, test_ip(10, 0, 0, 1), test_ip(10, 0, 0, 2), 1000, 5000);
        sf.snd_una = 1000;
        sf.snd_nxt = 5000;
        sf.cwnd = 10;
        sf.mss = 1460;
        let flight = sf.flight_size();
        assert!(flight <= sf.congestion_window_bytes());
    }

    #[test]
    fn test_subflow_stats() {
        let mut conn = mptcp_connection_new(100);
        let id = mptcp_add_subflow(
            &mut conn,
            test_ip(10, 0, 0, 1),
            test_ip(10, 0, 0, 2),
        )
        .unwrap();
        conn.find_subflow_mut(id).unwrap().bytes_sent = 1024;
        conn.find_subflow_mut(id).unwrap().bytes_received = 2048;
        assert_eq!(conn.total_bytes_sent(), 1024);
        assert_eq!(conn.total_bytes_received(), 2048);
    }

    #[test]
    fn test_mp_capable_parse_invalid_kind() {
        let mut data = [0u8; 12];
        data[0] = 0xFF;
        assert!(mptcp_parse_mp_capable(&data).is_none());
    }

    #[test]
    fn test_mp_join_parse_invalid_kind() {
        let mut data = [0u8; 16];
        data[0] = 0xFF;
        assert!(mptcp_parse_mp_join(&data).is_none());
    }

    #[test]
    fn test_remove_addr_parse_invalid_kind() {
        let mut data = [0u8; 4];
        data[0] = 0xFF;
        assert!(mptcp_parse_remove_addr(&data).is_none());
    }

    #[test]
    fn test_mp_capable_short_data() {
        assert!(mptcp_parse_mp_capable(&[0; 8]).is_none());
    }

    #[test]
    fn test_mp_join_short_data() {
        assert!(mptcp_parse_mp_join(&[0; 8]).is_none());
    }

    #[test]
    fn test_version_from_u8() {
        assert_eq!(MptcpVersion::from_u8(0), Some(MptcpVersion::V0));
        assert_eq!(MptcpVersion::from_u8(1), Some(MptcpVersion::V1));
        assert_eq!(MptcpVersion::from_u8(2), None);
    }
}
