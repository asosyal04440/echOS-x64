//! # Devlink Generic Netlink — NETLINK_GENERIC ailesi
//!
//! Linux `devlink` aygit yönetim arayüzünün netlink üzerinden çalışan
//! modern arayüzü. Kullanıcı uzayı, `DEVLINK_CMD_*` komutlarını
//! `DEVLINK_GENL_ID` family ID'si ile generic netlink üzerinden gönderir.
//!
//! ## Desteklenen komutlar
//!
//! | Komut                          | ID | Dönen bilgi                       |
//! |--------------------------------|----|-----------------------------------|
//! | DEVLINK_CMD_GET                | 1  | Cihaz listesi (bus/dev)           |
//! | DEVLINK_CMD_INFO_GET           | 51 | Sürücü/ürün-yazılım sürüm bilgisi |
//! | DEVLINK_CMD_HEALTH_REPORTER_GET| 52 | Sağlık raporlayıcı durumu         |

use alloc::vec::Vec;
use alloc::vec;
use alloc::format;
use core::mem::size_of;

use crate::net::net_device::NET_DEVICE_MANAGER;
use crate::net::netlink::{NlMsgHdr, NlAttr, NETLINK_MANAGER};

// ============================================================================
// DEVLINK GENL SABITLERI (Linux include/uapi/linux/devlink.h)
// ============================================================================

/// Devlink generic netlink aile ID (Linux'ta dinamik, echOS'ta sabit)
pub const DEVLINK_GENL_ID: u16 = 43;

/// Devlink genl versiyonu
pub const DEVLINK_GENL_VERSION: u8 = 1;

