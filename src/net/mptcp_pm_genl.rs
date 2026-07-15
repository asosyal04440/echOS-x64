use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::vec;
use core::mem::size_of;
use core::sync::atomic::AtomicU32;
use core::sync::atomic::Ordering;
use spin::Mutex;
use lazy_static::lazy_static;

use crate::net::netlink::{NlMsgHdr, NlAttr, NETLINK_MANAGER};
use crate::net::mptcp::{mptcp_connection_new, mptcp_add_subflow, mptcp_remove_subflow};
use crate::net::{IpAddr, Ipv4Addr};

pub const MPTCP_PM_GENL_ID: u16 = 46;
pub const MPTCP_PM_GENL_VERSION: u8 = 1;

pub const MPTCP_PM_CMD_UNSPEC: u8 = 0;
pub const MPTCP_PM_CMD_ADD_ADDR: u8 = 1;
pub const MPTCP_PM_CMD_DEL_ADDR: u8 = 2;
pub const MPTCP_PM_CMD_GET_ADDR: u8 = 3;
pub const MPTCP_PM_CMD_FLUSH_ADDRS: u8 = 4;
pub const MPTCP_PM_CMD_SET_LIMITS: u8 = 5;
pub const MPTCP_PM_CMD_GET_LIMITS: u8 = 6;
pub const MPTCP_PM_CMD_SET_FLAGS: u8 = 7;
pub const MPTCP_PM_CMD_ANNOUNCE: u8 = 8;
pub const MPTCP_PM_CMD_REMOVE: u8 = 9;
pub const MPTCP_PM_CMD_SUBFLOW_CREATE: u8 = 10;
pub const MPTCP_PM_CMD_SUBFLOW_DESTROY: u8 = 11;

pub const MPTCP_PM_ADDR_ATTR_UNSPEC: u16 = 0;
pub const MPTCP_PM_ADDR_ATTR_FAMILY: u16 = 1;
pub const MPTCP_PM_ADDR_ATTR_ID: u16 = 2;
pub const MPTCP_PM_ADDR_ATTR_ADDR4: u16 = 3;
pub const MPTCP_PM_ADDR_ATTR_ADDR6: u16 = 4;
pub const MPTCP_PM_ADDR_ATTR_PORT: u16 = 5;
pub const MPTCP_PM_ADDR_ATTR_FLAGS: u16 = 6;
pub const MPTCP_PM_ADDR_ATTR_IF_IDX: u16 = 7;

pub const MPTCP_PM_ATTR_UNSPEC: u16 = 0;
pub const MPTCP_PM_ATTR_ADDR: u16 = 1;
pub const MPTCP_PM_ATTR_RCV_ADD_ADDRS: u16 = 2;
pub const MPTCP_PM_ATTR_SUBFLOWS: u16 = 3;
pub const MPTCP_PM_ATTR_TOKEN: u16 = 4;
pub const MPTCP_PM_ATTR_LOC_ID: u16 = 5;
pub const MPTCP_PM_ATTR_ADDR_REMOTE: u16 = 6;

pub const MPTCP_PM_EVENT_ATTR_UNSPEC: u16 = 0;
pub const MPTCP_PM_EVENT_ATTR_TOKEN: u16 = 1;
pub const MPTCP_PM_EVENT_ATTR_FAMILY: u16 = 2;
pub const MPTCP_PM_EVENT_ATTR_LOC_ID: u16 = 3;
pub const MPTCP_PM_EVENT_ATTR_REM_ID: u16 = 4;
pub const MPTCP_PM_EVENT_ATTR_SADDR4: u16 = 5;
pub const MPTCP_PM_EVENT_ATTR_SADDR6: u16 = 6;
pub const MPTCP_PM_EVENT_ATTR_DADDR4: u16 = 7;
pub const MPTCP_PM_EVENT_ATTR_DADDR6: u16 = 8;
pub const MPTCP_PM_EVENT_ATTR_SPORT: u16 = 9;
pub const MPTCP_PM_EVENT_ATTR_DPORT: u16 = 10;
pub const MPTCP_PM_EVENT_ATTR_BACKUP: u16 = 11;
pub const MPTCP_PM_EVENT_ATTR_ERROR: u16 = 12;
pub const MPTCP_PM_EVENT_ATTR_FLAGS: u16 = 13;
pub const MPTCP_PM_EVENT_ATTR_TIMEOUT: u16 = 14;
pub const MPTCP_PM_EVENT_ATTR_IF_IDX: u16 = 15;
pub const MPTCP_PM_EVENT_ATTR_RESET_REASON: u16 = 16;
pub const MPTCP_PM_EVENT_ATTR_RESET_FLAGS: u16 = 17;
pub const MPTCP_PM_EVENT_ATTR_SERVER_SIDE: u16 = 18;

