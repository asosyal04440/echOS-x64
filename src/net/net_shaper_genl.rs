use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::vec;
use core::sync::atomic::Ordering;
use spin::Mutex;

use crate::net::net_device::NET_DEVICE_MANAGER;
use crate::net::netlink::{NlMsgHdr, NlAttr, NETLINK_MANAGER};

pub const NET_SHAPER_GENL_ID: u16 = 48;
pub const NET_SHAPER_GENL_VERSION: u8 = 1;

pub const NET_SHAPER_CMD_GET: u8 = 0;
pub const NET_SHAPER_CMD_SET: u8 = 1;
pub const NET_SHAPER_CMD_DELETE: u8 = 2;
pub const NET_SHAPER_CMD_GROUP: u8 = 3;
pub const NET_SHAPER_CMD_CAP_GET: u8 = 4;

pub const NET_SHAPER_ATTR_HANDLE: u16 = 1;
pub const NET_SHAPER_ATTR_METRIC: u16 = 2;
pub const NET_SHAPER_ATTR_BW_MIN: u16 = 3;
pub const NET_SHAPER_ATTR_BW_MAX: u16 = 4;
pub const NET_SHAPER_ATTR_BURST: u16 = 5;
pub const NET_SHAPER_ATTR_PRIORITY: u16 = 6;
pub const NET_SHAPER_ATTR_WEIGHT: u16 = 7;
pub const NET_SHAPER_ATTR_IFINDEX: u16 = 8;
pub const NET_SHAPER_ATTR_PARENT: u16 = 9;
pub const NET_SHAPER_ATTR_LEAVES: u16 = 10;

pub const NET_SHAPER_HANDLE_ATTR_SCOPE: u16 = 1;
pub const NET_SHAPER_HANDLE_ATTR_ID: u16 = 2;

pub const NET_SHAPER_LEAF_ATTR_HANDLE: u16 = 1;
pub const NET_SHAPER_LEAF_ATTR_PRIORITY: u16 = 2;
pub const NET_SHAPER_LEAF_ATTR_WEIGHT: u16 = 3;

pub const NET_SHAPER_CAPS_ATTR_IFINDEX: u16 = 1;
pub const NET_SHAPER_CAPS_ATTR_SCOPE: u16 = 2;
pub const NET_SHAPER_CAPS_ATTR_SUPPORT_METRIC_BPS: u16 = 3;
pub const NET_SHAPER_CAPS_ATTR_SUPPORT_METRIC_PPS: u16 = 4;
pub const NET_SHAPER_CAPS_ATTR_SUPPORT_NESTING: u16 = 5;
pub const NET_SHAPER_CAPS_ATTR_SUPPORT_BW_MIN: u16 = 6;
pub const NET_SHAPER_CAPS_ATTR_SUPPORT_BW_MAX: u16 = 7;
pub const NET_SHAPER_CAPS_ATTR_SUPPORT_BURST: u16 = 8;
pub const NET_SHAPER_CAPS_ATTR_SUPPORT_PRIORITY: u16 = 9;
pub const NET_SHAPER_CAPS_ATTR_SUPPORT_WEIGHT: u16 = 10;

pub const NET_SHAPER_SCOPE_UNSPEC: u32 = 0;
pub const NET_SHAPER_SCOPE_NETDEV: u32 = 1;
pub const NET_SHAPER_SCOPE_QUEUE: u32 = 2;
pub const NET_SHAPER_SCOPE_NODE: u32 = 3;

pub const NET_SHAPER_METRIC_BPS: u32 = 0;
pub const NET_SHAPER_METRIC_PPS: u32 = 1;

pub const NET_SHAPER_MAX_HANDLE_ID: u32 = 67108862;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ShaperHandle {
    scope: u32,
    id: u32,
}

#[derive(Clone, Debug)]
struct ShaperNode {
    handle: ShaperHandle,
    metric: u32,
    bw_min: u64,
    bw_max: u64,
    burst: u64,
    priority: u32,
    weight: u32,
    parent: Option<ShaperHandle>,
    children: Vec<ShaperHandle>,
}

