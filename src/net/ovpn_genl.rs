use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::vec;
use core::sync::atomic::Ordering;
use spin::Mutex;

use crate::net::net_device::NET_DEVICE_MANAGER;
use crate::net::netlink::{NlMsgHdr, NlAttr, NETLINK_MANAGER};

pub const OVPN_GENL_ID: u16 = 49;
pub const OVPN_GENL_VERSION: u8 = 1;

pub const OVPN_CMD_PEER_NEW: u8 = 0;
pub const OVPN_CMD_PEER_SET: u8 = 1;
pub const OVPN_CMD_PEER_GET: u8 = 2;
pub const OVPN_CMD_PEER_DEL: u8 = 3;
pub const OVPN_CMD_PEER_DEL_NTF: u8 = 4;
pub const OVPN_CMD_KEY_NEW: u8 = 5;
pub const OVPN_CMD_KEY_GET: u8 = 6;
pub const OVPN_CMD_KEY_SWAP: u8 = 7;
pub const OVPN_CMD_KEY_SWAP_NTF: u8 = 8;
pub const OVPN_CMD_KEY_DEL: u8 = 9;
pub const OVPN_CMD_PEER_FLOAT_NTF: u8 = 10;

pub const OVPN_ATTR_IFINDEX: u16 = 1;
pub const OVPN_ATTR_PEER: u16 = 2;
pub const OVPN_ATTR_KEYCONF: u16 = 3;

pub const OVPN_PEER_ATTR_IFINDEX: u16 = 0;
pub const OVPN_PEER_ATTR_ID: u16 = 1;
pub const OVPN_PEER_ATTR_REMOTE_IPV4: u16 = 2;
pub const OVPN_PEER_ATTR_REMOTE_IPV6: u16 = 3;
pub const OVPN_PEER_ATTR_REMOTE_IPV6_SCOPE_ID: u16 = 4;
pub const OVPN_PEER_ATTR_REMOTE_PORT: u16 = 5;
pub const OVPN_PEER_ATTR_SOCKET: u16 = 6;
pub const OVPN_PEER_ATTR_SOCKET_NETNSID: u16 = 7;
pub const OVPN_PEER_ATTR_VPN_IPV4: u16 = 8;
pub const OVPN_PEER_ATTR_VPN_IPV6: u16 = 9;
pub const OVPN_PEER_ATTR_LOCAL_IPV4: u16 = 10;
pub const OVPN_PEER_ATTR_LOCAL_IPV6: u16 = 11;
pub const OVPN_PEER_ATTR_LOCAL_PORT: u16 = 12;
pub const OVPN_PEER_ATTR_KEEPALIVE_INTERVAL: u16 = 13;
pub const OVPN_PEER_ATTR_KEEPALIVE_TIMEOUT: u16 = 14;
pub const OVPN_PEER_ATTR_DEL_REASON: u16 = 15;
pub const OVPN_PEER_ATTR_VPN_RX_BYTES: u16 = 16;
pub const OVPN_PEER_ATTR_VPN_TX_BYTES: u16 = 17;
pub const OVPN_PEER_ATTR_VPN_RX_PACKETS: u16 = 18;
pub const OVPN_PEER_ATTR_VPN_TX_PACKETS: u16 = 19;
pub const OVPN_PEER_ATTR_LINK_RX_BYTES: u16 = 20;
pub const OVPN_PEER_ATTR_LINK_TX_BYTES: u16 = 21;
pub const OVPN_PEER_ATTR_LINK_RX_PACKETS: u16 = 22;
pub const OVPN_PEER_ATTR_LINK_TX_PACKETS: u16 = 23;
pub const OVPN_PEER_ATTR_TX_ID: u16 = 24;

pub const OVPN_KEYCONF_ATTR_PEER_ID: u16 = 1;
pub const OVPN_KEYCONF_ATTR_SLOT: u16 = 2;
pub const OVPN_KEYCONF_ATTR_KEY_ID: u16 = 3;
pub const OVPN_KEYCONF_ATTR_CIPHER_ALG: u16 = 4;
pub const OVPN_KEYCONF_ATTR_ENCRYPT_DIR: u16 = 5;
pub const OVPN_KEYCONF_ATTR_DECRYPT_DIR: u16 = 6;

pub const OVPN_KEYDIR_ATTR_CIPHER_KEY: u16 = 1;
pub const OVPN_KEYDIR_ATTR_NONCE_TAIL: u16 = 2;

pub const OVPN_CIPHER_ALG_NONE: u32 = 0;
pub const OVPN_CIPHER_ALG_AES_GCM: u32 = 1;
pub const OVPN_CIPHER_ALG_CHACHA20_POLY1305: u32 = 2;

pub const OVPN_DEL_PEER_REASON_TEARDOWN: u32 = 0;
pub const OVPN_DEL_PEER_REASON_USERSPACE: u32 = 1;
pub const OVPN_DEL_PEER_REASON_EXPIRED: u32 = 2;
pub const OVPN_DEL_PEER_REASON_TRANSPORT_ERROR: u32 = 3;
pub const OVPN_DEL_PEER_REASON_TRANSPORT_DISCONNECT: u32 = 4;

pub const OVPN_KEY_SLOT_PRIMARY: u32 = 0;
pub const OVPN_KEY_SLOT_SECONDARY: u32 = 1;

pub const OVPN_MAX_PEER_ID: u32 = 0xFFFFFF;
pub const OVPN_MAX_TX_ID: u32 = 0xFFFFFF;
pub const OVPN_MAX_KEY_ID: u32 = 7;
pub const OVPN_NONCE_TAIL_SIZE: usize = 8;
pub const OVPN_GENL_MCGRP: u32 = 1;

#[derive(Clone, Debug)]
struct OvpnEndpoint {
    ipv4: u32,
    ipv6: [u8; 16],
    ipv6_scope_id: u32,
    port: u16,
}

impl Default for OvpnEndpoint {
    fn default() -> Self {
        OvpnEndpoint {
            ipv4: 0,
            ipv6: [0u8; 16],
            ipv6_scope_id: 0,
            port: 0,
        }
    }
}

#[derive(Clone, Debug)]
struct OvpnPeer {
    id: u32,
    remote: OvpnEndpoint,
    local: OvpnEndpoint,
    vpn_ipv4: u32,
    vpn_ipv6: [u8; 16],
    socket_fd: u32,
    socket_netnsid: i32,
    keepalive_interval: u32,
    keepalive_timeout: u32,
    tx_id: u32,
    vpn_rx_bytes: u64,
    vpn_tx_bytes: u64,
    vpn_rx_packets: u64,
    vpn_tx_packets: u64,
    link_rx_bytes: u64,
    link_tx_bytes: u64,
    link_rx_packets: u64,
    link_tx_packets: u64,
}

#[derive(Clone, Debug)]
struct OvpnKeyDir {
    cipher_key: Vec<u8>,
    nonce_tail: [u8; 8],
}

#[derive(Clone, Debug)]
struct OvpnKeyConf {
    peer_id: u32,
    slot: u32,
    key_id: u32,
    cipher_alg: u32,
    encrypt_dir: OvpnKeyDir,
    decrypt_dir: OvpnKeyDir,
}

#[derive(Clone, Debug)]
struct OvpnDevice {
    ifindex: u32,
    peers: BTreeMap<u32, OvpnPeer>,
    keyconfs: BTreeMap<(u32, u32), OvpnKeyConf>,
}

static OVPN_DEVICES: spin::Mutex<BTreeMap<u32, OvpnDevice>> =
    spin::Mutex::new(BTreeMap::new());

#[derive(Clone, Copy, Debug)]
#[repr(C)]
struct GenlMsgHdr {
    cmd: u8,
    version: u8,
    reserved: u16,
}

impl GenlMsgHdr {
    fn new(cmd: u8) -> Self {
        GenlMsgHdr {
            cmd,
            version: OVPN_GENL_VERSION,
            reserved: 0,
        }
    }

    fn as_bytes(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self as *const Self as *const u8, 4) }
    }
}

fn find_attr_u32(payload: &[u8], attr_type: u16) -> Option<u32> {
    let mut pos = 0;
    while pos + 4 <= payload.len() {
        let len = u16::from_le_bytes([payload[pos], payload[pos + 1]]) as usize;
        let typ = u16::from_le_bytes([payload[pos + 2], payload[pos + 3]]);
        if len < 4 {
            break;
        }
        let d = pos + 4;
        let e = pos + len;
        if typ == attr_type && e <= payload.len() && e - d >= 4 {
            return Some(u32::from_ne_bytes([
                payload[d],
                payload[d + 1],
                payload[d + 2],
                payload[d + 3],
            ]));
        }
        if len == 0 {
            break;
        }
        pos += len;
    }
    None
}