pub const MPTCP_PM_ADDR_FLAG_SIGNAL: u32 = 1;
pub const MPTCP_PM_ADDR_FLAG_SUBFLOW: u32 = 2;
pub const MPTCP_PM_ADDR_FLAG_BACKUP: u32 = 4;
pub const MPTCP_PM_ADDR_FLAG_REMOVE_ME: u32 = 8;

const AF_INET: u16 = 2;
const AF_INET6: u16 = 10;

#[derive(Clone, Debug)]
struct MptcpEndpoint {
    id: u8,
    family: u16,
    addr4: u32,
    addr6: [u8; 16],
    port: u16,
    flags: u32,
    if_idx: i32,
}

#[derive(Clone, Debug)]
struct MptcpLimits {
    rcv_add_addrs: u32,
    subflows: u32,
}

lazy_static! {
    static ref MPTCP_ENDPOINTS: Arc<Mutex<BTreeMap<u8, MptcpEndpoint>>> = {
        Arc::new(Mutex::new(BTreeMap::new()))
    };
    static ref MPTCP_LIMITS: Arc<Mutex<MptcpLimits>> = {
        Arc::new(Mutex::new(MptcpLimits { rcv_add_addrs: 0, subflows: 0 }))
    };
    static ref NEXT_ENDPOINT_ID: AtomicU32 = AtomicU32::new(1);
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
                payload[data_start], payload[data_start + 1],
                payload[data_start + 2], payload[data_start + 3],
            ]));
        }
        if len == 0 { break; }
        pos += len;
    }
    None
}

fn find_attr_u16(payload: &[u8], attr_type: u16) -> Option<u16> {
    let mut pos = 0;
    while pos + 4 <= payload.len() {
        let len = u16::from_le_bytes([payload[pos], payload[pos + 1]]) as usize;
        let typ = u16::from_le_bytes([payload[pos + 2], payload[pos + 3]]);
        if len < 4 { break; }
        let data_start = pos + 4;
        let data_end = pos + len;
        if typ == attr_type && data_end <= payload.len() && data_end - data_start >= 2 {
            return Some(u16::from_ne_bytes([payload[data_start], payload[data_start + 1]]));
        }
        if len == 0 { break; }
        pos += len;
    }
    None
}

fn find_attr_u8(payload: &[u8], attr_type: u16) -> Option<u8> {
    let mut pos = 0;
    while pos + 4 <= payload.len() {
        let len = u16::from_le_bytes([payload[pos], payload[pos + 1]]) as usize;
        let typ = u16::from_le_bytes([payload[pos + 2], payload[pos + 3]]);
        if len < 4 { break; }
        let data_start = pos + 4;
        let data_end = pos + len;
        if typ == attr_type && data_end <= payload.len() && data_end >= data_start + 1 {
            return Some(payload[data_start]);
        }
        if len == 0 { break; }
        pos += len;
    }
    None
}

fn find_attr_binary<'a>(payload: &'a [u8], attr_type: u16) -> Option<&'a [u8]> {
    let mut pos = 0;
    while pos + 4 <= payload.len() {
        let len = u16::from_le_bytes([payload[pos], payload[pos + 1]]) as usize;
        let typ = u16::from_le_bytes([payload[pos + 2], payload[pos + 3]]);
        if len < 4 { break; }
        let data_start = pos + 4;
        let data_end = pos + len;
        if typ == attr_type && data_end <= payload.len() {
            return Some(&payload[data_start..data_end]);
        }
        if len == 0 { break; }
        pos += len;
    }
    None
}