#[derive(Clone, Debug)]
struct ShaperCapEntry {
    scope: u32,
    support_metric_bps: bool,
    support_metric_pps: bool,
    support_nesting: bool,
    support_bw_min: bool,
    support_bw_max: bool,
    support_burst: bool,
    support_priority: bool,
    support_weight: bool,
}

static SHAPER_NODES: spin::Mutex<BTreeMap<u32, BTreeMap<(u32, u32), ShaperNode>>> =
    spin::Mutex::new(BTreeMap::new());

static SHAPER_CAPS: spin::Mutex<BTreeMap<u32, Vec<ShaperCapEntry>>> =
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
            version: NET_SHAPER_GENL_VERSION,
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

fn find_attr_nested(payload: &[u8], attr_type: u16) -> Option<&[u8]> {
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

fn find_attr_flag(payload: &[u8], attr_type: u16) -> bool {
    let mut pos = 0;
    while pos + 4 <= payload.len() {
        let len = u16::from_le_bytes([payload[pos], payload[pos + 1]]) as usize;
        let typ = u16::from_le_bytes([payload[pos + 2], payload[pos + 3]]);
        if len < 4 {
            break;
        }
        if typ == attr_type {
            return true;
        }
        if len == 0 {
            break;
        }
        pos += len;
    }
    false
}

fn parse_handle_attrs(data: &[u8]) -> Option<(u32, u32)> {
    let scope = find_attr_u32(data, NET_SHAPER_HANDLE_ATTR_SCOPE)?;
    let id = find_attr_u32(data, NET_SHAPER_HANDLE_ATTR_ID)?;
    if id > NET_SHAPER_MAX_HANDLE_ID {
        return None;
    }
    Some((scope, id))
}

fn send_response(
    src_pid: u32,
    seq: u32,
    cmd: u8,
    payload: &[u8],
    is_done: bool,
) {
    let mut inner = Vec::new();
    let ghdr_cmd = if is_done { 0u8 } else { cmd };
    let ghdr = GenlMsgHdr::new(ghdr_cmd);
    inner.extend_from_slice(ghdr.as_bytes());
    inner.extend_from_slice(payload);

    let msg_type = if is_done { 3u16 } else { NET_SHAPER_GENL_ID };
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

fn build_handle_attrs(handle: (u32, u32)) -> Vec<u8> {
    let mut attrs = Vec::new();
    let (scope, id) = handle;
    attrs.extend_from_slice(&NlAttr::new(NET_SHAPER_HANDLE_ATTR_SCOPE, &scope.to_ne_bytes()));
    attrs.extend_from_slice(&NlAttr::new(NET_SHAPER_HANDLE_ATTR_ID, &id.to_ne_bytes()));
    attrs
}

fn build_node_reply(node: &ShaperNode) -> Vec<u8> {
    let mut attrs = Vec::new();

    let handle_data = build_handle_attrs((node.handle.scope, node.handle.id));
    attrs.extend_from_slice(&NlAttr::new(NET_SHAPER_ATTR_HANDLE, &handle_data));

    attrs.extend_from_slice(&NlAttr::new(
        NET_SHAPER_ATTR_METRIC,
        &node.metric.to_ne_bytes(),
    ));
    attrs.extend_from_slice(&NlAttr::new(
        NET_SHAPER_ATTR_BW_MIN,
        &node.bw_min.to_ne_bytes(),
    ));
    attrs.extend_from_slice(&NlAttr::new(
        NET_SHAPER_ATTR_BW_MAX,
        &node.bw_max.to_ne_bytes(),
    ));
    attrs.extend_from_slice(&NlAttr::new(
        NET_SHAPER_ATTR_BURST,
        &node.burst.to_ne_bytes(),
    ));
    attrs.extend_from_slice(&NlAttr::new(
        NET_SHAPER_ATTR_PRIORITY,
        &node.priority.to_ne_bytes(),
    ));
    attrs.extend_from_slice(&NlAttr::new(
        NET_SHAPER_ATTR_WEIGHT,
        &node.weight.to_ne_bytes(),
    ));

    if let Some(parent) = &node.parent {
        let parent_data = build_handle_attrs((parent.scope, parent.id));
        attrs.extend_from_slice(&NlAttr::new(NET_SHAPER_ATTR_PARENT, &parent_data));
    }

    attrs
}

fn handle_get(src_pid: u32, seq: u32, attr_payload: &[u8], is_dump: bool) {
    let req_ifindex = find_attr_u32(attr_payload, NET_SHAPER_ATTR_IFINDEX);
    let req_handle = find_attr_nested(attr_payload, NET_SHAPER_ATTR_HANDLE)
        .and_then(parse_handle_attrs);

    let nodes = SHAPER_NODES.lock();

    if is_dump {
        if let Some(ifindex) = req_ifindex {
            if let Some(dev_nodes) = nodes.get(&ifindex) {
                for node in dev_nodes.values() {
                    let reply = build_node_reply(node);
                    let mut with_ifindex = Vec::new();
                    with_ifindex.extend_from_slice(&NlAttr::new(
                        NET_SHAPER_ATTR_IFINDEX,
                        &ifindex.to_ne_bytes(),
                    ));
                    with_ifindex.extend_from_slice(&reply);
                    send_response(src_pid, seq, NET_SHAPER_CMD_GET, &with_ifindex, false);
                }
            }
        }
    } else if let (Some(ifindex), Some(handle)) = (req_ifindex, req_handle) {
        if let Some(dev_nodes) = nodes.get(&ifindex) {
            if let Some(node) = dev_nodes.get(&(handle.0, handle.1)) {
                let reply = build_node_reply(node);
                let mut with_ifindex = Vec::new();
                with_ifindex.extend_from_slice(&NlAttr::new(
                    NET_SHAPER_ATTR_IFINDEX,
                    &ifindex.to_ne_bytes(),
                ));
                with_ifindex.extend_from_slice(&reply);
                send_response(src_pid, seq, NET_SHAPER_CMD_GET, &with_ifindex, false);
            }
        }
    }

    send_response(src_pid, seq, 0, &[], true);
}

fn handle_set(src_pid: u32, seq: u32, attr_payload: &[u8]) {
    let ifindex = match find_attr_u32(attr_payload, NET_SHAPER_ATTR_IFINDEX) {
        Some(v) => v,
        None => return send_response(src_pid, seq, 0, &[], true),
    };

    let handle = match find_attr_nested(attr_payload, NET_SHAPER_ATTR_HANDLE)
        .and_then(parse_handle_attrs)
    {
        Some(v) => v,
        None => return send_response(src_pid, seq, 0, &[], true),
    };

    let (scope, id) = handle;
    if scope == NET_SHAPER_SCOPE_NODE {
        return send_response(src_pid, seq, 0, &[], true);
    }

    let metric = find_attr_u32(attr_payload, NET_SHAPER_ATTR_METRIC).unwrap_or(0);
    let bw_min = find_attr_u64(attr_payload, NET_SHAPER_ATTR_BW_MIN).unwrap_or(0);
    let bw_max = find_attr_u64(attr_payload, NET_SHAPER_ATTR_BW_MAX).unwrap_or(0);
    let burst = find_attr_u64(attr_payload, NET_SHAPER_ATTR_BURST).unwrap_or(0);
    let priority = find_attr_u32(attr_payload, NET_SHAPER_ATTR_PRIORITY).unwrap_or(0);
    let weight = find_attr_u32(attr_payload, NET_SHAPER_ATTR_WEIGHT).unwrap_or(0);

    let mut nodes = SHAPER_NODES.lock();
    let dev_nodes = nodes.entry(ifindex).or_insert_with(BTreeMap::new);

    let node = ShaperNode {
        handle: ShaperHandle {
            scope,
            id,
        },
        metric,
        bw_min,
        bw_max,
        burst,
        priority,
        weight,
        parent: None,
        children: Vec::new(),
    };

    dev_nodes.insert((scope, id), node);
    send_response(src_pid, seq, NET_SHAPER_CMD_SET, &[], true);
}

fn handle_delete(src_pid: u32, seq: u32, attr_payload: &[u8]) {
    let ifindex = match find_attr_u32(attr_payload, NET_SHAPER_ATTR_IFINDEX) {
        Some(v) => v,
        None => return send_response(src_pid, seq, 0, &[], true),
    };

    let handle = match find_attr_nested(attr_payload, NET_SHAPER_ATTR_HANDLE)
        .and_then(parse_handle_attrs)
    {
        Some(v) => v,
        None => return send_response(src_pid, seq, 0, &[], true),
    };

    let mut nodes = SHAPER_NODES.lock();
    let dev_nodes = match nodes.get_mut(&ifindex) {
        Some(v) => v,
        None => return send_response(src_pid, seq, 0, &[], true),
    };

    let (scope, id) = handle;

    if scope == NET_SHAPER_SCOPE_NODE {
        let children: Vec<ShaperHandle> = dev_nodes
            .get(&(scope, id))
            .map(|n| n.children.clone())
            .unwrap_or_default();

        let parent_handle = dev_nodes.get(&(scope, id)).and_then(|n| n.parent);

        dev_nodes.remove(&(scope, id));

        for child in &children {
            if let Some(child_node) = dev_nodes.get_mut(&(child.scope, child.id)) {
                child_node.parent = parent_handle;
                if let Some(parent) = parent_handle {
                    if let Some(parent_node) = dev_nodes.get_mut(&(parent.scope, parent.id)) {
                        parent_node.children.push(*child);
                    }
                }
            }
        }

        if let Some(parent) = parent_handle {
            if let Some(parent_node) = dev_nodes.get(&(parent.scope, parent.id)) {
                if parent_node.children.is_empty() && parent_node.handle.scope == NET_SHAPER_SCOPE_NODE {
                    dev_nodes.remove(&(parent.scope, parent.id));
                }
            }
        }
    } else {
        if let Some(node) = dev_nodes.remove(&(scope, id)) {
            if let Some(parent) = node.parent {
                if let Some(parent_node) = dev_nodes.get_mut(&(parent.scope, parent.id)) {
                    parent_node.children.retain(|c| c.scope != scope || c.id != id);
                }
            }
        }
    }

    send_response(src_pid, seq, NET_SHAPER_CMD_DELETE, &[], true);
}

fn handle_group(src_pid: u32, seq: u32, attr_payload: &[u8]) {
    let ifindex = match find_attr_u32(attr_payload, NET_SHAPER_ATTR_IFINDEX) {
        Some(v) => v,
        None => return send_response(src_pid, seq, 0, &[], true),
    };

    let handle = match find_attr_nested(attr_payload, NET_SHAPER_ATTR_HANDLE)
        .and_then(parse_handle_attrs)
    {
        Some(v) => v,
        None => return send_response(src_pid, seq, 0, &[], true),
    };

    let (scope, id) = handle;
    if scope != NET_SHAPER_SCOPE_NODE && scope != NET_SHAPER_SCOPE_NETDEV {
        return send_response(src_pid, seq, 0, &[], true);
    }

    let parent = find_attr_nested(attr_payload, NET_SHAPER_ATTR_PARENT)
        .and_then(parse_handle_attrs);
    let metric = find_attr_u32(attr_payload, NET_SHAPER_ATTR_METRIC).unwrap_or(0);
    let bw_min = find_attr_u64(attr_payload, NET_SHAPER_ATTR_BW_MIN).unwrap_or(0);
    let bw_max = find_attr_u64(attr_payload, NET_SHAPER_ATTR_BW_MAX).unwrap_or(0);
    let burst = find_attr_u64(attr_payload, NET_SHAPER_ATTR_BURST).unwrap_or(0);
    let priority = find_attr_u32(attr_payload, NET_SHAPER_ATTR_PRIORITY).unwrap_or(0);
    let weight = find_attr_u32(attr_payload, NET_SHAPER_ATTR_WEIGHT).unwrap_or(0);

    let leaf_data = find_attr_nested(attr_payload, NET_SHAPER_ATTR_LEAVES);

    let mut nodes = SHAPER_NODES.lock();
    let dev_nodes = nodes.entry(ifindex).or_insert_with(BTreeMap::new);

    let final_id = if scope == NET_SHAPER_SCOPE_NODE && id == 0 {
        let new_id = (1u32..NET_SHAPER_MAX_HANDLE_ID)
            .find(|candidate| !dev_nodes.contains_key(&(NET_SHAPER_SCOPE_NODE, *candidate)))
            .unwrap_or(NET_SHAPER_MAX_HANDLE_ID);
        new_id
    } else {
        id
    };

    let node = ShaperNode {
        handle: ShaperHandle {
            scope,
            id: final_id,
        },
        metric,
        bw_min,
        bw_max,
        burst,
        priority,
        weight,
        parent: parent.map(|(s, i)| ShaperHandle { scope: s, id: i }),
        children: Vec::new(),
    };

    dev_nodes.insert((scope, final_id), node);

    if let Some(leaves_raw) = leaf_data {
        let mut leaf_pos = 0;
        while leaf_pos + 4 <= leaves_raw.len() {
            let llen = u16::from_le_bytes([
                leaves_raw[leaf_pos],
                leaves_raw[leaf_pos + 1],
            ]) as usize;
            let ltyp = u16::from_le_bytes([
                leaves_raw[leaf_pos + 2],
                leaves_raw[leaf_pos + 3],
            ]);
            if llen < 4 {
                break;
            }
            let ld = leaf_pos + 4;
            let le = leaf_pos + llen;
            if le > leaves_raw.len() {
                break;
            }

            if ltyp == NET_SHAPER_ATTR_LEAVES || ltyp == NET_SHAPER_LEAF_ATTR_HANDLE {
                if let Some(leaf_handle) = parse_handle_attrs(&leaves_raw[ld..le]) {
                    if let Some(child_node) = dev_nodes.get_mut(&(leaf_handle.0, leaf_handle.1)) {
                        child_node.parent = Some(ShaperHandle {
                            scope,
                            id: final_id,
                        });
                    }
                    if let Some(group_node) = dev_nodes.get_mut(&(scope, final_id)) {
                        group_node
                            .children
                            .push(ShaperHandle {
                                scope: leaf_handle.0,
                                id: leaf_handle.1,
                            });
                    }
                }
            }

            if llen == 0 {
                break;
            }
            leaf_pos += llen;
        }
    }

    let mut reply = Vec::new();
    let handle_data = build_handle_attrs((scope, final_id));
    reply.extend_from_slice(&NlAttr::new(NET_SHAPER_ATTR_HANDLE, &handle_data));
    send_response(src_pid, seq, NET_SHAPER_CMD_GROUP, &reply, true);
}

fn handle_cap_get(src_pid: u32, seq: u32, attr_payload: &[u8], is_dump: bool) {
    let req_ifindex = find_attr_u32(attr_payload, NET_SHAPER_CAPS_ATTR_IFINDEX);

    let caps = SHAPER_CAPS.lock();

    if is_dump {
        if let Some(ifindex) = req_ifindex {
            if let Some(entries) = caps.get(&ifindex) {
                for entry in entries {
                    let mut reply = Vec::new();
                    reply.extend_from_slice(&NlAttr::new(
                        NET_SHAPER_CAPS_ATTR_IFINDEX,
                        &ifindex.to_ne_bytes(),
                    ));
                    reply.extend_from_slice(&NlAttr::new(
                        NET_SHAPER_CAPS_ATTR_SCOPE,
                        &entry.scope.to_ne_bytes(),
                    ));
                    if entry.support_metric_bps {
                        reply.extend_from_slice(&NlAttr::new(NET_SHAPER_CAPS_ATTR_SUPPORT_METRIC_BPS, &[]));
                    }
                    if entry.support_metric_pps {
                        reply.extend_from_slice(&NlAttr::new(NET_SHAPER_CAPS_ATTR_SUPPORT_METRIC_PPS, &[]));
                    }
                    if entry.support_nesting {
                        reply.extend_from_slice(&NlAttr::new(NET_SHAPER_CAPS_ATTR_SUPPORT_NESTING, &[]));
                    }
                    if entry.support_bw_min {
                        reply.extend_from_slice(&NlAttr::new(NET_SHAPER_CAPS_ATTR_SUPPORT_BW_MIN, &[]));
                    }
                    if entry.support_bw_max {
                        reply.extend_from_slice(&NlAttr::new(NET_SHAPER_CAPS_ATTR_SUPPORT_BW_MAX, &[]));
                    }
                    if entry.support_burst {
                        reply.extend_from_slice(&NlAttr::new(NET_SHAPER_CAPS_ATTR_SUPPORT_BURST, &[]));
                    }
                    if entry.support_priority {
                        reply.extend_from_slice(&NlAttr::new(NET_SHAPER_CAPS_ATTR_SUPPORT_PRIORITY, &[]));
                    }
                    if entry.support_weight {
                        reply.extend_from_slice(&NlAttr::new(NET_SHAPER_CAPS_ATTR_SUPPORT_WEIGHT, &[]));
                    }
                    send_response(src_pid, seq, NET_SHAPER_CMD_CAP_GET, &reply, false);
                }
            }
        }
    } else {
        let req_scope = find_attr_u32(attr_payload, NET_SHAPER_CAPS_ATTR_SCOPE);
        if let (Some(ifindex), Some(scope)) = (req_ifindex, req_scope) {
            if let Some(entries) = caps.get(&ifindex) {
                for entry in entries {
                    if entry.scope != scope {
                        continue;
                    }
                    let mut reply = Vec::new();
                    reply.extend_from_slice(&NlAttr::new(
                        NET_SHAPER_CAPS_ATTR_IFINDEX,
                        &ifindex.to_ne_bytes(),
                    ));
                    reply.extend_from_slice(&NlAttr::new(
                        NET_SHAPER_CAPS_ATTR_SCOPE,
                        &scope.to_ne_bytes(),
                    ));
                    if entry.support_metric_bps {
                        reply.extend_from_slice(&NlAttr::new(NET_SHAPER_CAPS_ATTR_SUPPORT_METRIC_BPS, &[]));
                    }
                    if entry.support_metric_pps {
                        reply.extend_from_slice(&NlAttr::new(NET_SHAPER_CAPS_ATTR_SUPPORT_METRIC_PPS, &[]));
                    }
                    if entry.support_nesting {
                        reply.extend_from_slice(&NlAttr::new(NET_SHAPER_CAPS_ATTR_SUPPORT_NESTING, &[]));
                    }
                    if entry.support_bw_min {
                        reply.extend_from_slice(&NlAttr::new(NET_SHAPER_CAPS_ATTR_SUPPORT_BW_MIN, &[]));
                    }
                    if entry.support_bw_max {
                        reply.extend_from_slice(&NlAttr::new(NET_SHAPER_CAPS_ATTR_SUPPORT_BW_MAX, &[]));
                    }
                    if entry.support_burst {
                        reply.extend_from_slice(&NlAttr::new(NET_SHAPER_CAPS_ATTR_SUPPORT_BURST, &[]));
                    }
                    if entry.support_priority {
                        reply.extend_from_slice(&NlAttr::new(NET_SHAPER_CAPS_ATTR_SUPPORT_PRIORITY, &[]));
                    }
                    if entry.support_weight {
                        reply.extend_from_slice(&NlAttr::new(NET_SHAPER_CAPS_ATTR_SUPPORT_WEIGHT, &[]));
                    }
                    send_response(src_pid, seq, NET_SHAPER_CMD_CAP_GET, &reply, false);
                    break;
                }
            }
        }
    }

    send_response(src_pid, seq, 0, &[], true);
}

pub fn register_shaper_cap(
    ifindex: u32,
    scope: u32,
    support_metric_bps: bool,
    support_metric_pps: bool,
    support_nesting: bool,
    support_bw_min: bool,
    support_bw_max: bool,
    support_burst: bool,
    support_priority: bool,
    support_weight: bool,
) {
    let mut caps = SHAPER_CAPS.lock();
    let entries = caps.entry(ifindex).or_insert_with(Vec::new);
    entries.push(ShaperCapEntry {
        scope,
        support_metric_bps,
        support_metric_pps,
        support_nesting,
        support_bw_min,
        support_bw_max,
        support_burst,
        support_priority,
        support_weight,
    });
}

pub fn handle_net_shaper_genl_request(
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
        NET_SHAPER_CMD_GET => handle_get(src_pid, seq, attr_payload, false),
        NET_SHAPER_CMD_SET => handle_set(src_pid, seq, attr_payload),
        NET_SHAPER_CMD_DELETE => handle_delete(src_pid, seq, attr_payload),
        NET_SHAPER_CMD_GROUP => handle_group(src_pid, seq, attr_payload),
        NET_SHAPER_CMD_CAP_GET => handle_cap_get(src_pid, seq, attr_payload, false),
        _ => send_response(src_pid, seq, 0, &[], true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_net_shaper_constants() {
        assert_eq!(NET_SHAPER_GENL_ID, 48);
        assert_eq!(NET_SHAPER_GENL_VERSION, 1);
        assert_eq!(NET_SHAPER_CMD_GET, 0);
        assert_eq!(NET_SHAPER_CMD_SET, 1);
        assert_eq!(NET_SHAPER_CMD_DELETE, 2);
        assert_eq!(NET_SHAPER_CMD_GROUP, 3);
        assert_eq!(NET_SHAPER_CMD_CAP_GET, 4);
        assert_eq!(NET_SHAPER_SCOPE_UNSPEC, 0);
        assert_eq!(NET_SHAPER_SCOPE_NETDEV, 1);
        assert_eq!(NET_SHAPER_SCOPE_QUEUE, 2);
        assert_eq!(NET_SHAPER_SCOPE_NODE, 3);
        assert_eq!(NET_SHAPER_METRIC_BPS, 0);
        assert_eq!(NET_SHAPER_METRIC_PPS, 1);
        assert_eq!(NET_SHAPER_MAX_HANDLE_ID, 67108862);
    }

    #[test]
    fn test_net_shaper_attr_constants() {
        assert_eq!(NET_SHAPER_ATTR_HANDLE, 1);
        assert_eq!(NET_SHAPER_ATTR_METRIC, 2);
        assert_eq!(NET_SHAPER_ATTR_BW_MIN, 3);
        assert_eq!(NET_SHAPER_ATTR_BW_MAX, 4);
        assert_eq!(NET_SHAPER_ATTR_BURST, 5);
        assert_eq!(NET_SHAPER_ATTR_PRIORITY, 6);
        assert_eq!(NET_SHAPER_ATTR_WEIGHT, 7);
        assert_eq!(NET_SHAPER_ATTR_IFINDEX, 8);
        assert_eq!(NET_SHAPER_ATTR_PARENT, 9);
        assert_eq!(NET_SHAPER_ATTR_LEAVES, 10);
        assert_eq!(NET_SHAPER_HANDLE_ATTR_SCOPE, 1);
        assert_eq!(NET_SHAPER_HANDLE_ATTR_ID, 2);
        assert_eq!(NET_SHAPER_LEAF_ATTR_HANDLE, 1);
        assert_eq!(NET_SHAPER_LEAF_ATTR_PRIORITY, 2);
        assert_eq!(NET_SHAPER_LEAF_ATTR_WEIGHT, 3);
    }

    #[test]
    fn test_caps_attr_constants() {
        assert_eq!(NET_SHAPER_CAPS_ATTR_IFINDEX, 1);
        assert_eq!(NET_SHAPER_CAPS_ATTR_SCOPE, 2);
        assert_eq!(NET_SHAPER_CAPS_ATTR_SUPPORT_METRIC_BPS, 3);
        assert_eq!(NET_SHAPER_CAPS_ATTR_SUPPORT_METRIC_PPS, 4);
        assert_eq!(NET_SHAPER_CAPS_ATTR_SUPPORT_NESTING, 5);
        assert_eq!(NET_SHAPER_CAPS_ATTR_SUPPORT_BW_MIN, 6);
        assert_eq!(NET_SHAPER_CAPS_ATTR_SUPPORT_BW_MAX, 7);
        assert_eq!(NET_SHAPER_CAPS_ATTR_SUPPORT_BURST, 8);
        assert_eq!(NET_SHAPER_CAPS_ATTR_SUPPORT_PRIORITY, 9);
        assert_eq!(NET_SHAPER_CAPS_ATTR_SUPPORT_WEIGHT, 10);
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
    fn test_parse_handle_attrs() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&NlAttr::new(NET_SHAPER_HANDLE_ATTR_SCOPE, &2u32.to_ne_bytes()));
        buf.extend_from_slice(&NlAttr::new(NET_SHAPER_HANDLE_ATTR_ID, &5u32.to_ne_bytes()));
        let result = parse_handle_attrs(&buf);
        assert_eq!(result, Some((2, 5)));
    }

    #[test]
    fn test_parse_handle_attrs_rejects_oversized_id() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&NlAttr::new(NET_SHAPER_HANDLE_ATTR_SCOPE, &1u32.to_ne_bytes()));
        buf.extend_from_slice(&NlAttr::new(NET_SHAPER_HANDLE_ATTR_ID, &(NET_SHAPER_MAX_HANDLE_ID + 1).to_ne_bytes()));
        assert_eq!(parse_handle_attrs(&buf), None);
    }

    #[test]
    fn test_build_handle_attrs_roundtrip() {
        let built = build_handle_attrs((2, 7));
        let parsed = parse_handle_attrs(&built);
        assert_eq!(parsed, Some((2, 7)));
    }

    #[test]
    fn test_find_attr_flag() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&NlAttr::new(3, &[]));
        assert!(find_attr_flag(&buf, 3));
        assert!(!find_attr_flag(&buf, 99));
    }

    #[test]
    fn test_set_and_get_shaper() {
        let ifindex = 1u32;

        let mut set_payload = Vec::new();
        set_payload.extend_from_slice(&NlAttr::new(NET_SHAPER_ATTR_IFINDEX, &ifindex.to_ne_bytes()));
        let handle_data = build_handle_attrs((NET_SHAPER_SCOPE_QUEUE, 0));
        set_payload.extend_from_slice(&NlAttr::new(NET_SHAPER_ATTR_HANDLE, &handle_data));
        set_payload.extend_from_slice(&NlAttr::new(NET_SHAPER_ATTR_METRIC, &0u32.to_ne_bytes()));
        set_payload.extend_from_slice(&NlAttr::new(NET_SHAPER_ATTR_BW_MIN, &1_000_000_000u64.to_ne_bytes()));
        set_payload.extend_from_slice(&NlAttr::new(NET_SHAPER_ATTR_BW_MAX, &10_000_000_000u64.to_ne_bytes()));
        set_payload.extend_from_slice(&NlAttr::new(NET_SHAPER_ATTR_BURST, &65536u64.to_ne_bytes()));
        set_payload.extend_from_slice(&NlAttr::new(NET_SHAPER_ATTR_PRIORITY, &1u32.to_ne_bytes()));
        set_payload.extend_from_slice(&NlAttr::new(NET_SHAPER_ATTR_WEIGHT, &100u32.to_ne_bytes()));

        handle_set(0, 1, &set_payload);

        let nodes = SHAPER_NODES.lock();
        let dev_nodes = nodes.get(&ifindex).expect("should have ifindex entry");
        let node = dev_nodes.get(&(NET_SHAPER_SCOPE_QUEUE, 0)).expect("should have queue 0 shaper");
        assert_eq!(node.metric, 0);
        assert_eq!(node.bw_min, 1_000_000_000);
        assert_eq!(node.bw_max, 10_000_000_000);
        assert_eq!(node.burst, 65536);
        assert_eq!(node.priority, 1);
        assert_eq!(node.weight, 100);
    }

    #[test]
    fn test_register_caps_and_cap_get() {
        let ifindex = 2u32;
        register_shaper_cap(ifindex, NET_SHAPER_SCOPE_QUEUE, true, false, true, true, true, true, true, true);

        let caps = SHAPER_CAPS.lock();
        let entries = caps.get(&ifindex).expect("should have caps for ifindex");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].scope, NET_SHAPER_SCOPE_QUEUE);
        assert!(entries[0].support_metric_bps);
        assert!(!entries[0].support_metric_pps);
        assert!(entries[0].support_nesting);
        assert!(entries[0].support_bw_min);
    }

    #[test]
    fn test_genlmsghdr_size() {
        assert_eq!(core::mem::size_of::<GenlMsgHdr>(), 4);
    }
}
