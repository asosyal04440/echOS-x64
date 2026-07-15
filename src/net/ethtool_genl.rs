//! # Ethtool Generic Netlink — NETLINK_GENERIC ailesi
//!
//! Linux `ethtool` komutunun netlink üzerinden çalışan modern arayüzü.
//! Kullanıcı uzayı, `ETHTOOL_MSG_*` komutlarını `ETHTOOL_GENL_ID`
//! family ID'si ile generic netlink üzerinden gönderir.
//!
//! ## Desteklenen komutlar
//!
//! | Komut                        | ID | Dönen bilgi                         |
//! |------------------------------|----|--------------------------------------|
//! | ETHTOOL_MSG_STRSET_GET       | 1  | String set ID + string liste         |
//! | ETHTOOL_MSG_LINKINFO_GET     | 2  | Port türü, PHY adres, MDIX          |
//! | ETHTOOL_MSG_LINKSTATE_GET    | 6  | Link durumu (up/down)                |
//! | ETHTOOL_MSG_STATS_GET        | 32 | NIC istatistikleri                   |

use alloc::vec::Vec;
use alloc::vec;
use core::mem::size_of;

use crate::net::net_device::NET_DEVICE_MANAGER;
use crate::net::ethtool::ETH_TOOL_INTERFACES;
use crate::net::netlink::{NlMsgHdr, NlAttr, NLMSG_DONE, NLM_F_MULTI, NETLINK_MANAGER};

// ============================================================================
// ETHTOOL GENL SABITLERI
// ============================================================================

/// Ethtool generic netlink aile ID (Linux'ta dinamik, echOS'ta sabit)
pub const ETHTOOL_GENL_ID: u16 = 42;

/// Ethtool genl versiyonu
pub const ETHTOOL_GENL_VERSION: u8 = 1;

/// Ethtool netlink komutlari (ETHTOOL_MSG_*) — Linux uapi degerleri
pub const ETHTOOL_MSG_STRSET_GET: u8 = 1;
pub const ETHTOOL_MSG_LINKINFO_GET: u8 = 2;
pub const ETHTOOL_MSG_LINKSTATE_GET: u8 = 6;
pub const ETHTOOL_MSG_STATS_GET: u8 = 32;

/// Ethtool netlink attribute tipleri
pub const ETHTOOL_A_HEADER_DEV_INDEX: u16 = 1;

/// Link state attributes
pub const ETHTOOL_A_LINKSTATE_HEADER: u16 = 1;
pub const ETHTOOL_A_LINKSTATE_LINK: u16 = 2;

/// Link info attributes
pub const ETHTOOL_A_LINKINFO_HEADER: u16 = 1;
pub const ETHTOOL_A_LINKINFO_PORT: u16 = 2;
pub const ETHTOOL_A_LINKINFO_PHYADDR: u16 = 3;
pub const ETHTOOL_A_LINKINFO_TP_MDIX: u16 = 4;

/// Strset (top-level) attributes — Linux ETHTOOL_A_STRSET_*
pub const ETHTOOL_A_STRSET_HEADER: u16 = 1;
pub const ETHTOOL_A_STRSET_STRINGSETS: u16 = 2;
pub const ETHTOOL_A_STRSET_COUNTS_ONLY: u16 = 3;

/// Stringset (nested) attributes — Linux ETHTOOL_A_STRINGSET_*
pub const ETHTOOL_A_STRINGSET_ID: u16 = 1;
pub const ETHTOOL_A_STRINGSET_COUNT: u16 = 2;
pub const ETHTOOL_A_STRINGSET_STRINGS: u16 = 3;

/// String set IDs
pub const ETH_SS_STATS: u32 = 1;

/// Stats attributes — Linux ETHTOOL_A_STATS_* (pad=1, header=2, groups=3, grp=4)
pub const ETHTOOL_A_STATS_HEADER: u16 = 2;
pub const ETHTOOL_A_STATS_GRP: u16 = 4;

// ============================================================================
// GENLMSGHDR (Generic Netlink Message Header)
// ============================================================================

/// Linux `struct genlmsghdr` — 4 byte
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

// ============================================================================
// PAYLOAD PARSING
// ============================================================================

