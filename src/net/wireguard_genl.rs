use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::vec;
use alloc::string::String;
use alloc::string::ToString;
use core::mem::size_of;
use core::sync::atomic::Ordering;
use spin::Mutex;

use crate::net::netlink::{NlMsgHdr, NlAttr, NETLINK_MANAGER};
use crate::net::wireguard::{WG_MANAGER, WgDevice, WgKey, WgPeer, WG_KEY_SIZE};

pub const WIREGUARD_GENL_ID: u16 = 45;
pub const WIREGUARD_GENL_VERSION: u8 = 1;

pub const WG_CMD_GET_DEVICE: u8 = 0;
pub const WG_CMD_SET_DEVICE: u8 = 1;

pub const WGDEVICE_A_IFINDEX: u16 = 1;
pub const WGDEVICE_A_IFNAME: u16 = 2;
pub const WGDEVICE_A_PRIVATE_KEY: u16 = 3;
pub const WGDEVICE_A_PUBLIC_KEY: u16 = 4;
pub const WGDEVICE_A_FLAGS: u16 = 5;
pub const WGDEVICE_A_LISTEN_PORT: u16 = 6;
pub const WGDEVICE_A_FWMARK: u16 = 7;
pub const WGDEVICE_A_PEERS: u16 = 8;

pub const WGPEER_A_PUBLIC_KEY: u16 = 1;
pub const WGPEER_A_PRESHARED_KEY: u16 = 2;
pub const WGPEER_A_FLAGS: u16 = 3;
pub const WGPEER_A_ENDPOINT: u16 = 4;
pub const WGPEER_A_PERSISTENT_KEEPALIVE_INTERVAL: u16 = 5;
pub const WGPEER_A_LAST_HANDSHAKE_TIME: u16 = 6;
pub const WGPEER_A_RX_BYTES: u16 = 7;
pub const WGPEER_A_TX_BYTES: u16 = 8;
pub const WGPEER_A_ALLOWEDIPS: u16 = 9;
pub const WGPEER_A_PROTOCOL_VERSION: u16 = 10;

pub const WGALLOWEDIP_A_FAMILY: u16 = 1;
pub const WGALLOWEDIP_A_IPADDR: u16 = 2;
pub const WGALLOWEDIP_A_CIDR_MASK: u16 = 3;
pub const WGALLOWEDIP_A_FLAGS: u16 = 4;

pub const WGDEVICE_F_REPLACE_PEERS: u32 = 1;

pub const WGPEER_F_REMOVE_ME: u32 = 1;
pub const WGPEER_F_REPLACE_ALLOWEDIPS: u32 = 2;
pub const WGPEER_F_UPDATE_ONLY: u32 = 4;

pub const WGALLOWEDIP_F_REMOVE_ME: u32 = 1;

const AF_INET: u16 = 2;

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

fn find_attr_u16(payload: &[u8], attr_type: u16) -> Option<u16> {
    let mut pos = 0;
    while pos + 4 <= payload.len() {
        let len = u16::from_le_bytes([payload[pos], payload[pos + 1]]) as usize;
        let typ = u16::from_le_bytes([payload[pos + 2], payload[pos + 3]]);
        if len < 4 { break; }
        let data_start = pos + 4;
        let data_end = pos + len;
        if typ == attr_type && data_end <= payload.len() && data_end - data_start >= 2 {
            return Some(u16::from_ne_bytes([
                payload[data_start],
                payload[data_start + 1],
            ]));
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

fn find_attr_string<'a>(payload: &'a [u8], attr_type: u16) -> Option<&'a [u8]> {
    find_attr_binary(payload, attr_type)
}

fn parse_nested_attrs<'a>(payload: &'a [u8], attr_type: u16) -> Vec<&'a [u8]> {
    let mut results = Vec::new();
    let mut pos = 0;
    while pos + 4 <= payload.len() {
        let len = u16::from_le_bytes([payload[pos], payload[pos + 1]]) as usize;
        let typ = u16::from_le_bytes([payload[pos + 2], payload[pos + 3]]);
        if len < 4 { break; }
        let data_start = pos + 4;
        let data_end = pos + len;
        if typ == attr_type && data_end <= payload.len() {
            results.push(&payload[data_start..data_end]);
        }
        if len == 0 { break; }
        pos += len;
    }
    results
}

fn find_device_by_ifindex(ifindex: u32) -> Option<Arc<WgDevice>> {
    let devices = WG_MANAGER.devices.lock();
    for dev in devices.values() {
        if dev.ifindex.load(Ordering::Relaxed) == ifindex {
            return Some(dev.clone());
        }
    }
    None
}

