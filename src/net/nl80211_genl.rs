use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::vec;
use core::mem::size_of;
use spin::Mutex;

use crate::net::netlink::{NlMsgHdr, NlAttr, NETLINK_MANAGER};
use crate::net::wireless::*;

pub const NL80211_GENL_ID: u16 = 33;
pub const NL80211_GENL_VERSION: u8 = 1;

pub const NL80211_CMD_UNSPEC: u8 = 0;
pub const NL80211_CMD_GET_WIPHY: u8 = 1;
pub const NL80211_CMD_SET_WIPHY: u8 = 2;
pub const NL80211_CMD_NEW_WIPHY: u8 = 3;
pub const NL80211_CMD_DEL_WIPHY: u8 = 4;
pub const NL80211_CMD_GET_INTERFACE: u8 = 5;
pub const NL80211_CMD_SET_INTERFACE: u8 = 6;
pub const NL80211_CMD_NEW_INTERFACE: u8 = 7;
pub const NL80211_CMD_DEL_INTERFACE: u8 = 8;
pub const NL80211_CMD_GET_KEY: u8 = 9;
pub const NL80211_CMD_SET_KEY: u8 = 10;
pub const NL80211_CMD_NEW_KEY: u8 = 11;
pub const NL80211_CMD_DEL_KEY: u8 = 12;
pub const NL80211_CMD_GET_BEACON: u8 = 13;
pub const NL80211_CMD_SET_BEACON: u8 = 14;
pub const NL80211_CMD_NEW_BEACON: u8 = 15;
pub const NL80211_CMD_DEL_BEACON: u8 = 16;
pub const NL80211_CMD_GET_STATION: u8 = 17;
pub const NL80211_CMD_SET_STATION: u8 = 18;
pub const NL80211_CMD_NEW_STATION: u8 = 19;
pub const NL80211_CMD_DEL_STATION: u8 = 20;
pub const NL80211_CMD_GET_MPATH: u8 = 21;
pub const NL80211_CMD_SET_MPATH: u8 = 22;
pub const NL80211_CMD_NEW_MPATH: u8 = 23;
pub const NL80211_CMD_DEL_MPATH: u8 = 24;
pub const NL80211_CMD_SET_BSS: u8 = 25;
pub const NL80211_CMD_SET_REG: u8 = 26;
pub const NL80211_CMD_REQ_SET_REG: u8 = 27;
pub const NL80211_CMD_GET_MESH_CONFIG: u8 = 28;
pub const NL80211_CMD_SET_MESH_CONFIG: u8 = 29;
pub const NL80211_CMD_SET_MGMT_EXTRA_IE: u8 = 30;
pub const NL80211_CMD_GET_REG: u8 = 31;
pub const NL80211_CMD_GET_SCAN: u8 = 32;
pub const NL80211_CMD_TRIGGER_SCAN: u8 = 33;
pub const NL80211_CMD_NEW_SCAN_RESULTS: u8 = 34;
pub const NL80211_CMD_SCAN_ABORTED: u8 = 35;
pub const NL80211_CMD_REG_CHANGE: u8 = 36;
pub const NL80211_CMD_AUTHENTICATE: u8 = 37;
pub const NL80211_CMD_ASSOCIATE: u8 = 38;
pub const NL80211_CMD_DEAUTHENTICATE: u8 = 39;
pub const NL80211_CMD_DISASSOCIATE: u8 = 40;
pub const NL80211_CMD_MICHAEL_MIC_FAILURE: u8 = 41;
pub const NL80211_CMD_REG_BEACON_HINT: u8 = 42;
pub const NL80211_CMD_JOIN_IBSS: u8 = 43;
pub const NL80211_CMD_LEAVE_IBSS: u8 = 44;
pub const NL80211_CMD_TESTMODE: u8 = 45;
pub const NL80211_CMD_CONNECT: u8 = 46;
pub const NL80211_CMD_ROAM: u8 = 47;
pub const NL80211_CMD_DISCONNECT: u8 = 48;
pub const NL80211_CMD_SET_WIPHY_NETNS: u8 = 49;
pub const NL80211_CMD_GET_SURVEY: u8 = 50;
pub const NL80211_CMD_NEW_SURVEY_RESULTS: u8 = 51;
pub const NL80211_CMD_SET_PMKSA: u8 = 52;
pub const NL80211_CMD_DEL_PMKSA: u8 = 53;
pub const NL80211_CMD_FLUSH_PMKSA: u8 = 54;
pub const NL80211_CMD_REMAIN_ON_CHANNEL: u8 = 55;
pub const NL80211_CMD_CANCEL_REMAIN_ON_CHANNEL: u8 = 56;
pub const NL80211_CMD_SET_TX_BITRATE_MASK: u8 = 57;
pub const NL80211_CMD_REGISTER_ACTION: u8 = 58;
pub const NL80211_CMD_ACTION: u8 = 59;
pub const NL80211_CMD_ACTION_TX_STATUS: u8 = 60;
pub const NL80211_CMD_SET_POWER_SAVE: u8 = 61;
pub const NL80211_CMD_GET_POWER_SAVE: u8 = 62;
pub const NL80211_CMD_SET_CQM: u8 = 63;
pub const NL80211_CMD_NOTIFY_CQM: u8 = 64;
pub const NL80211_CMD_SET_CHANNEL: u8 = 65;
pub const NL80211_CMD_SET_WDS_PEER: u8 = 66;
pub const NL80211_CMD_FRAME_WAIT_CANCEL: u8 = 67;
pub const NL80211_CMD_JOIN_MESH: u8 = 68;
pub const NL80211_CMD_LEAVE_MESH: u8 = 69;
pub const NL80211_CMD_UNPROT_DEAUTHENTICATE: u8 = 70;
pub const NL80211_CMD_UNPROT_DISASSOCIATE: u8 = 71;
pub const NL80211_CMD_NEW_PEER_CANDIDATE: u8 = 72;
pub const NL80211_CMD_GET_WOWLAN: u8 = 73;
pub const NL80211_CMD_SET_WOWLAN: u8 = 74;
pub const NL80211_CMD_START_SCHED_SCAN: u8 = 75;
pub const NL80211_CMD_STOP_SCHED_SCAN: u8 = 76;
pub const NL80211_CMD_SCHED_SCAN_RESULTS: u8 = 77;
pub const NL80211_CMD_SCHED_SCAN_STOPPED: u8 = 78;
pub const NL80211_CMD_SET_REKEY_OFFLOAD: u8 = 79;
pub const NL80211_CMD_PMKSA_CANDIDATE: u8 = 80;
pub const NL80211_CMD_TDLS_OPER: u8 = 81;
pub const NL80211_CMD_TDLS_MGMT: u8 = 82;
pub const NL80211_CMD_UNEXPECTED_FRAME: u8 = 83;
pub const NL80211_CMD_PROBE_CLIENT: u8 = 84;
pub const NL80211_CMD_REGISTER_BEACONS: u8 = 85;
pub const NL80211_CMD_UNEXPECTED_4ADDR_FRAME: u8 = 86;
pub const NL80211_CMD_SET_NOACK_MAP: u8 = 87;
pub const NL80211_CMD_CH_SWITCH_NOTIFY: u8 = 88;
pub const NL80211_CMD_START_P2P_DEVICE: u8 = 89;
pub const NL80211_CMD_STOP_P2P_DEVICE: u8 = 90;
pub const NL80211_CMD_CONN_FAILED: u8 = 91;
pub const NL80211_CMD_SET_MCAST_RATE: u8 = 92;
pub const NL80211_CMD_SET_MAC_ACL: u8 = 93;
pub const NL80211_CMD_RADAR_DETECT: u8 = 94;
pub const NL80211_CMD_GET_PROTOCOL_FEATURES: u8 = 95;
pub const NL80211_CMD_UPDATE_FT_IES: u8 = 96;
pub const NL80211_CMD_FT_EVENT: u8 = 97;
pub const NL80211_CMD_CRIT_PROTOCOL_START: u8 = 98;
pub const NL80211_CMD_CRIT_PROTOCOL_STOP: u8 = 99;
pub const NL80211_CMD_GET_COALESCE: u8 = 100;
pub const NL80211_CMD_SET_COALESCE: u8 = 101;
pub const NL80211_CMD_CHANNEL_SWITCH: u8 = 102;
pub const NL80211_CMD_VENDOR: u8 = 103;
pub const NL80211_CMD_SET_QOS_MAP: u8 = 104;
pub const NL80211_CMD_ADD_TX_TS: u8 = 105;
pub const NL80211_CMD_DEL_TX_TS: u8 = 106;
pub const NL80211_CMD_GET_MPP: u8 = 107;
pub const NL80211_CMD_JOIN_OCB: u8 = 108;
pub const NL80211_CMD_LEAVE_OCB: u8 = 109;
pub const NL80211_CMD_CH_SWITCH_STARTED_NOTIFY: u8 = 110;
pub const NL80211_CMD_TDLS_CHANNEL_SWITCH: u8 = 111;
pub const NL80211_CMD_TDLS_CANCEL_CHANNEL_SWITCH: u8 = 112;
pub const NL80211_CMD_WIPHY_REG_CHANGE: u8 = 113;
pub const NL80211_CMD_ABORT_SCAN: u8 = 114;
pub const NL80211_CMD_START_NAN: u8 = 115;
pub const NL80211_CMD_STOP_NAN: u8 = 116;
pub const NL80211_CMD_ADD_NAN_FUNCTION: u8 = 117;
pub const NL80211_CMD_DEL_NAN_FUNCTION: u8 = 118;
pub const NL80211_CMD_CHANGE_NAN_CONFIG: u8 = 119;
pub const NL80211_CMD_NAN_MATCH: u8 = 120;
pub const NL80211_CMD_SET_MULTICAST_TO_UNICAST: u8 = 121;
pub const NL80211_CMD_UPDATE_CONNECT_PARAMS: u8 = 122;
pub const NL80211_CMD_SET_PMK: u8 = 123;
pub const NL80211_CMD_DEL_PMK: u8 = 124;
pub const NL80211_CMD_PORT_AUTHORIZED: u8 = 125;
pub const NL80211_CMD_RELOAD_REGDB: u8 = 126;
pub const NL80211_CMD_EXTERNAL_AUTH: u8 = 127;
pub const NL80211_CMD_STA_OPMODE_CHANGED: u8 = 128;
pub const NL80211_CMD_CONTROL_PORT_FRAME: u8 = 129;
pub const NL80211_CMD_GET_FTM_RESPONDER_STATS: u8 = 130;
pub const NL80211_CMD_PEER_MEASUREMENT_START: u8 = 131;
pub const NL80211_CMD_PEER_MEASUREMENT_RESULT: u8 = 132;
pub const NL80211_CMD_PEER_MEASUREMENT_COMPLETE: u8 = 133;
pub const NL80211_CMD_NOTIFY_RADAR: u8 = 134;
pub const NL80211_CMD_UPDATE_OWE_INFO: u8 = 135;
pub const NL80211_CMD_PROBE_MESH_LINK: u8 = 136;
pub const NL80211_CMD_SET_TID_CONFIG: u8 = 137;
pub const NL80211_CMD_UNPROT_BEACON: u8 = 138;
pub const NL80211_CMD_CONTROL_PORT_FRAME_TX_STATUS: u8 = 139;
pub const NL80211_CMD_SET_SAR_SPECS: u8 = 140;
pub const NL80211_CMD_OBSS_COLOR_COLLISION: u8 = 141;
pub const NL80211_CMD_COLOR_CHANGE_REQUEST: u8 = 142;
pub const NL80211_CMD_COLOR_CHANGE_STARTED: u8 = 143;
pub const NL80211_CMD_COLOR_CHANGE_ABORTED: u8 = 144;
pub const NL80211_CMD_COLOR_CHANGE_COMPLETED: u8 = 145;
pub const NL80211_CMD_SET_FILS_AAD: u8 = 146;
pub const NL80211_CMD_ASSOC_COMEBACK: u8 = 147;
pub const NL80211_CMD_ADD_LINK: u8 = 148;
pub const NL80211_CMD_REMOVE_LINK: u8 = 149;
pub const NL80211_CMD_ADD_LINK_STA: u8 = 150;
pub const NL80211_CMD_MODIFY_LINK_STA: u8 = 151;
pub const NL80211_CMD_REMOVE_LINK_STA: u8 = 152;
pub const NL80211_CMD_SET_HW_TIMESTAMP: u8 = 153;
pub const NL80211_CMD_LINKS_REMOVED: u8 = 154;
pub const NL80211_CMD_SET_TID_TO_LINK_MAPPING: u8 = 155;