fn find_attr_u64(payload: &[u8], attr_type: u16) -> Option<u64> {
    let mut pos = 0;
    while pos + 4 <= payload.len() {
        let len = u16::from_le_bytes([payload[pos], payload[pos + 1]]) as usize;
        let typ = u16::from_le_bytes([payload[pos + 2], payload[pos + 3]]);
        if len < 4 {
            break;
        }
        let d = pos + 4;
        let e = pos + len;
        if typ == attr_type && e <= payload.len() && e - d >= 8 {
            return Some(u64::from_ne_bytes([
                payload[d],
                payload[d + 1],
                payload[d + 2],
                payload[d + 3],
                payload[d + 4],
                payload[d + 5],
                payload[d + 6],
                payload[d + 7],
            ]));
        }
        if len == 0 {
            break;
        }
        pos += len;
    }
    None
}

fn find_attr_binary<'a>(payload: &'a [u8], attr_type: u16, min_len: usize) -> Option<&'a [u8]> {
    let mut pos = 0;
    while pos + 4 <= payload.len() {
        let len = u16::from_le_bytes([payload[pos], payload[pos + 1]]) as usize;
        let typ = u16::from_le_bytes([payload[pos + 2], payload[pos + 3]]);
        if len < 4 {
            break;
        }
        let d = pos + 4;
        let e = pos + len;
        if typ == attr_type && e <= payload.len() && e - d >= min_len {
            return Some(&payload[d..e]);
        }
        if len == 0 {
            break;
        }
        pos += len;
    }
    None
}

fn find_attr_nested<'a>(payload: &'a [u8], attr_type: u16) -> Option<&'a [u8]> {
    let mut pos = 0;
    while pos + 4 <= payload.len() {
        let len = u16::from_le_bytes([payload[pos], payload[pos + 1]]) as usize;
        let typ = u16::from_le_bytes([payload[pos + 2], payload[pos + 3]]);
        if len < 4 {
            break;
        }
        let d = pos + 4;
        let e = pos + len;
        if typ == attr_type && e <= payload.len() {
            return Some(&payload[d..e]);
        }
        if len == 0 {
            break;
        }
        pos += len;
    }
    None
}

fn find_attr_i32(payload: &[u8], attr_type: u16) -> Option<i32> {
    find_attr_u32(payload, attr_type).map(|v| v as i32)
}

fn send_response(src_pid: u32, seq: u32, cmd: u8, payload: &[u8], is_done: bool) {
    let mut inner = Vec::new();
    let ghdr_cmd = if is_done { 0u8 } else { cmd };
    let ghdr = GenlMsgHdr::new(ghdr_cmd);
    inner.extend_from_slice(ghdr.as_bytes());
    inner.extend_from_slice(payload);

    let msg_type = if is_done { 3u16 } else { OVPN_GENL_ID };
    let total_len = (core::mem::size_of::<NlMsgHdr>() + inner.len()) as u32;
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

fn send_notification(cmd: u8, payload: &[u8]) {
    let ghdr = GenlMsgHdr::new(cmd);
    let mut inner = Vec::new();
    inner.extend_from_slice(ghdr.as_bytes());
    inner.extend_from_slice(payload);

    let total_len = (core::mem::size_of::<NlMsgHdr>() + inner.len()) as u32;
    let hdr = NlMsgHdr::new(total_len, OVPN_GENL_ID, 0, 0, 0);
    let msg = crate::net::netlink::NetlinkMessage {
        header: hdr,
        payload: inner,
    };

    NETLINK_MANAGER.broadcast(OVPN_GENL_MCGRP, msg);
}

fn build_peer_attrs(peer: &OvpnPeer) -> Vec<u8> {
    let mut attrs = Vec::new();

    attrs.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_ID, &peer.id.to_ne_bytes()));

    if peer.remote.ipv4 != 0 {
        attrs.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_REMOTE_IPV4, &peer.remote.ipv4.to_be_bytes()));
    }

    if peer.remote.ipv6 != [0u8; 16] {
        attrs.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_REMOTE_IPV6, &peer.remote.ipv6));
    }

    if peer.remote.ipv6_scope_id != 0 {
        attrs.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_REMOTE_IPV6_SCOPE_ID, &peer.remote.ipv6_scope_id.to_ne_bytes()));
    }

    if peer.remote.port != 0 {
        attrs.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_REMOTE_PORT, &peer.remote.port.to_be_bytes()));
    }

    if peer.socket_fd != 0 {
        attrs.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_SOCKET, &peer.socket_fd.to_ne_bytes()));
    }

    if peer.socket_netnsid != -1 {
        attrs.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_SOCKET_NETNSID, &(peer.socket_netnsid as u32).to_ne_bytes()));
    }

    if peer.vpn_ipv4 != 0 {
        attrs.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_VPN_IPV4, &peer.vpn_ipv4.to_be_bytes()));
    }

    if peer.vpn_ipv6 != [0u8; 16] {
        attrs.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_VPN_IPV6, &peer.vpn_ipv6));
    }

    if peer.local.ipv4 != 0 {
        attrs.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_LOCAL_IPV4, &peer.local.ipv4.to_be_bytes()));
    }

    if peer.local.ipv6 != [0u8; 16] {
        attrs.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_LOCAL_IPV6, &peer.local.ipv6));
    }

    if peer.local.port != 0 {
        attrs.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_LOCAL_PORT, &peer.local.port.to_be_bytes()));
    }

    if peer.keepalive_interval != 0 {
        attrs.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_KEEPALIVE_INTERVAL, &peer.keepalive_interval.to_ne_bytes()));
    }

    if peer.keepalive_timeout != 0 {
        attrs.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_KEEPALIVE_TIMEOUT, &peer.keepalive_timeout.to_ne_bytes()));
    }

    if peer.tx_id != 0 {
        attrs.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_TX_ID, &peer.tx_id.to_ne_bytes()));
    }

    attrs.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_VPN_RX_BYTES, &peer.vpn_rx_bytes.to_ne_bytes()));
    attrs.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_VPN_TX_BYTES, &peer.vpn_tx_bytes.to_ne_bytes()));
    attrs.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_VPN_RX_PACKETS, &peer.vpn_rx_packets.to_ne_bytes()));
    attrs.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_VPN_TX_PACKETS, &peer.vpn_tx_packets.to_ne_bytes()));
    attrs.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_LINK_RX_BYTES, &peer.link_rx_bytes.to_ne_bytes()));
    attrs.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_LINK_TX_BYTES, &peer.link_tx_bytes.to_ne_bytes()));
    attrs.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_LINK_RX_PACKETS, &peer.link_rx_packets.to_ne_bytes()));
    attrs.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_LINK_TX_PACKETS, &peer.link_tx_packets.to_ne_bytes()));

    attrs
}

fn build_peer_float_dump(ifindex: u32, peer: &OvpnPeer) -> Vec<u8> {
    let mut attrs = Vec::new();
    attrs.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_IFINDEX, &ifindex.to_ne_bytes()));
    attrs.extend_from_slice(&build_peer_attrs(peer));
    attrs
}

fn build_keydir_attrs(keydir: &OvpnKeyDir) -> Vec<u8> {
    let mut attrs = Vec::new();
    if !keydir.cipher_key.is_empty() {
        attrs.extend_from_slice(&NlAttr::new(OVPN_KEYDIR_ATTR_CIPHER_KEY, &keydir.cipher_key));
    }
    if keydir.nonce_tail != [0u8; OVPN_NONCE_TAIL_SIZE] {
        attrs.extend_from_slice(&NlAttr::new(OVPN_KEYDIR_ATTR_NONCE_TAIL, &keydir.nonce_tail));
    }
    attrs
}

fn build_keyconf_attrs(keyconf: &OvpnKeyConf) -> Vec<u8> {
    let mut attrs = Vec::new();
    attrs.extend_from_slice(&NlAttr::new(OVPN_KEYCONF_ATTR_PEER_ID, &keyconf.peer_id.to_ne_bytes()));
    attrs.extend_from_slice(&NlAttr::new(OVPN_KEYCONF_ATTR_SLOT, &keyconf.slot.to_ne_bytes()));
    attrs.extend_from_slice(&NlAttr::new(OVPN_KEYCONF_ATTR_KEY_ID, &keyconf.key_id.to_ne_bytes()));
    attrs.extend_from_slice(&NlAttr::new(OVPN_KEYCONF_ATTR_CIPHER_ALG, &keyconf.cipher_alg.to_ne_bytes()));

    let enc_attrs = build_keydir_attrs(&keyconf.encrypt_dir);
    if !enc_attrs.is_empty() {
        attrs.extend_from_slice(&NlAttr::new(OVPN_KEYCONF_ATTR_ENCRYPT_DIR, &enc_attrs));
    }

    let dec_attrs = build_keydir_attrs(&keyconf.decrypt_dir);
    if !dec_attrs.is_empty() {
        attrs.extend_from_slice(&NlAttr::new(OVPN_KEYCONF_ATTR_DECRYPT_DIR, &dec_attrs));
    }

    attrs
}

