//! # Bluetooth Jail — TIER 2 Bluetooth Surucusu
//!
//! Bluetooth donanimi HCI transport uzerinden jail sandbox ortaminda calisir.
//! LE advertising, scanning, baglanti ve SMP pairing jail icinde izole edilir.
//!
//! ## Mimari
//!
//! ```text
//! ┌────────────────┐     SPSC Ring      ┌────────────────┐     HCI UART/USB  ┌──────────┐
//! │  BT Stack      │ ◄════════════════► │  BtJailWorker  │ ◄══════════════► │  BT HW   │
//! │  (Core)        │   CommandRing      │  (kernel)      │   TX/RX Rings    │          │
//! │                │   CompletionRing   │                │                  │          │
//! │  advertise()   │                    │  HCI transport │                  │          │
//! │  scan()        │                    │  IRQ handling  │                  │          │
//! │  connect()     │                    │  LE stack      │                  │          │
//! │  pair()        │                    │  SMP pairing   │                  │          │
//! └────────────────┘                    └────────────────┘                └──────────┘
//! ```
//!
//! ## HCI Transport
//!
//! - USB HCI (USB class 0xE0, subclass 0x01, protocol 0x01)
//! - UART HCI (115200 baud, 8N1, H4 protokol)
//! - SDIO HCI (SDIO function 1, Bluetooth)
//!
//! ## HCI Initialization Flow (Bluetooth Core Spec v5.4)
//!
//! ```text
//! 1. HCI Reset (0x0C03) → Command Complete
//! 2. Read Local Version (0x1001) → hci_ver, hci_rev, manufacturer
//! 3. Read BD_ADDR (0x1009) → public address
//! 4. Read LE Buffer Size (0x2002) → LE ACL MTU, max packets
//! 5. Set Event Mask (0x0C01) → enable relevant events
//! 6. LE Set Event Mask (0x2001) → enable LE events
//! 7. Write LE Host Supported (0x0C6D) → tell controller host supports LE
//! ```
//!
//! ## LE Scanning Flow (Central)
//!
//! ```text
//! 1. LE Set Scan Parameters (0x200B)
//! 2. LE Set Scan Enable (0x200C, enable=1, filter_dup=1)
//!    → Receive LE Advertising Report events (0x3E, subevent 0x02)
//! 3. When target found:
//!    a. LE Set Scan Enable (0x200C, enable=0)
//!    b. LE Create Connection (0x200D)
//!       → Wait for LE Connection Complete (0x3E, subevent 0x01)
//! ```
//!
//! ## LE Advertising Flow (Peripheral)
//!
//! ```text
//! 1. LE Set Advertising Parameters (0x2006)
//! 2. LE Set Advertising Data (0x2008)
//! 3. LE Set Scan Response Data (0x2009)  // optional
//! 4. LE Set Advertise Enable (0x200A, enable=1)
//!    → Wait for LE Connection Complete event
//! ```
//!
//! ## SMP Pairing (BLE 4.2+ LE Secure Connections)
//!
//! ```text
//! Phase 1: Pairing Request → Pairing Response (IO capability exchange)
//! Phase 2: Public Key exchange → DHKey computation → Numeric comparison / Passkey
//! Phase 3: LTK, IRK, CSRK distribution
//! ```
//!
//! ## Guvenlik
//!
//! - Tum HCI komutlari sandbox icinden gonderilir
//! - Firmware yukleme JailWorker tarafinda denetlenir
//! - Crash-only microreboot ile izolasyon (MINIX 3 modeli)

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU64, AtomicU8, Ordering};
use spin::Mutex;

use crate::drivers::jail_ring::{JailChannel, JailEvent, JailOpcode, JailRequest};

// ============================================================================
// HCI Sabitleri (Bluetooth Core Spec v5.4, Vol 2, Part E)
// ============================================================================

/// HCI paket turleri (UART/SDIO framing prefix)
const HCI_CMD_PKT: u8 = 0x01;
const HCI_ACL_PKT: u8 = 0x02;
const HCI_SCO_PKT: u8 = 0x03;
const HCI_EVT_PKT: u8 = 0x04;
const HCI_ISO_PKT: u8 = 0x05;

/// HCI OGF (OpCode Group Field)
const OGF_LINK_CONTROL: u8 = 0x01;
const OGF_LINK_POLICY: u8 = 0x02;
const OGF_CONTROLLER_BASEBAND: u8 = 0x03;
const OGF_INFO_PARAMS: u8 = 0x04;
const OGF_STATUS_PARAMS: u8 = 0x05;
const OGF_TESTING: u8 = 0x06;
const OGF_LE: u8 = 0x08;
const OGF_VENDOR: u8 = 0x3F;

/// HCI OCF (OpCode Command Field)
const OCF_RESET: u16 = 0x0003;
const OCF_READ_BD_ADDR: u16 = 0x0009;
const OCF_LE_SET_EVENT_MASK: u16 = 0x0001;
const OCF_LE_SET_ADV_PARAM: u16 = 0x0006;
const OCF_LE_SET_ADV_DATA: u16 = 0x0008;
const OCF_LE_SET_SCAN_PARAM: u16 = 0x000B;
const OCF_LE_SET_SCAN_ENABLE: u16 = 0x000C;
const OCF_LE_SET_ADV_ENABLE: u16 = 0x000A;
const OCF_LE_CREATE_CONN: u16 = 0x000D;

/// HCI Event Codes (Vol 2, Part E, §7.7.14)
const EVT_CONN_COMPLETE: u8 = 0x03;
const EVT_DISCONN_COMPLETE: u8 = 0x05;
const EVT_CMD_COMPLETE: u8 = 0x0E;
const EVT_CMD_STATUS: u8 = 0x0F;
const EVT_NUM_COMPLETED_PKTS: u8 = 0x13;
const EVT_LE_META: u8 = 0x3E;

/// LE Meta Subevent Codes (Vol 2, Part E, §7.7.65)
const LE_SUBEV_CONN_COMPLETE: u8 = 0x01;
const LE_SUBEV_ADV_REPORT: u8 = 0x02;
const LE_SUBEV_CONN_UPDATE_COMPLETE: u8 = 0x03;
const LE_SUBEV_READ_REMOTE_FEATURES: u8 = 0x04;
const LE_SUBEV_LTK_REQUEST: u8 = 0x05;
const LE_SUBEV_ENHANCED_CONN_COMPLETE: u8 = 0x0A;
const LE_SUBEV_EXTENDED_ADV_REPORT: u8 = 0x0D;

/// HCI Status Codes (Vol 2, Part D, §1.3)
const HCI_SUCCESS: u8 = 0x00;
const HCI_ERR_UNKNOWN_CMD: u8 = 0x01;
const HCI_ERR_UNKNOWN_CONN_ID: u8 = 0x02;
const HCI_ERR_HW_FAILURE: u8 = 0x03;
const HCI_ERR_CONN_TIMEOUT: u8 = 0x08;
const HCI_ERR_MAX_CONNECTIONS: u8 = 0x09;
const HCI_ERR_COMMAND_DISALLOWED: u8 = 0x0C;
const HCI_ERR_UNSUPPORTED_FEATURE: u8 = 0x11;
const HCI_ERR_INVALID_PARAMS: u8 = 0x12;
const HCI_ERR_REMOTE_USER_TERM: u8 = 0x13;
const HCI_ERR_REMOTE_LOW_RESOURCES: u8 = 0x14;
const HCI_ERR_REMOTE_POWER_OFF: u8 = 0x15;
const HCI_ERR_LOCAL_HOST_TERM: u8 = 0x16;
const HCI_ERR_UNSUPPORTED_REMOTE: u8 = 0x1A;
const HCI_ERR_CONTROLLER_BUSY: u8 = 0x3C;

/// LE Advertising tipleri (Vol 2, Part E, §7.8.12)
const ADV_IND: u8 = 0x00;
const ADV_DIRECT_IND_HIGH: u8 = 0x01;
const ADV_SCAN_IND: u8 = 0x02;
const ADV_NONCONN_IND: u8 = 0x03;
const ADV_DIRECT_IND_LOW: u8 = 0x04;

/// LE Address tipleri
const ADDR_PUBLIC: u8 = 0x00;
const ADDR_RANDOM: u8 = 0x01;
const ADDR_RPA_PUBLIC: u8 = 0x02;
const ADDR_RPA_RANDOM: u8 = 0x03;

/// LE Advertising channel map
const ADV_CHAN_37: u8 = 0x01;
const ADV_CHAN_38: u8 = 0x02;
const ADV_CHAN_39: u8 = 0x04;
const ADV_CHAN_ALL: u8 = 0x07;

/// LE Scan tipleri
const SCAN_PASSIVE: u8 = 0x00;
const SCAN_ACTIVE: u8 = 0x01;

/// LE Filter policy
const FILTER_POLICY_ALL: u8 = 0x00;
const FILTER_POLICY_WHITE_LIST: u8 = 0x01;

/// Role
const ROLE_CENTRAL: u8 = 0x00;
const ROLE_PERIPHERAL: u8 = 0x01;

/// L2CAP CID'ler (LE)
const L2CAP_CID_LE_SIGNALING: u16 = 0x0005;
const L2CAP_CID_ATT: u16 = 0x0004;
const L2CAP_CID_SMP: u16 = 0x0006;

/// SMP Command Codes (Core Spec v5.4, Vol 3, Part H, §3.3)
const SMP_PAIRING_REQ: u8 = 0x01;
const SMP_PAIRING_RSP: u8 = 0x02;
const SMP_PAIRING_CONFIRM: u8 = 0x03;
const SMP_PAIRING_RANDOM: u8 = 0x04;
const SMP_PAIRING_FAILED: u8 = 0x05;
const SMP_ENC_INFO: u8 = 0x06;
const SMP_MASTER_IDENT: u8 = 0x07;
const SMP_IDENT_INFO: u8 = 0x08;
const SMP_IDENT_ADDR_INFO: u8 = 0x09;
const SMP_SIGN_INFO: u8 = 0x0A;
const SMP_SEC_REQ: u8 = 0x0B;
const SMP_PAIRING_PUBKEY: u8 = 0x0C;
const SMP_PAIRING_DHKEY_CHECK: u8 = 0x0D;
const SMP_KEYPRESS_NOTIFY: u8 = 0x0E;

/// SMP AuthReq bits
const SMP_AUTH_BONDING: u8 = 0x01;
const SMP_AUTH_MITM: u8 = 0x04;
const SMP_AUTH_SC: u8 = 0x08;
const SMP_AUTH_CT2: u8 = 0x10;

/// SMP IO Capabilities
const SMP_IO_DISPLAY_ONLY: u8 = 0x00;
const SMP_IO_DISPLAY_YESNO: u8 = 0x01;
const SMP_IO_KEYBOARD_ONLY: u8 = 0x02;
const SMP_IO_NO_INPUT_OUTPUT: u8 = 0x03;
const SMP_IO_KEYBOARD_DISPLAY: u8 = 0x04;

/// SMP Key Distribution flags
const SMP_DIST_ENC_KEY: u8 = 0x01;
const SMP_DIST_ID_KEY: u8 = 0x02;
const SMP_DIST_SIGN_KEY: u8 = 0x04;
const SMP_DIST_LINK_KEY: u8 = 0x08;

/// SMP Pairing Failed reasons
const SMP_ERR_PASSKEY_ENTRY: u8 = 0x01;
const SMP_ERR_OOB_NOT_AVAIL: u8 = 0x02;
const SMP_ERR_AUTH_REQ: u8 = 0x03;
const SMP_ERR_CONFIRM_FAILED: u8 = 0x04;
const SMP_ERR_PAIRING_NOT_SUPP: u8 = 0x05;
const SMP_ERR_ENC_KEY_SIZE: u8 = 0x06;
const SMP_ERR_CMD_NOT_SUPP: u8 = 0x07;
const SMP_ERR_UNSPECIFIED: u8 = 0x08;
const SMP_ERR_REPEATED_ATTEMPTS: u8 = 0x09;
const SMP_ERR_INVALID_PARAMS: u8 = 0x0A;
const SMP_ERR_DHKEY_CHECK: u8 = 0x0B;
const SMP_ERR_NUMERIC_FAILED: u8 = 0x0C;

/// ACL PB (Packet Boundary) flags (Vol 2, Part E, §5.4.2)
const ACL_PB_FIRST_NON_AUTO: u16 = 0x0000;
const ACL_PB_CONTINUATION: u16 = 0x0001;
const ACL_PB_FIRST_AUTO: u16 = 0x0002;
const ACL_PB_COMPLETE: u16 = 0x0003;