pub const NL80211_ATTR_WIPHY: u16 = 1;
pub const NL80211_ATTR_WIPHY_NAME: u16 = 2;
pub const NL80211_ATTR_IFINDEX: u16 = 3;
pub const NL80211_ATTR_IFNAME: u16 = 4;
pub const NL80211_ATTR_IFTYPE: u16 = 5;
pub const NL80211_ATTR_MAC: u16 = 6;
pub const NL80211_ATTR_KEY_DATA: u16 = 7;
pub const NL80211_ATTR_KEY_IDX: u16 = 8;
pub const NL80211_ATTR_KEY_CIPHER: u16 = 9;
pub const NL80211_ATTR_KEY_SEQ: u16 = 10;
pub const NL80211_ATTR_KEY_DEFAULT: u16 = 11;
pub const NL80211_ATTR_BEACON_INTERVAL: u16 = 12;
pub const NL80211_ATTR_DTIM_PERIOD: u16 = 13;
pub const NL80211_ATTR_BEACON_HEAD: u16 = 14;
pub const NL80211_ATTR_BEACON_TAIL: u16 = 15;
pub const NL80211_ATTR_STA_AID: u16 = 16;
pub const NL80211_ATTR_STA_FLAGS: u16 = 17;
pub const NL80211_ATTR_STA_LISTEN_INTERVAL: u16 = 18;
pub const NL80211_ATTR_STA_SUPPORTED_RATES: u16 = 19;
pub const NL80211_ATTR_STA_VLAN: u16 = 20;
pub const NL80211_ATTR_STA_INFO: u16 = 21;
pub const NL80211_ATTR_WIPHY_BANDS: u16 = 22;
pub const NL80211_ATTR_MNTR_FLAGS: u16 = 23;
pub const NL80211_ATTR_MESH_ID: u16 = 24;
pub const NL80211_ATTR_STA_PLINK_ACTION: u16 = 25;
pub const NL80211_ATTR_MPATH_NEXT_HOP: u16 = 26;
pub const NL80211_ATTR_MPATH_INFO: u16 = 27;
pub const NL80211_ATTR_BSS_CTS_PROT: u16 = 28;
pub const NL80211_ATTR_BSS_SHORT_PREAMBLE: u16 = 29;
pub const NL80211_ATTR_BSS_SHORT_SLOT_TIME: u16 = 30;
pub const NL80211_ATTR_HT_CAPABILITY: u16 = 31;
pub const NL80211_ATTR_SUPPORTED_IFTYPES: u16 = 32;
pub const NL80211_ATTR_REG_ALPHA2: u16 = 33;
pub const NL80211_ATTR_REG_RULES: u16 = 34;
pub const NL80211_ATTR_MESH_CONFIG: u16 = 35;
pub const NL80211_ATTR_BSS_BASIC_RATES: u16 = 36;
pub const NL80211_ATTR_WIPHY_TXQ_PARAMS: u16 = 37;
pub const NL80211_ATTR_WIPHY_FREQ: u16 = 38;
pub const NL80211_ATTR_WIPHY_CHANNEL_TYPE: u16 = 39;
pub const NL80211_ATTR_KEY_DEFAULT_MGMT: u16 = 40;
pub const NL80211_ATTR_MGMT_SUBTYPE: u16 = 41;
pub const NL80211_ATTR_IE: u16 = 42;
pub const NL80211_ATTR_MAX_NUM_SCAN_SSIDS: u16 = 43;
pub const NL80211_ATTR_SCAN_FREQUENCIES: u16 = 44;
pub const NL80211_ATTR_SCAN_SSIDS: u16 = 45;
pub const NL80211_ATTR_GENERATION: u16 = 46;
pub const NL80211_ATTR_BSS: u16 = 47;
pub const NL80211_ATTR_REG_INITIATOR: u16 = 48;
pub const NL80211_ATTR_REG_TYPE: u16 = 49;
pub const NL80211_ATTR_SUPPORTED_COMMANDS: u16 = 50;
pub const NL80211_ATTR_FRAME: u16 = 51;
pub const NL80211_ATTR_SSID: u16 = 52;
pub const NL80211_ATTR_AUTH_TYPE: u16 = 53;
pub const NL80211_ATTR_REASON_CODE: u16 = 54;
pub const NL80211_ATTR_KEY_TYPE: u16 = 55;
pub const NL80211_ATTR_MAX_SCAN_IE_LEN: u16 = 56;
pub const NL80211_ATTR_CIPHER_SUITES: u16 = 57;
pub const NL80211_ATTR_FREQ_BEFORE: u16 = 58;
pub const NL80211_ATTR_FREQ_AFTER: u16 = 59;
pub const NL80211_ATTR_FREQ_FIXED: u16 = 60;
pub const NL80211_ATTR_WIPHY_RETRY_SHORT: u16 = 61;
pub const NL80211_ATTR_WIPHY_RETRY_LONG: u16 = 62;
pub const NL80211_ATTR_WIPHY_FRAG_THRESHOLD: u16 = 63;
pub const NL80211_ATTR_WIPHY_RTS_THRESHOLD: u16 = 64;
pub const NL80211_ATTR_TIMED_OUT: u16 = 65;
pub const NL80211_ATTR_USE_MFP: u16 = 66;
pub const NL80211_ATTR_STA_FLAGS2: u16 = 67;
pub const NL80211_ATTR_CONTROL_PORT: u16 = 68;
pub const NL80211_ATTR_TESTDATA: u16 = 69;
pub const NL80211_ATTR_PRIVACY: u16 = 70;
pub const NL80211_ATTR_DISCONNECTED_BY_AP: u16 = 71;
pub const NL80211_ATTR_STATUS_CODE: u16 = 72;
pub const NL80211_ATTR_CIPHER_SUITES_PAIRWISE: u16 = 73;
pub const NL80211_ATTR_CIPHER_SUITE_GROUP: u16 = 74;
pub const NL80211_ATTR_WPA_VERSIONS: u16 = 75;
pub const NL80211_ATTR_AKM_SUITES: u16 = 76;
pub const NL80211_ATTR_REQ_IE: u16 = 77;
pub const NL80211_ATTR_RESP_IE: u16 = 78;
pub const NL80211_ATTR_PREV_BSSID: u16 = 79;
pub const NL80211_ATTR_KEY: u16 = 80;
pub const NL80211_ATTR_KEYS: u16 = 81;
pub const NL80211_ATTR_PID: u16 = 82;
pub const NL80211_ATTR_4ADDR: u16 = 83;
pub const NL80211_ATTR_SURVEY_INFO: u16 = 84;
pub const NL80211_ATTR_PMKID: u16 = 85;
pub const NL80211_ATTR_MAX_NUM_PMKIDS: u16 = 86;
pub const NL80211_ATTR_DURATION: u16 = 87;
pub const NL80211_ATTR_COOKIE: u16 = 88;
pub const NL80211_ATTR_WIPHY_COVERAGE_CLASS: u16 = 89;
pub const NL80211_ATTR_TX_RATES: u16 = 90;
pub const NL80211_ATTR_FRAME_MATCH: u16 = 91;
pub const NL80211_ATTR_ACK: u16 = 92;
pub const NL80211_ATTR_PS_STATE: u16 = 93;
pub const NL80211_ATTR_CQM: u16 = 94;
pub const NL80211_ATTR_LOCAL_STATE_CHANGE: u16 = 95;
pub const NL80211_ATTR_AP_ISOLATE: u16 = 96;
pub const NL80211_ATTR_WIPHY_TX_POWER_SETTING: u16 = 97;
pub const NL80211_ATTR_WIPHY_TX_POWER_LEVEL: u16 = 98;
pub const NL80211_ATTR_TX_FRAME_TYPES: u16 = 99;
pub const NL80211_ATTR_RX_FRAME_TYPES: u16 = 100;
pub const NL80211_ATTR_FRAME_TYPE: u16 = 101;
pub const NL80211_ATTR_CONTROL_PORT_ETHERTYPE: u16 = 102;
pub const NL80211_ATTR_CONTROL_PORT_NO_ENCRYPT: u16 = 103;
pub const NL80211_ATTR_SUPPORT_IBSS_RSN: u16 = 104;
pub const NL80211_ATTR_WIPHY_ANTENNA_TX: u16 = 105;
pub const NL80211_ATTR_WIPHY_ANTENNA_RX: u16 = 106;
pub const NL80211_ATTR_MCAST_RATE: u16 = 107;
pub const NL80211_ATTR_OFFCHANNEL_TX_OK: u16 = 108;
pub const NL80211_ATTR_BSS_HT_OPMODE: u16 = 109;
pub const NL80211_ATTR_KEY_DEFAULT_TYPES: u16 = 110;
pub const NL80211_ATTR_MAX_REMAIN_ON_CHANNEL_DURATION: u16 = 111;
pub const NL80211_ATTR_MESH_SETUP: u16 = 112;
pub const NL80211_ATTR_WIPHY_ANTENNA_AVAIL_TX: u16 = 113;
pub const NL80211_ATTR_WIPHY_ANTENNA_AVAIL_RX: u16 = 114;
pub const NL80211_ATTR_SUPPORT_MESH_AUTH: u16 = 115;
pub const NL80211_ATTR_STA_PLINK_STATE: u16 = 116;
pub const NL80211_ATTR_WOWLAN_TRIGGERS: u16 = 117;
pub const NL80211_ATTR_WOWLAN_TRIGGERS_SUPPORTED: u16 = 118;
pub const NL80211_ATTR_SCHED_SCAN_INTERVAL: u16 = 119;
pub const NL80211_ATTR_INTERFACE_COMBINATIONS: u16 = 120;
pub const NL80211_ATTR_SOFTWARE_IFTYPES: u16 = 121;
pub const NL80211_ATTR_REKEY_DATA: u16 = 122;
pub const NL80211_ATTR_MAX_NUM_SCHED_SCAN_SSIDS: u16 = 123;
pub const NL80211_ATTR_MAX_SCHED_SCAN_IE_LEN: u16 = 124;
pub const NL80211_ATTR_SCAN_SUPP_RATES: u16 = 125;
pub const NL80211_ATTR_HIDDEN_SSID: u16 = 126;
pub const NL80211_ATTR_IE_PROBE_RESP: u16 = 127;
pub const NL80211_ATTR_IE_ASSOC_RESP: u16 = 128;
pub const NL80211_ATTR_STA_WME: u16 = 129;
pub const NL80211_ATTR_SUPPORT_AP_UAPSD: u16 = 130;
pub const NL80211_ATTR_ROAM_SUPPORT: u16 = 131;
pub const NL80211_ATTR_SCHED_SCAN_MATCH: u16 = 132;
pub const NL80211_ATTR_MAX_MATCH_SETS: u16 = 133;
pub const NL80211_ATTR_PMKSA_CANDIDATE: u16 = 134;
pub const NL80211_ATTR_TX_NO_CCK_RATE: u16 = 135;
pub const NL80211_ATTR_TDLS_ACTION: u16 = 136;
pub const NL80211_ATTR_TDLS_DIALOG_TOKEN: u16 = 137;
pub const NL80211_ATTR_TDLS_OPERATION: u16 = 138;
pub const NL80211_ATTR_TDLS_SUPPORT: u16 = 139;
pub const NL80211_ATTR_TDLS_EXTERNAL_SETUP: u16 = 140;
pub const NL80211_ATTR_DEVICE_AP_SME: u16 = 141;
pub const NL80211_ATTR_DONT_WAIT_FOR_ACK: u16 = 142;
pub const NL80211_ATTR_FEATURE_FLAGS: u16 = 143;
pub const NL80211_ATTR_PROBE_RESP_OFFLOAD: u16 = 144;
pub const NL80211_ATTR_PROBE_RESP: u16 = 145;
pub const NL80211_ATTR_DFS_REGION: u16 = 146;
pub const NL80211_ATTR_DISABLE_HT: u16 = 147;
pub const NL80211_ATTR_HT_CAPABILITY_MASK: u16 = 148;
pub const NL80211_ATTR_NOACK_MAP: u16 = 149;
pub const NL80211_ATTR_INACTIVITY_TIMEOUT: u16 = 150;
pub const NL80211_ATTR_RX_SIGNAL_DBM: u16 = 151;
pub const NL80211_ATTR_BG_SCAN_PERIOD: u16 = 152;
pub const NL80211_ATTR_WDEV: u16 = 153;
pub const NL80211_ATTR_USER_REG_HINT_TYPE: u16 = 154;
pub const NL80211_ATTR_CONN_FAILED_REASON: u16 = 155;
pub const NL80211_ATTR_AUTH_DATA: u16 = 156;
pub const NL80211_ATTR_VHT_CAPABILITY: u16 = 157;
pub const NL80211_ATTR_SCAN_FLAGS: u16 = 158;
pub const NL80211_ATTR_CHANNEL_WIDTH: u16 = 159;
pub const NL80211_ATTR_CENTER_FREQ1: u16 = 160;
pub const NL80211_ATTR_CENTER_FREQ2: u16 = 161;
pub const NL80211_ATTR_P2P_CTWINDOW: u16 = 162;
pub const NL80211_ATTR_P2P_OPPPS: u16 = 163;
pub const NL80211_ATTR_LOCAL_MESH_POWER_MODE: u16 = 164;
pub const NL80211_ATTR_ACL_POLICY: u16 = 165;
pub const NL80211_ATTR_MAC_ADDRS: u16 = 166;
pub const NL80211_ATTR_MAC_ACL_MAX: u16 = 167;
pub const NL80211_ATTR_RADAR_EVENT: u16 = 168;
pub const NL80211_ATTR_EXT_CAPA: u16 = 169;
pub const NL80211_ATTR_EXT_CAPA_MASK: u16 = 170;
pub const NL80211_ATTR_STA_CAPABILITY: u16 = 171;
pub const NL80211_ATTR_STA_EXT_CAPABILITY: u16 = 172;
pub const NL80211_ATTR_PROTOCOL_FEATURES: u16 = 173;
pub const NL80211_ATTR_SPLIT_WIPHY_DUMP: u16 = 174;
pub const NL80211_ATTR_DISABLE_VHT: u16 = 175;
pub const NL80211_ATTR_VHT_CAPABILITY_MASK: u16 = 176;
pub const NL80211_ATTR_MDID: u16 = 177;
pub const NL80211_ATTR_IE_RIC: u16 = 178;
pub const NL80211_ATTR_CRIT_PROT_ID: u16 = 179;
pub const NL80211_ATTR_MAX_CRIT_PROT_DURATION: u16 = 180;
pub const NL80211_ATTR_PEER_AID: u16 = 181;
pub const NL80211_ATTR_COALESCE_RULE: u16 = 182;
pub const NL80211_ATTR_CH_SWITCH_COUNT: u16 = 183;
pub const NL80211_ATTR_CH_SWITCH_BLOCK_TX: u16 = 184;
pub const NL80211_ATTR_CSA_IES: u16 = 185;
pub const NL80211_ATTR_CNTDWN_OFFS_BEACON: u16 = 186;
pub const NL80211_ATTR_CNTDWN_OFFS_PRESP: u16 = 187;
pub const NL80211_ATTR_RXMGMT_FLAGS: u16 = 188;
pub const NL80211_ATTR_STA_SUPPORTED_CHANNELS: u16 = 189;
pub const NL80211_ATTR_STA_SUPPORTED_OPER_CLASSES: u16 = 190;
pub const NL80211_ATTR_HANDLE_DFS: u16 = 191;
pub const NL80211_ATTR_SUPPORT_5_MHZ: u16 = 192;
pub const NL80211_ATTR_SUPPORT_10_MHZ: u16 = 193;
pub const NL80211_ATTR_OPMODE_NOTIF: u16 = 194;
pub const NL80211_ATTR_VENDOR_ID: u16 = 195;
pub const NL80211_ATTR_VENDOR_SUBCMD: u16 = 196;
pub const NL80211_ATTR_VENDOR_DATA: u16 = 197;
pub const NL80211_ATTR_VENDOR_EVENTS: u16 = 198;
pub const NL80211_ATTR_QOS_MAP: u16 = 199;
pub const NL80211_ATTR_MAC_HINT: u16 = 200;
pub const NL80211_ATTR_WIPHY_FREQ_HINT: u16 = 201;
pub const NL80211_ATTR_MAX_AP_ASSOC_STA: u16 = 202;
pub const NL80211_ATTR_TDLS_PEER_CAPABILITY: u16 = 203;
pub const NL80211_ATTR_SOCKET_OWNER: u16 = 204;
pub const NL80211_ATTR_CSA_C_OFFSETS_TX: u16 = 205;
pub const NL80211_ATTR_MAX_CSA_COUNTERS: u16 = 206;
pub const NL80211_ATTR_TDLS_INITIATOR: u16 = 207;
pub const NL80211_ATTR_USE_RRM: u16 = 208;
pub const NL80211_ATTR_WIPHY_DYN_ACK: u16 = 209;
pub const NL80211_ATTR_TSID: u16 = 210;
pub const NL80211_ATTR_USER_PRIO: u16 = 211;
pub const NL80211_ATTR_ADMITTED_TIME: u16 = 212;
pub const NL80211_ATTR_SMPS_MODE: u16 = 213;
pub const NL80211_ATTR_OPER_CLASS: u16 = 214;
pub const NL80211_ATTR_MAC_MASK: u16 = 215;
pub const NL80211_ATTR_WIPHY_SELF_MANAGED_REG: u16 = 216;
pub const NL80211_ATTR_EXT_FEATURES: u16 = 217;
pub const NL80211_ATTR_SURVEY_RADIO_STATS: u16 = 218;
pub const NL80211_ATTR_NETNS_FD: u16 = 219;
pub const NL80211_ATTR_SCHED_SCAN_DELAY: u16 = 220;
pub const NL80211_ATTR_REG_INDOOR: u16 = 221;
pub const NL80211_ATTR_MAX_NUM_SCHED_SCAN_PLANS: u16 = 222;
pub const NL80211_ATTR_MAX_SCAN_PLAN_INTERVAL: u16 = 223;
pub const NL80211_ATTR_MAX_SCAN_PLAN_ITERATIONS: u16 = 224;
pub const NL80211_ATTR_SCHED_SCAN_PLANS: u16 = 225;
pub const NL80211_ATTR_PBSS: u16 = 226;
pub const NL80211_ATTR_BSS_SELECT: u16 = 227;
pub const NL80211_ATTR_STA_SUPPORT_P2P_PS: u16 = 228;
pub const NL80211_ATTR_PAD: u16 = 229;
pub const NL80211_ATTR_IFTYPE_EXT_CAPA: u16 = 230;
pub const NL80211_ATTR_MU_MIMO_GROUP_DATA: u16 = 231;
pub const NL80211_ATTR_MU_MIMO_FOLLOW_MAC_ADDR: u16 = 232;
pub const NL80211_ATTR_SCAN_START_TIME_TSF: u16 = 233;
pub const NL80211_ATTR_SCAN_START_TIME_TSF_BSSID: u16 = 234;
pub const NL80211_ATTR_MEASUREMENT_DURATION: u16 = 235;
pub const NL80211_ATTR_MEASUREMENT_DURATION_MANDATORY: u16 = 236;
pub const NL80211_ATTR_MESH_PEER_AID: u16 = 237;
pub const NL80211_ATTR_NAN_MASTER_PREF: u16 = 238;
pub const NL80211_ATTR_BANDS: u16 = 239;
pub const NL80211_ATTR_NAN_FUNC: u16 = 240;
pub const NL80211_ATTR_NAN_MATCH: u16 = 241;
pub const NL80211_ATTR_FILS_KEK: u16 = 242;
pub const NL80211_ATTR_FILS_NONCES: u16 = 243;
pub const NL80211_ATTR_MULTICAST_TO_UNICAST_ENABLED: u16 = 244;
pub const NL80211_ATTR_BSSID: u16 = 245;
pub const NL80211_ATTR_SCHED_SCAN_RELATIVE_RSSI: u16 = 246;
pub const NL80211_ATTR_SCHED_SCAN_RSSI_ADJUST: u16 = 247;
pub const NL80211_ATTR_TIMEOUT_REASON: u16 = 248;
pub const NL80211_ATTR_FILS_ERP_USERNAME: u16 = 249;
pub const NL80211_ATTR_FILS_ERP_REALM: u16 = 250;
pub const NL80211_ATTR_FILS_ERP_NEXT_SEQ_NUM: u16 = 251;
pub const NL80211_ATTR_FILS_ERP_RRK: u16 = 252;
pub const NL80211_ATTR_FILS_CACHE_ID: u16 = 253;
pub const NL80211_ATTR_PMK: u16 = 254;
pub const NL80211_ATTR_SCHED_SCAN_MULTI: u16 = 255;
pub const NL80211_ATTR_SCHED_SCAN_MAX_REQS: u16 = 256;
pub const NL80211_ATTR_WANT_1X_4WAY_HS: u16 = 257;
pub const NL80211_ATTR_PMKR0_NAME: u16 = 258;
pub const NL80211_ATTR_PORT_AUTHORIZED: u16 = 259;
pub const NL80211_ATTR_EXTERNAL_AUTH_ACTION: u16 = 260;
pub const NL80211_ATTR_EXTERNAL_AUTH_SUPPORT: u16 = 261;
pub const NL80211_ATTR_NSS: u16 = 262;
pub const NL80211_ATTR_ACK_SIGNAL: u16 = 263;
pub const NL80211_ATTR_CONTROL_PORT_OVER_NL80211: u16 = 264;
pub const NL80211_ATTR_TXQ_STATS: u16 = 265;
pub const NL80211_ATTR_TXQ_LIMIT: u16 = 266;
pub const NL80211_ATTR_TXQ_MEMORY_LIMIT: u16 = 267;
pub const NL80211_ATTR_TXQ_QUANTUM: u16 = 268;
pub const NL80211_ATTR_HE_CAPABILITY: u16 = 269;
pub const NL80211_ATTR_FTM_RESPONDER: u16 = 270;
pub const NL80211_ATTR_FTM_RESPONDER_STATS: u16 = 271;
pub const NL80211_ATTR_TIMEOUT: u16 = 272;
pub const NL80211_ATTR_PEER_MEASUREMENTS: u16 = 273;
pub const NL80211_ATTR_AIRTIME_WEIGHT: u16 = 274;
pub const NL80211_ATTR_STA_TX_POWER_SETTING: u16 = 275;
pub const NL80211_ATTR_STA_TX_POWER: u16 = 276;
pub const NL80211_ATTR_SAE_PASSWORD: u16 = 277;
pub const NL80211_ATTR_TWT_RESPONDER: u16 = 278;
pub const NL80211_ATTR_HE_OBSS_PD: u16 = 279;
pub const NL80211_ATTR_WIPHY_EDMG_CHANNELS: u16 = 280;
pub const NL80211_ATTR_WIPHY_EDMG_BW_CONFIG: u16 = 281;
pub const NL80211_ATTR_VLAN_ID: u16 = 282;
pub const NL80211_ATTR_HE_BSS_COLOR: u16 = 283;
pub const NL80211_ATTR_IFTYPE_AKM_SUITES: u16 = 284;
pub const NL80211_ATTR_TID_CONFIG: u16 = 285;
pub const NL80211_ATTR_CONTROL_PORT_NO_PREAUTH: u16 = 286;
pub const NL80211_ATTR_PMK_LIFETIME: u16 = 287;
pub const NL80211_ATTR_PMK_REAUTH_THRESHOLD: u16 = 288;
pub const NL80211_ATTR_RECEIVE_MULTICAST: u16 = 289;
pub const NL80211_ATTR_WIPHY_FREQ_OFFSET: u16 = 290;
pub const NL80211_ATTR_CENTER_FREQ1_OFFSET: u16 = 291;
pub const NL80211_ATTR_SCAN_FREQ_KHZ: u16 = 292;
pub const NL80211_ATTR_HE_6GHZ_CAPABILITY: u16 = 293;
pub const NL80211_ATTR_FILS_DISCOVERY: u16 = 294;
pub const NL80211_ATTR_UNSOL_BCAST_PROBE_RESP: u16 = 295;
pub const NL80211_ATTR_S1G_CAPABILITY: u16 = 296;
pub const NL80211_ATTR_S1G_CAPABILITY_MASK: u16 = 297;
pub const NL80211_ATTR_SAE_PWE: u16 = 298;
pub const NL80211_ATTR_RECONNECT_REQUESTED: u16 = 299;
pub const NL80211_ATTR_SAR_SPEC: u16 = 300;
pub const NL80211_ATTR_DISABLE_HE: u16 = 301;
pub const NL80211_ATTR_OBSS_COLOR_BITMAP: u16 = 302;
pub const NL80211_ATTR_COLOR_CHANGE_COUNT: u16 = 303;
pub const NL80211_ATTR_COLOR_CHANGE_COLOR: u16 = 304;
pub const NL80211_ATTR_COLOR_CHANGE_ELEMS: u16 = 305;
pub const NL80211_ATTR_MBSSID_CONFIG: u16 = 306;
pub const NL80211_ATTR_MBSSID_ELEMS: u16 = 307;
pub const NL80211_ATTR_RADAR_BACKGROUND: u16 = 308;
pub const NL80211_ATTR_AP_SETTINGS_FLAGS: u16 = 309;
pub const NL80211_ATTR_EHT_CAPABILITY: u16 = 310;
pub const NL80211_ATTR_DISABLE_EHT: u16 = 311;
pub const NL80211_ATTR_MLO_LINKS: u16 = 312;
pub const NL80211_ATTR_MLO_LINK_ID: u16 = 313;
pub const NL80211_ATTR_MLD_ADDR: u16 = 314;
pub const NL80211_ATTR_MLO_SUPPORT: u16 = 315;
pub const NL80211_ATTR_MAX_NUM_AKM_SUITES: u16 = 316;
pub const NL80211_ATTR_EML_CAPABILITY: u16 = 317;
pub const NL80211_ATTR_MLD_CAPA_AND_OPS: u16 = 318;
pub const NL80211_ATTR_TX_HW_TIMESTAMP: u16 = 319;
pub const NL80211_ATTR_RX_HW_TIMESTAMP: u16 = 320;
pub const NL80211_ATTR_TD_BITMAP: u16 = 321;
pub const NL80211_ATTR_PUNCT_BITMAP: u16 = 322;
pub const NL80211_ATTR_MAX_HW_TIMESTAMP_PEERS: u16 = 323;
pub const NL80211_ATTR_HW_TIMESTAMP_ENABLED: u16 = 324;
pub const NL80211_ATTR_EMA_RNR_ELEMS: u16 = 325;
pub const NL80211_ATTR_MLO_LINK_DISABLED: u16 = 326;
pub const NL80211_ATTR_BSS_DUMP_INCLUDE_USE_DATA: u16 = 327;
pub const NL80211_ATTR_MLO_TTLM_DLINK: u16 = 328;
pub const NL80211_ATTR_MLO_TTLM_ULINK: u16 = 329;
pub const NL80211_ATTR_ASSOC_SPP_AMSDU: u16 = 330;
pub const NL80211_ATTR_WIPHY_RADIOS: u16 = 331;
pub const NL80211_ATTR_WIPHY_INTERFACE_COMBINATIONS: u16 = 332;
pub const NL80211_ATTR_VIF_RADIO_MASK: u16 = 333;

