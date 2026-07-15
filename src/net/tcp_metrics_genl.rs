use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::vec;
use core::mem::size_of;
use spin::Mutex;

use crate::net::netlink::{NlMsgHdr, NlAttr, NETLINK_MANAGER};

pub const TCP_METRICS_GENL_ID: u16 = 44;
pub const TCP_METRICS_GENL_VERSION: u8 = 1;

pub const TCP_METRICS_CMD_UNSPEC: u8 = 0;
pub const TCP_METRICS_CMD_GET: u8 = 1;
pub const TCP_METRICS_CMD_DEL: u8 = 2;

pub const TCP_METRICS_ATTR_ADDR_IPV4: u16 = 1;
pub const TCP_METRICS_ATTR_ADDR_IPV6: u16 = 2;
pub const TCP_METRICS_ATTR_AGE: u16 = 3;
pub const TCP_METRICS_ATTR_TW_TSVAL: u16 = 4;
pub const TCP_METRICS_ATTR_TW_TS_STAMP: u16 = 5;
pub const TCP_METRICS_ATTR_VALS: u16 = 6;
pub const TCP_METRICS_ATTR_FOPEN_MSS: u16 = 7;
pub const TCP_METRICS_ATTR_FOPEN_SYN_DROPS: u16 = 8;
pub const TCP_METRICS_ATTR_FOPEN_SYN_DROP_TS: u16 = 9;
pub const TCP_METRICS_ATTR_FOPEN_COOKIE: u16 = 10;
pub const TCP_METRICS_ATTR_SADDR_IPV4: u16 = 11;
pub const TCP_METRICS_ATTR_SADDR_IPV6: u16 = 12;
pub const TCP_METRICS_ATTR_PAD: u16 = 13;

pub const TCP_METRICS_ATTR_RTT: u16 = 1;
pub const TCP_METRICS_ATTR_RTTVAR: u16 = 2;
pub const TCP_METRICS_ATTR_SSTHRESH: u16 = 3;
pub const TCP_METRICS_ATTR_CWND: u16 = 4;
pub const TCP_METRICS_ATTR_REORDERING: u16 = 5;
pub const TCP_METRICS_ATTR_RTT_US: u16 = 6;
pub const TCP_METRICS_ATTR_RTTVAR_US: u16 = 7;

#[derive(Clone, Debug)]
pub struct TcpMetricsEntry {
    pub saddr: u32,
    pub daddr: u32,
    pub rtt: u32,
    pub rttvar: u32,
    pub ssthresh: u32,
    pub cwnd: u32,
    pub reordering: u32,
    pub age_ticks: u64,
    pub rtt_us: u32,
    pub rttvar_us: u32,
}

impl Default for TcpMetricsEntry {
    fn default() -> Self {
        TcpMetricsEntry {
            saddr: 0,
            daddr: 0,
            rtt: 0,
            rttvar: 0,
            ssthresh: 65535,
            cwnd: 14600,
            reordering: 3,
            age_ticks: 0,
            rtt_us: 0,
            rttvar_us: 0,
        }
    }
}

type MetricsKey = (u32, u32);

lazy_static::lazy_static! {
    static ref TCP_METRICS_REGISTRY: Arc<Mutex<BTreeMap<MetricsKey, TcpMetricsEntry>>> = {
        Arc::new(Mutex::new(BTreeMap::new()))
    };
}

pub fn record_metrics(
    saddr: u32,
    daddr: u32,
    rtt: u32,
    rttvar: u32,
    ssthresh: u32,
    cwnd: u32,
) {
    let mut reg = TCP_METRICS_REGISTRY.lock();
    let entry = reg.entry((saddr, daddr)).or_insert_with(|| {
        let mut e = TcpMetricsEntry::default();
        e.saddr = saddr;
        e.daddr = daddr;
        e
    });
    entry.rtt = rtt;
    entry.rttvar = rttvar;
    entry.ssthresh = ssthresh;
    entry.cwnd = cwnd;
    entry.rtt_us = rtt * 1000;
    entry.rttvar_us = rttvar * 1000;
    entry.age_ticks = 0;
}

pub fn delete_metrics(saddr: u32, daddr: u32) {
    let mut reg = TCP_METRICS_REGISTRY.lock();
    reg.remove(&(saddr, daddr));
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
struct GenlMsgHdr {
    cmd: u8,
    version: u8,
    reserved: u16,
}

impl GenlMsgHdr {
    fn new(cmd: u8, version: u8) -> Self {
        GenlMsgHdr { cmd, version, reserved: 0 }
    }

    fn as_bytes(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(
                self as *const GenlMsgHdr as *const u8,
                size_of::<GenlMsgHdr>(),
            )
        }
    }
}