/// Max advertising data length
const HCI_MAX_ADV_DATA_LEN: usize = 31;

/// BD_ADDR size
const BD_ADDR_SIZE: usize = 6;

/// LTK / IRK / CSRK size
const KEY_SIZE: usize = 16;

/// Max LE ACL MTU
const LE_ACL_MTU: u16 = 251;

/// HCI command opcode helper
const fn hci_opcode(ogf: u8, ocf: u16) -> u16 {
    (ocf & 0x03FF) | ((ogf as u16 & 0x003F) << 10)
}

// ============================================================================
// HCI Packet Structures
// ============================================================================

/// HCI Command header (Vol 2, Part E, §5.4.1)
///
/// | Field    | Size |
/// |----------|------|
/// | Opcode   | 2    | (OCF[10:0] | OGF[5:0]<<10)
/// | ParamLen | 1    |
/// | Params   | var  |
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct HciCmdHdr {
    pub opcode: u16,
    pub param_len: u8,
}

/// HCI Event header (Vol 2, Part E, §5.4.4)
///
/// | Field    | Size |
/// |----------|------|
/// | EventCode| 1    |
/// | ParamLen | 1    |
/// | Params   | var  |
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct HciEvtHdr {
    pub evt_code: u8,
    pub param_len: u8,
}

/// HCI ACL Data header (Vol 2, Part E, §5.4.2)
///
/// | Field    | Size |
/// |----------|------|
/// | Handle   | 2    | (Handle[11:0] | PB[1:0]<<12 | BC[1:0]<<14)
/// | DataLen  | 2    |
/// | Data     | var  |
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct HciAclHdr {
    pub handle_pb_bc: u16,
    pub data_len: u16,
}

impl HciAclHdr {
    pub fn connection_handle(&self) -> u16 {
        self.handle_pb_bc & 0x0FFF
    }

    pub fn pb_flag(&self) -> u16 {
        (self.handle_pb_bc >> 12) & 0x0003
    }

    pub fn bc_flag(&self) -> u16 {
        (self.handle_pb_bc >> 14) & 0x0003
    }
}

// ============================================================================
// HCI Event Parsing
// ============================================================================

/// Command Complete event parameters (Vol 2, Part E, §7.7.14.1)
///
/// | Field      | Size |
/// |------------|------|
/// | NumCmdPkts | 1    |
/// | CmdOpcode  | 2    |
/// | ReturnParams | var|
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct EvtCmdComplete {
    pub num_cmds: u8,
    pub opcode: u16,
}

/// Command Status event parameters (Vol 2, Part E, §7.7.14.3)
///
/// | Field      | Size |
/// |------------|------|
/// | Status     | 1    |
/// | NumCmdPkts | 1    |
/// | CmdOpcode  | 2    |
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct EvtCmdStatus {
    pub status: u8,
    pub num_cmds: u8,
    pub opcode: u16,
}

/// LE Meta Event header (Vol 2, Part E, §7.7.65)
///
/// | Field      | Size |
/// |------------|------|
/// | Subevent   | 1    |
/// | Params     | var  |
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct EvtLeMetaHdr {
    pub subevent: u8,
}

/// LE Connection Complete subevent (Vol 2, Part E, §7.7.65.1)
///
/// | Field           | Size |
/// |------------------|------|
/// | Status          | 1    |
/// | ConnHandle      | 2    |
/// | Role            | 1    |
/// | PeerAddrType    | 1    |
/// | PeerAddr        | 6    |
/// | ConnInterval    | 2    | (N * 1.25ms)
/// | ConnLatency     | 2    |
/// | SupervisionTmo  | 2    | (N * 10ms)
/// | MasterClkAcc    | 1    |
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct EvtLeConnComplete {
    pub status: u8,
    pub conn_handle: u16,
    pub role: u8,
    pub peer_addr_type: u8,
    pub peer_addr: [u8; 6],
    pub conn_interval: u16,
    pub conn_latency: u16,
    pub supervision_timeout: u16,
    pub master_clock_accuracy: u8,
}

/// LE Advertising Report subevent (Vol 2, Part E, §7.7.65.2)
///
/// | Field      | Size |
/// |------------|------|
/// | NumReports | 1    |
/// | Reports... | var  |
///
/// Per-report layout:
/// | Field      | Size |
/// |------------|------|
/// | EventType  | 1    |
/// | AddrType   | 1    |
/// | Address    | 6    |
/// | DataLen    | 1    |
/// | Data       | var  |
/// | RSSI       | 1    |
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct EvtLeAdvReport {
    pub event_type: u8,
    pub addr_type: u8,
    pub addr: [u8; 6],
    pub data_len: u8,
}

/// Disconnection Complete event (Vol 2, Part E, §7.7.14.4)
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct EvtDisconnComplete {
    pub status: u8,
    pub conn_handle: u16,
    pub reason: u8,
}

/// Read BD_ADDR return parameters (Vol 2, Part E, §7.4.6)
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct RpReadBdAddr {
    pub status: u8,
    pub bd_addr: [u8; 6],
}

/// LE Read Buffer Size return parameters (Vol 2, Part E, §7.8.9)
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct RpLeReadBufferSize {
    pub status: u8,
    pub le_acl_data_pkt_len: u16,
    pub le_total_num_acl_data_pkts: u8,
}

/// Read Local Version return parameters (Vol 2, Part E, §7.4.1)
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct RpReadLocalVersion {
    pub status: u8,
    pub hci_version: u8,
    pub hci_revision: u16,
    pub lmp_pal_version: u8,
    pub manufacturer: u16,
    pub lmp_pal_subversion: u16,
}

// ============================================================================
// HCI Command Parameter Structures
// ============================================================================

/// LE Set Advertising Parameters (Vol 2, Part E, §7.8.12)
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct CpLeSetAdvParam {
    pub adv_interval_min: u16, // N * 0.625ms (0x0020..0x4000)
    pub adv_interval_max: u16, // N * 0.625ms (0x0020..0x4000)
    pub adv_type: u8,
    pub own_addr_type: u8,
    pub peer_addr_type: u8,
    pub peer_addr: [u8; 6],
    pub adv_channel_map: u8,
    pub adv_filter_policy: u8,
}

/// LE Set Scan Parameters (Vol 2, Part E, §7.8.10)
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct CpLeSetScanParam {
    pub scan_type: u8,      // 0=passive, 1=active
    pub scan_interval: u16, // N * 0.625ms
    pub scan_window: u16,   // N * 0.625ms
    pub own_addr_type: u8,
    pub scan_filter_policy: u8,
}

/// LE Set Scan Enable (Vol 2, Part E, §7.8.11)
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct CpLeSetScanEnable {
    pub enable: u8, // 0=disable, 1=enable
    pub filter_duplicates: u8,
}

/// LE Create Connection (Vol 2, Part E, §7.8.12)
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct CpLeCreateConn {
    pub scan_interval: u16,
    pub scan_window: u16,
    pub filter_policy: u8,
    pub peer_addr_type: u8,
    pub peer_addr: [u8; 6],
    pub own_addr_type: u8,
    pub conn_interval_min: u16,   // N * 1.25ms
    pub conn_interval_max: u16,   // N * 1.25ms
    pub conn_latency: u16,        // 0..499
    pub supervision_timeout: u16, // N * 10ms
    pub min_ce_len: u16,
    pub max_ce_len: u16,
}

/// LE Set Advertising Data (Vol 2, Part E, §7.8.13)
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct CpLeSetAdvData {
    pub adv_data_len: u8,
    pub adv_data: [u8; 31],
}

/// LE Set Advertise Enable (Vol 2, Part E, §7.8.14)
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct CpLeSetAdvEnable {
    pub enable: u8,
}

/// Disconnect (Vol 2, Part E, §7.1.6)
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct CpDisconnect {
    pub conn_handle: u16,
    pub reason: u8,
}

// ============================================================================
// L2CAP Structures
// ============================================================================

/// L2CAP header (4 bytes, little-endian)
///
/// | Field      | Size |
/// |------------|------|
/// | PduLen     | 2    | (excludes this 4-byte header)
/// | ChannelId  | 2    |
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct L2capHdr {
    pub pdu_len: u16,
    pub cid: u16,
}

/// L2CAP Signaling C-frame header (CID 0x0005)
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct L2capSignalHdr {
    pub code: u8,
    pub identifier: u8,
    pub length: u16,
}

/// L2CAP Connection Parameter Update Request (CID 0x0005, Code 0x12)
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct L2capConnParamUpdateReq {
    pub interval_min: u16,
    pub interval_max: u16,
    pub latency: u16,
    pub timeout: u16,
}

/// L2CAP Connection Parameter Update Response (CID 0x0005, Code 0x13)
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct L2capConnParamUpdateRsp {
    pub result: u16, // 0=accepted, 1=rejected
}

// ============================================================================
// SMP Structures (Core Spec v5.4, Vol 3, Part H)
// ============================================================================

/// SMP Pairing Request/Response (10 bytes after Code)
///
/// | Field              | Size |
/// |---------------------|------|
/// | IOCapability       | 1    |
/// | OOBDataFlag        | 1    |
/// | AuthReq            | 1    |
/// | MaxEncryptionKeySz | 1    | (7..16)
/// | InitiatorKeyDist   | 1    |
/// | ResponderKeyDist   | 1    |
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct SmpPairingFeatures {
    pub io_capability: u8,
    pub oob_data_flag: u8,
    pub auth_req: u8,
    pub max_enc_key_size: u8,
    pub initiator_key_dist: u8,
    pub responder_key_dist: u8,
}

/// SMP Pairing Failed
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct SmpPairingFailed {
    pub reason: u8,
}

/// SMP Encryption Information (LTK)
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct SmpEncInfo {
    pub ltk: [u8; KEY_SIZE],
}

/// SMP Master Identification (EDIV + Rand)
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct SmpMasterIdent {
    pub ediv: u16,
    pub rand: [u8; 8],
}

/// SMP Identity Information (IRK)
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct SmpIdentInfo {
    pub irk: [u8; KEY_SIZE],
}

/// SMP Identity Address Information
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct SmpIdentAddrInfo {
    pub addr_type: u8,
    pub addr: [u8; 6],
}

/// SMP Signing Information (CSRK)
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct SmpSignInfo {
    pub csrk: [u8; KEY_SIZE],
}

/// SMP Security Request
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct SmpSecReq {
    pub auth_req: u8,
}