pub const NL80211_BAND_ATTR_FREQS: u8 = 1;
pub const NL80211_BAND_ATTR_RATES: u8 = 2;
pub const NL80211_BAND_ATTR_HT_MCS_SET: u8 = 3;
pub const NL80211_BAND_ATTR_HT_CAPA: u8 = 4;
pub const NL80211_BAND_ATTR_HT_AMPDU_FACTOR: u8 = 5;
pub const NL80211_BAND_ATTR_HT_AMPDU_DENSITY: u8 = 6;
pub const NL80211_BAND_ATTR_VHT_MCS_SET: u8 = 7;
pub const NL80211_BAND_ATTR_VHT_CAPA: u8 = 8;
pub const NL80211_BAND_ATTR_IFTYPE_DATA: u8 = 9;
pub const NL80211_BAND_ATTR_EDMG_CHANNELS: u8 = 10;
pub const NL80211_BAND_ATTR_EDMG_BW_CONFIG: u8 = 11;
pub const NL80211_BAND_ATTR_S1G_MCS_NSS_SET: u8 = 12;
pub const NL80211_BAND_ATTR_S1G_CAPA: u8 = 13;

pub const NL80211_FREQUENCY_ATTR_FREQ: u8 = 1;
pub const NL80211_FREQUENCY_ATTR_DISABLED: u8 = 2;
pub const NL80211_FREQUENCY_ATTR_NO_IR: u8 = 3;
pub const NL80211_FREQUENCY_ATTR_RADAR: u8 = 5;
pub const NL80211_FREQUENCY_ATTR_MAX_TX_POWER: u8 = 6;
pub const NL80211_FREQUENCY_ATTR_DFS_STATE: u8 = 7;
pub const NL80211_FREQUENCY_ATTR_DFS_TIME: u8 = 8;
pub const NL80211_FREQUENCY_ATTR_NO_HT40_MINUS: u8 = 9;
pub const NL80211_FREQUENCY_ATTR_NO_HT40_PLUS: u8 = 10;
pub const NL80211_FREQUENCY_ATTR_NO_80MHZ: u8 = 11;
pub const NL80211_FREQUENCY_ATTR_NO_160MHZ: u8 = 12;
pub const NL80211_FREQUENCY_ATTR_DFS_CAC_TIME: u8 = 13;
pub const NL80211_FREQUENCY_ATTR_INDOOR_ONLY: u8 = 14;
pub const NL80211_FREQUENCY_ATTR_IR_CONCURRENT: u8 = 15;
pub const NL80211_FREQUENCY_ATTR_NO_20MHZ: u8 = 16;
pub const NL80211_FREQUENCY_ATTR_NO_10MHZ: u8 = 17;
pub const NL80211_FREQUENCY_ATTR_WMM: u8 = 18;
pub const NL80211_FREQUENCY_ATTR_NO_HE: u8 = 19;
pub const NL80211_FREQUENCY_ATTR_OFFSET: u8 = 20;
pub const NL80211_FREQUENCY_ATTR_1MHZ: u8 = 21;
pub const NL80211_FREQUENCY_ATTR_2MHZ: u8 = 22;
pub const NL80211_FREQUENCY_ATTR_4MHZ: u8 = 23;
pub const NL80211_FREQUENCY_ATTR_8MHZ: u8 = 24;
pub const NL80211_FREQUENCY_ATTR_16MHZ: u8 = 25;
pub const NL80211_FREQUENCY_ATTR_NO_320MHZ: u8 = 26;
pub const NL80211_FREQUENCY_ATTR_NO_EHT: u8 = 27;
pub const NL80211_FREQUENCY_ATTR_PSD: u8 = 28;
pub const NL80211_FREQUENCY_ATTR_DFS_CONCURRENT: u8 = 29;
pub const NL80211_FREQUENCY_ATTR_NO_6GHZ_VLP_CLIENT: u8 = 30;
pub const NL80211_FREQUENCY_ATTR_NO_6GHZ_AFC_CLIENT: u8 = 31;
pub const NL80211_FREQUENCY_ATTR_CAN_MONITOR: u8 = 32;
pub const NL80211_FREQUENCY_ATTR_ALLOW_6GHZ_VLP_AP: u8 = 33;