/// Devlink netlink komutları (Linux enum devlink_command)
pub const DEVLINK_CMD_UNSPEC: u8 = 0;
pub const DEVLINK_CMD_GET: u8 = 1;
pub const DEVLINK_CMD_SET: u8 = 2;
pub const DEVLINK_CMD_NEW: u8 = 3;
pub const DEVLINK_CMD_DEL: u8 = 4;
pub const DEVLINK_CMD_PORT_GET: u8 = 5;
pub const DEVLINK_CMD_PORT_SET: u8 = 6;
pub const DEVLINK_CMD_PORT_NEW: u8 = 7;
pub const DEVLINK_CMD_PORT_DEL: u8 = 8;
pub const DEVLINK_CMD_PORT_SPLIT: u8 = 9;
pub const DEVLINK_CMD_PORT_UNSPLIT: u8 = 10;
pub const DEVLINK_CMD_SB_GET: u8 = 11;
pub const DEVLINK_CMD_SB_SET: u8 = 12;
pub const DEVLINK_CMD_SB_NEW: u8 = 13;
pub const DEVLINK_CMD_SB_DEL: u8 = 14;
pub const DEVLINK_CMD_SB_POOL_GET: u8 = 15;
pub const DEVLINK_CMD_SB_POOL_SET: u8 = 16;
pub const DEVLINK_CMD_SB_POOL_NEW: u8 = 17;
pub const DEVLINK_CMD_SB_POOL_DEL: u8 = 18;
pub const DEVLINK_CMD_SB_PORT_POOL_GET: u8 = 19;
pub const DEVLINK_CMD_SB_PORT_POOL_SET: u8 = 20;
pub const DEVLINK_CMD_SB_PORT_POOL_NEW: u8 = 21;
pub const DEVLINK_CMD_SB_PORT_POOL_DEL: u8 = 22;
pub const DEVLINK_CMD_SB_TC_POOL_BIND_GET: u8 = 23;
pub const DEVLINK_CMD_SB_TC_POOL_BIND_SET: u8 = 24;
pub const DEVLINK_CMD_SB_TC_POOL_BIND_NEW: u8 = 25;
pub const DEVLINK_CMD_SB_TC_POOL_BIND_DEL: u8 = 26;
pub const DEVLINK_CMD_SB_OCC_SNAPSHOT: u8 = 27;
pub const DEVLINK_CMD_SB_OCC_MAX_CLEAR: u8 = 28;
pub const DEVLINK_CMD_ESWITCH_GET: u8 = 29;
pub const DEVLINK_CMD_ESWITCH_SET: u8 = 30;
pub const DEVLINK_CMD_DPIPE_TABLE_GET: u8 = 31;
pub const DEVLINK_CMD_DPIPE_ENTRIES_GET: u8 = 32;
pub const DEVLINK_CMD_DPIPE_HEADERS_GET: u8 = 33;
pub const DEVLINK_CMD_DPIPE_TABLE_COUNTERS_SET: u8 = 34;
pub const DEVLINK_CMD_RESOURCE_SET: u8 = 35;
pub const DEVLINK_CMD_RESOURCE_DUMP: u8 = 36;
pub const DEVLINK_CMD_RELOAD: u8 = 37;
pub const DEVLINK_CMD_PARAM_GET: u8 = 38;
pub const DEVLINK_CMD_PARAM_SET: u8 = 39;
pub const DEVLINK_CMD_PARAM_NEW: u8 = 40;
pub const DEVLINK_CMD_PARAM_DEL: u8 = 41;
pub const DEVLINK_CMD_REGION_GET: u8 = 42;
pub const DEVLINK_CMD_REGION_SET: u8 = 43;
pub const DEVLINK_CMD_REGION_NEW: u8 = 44;
pub const DEVLINK_CMD_REGION_DEL: u8 = 45;
pub const DEVLINK_CMD_REGION_READ: u8 = 46;
pub const DEVLINK_CMD_PORT_PARAM_GET: u8 = 47;
pub const DEVLINK_CMD_PORT_PARAM_SET: u8 = 48;
pub const DEVLINK_CMD_PORT_PARAM_NEW: u8 = 49;
pub const DEVLINK_CMD_PORT_PARAM_DEL: u8 = 50;
pub const DEVLINK_CMD_INFO_GET: u8 = 51;
pub const DEVLINK_CMD_HEALTH_REPORTER_GET: u8 = 52;
pub const DEVLINK_CMD_HEALTH_REPORTER_SET: u8 = 53;
pub const DEVLINK_CMD_HEALTH_REPORTER_RECOVER: u8 = 54;
pub const DEVLINK_CMD_HEALTH_REPORTER_DIAGNOSE: u8 = 55;
pub const DEVLINK_CMD_HEALTH_REPORTER_DUMP_GET: u8 = 56;
pub const DEVLINK_CMD_HEALTH_REPORTER_DUMP_CLEAR: u8 = 57;
pub const DEVLINK_CMD_FLASH_UPDATE: u8 = 58;
pub const DEVLINK_CMD_FLASH_UPDATE_END: u8 = 59;
pub const DEVLINK_CMD_FLASH_UPDATE_STATUS: u8 = 60;
pub const DEVLINK_CMD_TRAP_GET: u8 = 61;
pub const DEVLINK_CMD_TRAP_SET: u8 = 62;
pub const DEVLINK_CMD_TRAP_NEW: u8 = 63;
pub const DEVLINK_CMD_TRAP_DEL: u8 = 64;
pub const DEVLINK_CMD_TRAP_GROUP_GET: u8 = 65;
pub const DEVLINK_CMD_TRAP_GROUP_SET: u8 = 66;
pub const DEVLINK_CMD_TRAP_GROUP_NEW: u8 = 67;
pub const DEVLINK_CMD_TRAP_GROUP_DEL: u8 = 68;
pub const DEVLINK_CMD_TRAP_POLICER_GET: u8 = 69;
pub const DEVLINK_CMD_TRAP_POLICER_SET: u8 = 70;
pub const DEVLINK_CMD_TRAP_POLICER_NEW: u8 = 71;
pub const DEVLINK_CMD_TRAP_POLICER_DEL: u8 = 72;
pub const DEVLINK_CMD_HEALTH_REPORTER_TEST: u8 = 73;
pub const DEVLINK_CMD_RATE_GET: u8 = 74;
pub const DEVLINK_CMD_RATE_SET: u8 = 75;
pub const DEVLINK_CMD_RATE_NEW: u8 = 76;
pub const DEVLINK_CMD_RATE_DEL: u8 = 77;
pub const DEVLINK_CMD_LINECARD_GET: u8 = 78;
pub const DEVLINK_CMD_LINECARD_SET: u8 = 79;
pub const DEVLINK_CMD_LINECARD_NEW: u8 = 80;
pub const DEVLINK_CMD_LINECARD_DEL: u8 = 81;
pub const DEVLINK_CMD_SELFTESTS_GET: u8 = 82;
pub const DEVLINK_CMD_SELFTESTS_RUN: u8 = 83;
pub const DEVLINK_CMD_NOTIFY_FILTER_SET: u8 = 84;

/// Devlink attribute tipleri (bus-name + dev-name = cihaz tanıtıcısı)
pub const DEVLINK_ATTR_BUS_NAME: u16 = 1;
pub const DEVLINK_ATTR_DEV_NAME: u16 = 2;
pub const DEVLINK_ATTR_INDEX: u16 = 184;