fn parse_peer_new_input(data: &[u8]) -> Option<OvpnPeer> {
    let id = find_attr_u32(data, OVPN_PEER_ATTR_ID)?;
    if id > OVPN_MAX_PEER_ID {
        return None;
    }

    let mut peer = OvpnPeer {
        id,
        remote: OvpnEndpoint::default(),
        local: OvpnEndpoint::default(),
        vpn_ipv4: 0,
        vpn_ipv6: [0u8; 16],
        socket_fd: 0,
        socket_netnsid: -1,
        keepalive_interval: 0,
        keepalive_timeout: 0,
        tx_id: 0,
        vpn_rx_bytes: 0,
        vpn_tx_bytes: 0,
        vpn_rx_packets: 0,
        vpn_tx_packets: 0,
        link_rx_bytes: 0,
        link_tx_bytes: 0,
        link_rx_packets: 0,
        link_tx_packets: 0,
    };

    if let Some(ipv4) = find_attr_u32(data, OVPN_PEER_ATTR_REMOTE_IPV4) {
        peer.remote.ipv4 = ipv4;
    }
    if let Some(ipv6) = find_attr_binary(data, OVPN_PEER_ATTR_REMOTE_IPV6, 16) {
        peer.remote.ipv6.copy_from_slice(&ipv6[..16]);
    }
    if let Some(scope) = find_attr_u32(data, OVPN_PEER_ATTR_REMOTE_IPV6_SCOPE_ID) {
        peer.remote.ipv6_scope_id = scope;
    }
    if let Some(port) = find_attr_u32(data, OVPN_PEER_ATTR_REMOTE_PORT) {
        peer.remote.port = port as u16;
    }
    if let Some(fd) = find_attr_u32(data, OVPN_PEER_ATTR_SOCKET) {
        peer.socket_fd = fd;
    }
    if let Some(vpn4) = find_attr_u32(data, OVPN_PEER_ATTR_VPN_IPV4) {
        peer.vpn_ipv4 = vpn4;
    }
    if let Some(vpn6) = find_attr_binary(data, OVPN_PEER_ATTR_VPN_IPV6, 16) {
        peer.vpn_ipv6.copy_from_slice(&vpn6[..16]);
    }
    if let Some(local4) = find_attr_u32(data, OVPN_PEER_ATTR_LOCAL_IPV4) {
        peer.local.ipv4 = local4;
    }
    if let Some(local6) = find_attr_binary(data, OVPN_PEER_ATTR_LOCAL_IPV6, 16) {
        peer.local.ipv6.copy_from_slice(&local6[..16]);
    }
    if let Some(port) = find_attr_u32(data, OVPN_PEER_ATTR_LOCAL_PORT) {
        peer.local.port = port as u16;
    }
    if let Some(interval) = find_attr_u32(data, OVPN_PEER_ATTR_KEEPALIVE_INTERVAL) {
        peer.keepalive_interval = interval;
    }
    if let Some(timeout) = find_attr_u32(data, OVPN_PEER_ATTR_KEEPALIVE_TIMEOUT) {
        peer.keepalive_timeout = timeout;
    }
    if let Some(tx_id) = find_attr_u32(data, OVPN_PEER_ATTR_TX_ID) {
        if tx_id > OVPN_MAX_TX_ID {
            return None;
        }
        peer.tx_id = tx_id;
    }

    Some(peer)
}

fn parse_peer_set_input(data: &[u8]) -> Option<OvpnPeer> {
    let id = find_attr_u32(data, OVPN_PEER_ATTR_ID)?;
    if id > OVPN_MAX_PEER_ID {
        return None;
    }

    let mut peer = OvpnPeer {
        id,
        remote: OvpnEndpoint::default(),
        local: OvpnEndpoint::default(),
        vpn_ipv4: 0,
        vpn_ipv6: [0u8; 16],
        socket_fd: 0,
        socket_netnsid: -1,
        keepalive_interval: 0,
        keepalive_timeout: 0,
        tx_id: 0,
        vpn_rx_bytes: 0,
        vpn_tx_bytes: 0,
        vpn_rx_packets: 0,
        vpn_tx_packets: 0,
        link_rx_bytes: 0,
        link_tx_bytes: 0,
        link_rx_packets: 0,
        link_tx_packets: 0,
    };

    if let Some(ipv4) = find_attr_u32(data, OVPN_PEER_ATTR_REMOTE_IPV4) {
        peer.remote.ipv4 = ipv4;
    }
    if let Some(ipv6) = find_attr_binary(data, OVPN_PEER_ATTR_REMOTE_IPV6, 16) {
        peer.remote.ipv6.copy_from_slice(&ipv6[..16]);
    }
    if let Some(scope) = find_attr_u32(data, OVPN_PEER_ATTR_REMOTE_IPV6_SCOPE_ID) {
        peer.remote.ipv6_scope_id = scope;
    }
    if let Some(port) = find_attr_u32(data, OVPN_PEER_ATTR_REMOTE_PORT) {
        peer.remote.port = port as u16;
    }
    if let Some(vpn4) = find_attr_u32(data, OVPN_PEER_ATTR_VPN_IPV4) {
        peer.vpn_ipv4 = vpn4;
    }
    if let Some(vpn6) = find_attr_binary(data, OVPN_PEER_ATTR_VPN_IPV6, 16) {
        peer.vpn_ipv6.copy_from_slice(&vpn6[..16]);
    }
    if let Some(local4) = find_attr_u32(data, OVPN_PEER_ATTR_LOCAL_IPV4) {
        peer.local.ipv4 = local4;
    }
    if let Some(local6) = find_attr_binary(data, OVPN_PEER_ATTR_LOCAL_IPV6, 16) {
        peer.local.ipv6.copy_from_slice(&local6[..16]);
    }
    if let Some(port) = find_attr_u32(data, OVPN_PEER_ATTR_LOCAL_PORT) {
        peer.local.port = port as u16;
    }
    if let Some(interval) = find_attr_u32(data, OVPN_PEER_ATTR_KEEPALIVE_INTERVAL) {
        peer.keepalive_interval = interval;
    }
    if let Some(timeout) = find_attr_u32(data, OVPN_PEER_ATTR_KEEPALIVE_TIMEOUT) {
        peer.keepalive_timeout = timeout;
    }
    if let Some(tx_id) = find_attr_u32(data, OVPN_PEER_ATTR_TX_ID) {
        if tx_id > OVPN_MAX_TX_ID {
            return None;
        }
        peer.tx_id = tx_id;
    }

    Some(peer)
}

fn parse_keydir(data: &[u8]) -> OvpnKeyDir {
    let mut keydir = OvpnKeyDir {
        cipher_key: Vec::new(),
        nonce_tail: [0u8; OVPN_NONCE_TAIL_SIZE],
    };

    if let Some(key) = find_attr_binary(data, OVPN_KEYDIR_ATTR_CIPHER_KEY, 0) {
        if key.len() <= 256 {
            keydir.cipher_key = key.to_vec();
        }
    }
    if let Some(tail) = find_attr_binary(data, OVPN_KEYDIR_ATTR_NONCE_TAIL, OVPN_NONCE_TAIL_SIZE) {
        keydir.nonce_tail.copy_from_slice(&tail[..OVPN_NONCE_TAIL_SIZE]);
    }

    keydir
}