fn parse_addr_nested(inner: &[u8]) -> Option<MptcpEndpoint> {
    let family = find_attr_u16(inner, MPTCP_PM_ADDR_ATTR_FAMILY).unwrap_or(AF_INET);
    let id = find_attr_u8(inner, MPTCP_PM_ADDR_ATTR_ID).unwrap_or(0);
    let addr4 = find_attr_u32(inner, MPTCP_PM_ADDR_ATTR_ADDR4).unwrap_or(0);
    let addr6_bytes = find_attr_binary(inner, MPTCP_PM_ADDR_ATTR_ADDR6);
    let mut addr6 = [0u8; 16];
    if let Some(b) = addr6_bytes {
        let copy_len = b.len().min(16);
        addr6[..copy_len].copy_from_slice(&b[..copy_len]);
    }
    let port = find_attr_u16(inner, MPTCP_PM_ADDR_ATTR_PORT).unwrap_or(0);
    let flags = find_attr_u32(inner, MPTCP_PM_ADDR_ATTR_FLAGS).unwrap_or(0);
    let if_idx = find_attr_u32(inner, MPTCP_PM_ADDR_ATTR_IF_IDX).map(|v| v as i32).unwrap_or(0);
    Some(MptcpEndpoint { id, family, addr4, addr6, port, flags, if_idx })
}

fn build_addr_payload(ep: &MptcpEndpoint) -> Vec<u8> {
    let mut inner = Vec::new();
    inner.extend_from_slice(&NlAttr::new(MPTCP_PM_ADDR_ATTR_FAMILY, &ep.family.to_ne_bytes()));
    inner.extend_from_slice(&NlAttr::new(MPTCP_PM_ADDR_ATTR_ID, &[ep.id]));
    if ep.family == AF_INET {
        inner.extend_from_slice(&NlAttr::new(MPTCP_PM_ADDR_ATTR_ADDR4, &ep.addr4.to_be_bytes()));
    } else {
        inner.extend_from_slice(&NlAttr::new(MPTCP_PM_ADDR_ATTR_ADDR6, &ep.addr6));
    }
    if ep.port != 0 {
        inner.extend_from_slice(&NlAttr::new(MPTCP_PM_ADDR_ATTR_PORT, &ep.port.to_ne_bytes()));
    }
    inner.extend_from_slice(&NlAttr::new(MPTCP_PM_ADDR_ATTR_FLAGS, &ep.flags.to_ne_bytes()));
    inner.extend_from_slice(&NlAttr::new(MPTCP_PM_ADDR_ATTR_IF_IDX, &(ep.if_idx as u32).to_ne_bytes()));
    inner
}

fn handle_add_addr(attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let addr_nested = find_attr_binary(attr_payload, MPTCP_PM_ATTR_ADDR);
    if let Some(nested) = addr_nested {
        if let Some(mut ep) = parse_addr_nested(nested) {
            if ep.id == 0 {
                ep.id = NEXT_ENDPOINT_ID.fetch_add(1, Ordering::Relaxed) as u8;
            }
            let mut endpoints = MPTCP_ENDPOINTS.lock();
            if !endpoints.contains_key(&ep.id) {
                endpoints.insert(ep.id, ep);
            }
        }
    }
    vec![]
}

fn handle_del_addr(attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let addr_nested = find_attr_binary(attr_payload, MPTCP_PM_ATTR_ADDR);
    if let Some(nested) = addr_nested {
        if let Some(ep) = parse_addr_nested(nested) {
            let mut endpoints = MPTCP_ENDPOINTS.lock();
            if ep.id != 0 {
                endpoints.remove(&ep.id);
            }
        }
    }
    vec![]
}

fn handle_get_addr(attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let _token = find_attr_u32(attr_payload, MPTCP_PM_ATTR_TOKEN);
    let addr_nested = find_attr_binary(attr_payload, MPTCP_PM_ATTR_ADDR);
    let endpoints = MPTCP_ENDPOINTS.lock();

    if let Some(nested) = addr_nested {
        if let Some(ep) = parse_addr_nested(nested) {
            if let Some(found) = endpoints.get(&ep.id) {
                let payload = build_addr_payload(found);
                return vec![(MPTCP_PM_CMD_GET_ADDR, NlAttr::new(MPTCP_PM_ATTR_ADDR, &payload))];
            }
        }
    }
    vec![]
}