/// Devlink info-get attribute'leri
pub const DEVLINK_ATTR_INFO_DRIVER_NAME: u16 = 98;
pub const DEVLINK_ATTR_INFO_SERIAL_NUMBER: u16 = 99;
pub const DEVLINK_ATTR_INFO_VERSION_FIXED: u16 = 100;
pub const DEVLINK_ATTR_INFO_VERSION_RUNNING: u16 = 101;
pub const DEVLINK_ATTR_INFO_VERSION_STORED: u16 = 102;
pub const DEVLINK_ATTR_INFO_VERSION_NAME: u16 = 103;
pub const DEVLINK_ATTR_INFO_VERSION_VALUE: u16 = 104;
pub const DEVLINK_ATTR_INFO_BOARD_SERIAL_NUMBER: u16 = 146;

/// Devlink health-reporter attribute'leri
pub const DEVLINK_ATTR_HEALTH_REPORTER: u16 = 114;
pub const DEVLINK_ATTR_HEALTH_REPORTER_NAME: u16 = 115;
pub const DEVLINK_ATTR_HEALTH_REPORTER_STATE: u16 = 116;
pub const DEVLINK_ATTR_HEALTH_REPORTER_ERR_COUNT: u16 = 117;
pub const DEVLINK_ATTR_HEALTH_REPORTER_RECOVER_COUNT: u16 = 118;

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