fn parse_keyconf_input(data: &[u8]) -> Option<OvpnKeyConf> {
    let peer_id = find_attr_u32(data, OVPN_KEYCONF_ATTR_PEER_ID)?;
    if peer_id > OVPN_MAX_PEER_ID {
        return None;
    }
    let slot = find_attr_u32(data, OVPN_KEYCONF_ATTR_SLOT).unwrap_or(OVPN_KEY_SLOT_PRIMARY);
    if slot != OVPN_KEY_SLOT_PRIMARY && slot != OVPN_KEY_SLOT_SECONDARY {
        return None;
    }
    let key_id = find_attr_u32(data, OVPN_KEYCONF_ATTR_KEY_ID).unwrap_or(0);
    if key_id > OVPN_MAX_KEY_ID {
        return None;
    }
    let cipher_alg = find_attr_u32(data, OVPN_KEYCONF_ATTR_CIPHER_ALG).unwrap_or(OVPN_CIPHER_ALG_NONE);

    let encrypt_dir = find_attr_nested(data, OVPN_KEYCONF_ATTR_ENCRYPT_DIR)
        .map(parse_keydir)
        .unwrap_or(OvpnKeyDir {
            cipher_key: Vec::new(),
            nonce_tail: [0u8; OVPN_NONCE_TAIL_SIZE],
        });

    let decrypt_dir = find_attr_nested(data, OVPN_KEYCONF_ATTR_DECRYPT_DIR)
        .map(parse_keydir)
        .unwrap_or(OvpnKeyDir {
            cipher_key: Vec::new(),
            nonce_tail: [0u8; OVPN_NONCE_TAIL_SIZE],
        });

    Some(OvpnKeyConf {
        peer_id,
        slot,
        key_id,
        cipher_alg,
        encrypt_dir,
        decrypt_dir,
    })
}

fn get_or_create_device(ifindex: u32) -> OvpnDevice {
    let mut devices = OVPN_DEVICES.lock();
    devices.entry(ifindex).or_insert_with(|| OvpnDevice {
        ifindex,
        peers: BTreeMap::new(),
        keyconfs: BTreeMap::new(),
    }).clone()
}

fn handle_peer_new(src_pid: u32, seq: u32, attr_payload: &[u8]) {
    let ifindex = match find_attr_u32(attr_payload, OVPN_ATTR_IFINDEX) {
        Some(v) => v,
        None => return send_response(src_pid, seq, 0, &[], true),
    };

    let peer_nested = match find_attr_nested(attr_payload, OVPN_ATTR_PEER) {
        Some(v) => v,
        None => return send_response(src_pid, seq, 0, &[], true),
    };

    let peer = match parse_peer_new_input(peer_nested) {
        Some(p) => p,
        None => return send_response(src_pid, seq, 0, &[], true),
    };

    let mut devices = OVPN_DEVICES.lock();
    let device = devices.entry(ifindex).or_insert_with(|| OvpnDevice {
        ifindex,
        peers: BTreeMap::new(),
        keyconfs: BTreeMap::new(),
    });

    if device.peers.contains_key(&peer.id) {
        return send_response(src_pid, seq, 0, &[], true);
    }

    device.peers.insert(peer.id, peer);
    send_response(src_pid, seq, OVPN_CMD_PEER_NEW, &[], true);
}

fn handle_peer_set(src_pid: u32, seq: u32, attr_payload: &[u8]) {
    let ifindex = match find_attr_u32(attr_payload, OVPN_ATTR_IFINDEX) {
        Some(v) => v,
        None => return send_response(src_pid, seq, 0, &[], true),
    };

    let peer_nested = match find_attr_nested(attr_payload, OVPN_ATTR_PEER) {
        Some(v) => v,
        None => return send_response(src_pid, seq, 0, &[], true),
    };

    let update = match parse_peer_set_input(peer_nested) {
        Some(p) => p,
        None => return send_response(src_pid, seq, 0, &[], true),
    };

    let mut devices = OVPN_DEVICES.lock();
    let device = match devices.get_mut(&ifindex) {
        Some(d) => d,
        None => return send_response(src_pid, seq, 0, &[], true),
    };

    let peer = match device.peers.get_mut(&update.id) {
        Some(p) => p,
        None => return send_response(src_pid, seq, 0, &[], true),
    };

    if update.remote.ipv4 != 0 { peer.remote.ipv4 = update.remote.ipv4; }
    if update.remote.ipv6 != [0u8; 16] { peer.remote.ipv6 = update.remote.ipv6; }
    if update.remote.ipv6_scope_id != 0 { peer.remote.ipv6_scope_id = update.remote.ipv6_scope_id; }
    if update.remote.port != 0 { peer.remote.port = update.remote.port; }
    if update.local.ipv4 != 0 { peer.local.ipv4 = update.local.ipv4; }
    if update.local.ipv6 != [0u8; 16] { peer.local.ipv6 = update.local.ipv6; }
    if update.local.port != 0 { peer.local.port = update.local.port; }
    if update.vpn_ipv4 != 0 { peer.vpn_ipv4 = update.vpn_ipv4; }
    if update.vpn_ipv6 != [0u8; 16] { peer.vpn_ipv6 = update.vpn_ipv6; }
    if update.keepalive_interval != 0 { peer.keepalive_interval = update.keepalive_interval; }
    if update.keepalive_timeout != 0 { peer.keepalive_timeout = update.keepalive_timeout; }
    if update.tx_id != 0 { peer.tx_id = update.tx_id; }

    send_response(src_pid, seq, OVPN_CMD_PEER_SET, &[], true);
}

fn handle_peer_get(src_pid: u32, seq: u32, attr_payload: &[u8], is_dump: bool) {
    let devices = OVPN_DEVICES.lock();

    if is_dump {
        for (ifindex, device) in devices.iter() {
            for peer in device.peers.values() {
                let reply = build_peer_float_dump(*ifindex, peer);
                send_response(src_pid, seq, OVPN_CMD_PEER_GET, &reply, false);
            }
        }
    } else {
        let ifindex = match find_attr_u32(attr_payload, OVPN_ATTR_IFINDEX) {
            Some(v) => v,
            None => return send_response(src_pid, seq, 0, &[], true),
        };
        let req_peer_id = find_attr_nested(attr_payload, OVPN_ATTR_PEER)
            .and_then(|n| find_attr_u32(n, OVPN_PEER_ATTR_ID));

        if let Some(device) = devices.get(&ifindex) {
            if let Some(pid) = req_peer_id {
                if let Some(peer) = device.peers.get(&pid) {
                    let reply = build_peer_float_dump(ifindex, peer);
                    send_response(src_pid, seq, OVPN_CMD_PEER_GET, &reply, false);
                }
            }
        }
    }

    send_response(src_pid, seq, 0, &[], true);
}

fn handle_peer_del(src_pid: u32, seq: u32, attr_payload: &[u8]) {
    let ifindex = match find_attr_u32(attr_payload, OVPN_ATTR_IFINDEX) {
        Some(v) => v,
        None => return send_response(src_pid, seq, 0, &[], true),
    };

    let peer_nested = match find_attr_nested(attr_payload, OVPN_ATTR_PEER) {
        Some(v) => v,
        None => return send_response(src_pid, seq, 0, &[], true),
    };

    let peer_id = match find_attr_u32(peer_nested, OVPN_PEER_ATTR_ID) {
        Some(v) => v,
        None => return send_response(src_pid, seq, 0, &[], true),
    };

    let mut devices = OVPN_DEVICES.lock();
    let device = match devices.get_mut(&ifindex) {
        Some(d) => d,
        None => return send_response(src_pid, seq, 0, &[], true),
    };

    if device.peers.remove(&peer_id).is_some() {
        device.keyconfs.retain(|k, _| k.0 != peer_id);
    }

    send_response(src_pid, seq, OVPN_CMD_PEER_DEL, &[], true);
}

fn handle_key_new(src_pid: u32, seq: u32, attr_payload: &[u8]) {
    let ifindex = match find_attr_u32(attr_payload, OVPN_ATTR_IFINDEX) {
        Some(v) => v,
        None => return send_response(src_pid, seq, 0, &[], true),
    };

    let keyconf_nested = match find_attr_nested(attr_payload, OVPN_ATTR_KEYCONF) {
        Some(v) => v,
        None => return send_response(src_pid, seq, 0, &[], true),
    };

    let keyconf = match parse_keyconf_input(keyconf_nested) {
        Some(k) => k,
        None => return send_response(src_pid, seq, 0, &[], true),
    };

    let mut devices = OVPN_DEVICES.lock();
    let device = match devices.get_mut(&ifindex) {
        Some(d) => d,
        None => return send_response(src_pid, seq, 0, &[], true),
    };

    if !device.peers.contains_key(&keyconf.peer_id) {
        return send_response(src_pid, seq, 0, &[], true);
    }

    let key = (keyconf.peer_id, keyconf.slot);
    if device.keyconfs.contains_key(&key) {
        return send_response(src_pid, seq, 0, &[], true);
    }

    device.keyconfs.insert(key, keyconf);
    send_response(src_pid, seq, OVPN_CMD_KEY_NEW, &[], true);
}