// ============================================================================
// SMP Pairing State Machine
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmpState {
    Idle,
    PairingReqSent,
    PairingRspReceived,
    PublicKeyExchanged,
    ConfirmSent,
    ConfirmReceived,
    RandomSent,
    RandomReceived,
    DhkeyCheckSent,
    DhkeyCheckReceived,
    KeyDistribution,
    Complete,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmpIoCapability {
    DisplayOnly,
    DisplayYesNo,
    KeyboardOnly,
    NoInputNoOutput,
    KeyboardDisplay,
}

impl SmpIoCapability {
    pub fn to_u8(self) -> u8 {
        match self {
            Self::DisplayOnly => SMP_IO_DISPLAY_ONLY,
            Self::DisplayYesNo => SMP_IO_DISPLAY_YESNO,
            Self::KeyboardOnly => SMP_IO_KEYBOARD_ONLY,
            Self::NoInputNoOutput => SMP_IO_NO_INPUT_OUTPUT,
            Self::KeyboardDisplay => SMP_IO_KEYBOARD_DISPLAY,
        }
    }
}

/// SMP pairing context
#[derive(Clone, Debug)]
pub struct SmpContext {
    pub state: SmpState,
    pub role: u8, // ROLE_CENTRAL or ROLE_PERIPHERAL
    pub io_capability: SmpIoCapability,
    pub auth_req: u8,
    pub max_enc_key_size: u8,
    pub initiator_key_dist: u8,
    pub responder_key_dist: u8,
    pub peer_io_capability: u8,
    pub peer_auth_req: u8,
    pub peer_max_enc_key_size: u8,
    pub peer_initiator_key_dist: u8,
    pub peer_responder_key_dist: u8,
    pub local_nonce: [u8; 16],
    pub peer_nonce: [u8; 16],
    pub local_confirm: [u8; 16],
    pub peer_confirm: [u8; 16],
    pub ltk: [u8; KEY_SIZE],
    pub ediv: u16,
    pub rand: [u8; 8],
    pub irk: [u8; KEY_SIZE],
    pub csrk: [u8; KEY_SIZE],
    pub peer_irk: [u8; KEY_SIZE],
    pub peer_addr_resolved: bool,
    // LE Secure Connections Phase 2
    pub local_public_key: [u8; 64],
    pub peer_public_key: [u8; 64],
    pub peer_dhkey_check: [u8; 16],
}

impl SmpContext {
    pub fn new(role: u8, io_cap: SmpIoCapability) -> Self {
        Self {
            state: SmpState::Idle,
            role,
            io_capability: io_cap,
            auth_req: SMP_AUTH_BONDING | SMP_AUTH_MITM | SMP_AUTH_SC | SMP_AUTH_CT2,
            max_enc_key_size: 16,
            initiator_key_dist: SMP_DIST_ENC_KEY | SMP_DIST_ID_KEY,
            responder_key_dist: SMP_DIST_ENC_KEY | SMP_DIST_ID_KEY,
            peer_io_capability: 0,
            peer_auth_req: 0,
            peer_max_enc_key_size: 0,
            peer_initiator_key_dist: 0,
            peer_responder_key_dist: 0,
            local_nonce: [0; 16],
            peer_nonce: [0; 16],
            local_confirm: [0; 16],
            peer_confirm: [0; 16],
            ltk: [0; KEY_SIZE],
            ediv: 0,
            rand: [0; 8],
            irk: [0; KEY_SIZE],
            csrk: [0; KEY_SIZE],
            peer_irk: [0; KEY_SIZE],
            peer_addr_resolved: false,
            local_public_key: [0; 64],
            peer_public_key: [0; 64],
            peer_dhkey_check: [0; 16],
        }
    }

    /// Build Pairing Request PDU
    pub fn build_pairing_req(&self) -> [u8; 7] {
        let mut pdu = [0u8; 7];
        pdu[0] = self.io_capability.to_u8();
        pdu[1] = 0; // OOB not available
        pdu[2] = self.auth_req;
        pdu[3] = self.max_enc_key_size;
        pdu[4] = self.initiator_key_dist;
        pdu[5] = self.responder_key_dist;
        pdu
    }

    /// Parse Pairing Response PDU
    pub fn parse_pairing_rsp(&mut self, data: &[u8]) -> bool {
        if data.len() < 6 {
            return false;
        }
        self.peer_io_capability = data[0];
        self.peer_auth_req = data[2];
        self.peer_max_enc_key_size = data[3];
        self.peer_initiator_key_dist = data[4];
        self.peer_responder_key_dist = data[5];
        true
    }

    /// Determine pairing method from IO capabilities (Core Spec v5.4, Vol 3, Part H, §2.3.2)
    pub fn pairing_method(&self) -> PairingMethod {
        let local = self.io_capability.to_u8();
        let peer = self.peer_io_capability;

        // LE Secure Connections pairing method selection table
        match (local, peer) {
            (SMP_IO_NO_INPUT_OUTPUT, _) | (_, SMP_IO_NO_INPUT_OUTPUT) => PairingMethod::JustWorks,
            (SMP_IO_DISPLAY_ONLY, SMP_IO_DISPLAY_ONLY)
            | (SMP_IO_DISPLAY_ONLY, SMP_IO_DISPLAY_YESNO) => PairingMethod::JustWorks,
            (SMP_IO_DISPLAY_ONLY, SMP_IO_KEYBOARD_ONLY)
            | (SMP_IO_DISPLAY_ONLY, SMP_IO_KEYBOARD_DISPLAY) => {
                PairingMethod::PasskeyEntryResponder
            }
            (SMP_IO_DISPLAY_YESNO, SMP_IO_DISPLAY_ONLY)
            | (SMP_IO_DISPLAY_YESNO, SMP_IO_DISPLAY_YESNO) => PairingMethod::NumericComparison,
            (SMP_IO_DISPLAY_YESNO, SMP_IO_KEYBOARD_ONLY)
            | (SMP_IO_DISPLAY_YESNO, SMP_IO_KEYBOARD_DISPLAY) => {
                PairingMethod::PasskeyEntryResponder
            }
            (SMP_IO_KEYBOARD_ONLY, SMP_IO_DISPLAY_ONLY)
            | (SMP_IO_KEYBOARD_ONLY, SMP_IO_DISPLAY_YESNO)
            | (SMP_IO_KEYBOARD_ONLY, SMP_IO_KEYBOARD_ONLY)
            | (SMP_IO_KEYBOARD_ONLY, SMP_IO_KEYBOARD_DISPLAY) => {
                PairingMethod::PasskeyEntryInitiator
            }
            (SMP_IO_KEYBOARD_DISPLAY, SMP_IO_DISPLAY_ONLY)
            | (SMP_IO_KEYBOARD_DISPLAY, SMP_IO_KEYBOARD_ONLY) => {
                PairingMethod::PasskeyEntryResponder
            }
            (SMP_IO_KEYBOARD_DISPLAY, SMP_IO_DISPLAY_YESNO)
            | (SMP_IO_KEYBOARD_DISPLAY, SMP_IO_KEYBOARD_DISPLAY) => {
                PairingMethod::NumericComparison
            }
            _ => PairingMethod::JustWorks,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PairingMethod {
    JustWorks,
    PasskeyEntryInitiator,
    PasskeyEntryResponder,
    NumericComparison,
    OutOfBand,
}

// ============================================================================
// LE Connection
// ============================================================================

#[derive(Clone, Debug)]
pub struct LeConnection {
    pub handle: u16,
    pub role: u8,
    pub peer_addr: [u8; 6],
    pub peer_addr_type: u8,
    pub conn_interval: u16, // N * 1.25ms
    pub conn_latency: u16,
    pub supervision_timeout: u16, // N * 10ms
    pub state: LeConnState,
    pub smp: Option<SmpContext>,
    pub acl_mtu: u16,
    pub acl_max_pkt: u8,
    pub acl_pending: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeConnState {
    Connecting,
    Connected,
    Pairing,
    Paired,
    Disconnecting,
    Disconnected,
}

// ============================================================================
// LE Device (scan result)
// ============================================================================

#[derive(Clone, Debug)]
pub struct LeDevice {
    pub addr: [u8; 6],
    pub addr_type: u8,
    pub rssi: i8,
    pub adv_type: u8,
    pub adv_data: Vec<u8>,
    pub name: String,
    pub services_16: Vec<u16>,
    pub flags: u8,
}

// ============================================================================
// HCI Command Queue
// ============================================================================

/// Pending HCI command with expected response opcode
#[derive(Clone, Debug)]
pub struct PendingHciCmd {
    pub opcode: u16,
    pub timeout_ms: u32,
    pub sent_at: u64,
}

/// HCI command queue (strictly one command at a time per spec)
pub struct HciCmdQueue {
    pub pending: Option<PendingHciCmd>,
    pub cmd_count: u32,
}

impl HciCmdQueue {
    pub fn new() -> Self {
        Self {
            pending: None,
            cmd_count: 0,
        }
    }

    pub fn is_busy(&self) -> bool {
        self.pending.is_some()
    }

    pub fn enqueue(&mut self, opcode: u16, timeout_ms: u32, now: u64) -> bool {
        if self.pending.is_some() {
            return false; // Already have a pending command
        }
        self.pending = Some(PendingHciCmd {
            opcode,
            timeout_ms,
            sent_at: now,
        });
        self.cmd_count += 1;
        true
    }

    pub fn complete(&mut self, completed_opcode: u16) -> bool {
        if let Some(pending) = &self.pending {
            if pending.opcode == completed_opcode {
                self.pending = None;
                return true;
            }
        }
        false
    }

    pub fn check_timeout(&self, now: u64) -> bool {
        if let Some(pending) = &self.pending {
            now.saturating_sub(pending.sent_at) > pending.timeout_ms as u64
        } else {
            false
        }
    }
}

// ============================================================================
// ACL Reassembly
// ============================================================================

/// ACL reassembly buffer for L2CAP PDUs
pub struct AclReassembler {
    pub conn_handle: u16,
    pub pdu_len: u16,
    pub received: u16,
    pub buffer: Vec<u8>,
}

impl AclReassembler {
    pub fn new() -> Self {
        Self {
            conn_handle: 0,
            pdu_len: 0,
            received: 0,
            buffer: Vec::with_capacity(LE_ACL_MTU as usize * 4),
        }
    }

    /// Feed an ACL fragment. Returns Some(complete_pdu) when reassembly is done.
    pub fn feed(&mut self, pb_flag: u16, conn_handle: u16, data: &[u8]) -> Option<Vec<u8>> {
        match pb_flag {
            ACL_PB_FIRST_NON_AUTO | ACL_PB_FIRST_AUTO => {
                // Start of new L2CAP PDU
                if data.len() < 4 {
                    return None; // Need at least L2CAP header
                }
                self.conn_handle = conn_handle;
                self.pdu_len = u16::from_le_bytes([data[0], data[1]]).saturating_add(4);
                self.received = data.len() as u16;
                self.buffer.clear();
                self.buffer.extend_from_slice(data);
                if self.received >= self.pdu_len {
                    let pdu = self.buffer.clone();
                    self.buffer.clear();
                    Some(pdu)
                } else {
                    None
                }
            }
            ACL_PB_CONTINUATION => {
                // Continuation fragment
                if conn_handle != self.conn_handle {
                    return None; // Wrong connection
                }
                self.buffer.extend_from_slice(data);
                self.received += data.len() as u16;
                if self.received >= self.pdu_len {
                    let pdu = self.buffer.clone();
                    self.buffer.clear();
                    Some(pdu)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

// ============================================================================
// Advertising Data Parser
// ============================================================================

/// Parse EIR/AD (Extended Inquiry Response / Advertising Data) structures
pub fn parse_adv_data(data: &[u8]) -> (String, Vec<u16>, u8) {
    let mut name = String::new();
    let mut services_16 = Vec::new();
    let mut flags = 0;
    let mut offset = 0;

    while offset < data.len() {
        let len = data[offset] as usize;
        if len == 0 || offset + 1 + len > data.len() {
            break;
        }
        let ad_type = data[offset + 1];
        let ad_data = &data[offset + 2..offset + 1 + len];

        match ad_type {
            0x01 => {
                // Flags
                if !ad_data.is_empty() {
                    flags = ad_data[0];
                }
            }
            0x08 | 0x09 => {
                // Shortened/Complete Local Name
                if let Ok(s) = core::str::from_utf8(ad_data) {
                    name = s.to_string();
                }
            }
            0x02 | 0x03 => {
                // Incomplete/Complete List of 16-bit Service UUIDs
                for chunk in ad_data.chunks(2) {
                    if chunk.len() == 2 {
                        services_16.push(u16::from_le_bytes([chunk[0], chunk[1]]));
                    }
                }
            }
            0x16 => {
                // Service Data - 16-bit UUID
                // First 2 bytes are UUID, rest is data
            }
            0xFF => {
                // Manufacturer Specific Data
            }
            _ => {}
        }

        offset += 1 + len;
    }

    (name, services_16, flags)
}

// ============================================================================
// HCI Event Dispatcher
// ============================================================================

/// Dispatched HCI event
#[derive(Clone, Debug)]
pub enum HciEvent {
    CmdComplete {
        opcode: u16,
        status: u8,
        params: Vec<u8>,
    },
    CmdStatus {
        status: u8,
        opcode: u16,
    },
    LeConnComplete {
        status: u8,
        conn_handle: u16,
        role: u8,
        peer_addr_type: u8,
        peer_addr: [u8; 6],
        conn_interval: u16,
        conn_latency: u16,
        supervision_timeout: u16,
    },
    LeAdvReport {
        event_type: u8,
        addr_type: u8,
        addr: [u8; 6],
        rssi: i8,
        adv_data: Vec<u8>,
    },
    DisconnComplete {
        status: u8,
        conn_handle: u16,
        reason: u8,
    },
    AclData {
        conn_handle: u16,
        pb_flag: u16,
        data: Vec<u8>,
    },
    NumCompletedPackets {
        handles: Vec<(u16, u16)>,
    },
    Unknown {
        evt_code: u8,
        params: Vec<u8>,
    },
}

/// Parse raw HCI event bytes into typed event
pub fn parse_hci_event(raw: &[u8]) -> Option<HciEvent> {
    if raw.len() < 2 {
        return None;
    }

    let evt_code = raw[0];
    let param_len = raw[1] as usize;

    if raw.len() < 2 + param_len {
        return None;
    }

    let params = &raw[2..2 + param_len];

    match evt_code {
        EVT_CMD_COMPLETE => {
            if params.len() < 3 {
                return None;
            }
            let num_cmds = params[0];
            let opcode = u16::from_le_bytes([params[1], params[2]]);
            let status = if params.len() > 3 { params[3] } else { 0 };
            let return_params = if params.len() > 4 {
                params[4..].to_vec()
            } else {
                Vec::new()
            };
            let _ = num_cmds;
            Some(HciEvent::CmdComplete {
                opcode,
                status,
                params: return_params,
            })
        }
        EVT_CMD_STATUS => {
            if params.len() < 4 {
                return None;
            }
            let status = params[0];
            let num_cmds = params[1];
            let opcode = u16::from_le_bytes([params[2], params[3]]);
            let _ = num_cmds;
            Some(HciEvent::CmdStatus { status, opcode })
        }
        EVT_LE_META => {
            if params.is_empty() {
                return None;
            }
            let subevent = params[0];
            let sub_params = &params[1..];

            match subevent {
                LE_SUBEV_CONN_COMPLETE => {
                    if sub_params.len() < 18 {
                        return None;
                    }
                    let evt = unsafe { &*(sub_params.as_ptr() as *const EvtLeConnComplete) };
                    Some(HciEvent::LeConnComplete {
                        status: evt.status,
                        conn_handle: evt.conn_handle,
                        role: evt.role,
                        peer_addr_type: evt.peer_addr_type,
                        peer_addr: evt.peer_addr,
                        conn_interval: evt.conn_interval,
                        conn_latency: evt.conn_latency,
                        supervision_timeout: evt.supervision_timeout,
                    })
                }
                LE_SUBEV_ADV_REPORT => {
                    if sub_params.is_empty() {
                        return None;
                    }
                    let num_reports = sub_params[0];
                    if num_reports == 0 || sub_params.len() < 2 {
                        return None;
                    }
                    // Parse first report only (simplified)
                    let report_offset = 1;
                    if sub_params.len() < report_offset + core::mem::size_of::<EvtLeAdvReport>() {
                        return None;
                    }
                    let report = unsafe {
                        &*(sub_params[report_offset..].as_ptr() as *const EvtLeAdvReport)
                    };
                    let data_start = report_offset + core::mem::size_of::<EvtLeAdvReport>();
                    let data_len = report.data_len as usize;
                    if data_start + data_len >= sub_params.len() {
                        return None;
                    }
                    let adv_data = sub_params[data_start..data_start + data_len].to_vec();
                    let rssi_offset = data_start + data_len;
                    if rssi_offset >= sub_params.len() {
                        return None;
                    }
                    let rssi = sub_params[rssi_offset] as i8;

                    Some(HciEvent::LeAdvReport {
                        event_type: report.event_type,
                        addr_type: report.addr_type,
                        addr: report.addr,
                        rssi,
                        adv_data,
                    })
                }
                _ => Some(HciEvent::Unknown {
                    evt_code,
                    params: params.to_vec(),
                }),
            }
        }
        EVT_DISCONN_COMPLETE => {
            if params.len() < 4 {
                return None;
            }
            let status = params[0];
            let conn_handle = u16::from_le_bytes([params[1], params[2]]);
            let reason = params[3];
            Some(HciEvent::DisconnComplete {
                status,
                conn_handle,
                reason,
            })
        }
        EVT_NUM_COMPLETED_PKTS => {
            if params.is_empty() {
                return None;
            }
            let num_handles = params[0];
            let mut handles = Vec::with_capacity(num_handles as usize);
            let mut offset = 1;
            for _ in 0..num_handles {
                if offset + 4 > params.len() {
                    break;
                }
                let handle = u16::from_le_bytes([params[offset], params[offset + 1]]);
                let num_pkts = u16::from_le_bytes([params[offset + 2], params[offset + 3]]);
                handles.push((handle, num_pkts));
                offset += 4;
            }
            Some(HciEvent::NumCompletedPackets { handles })
        }
        _ => Some(HciEvent::Unknown {
            evt_code,
            params: params.to_vec(),
        }),
    }
}

/// Parse ACL data packet
pub fn parse_acl_data(raw: &[u8]) -> Option<(u16, u16, Vec<u8>)> {
    if raw.len() < 4 {
        return None;
    }
    let handle_pb_bc = u16::from_le_bytes([raw[0], raw[1]]);
    let data_len = u16::from_le_bytes([raw[2], raw[3]]);

    let conn_handle = handle_pb_bc & 0x0FFF;
    let pb_flag = (handle_pb_bc >> 12) & 0x0003;

    if raw.len() < 4 + data_len as usize {
        return None;
    }

    let data = raw[4..4 + data_len as usize].to_vec();
    Some((conn_handle, pb_flag, data))
}

/// Parse L2CAP PDU
pub fn parse_l2cap_pdu(pdu: &[u8]) -> Option<(u16, &[u8])> {
    if pdu.len() < 4 {
        return None;
    }
    let pdu_len = u16::from_le_bytes([pdu[0], pdu[1]]) as usize;
    let cid = u16::from_le_bytes([pdu[2], pdu[3]]);

    if pdu.len() < 4 + pdu_len {
        return None;
    }

    Some((cid, &pdu[4..4 + pdu_len]))
}

/// Parse SMP PDU
pub fn parse_smp_pdu(pdu: &[u8]) -> Option<(u8, &[u8])> {
    if pdu.is_empty() {
        return None;
    }
    Some((pdu[0], &pdu[1..]))
}

// ============================================================================
// Bluetooth Jail Command/Response
// ============================================================================

#[derive(Clone, Debug)]
pub enum BtJailCommand {
    ResetController,
    ReadBdAddr,
    SetAdvParam {
        adv_min_interval: u16,
        adv_max_interval: u16,
        adv_type: u8,
        own_addr_type: u8,
        direct_addr: [u8; 6],
        channel_map: u8,
        filter_policy: u8,
    },
    SetAdvData {
        data: Vec<u8>,
    },
    SetAdvEnable(bool),
    SetScanEnable {
        enable: bool,
        filter_duplicates: bool,
    },
    SetScanParam {
        scan_type: u8,
        scan_interval: u16,
        scan_window: u16,
        own_addr_type: u8,
        filter_policy: u8,
    },
    CreateConn {
        peer_addr: [u8; 6],
        peer_addr_type: u8,
        conn_interval_min: u16,
        conn_interval_max: u16,
        conn_latency: u16,
        supervision_timeout: u16,
    },
    Disconnect {
        handle: u16,
        reason: u8,
    },
    StartPairing {
        conn_handle: u16,
        io_cap: SmpIoCapability,
    },
    GetStatus,
    GetFirmwareVersion,
}

#[derive(Clone, Debug)]
pub enum BtJailResponse {
    Ok,
    Error(BtJailError),
    BdAddr([u8; 6]),
    Status(BtJailStatus),
    ScanResults(Vec<LeDevice>),
    FirmwareVersion(String),
    ConnComplete { handle: u16, peer_addr: [u8; 6] },
    PairingComplete { success: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BtJailError {
    NotInitialized,
    HciTransportError,
    CommandTimeout,
    InvalidParameter,
    ControllerBusy,
    ConnectionFailed,
    AdvertisingFailed,
    ScanFailed,
    JailChannelClosed,
    PairingFailed,
    AclReassemblyError,
}

#[derive(Clone, Debug)]
pub struct BtJailStatus {
    pub initialized: bool,
    pub advertising: bool,
    pub scanning: bool,
    pub connection_count: usize,
    pub bd_addr: [u8; 6],
    pub hci_version: u8,
    pub manufacturer: u16,
    pub crash_count: u32,
    pub last_reboot_seq: u64,
}

// ============================================================================
// HCI Transport
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HciTransportType {
    Usb,
    Uart,
    Sdio,
}

pub struct HciTransport {
    pub transport_type: HciTransportType,
    pub initialized: bool,
    pub le_acl_mtu: u16,
    pub le_acl_max_pkts: u8,
    pub rx_buf: [u8; 512],
    pub rx_len: usize,
    pub tx_buf: [u8; 512],
    pub tx_len: usize,
}

impl HciTransport {
    pub fn new(transport_type: HciTransportType) -> Self {
        Self {
            transport_type,
            initialized: false,
            le_acl_mtu: LE_ACL_MTU,
            le_acl_max_pkts: 1,
            rx_buf: [0; 512],
            rx_len: 0,
            tx_buf: [0; 512],
            tx_len: 0,
        }
    }

    /// Build HCI command packet (Vol 2, Part E, §5.4.1)
    pub fn build_cmd(&mut self, ogf: u8, ocf: u16, params: &[u8]) -> usize {
        let opcode = hci_opcode(ogf, ocf);

        match self.transport_type {
            HciTransportType::Uart => {
                // UART H4: packet-type prefix (0x01 = Command)
                self.tx_buf[0] = HCI_CMD_PKT;
                self.tx_buf[1] = opcode as u8;
                self.tx_buf[2] = (opcode >> 8) as u8;
                self.tx_buf[3] = params.len() as u8;
                let param_len = params.len().min(252);
                self.tx_buf[4..4 + param_len].copy_from_slice(&params[..param_len]);
                4 + param_len
            }
            HciTransportType::Sdio => {
                // SDIO: function-level multiplexing, no H4 prefix
                // HCI command sent directly on BT function (typically function 1)
                self.tx_buf[0] = opcode as u8;
                self.tx_buf[1] = (opcode >> 8) as u8;
                self.tx_buf[2] = params.len() as u8;
                let param_len = params.len().min(252);
                self.tx_buf[3..3 + param_len].copy_from_slice(&params[..param_len]);
                3 + param_len
            }
            HciTransportType::Usb => {
                // USB: no packet-type byte (endpoint determines type)
                self.tx_buf[0] = opcode as u8;
                self.tx_buf[1] = (opcode >> 8) as u8;
                self.tx_buf[2] = params.len() as u8;
                let param_len = params.len().min(252);
                self.tx_buf[3..3 + param_len].copy_from_slice(&params[..param_len]);
                3 + param_len
            }
        }
    }

    /// Build ACL data packet (Vol 2, Part E, §5.4.2)
    pub fn build_acl(&mut self, conn_handle: u16, pb_flag: u16, data: &[u8]) -> usize {
        let handle_pb_bc = (conn_handle & 0x0FFF) | ((pb_flag & 0x0003) << 12);

        match self.transport_type {
            HciTransportType::Uart => {
                // UART H4: packet-type prefix (0x02 = ACL)
                self.tx_buf[0] = HCI_ACL_PKT;
                self.tx_buf[1] = handle_pb_bc as u8;
                self.tx_buf[2] = (handle_pb_bc >> 8) as u8;
                self.tx_buf[3] = data.len() as u8;
                self.tx_buf[4] = (data.len() >> 8) as u8;
                let data_len = data.len().min(507);
                self.tx_buf[5..5 + data_len].copy_from_slice(&data[..data_len]);
                5 + data_len
            }
            HciTransportType::Sdio | HciTransportType::Usb => {
                // SDIO/USB: no packet-type byte
                self.tx_buf[0] = handle_pb_bc as u8;
                self.tx_buf[1] = (handle_pb_bc >> 8) as u8;
                self.tx_buf[2] = data.len() as u8;
                self.tx_buf[3] = (data.len() >> 8) as u8;
                let data_len = data.len().min(508);
                self.tx_buf[4..4 + data_len].copy_from_slice(&data[..data_len]);
                4 + data_len
            }
        }
    }

    /// Parse received HCI event (assumes packet-type byte already stripped for UART)
    pub fn parse_event(&self, data: &[u8]) -> Option<HciEvent> {
        parse_hci_event(data)
    }

    /// Parse received ACL data
    pub fn parse_acl(&self, data: &[u8]) -> Option<(u16, u16, Vec<u8>)> {
        parse_acl_data(data)
    }
}

// ============================================================================
// Bluetooth Jail Controller
// ============================================================================

pub struct BtJailController {
    transport: Mutex<HciTransport>,
    bd_addr: Mutex<[u8; 6]>,
    hci_version: AtomicU8,
    manufacturer: AtomicU16,
    advertising: AtomicBool,
    scanning: AtomicBool,
    connections: Mutex<Vec<LeConnection>>,
    ready: AtomicBool,
    pub jail_id: u32,

    // HCI command queue
    cmd_queue: Mutex<HciCmdQueue>,

    // ACL reassembly per connection
    acl_reassembler: Mutex<AclReassembler>,

    // Crash-only microreboot state
    crash_count: AtomicU32,
    reboot_seq: AtomicU64,
    last_healthy_seq: AtomicU64,
    jail_channel: Option<JailChannel>,
    watchdog_timeout_ms: AtomicU32,
    last_heartbeat: AtomicU64,

    // Scanning results
    scan_results: Mutex<Vec<LeDevice>>,

    // SMP contexts per connection
    smp_contexts: Mutex<Vec<(u16, SmpContext)>>,
}

impl BtJailController {
    pub fn new(transport_type: HciTransportType) -> Self {
        Self {
            transport: Mutex::new(HciTransport::new(transport_type)),
            bd_addr: Mutex::new([0; 6]),
            hci_version: AtomicU8::new(0),
            manufacturer: AtomicU16::new(0),
            advertising: AtomicBool::new(false),
            scanning: AtomicBool::new(false),
            connections: Mutex::new(Vec::new()),
            ready: AtomicBool::new(false),
            jail_id: 0,
            cmd_queue: Mutex::new(HciCmdQueue::new()),
            acl_reassembler: Mutex::new(AclReassembler::new()),
            crash_count: AtomicU32::new(0),
            reboot_seq: AtomicU64::new(0),
            last_healthy_seq: AtomicU64::new(0),
            jail_channel: None,
            watchdog_timeout_ms: AtomicU32::new(5000),
            last_heartbeat: AtomicU64::new(0),
            scan_results: Mutex::new(Vec::new()),
            smp_contexts: Mutex::new(Vec::new()),
        }
    }

    // ========================================================================
    // Crash-Only Microreboot Contract (MINIX 3 model)
    // ========================================================================

    pub fn crash_and_reboot(&self) -> Result<u64, BtJailError> {
        self.crash_count.fetch_add(1, Ordering::SeqCst);
        let new_seq = self.reboot_seq.fetch_add(1, Ordering::SeqCst) + 1;

        crate::serial_println!(
            "[BT-Jail] Crash #{}, initiating microreboot seq={}",
            self.crash_count.load(Ordering::Relaxed),
            new_seq
        );

        self.reset_internal()?;
        self.ready.store(true, Ordering::SeqCst);
        self.last_healthy_seq.store(new_seq, Ordering::SeqCst);

        crate::serial_println!("[BT-Jail] Microreboot complete, seq={}", new_seq);
        Ok(new_seq)
    }

    fn reset_internal(&self) -> Result<(), BtJailError> {
        self.advertising.store(false, Ordering::SeqCst);
        self.scanning.store(false, Ordering::SeqCst);
        *self.connections.lock() = Vec::new();
        *self.scan_results.lock() = Vec::new();
        *self.cmd_queue.lock() = HciCmdQueue::new();
        *self.acl_reassembler.lock() = AclReassembler::new();
        *self.smp_contexts.lock() = Vec::new();

        crate::serial_println!("[BT-Jail] Controller reset complete");
        Ok(())
    }

    pub fn heartbeat(&self) {
        let now = crate::interrupts::get_ticks();
        self.last_heartbeat.store(now, Ordering::Relaxed);
    }

    pub fn check_watchdog(&self) -> bool {
        let now = crate::interrupts::get_ticks();
        let last = self.last_heartbeat.load(Ordering::Relaxed);
        let timeout = self.watchdog_timeout_ms.load(Ordering::Relaxed) as u64;
        now.saturating_sub(last) > timeout
    }

    // ========================================================================
    // HCI Command Helpers
    // ========================================================================

    fn send_hci_cmd(&self, ogf: u8, ocf: u16, params: &[u8]) -> Result<usize, BtJailError> {
        let opcode = hci_opcode(ogf, ocf);
        let now = crate::interrupts::get_ticks();

        let mut queue = self.cmd_queue.lock();
        if queue.is_busy() {
            return Err(BtJailError::ControllerBusy);
        }
        queue.enqueue(opcode, 2000, now);
        drop(queue);

        let len = {
            let mut transport = self.transport.lock();
            let len = transport.build_cmd(ogf, ocf, params);
            transport.tx_len = len;
            len
        };

        crate::serial_println!(
            "[BT-Jail] HCI CMD: ogf={:#04x} ocf={:#06x} opcode={:#06x} len={}",
            ogf,
            ocf,
            opcode,
            params.len()
        );

        Ok(len)
    }

    fn complete_hci_cmd(&self, opcode: u16) {
        self.cmd_queue.lock().complete(opcode);
    }

    // ========================================================================
    // Initialization Sequence
    // ========================================================================

    pub fn init(&mut self) -> Result<(), BtJailError> {
        crate::serial_println!("[BT-Jail] TIER 2 Bluetooth Jail initializing...");

        // Step 1: HCI Reset
        self.send_hci_cmd(OGF_CONTROLLER_BASEBAND, OCF_RESET, &[])?;

        // Step 2: Read Local Version
        self.send_hci_cmd(OGF_INFO_PARAMS, 0x0001, &[])?;

        // Step 3: Read BD_ADDR
        self.send_hci_cmd(OGF_INFO_PARAMS, OCF_READ_BD_ADDR, &[])?;

        // Step 4: LE Set Event Mask (enable all LE events)
        let le_event_mask: [u8; 8] = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x3F];
        self.send_hci_cmd(OGF_LE, OCF_LE_SET_EVENT_MASK, &le_event_mask)?;

        // Step 5: Set Event Mask (enable BR/EDR events)
        let event_mask: [u8; 8] = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x37, 0x00, 0xC0];
        self.send_hci_cmd(OGF_CONTROLLER_BASEBAND, 0x0001, &event_mask)?;

        self.transport.lock().initialized = true;
        self.ready.store(true, Ordering::SeqCst);

        crate::serial_println!("[BT-Jail] Controller initialized");
        Ok(())
    }

    // ========================================================================
    // Event Processing
    // ========================================================================

    /// Process incoming HCI event bytes
    pub fn process_event(&self, raw: &[u8]) -> Option<HciEvent> {
        let event = self.transport.lock().parse_event(raw)?;

        match &event {
            HciEvent::CmdComplete {
                opcode,
                status,
                params,
            } => {
                self.complete_hci_cmd(*opcode);

                // Handle specific command completions
                if *status == HCI_SUCCESS {
                    if *opcode == hci_opcode(OGF_INFO_PARAMS, OCF_READ_BD_ADDR) {
                        if params.len() >= 6 {
                            let mut addr = [0u8; 6];
                            addr.copy_from_slice(&params[..6]);
                            // Note: bd_addr is stored in little-endian in HCI
                            self.bd_addr.lock().copy_from_slice(&addr);
                            crate::serial_println!(
                                "[BT-Jail] BD_ADDR: {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
                                addr[5],
                                addr[4],
                                addr[3],
                                addr[2],
                                addr[1],
                                addr[0]
                            );
                        }
                    } else if *opcode == hci_opcode(OGF_INFO_PARAMS, 0x0001) {
                        // Read Local Version
                        if params.len() >= 7 {
                            self.hci_version.store(params[0], Ordering::Relaxed);
                            self.manufacturer.store(
                                u16::from_le_bytes([params[2], params[3]]),
                                Ordering::Relaxed,
                            );
                        }
                    }
                }
            }
            HciEvent::LeConnComplete {
                status,
                conn_handle,
                role,
                peer_addr_type,
                peer_addr,
                conn_interval,
                conn_latency,
                supervision_timeout,
            } => {
                if *status == HCI_SUCCESS {
                    let conn = LeConnection {
                        handle: *conn_handle,
                        role: *role,
                        peer_addr: *peer_addr,
                        peer_addr_type: *peer_addr_type,
                        conn_interval: *conn_interval,
                        conn_latency: *conn_latency,
                        supervision_timeout: *supervision_timeout,
                        state: LeConnState::Connected,
                        smp: None,
                        acl_mtu: self.transport.lock().le_acl_mtu,
                        acl_max_pkt: self.transport.lock().le_acl_max_pkts,
                        acl_pending: 0,
                    };
                    self.connections.lock().push(conn);
                    self.advertising.store(false, Ordering::SeqCst);
                    self.scanning.store(false, Ordering::SeqCst);
                }
            }
            HciEvent::LeAdvReport {
                event_type,
                addr_type,
                addr,
                rssi,
                adv_data,
            } => {
                let (name, services_16, flags) = parse_adv_data(adv_data);
                let device = LeDevice {
                    addr: *addr,
                    addr_type: *addr_type,
                    rssi: *rssi,
                    adv_type: *event_type,
                    adv_data: adv_data.clone(),
                    name,
                    services_16,
                    flags,
                };
                self.add_scan_result(device);
            }
            HciEvent::DisconnComplete {
                conn_handle,
                reason,
                ..
            } => {
                self.connections.lock().retain(|c| c.handle != *conn_handle);
                self.smp_contexts.lock().retain(|(h, _)| *h != *conn_handle);
                crate::serial_println!(
                    "[BT-Jail] Disconnected: handle={:#06x} reason={:#04x}",
                    conn_handle,
                    reason
                );
            }
            HciEvent::AclData {
                conn_handle,
                pb_flag,
                data,
            } => {
                // Feed to ACL reassembler
                let mut reassembler = self.acl_reassembler.lock();
                if let Some(pdu) = reassembler.feed(*pb_flag, *conn_handle, data) {
                    drop(reassembler);
                    // Process L2CAP PDU
                    self.process_l2cap_pdu(*conn_handle, &pdu);
                }
            }
            _ => {}
        }

        Some(event)
    }

    /// Process L2CAP PDU
    fn process_l2cap_pdu(&self, conn_handle: u16, pdu: &[u8]) {
        if let Some((cid, payload)) = parse_l2cap_pdu(pdu) {
            match cid {
                L2CAP_CID_SMP => {
                    self.process_smp_pdu(conn_handle, payload);
                }
                L2CAP_CID_ATT => {
                    // ATT/GATT processing (future)
                }
                L2CAP_CID_LE_SIGNALING => {
                    self.process_le_signaling(conn_handle, payload);
                }
                _ => {}
            }
        }
    }

    /// Process SMP PDU
    fn process_smp_pdu(&self, conn_handle: u16, pdu: &[u8]) {
        if let Some((code, data)) = parse_smp_pdu(pdu) {
            match code {
                SMP_PAIRING_RSP => {
                    // Find SMP context for this connection
                    let mut contexts = self.smp_contexts.lock();
                    if let Some((_, ctx)) = contexts.iter_mut().find(|(h, _)| *h == conn_handle) {
                        if ctx.parse_pairing_rsp(data) {
                            ctx.state = SmpState::PairingRspReceived;
                            crate::serial_println!("[BT-Jail] SMP: Pairing Response received");
                        }
                    }
                }
                SMP_PAIRING_CONFIRM => {
                    let mut contexts = self.smp_contexts.lock();
                    if let Some((_, ctx)) = contexts.iter_mut().find(|(h, _)| *h == conn_handle) {
                        if data.len() >= 16 {
                            ctx.peer_confirm.copy_from_slice(&data[..16]);
                            ctx.state = SmpState::ConfirmReceived;
                        }
                    }
                }
                SMP_PAIRING_RANDOM => {
                    let mut contexts = self.smp_contexts.lock();
                    if let Some((_, ctx)) = contexts.iter_mut().find(|(h, _)| *h == conn_handle) {
                        if data.len() >= 16 {
                            ctx.peer_nonce.copy_from_slice(&data[..16]);
                            ctx.state = SmpState::RandomReceived;
                        }
                    }
                }
                SMP_PAIRING_FAILED => {
                    let reason = if !data.is_empty() { data[0] } else { 0 };
                    crate::serial_println!("[BT-Jail] SMP: Pairing Failed, reason={:#04x}", reason);
                    let mut contexts = self.smp_contexts.lock();
                    if let Some((_, ctx)) = contexts.iter_mut().find(|(h, _)| *h == conn_handle) {
                        ctx.state = SmpState::Failed;
                    }
                }
                SMP_ENC_INFO => {
                    let mut contexts = self.smp_contexts.lock();
                    if let Some((_, ctx)) = contexts.iter_mut().find(|(h, _)| *h == conn_handle) {
                        if data.len() >= KEY_SIZE {
                            ctx.ltk.copy_from_slice(&data[..KEY_SIZE]);
                        }
                    }
                }
                SMP_MASTER_IDENT => {
                    let mut contexts = self.smp_contexts.lock();
                    if let Some((_, ctx)) = contexts.iter_mut().find(|(h, _)| *h == conn_handle) {
                        if data.len() >= 10 {
                            ctx.ediv = u16::from_le_bytes([data[0], data[1]]);
                            ctx.rand.copy_from_slice(&data[2..10]);
                        }
                    }
                }
                SMP_IDENT_INFO => {
                    let mut contexts = self.smp_contexts.lock();
                    if let Some((_, ctx)) = contexts.iter_mut().find(|(h, _)| *h == conn_handle) {
                        if data.len() >= KEY_SIZE {
                            ctx.peer_irk.copy_from_slice(&data[..KEY_SIZE]);
                        }
                    }
                }
                SMP_IDENT_ADDR_INFO => {
                    // Peer identity address received
                }
                SMP_PAIRING_PUBLIC_KEY => {
                    // LE Secure Connections Phase 2: Public Key exchange
                    // Per BT Core Spec v5.4 Vol 3, Part H §2.3.5.2
                    if data.len() >= 64 {
                        let mut contexts = self.smp_contexts.lock();
                        if let Some((_, ctx)) = contexts.iter_mut().find(|(h, _)| *h == conn_handle)
                        {
                            ctx.peer_public_key.copy_from_slice(&data[..64]);
                            ctx.state = SmpState::PublicKeyExchanged;
                            crate::serial_println!(
                                "[BT-Jail] SMP: Public Key received (LE SC Phase 2)"
                            );
                        }
                    }
                }
                SMP_DHKEY_CHECK => {
                    // LE Secure Connections Phase 2: DHKey Check
                    // Per BT Core Spec v5.4 Vol 3, Part H §2.3.5.6
                    if data.len() >= 16 {
                        let mut contexts = self.smp_contexts.lock();
                        if let Some((_, ctx)) = contexts.iter_mut().find(|(h, _)| *h == conn_handle)
                        {
                            ctx.peer_dhkey_check.copy_from_slice(&data[..16]);
                            ctx.state = SmpState::DhkeyCheckReceived;
                            crate::serial_println!(
                                "[BT-Jail] SMP: DHKey Check received (LE SC Phase 2)"
                            );
                        }
                    }
                }
                SMP_KEYPRESS_NOTIFY => {
                    // Keypress Notification (optional, BT Core Spec v5.4 Vol 3, Part H §2.3.5.5)
                    if !data.is_empty() {
                        crate::serial_println!("[BT-Jail] SMP: Keypress Notify type={}", data[0]);
                    }
                }
                SMP_SEC_REQ => {
                    // Security request from peer
                    let auth_req = if !data.is_empty() { data[0] } else { 0 };
                    crate::serial_println!(
                        "[BT-Jail] SMP: Security Request, auth_req={:#04x}",
                        auth_req
                    );
                }
                _ => {
                    crate::serial_println!("[BT-Jail] SMP: Unknown code={:#04x}", code);
                }
            }
        }
    }

    /// Process LE Signaling
    fn process_le_signaling(&self, conn_handle: u16, payload: &[u8]) {
        if payload.len() < 4 {
            return;
        }
        let code = payload[0];
        let identifier = payload[1];
        let length = u16::from_le_bytes([payload[2], payload[3]]);

        // Bounds check: payload must contain 4-byte header + length bytes of data
        if payload.len() < 4 + length as usize {
            crate::serial_println!(
                "[BT-Jail] L2CAP: truncated signal packet (need {} got {})",
                4 + length,
                payload.len()
            );
            return;
        }

        let _ = (conn_handle, identifier, length);

        match code {
            0x12 => {
                // Connection Parameter Update Request
                if payload.len() >= 12 {
                    let req = unsafe {
                        core::ptr::read_unaligned(
                            payload[4..].as_ptr() as *const L2capConnParamUpdateReq
                        )
                    };
                    let imin = req.interval_min;
                    let imax = req.interval_max;
                    let lat = req.latency;
                    let tmo = req.timeout;
                    crate::serial_println!(
                        "[BT-Jail] L2CAP: Conn Param Update Req: interval={}-{} latency={} timeout={}",
                        imin, imax, lat, tmo
                    );
                }
            }
            0x13 => {
                // Connection Parameter Update Response
                if payload.len() >= 6 {
                    let rsp = unsafe {
                        core::ptr::read_unaligned(
                            payload[4..].as_ptr() as *const L2capConnParamUpdateRsp
                        )
                    };
                    let result = rsp.result;
                    crate::serial_println!(
                        "[BT-Jail] L2CAP: Conn Param Update Rsp: result={}",
                        result
                    );
                }
            }
            _ => {}
        }
    }

    // ========================================================================
    // LE Advertising
    // ========================================================================

    pub fn set_adv_param(
        &self,
        adv_min_interval: u16,
        adv_max_interval: u16,
        adv_type: u8,
        own_addr_type: u8,
        direct_addr: [u8; 6],
        channel_map: u8,
        filter_policy: u8,
    ) -> Result<(), BtJailError> {
        let mut params = [0u8; 15];
        params[0..2].copy_from_slice(&adv_min_interval.to_le_bytes());
        params[2..4].copy_from_slice(&adv_max_interval.to_le_bytes());
        params[4] = adv_type;
        params[5] = own_addr_type;
        params[6..12].copy_from_slice(&direct_addr);
        params[12] = channel_map;
        params[13] = filter_policy;

        crate::serial_println!(
            "[BT-Jail] LE Set Adv Param: type={} interval={}-{}",
            adv_type,
            adv_min_interval,
            adv_max_interval
        );

        self.send_hci_cmd(OGF_LE, OCF_LE_SET_ADV_PARAM, &params)?;
        Ok(())
    }

    pub fn set_adv_data(&self, data: &[u8]) -> Result<(), BtJailError> {
        let mut params = [0u8; 32];
        let len = data.len().min(HCI_MAX_ADV_DATA_LEN);
        params[0] = len as u8;
        params[1..1 + len].copy_from_slice(&data[..len]);

        crate::serial_println!("[BT-Jail] LE Set Adv Data: {} bytes", len);
        self.send_hci_cmd(OGF_LE, OCF_LE_SET_ADV_DATA, &params)?;
        Ok(())
    }

    pub fn set_adv_enable(&self, enable: bool) -> Result<(), BtJailError> {
        let params = [if enable { 1 } else { 0 }];
        self.send_hci_cmd(OGF_LE, OCF_LE_SET_ADV_ENABLE, &params)?;
        self.advertising.store(enable, Ordering::SeqCst);
        crate::serial_println!(
            "[BT-Jail] LE Advertising {}",
            if enable { "enabled" } else { "disabled" }
        );
        Ok(())
    }

    // ========================================================================
    // LE Scanning
    // ========================================================================

    pub fn set_scan_param(
        &self,
        scan_type: u8,
        scan_interval: u16,
        scan_window: u16,
        own_addr_type: u8,
        filter_policy: u8,
    ) -> Result<(), BtJailError> {
        let mut params = [0u8; 7];
        params[0] = scan_type;
        params[1..3].copy_from_slice(&scan_interval.to_le_bytes());
        params[3..5].copy_from_slice(&scan_window.to_le_bytes());
        params[5] = own_addr_type;
        params[6] = filter_policy;

        crate::serial_println!(
            "[BT-Jail] LE Set Scan Param: type={} interval={} window={}",
            scan_type,
            scan_interval,
            scan_window
        );

        self.send_hci_cmd(OGF_LE, OCF_LE_SET_SCAN_PARAM, &params)?;
        Ok(())
    }

    pub fn set_scan_enable(
        &self,
        enable: bool,
        filter_duplicates: bool,
    ) -> Result<(), BtJailError> {
        let mut params = [0u8; 2];
        params[0] = if enable { 1 } else { 0 };
        params[1] = if filter_duplicates { 1 } else { 0 };

        self.send_hci_cmd(OGF_LE, OCF_LE_SET_SCAN_ENABLE, &params)?;

        if enable {
            self.scanning.store(true, Ordering::SeqCst);
            *self.scan_results.lock() = Vec::new();
            crate::serial_println!("[BT-Jail] LE Scanning enabled");
        } else {
            self.scanning.store(false, Ordering::SeqCst);
            crate::serial_println!("[BT-Jail] LE Scanning disabled");
        }
        Ok(())
    }

    pub fn add_scan_result(&self, device: LeDevice) {
        let mut results = self.scan_results.lock();
        if !results.iter().any(|d| d.addr == device.addr) {
            results.push(device);
        }
    }

    // ========================================================================
    // LE Connection
    // ========================================================================

    pub fn create_conn(
        &self,
        peer_addr: [u8; 6],
        peer_addr_type: u8,
        conn_interval_min: u16,
        conn_interval_max: u16,
        conn_latency: u16,
        supervision_timeout: u16,
    ) -> Result<(), BtJailError> {
        let mut params = [0u8; 25];
        params[0..2].copy_from_slice(&60u16.to_le_bytes()); // scan_interval
        params[2..4].copy_from_slice(&30u16.to_le_bytes()); // scan_window
        params[4] = FILTER_POLICY_ALL;
        params[5] = peer_addr_type;
        params[6..12].copy_from_slice(&peer_addr);
        params[12] = ADDR_PUBLIC;
        params[13..15].copy_from_slice(&conn_interval_min.to_le_bytes());
        params[15..17].copy_from_slice(&conn_interval_max.to_le_bytes());
        params[17..19].copy_from_slice(&conn_latency.to_le_bytes());
        params[19..21].copy_from_slice(&supervision_timeout.to_le_bytes());
        params[21..23].copy_from_slice(&0u16.to_le_bytes()); // min_ce_len
        params[23..25].copy_from_slice(&0u16.to_le_bytes()); // max_ce_len

        crate::serial_println!(
            "[BT-Jail] LE Create Conn: {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
            peer_addr[5],
            peer_addr[4],
            peer_addr[3],
            peer_addr[2],
            peer_addr[1],
            peer_addr[0]
        );

        self.send_hci_cmd(OGF_LE, OCF_LE_CREATE_CONN, &params)?;
        Ok(())
    }

    pub fn disconnect(&self, handle: u16, reason: u8) -> Result<(), BtJailError> {
        let mut params = [0u8; 3];
        params[0..2].copy_from_slice(&handle.to_le_bytes());
        params[2] = reason;

        self.send_hci_cmd(OGF_LINK_CONTROL, 0x0006, &params)?;

        let mut conns = self.connections.lock();
        conns.retain(|c| c.handle != handle);
        crate::serial_println!(
            "[BT-Jail] Disconnect: handle={:#06x} reason={:#04x}",
            handle,
            reason
        );
        Ok(())
    }

    // ========================================================================
    // SMP Pairing
    // ========================================================================

    pub fn start_pairing(
        &self,
        conn_handle: u16,
        io_cap: SmpIoCapability,
    ) -> Result<(), BtJailError> {
        let conn_exists = self
            .connections
            .lock()
            .iter()
            .any(|c| c.handle == conn_handle);
        if !conn_exists {
            return Err(BtJailError::ConnectionFailed);
        }

        let ctx = SmpContext::new(ROLE_CENTRAL, io_cap);
        self.smp_contexts.lock().push((conn_handle, ctx.clone()));

        // Build SMP Pairing Request
        let mut smp_pdu = [0u8; 7];
        smp_pdu[0] = SMP_PAIRING_REQ;
        let features = ctx.build_pairing_req();
        smp_pdu[1..7].copy_from_slice(&features);

        // Build L2CAP header + SMP PDU
        let l2cap_len = (4 + smp_pdu.len()) as u16;
        let mut l2cap_pdu = vec![0u8; l2cap_len as usize];
        l2cap_pdu[0..2].copy_from_slice(&(smp_pdu.len() as u16).to_le_bytes());
        l2cap_pdu[2..4].copy_from_slice(&L2CAP_CID_SMP.to_le_bytes());
        l2cap_pdu[4..].copy_from_slice(&smp_pdu);

        // Build ACL packet
        let acl_len = {
            let mut transport = self.transport.lock();
            let len = transport.build_acl(conn_handle, ACL_PB_FIRST_NON_AUTO, &l2cap_pdu);
            transport.tx_len = len;
            len
        };

        crate::serial_println!(
            "[BT-Jail] SMP: Pairing Request sent (handle={:#06x})",
            conn_handle
        );
        Ok(())
    }

    // ========================================================================
    // Advertising Data Builder
    // ========================================================================

    pub fn build_adv_data(name: &str, services: &[u16]) -> Vec<u8> {
        let mut data = Vec::with_capacity(31);

        // Flags: LE General Discoverable, BR/EDR Not Supported
        data.push(0x02);
        data.push(0x01);
        data.push(0x06);

        // Shortened Local Name
        if !name.is_empty() {
            let name_bytes = name.as_bytes();
            let len = name_bytes.len().min(28);
            data.push((len + 1) as u8);
            data.push(0x08);
            data.extend_from_slice(&name_bytes[..len]);
        }

        // 16-bit Service UUIDs
        if !services.is_empty() {
            let count = services.len().min(14);
            data.push((count * 2 + 1) as u8);
            data.push(0x02);
            for &uuid in &services[..count] {
                data.extend_from_slice(&uuid.to_le_bytes());
            }
        }

        data
    }

    // ========================================================================
    // Jail Command Processing
    // ========================================================================

    pub fn process_command(&self, cmd: BtJailCommand) -> BtJailResponse {
        if !self.ready.load(Ordering::SeqCst) {
            return BtJailResponse::Error(BtJailError::NotInitialized);
        }

        match cmd {
            BtJailCommand::ResetController => match self.crash_and_reboot() {
                Ok(seq) => {
                    crate::serial_println!("[BT-Jail] Controller rebooted, seq={}", seq);
                    BtJailResponse::Ok
                }
                Err(e) => BtJailResponse::Error(e),
            },
            BtJailCommand::ReadBdAddr => BtJailResponse::BdAddr(*self.bd_addr.lock()),
            BtJailCommand::SetAdvParam {
                adv_min_interval,
                adv_max_interval,
                adv_type,
                own_addr_type,
                direct_addr,
                channel_map,
                filter_policy,
            } => match self.set_adv_param(
                adv_min_interval,
                adv_max_interval,
                adv_type,
                own_addr_type,
                direct_addr,
                channel_map,
                filter_policy,
            ) {
                Ok(()) => BtJailResponse::Ok,
                Err(e) => BtJailResponse::Error(e),
            },
            BtJailCommand::SetAdvData { data } => match self.set_adv_data(&data) {
                Ok(()) => BtJailResponse::Ok,
                Err(e) => BtJailResponse::Error(e),
            },
            BtJailCommand::SetAdvEnable(enable) => match self.set_adv_enable(enable) {
                Ok(()) => BtJailResponse::Ok,
                Err(e) => BtJailResponse::Error(e),
            },
            BtJailCommand::SetScanEnable {
                enable,
                filter_duplicates,
            } => match self.set_scan_enable(enable, filter_duplicates) {
                Ok(()) => {
                    if !enable {
                        let results = self.scan_results.lock().clone();
                        BtJailResponse::ScanResults(results)
                    } else {
                        BtJailResponse::Ok
                    }
                }
                Err(e) => BtJailResponse::Error(e),
            },
            BtJailCommand::SetScanParam {
                scan_type,
                scan_interval,
                scan_window,
                own_addr_type,
                filter_policy,
            } => match self.set_scan_param(
                scan_type,
                scan_interval,
                scan_window,
                own_addr_type,
                filter_policy,
            ) {
                Ok(()) => BtJailResponse::Ok,
                Err(e) => BtJailResponse::Error(e),
            },
            BtJailCommand::CreateConn {
                peer_addr,
                peer_addr_type,
                conn_interval_min,
                conn_interval_max,
                conn_latency,
                supervision_timeout,
            } => match self.create_conn(
                peer_addr,
                peer_addr_type,
                conn_interval_min,
                conn_interval_max,
                conn_latency,
                supervision_timeout,
            ) {
                Ok(()) => BtJailResponse::Ok,
                Err(e) => BtJailResponse::Error(e),
            },
            BtJailCommand::Disconnect { handle, reason } => match self.disconnect(handle, reason) {
                Ok(()) => BtJailResponse::Ok,
                Err(e) => BtJailResponse::Error(e),
            },
            BtJailCommand::StartPairing {
                conn_handle,
                io_cap,
            } => match self.start_pairing(conn_handle, io_cap) {
                Ok(()) => BtJailResponse::PairingComplete { success: true },
                Err(e) => BtJailResponse::PairingComplete { success: false },
            },
            BtJailCommand::GetStatus => BtJailResponse::Status(self.get_status()),
            BtJailCommand::GetFirmwareVersion => BtJailResponse::FirmwareVersion(format!(
                "echOS-BT-Jail v1.0.0 HCI={}.{} MFG={:#06x}",
                self.hci_version.load(Ordering::Relaxed),
                self.transport.lock().le_acl_mtu,
                self.manufacturer.load(Ordering::Relaxed)
            )),
        }
    }

    pub fn get_status(&self) -> BtJailStatus {
        BtJailStatus {
            initialized: self.ready.load(Ordering::Relaxed),
            advertising: self.advertising.load(Ordering::Relaxed),
            scanning: self.scanning.load(Ordering::Relaxed),
            connection_count: self.connections.lock().len(),
            bd_addr: *self.bd_addr.lock(),
            hci_version: self.hci_version.load(Ordering::Relaxed),
            manufacturer: self.manufacturer.load(Ordering::Relaxed),
            crash_count: self.crash_count.load(Ordering::Relaxed),
            last_reboot_seq: self.reboot_seq.load(Ordering::Relaxed),
        }
    }

    // ========================================================================
    // JailChannel Integration
    // ========================================================================

    pub fn attach_channel(&mut self, channel: JailChannel) {
        let cid = channel.channel_id;
        self.jail_channel = Some(channel);
        crate::serial_println!("[BT-Jail] JailChannel {} attached", cid);
    }

    pub fn poll_requests(&self) -> Option<JailRequest> {
        self.jail_channel.as_ref().and_then(|ch| ch.poll_request())
    }

    pub fn submit_event(&self, event: JailEvent) -> Result<(), JailEvent> {
        match &self.jail_channel {
            Some(ch) => ch.submit_event(event),
            None => Err(event),
        }
    }

    pub fn handle_jail_request(&self, req: JailRequest) -> JailEvent {
        let result = match req.opcode {
            JailOpcode::Read => 0i64,
            JailOpcode::Write => req.length as i64,
            JailOpcode::Control => 0i64,
            JailOpcode::Reset => match self.crash_and_reboot() {
                Ok(_) => 0i64,
                Err(_) => -1i64,
            },
            JailOpcode::Status => 0i64,
            JailOpcode::Nop => 0i64,
            JailOpcode::Flush => 0i64,
        };

        JailEvent {
            request_id: req.request_id,
            result,
            data_len: if result >= 0 { result as u32 } else { 0 },
            jail_id: self.jail_id as u16,
            flags: 0,
        }
    }
}

// ============================================================================
// Global Registry
// ============================================================================

lazy_static::lazy_static! {
    static ref BT_CONTROLLERS: Mutex<Vec<BtJailController>> = Mutex::new(Vec::new());
}

pub fn init() {
    crate::serial_println!("[BT-Jail] TIER 2 Bluetooth Jail driver initializing...");

    // USB Bluetooth: class=0xE0, subclass=0x01, protocol=0x01
    let devices = crate::drivers::pci::scan();
    for dev in devices {
        let is_bt = (dev.class_code == 0xE0 && dev.subclass == 0x01)
            || (dev.vendor_id == 0x8087 && dev.device_id == 0x0026)
            || (dev.vendor_id == 0x8087 && dev.device_id == 0x0AA7)
            || (dev.vendor_id == 0x8086 && dev.class_code == 0x0D);

        if is_bt {
            crate::serial_println!(
                "[BT-Jail] Found BT adapter: {:04x}:{:04x} (class={:02x}.{:02x})",
                dev.vendor_id,
                dev.device_id,
                dev.class_code,
                dev.subclass
            );

            let ctrl = BtJailController::new(HciTransportType::Usb);
            BT_CONTROLLERS.lock().push(ctrl);
        }
    }

    if BT_CONTROLLERS.lock().is_empty() {
        crate::serial_println!(
            "[BT-Jail] No Bluetooth adapter found; controller registry remains empty"
        );
    }
}

pub fn controller_count() -> usize {
    BT_CONTROLLERS.lock().len()
}

// ============================================================================
// Host Corpus Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hci_opcode_encoding() {
        // Reset: OGF=0x03, OCF=0x0003 → 0x0C03
        assert_eq!(hci_opcode(0x03, 0x0003), 0x0C03);
        // Read BD_ADDR: OGF=0x04, OCF=0x0009 → 0x1009
        assert_eq!(hci_opcode(0x04, 0x0009), 0x1009);
        // LE Set Adv Enable: OGF=0x08, OCF=0x000A → 0x200A
        assert_eq!(hci_opcode(0x08, 0x000A), 0x200A);
        // LE Create Connection: OGF=0x08, OCF=0x000D → 0x200D
        assert_eq!(hci_opcode(0x08, 0x000D), 0x200D);
    }

    #[test]
    fn hci_cmd_complete_parsing() {
        // Raw event: EventCode=0x0E, ParamLen=4, NumCmds=1, Opcode=0x0C03, Status=0x00
        let raw = [0x0E, 0x04, 0x01, 0x03, 0x0C, 0x00];
        let event = parse_hci_event(&raw).unwrap();
        match event {
            HciEvent::CmdComplete { opcode, status, .. } => {
                assert_eq!(opcode, 0x0C03);
                assert_eq!(status, 0x00);
            }
            _ => panic!("Expected CmdComplete"),
        }
    }

    #[test]
    fn hci_cmd_status_parsing() {
        // Raw event: EventCode=0x0F, ParamLen=4, Status=0x00, NumCmds=1, Opcode=0x200D
        let raw = [0x0F, 0x04, 0x00, 0x01, 0x0D, 0x20];
        let event = parse_hci_event(&raw).unwrap();
        match event {
            HciEvent::CmdStatus { status, opcode } => {
                assert_eq!(status, 0x00);
                assert_eq!(opcode, 0x200D);
            }
            _ => panic!("Expected CmdStatus"),
        }
    }

    #[test]
    fn le_conn_complete_parsing() {
        // LE Meta event with Connection Complete subevent
        let mut raw = [0u8; 22];
        raw[0] = EVT_LE_META;
        raw[1] = 19; // param_len
        raw[2] = LE_SUBEV_CONN_COMPLETE;
        raw[3] = HCI_SUCCESS;
        raw[4..6].copy_from_slice(&0x0042u16.to_le_bytes()); // conn_handle
        raw[6] = ROLE_CENTRAL;
        raw[7] = ADDR_PUBLIC;
        raw[8..14].copy_from_slice(&[0x11, 0x22, 0x33, 0x44, 0x55, 0x66]);
        raw[14..16].copy_from_slice(&24u16.to_le_bytes()); // 30ms interval
        raw[16..18].copy_from_slice(&0u16.to_le_bytes()); // latency
        raw[18..20].copy_from_slice(&500u16.to_le_bytes()); // 5s timeout
        raw[20] = 0; // master_clock_accuracy

        let event = parse_hci_event(&raw).unwrap();
        match event {
            HciEvent::LeConnComplete {
                conn_handle,
                role,
                peer_addr,
                conn_interval,
                supervision_timeout,
                ..
            } => {
                assert_eq!(conn_handle, 0x0042);
                assert_eq!(role, ROLE_CENTRAL);
                assert_eq!(peer_addr, [0x11, 0x22, 0x33, 0x44, 0x55, 0x66]);
                assert_eq!(conn_interval, 24);
                assert_eq!(supervision_timeout, 500);
            }
            _ => panic!("Expected LeConnComplete"),
        }
    }

    #[test]
    fn le_adv_report_parsing() {
        // LE Meta event with Advertising Report subevent
        let adv_payload = [0x02, 0x01, 0x06, 0x05, 0x09, 0x65, 0x63, 0x68]; // Flags + "ech"
        let mut raw = vec![0u8; 16 + adv_payload.len()];
        raw[0] = EVT_LE_META;
        raw[1] = (12 + adv_payload.len()) as u8;
        raw[2] = LE_SUBEV_ADV_REPORT;
        raw[3] = 1; // num_reports
        raw[4] = ADV_IND; // event_type
        raw[5] = ADDR_RANDOM; // addr_type
        raw[6..12].copy_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
        raw[12] = adv_payload.len() as u8;
        raw[13..13 + adv_payload.len()].copy_from_slice(&adv_payload);
        raw[13 + adv_payload.len()] = 0xC8; // RSSI = -56

        let event = parse_hci_event(&raw).unwrap();
        match event {
            HciEvent::LeAdvReport {
                addr_type,
                addr,
                rssi,
                adv_data,
                ..
            } => {
                assert_eq!(addr_type, ADDR_RANDOM);
                assert_eq!(addr, [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
                assert_eq!(rssi, -56);
                assert_eq!(adv_data, adv_payload);
            }
            _ => panic!("Expected LeAdvReport"),
        }
    }

    #[test]
    fn adv_data_parsing() {
        let data = [
            0x02, 0x01, 0x06, // Flags: LE General, BR/EDR not supported
            0x04, 0x09, 0x65, 0x63, 0x68, // Complete Local Name: "ech"
            0x05, 0x02, 0x0F, 0x18, 0x0D,
            0x18, // Complete 16-bit UUIDs: Battery (0x180F), Health (0x180D)
        ];
        let (name, services, flags) = parse_adv_data(&data);
        assert_eq!(name, "ech");
        assert_eq!(services, vec![0x180F, 0x180D]);
        assert_eq!(flags, 0x06);
    }

    #[test]
    fn acl_header_decode() {
        let raw = [0x42, 0x00, 0x0A, 0x00]; // handle=0x42, pb=0, data_len=10
        let (conn_handle, pb_flag, data_len) = {
            let handle_pb_bc = u16::from_le_bytes([raw[0], raw[1]]);
            let data_len = u16::from_le_bytes([raw[2], raw[3]]);
            (
                handle_pb_bc & 0x0FFF,
                (handle_pb_bc >> 12) & 0x0003,
                data_len,
            )
        };
        assert_eq!(conn_handle, 0x0042);
        assert_eq!(pb_flag, ACL_PB_FIRST_NON_AUTO);
        assert_eq!(data_len, 10);
    }

    #[test]
    fn l2cap_pdu_parsing() {
        // L2CAP: pdu_len=7, cid=0x0006 (SMP), payload=SMP Pairing Request
        let pdu = [
            0x07,
            0x00,
            0x06,
            0x00,
            SMP_PAIRING_REQ,
            0x03,
            0x00,
            0x0B,
            0x10,
            0x01,
            0x03,
        ];
        let (cid, payload) = parse_l2cap_pdu(&pdu).unwrap();
        assert_eq!(cid, L2CAP_CID_SMP);
        assert_eq!(payload[0], SMP_PAIRING_REQ);
        assert_eq!(payload.len(), 7);
    }

    #[test]
    fn smp_pairing_req_build() {
        let ctx = SmpContext::new(ROLE_CENTRAL, SmpIoCapability::NoInputNoOutput);
        let features = ctx.build_pairing_req();
        assert_eq!(features[0], SMP_IO_NO_INPUT_OUTPUT);
        assert_eq!(
            features[2],
            SMP_AUTH_BONDING | SMP_AUTH_MITM | SMP_AUTH_SC | SMP_AUTH_CT2
        );
        assert_eq!(features[3], 16); // max_enc_key_size
        assert_eq!(features[4], SMP_DIST_ENC_KEY | SMP_DIST_ID_KEY);
        assert_eq!(features[5], SMP_DIST_ENC_KEY | SMP_DIST_ID_KEY);
    }

    #[test]
    fn smp_pairing_method_just_works() {
        let mut ctx = SmpContext::new(ROLE_CENTRAL, SmpIoCapability::NoInputNoOutput);
        ctx.peer_io_capability = SMP_IO_DISPLAY_ONLY;
        assert_eq!(ctx.pairing_method(), PairingMethod::JustWorks);
    }

    #[test]
    fn smp_pairing_method_numeric_comparison() {
        let mut ctx = SmpContext::new(ROLE_CENTRAL, SmpIoCapability::DisplayYesNo);
        ctx.peer_io_capability = SMP_IO_DISPLAY_YESNO;
        assert_eq!(ctx.pairing_method(), PairingMethod::NumericComparison);
    }

    #[test]
    fn smp_pairing_method_passkey_initiator() {
        let mut ctx = SmpContext::new(ROLE_CENTRAL, SmpIoCapability::KeyboardOnly);
        ctx.peer_io_capability = SMP_IO_DISPLAY_ONLY;
        assert_eq!(ctx.pairing_method(), PairingMethod::PasskeyEntryInitiator);
    }

    #[test]
    fn hci_cmd_queue_single() {
        let mut queue = HciCmdQueue::new();
        assert!(!queue.is_busy());
        assert!(queue.enqueue(0x0C03, 2000, 0));
        assert!(queue.is_busy());
        assert!(queue.complete(0x0C03));
        assert!(!queue.is_busy());
    }

    #[test]
    fn hci_cmd_queue_busy_reject() {
        let mut queue = HciCmdQueue::new();
        queue.enqueue(0x0C03, 2000, 0);
        assert!(!queue.enqueue(0x1009, 2000, 0)); // Should fail, already busy
    }

    #[test]
    fn hci_cmd_queue_timeout() {
        let mut queue = HciCmdQueue::new();
        queue.enqueue(0x0C03, 2000, 0);
        assert!(!queue.check_timeout(1000)); // Not timed out yet
        assert!(queue.check_timeout(3000)); // Timed out
    }

    #[test]
    fn acl_reassembly_two_fragments() {
        let mut reassembler = AclReassembler::new();

        // First fragment: L2CAP header + partial payload
        let l2cap_header = [0x07, 0x00, 0x06, 0x00]; // payload_len=7, cid=0x0006
        let first_frag = [
            l2cap_header[0],
            l2cap_header[1],
            l2cap_header[2],
            l2cap_header[3],
            SMP_PAIRING_REQ,
            0x03,
            0x00,
            0x0B,
        ];
        assert!(reassembler
            .feed(ACL_PB_FIRST_NON_AUTO, 0x0042, &first_frag)
            .is_none());

        // Second fragment: remaining payload
        let second_frag = [0x10, 0x01, 0x03];
        let pdu = reassembler
            .feed(ACL_PB_CONTINUATION, 0x0042, &second_frag)
            .unwrap();
        assert_eq!(pdu.len(), 11);
        assert_eq!(pdu[0], 0x07); // payload_len low
        assert_eq!(pdu[2], 0x06); // cid low
        assert_eq!(pdu[4], SMP_PAIRING_REQ);
    }

    #[test]
    fn disconn_complete_parsing() {
        let raw = [EVT_DISCONN_COMPLETE, 0x04, 0x00, 0x42, 0x00, 0x13];
        let event = parse_hci_event(&raw).unwrap();
        match event {
            HciEvent::DisconnComplete {
                conn_handle,
                reason,
                ..
            } => {
                assert_eq!(conn_handle, 0x0042);
                assert_eq!(reason, 0x13); // Remote User Terminated
            }
            _ => panic!("Expected DisconnComplete"),
        }
    }

    #[test]
    fn num_completed_packets_parsing() {
        let raw = [
            EVT_NUM_COMPLETED_PKTS,
            0x09,
            0x02,
            0x42,
            0x00,
            0x03,
            0x00,
            0x43,
            0x00,
            0x05,
            0x00,
        ];
        let event = parse_hci_event(&raw).unwrap();
        match event {
            HciEvent::NumCompletedPackets { handles } => {
                assert_eq!(handles.len(), 2);
                assert_eq!(handles[0], (0x0042, 3));
                assert_eq!(handles[1], (0x0043, 5));
            }
            _ => panic!("Expected NumCompletedPackets"),
        }
    }

    #[test]
    fn bt_jail_controller_creation() {
        let ctrl = BtJailController::new(HciTransportType::Usb);
        assert!(!ctrl.ready.load(Ordering::Relaxed));
        assert!(!ctrl.advertising.load(Ordering::Relaxed));
        assert!(!ctrl.scanning.load(Ordering::Relaxed));
        assert_eq!(*ctrl.bd_addr.lock(), [0; 6]);
    }

    #[test]
    fn bt_jail_status_initial() {
        let ctrl = BtJailController::new(HciTransportType::Uart);
        let status = ctrl.get_status();
        assert!(!status.initialized);
        assert!(!status.advertising);
        assert!(!status.scanning);
        assert_eq!(status.connection_count, 0);
        assert_eq!(status.crash_count, 0);
    }

    #[test]
    fn bt_jail_adv_data_builder() {
        let data = BtJailController::build_adv_data("echOS", &[0x180F, 0x180D]);
        assert!(!data.is_empty());
        assert!(data.len() <= 31);
        // Check flags
        assert_eq!(data[0], 0x02); // length
        assert_eq!(data[1], 0x01); // type: flags
        assert_eq!(data[2], 0x06); // LE general, BR/EDR not supported
    }

    #[test]
    fn bt_jail_command_not_initialized() {
        let ctrl = BtJailController::new(HciTransportType::Usb);
        let resp = ctrl.process_command(BtJailCommand::GetStatus);
        match resp {
            BtJailResponse::Error(BtJailError::NotInitialized) => {}
            _ => panic!("Expected NotInitialized error"),
        }
    }

    #[test]
    fn bt_jail_crash_reboot() {
        let mut ctrl = BtJailController::new(HciTransportType::Usb);
        ctrl.ready.store(true, Ordering::SeqCst);

        let seq = ctrl.crash_and_reboot().unwrap();
        assert_eq!(seq, 1);
        assert_eq!(ctrl.crash_count.load(Ordering::Relaxed), 1);
        assert!(ctrl.ready.load(Ordering::Relaxed));
    }

    #[test]
    fn bt_jail_multiple_crashes() {
        let mut ctrl = BtJailController::new(HciTransportType::Usb);
        ctrl.ready.store(true, Ordering::SeqCst);

        ctrl.crash_and_reboot().unwrap();
        ctrl.crash_and_reboot().unwrap();
        ctrl.crash_and_reboot().unwrap();

        assert_eq!(ctrl.crash_count.load(Ordering::Relaxed), 3);
        assert_eq!(ctrl.reboot_seq.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn smp_pairing_rsp_parsing() {
        let mut ctx = SmpContext::new(ROLE_CENTRAL, SmpIoCapability::NoInputNoOutput);
        let rsp_data = [SMP_IO_DISPLAY_YESNO, 0x00, 0x0B, 0x10, 0x03, 0x03];
        assert!(ctx.parse_pairing_rsp(&rsp_data));
        assert_eq!(ctx.peer_io_capability, SMP_IO_DISPLAY_YESNO);
        assert_eq!(ctx.peer_auth_req, 0x0B);
        assert_eq!(ctx.peer_max_enc_key_size, 0x10);
    }

    #[test]
    fn hci_transport_uart_cmd_build() {
        let mut transport = HciTransport::new(HciTransportType::Uart);
        let len = transport.build_cmd(OGF_CONTROLLER_BASEBAND, OCF_RESET, &[]);
        assert_eq!(len, 4); // packet_type(1) + opcode(2) + param_len(1)
        assert_eq!(transport.tx_buf[0], HCI_CMD_PKT);
        assert_eq!(transport.tx_buf[1], 0x03); // opcode low
        assert_eq!(transport.tx_buf[2], 0x0C); // opcode high
        assert_eq!(transport.tx_buf[3], 0x00); // param_len
    }

    #[test]
    fn hci_transport_usb_cmd_build() {
        let mut transport = HciTransport::new(HciTransportType::Usb);
        let len = transport.build_cmd(OGF_CONTROLLER_BASEBAND, OCF_RESET, &[]);
        assert_eq!(len, 3); // opcode(2) + param_len(1), no packet_type
        assert_eq!(transport.tx_buf[0], 0x03); // opcode low
        assert_eq!(transport.tx_buf[1], 0x0C); // opcode high
        assert_eq!(transport.tx_buf[2], 0x00); // param_len
    }

    #[test]
    fn hci_transport_uart_acl_build() {
        let mut transport = HciTransport::new(HciTransportType::Uart);
        let data = [0x01, 0x02, 0x03];
        let len = transport.build_acl(0x0042, ACL_PB_FIRST_NON_AUTO, &data);
        assert_eq!(len, 8); // packet_type(1) + handle(2) + data_len(2) + data(3)
        assert_eq!(transport.tx_buf[0], HCI_ACL_PKT);
    }

    #[test]
    fn smp_context_default_key_dist() {
        let ctx = SmpContext::new(ROLE_CENTRAL, SmpIoCapability::NoInputNoOutput);
        assert_eq!(ctx.initiator_key_dist, SMP_DIST_ENC_KEY | SMP_DIST_ID_KEY);
        assert_eq!(ctx.responder_key_dist, SMP_DIST_ENC_KEY | SMP_DIST_ID_KEY);
    }

    #[test]
    fn le_conn_state_transitions() {
        let conn = LeConnection {
            handle: 0x0042,
            role: ROLE_CENTRAL,
            peer_addr: [0x11, 0x22, 0x33, 0x44, 0x55, 0x66],
            peer_addr_type: ADDR_PUBLIC,
            conn_interval: 24,
            conn_latency: 0,
            supervision_timeout: 500,
            state: LeConnState::Connecting,
            smp: None,
            acl_mtu: LE_ACL_MTU,
            acl_max_pkt: 1,
            acl_pending: 0,
        };
        assert_eq!(conn.state, LeConnState::Connecting);
        assert_eq!(conn.handle, 0x0042);
    }
}