fn find_attr_u32(payload: &[u8], attr_type: u16) -> Option<u32> {
    let mut pos = 0;
    while pos + 4 <= payload.len() {
        let len = u16::from_le_bytes([payload[pos], payload[pos + 1]]) as usize;
        let typ = u16::from_le_bytes([payload[pos + 2], payload[pos + 3]]);
        if len < 4 { break; }
        let data_start = pos + 4;
        let data_end = pos + len;
        if typ == attr_type && data_end <= payload.len() && data_end - data_start >= 4 {
            return Some(u32::from_ne_bytes([
                payload[data_start],
                payload[data_start + 1],
                payload[data_start + 2],
                payload[data_start + 3],
            ]));
        }
        if len == 0 { break; }
        pos += len;
    }
    None
}

fn build_metrics_nested(entry: &TcpMetricsEntry) -> Vec<u8> {
    let mut nested = Vec::new();
    nested.extend_from_slice(&NlAttr::new(TCP_METRICS_ATTR_RTT, &entry.rtt.to_ne_bytes()));
    nested.extend_from_slice(&NlAttr::new(TCP_METRICS_ATTR_RTTVAR, &entry.rttvar.to_ne_bytes()));
    nested.extend_from_slice(&NlAttr::new(TCP_METRICS_ATTR_SSTHRESH, &entry.ssthresh.to_ne_bytes()));
    nested.extend_from_slice(&NlAttr::new(TCP_METRICS_ATTR_CWND, &entry.cwnd.to_ne_bytes()));
    nested.extend_from_slice(&NlAttr::new(TCP_METRICS_ATTR_REORDERING, &entry.reordering.to_ne_bytes()));
    nested.extend_from_slice(&NlAttr::new(TCP_METRICS_ATTR_RTT_US, &entry.rtt_us.to_ne_bytes()));
    nested.extend_from_slice(&NlAttr::new(TCP_METRICS_ATTR_RTTVAR_US, &entry.rttvar_us.to_ne_bytes()));
    nested
}

fn build_entry_payload(entry: &TcpMetricsEntry) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&NlAttr::new(
        TCP_METRICS_ATTR_ADDR_IPV4,
        &entry.daddr.to_be_bytes(),
    ));
    payload.extend_from_slice(&NlAttr::new(
        TCP_METRICS_ATTR_SADDR_IPV4,
        &entry.saddr.to_be_bytes(),
    ));
    let age_bytes = entry.age_ticks.to_ne_bytes();
    payload.extend_from_slice(&NlAttr::new(TCP_METRICS_ATTR_AGE, &age_bytes));
    let vals = build_metrics_nested(entry);
    payload.extend_from_slice(&NlAttr::new(TCP_METRICS_ATTR_VALS, &vals));
    payload
}

fn handle_get(attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let daddr = find_attr_u32(attr_payload, TCP_METRICS_ATTR_ADDR_IPV4);
    let saddr = find_attr_u32(attr_payload, TCP_METRICS_ATTR_SADDR_IPV4);

    if let Some(da) = daddr {
        let sa = saddr.unwrap_or(0);
        let reg = TCP_METRICS_REGISTRY.lock();
        if let Some(entry) = reg.get(&(sa, da)) {
            let mut entry_clone = entry.clone();
            entry_clone.age_ticks += 1;
            return vec![(TCP_METRICS_CMD_GET, build_entry_payload(&entry_clone))];
        }
    }
    vec![]
}

fn handle_get_dump(_attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let reg = TCP_METRICS_REGISTRY.lock();
    reg.values().map(|entry| {
        let mut e = entry.clone();
        e.age_ticks += 1;
        (TCP_METRICS_CMD_GET, build_entry_payload(&e))
    }).collect()
}

fn handle_del(attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let daddr = find_attr_u32(attr_payload, TCP_METRICS_ATTR_ADDR_IPV4);
    let saddr = find_attr_u32(attr_payload, TCP_METRICS_ATTR_SADDR_IPV4);
    if let Some(da) = daddr {
        let sa = saddr.unwrap_or(0);
        delete_metrics(sa, da);
    }
    vec![]
}