fn handle_key_get(src_pid: u32, seq: u32, attr_payload: &[u8]) {
    let ifindex = match find_attr_u32(attr_payload, OVPN_ATTR_IFINDEX) {
        Some(v) => v,
        None => return send_response(src_pid, seq, 0, &[], true),
    };

    let devices = OVPN_DEVICES.lock();
    let device = match devices.get(&ifindex) {
        Some(d) => d,
        None => return send_response(src_pid, seq, 0, &[], true),
    };

    let req_keyconf = find_attr_nested(attr_payload, OVPN_ATTR_KEYCONF);
    if let Some(kc_data) = req_keyconf {
        let req_peer_id = find_attr_u32(kc_data, OVPN_KEYCONF_ATTR_PEER_ID);
        let req_slot = find_attr_u32(kc_data, OVPN_KEYCONF_ATTR_SLOT);

        if let (Some(pid), Some(slot)) = (req_peer_id, req_slot) {
            if let Some(keyconf) = device.keyconfs.get(&(pid, slot)) {
                let mut reply = Vec::new();
                reply.extend_from_slice(&NlAttr::new(OVPN_ATTR_IFINDEX, &ifindex.to_ne_bytes()));
                let kc_reply = build_keyconf_attrs(keyconf);
                reply.extend_from_slice(&NlAttr::new(OVPN_ATTR_KEYCONF, &kc_reply));
                send_response(src_pid, seq, OVPN_CMD_KEY_GET, &reply, false);
            }
        }
    }

    send_response(src_pid, seq, 0, &[], true);
}

fn handle_key_swap(src_pid: u32, seq: u32, attr_payload: &[u8]) {
    let ifindex = match find_attr_u32(attr_payload, OVPN_ATTR_IFINDEX) {
        Some(v) => v,
        None => return send_response(src_pid, seq, 0, &[], true),
    };

    let keyconf_nested = match find_attr_nested(attr_payload, OVPN_ATTR_KEYCONF) {
        Some(v) => v,
        None => return send_response(src_pid, seq, 0, &[], true),
    };

    let peer_id = match find_attr_u32(keyconf_nested, OVPN_KEYCONF_ATTR_PEER_ID) {
        Some(v) => v,
        None => return send_response(src_pid, seq, 0, &[], true),
    };

    let mut devices = OVPN_DEVICES.lock();
    let device = match devices.get_mut(&ifindex) {
        Some(d) => d,
        None => return send_response(src_pid, seq, 0, &[], true),
    };

    let primary = device.keyconfs.get(&(peer_id, OVPN_KEY_SLOT_PRIMARY)).cloned();
    let secondary = device.keyconfs.get(&(peer_id, OVPN_KEY_SLOT_SECONDARY)).cloned();

    if let (Some(prim), Some(sec)) = (primary, secondary) {
        let mut prim_mut = prim.clone();
        prim_mut.slot = OVPN_KEY_SLOT_SECONDARY;
        let mut sec_mut = sec.clone();
        sec_mut.slot = OVPN_KEY_SLOT_PRIMARY;

        device.keyconfs.remove(&(peer_id, OVPN_KEY_SLOT_PRIMARY));
        device.keyconfs.remove(&(peer_id, OVPN_KEY_SLOT_SECONDARY));
        device.keyconfs.insert((peer_id, OVPN_KEY_SLOT_PRIMARY), sec_mut);
        device.keyconfs.insert((peer_id, OVPN_KEY_SLOT_SECONDARY), prim_mut);
    }

    send_response(src_pid, seq, OVPN_CMD_KEY_SWAP, &[], true);
}

fn handle_key_del(src_pid: u32, seq: u32, attr_payload: &[u8]) {
    let ifindex = match find_attr_u32(attr_payload, OVPN_ATTR_IFINDEX) {
        Some(v) => v,
        None => return send_response(src_pid, seq, 0, &[], true),
    };

    let keyconf_nested = match find_attr_nested(attr_payload, OVPN_ATTR_KEYCONF) {
        Some(v) => v,
        None => return send_response(src_pid, seq, 0, &[], true),
    };

    let peer_id = match find_attr_u32(keyconf_nested, OVPN_KEYCONF_ATTR_PEER_ID) {
        Some(v) => v,
        None => return send_response(src_pid, seq, 0, &[], true),
    };
    let slot = find_attr_u32(keyconf_nested, OVPN_KEYCONF_ATTR_SLOT).unwrap_or(OVPN_KEY_SLOT_PRIMARY);

    let mut devices = OVPN_DEVICES.lock();
    let device = match devices.get_mut(&ifindex) {
        Some(d) => d,
        None => return send_response(src_pid, seq, 0, &[], true),
    };

    device.keyconfs.remove(&(peer_id, slot));
    send_response(src_pid, seq, OVPN_CMD_KEY_DEL, &[], true);
}