pub const NL80211_BITRATE_ATTR_RATE: u8 = 1;
pub const NL80211_BITRATE_ATTR_2GHZ_SHORTPREAMBLE: u8 = 2;

pub const NL80211_IFTYPE_UNSPECIFIED: u8 = 0;
pub const NL80211_IFTYPE_ADHOC: u8 = 1;
pub const NL80211_IFTYPE_STATION: u8 = 2;
pub const NL80211_IFTYPE_AP: u8 = 3;
pub const NL80211_IFTYPE_AP_VLAN: u8 = 4;
pub const NL80211_IFTYPE_WDS: u8 = 5;
pub const NL80211_IFTYPE_MONITOR: u8 = 6;
pub const NL80211_IFTYPE_MESH_POINT: u8 = 7;
pub const NL80211_IFTYPE_P2P_CLIENT: u8 = 8;
pub const NL80211_IFTYPE_P2P_GO: u8 = 9;
pub const NL80211_IFTYPE_P2P_DEVICE: u8 = 10;
pub const NL80211_IFTYPE_OCB: u8 = 11;
pub const NL80211_IFTYPE_NAN: u8 = 12;

const AF_INET: u16 = 2;

const NL80211_FEATURE_SK_TX_STATUS: u32 = 1 << 0;
const NL80211_FEATURE_HT_IBSS: u32 = 1 << 1;
const NL80211_FEATURE_INACTIVITY_TIMER: u32 = 1 << 2;
const NL80211_FEATURE_CELL_BASE_REG_HINTS: u32 = 1 << 3;
const NL80211_FEATURE_P2P_DEVICE_NEEDS_CHANNEL: u32 = 1 << 4;
const NL80211_FEATURE_SAE: u32 = 1 << 5;
const NL80211_FEATURE_LOW_PRIORITY_SCAN: u32 = 1 << 6;
const NL80211_FEATURE_SCAN_FLUSH: u32 = 1 << 7;
const NL80211_FEATURE_AP_SCAN: u32 = 1 << 8;
const NL80211_FEATURE_VIF_TXPOWER: u32 = 1 << 9;
const NL80211_FEATURE_NEED_OBSS_SCAN: u32 = 1 << 10;
const NL80211_FEATURE_P2P_GO_CTWIN: u32 = 1 << 11;
const NL80211_FEATURE_P2P_GO_OPPPS: u32 = 1 << 12;
const NL80211_FEATURE_ADVERTISE_CHAN_LIMITS: u32 = 1 << 14;
const NL80211_FEATURE_FULL_AP_CLIENT_STATE: u32 = 1 << 15;
const NL80211_FEATURE_USERSPACE_MPM: u32 = 1 << 16;
const NL80211_FEATURE_ACTIVE_MONITOR: u32 = 1 << 17;
const NL80211_FEATURE_AP_MODE_CHAN_WIDTH_CHANGE: u32 = 1 << 18;
const NL80211_FEATURE_DS_PARAM_SET_IE_IN_PROBES: u32 = 1 << 19;
const NL80211_FEATURE_WFA_TPC_IE_IN_PROBES: u32 = 1 << 20;
const NL80211_FEATURE_QUIET: u32 = 1 << 21;
const NL80211_FEATURE_TX_POWER_INSERTION: u32 = 1 << 22;
const NL80211_FEATURE_ACKTO_ESTIMATION: u32 = 1 << 23;
const NL80211_FEATURE_STATIC_SMPS: u32 = 1 << 24;
const NL80211_FEATURE_DYNAMIC_SMPS: u32 = 1 << 25;
const NL80211_FEATURE_SUPPORTS_WMM_ADMISSION: u32 = 1 << 26;
const NL80211_FEATURE_MAC_ON_CREATE: u32 = 1 << 27;
const NL80211_FEATURE_TDLS_CHANNEL_SWITCH: u32 = 1 << 28;
const NL80211_FEATURE_SCAN_RANDOM_MAC_ADDR: u32 = 1 << 29;
const NL80211_FEATURE_SCHED_SCAN_RANDOM_MAC_ADDR: u32 = 1 << 30;
const NL80211_FEATURE_NO_RANDOM_MAC_ADDR: u32 = 1 << 31;