fn find_device_by_name(name: &str) -> Option<Arc<WgDevice>> {
    WG_MANAGER.get_device(name)
}

fn build_sockaddr_in(ip: u32, port: u16) -> Vec<u8> {
    let mut buf = Vec::with_capacity(16);
    buf.extend_from_slice(&AF_INET.to_be_bytes());
    buf.extend_from_slice(&port.to_be_bytes());
    buf.extend_from_slice(&ip.to_be_bytes());
    buf.extend_from_slice(&[0u8; 8]);
    buf
}

fn parse_sockaddr_in(data: &[u8]) -> Option<(u32, u16)> {
    if data.len() < 16 { return None; }
    let family = u16::from_be_bytes([data[0], data[1]]);
    if family != AF_INET { return None; }
    let port = u16::from_be_bytes([data[2], data[3]]);
    let ip = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
    Some((ip, port))
}

fn build_allowedip_payload(ip: u32, prefix_len: u8) -> Vec<u8> {
    let mut inner = Vec::new();
    inner.extend_from_slice(&NlAttr::new(WGALLOWEDIP_A_FAMILY, &AF_INET.to_ne_bytes()));
    inner.extend_from_slice(&NlAttr::new(WGALLOWEDIP_A_IPADDR, &ip.to_be_bytes()));
    inner.extend_from_slice(&NlAttr::new(WGALLOWEDIP_A_CIDR_MASK, &[prefix_len]));
    inner.extend_from_slice(&NlAttr::new(WGALLOWEDIP_A_FLAGS, &0u32.to_ne_bytes()));
    inner
}

fn build_peer_payload(peer: &WgPeer, with_allowedips: bool) -> Vec<u8> {
    let mut inner = Vec::new();
    inner.extend_from_slice(&NlAttr::new(WGPEER_A_PUBLIC_KEY, peer.public_key.as_bytes()));
    inner.extend_from_slice(&NlAttr::new(WGPEER_A_PRESHARED_KEY, peer.preshared_key.as_bytes()));
    inner.extend_from_slice(&NlAttr::new(WGPEER_A_FLAGS, &0u32.to_ne_bytes()));

    if peer.endpoint_ip != 0 {
        let endpoint = build_sockaddr_in(peer.endpoint_ip, peer.endpoint_port);
        inner.extend_from_slice(&NlAttr::new(WGPEER_A_ENDPOINT, &endpoint));
    }

    inner.extend_from_slice(&NlAttr::new(
        WGPEER_A_PERSISTENT_KEEPALIVE_INTERVAL,
        &(peer.keepalive.load(Ordering::Relaxed) as u16).to_ne_bytes(),
    ));

    let handshake_sec = peer.last_handshake.load(Ordering::Relaxed);
    let mut timespec = [0u8; 16];
    timespec[..8].copy_from_slice(&handshake_sec.to_ne_bytes());
    inner.extend_from_slice(&NlAttr::new(WGPEER_A_LAST_HANDSHAKE_TIME, &timespec));

    inner.extend_from_slice(&NlAttr::new(
        WGPEER_A_RX_BYTES,
        &peer.rx_bytes.load(Ordering::Relaxed).to_ne_bytes(),
    ));
    inner.extend_from_slice(&NlAttr::new(
        WGPEER_A_TX_BYTES,
        &peer.tx_bytes.load(Ordering::Relaxed).to_ne_bytes(),
    ));

    if with_allowedips {
        let mut allowedips_inner = Vec::new();
        for (ip, prefix_len) in &peer.allowed_ips {
            let aip = build_allowedip_payload(*ip, *prefix_len);
            allowedips_inner.extend_from_slice(&NlAttr::new(0, &aip));
        }
        inner.extend_from_slice(&NlAttr::new(WGPEER_A_ALLOWEDIPS, &allowedips_inner));
    }

    inner
}