pub fn handle_ovpn_genl_request(
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

    match cmd {
        OVPN_CMD_PEER_NEW => handle_peer_new(src_pid, seq, attr_payload),
        OVPN_CMD_PEER_SET => handle_peer_set(src_pid, seq, attr_payload),
        OVPN_CMD_PEER_GET => handle_peer_get(src_pid, seq, attr_payload, false),
        OVPN_CMD_PEER_DEL => handle_peer_del(src_pid, seq, attr_payload),
        OVPN_CMD_KEY_NEW => handle_key_new(src_pid, seq, attr_payload),
        OVPN_CMD_KEY_GET => handle_key_get(src_pid, seq, attr_payload),
        OVPN_CMD_KEY_SWAP => handle_key_swap(src_pid, seq, attr_payload),
        OVPN_CMD_KEY_DEL => handle_key_del(src_pid, seq, attr_payload),
        OVPN_CMD_PEER_DEL_NTF | OVPN_CMD_KEY_SWAP_NTF | OVPN_CMD_PEER_FLOAT_NTF => {
            send_response(src_pid, seq, 0, &[], true);
        }
        _ => send_response(src_pid, seq, 0, &[], true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ovpn_constants() {
        assert_eq!(OVPN_GENL_ID, 49);
        assert_eq!(OVPN_GENL_VERSION, 1);
        assert_eq!(OVPN_CMD_PEER_NEW, 0);
        assert_eq!(OVPN_CMD_PEER_SET, 1);
        assert_eq!(OVPN_CMD_PEER_GET, 2);
        assert_eq!(OVPN_CMD_PEER_DEL, 3);
        assert_eq!(OVPN_CMD_PEER_DEL_NTF, 4);
        assert_eq!(OVPN_CMD_KEY_NEW, 5);
        assert_eq!(OVPN_CMD_KEY_GET, 6);
        assert_eq!(OVPN_CMD_KEY_SWAP, 7);
        assert_eq!(OVPN_CMD_KEY_SWAP_NTF, 8);
        assert_eq!(OVPN_CMD_KEY_DEL, 9);
        assert_eq!(OVPN_CMD_PEER_FLOAT_NTF, 10);
    }

    #[test]
    fn test_ovpn_attr_constants() {
        assert_eq!(OVPN_ATTR_IFINDEX, 1);
        assert_eq!(OVPN_ATTR_PEER, 2);
        assert_eq!(OVPN_ATTR_KEYCONF, 3);
        assert_eq!(OVPN_PEER_ATTR_IFINDEX, 0);
        assert_eq!(OVPN_PEER_ATTR_ID, 1);
        assert_eq!(OVPN_PEER_ATTR_REMOTE_IPV4, 2);
        assert_eq!(OVPN_PEER_ATTR_REMOTE_PORT, 5);
        assert_eq!(OVPN_PEER_ATTR_VPN_IPV4, 8);
        assert_eq!(OVPN_PEER_ATTR_VPN_RX_BYTES, 16);
        assert_eq!(OVPN_PEER_ATTR_TX_ID, 24);
    }

    #[test]
    fn test_keyconf_constants() {
        assert_eq!(OVPN_KEYCONF_ATTR_PEER_ID, 1);
        assert_eq!(OVPN_KEYCONF_ATTR_SLOT, 2);
        assert_eq!(OVPN_KEYCONF_ATTR_KEY_ID, 3);
        assert_eq!(OVPN_KEYCONF_ATTR_CIPHER_ALG, 4);
        assert_eq!(OVPN_KEYCONF_ATTR_ENCRYPT_DIR, 5);
        assert_eq!(OVPN_KEYCONF_ATTR_DECRYPT_DIR, 6);
    }

    #[test]
    fn test_keydir_constants() {
        assert_eq!(OVPN_KEYDIR_ATTR_CIPHER_KEY, 1);
        assert_eq!(OVPN_KEYDIR_ATTR_NONCE_TAIL, 2);
    }

    #[test]
    fn test_enum_constants() {
        assert_eq!(OVPN_CIPHER_ALG_NONE, 0);
        assert_eq!(OVPN_CIPHER_ALG_AES_GCM, 1);
        assert_eq!(OVPN_CIPHER_ALG_CHACHA20_POLY1305, 2);
        assert_eq!(OVPN_DEL_PEER_REASON_TEARDOWN, 0);
        assert_eq!(OVPN_DEL_PEER_REASON_USERSPACE, 1);
        assert_eq!(OVPN_DEL_PEER_REASON_EXPIRED, 2);
        assert_eq!(OVPN_DEL_PEER_REASON_TRANSPORT_ERROR, 3);
        assert_eq!(OVPN_DEL_PEER_REASON_TRANSPORT_DISCONNECT, 4);
        assert_eq!(OVPN_KEY_SLOT_PRIMARY, 0);
        assert_eq!(OVPN_KEY_SLOT_SECONDARY, 1);
    }

    #[test]
    fn test_max_constants() {
        assert_eq!(OVPN_MAX_PEER_ID, 0xFFFFFF);
        assert_eq!(OVPN_MAX_TX_ID, 0xFFFFFF);
        assert_eq!(OVPN_MAX_KEY_ID, 7);
        assert_eq!(OVPN_NONCE_TAIL_SIZE, 8);
    }

    #[test]
    fn test_genlmsghdr_size() {
        assert_eq!(core::mem::size_of::<GenlMsgHdr>(), 4);
    }

    #[test]
    fn test_find_attr_u32() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&NlAttr::new(5, &42u32.to_ne_bytes()));
        assert_eq!(find_attr_u32(&buf, 5), Some(42));
        assert_eq!(find_attr_u32(&buf, 99), None);
    }

    #[test]
    fn test_find_attr_u64() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&NlAttr::new(3, &0xDEADBEEFCAFEu64.to_ne_bytes()));
        assert_eq!(find_attr_u64(&buf, 3), Some(0xDEADBEEFCAFE));
        assert_eq!(find_attr_u64(&buf, 99), None);
    }

    #[test]
    fn test_find_attr_nested() {
        let inner = NlAttr::new(42, &[1u8, 2, 3, 4]);
        let buf = NlAttr::new(OVPN_ATTR_PEER, &inner);
        let found = find_attr_nested(&buf, OVPN_ATTR_PEER);
        assert!(found.is_some());
        assert_eq!(found.unwrap(), &inner[..]);
    }

    #[test]
    fn test_find_attr_binary() {
        let buf = NlAttr::new(OVPN_KEYDIR_ATTR_CIPHER_KEY, &[0x01u8, 0x02, 0x03, 0x04]);
        let found = find_attr_binary(&buf, OVPN_KEYDIR_ATTR_CIPHER_KEY, 4);
        assert!(found.is_some());
        assert_eq!(found.unwrap(), &[0x01, 0x02, 0x03, 0x04]);
    }

    #[test]
    fn test_find_attr_i32() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_SOCKET_NETNSID, &(-1i32 as u32).to_ne_bytes()));
        assert_eq!(find_attr_i32(&buf, OVPN_PEER_ATTR_SOCKET_NETNSID), Some(-1));
    }

    #[test]
    fn test_parse_peer_new_input_basic() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_ID, &1u32.to_ne_bytes()));
        buf.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_REMOTE_IPV4, &0xC0A80001u32.to_be_bytes()));
        buf.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_REMOTE_PORT, &1194u16.to_be_bytes()));
        buf.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_VPN_IPV4, &0x0A000001u32.to_be_bytes()));

        let peer = parse_peer_new_input(&buf).expect("should parse");
        assert_eq!(peer.id, 1);
        assert_eq!(peer.remote.ipv4, 0xC0A80001);
        assert_eq!(peer.remote.port, 1194);
        assert_eq!(peer.vpn_ipv4, 0x0A000001);
    }

    #[test]
    fn test_parse_peer_new_input_rejects_invalid_id() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_ID, &0x1000000u32.to_ne_bytes()));
        assert!(parse_peer_new_input(&buf).is_none());
    }

    #[test]
    fn test_parse_peer_new_input_rejects_invalid_tx_id() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_ID, &1u32.to_ne_bytes()));
        buf.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_TX_ID, &0x1000000u32.to_ne_bytes()));
        assert!(parse_peer_new_input(&buf).is_none());
    }

    #[test]
    fn test_parse_keyconf_input() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&NlAttr::new(OVPN_KEYCONF_ATTR_PEER_ID, &1u32.to_ne_bytes()));
        buf.extend_from_slice(&NlAttr::new(OVPN_KEYCONF_ATTR_SLOT, &OVPN_KEY_SLOT_PRIMARY.to_ne_bytes()));
        buf.extend_from_slice(&NlAttr::new(OVPN_KEYCONF_ATTR_KEY_ID, &3u32.to_ne_bytes()));
        buf.extend_from_slice(&NlAttr::new(OVPN_KEYCONF_ATTR_CIPHER_ALG, &OVPN_CIPHER_ALG_AES_GCM.to_ne_bytes()));

        let keyconf = parse_keyconf_input(&buf).expect("should parse");
        assert_eq!(keyconf.peer_id, 1);
        assert_eq!(keyconf.slot, OVPN_KEY_SLOT_PRIMARY);
        assert_eq!(keyconf.key_id, 3);
        assert_eq!(keyconf.cipher_alg, OVPN_CIPHER_ALG_AES_GCM);
    }

    #[test]
    fn test_parse_keyconf_input_rejects_invalid_peer_id() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&NlAttr::new(OVPN_KEYCONF_ATTR_PEER_ID, &0x1000000u32.to_ne_bytes()));
        assert!(parse_keyconf_input(&buf).is_none());
    }

    #[test]
    fn test_parse_keyconf_input_rejects_invalid_slot() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&NlAttr::new(OVPN_KEYCONF_ATTR_PEER_ID, &1u32.to_ne_bytes()));
        buf.extend_from_slice(&NlAttr::new(OVPN_KEYCONF_ATTR_SLOT, &99u32.to_ne_bytes()));
        assert!(parse_keyconf_input(&buf).is_none());
    }

    #[test]
    fn test_parse_keyconf_input_rejects_invalid_key_id() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&NlAttr::new(OVPN_KEYCONF_ATTR_PEER_ID, &1u32.to_ne_bytes()));
        buf.extend_from_slice(&NlAttr::new(OVPN_KEYCONF_ATTR_KEY_ID, &8u32.to_ne_bytes()));
        assert!(parse_keyconf_input(&buf).is_none());
    }

    #[test]
    fn test_build_peer_attrs_roundtrip() {
        let peer = OvpnPeer {
            id: 1,
            remote: OvpnEndpoint {
                ipv4: 0xC0A80001,
                ipv6: [0u8; 16],
                ipv6_scope_id: 0,
                port: 1194,
            },
            local: OvpnEndpoint {
                ipv4: 0xC0A80064,
                ipv6: [0u8; 16],
                ipv6_scope_id: 0,
                port: 0,
            },
            vpn_ipv4: 0x0A000001,
            vpn_ipv6: [0u8; 16],
            socket_fd: 3,
            socket_netnsid: -1,
            keepalive_interval: 10,
            keepalive_timeout: 60,
            tx_id: 0,
            vpn_rx_bytes: 1000,
            vpn_tx_bytes: 500,
            vpn_rx_packets: 10,
            vpn_tx_packets: 5,
            link_rx_bytes: 2000,
            link_tx_bytes: 1000,
            link_rx_packets: 20,
            link_tx_packets: 10,
        };

        let attrs = build_peer_attrs(&peer);

        assert_eq!(find_attr_u32(&attrs, OVPN_PEER_ATTR_ID), Some(1));
        assert_eq!(find_attr_u32(&attrs, OVPN_PEER_ATTR_REMOTE_IPV4), Some(0xC0A80001));
    }

    #[test]
    fn test_build_keydir_attrs_roundtrip() {
        let keydir = OvpnKeyDir {
            cipher_key: vec![0x01, 0x02, 0x03, 0x04, 0x05],
            nonce_tail: [0x10, 0x20, 0x30, 0x40, 0x50, 0x60, 0x70, 0x80],
        };

        let attrs = build_keydir_attrs(&keydir);

        let cipher_key = find_attr_binary(&attrs, OVPN_KEYDIR_ATTR_CIPHER_KEY, 0);
        assert!(cipher_key.is_some());
        assert_eq!(cipher_key.unwrap(), &[0x01, 0x02, 0x03, 0x04, 0x05]);

        let nonce = find_attr_binary(&attrs, OVPN_KEYDIR_ATTR_NONCE_TAIL, 8);
        assert!(nonce.is_some());
        assert_eq!(nonce.unwrap(), &[0x10, 0x20, 0x30, 0x40, 0x50, 0x60, 0x70, 0x80]);
    }

    #[test]
    fn test_peer_new_and_get() {
        let ifindex = 100u32;
        let seq = 1u32;

        // Build peer-new request
        let mut peer_input = Vec::new();
        peer_input.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_ID, &1u32.to_ne_bytes()));
        peer_input.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_REMOTE_IPV4, &0xC0A80001u32.to_be_bytes()));
        peer_input.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_REMOTE_PORT, &1194u16.to_be_bytes()));
        peer_input.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_VPN_IPV4, &0x0A000001u32.to_be_bytes()));

        let mut req = Vec::new();
        req.extend_from_slice(&NlAttr::new(OVPN_ATTR_IFINDEX, &ifindex.to_ne_bytes()));
        req.extend_from_slice(&NlAttr::new(OVPN_ATTR_PEER, &peer_input));

        handle_peer_new(0, seq, &req);

        // Verify peer was created
        let devices = OVPN_DEVICES.lock();
        let device = devices.get(&ifindex).expect("device should exist");
        let peer = device.peers.get(&1).expect("peer should exist");
        assert_eq!(peer.remote.ipv4, 0xC0A80001);
        assert_eq!(peer.remote.port, 1194);
        assert_eq!(peer.vpn_ipv4, 0x0A000001);
        drop(devices);

        // Cleanup
        OVPN_DEVICES.lock().remove(&ifindex);
    }

    #[test]
    fn test_peer_new_duplicate_rejected() {
        let ifindex = 101u32;

        let mut peer_input = Vec::new();
        peer_input.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_ID, &1u32.to_ne_bytes()));

        let mut req = Vec::new();
        req.extend_from_slice(&NlAttr::new(OVPN_ATTR_IFINDEX, &ifindex.to_ne_bytes()));
        req.extend_from_slice(&NlAttr::new(OVPN_ATTR_PEER, &peer_input));

        handle_peer_new(0, 1, &req);
        handle_peer_new(0, 1, &req);

        let devices = OVPN_DEVICES.lock();
        assert_eq!(devices.get(&ifindex).unwrap().peers.len(), 1);
        drop(devices);

        OVPN_DEVICES.lock().remove(&ifindex);
    }

    #[test]
    fn test_peer_set_updates_fields() {
        let ifindex = 102u32;

        // Create peer
        let mut peer_input = Vec::new();
        peer_input.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_ID, &1u32.to_ne_bytes()));
        peer_input.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_REMOTE_IPV4, &0xC0A80001u32.to_be_bytes()));

        let mut req = Vec::new();
        req.extend_from_slice(&NlAttr::new(OVPN_ATTR_IFINDEX, &ifindex.to_ne_bytes()));
        req.extend_from_slice(&NlAttr::new(OVPN_ATTR_PEER, &peer_input));
        handle_peer_new(0, 1, &req);

        // Set new fields
        let mut set_input = Vec::new();
        set_input.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_ID, &1u32.to_ne_bytes()));
        set_input.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_REMOTE_IPV4, &0xC0A80002u32.to_be_bytes()));
        set_input.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_REMOTE_PORT, &443u16.to_be_bytes()));
        set_input.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_KEEPALIVE_INTERVAL, &10u32.to_ne_bytes()));

        let mut set_req = Vec::new();
        set_req.extend_from_slice(&NlAttr::new(OVPN_ATTR_IFINDEX, &ifindex.to_ne_bytes()));
        set_req.extend_from_slice(&NlAttr::new(OVPN_ATTR_PEER, &set_input));
        handle_peer_set(0, 2, &set_req);

        let devices = OVPN_DEVICES.lock();
        let peer = devices.get(&ifindex).unwrap().peers.get(&1).unwrap();
        assert_eq!(peer.remote.ipv4, 0xC0A80002);
        assert_eq!(peer.remote.port, 443);
        assert_eq!(peer.keepalive_interval, 10);
        drop(devices);

        OVPN_DEVICES.lock().remove(&ifindex);
    }

    #[test]
    fn test_peer_del_removes_peer_and_keys() {
        let ifindex = 103u32;

        // Create peer
        let mut peer_input = Vec::new();
        peer_input.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_ID, &1u32.to_ne_bytes()));

        let mut req = Vec::new();
        req.extend_from_slice(&NlAttr::new(OVPN_ATTR_IFINDEX, &ifindex.to_ne_bytes()));
        req.extend_from_slice(&NlAttr::new(OVPN_ATTR_PEER, &peer_input));
        handle_peer_new(0, 1, &req);

        // Add key
        let mut key_input = Vec::new();
        key_input.extend_from_slice(&NlAttr::new(OVPN_KEYCONF_ATTR_PEER_ID, &1u32.to_ne_bytes()));
        key_input.extend_from_slice(&NlAttr::new(OVPN_KEYCONF_ATTR_SLOT, &OVPN_KEY_SLOT_PRIMARY.to_ne_bytes()));

        let mut key_req = Vec::new();
        key_req.extend_from_slice(&NlAttr::new(OVPN_ATTR_IFINDEX, &ifindex.to_ne_bytes()));
        key_req.extend_from_slice(&NlAttr::new(OVPN_ATTR_KEYCONF, &key_input));
        handle_key_new(0, 2, &key_req);

        // Delete peer
        let mut del_input = Vec::new();
        del_input.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_ID, &1u32.to_ne_bytes()));

        let mut del_req = Vec::new();
        del_req.extend_from_slice(&NlAttr::new(OVPN_ATTR_IFINDEX, &ifindex.to_ne_bytes()));
        del_req.extend_from_slice(&NlAttr::new(OVPN_ATTR_PEER, &del_input));
        handle_peer_del(0, 3, &del_req);

        let devices = OVPN_DEVICES.lock();
        let device = devices.get(&ifindex).unwrap();
        assert!(device.peers.is_empty());
        assert!(device.keyconfs.is_empty());
        drop(devices);

        OVPN_DEVICES.lock().remove(&ifindex);
    }

    #[test]
    fn test_key_new_and_get() {
        let ifindex = 104u32;

        // Create peer first
        let mut peer_input = Vec::new();
        peer_input.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_ID, &1u32.to_ne_bytes()));

        let mut peer_req = Vec::new();
        peer_req.extend_from_slice(&NlAttr::new(OVPN_ATTR_IFINDEX, &ifindex.to_ne_bytes()));
        peer_req.extend_from_slice(&NlAttr::new(OVPN_ATTR_PEER, &peer_input));
        handle_peer_new(0, 1, &peer_req);

        // Add key
        let mut key_input = Vec::new();
        key_input.extend_from_slice(&NlAttr::new(OVPN_KEYCONF_ATTR_PEER_ID, &1u32.to_ne_bytes()));
        key_input.extend_from_slice(&NlAttr::new(OVPN_KEYCONF_ATTR_SLOT, &OVPN_KEY_SLOT_PRIMARY.to_ne_bytes()));
        key_input.extend_from_slice(&NlAttr::new(OVPN_KEYCONF_ATTR_KEY_ID, &2u32.to_ne_bytes()));
        key_input.extend_from_slice(&NlAttr::new(OVPN_KEYCONF_ATTR_CIPHER_ALG, &OVPN_CIPHER_ALG_AES_GCM.to_ne_bytes()));

        let mut key_req = Vec::new();
        key_req.extend_from_slice(&NlAttr::new(OVPN_ATTR_IFINDEX, &ifindex.to_ne_bytes()));
        key_req.extend_from_slice(&NlAttr::new(OVPN_ATTR_KEYCONF, &key_input));
        handle_key_new(0, 2, &key_req);

        // Verify
        let devices = OVPN_DEVICES.lock();
        let keyconf = devices.get(&ifindex)
            .and_then(|d| d.keyconfs.get(&(1, OVPN_KEY_SLOT_PRIMARY)))
            .expect("keyconf should exist");
        assert_eq!(keyconf.key_id, 2);
        assert_eq!(keyconf.cipher_alg, OVPN_CIPHER_ALG_AES_GCM);
        drop(devices);

        OVPN_DEVICES.lock().remove(&ifindex);
    }

    #[test]
    fn test_key_new_rejects_missing_peer() {
        let ifindex = 105u32;

        let mut key_input = Vec::new();
        key_input.extend_from_slice(&NlAttr::new(OVPN_KEYCONF_ATTR_PEER_ID, &999u32.to_ne_bytes()));
        key_input.extend_from_slice(&NlAttr::new(OVPN_KEYCONF_ATTR_SLOT, &OVPN_KEY_SLOT_PRIMARY.to_ne_bytes()));

        let mut key_req = Vec::new();
        key_req.extend_from_slice(&NlAttr::new(OVPN_ATTR_IFINDEX, &ifindex.to_ne_bytes()));
        key_req.extend_from_slice(&NlAttr::new(OVPN_ATTR_KEYCONF, &key_input));
        handle_key_new(0, 1, &key_req);

        let devices = OVPN_DEVICES.lock();
        assert!(devices.get(&ifindex).is_none());
        drop(devices);
    }

    #[test]
    fn test_key_swap() {
        let ifindex = 106u32;

        // Create peer
        let mut peer_input = Vec::new();
        peer_input.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_ID, &1u32.to_ne_bytes()));

        let mut peer_req = Vec::new();
        peer_req.extend_from_slice(&NlAttr::new(OVPN_ATTR_IFINDEX, &ifindex.to_ne_bytes()));
        peer_req.extend_from_slice(&NlAttr::new(OVPN_ATTR_PEER, &peer_input));
        handle_peer_new(0, 1, &peer_req);

        // Add primary key
        let mut prim_input = Vec::new();
        prim_input.extend_from_slice(&NlAttr::new(OVPN_KEYCONF_ATTR_PEER_ID, &1u32.to_ne_bytes()));
        prim_input.extend_from_slice(&NlAttr::new(OVPN_KEYCONF_ATTR_SLOT, &OVPN_KEY_SLOT_PRIMARY.to_ne_bytes()));
        prim_input.extend_from_slice(&NlAttr::new(OVPN_KEYCONF_ATTR_KEY_ID, &1u32.to_ne_bytes()));

        let mut prim_req = Vec::new();
        prim_req.extend_from_slice(&NlAttr::new(OVPN_ATTR_IFINDEX, &ifindex.to_ne_bytes()));
        prim_req.extend_from_slice(&NlAttr::new(OVPN_ATTR_KEYCONF, &prim_input));
        handle_key_new(0, 2, &prim_req);

        // Add secondary key
        let mut sec_input = Vec::new();
        sec_input.extend_from_slice(&NlAttr::new(OVPN_KEYCONF_ATTR_PEER_ID, &1u32.to_ne_bytes()));
        sec_input.extend_from_slice(&NlAttr::new(OVPN_KEYCONF_ATTR_SLOT, &OVPN_KEY_SLOT_SECONDARY.to_ne_bytes()));
        sec_input.extend_from_slice(&NlAttr::new(OVPN_KEYCONF_ATTR_KEY_ID, &2u32.to_ne_bytes()));

        let mut sec_req = Vec::new();
        sec_req.extend_from_slice(&NlAttr::new(OVPN_ATTR_IFINDEX, &ifindex.to_ne_bytes()));
        sec_req.extend_from_slice(&NlAttr::new(OVPN_ATTR_KEYCONF, &sec_input));
        handle_key_new(0, 3, &sec_req);

        // Swap keys
        let mut swap_input = Vec::new();
        swap_input.extend_from_slice(&NlAttr::new(OVPN_KEYCONF_ATTR_PEER_ID, &1u32.to_ne_bytes()));

        let mut swap_req = Vec::new();
        swap_req.extend_from_slice(&NlAttr::new(OVPN_ATTR_IFINDEX, &ifindex.to_ne_bytes()));
        swap_req.extend_from_slice(&NlAttr::new(OVPN_ATTR_KEYCONF, &swap_input));
        handle_key_swap(0, 4, &swap_req);

        // Verify swap
        let devices = OVPN_DEVICES.lock();
        let device = devices.get(&ifindex).unwrap();
        let primary = device.keyconfs.get(&(1, OVPN_KEY_SLOT_PRIMARY)).unwrap();
        let secondary = device.keyconfs.get(&(1, OVPN_KEY_SLOT_SECONDARY)).unwrap();
        assert_eq!(primary.key_id, 2);
        assert_eq!(secondary.key_id, 1);
        drop(devices);

        OVPN_DEVICES.lock().remove(&ifindex);
    }

    #[test]
    fn test_key_del() {
        let ifindex = 107u32;

        // Create peer
        let mut peer_input = Vec::new();
        peer_input.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_ID, &1u32.to_ne_bytes()));

        let mut peer_req = Vec::new();
        peer_req.extend_from_slice(&NlAttr::new(OVPN_ATTR_IFINDEX, &ifindex.to_ne_bytes()));
        peer_req.extend_from_slice(&NlAttr::new(OVPN_ATTR_PEER, &peer_input));
        handle_peer_new(0, 1, &peer_req);

        // Add key
        let mut key_input = Vec::new();
        key_input.extend_from_slice(&NlAttr::new(OVPN_KEYCONF_ATTR_PEER_ID, &1u32.to_ne_bytes()));
        key_input.extend_from_slice(&NlAttr::new(OVPN_KEYCONF_ATTR_SLOT, &OVPN_KEY_SLOT_PRIMARY.to_ne_bytes()));

        let mut key_req = Vec::new();
        key_req.extend_from_slice(&NlAttr::new(OVPN_ATTR_IFINDEX, &ifindex.to_ne_bytes()));
        key_req.extend_from_slice(&NlAttr::new(OVPN_ATTR_KEYCONF, &key_input));
        handle_key_new(0, 2, &key_req);

        // Delete key
        let mut del_input = Vec::new();
        del_input.extend_from_slice(&NlAttr::new(OVPN_KEYCONF_ATTR_PEER_ID, &1u32.to_ne_bytes()));
        del_input.extend_from_slice(&NlAttr::new(OVPN_KEYCONF_ATTR_SLOT, &OVPN_KEY_SLOT_PRIMARY.to_ne_bytes()));

        let mut del_req = Vec::new();
        del_req.extend_from_slice(&NlAttr::new(OVPN_ATTR_IFINDEX, &ifindex.to_ne_bytes()));
        del_req.extend_from_slice(&NlAttr::new(OVPN_ATTR_KEYCONF, &del_input));
        handle_key_del(0, 3, &del_req);

        let devices = OVPN_DEVICES.lock();
        assert!(devices.get(&ifindex).unwrap().keyconfs.is_empty());
        drop(devices);

        OVPN_DEVICES.lock().remove(&ifindex);
    }

    #[test]
    fn test_parse_keydir_with_nonce_tail_only() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&NlAttr::new(OVPN_KEYDIR_ATTR_NONCE_TAIL, &[0x01u8, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08]));

        let keydir = parse_keydir(&buf);
        assert!(keydir.cipher_key.is_empty());
        assert_eq!(keydir.nonce_tail, [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08]);
    }

    #[test]
    fn test_key_new_duplicate_rejected() {
        let ifindex = 108u32;

        // Create peer
        let mut peer_input = Vec::new();
        peer_input.extend_from_slice(&NlAttr::new(OVPN_PEER_ATTR_ID, &1u32.to_ne_bytes()));

        let mut peer_req = Vec::new();
        peer_req.extend_from_slice(&NlAttr::new(OVPN_ATTR_IFINDEX, &ifindex.to_ne_bytes()));
        peer_req.extend_from_slice(&NlAttr::new(OVPN_ATTR_PEER, &peer_input));
        handle_peer_new(0, 1, &peer_req);

        // Add same key twice
        let mut key_input = Vec::new();
        key_input.extend_from_slice(&NlAttr::new(OVPN_KEYCONF_ATTR_PEER_ID, &1u32.to_ne_bytes()));
        key_input.extend_from_slice(&NlAttr::new(OVPN_KEYCONF_ATTR_SLOT, &OVPN_KEY_SLOT_PRIMARY.to_ne_bytes()));

        let mut key_req = Vec::new();
        key_req.extend_from_slice(&NlAttr::new(OVPN_ATTR_IFINDEX, &ifindex.to_ne_bytes()));
        key_req.extend_from_slice(&NlAttr::new(OVPN_ATTR_KEYCONF, &key_input));
        handle_key_new(0, 2, &key_req);
        handle_key_new(0, 3, &key_req);

        let devices = OVPN_DEVICES.lock();
        assert_eq!(devices.get(&ifindex).unwrap().keyconfs.len(), 1);
        drop(devices);

        OVPN_DEVICES.lock().remove(&ifindex);
    }
}