fn handle_get_addr_dump(_attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let endpoints = MPTCP_ENDPOINTS.lock();
    endpoints.values().map(|ep| {
        let payload = build_addr_payload(ep);
        (MPTCP_PM_CMD_GET_ADDR, NlAttr::new(MPTCP_PM_ATTR_ADDR, &payload))
    }).collect()
}

fn handle_flush_addrs(_attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    MPTCP_ENDPOINTS.lock().clear();
    vec![]
}

fn handle_set_limits(attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let rcv = find_attr_u32(attr_payload, MPTCP_PM_ATTR_RCV_ADD_ADDRS);
    let subflows = find_attr_u32(attr_payload, MPTCP_PM_ATTR_SUBFLOWS);
    let mut limits = MPTCP_LIMITS.lock();
    if let Some(v) = rcv { limits.rcv_add_addrs = v; }
    if let Some(v) = subflows { limits.subflows = v; }
    vec![]
}

fn handle_get_limits(_attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let limits = MPTCP_LIMITS.lock();
    let mut payload = Vec::new();
    payload.extend_from_slice(&NlAttr::new(MPTCP_PM_ATTR_RCV_ADD_ADDRS, &limits.rcv_add_addrs.to_ne_bytes()));
    payload.extend_from_slice(&NlAttr::new(MPTCP_PM_ATTR_SUBFLOWS, &limits.subflows.to_ne_bytes()));
    vec![(MPTCP_PM_CMD_GET_LIMITS, payload)]
}

fn handle_set_flags(attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let token = find_attr_u32(attr_payload, MPTCP_PM_ATTR_TOKEN);
    let addr_nested = find_attr_binary(attr_payload, MPTCP_PM_ATTR_ADDR);
    let addr_remote_nested = find_attr_binary(attr_payload, MPTCP_PM_ATTR_ADDR_REMOTE);

    if let Some(nested) = addr_nested {
        if let Some(ep) = parse_addr_nested(nested) {
            let mut endpoints = MPTCP_ENDPOINTS.lock();
            if let Some(existing) = endpoints.get_mut(&ep.id) {
                if ep.flags != 0 { existing.flags = ep.flags; }
                if ep.port != 0 { existing.port = ep.port; }
                if ep.if_idx != 0 { existing.if_idx = ep.if_idx; }
            }
        }
    }
    vec![]
}

fn handle_announce(attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let token = find_attr_u32(attr_payload, MPTCP_PM_ATTR_TOKEN);
    let addr_nested = find_attr_binary(attr_payload, MPTCP_PM_ATTR_ADDR);
    if let Some(nested) = addr_nested {
        if let Some(ep) = parse_addr_nested(nested) {
            crate::serial_println!("[MPTCP_PM] Announce addr token={:#x} id={}", token.unwrap_or(0), ep.id);
        }
    }
    vec![]
}

fn handle_remove(attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let token = find_attr_u32(attr_payload, MPTCP_PM_ATTR_TOKEN);
    let loc_id = find_attr_u8(attr_payload, MPTCP_PM_ATTR_LOC_ID);
    if let Some(id) = loc_id {
        let mut endpoints = MPTCP_ENDPOINTS.lock();
        endpoints.remove(&id);
    }
    vec![]
}

fn handle_subflow_create(attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let token = find_attr_u32(attr_payload, MPTCP_PM_ATTR_TOKEN);
    let addr_nested = find_attr_binary(attr_payload, MPTCP_PM_ATTR_ADDR);
    let addr_remote_nested = find_attr_binary(attr_payload, MPTCP_PM_ATTR_ADDR_REMOTE);

    if let (Some(local_nested), Some(remote_nested)) = (addr_nested, addr_remote_nested) {
        if let (Some(local), Some(remote)) = (parse_addr_nested(local_nested), parse_addr_nested(remote_nested)) {
            let local_ip = if local.family == AF_INET {
                IpAddr::V4(Ipv4Addr::from_u32(local.addr4))
            } else {
                return vec![];
            };
            let remote_ip = if remote.family == AF_INET {
                IpAddr::V4(Ipv4Addr::from_u32(remote.addr4))
            } else {
                return vec![];
            };
            let mut conn = mptcp_connection_new(token.unwrap_or(0));
            let _ = mptcp_add_subflow(&mut conn, local_ip, remote_ip);
        }
    }
    vec![]
}