fn build_device_payload(device: &WgDevice, with_peers: bool, with_allowedips: bool) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&NlAttr::new(
        WGDEVICE_A_IFINDEX,
        &device.ifindex.load(Ordering::Relaxed).to_ne_bytes(),
    ));
    let name_bytes = device.name.as_bytes();
    payload.extend_from_slice(&NlAttr::new(WGDEVICE_A_IFNAME, name_bytes));
    payload.extend_from_slice(&NlAttr::new(
        WGDEVICE_A_PRIVATE_KEY,
        device.private_key.lock().as_bytes(),
    ));
    payload.extend_from_slice(&NlAttr::new(
        WGDEVICE_A_PUBLIC_KEY,
        device.public_key.lock().as_bytes(),
    ));
    payload.extend_from_slice(&NlAttr::new(
        WGDEVICE_A_FLAGS,
        &0u32.to_ne_bytes(),
    ));
    payload.extend_from_slice(&NlAttr::new(
        WGDEVICE_A_LISTEN_PORT,
        &(device.listen_port.load(Ordering::Relaxed) as u16).to_ne_bytes(),
    ));
    payload.extend_from_slice(&NlAttr::new(
        WGDEVICE_A_FWMARK,
        &device.fwmark.load(Ordering::Relaxed).to_ne_bytes(),
    ));

    if with_peers {
        let peers_attr = build_peers_payload(device, with_allowedips);
        payload.extend_from_slice(&NlAttr::new(WGDEVICE_A_PEERS, &peers_attr));
    }

    payload
}

fn build_peers_payload(device: &WgDevice, with_allowedips: bool) -> Vec<u8> {
    let mut peers_inner = Vec::new();
    let peers = device.peers.lock();
    for peer in peers.values() {
        let p = build_peer_payload(peer.as_ref(), with_allowedips);
        peers_inner.extend_from_slice(&NlAttr::new(0, &p));
    }
    peers_inner
}

fn handle_get_device(attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let ifindex = find_attr_u32(attr_payload, WGDEVICE_A_IFINDEX);
    let ifname = find_attr_string(attr_payload, WGDEVICE_A_IFNAME);

    let device = if let Some(idx) = ifindex {
        find_device_by_ifindex(idx)
    } else if let Some(name_bytes) = ifname {
        let name = core::str::from_utf8(name_bytes).unwrap_or("");
        find_device_by_name(name)
    } else {
        None
    };

    if let Some(dev) = device {
        let payload = build_device_payload(&dev, true, true);
        vec![(WG_CMD_GET_DEVICE, payload)]
    } else {
        vec![]
    }
}

fn handle_get_device_dump(_attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let devices = WG_MANAGER.devices.lock();
    let mut results = Vec::new();
    for dev in devices.values() {
        let payload = build_device_payload(dev.as_ref(), true, true);
        results.push((WG_CMD_GET_DEVICE, payload));
    }
    results
}

fn parse_allowedips(payload: &[u8], replace: bool) -> Vec<(u32, u8)> {
    let mut allowed_ips: Vec<(u32, u8)> = Vec::new();
    let mut pos = 0;
    while pos + 4 <= payload.len() {
        let len = u16::from_le_bytes([payload[pos], payload[pos + 1]]) as usize;
        let _typ = u16::from_le_bytes([payload[pos + 2], payload[pos + 3]]);
        if len < 4 { break; }
        let data_start = pos + 4;
        let data_end = pos + len;
        if data_end > payload.len() { break; }
        let inner = &payload[data_start..data_end];
        let flags = find_attr_u32(inner, WGALLOWEDIP_A_FLAGS).unwrap_or(0);
        let family = find_attr_u16(inner, WGALLOWEDIP_A_FAMILY).unwrap_or(0);
        let ipaddr = find_attr_binary(inner, WGALLOWEDIP_A_IPADDR);
        let cidr_mask = find_attr_u8(inner, WGALLOWEDIP_A_CIDR_MASK).unwrap_or(32);

        if flags & WGALLOWEDIP_F_REMOVE_ME != 0 {
            continue;
        }

        if let Some(ip_bytes) = ipaddr {
            if family == AF_INET && ip_bytes.len() >= 4 {
                let ip = u32::from_be_bytes([ip_bytes[0], ip_bytes[1], ip_bytes[2], ip_bytes[3]]);
                if !replace {
                    allowed_ips.push((ip, cidr_mask));
                }
            }
        }
        if len == 0 { break; }
        pos += len;
    }
    if replace {
        allowed_ips
    } else {
        allowed_ips
    }
}