/// Extract a u32 attribute value from a sequence of NlAttr bytes
fn find_attr_u32(payload: &[u8], attr_type: u16) -> Option<u32> {
    let mut pos = 0;
    while pos + 4 <= payload.len() {
        let len = u16::from_le_bytes([payload[pos], payload[pos + 1]]) as usize;
        let typ = u16::from_le_bytes([payload[pos + 2], payload[pos + 3]]);
        if len < 4 { break; }
        let data_start = pos + 4;
        let data_end = pos + len;
        if typ == attr_type && data_end <= payload.len() {
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

/// Build nested header attribute with dev_index
fn build_header_attr(dev_index: u32) -> Vec<u8> {
    NlAttr::new(ETHTOOL_A_HEADER_DEV_INDEX, &dev_index.to_ne_bytes())
}

// ============================================================================
// MAIN HANDLER
// ============================================================================

/// Handle ethtool generic netlink request.
/// Returns (genl_cmd, payload) tuples for response.
pub fn handle_ethtool_genl_request(
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

    let cmd_resp = match cmd {
        ETHTOOL_MSG_LINKSTATE_GET => handle_get_link(attr_payload),
        ETHTOOL_MSG_LINKINFO_GET => handle_get_linkinfo(attr_payload),
        ETHTOOL_MSG_STRSET_GET => handle_get_stringset(attr_payload),
        ETHTOOL_MSG_STATS_GET => handle_get_stats(attr_payload),
        _ => return,
    };

    // Build response messages and route
    let mut responses: Vec<(u8, Vec<u8>)> = cmd_resp;
    responses.push((0, Vec::new())); // NLMSG_DONE equivalent

    for (resp_cmd, resp_payload) in &responses {
        let mut inner = Vec::new();
        let ghdr = GenlMsgHdr::new(*resp_cmd, ETHTOOL_GENL_VERSION);
        inner.extend_from_slice(ghdr.as_bytes());
        inner.extend_from_slice(resp_payload);

        // Check if last (NLMSG_DONE)
        let is_done = resp_payload.is_empty() && *resp_cmd == 0;
        let msg_type = if is_done { 3u16 } else { ETHTOOL_GENL_ID };

        let total_len = (size_of::<NlMsgHdr>() + inner.len()) as u32;
        let reply_hdr = crate::net::netlink::NlMsgHdr::new(total_len, msg_type, if is_done { 0 } else { NLM_F_MULTI }, seq, 0);
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

// ============================================================================
// COMMAND HANDLERS
// ============================================================================

fn handle_get_link(attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let dev_index = find_attr_u32(attr_payload, ETHTOOL_A_HEADER_DEV_INDEX)
        .unwrap_or(0);
    let devices = NET_DEVICE_MANAGER.all();
    let link_up = devices.iter()
        .find(|d| d.dev_id == dev_index)
        .map(|d| d.up.load(core::sync::atomic::Ordering::Acquire))
        .unwrap_or(false);

    let mut payload = Vec::new();
    payload.extend_from_slice(&build_header_attr(dev_index));
    payload.extend_from_slice(&NlAttr::new(ETHTOOL_A_LINKSTATE_LINK, &[link_up as u8]));
    vec![(ETHTOOL_MSG_LINKSTATE_GET, payload)]
}

fn handle_get_linkinfo(attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let dev_index = find_attr_u32(attr_payload, ETHTOOL_A_HEADER_DEV_INDEX)
        .unwrap_or(0);

    // Read from ETH_TOOL_INTERFACES or use defaults
    let (port, phyad) = {
        let interfaces = ETH_TOOL_INTERFACES.lock();
        match interfaces.get(&dev_index) {
            Some(iface) => {
                let li = iface.link_info.lock();
                (li.port as u8, li.phy_addr)
            }
            None => (0u8, 0u8),
        }
    };

    let mut payload = Vec::new();
    payload.extend_from_slice(&build_header_attr(dev_index));
    payload.extend_from_slice(&NlAttr::new(ETHTOOL_A_LINKINFO_PORT, &[port]));
    payload.extend_from_slice(&NlAttr::new(ETHTOOL_A_LINKINFO_PHYADDR, &[phyad]));
    vec![(ETHTOOL_MSG_LINKINFO_GET, payload)]
}

fn handle_get_stringset(attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let dev_index = find_attr_u32(attr_payload, ETHTOOL_A_HEADER_DEV_INDEX)
        .unwrap_or(0);
    let stringset_id = find_attr_u32(attr_payload, ETHTOOL_A_STRINGSET_ID)
        .unwrap_or(ETH_SS_STATS);

    let mut payload = Vec::new();
    payload.extend_from_slice(&build_header_attr(dev_index));
    payload.extend_from_slice(&NlAttr::new(ETHTOOL_A_STRINGSET_ID, &stringset_id.to_ne_bytes()));

    if stringset_id == ETH_SS_STATS {
        // Standard ethtool stats string names
        let stat_names = [
            "rx_packets\0", "tx_packets\0", "rx_bytes\0", "tx_bytes\0",
            "rx_errors\0", "tx_errors\0", "rx_dropped\0", "tx_dropped\0",
        ];
        let count = stat_names.len() as u32;
        payload.extend_from_slice(&NlAttr::new(ETHTOOL_A_STRINGSET_COUNT, &count.to_ne_bytes()));

        let mut strings_data = Vec::new();
        for name in &stat_names {
            strings_data.extend_from_slice(name.as_bytes());
        }
        payload.extend_from_slice(&NlAttr::new(ETHTOOL_A_STRINGSET_STRINGS, &strings_data));
    }

    vec![(ETHTOOL_MSG_STRSET_GET, payload)]
}

fn handle_get_stats(attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let dev_index = find_attr_u32(attr_payload, ETHTOOL_A_HEADER_DEV_INDEX)
        .unwrap_or(0);

    let devices = NET_DEVICE_MANAGER.all();
    let stats = devices.iter()
        .find(|d| d.dev_id == dev_index)
        .map(|d| d.get_stats());

    let mut payload = Vec::new();
    payload.extend_from_slice(&build_header_attr(dev_index));

    if let Some(s) = stats {
        // Pack stats as u64 array for ETHTOOL_A_STATS_GRP
        let stat_values = [
            s.rx_bytes,    s.rx_packets, s.tx_bytes,    s.tx_packets,
            s.rx_errors,   s.tx_errors,  s.rx_dropped,  s.tx_dropped,
        ];
        let mut grp_data = Vec::new();
        for val in &stat_values {
            grp_data.extend_from_slice(&val.to_ne_bytes());
        }
        payload.extend_from_slice(&NlAttr::new(ETHTOOL_A_STATS_GRP, &grp_data));
    }

    vec![(ETHTOOL_MSG_STATS_GET, payload)]
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::MacAddr;
    use crate::net::net_device::NetDevice;
    use alloc::sync::Arc;

    #[test]
    fn test_genlmsghdr_size() {
        assert_eq!(size_of::<GenlMsgHdr>(), 4);
    }

    #[test]
    fn test_ethtool_constants() {
        assert_eq!(ETHTOOL_GENL_ID, 42);
        assert_eq!(ETHTOOL_MSG_STRSET_GET, 1);
        assert_eq!(ETHTOOL_MSG_LINKINFO_GET, 2);
        assert_eq!(ETHTOOL_MSG_LINKSTATE_GET, 6);
        assert_eq!(ETHTOOL_MSG_STATS_GET, 32);

        assert_eq!(ETHTOOL_A_LINKSTATE_LINK, 2);
        assert_eq!(ETHTOOL_A_LINKINFO_PORT, 2);
        assert_eq!(ETHTOOL_A_STRINGSET_ID, 1);
        assert_eq!(ETH_SS_STATS, 1);
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
    fn test_build_header_attr() {
        let attr = build_header_attr(1);
        assert!(!attr.is_empty());
        // NlAttr: 2 bytes len + 2 bytes type = 4-byte header
        let len = u16::from_le_bytes([attr[0], attr[1]]);
        let typ = u16::from_le_bytes([attr[2], attr[3]]);
        assert_eq!(typ, ETHTOOL_A_HEADER_DEV_INDEX);
        assert!(len >= 8); // 4 header + 4 data
    }

    #[test]
    fn test_handle_get_link_unspecified() {
        let result = handle_get_link(&[0u8; 0]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, ETHTOOL_MSG_LINKSTATE_GET);
    }

    #[test]
    fn test_handle_get_link_with_device() {
        let dev = Arc::new(NetDevice::new("eth_genl", MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x10]), 1500));
        NET_DEVICE_MANAGER.register(dev.clone());

        // Build request payload: genlmsghdr + dev_index attr
        let mut req = Vec::new();
        req.extend_from_slice(&build_header_attr(dev.dev_id));

        let result = handle_get_link(&req);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, ETHTOOL_MSG_LINKSTATE_GET);

        // Parse response to find ETHTOOL_A_LINKSTATE_LINK
        let payload = &result[0].1;
        let link_val = find_attr_u32(payload, ETHTOOL_A_LINKSTATE_LINK);
        assert_eq!(link_val, Some(0)); // device is down

        NET_DEVICE_MANAGER.unregister("eth_genl");
    }

    #[test]
    fn test_handle_get_linkinfo_unspecified() {
        let result = handle_get_linkinfo(&[0u8; 0]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, ETHTOOL_MSG_LINKINFO_GET);
    }

    #[test]
    fn test_handle_get_stringset_default() {
        // Build request with no attributes (uses defaults: ETH_SS_STATS)
        let result = handle_get_stringset(&[0u8; 0]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, ETHTOOL_MSG_STRSET_GET);

        // Check that stringset count and strings exist
        let payload = &result[0].1;
        let count = find_attr_u32(payload, ETHTOOL_A_STRINGSET_COUNT);
        assert_eq!(count, Some(8)); // 8 stat names
    }

    #[test]
    fn test_handle_get_stats_unspecified() {
        let result = handle_get_stats(&[0u8; 0]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, ETHTOOL_MSG_STATS_GET);
    }

    #[test]
    fn test_handle_get_stats_with_device() {
        let dev = Arc::new(NetDevice::new("eth_stats_genl", MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x11]), 1500));
        NET_DEVICE_MANAGER.register(dev.clone());

        let result = handle_get_stats(&build_header_attr(dev.dev_id));
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, ETHTOOL_MSG_STATS_GET);

        // Verify stats group exists
        let payload = &result[0].1;
        let mut pos = 0;
        let mut found_grp = false;
        while pos + 4 <= payload.len() {
            let len = u16::from_le_bytes([payload[pos], payload[pos+1]]);
            let typ = u16::from_le_bytes([payload[pos+2], payload[pos+3]]);
            if typ == ETHTOOL_A_STATS_GRP { found_grp = true; break; }
            if len == 0 { break; }
            pos += len as usize;
        }
        assert!(found_grp, "ETHTOOL_A_STATS_GRP not found");

        NET_DEVICE_MANAGER.unregister("eth_stats_genl");
    }
}