/// Find a NlAttr by type and return its data bytes (len + type stripped)
fn find_attr_raw(payload: &[u8], attr_type: u16) -> Option<&[u8]> {
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

/// Build a devlink dev identifier (nested attr with bus-name + dev-name)
fn build_dev_id_attrs(bus_name: &str, dev_name: &str) -> Vec<u8> {
    let mut attrs = Vec::new();
    attrs.extend_from_slice(&NlAttr::new(DEVLINK_ATTR_BUS_NAME, bus_name.as_bytes()));
    attrs.extend_from_slice(&NlAttr::new(DEVLINK_ATTR_DEV_NAME, dev_name.as_bytes()));
    attrs
}

// ============================================================================
// MAIN HANDLER
// ============================================================================

/// Handle devlink generic netlink request.
pub fn handle_devlink_genl_request(
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
        DEVLINK_CMD_GET => handle_dev_get(attr_payload),
        DEVLINK_CMD_INFO_GET => handle_info_get(attr_payload),
        DEVLINK_CMD_HEALTH_REPORTER_GET => handle_health_reporter_get(attr_payload),
        _ => return,
    };

    // Build response messages and route
    let mut responses: Vec<(u8, Vec<u8>)> = cmd_resp;
    responses.push((0, Vec::new())); // NLMSG_DONE equivalent

    for (resp_cmd, resp_payload) in &responses {
        let mut inner = Vec::new();
        let ghdr = GenlMsgHdr::new(*resp_cmd, DEVLINK_GENL_VERSION);
        inner.extend_from_slice(ghdr.as_bytes());
        inner.extend_from_slice(resp_payload);

        let is_done = resp_payload.is_empty() && *resp_cmd == 0;
        let msg_type = if is_done { 3u16 } else { DEVLINK_GENL_ID };

        let total_len = (size_of::<NlMsgHdr>() + inner.len()) as u32;
        let reply_hdr = NlMsgHdr::new(total_len, msg_type, if is_done { 0 } else { 2u16 }, seq, 0);
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

fn handle_dev_get(attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let mut responses = Vec::new();
    let devices = NET_DEVICE_MANAGER.all();
    for dev in &devices {
        // Response payload: bus-name + dev-name + index
        let mut payload = Vec::new();
        payload.extend_from_slice(&NlAttr::new(DEVLINK_ATTR_BUS_NAME, b"sim\0"));
        let dev_name = format!("{}\0", dev.name);
        payload.extend_from_slice(&NlAttr::new(DEVLINK_ATTR_DEV_NAME, dev_name.as_bytes()));
        payload.extend_from_slice(&NlAttr::new(DEVLINK_ATTR_INDEX, &dev.dev_id.to_ne_bytes()));
        responses.push((DEVLINK_CMD_GET, payload));
    }
    if responses.is_empty() {
        // Return a minimal response even without known devices
        let mut payload = Vec::new();
        payload.extend_from_slice(&NlAttr::new(DEVLINK_ATTR_BUS_NAME, b"sim\0"));
        payload.extend_from_slice(&NlAttr::new(DEVLINK_ATTR_DEV_NAME, b"lo\0"));
        let index = 1u32;
        payload.extend_from_slice(&NlAttr::new(DEVLINK_ATTR_INDEX, &index.to_ne_bytes()));
        responses.push((DEVLINK_CMD_GET, payload));
    }
    responses
}

fn handle_info_get(attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let bus_name = find_attr_raw(attr_payload, DEVLINK_ATTR_BUS_NAME)
        .and_then(|d| core::str::from_utf8(d).ok())
        .unwrap_or("sim");
    let _dev_name = find_attr_raw(attr_payload, DEVLINK_ATTR_DEV_NAME)
        .and_then(|d| core::str::from_utf8(d).ok())
        .unwrap_or("unknown");

    let mut payload = Vec::new();
    payload.extend_from_slice(&NlAttr::new(DEVLINK_ATTR_BUS_NAME, bus_name.as_bytes()));

    // echOS driver bilgisi
    payload.extend_from_slice(&NlAttr::new(DEVLINK_ATTR_INFO_DRIVER_NAME, b"echOS\0"));

    // Sabit donanım sürümü
    let fixed = build_info_version("hw.version", "1.0");
    payload.extend_from_slice(&NlAttr::new(DEVLINK_ATTR_INFO_VERSION_FIXED, &fixed));

    // Çalışan yazılım sürümü (kernel/firmware)
    let running = build_info_version("fw.version", "7.0-echOS");
    payload.extend_from_slice(&NlAttr::new(DEVLINK_ATTR_INFO_VERSION_RUNNING, &running));
    let running_kernel = build_info_version("kernel", "1.0.0");
    payload.extend_from_slice(&NlAttr::new(DEVLINK_ATTR_INFO_VERSION_RUNNING, &running_kernel));

    // Depolanmış (stored) sürüm bilgisi
    let stored = build_info_version("stored.fw", "7.0-echOS-stable");
    payload.extend_from_slice(&NlAttr::new(DEVLINK_ATTR_INFO_VERSION_STORED, &stored));

    vec![(DEVLINK_CMD_INFO_GET, payload)]
}

/// Build an info-version nested attribute (dl-info-version sub-attribute-set)
fn build_info_version(name: &str, value: &str) -> Vec<u8> {
    let mut nested = Vec::new();
    nested.extend_from_slice(&NlAttr::new(DEVLINK_ATTR_INFO_VERSION_NAME, name.as_bytes()));
    nested.extend_from_slice(&NlAttr::new(DEVLINK_ATTR_INFO_VERSION_VALUE, value.as_bytes()));
    nested
}

fn handle_health_reporter_get(attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let bus_name = find_attr_raw(attr_payload, DEVLINK_ATTR_BUS_NAME)
        .and_then(|d| core::str::from_utf8(d).ok())
        .unwrap_or("sim");

    let mut payload = Vec::new();
    payload.extend_from_slice(&NlAttr::new(DEVLINK_ATTR_BUS_NAME, bus_name.as_bytes()));

    // Health reporter: driver
    let mut report = Vec::new();
    report.extend_from_slice(&NlAttr::new(DEVLINK_ATTR_HEALTH_REPORTER_NAME, b"driver\0"));
    report.extend_from_slice(&NlAttr::new(DEVLINK_ATTR_HEALTH_REPORTER_STATE, &[1u8])); // HEALTHY=1
    let err_count = 0u64;
    report.extend_from_slice(&NlAttr::new(DEVLINK_ATTR_HEALTH_REPORTER_ERR_COUNT, &err_count.to_ne_bytes()));
    let recov_count = 0u64;
    report.extend_from_slice(&NlAttr::new(DEVLINK_ATTR_HEALTH_REPORTER_RECOVER_COUNT, &recov_count.to_ne_bytes()));
    payload.extend_from_slice(&NlAttr::new(DEVLINK_ATTR_HEALTH_REPORTER, &report));

    // Health reporter: tx
    let mut tx_report = Vec::new();
    tx_report.extend_from_slice(&NlAttr::new(DEVLINK_ATTR_HEALTH_REPORTER_NAME, b"tx\0"));
    tx_report.extend_from_slice(&NlAttr::new(DEVLINK_ATTR_HEALTH_REPORTER_STATE, &[1u8]));
    payload.extend_from_slice(&NlAttr::new(DEVLINK_ATTR_HEALTH_REPORTER, &tx_report));

    // Health reporter: rx
    let mut rx_report = Vec::new();
    rx_report.extend_from_slice(&NlAttr::new(DEVLINK_ATTR_HEALTH_REPORTER_NAME, b"rx\0"));
    rx_report.extend_from_slice(&NlAttr::new(DEVLINK_ATTR_HEALTH_REPORTER_STATE, &[1u8]));
    payload.extend_from_slice(&NlAttr::new(DEVLINK_ATTR_HEALTH_REPORTER, &rx_report));

    vec![(DEVLINK_CMD_HEALTH_REPORTER_GET, payload)]
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
    fn test_devlink_constants() {
        assert_eq!(DEVLINK_GENL_ID, 43);
        assert_eq!(DEVLINK_CMD_GET, 1);
        assert_eq!(DEVLINK_CMD_INFO_GET, 51);
        assert_eq!(DEVLINK_CMD_HEALTH_REPORTER_GET, 52);

        assert_eq!(DEVLINK_ATTR_BUS_NAME, 1);
        assert_eq!(DEVLINK_ATTR_DEV_NAME, 2);
        assert_eq!(DEVLINK_ATTR_INDEX, 184);
        assert_eq!(DEVLINK_ATTR_INFO_DRIVER_NAME, 98);
        assert_eq!(DEVLINK_ATTR_INFO_VERSION_VALUE, 104);
        assert_eq!(DEVLINK_ATTR_HEALTH_REPORTER, 114);
        assert_eq!(DEVLINK_ATTR_HEALTH_REPORTER_STATE, 116);
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
    fn test_find_attr_raw() {
        let mut data = Vec::new();
        data.extend_from_slice(&NlAttr::new(10, b"hello"));
        data.extend_from_slice(&NlAttr::new(11, b"world"));
        assert_eq!(find_attr_raw(&data, 10), Some(&b"hello"[..]));
        assert_eq!(find_attr_raw(&data, 11), Some(&b"world"[..]));
        assert_eq!(find_attr_raw(&data, 12), None);
    }

    #[test]
    fn test_handle_dev_get_empty() {
        let result = handle_dev_get(&[0u8; 0]);
        // Should return at least one response (the fallback "lo" device)
        assert!(!result.is_empty(), "Empty dev_get should return fallback");
        assert_eq!(result[0].0, DEVLINK_CMD_GET);
    }

    #[test]
    fn test_handle_dev_get_with_device() {
        let dev = Arc::new(NetDevice::new("devl_test", MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x20]), 1500));
        NET_DEVICE_MANAGER.register(dev.clone());

        let result = handle_dev_get(&[0u8; 0]);
        assert!(!result.is_empty());
        assert_eq!(result[0].0, DEVLINK_CMD_GET);

        // Check that bus-name attr exists in response
        let found_bus = result.iter().any(|(_, p)| find_attr_raw(p, DEVLINK_ATTR_BUS_NAME).is_some());
        assert!(found_bus, "Response should contain DEVLINK_ATTR_BUS_NAME");

        NET_DEVICE_MANAGER.unregister("devl_test");
    }

    #[test]
    fn test_handle_info_get_default() {
        let result = handle_info_get(&[0u8; 0]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, DEVLINK_CMD_INFO_GET);

        // Check presence of driver-name attribute
        let payload = &result[0].1;
        let driver_name = find_attr_raw(payload, DEVLINK_ATTR_INFO_DRIVER_NAME);
        assert!(driver_name.is_some(), "info-get should contain info-driver-name");
    }

    #[test]
    fn test_handle_health_reporter_get_default() {
        let result = handle_health_reporter_get(&[0u8; 0]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, DEVLINK_CMD_HEALTH_REPORTER_GET);

        let payload = &result[0].1;
        // Should contain at least one health-reporter nested attribute
        let health = find_attr_raw(payload, DEVLINK_ATTR_HEALTH_REPORTER);
        assert!(health.is_some(), "Response should contain DEVLINK_ATTR_HEALTH_REPORTER");
    }

    #[test]
    fn test_build_info_version() {
        let result = build_info_version("test.name", "test.val");
        assert!(!result.is_empty());
        // Should contain version-name and version-value attributes
        let name = find_attr_raw(&result, DEVLINK_ATTR_INFO_VERSION_NAME);
        let val = find_attr_raw(&result, DEVLINK_ATTR_INFO_VERSION_VALUE);
        assert!(name.is_some(), "build_info_version should contain version-name");
        assert!(val.is_some(), "build_info_version should contain version-value");
    }
}