fn parse_peers(payload: &[u8], device: &WgDevice, replace_peers: bool) {
    let mut new_peers: BTreeMap<[u8; WG_KEY_SIZE], Arc<WgPeer>> = BTreeMap::new();

    let mut pos = 0;
    while pos + 4 <= payload.len() {
        let len = u16::from_le_bytes([payload[pos], payload[pos + 1]]) as usize;
        let _typ = u16::from_le_bytes([payload[pos + 2], payload[pos + 3]]);
        if len < 4 { break; }
        let data_start = pos + 4;
        let data_end = pos + len;
        if data_end > payload.len() { break; }
        let inner = &payload[data_start..data_end];

        let public_key_bytes = find_attr_binary(inner, WGPEER_A_PUBLIC_KEY);
        let flags = find_attr_u32(inner, WGPEER_A_FLAGS).unwrap_or(0);

        if let Some(pk_bytes) = public_key_bytes {
            if pk_bytes.len() != WG_KEY_SIZE { pos += len; continue; }
            let mut pk = [0u8; WG_KEY_SIZE];
            pk.copy_from_slice(pk_bytes);

            if flags & WGPEER_F_REMOVE_ME != 0 {
                device.remove_peer(&WgKey::from_bytes(pk));
                pos += len;
                continue;
            }

            let existing = device.get_peer(&WgKey::from_bytes(pk));

            if flags & WGPEER_F_UPDATE_ONLY != 0 && existing.is_none() {
                pos += len;
                continue;
            }

            let mut peer = if let Some(ex) = existing {
                (*ex).clone()
            } else {
                WgPeer::new(WgKey::from_bytes(pk))
            };

            if let Some(preshared_key_bytes) = find_attr_binary(inner, WGPEER_A_PRESHARED_KEY) {
                if preshared_key_bytes.len() == WG_KEY_SIZE {
                    let mut psk = [0u8; WG_KEY_SIZE];
                    psk.copy_from_slice(preshared_key_bytes);
                    peer.preshared_key = WgKey::from_bytes(psk);
                }
            }

            if let Some(endpoint_bytes) = find_attr_binary(inner, WGPEER_A_ENDPOINT) {
                if let Some((ip, port)) = parse_sockaddr_in(endpoint_bytes) {
                    peer.endpoint_ip = ip;
                    peer.endpoint_port = port;
                }
            }

            if let Some(ka) = find_attr_u16(inner, WGPEER_A_PERSISTENT_KEEPALIVE_INTERVAL) {
                peer.keepalive.store(ka as u32, Ordering::Relaxed);
            }

            let replace_allowedips = (flags & WGPEER_F_REPLACE_ALLOWEDIPS) != 0;
            if let Some(allowedips_bytes) = find_attr_binary(inner, WGPEER_A_ALLOWEDIPS) {
                let new_ips = parse_allowedips(allowedips_bytes, replace_allowedips);
                if replace_allowedips {
                    peer.allowed_ips = new_ips;
                } else {
                    for (ip, prefix) in new_ips {
                        if !peer.allowed_ips.contains(&(ip, prefix)) {
                            peer.allowed_ips.push((ip, prefix));
                        }
                    }
                }
            }

            new_peers.insert(pk, Arc::new(peer));
        }
        if len == 0 { break; }
        pos += len;
    }

    let mut current_peers = device.peers.lock();
    for (pk, peer) in new_peers {
        current_peers.insert(pk, peer);
    }
}

fn handle_set_device(attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let ifindex = find_attr_u32(attr_payload, WGDEVICE_A_IFINDEX);
    let ifname = find_attr_string(attr_payload, WGDEVICE_A_IFNAME);

    let device = if let Some(idx) = ifindex {
        find_device_by_ifindex(idx)
    } else if let Some(name_bytes) = ifname {
        let name = core::str::from_utf8(name_bytes).unwrap_or("");
        find_device_by_name(name)
    } else {
        None
    };

    let device = if let Some(d) = device {
        d
    } else {
        let name = if let Some(name_bytes) = ifname {
            core::str::from_utf8(name_bytes).unwrap_or("wg0")
        } else {
            "wg0"
        };
        WG_MANAGER.create_device(name)
    };

    let flags = find_attr_u32(attr_payload, WGDEVICE_A_FLAGS).unwrap_or(0);
    let replace_peers = (flags & WGDEVICE_F_REPLACE_PEERS) != 0;

    if let Some(pk_bytes) = find_attr_binary(attr_payload, WGDEVICE_A_PRIVATE_KEY) {
        if pk_bytes.len() == WG_KEY_SIZE {
            let all_zero = pk_bytes.iter().all(|&b| b == 0);
            if !all_zero {
                let mut key = [0u8; WG_KEY_SIZE];
                key.copy_from_slice(pk_bytes);
                *device.private_key.lock() = WgKey::from_bytes(key);
                let x25519_priv = crate::crypto::ed25519::X25519PrivateKey::from_bytes(key);
                let pub_key = WgKey::from_bytes(*x25519_priv.public_key().as_bytes());
                *device.public_key.lock() = pub_key;
            }
        }
    }

    if let Some(port) = find_attr_u16(attr_payload, WGDEVICE_A_LISTEN_PORT) {
        device.listen_port.store(port as u32, Ordering::Relaxed);
    }

    if let Some(fwmark) = find_attr_u32(attr_payload, WGDEVICE_A_FWMARK) {
        device.fwmark.store(fwmark, Ordering::Relaxed);
    }

    if replace_peers {
        *device.peers.lock() = BTreeMap::new();
    }

    if let Some(peers_bytes) = find_attr_binary(attr_payload, WGDEVICE_A_PEERS) {
        parse_peers(peers_bytes, &device, replace_peers);
    }

    vec![]
}