const NL80211_PROTOCOL_FEATURE_SPLIT_WIPHY_DUMP: u32 = 1 << 0;

const NL80211_STA_FLAG_AUTHORIZED: u32 = 1;
const NL80211_STA_FLAG_SHORT_PREAMBLE: u32 = 2;
const NL80211_STA_FLAG_WME: u32 = 4;
const NL80211_STA_FLAG_MFP: u32 = 8;
const NL80211_STA_FLAG_AUTHENTICATED: u32 = 16;
const NL80211_STA_FLAG_TDLS_PEER: u32 = 32;

#[derive(Clone, Copy, Debug)]
#[repr(C)]
struct GenlMsgHdr {
    cmd: u8,
    version: u8,
    reserved: u16,
}

impl GenlMsgHdr {
    fn new(cmd: u8) -> Self {
        GenlMsgHdr { cmd, version: NL80211_GENL_VERSION, reserved: 0 }
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
        if len < 4 { break; }
        let d = pos + 4;
        let e = pos + len;
        if typ == attr_type && e <= payload.len() && e - d >= 4 {
            return Some(u32::from_ne_bytes([payload[d], payload[d+1], payload[d+2], payload[d+3]]));
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
        let d = pos + 4;
        let e = pos + len;
        if typ == attr_type && e <= payload.len() && e > d {
            return Some(payload[d]);
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
        let d = pos + 4;
        let e = pos + len;
        if typ == attr_type && e <= payload.len() && e - d >= 2 {
            return Some(u16::from_ne_bytes([payload[d], payload[d+1]]));
        }
        if len == 0 { break; }
        pos += len;
    }
    None
}

fn find_attr_string<'a>(payload: &'a [u8], attr_type: u16) -> Option<&'a [u8]> {
    let mut pos = 0;
    while pos + 4 <= payload.len() {
        let len = u16::from_le_bytes([payload[pos], payload[pos + 1]]) as usize;
        let typ = u16::from_le_bytes([payload[pos + 2], payload[pos + 3]]);
        if len < 4 { break; }
        let d = pos + 4;
        let e = pos + len;
        if typ == attr_type && e <= payload.len() {
            return Some(&payload[d..e]);
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
        let d = pos + 4;
        let e = pos + len;
        if typ == attr_type && e <= payload.len() {
            return Some(&payload[d..e]);
        }
        if len == 0 { break; }
        pos += len;
    }
    None
}

fn build_wiphy_reply(w: &Wiphy, gen: u32) -> Vec<u8> {
    let mut p = Vec::new();
    let name_bytes = w.name.as_bytes();
    p.extend_from_slice(&NlAttr::new(NL80211_ATTR_WIPHY, &w.id.to_ne_bytes()));
    p.extend_from_slice(&NlAttr::new(NL80211_ATTR_WIPHY_NAME, name_bytes));
    p.extend_from_slice(&NlAttr::new(NL80211_ATTR_MAC, &w.mac));
    p.extend_from_slice(&NlAttr::new(NL80211_ATTR_GENERATION, &gen.to_ne_bytes()));
    p.extend_from_slice(&NlAttr::new(NL80211_ATTR_FEATURE_FLAGS, &w.feature_flags.to_ne_bytes()));
    p.extend_from_slice(&NlAttr::new(NL80211_ATTR_MAX_NUM_SCAN_SSIDS, &[w.max_scan_ssids]));
    p.extend_from_slice(&NlAttr::new(NL80211_ATTR_MAX_NUM_PMKIDS, &[w.max_num_pmkids]));
    p.extend_from_slice(&NlAttr::new(NL80211_ATTR_MAX_REMAIN_ON_CHANNEL_DURATION, &w.max_remain_on_channel.to_ne_bytes()));
    p.extend_from_slice(&NlAttr::new(NL80211_ATTR_MAX_SCAN_IE_LEN, &w.max_scan_ie_len.to_ne_bytes()));
    p.extend_from_slice(&NlAttr::new(NL80211_ATTR_MAX_MATCH_SETS, &[w.max_match_sets]));
    p.extend_from_slice(&NlAttr::new(NL80211_ATTR_MAX_CSA_COUNTERS, &[w.max_csa_counters]));
    p.extend_from_slice(&NlAttr::new(NL80211_ATTR_WIPHY_COVERAGE_CLASS, &[w.coverage_class]));
    p.extend_from_slice(&NlAttr::new(NL80211_ATTR_WIPHY_FRAG_THRESHOLD, &w.frag_threshold.to_ne_bytes()));
    p.extend_from_slice(&NlAttr::new(NL80211_ATTR_WIPHY_RTS_THRESHOLD, &w.rts_threshold.to_ne_bytes()));
    p.extend_from_slice(&NlAttr::new(NL80211_ATTR_WIPHY_RETRY_SHORT, &[w.retry_short]));
    p.extend_from_slice(&NlAttr::new(NL80211_ATTR_WIPHY_RETRY_LONG, &[w.retry_long]));
    p.extend_from_slice(&NlAttr::new(NL80211_ATTR_WIPHY_ANTENNA_TX, &w.antenna_tx.to_ne_bytes()));
    p.extend_from_slice(&NlAttr::new(NL80211_ATTR_WIPHY_ANTENNA_RX, &w.antenna_rx.to_ne_bytes()));
    p.extend_from_slice(&NlAttr::new(NL80211_ATTR_WIPHY_ANTENNA_AVAIL_TX, &w.antenna_avail_tx.to_ne_bytes()));
    p.extend_from_slice(&NlAttr::new(NL80211_ATTR_WIPHY_ANTENNA_AVAIL_RX, &w.antenna_avail_rx.to_ne_bytes()));
    if !w.cipher_suites.is_empty() {
        let cs: Vec<u8> = w.cipher_suites.iter().flat_map(|c| c.to_ne_bytes()).collect();
        p.extend_from_slice(&NlAttr::new(NL80211_ATTR_CIPHER_SUITES, &cs));
    }
    let mut sc_bytes = Vec::new();
    for cmd in &w.supported_commands {
        sc_bytes.extend_from_slice(&cmd.to_ne_bytes());
    }
    p.extend_from_slice(&NlAttr::new(NL80211_ATTR_SUPPORTED_COMMANDS, &sc_bytes));
    let mut iftypes: u32 = w.supported_iftypes;
    p.extend_from_slice(&NlAttr::new(NL80211_ATTR_SUPPORTED_IFTYPES, &iftypes.to_ne_bytes()));
    if w.self_managed_reg {
        p.extend_from_slice(&NlAttr::new(NL80211_ATTR_WIPHY_SELF_MANAGED_REG, &[1u8]));
    }
    p
}

fn handle_get_wiphy(attr_payload: &[u8], dump: bool) -> Vec<(u8, Vec<u8>)> {
    let wiphy_id = find_attr_u32(attr_payload, NL80211_ATTR_WIPHY);
    let gen = get_generation();

    if let Some(wid) = wiphy_id {
        if let Some(w) = get_wiphy(wid) {
            let payload = build_wiphy_reply(&w.lock(), gen);
            return vec![(NL80211_CMD_NEW_WIPHY, payload)];
        }
        return vec![];
    }

    let registry = WIPHY_REGISTRY.lock();
    let mut results = Vec::new();
    for w in registry.values() {
        let payload = build_wiphy_reply(&w.lock(), gen);
        results.push((NL80211_CMD_NEW_WIPHY, payload));
    }
    results
}

fn build_interface_reply(iface: &WirelessInterface, gen: u32) -> Vec<u8> {
    let mut p = Vec::new();
    let name_bytes = iface.name.as_bytes();
    p.extend_from_slice(&NlAttr::new(NL80211_ATTR_IFNAME, name_bytes));
    p.extend_from_slice(&NlAttr::new(NL80211_ATTR_IFTYPE, &(iface.iftype as u32).to_ne_bytes()));
    p.extend_from_slice(&NlAttr::new(NL80211_ATTR_IFINDEX, &iface.ifindex.to_ne_bytes()));
    p.extend_from_slice(&NlAttr::new(NL80211_ATTR_WIPHY, &iface.wiphy_id.to_ne_bytes()));
    p.extend_from_slice(&NlAttr::new(NL80211_ATTR_WDEV, &iface.wdev.to_ne_bytes()));
    p.extend_from_slice(&NlAttr::new(NL80211_ATTR_MAC, &iface.mac));
    p.extend_from_slice(&NlAttr::new(NL80211_ATTR_GENERATION, &gen.to_ne_bytes()));
    p.extend_from_slice(&NlAttr::new(NL80211_ATTR_TXQ_LIMIT, &iface.txq_limit.to_ne_bytes()));
    p.extend_from_slice(&NlAttr::new(NL80211_ATTR_TXQ_MEMORY_LIMIT, &iface.txq_memory_limit.to_ne_bytes()));
    p.extend_from_slice(&NlAttr::new(NL80211_ATTR_TXQ_QUANTUM, &iface.txq_quantum.to_ne_bytes()));
    p
}

fn handle_get_interface(attr_payload: &[u8], dump: bool) -> Vec<(u8, Vec<u8>)> {
    let gen = get_generation();
    let ifname_bytes = find_attr_string(attr_payload, NL80211_ATTR_IFNAME);

    if let Some(name_bytes) = ifname_bytes {
        let name = core::str::from_utf8(name_bytes).unwrap_or("");
        let registry = WDEV_REGISTRY.lock();
        for wdev in registry.values() {
            let iface = wdev.lock();
            if iface.name == name {
                let payload = build_interface_reply(&iface, gen);
                return vec![(NL80211_CMD_NEW_INTERFACE, payload)];
            }
        }
        return vec![];
    }

    let registry = WDEV_REGISTRY.lock();
    let mut results = Vec::new();
    for wdev in registry.values() {
        let payload = build_interface_reply(&wdev.lock(), gen);
        results.push((NL80211_CMD_NEW_INTERFACE, payload));
    }
    results
}

fn handle_set_interface(attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let iftype = find_attr_u32(attr_payload, NL80211_ATTR_IFTYPE);
    let ifname_bytes = find_attr_string(attr_payload, NL80211_ATTR_IFNAME);

    if let Some(name_bytes) = ifname_bytes {
        let name = core::str::from_utf8(name_bytes).unwrap_or("");
        let registry = WDEV_REGISTRY.lock();
        for wdev in registry.values() {
            let mut iface = wdev.lock();
            if iface.name == name {
                if let Some(typ) = iftype {
                    iface.iftype = match typ {
                        1 => Nl80211Iftype::Adhoc,
                        2 => Nl80211Iftype::Station,
                        3 => Nl80211Iftype::Ap,
                        _ => Nl80211Iftype::Station,
                    };
                }
                break;
            }
        }
    }
    vec![]
}

fn handle_new_interface(attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let iftype = find_attr_u32(attr_payload, NL80211_ATTR_IFTYPE).unwrap_or(2);
    let wiphy_id = find_attr_u32(attr_payload, NL80211_ATTR_WIPHY).unwrap_or(1);
    let name_bytes = find_attr_string(attr_payload, NL80211_ATTR_IFNAME).unwrap_or(b"wlan0");
    let name = core::str::from_utf8(name_bytes).unwrap_or("wlan0");
    let mac_bytes = find_attr_binary(attr_payload, NL80211_ATTR_MAC);

    let iftype_enum = match iftype {
        1 => Nl80211Iftype::Adhoc,
        2 => Nl80211Iftype::Station,
        3 => Nl80211Iftype::Ap,
        4 => Nl80211Iftype::ApVlan,
        6 => Nl80211Iftype::Monitor,
        _ => Nl80211Iftype::Station,
    };

    let mac = if let Some(m) = mac_bytes {
        let mut arr = [0u8; MAC_LEN];
        let copy_len = m.len().min(MAC_LEN);
        arr[..copy_len].copy_from_slice(&m[..copy_len]);
        arr
    } else {
        [0x02; 6]
    };

    let _iface = create_interface(wiphy_id, name, iftype_enum, mac);
    vec![]
}

fn handle_del_interface(attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let wdev = find_attr_u64(attr_payload, NL80211_ATTR_WDEV);
    if let Some(w) = wdev {
        delete_interface(w);
    }
    vec![]
}

fn find_attr_u64(payload: &[u8], attr_type: u16) -> Option<u64> {
    let mut pos = 0;
    while pos + 4 <= payload.len() {
        let len = u16::from_le_bytes([payload[pos], payload[pos + 1]]) as usize;
        let typ = u16::from_le_bytes([payload[pos + 2], payload[pos + 3]]);
        if len < 4 { break; }
        let d = pos + 4;
        let e = pos + len;
        if typ == attr_type && e <= payload.len() && e - d >= 8 {
            let arr: [u8; 8] = [payload[d], payload[d+1], payload[d+2], payload[d+3],
                                payload[d+4], payload[d+5], payload[d+6], payload[d+7]];
            return Some(u64::from_ne_bytes(arr));
        }
        if len == 0 { break; }
        pos += len;
    }
    None
}

fn handle_get_station(attr_payload: &[u8], dump: bool) -> Vec<(u8, Vec<u8>)> {
    let _ifindex = find_attr_u32(attr_payload, NL80211_ATTR_IFINDEX);
    let mac_bytes = find_attr_binary(attr_payload, NL80211_ATTR_MAC);

    let gen = get_generation();

    if let Some(m) = mac_bytes {
        if m.len() >= MAC_LEN {
            let mut mac = [0u8; MAC_LEN];
            mac.copy_from_slice(&m[..MAC_LEN]);
            if let Some(_sta) = get_station(0, &mac) {
                let mut p = Vec::new();
                p.extend_from_slice(&NlAttr::new(NL80211_ATTR_MAC, &mac));
                p.extend_from_slice(&NlAttr::new(NL80211_ATTR_GENERATION, &gen.to_ne_bytes()));
                return vec![(NL80211_CMD_NEW_STATION, p)];
            }
        }
        return vec![];
    }

    let mut results = Vec::new();
    let reg = STATION_REGISTRY.lock();
    for ((_idx, mac), _sta) in reg.iter() {
        let mut p = Vec::new();
        p.extend_from_slice(&NlAttr::new(NL80211_ATTR_MAC, mac));
        p.extend_from_slice(&NlAttr::new(NL80211_ATTR_GENERATION, &gen.to_ne_bytes()));
        results.push((NL80211_CMD_NEW_STATION, p));
    }
    results
}

fn handle_trigger_scan(attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let ssid = find_attr_binary(attr_payload, NL80211_ATTR_SSID);
    let _freqs = find_attr_binary(attr_payload, NL80211_ATTR_SCAN_FREQUENCIES);
    let _ie = find_attr_binary(attr_payload, NL80211_ATTR_IE);

    crate::serial_println!("[nl80211] Trigger scan requested (ssid={:?})", ssid);
    vec![]
}

fn handle_get_scan(attr_payload: &[u8], dump: bool) -> Vec<(u8, Vec<u8>)> {
    vec![]
}

fn handle_authenticate(attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let _mac = find_attr_binary(attr_payload, NL80211_ATTR_MAC);
    let _ssid = find_attr_binary(attr_payload, NL80211_ATTR_SSID);
    let _auth_type = find_attr_u32(attr_payload, NL80211_ATTR_AUTH_TYPE);
    let _ie = find_attr_binary(attr_payload, NL80211_ATTR_IE);
    vec![]
}

fn handle_associate(attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let _mac = find_attr_binary(attr_payload, NL80211_ATTR_MAC);
    let _ssid = find_attr_binary(attr_payload, NL80211_ATTR_SSID);
    let _ie = find_attr_binary(attr_payload, NL80211_ATTR_IE);
    vec![]
}

fn handle_deauthenticate(attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let _mac = find_attr_binary(attr_payload, NL80211_ATTR_MAC);
    let _reason = find_attr_u16(attr_payload, NL80211_ATTR_REASON_CODE);
    vec![]
}

fn handle_disassociate(attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let _mac = find_attr_binary(attr_payload, NL80211_ATTR_MAC);
    let _reason = find_attr_u16(attr_payload, NL80211_ATTR_REASON_CODE);
    vec![]
}

fn handle_connect(attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let _ssid = find_attr_binary(attr_payload, NL80211_ATTR_SSID);
    let _bssid = find_attr_binary(attr_payload, NL80211_ATTR_BSSID);
    let _ie = find_attr_binary(attr_payload, NL80211_ATTR_IE);
    let _auth_type = find_attr_u32(attr_payload, NL80211_ATTR_AUTH_TYPE);
    let _privacy = find_attr_u8(attr_payload, NL80211_ATTR_PRIVACY);
    vec![]
}

fn handle_disconnect(attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let _reason = find_attr_u16(attr_payload, NL80211_ATTR_REASON_CODE);
    vec![]
}

fn handle_get_protocol_features(attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let mut p = Vec::new();
    let features = NL80211_PROTOCOL_FEATURE_SPLIT_WIPHY_DUMP;
    p.extend_from_slice(&NlAttr::new(NL80211_ATTR_PROTOCOL_FEATURES, &features.to_ne_bytes()));
    vec![(NL80211_CMD_GET_PROTOCOL_FEATURES, p)]
}

fn handle_set_wiphy(attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let wiphy_id = find_attr_u32(attr_payload, NL80211_ATTR_WIPHY);
    if let Some(wid) = wiphy_id {
        if let Some(w) = get_wiphy(wid) {
            let mut wiphy = w.lock();
            if let Some(name) = find_attr_string(attr_payload, NL80211_ATTR_WIPHY_NAME) {
                if let Ok(s) = core::str::from_utf8(name) {
                    wiphy.name = alloc::string::String::from(s);
                }
            }
            if let Some(frag) = find_attr_u32(attr_payload, NL80211_ATTR_WIPHY_FRAG_THRESHOLD) {
                wiphy.frag_threshold = frag;
            }
            if let Some(rts) = find_attr_u32(attr_payload, NL80211_ATTR_WIPHY_RTS_THRESHOLD) {
                wiphy.rts_threshold = rts;
            }
            if let Some(cc) = find_attr_u8(attr_payload, NL80211_ATTR_WIPHY_COVERAGE_CLASS) {
                wiphy.coverage_class = cc;
            }
            if let Some(retry_s) = find_attr_u8(attr_payload, NL80211_ATTR_WIPHY_RETRY_SHORT) {
                wiphy.retry_short = retry_s;
            }
            if let Some(retry_l) = find_attr_u8(attr_payload, NL80211_ATTR_WIPHY_RETRY_LONG) {
                wiphy.retry_long = retry_l;
            }
        }
    }
    vec![]
}

fn handle_set_bss(attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let _cts = find_attr_u8(attr_payload, NL80211_ATTR_BSS_CTS_PROT);
    let _preamble = find_attr_u8(attr_payload, NL80211_ATTR_BSS_SHORT_PREAMBLE);
    let _slot = find_attr_u8(attr_payload, NL80211_ATTR_BSS_SHORT_SLOT_TIME);
    vec![]
}

fn handle_get_reg(attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let mut p = Vec::new();
    p.extend_from_slice(&NlAttr::new(NL80211_ATTR_REG_ALPHA2, b"00"));
    p.extend_from_slice(&NlAttr::new(NL80211_ATTR_REG_INITIATOR, &[0u8]));
    p.extend_from_slice(&NlAttr::new(NL80211_ATTR_REG_TYPE, &[0u8]));
    vec![(NL80211_CMD_GET_REG, p)]
}

fn handle_set_reg(attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let _alpha2 = find_attr_binary(attr_payload, NL80211_ATTR_REG_ALPHA2);
    vec![]
}

fn handle_get_key(attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    vec![]
}

fn handle_set_key(attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    vec![]
}

fn handle_del_key(attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    vec![]
}

fn handle_get_beacon(attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    vec![]
}

fn handle_set_beacon(attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    vec![]
}

fn handle_del_beacon(attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    vec![]
}

fn handle_set_station(attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let _mac = find_attr_binary(attr_payload, NL80211_ATTR_MAC);
    let _flags = find_attr_u32(attr_payload, NL80211_ATTR_STA_FLAGS);
    vec![]
}

fn handle_del_station(attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let _mac = find_attr_binary(attr_payload, NL80211_ATTR_MAC);
    vec![]
}

fn handle_new_station(attr_payload: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let _mac = find_attr_binary(attr_payload, NL80211_ATTR_MAC);
    let _aid = find_attr_u16(attr_payload, NL80211_ATTR_STA_AID);
    let _listen = find_attr_u16(attr_payload, NL80211_ATTR_STA_LISTEN_INTERVAL);
    let _rates = find_attr_binary(attr_payload, NL80211_ATTR_STA_SUPPORTED_RATES);
    let _cap = find_attr_u16(attr_payload, NL80211_ATTR_STA_CAPABILITY);
    if let Some(m) = _mac {
        if m.len() >= 6 {
            let mut mac = [0u8; 6];
            mac.copy_from_slice(&m[..6]);
            get_or_create_station(0, mac);
        }
    }
    vec![]
}

pub fn handle_nl80211_genl_request(
    src_pid: u32,
    seq: u32,
    payload: &[u8],
) {
    if payload.len() < 4 { return; }

    let hdr = unsafe { &*(payload.as_ptr() as *const GenlMsgHdr) };
    let cmd = hdr.cmd;
    let attr_payload = &payload[4..];

    let cmd_responses = match cmd {
        NL80211_CMD_GET_WIPHY => handle_get_wiphy(attr_payload, false),
        NL80211_CMD_SET_WIPHY => handle_set_wiphy(attr_payload),
        NL80211_CMD_GET_INTERFACE => handle_get_interface(attr_payload, false),
        NL80211_CMD_SET_INTERFACE => handle_set_interface(attr_payload),
        NL80211_CMD_NEW_INTERFACE => handle_new_interface(attr_payload),
        NL80211_CMD_DEL_INTERFACE => handle_del_interface(attr_payload),
        NL80211_CMD_GET_KEY => handle_get_key(attr_payload),
        NL80211_CMD_SET_KEY => handle_set_key(attr_payload),
        NL80211_CMD_DEL_KEY => handle_del_key(attr_payload),
        NL80211_CMD_GET_BEACON => handle_get_beacon(attr_payload),
        NL80211_CMD_SET_BEACON => handle_set_beacon(attr_payload),
        NL80211_CMD_DEL_BEACON => handle_del_beacon(attr_payload),
        NL80211_CMD_GET_STATION => handle_get_station(attr_payload, false),
        NL80211_CMD_SET_STATION => handle_set_station(attr_payload),
        NL80211_CMD_NEW_STATION => handle_new_station(attr_payload),
        NL80211_CMD_DEL_STATION => handle_del_station(attr_payload),
        NL80211_CMD_SET_BSS => handle_set_bss(attr_payload),
        NL80211_CMD_SET_REG => handle_set_reg(attr_payload),
        NL80211_CMD_GET_REG => handle_get_reg(attr_payload),
        NL80211_CMD_TRIGGER_SCAN => handle_trigger_scan(attr_payload),
        NL80211_CMD_GET_SCAN => handle_get_scan(attr_payload, false),
        NL80211_CMD_AUTHENTICATE => handle_authenticate(attr_payload),
        NL80211_CMD_ASSOCIATE => handle_associate(attr_payload),
        NL80211_CMD_DEAUTHENTICATE => handle_deauthenticate(attr_payload),
        NL80211_CMD_DISASSOCIATE => handle_disassociate(attr_payload),
        NL80211_CMD_CONNECT => handle_connect(attr_payload),
        NL80211_CMD_DISCONNECT => handle_disconnect(attr_payload),
        NL80211_CMD_GET_PROTOCOL_FEATURES => handle_get_protocol_features(attr_payload),
        NL80211_CMD_SET_WIPHY_NETNS => vec![],
        NL80211_CMD_NEW_WIPHY => vec![],
        NL80211_CMD_DEL_WIPHY => vec![],
        NL80211_CMD_SET_POWER_SAVE => vec![],
        NL80211_CMD_GET_POWER_SAVE => {
            let mut p = Vec::new();
            p.extend_from_slice(&NlAttr::new(NL80211_ATTR_PS_STATE, &0u32.to_ne_bytes()));
            vec![(NL80211_CMD_GET_POWER_SAVE, p)]
        }
        _ => vec![],
    };

    let mut all_responses: Vec<(u8, Vec<u8>)> = cmd_responses;
    all_responses.push((0, Vec::new()));

    for (resp_cmd, resp_payload) in &all_responses {
        let mut inner = Vec::new();
        let ghdr = GenlMsgHdr::new(*resp_cmd);
        inner.extend_from_slice(ghdr.as_bytes());
        inner.extend_from_slice(resp_payload);

        let is_done = resp_payload.is_empty() && *resp_cmd == 0;
        let msg_type = if is_done { 3u16 } else { NL80211_GENL_ID };

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
    fn test_nl80211_constants() {
        assert_eq!(NL80211_GENL_ID, 33);
        assert_eq!(NL80211_CMD_GET_WIPHY, 1);
        assert_eq!(NL80211_CMD_GET_INTERFACE, 5);
        assert_eq!(NL80211_CMD_GET_STATION, 17);
        assert_eq!(NL80211_CMD_TRIGGER_SCAN, 33);
        assert_eq!(NL80211_CMD_AUTHENTICATE, 37);
        assert_eq!(NL80211_CMD_ASSOCIATE, 38);
        assert_eq!(NL80211_CMD_CONNECT, 46);
        assert_eq!(NL80211_CMD_GET_PROTOCOL_FEATURES, 95);

        assert_eq!(NL80211_ATTR_WIPHY, 1);
        assert_eq!(NL80211_ATTR_IFINDEX, 3);
        assert_eq!(NL80211_ATTR_MAC, 6);
        assert_eq!(NL80211_ATTR_SSID, 52);
        assert_eq!(NL80211_ATTR_GENERATION, 46);
        assert_eq!(NL80211_ATTR_FEATURE_FLAGS, 143);
        assert_eq!(NL80211_ATTR_WDEV, 153);
    }

    #[test]
    fn test_get_wiphy_empty() {
        let result = handle_get_wiphy(&[], false);
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_create_and_get_interface() {
        let _iface = create_interface(1, "wlan0", Nl80211Iftype::Station, [0x02; 6]);
        let result = handle_get_interface(&[], false);
        assert!(result.len() >= 1);
    }

    #[test]
    fn test_get_protocol_features() {
        let result = handle_get_protocol_features(&[]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, NL80211_CMD_GET_PROTOCOL_FEATURES);
    }
}