pub fn handle_tcp_metrics_genl_request(
    src_pid: u32,
    seq: u32,
    payload: &[u8],
) {
    if payload.len() < 4 {
        return;
    }

    let hdr = unsafe { &*(payload.as_ptr() as *const GenlMsgHdr) };
    let cmd = hdr.cmd;
    let attr_payload = &payload[4..];

    let cmd_responses = match cmd {
        TCP_METRICS_CMD_GET => handle_get(attr_payload),
        TCP_METRICS_CMD_DEL => handle_del(attr_payload),
        _ => return,
    };

    let mut all_responses: Vec<(u8, Vec<u8>)> = cmd_responses;
    all_responses.push((0, Vec::new()));

    for (resp_cmd, resp_payload) in &all_responses {
        let mut inner = Vec::new();
        let ghdr = GenlMsgHdr::new(*resp_cmd, TCP_METRICS_GENL_VERSION);
        inner.extend_from_slice(ghdr.as_bytes());
        inner.extend_from_slice(resp_payload);

        let is_done = resp_payload.is_empty() && *resp_cmd == 0;
        let msg_type = if is_done { 3u16 } else { TCP_METRICS_GENL_ID };

        let total_len = (size_of::<NlMsgHdr>() + inner.len()) as u32;
        let reply_hdr = NlMsgHdr::new(
            total_len,
            msg_type,
            if is_done { 0 } else { 2u16 },
            seq,
            0,
        );
        let reply_msg = crate::net::netlink::NetlinkMessage {
            header: reply_hdr,
            payload: inner,
        };

        if src_pid != 0 {
            if let Some(sock) = NETLINK_MANAGER.get_socket(src_pid) {
                sock.rx_buf.lock().push(reply_msg);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_genlmsghdr_size() {
        assert_eq!(size_of::<GenlMsgHdr>(), 4);
    }

    #[test]
    fn test_constants() {
        assert_eq!(TCP_METRICS_GENL_ID, 44);
        assert_eq!(TCP_METRICS_CMD_GET, 1);
        assert_eq!(TCP_METRICS_CMD_DEL, 2);
        assert_eq!(TCP_METRICS_ATTR_ADDR_IPV4, 1);
        assert_eq!(TCP_METRICS_ATTR_VALS, 6);
        assert_eq!(TCP_METRICS_ATTR_SADDR_IPV4, 11);
        assert_eq!(TCP_METRICS_ATTR_RTT, 1);
        assert_eq!(TCP_METRICS_ATTR_CWND, 4);
    }

    #[test]
    fn test_find_attr_u32() {
        let mut data = Vec::new();
        data.extend_from_slice(&NlAttr::new(1, &42u32.to_ne_bytes()));
        data.extend_from_slice(&NlAttr::new(2, &99u32.to_ne_bytes()));
        assert_eq!(find_attr_u32(&data, 1), Some(42));
        assert_eq!(find_attr_u32(&data, 2), Some(99));
        assert_eq!(find_attr_u32(&data, 3), None);
    }

    #[test]
    fn test_record_and_get() {
        record_metrics(0x0100007F, 0xC0A80101, 25, 10, 65535, 29200);
        let reg = TCP_METRICS_REGISTRY.lock();
        let entry = reg.get(&(0x0100007F, 0xC0A80101));
        assert!(entry.is_some());
        let e = entry.unwrap();
        assert_eq!(e.rtt, 25);
        assert_eq!(e.rttvar, 10);
        assert_eq!(e.ssthresh, 65535);
        assert_eq!(e.cwnd, 29200);
        assert_eq!(e.rtt_us, 25000);
        assert_eq!(e.rttvar_us, 10000);
        drop(reg);
        delete_metrics(0x0100007F, 0xC0A80101);
        assert!(TCP_METRICS_REGISTRY.lock().get(&(0x0100007F, 0xC0A80101)).is_none());
    }

    #[test]
    fn test_build_entry_payload() {
        let entry = TcpMetricsEntry {
            saddr: 0x0100007F,
            daddr: 0xC0A80101,
            rtt: 25,
            rttvar: 10,
            ssthresh: 65535,
            cwnd: 29200,
            reordering: 3,
            age_ticks: 100,
            rtt_us: 25000,
            rttvar_us: 10000,
        };
        let payload = build_entry_payload(&entry);
        assert!(!payload.is_empty());
        assert!(find_attr_u32(&payload, TCP_METRICS_ATTR_ADDR_IPV4).is_some());
        assert!(find_attr_u32(&payload, TCP_METRICS_ATTR_SADDR_IPV4).is_some());
    }

    #[test]
    fn test_handle_get_missing() {
        let result = handle_get(&[0u8; 0]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_handle_del_missing() {
        let result = handle_del(&[0u8; 0]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_handle_get_dump_empty() {
        let result = handle_get_dump(&[0u8; 0]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_handle_get_dump_with_entry() {
        record_metrics(0x0100007F, 0xC0A80101, 25, 10, 65535, 29200);
        let result = handle_get_dump(&[0u8; 0]);
        assert!(!result.is_empty());
        assert_eq!(result[0].0, TCP_METRICS_CMD_GET);
        let payload = &result[0].1;
        assert!(find_attr_u32(payload, TCP_METRICS_ATTR_ADDR_IPV4) == Some(0xC0A80101));
        assert!(find_attr_u32(payload, TCP_METRICS_ATTR_SADDR_IPV4) == Some(0x0100007F));
        delete_metrics(0x0100007F, 0xC0A80101);
    }
}