pub fn handle_wireguard_genl_request(
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
        WG_CMD_GET_DEVICE => {
            let mut responses = handle_get_device_dump(attr_payload);
            if responses.is_empty() {
                responses = handle_get_device(attr_payload);
            }
            responses
        }
        WG_CMD_SET_DEVICE => handle_set_device(attr_payload),
        _ => return,
    };

    let mut all_responses: Vec<(u8, Vec<u8>)> = cmd_responses;
    all_responses.push((0, Vec::new()));

    for (resp_cmd, resp_payload) in &all_responses {
        let mut inner = Vec::new();
        let ghdr = GenlMsgHdr::new(*resp_cmd, WIREGUARD_GENL_VERSION);
        inner.extend_from_slice(ghdr.as_bytes());
        inner.extend_from_slice(resp_payload);

        let is_done = resp_payload.is_empty() && *resp_cmd == 0;
        let msg_type = if is_done { 3u16 } else { WIREGUARD_GENL_ID };

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
    use crate::net::wireguard::WgKey;

    #[test]
    fn test_wireguard_genl_constants() {
        assert_eq!(WIREGUARD_GENL_ID, 45);
        assert_eq!(WG_CMD_GET_DEVICE, 0);
        assert_eq!(WG_CMD_SET_DEVICE, 1);

        assert_eq!(WGDEVICE_A_IFINDEX, 1);
        assert_eq!(WGDEVICE_A_IFNAME, 2);
        assert_eq!(WGDEVICE_A_PRIVATE_KEY, 3);
        assert_eq!(WGDEVICE_A_PUBLIC_KEY, 4);
        assert_eq!(WGDEVICE_A_FLAGS, 5);
        assert_eq!(WGDEVICE_A_LISTEN_PORT, 6);
        assert_eq!(WGDEVICE_A_FWMARK, 7);
        assert_eq!(WGDEVICE_A_PEERS, 8);

        assert_eq!(WGPEER_A_PUBLIC_KEY, 1);
        assert_eq!(WGPEER_A_PRESHARED_KEY, 2);
        assert_eq!(WGPEER_A_FLAGS, 3);
        assert_eq!(WGPEER_A_ENDPOINT, 4);
        assert_eq!(WGPEER_A_PERSISTENT_KEEPALIVE_INTERVAL, 5);
        assert_eq!(WGPEER_A_LAST_HANDSHAKE_TIME, 6);
        assert_eq!(WGPEER_A_RX_BYTES, 7);
        assert_eq!(WGPEER_A_TX_BYTES, 8);
        assert_eq!(WGPEER_A_ALLOWEDIPS, 9);
        assert_eq!(WGPEER_A_PROTOCOL_VERSION, 10);

        assert_eq!(WGALLOWEDIP_A_FAMILY, 1);
        assert_eq!(WGALLOWEDIP_A_IPADDR, 2);
        assert_eq!(WGALLOWEDIP_A_CIDR_MASK, 3);
        assert_eq!(WGALLOWEDIP_A_FLAGS, 4);

        assert_eq!(WGDEVICE_F_REPLACE_PEERS, 1);
        assert_eq!(WGPEER_F_REMOVE_ME, 1);
        assert_eq!(WGPEER_F_REPLACE_ALLOWEDIPS, 2);
        assert_eq!(WGPEER_F_UPDATE_ONLY, 4);
        assert_eq!(WGALLOWEDIP_F_REMOVE_ME, 1);
    }

    #[test]
    fn test_sockaddr_in_roundtrip() {
        let ip = 0x0100007f;
        let port = 51820;
        let encoded = build_sockaddr_in(ip, port);
        assert_eq!(encoded.len(), 16);
        let (decoded_ip, decoded_port) = parse_sockaddr_in(&encoded).unwrap();
        assert_eq!(decoded_ip, ip);
        assert_eq!(decoded_port, port);
    }

    #[test]
    fn test_build_device_and_peer_payload() {
        let device = WG_MANAGER.create_device("wg-genl-test");
        let payload = build_device_payload(&device, true, true);
        assert!(!payload.is_empty());
    }
}