fn handle_subflow_destroy(_attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    vec![]
}

pub fn handle_mptcp_pm_genl_request(
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
        MPTCP_PM_CMD_ADD_ADDR => handle_add_addr(attr_payload),
        MPTCP_PM_CMD_DEL_ADDR => handle_del_addr(attr_payload),
        MPTCP_PM_CMD_GET_ADDR => {
            let mut responses = handle_get_addr_dump(attr_payload);
            if responses.is_empty() {
                responses = handle_get_addr(attr_payload);
            }
            responses
        }
        MPTCP_PM_CMD_FLUSH_ADDRS => handle_flush_addrs(attr_payload),
        MPTCP_PM_CMD_SET_LIMITS => handle_set_limits(attr_payload),
        MPTCP_PM_CMD_GET_LIMITS => handle_get_limits(attr_payload),
        MPTCP_PM_CMD_SET_FLAGS => handle_set_flags(attr_payload),
        MPTCP_PM_CMD_ANNOUNCE => handle_announce(attr_payload),
        MPTCP_PM_CMD_REMOVE => handle_remove(attr_payload),
        MPTCP_PM_CMD_SUBFLOW_CREATE => handle_subflow_create(attr_payload),
        MPTCP_PM_CMD_SUBFLOW_DESTROY => handle_subflow_destroy(attr_payload),
        _ => return,
    };

    let mut all_responses: Vec<(u8, Vec<u8>)> = cmd_responses;
    all_responses.push((0, Vec::new()));

    for (resp_cmd, resp_payload) in &all_responses {
        let mut inner = Vec::new();
        let ghdr = GenlMsgHdr::new(*resp_cmd, MPTCP_PM_GENL_VERSION);
        inner.extend_from_slice(ghdr.as_bytes());
        inner.extend_from_slice(resp_payload);

        let is_done = resp_payload.is_empty() && *resp_cmd == 0;
        let msg_type = if is_done { 3u16 } else { MPTCP_PM_GENL_ID };

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
    fn test_mptcp_pm_constants() {
        assert_eq!(MPTCP_PM_GENL_ID, 46);
        assert_eq!(MPTCP_PM_CMD_ADD_ADDR, 1);
        assert_eq!(MPTCP_PM_CMD_DEL_ADDR, 2);
        assert_eq!(MPTCP_PM_CMD_GET_ADDR, 3);
        assert_eq!(MPTCP_PM_CMD_FLUSH_ADDRS, 4);
        assert_eq!(MPTCP_PM_CMD_SET_LIMITS, 5);
        assert_eq!(MPTCP_PM_CMD_GET_LIMITS, 6);
        assert_eq!(MPTCP_PM_CMD_SET_FLAGS, 7);
        assert_eq!(MPTCP_PM_CMD_ANNOUNCE, 8);
        assert_eq!(MPTCP_PM_CMD_REMOVE, 9);
        assert_eq!(MPTCP_PM_CMD_SUBFLOW_CREATE, 10);
        assert_eq!(MPTCP_PM_CMD_SUBFLOW_DESTROY, 11);

        assert_eq!(MPTCP_PM_ADDR_ATTR_FAMILY, 1);
        assert_eq!(MPTCP_PM_ADDR_ATTR_ID, 2);
        assert_eq!(MPTCP_PM_ADDR_ATTR_ADDR4, 3);
        assert_eq!(MPTCP_PM_ADDR_ATTR_ADDR6, 4);
        assert_eq!(MPTCP_PM_ADDR_ATTR_PORT, 5);
        assert_eq!(MPTCP_PM_ADDR_ATTR_FLAGS, 6);
        assert_eq!(MPTCP_PM_ADDR_ATTR_IF_IDX, 7);

        assert_eq!(MPTCP_PM_ATTR_ADDR, 1);
        assert_eq!(MPTCP_PM_ATTR_RCV_ADD_ADDRS, 2);
        assert_eq!(MPTCP_PM_ATTR_SUBFLOWS, 3);
        assert_eq!(MPTCP_PM_ATTR_TOKEN, 4);
        assert_eq!(MPTCP_PM_ATTR_LOC_ID, 5);
        assert_eq!(MPTCP_PM_ATTR_ADDR_REMOTE, 6);
    }
}
