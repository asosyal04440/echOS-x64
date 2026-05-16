//! # WiFi Jail — TIER 2 Kablosuz Ag Adaptörü Surucusu
//!
//! WiFi donanimı guvenilmez vendor driver'lar barındırdıgından TIER 2 (JAIL)
//! sınıfında calıstırılır. Tum MMIO erisimi SPSC ring buffer uzerinden
//! sandbox icinden gecer.
//!
//! ## Mimari
//!
//! ```text
//! ┌────────────────┐     SPSC Ring      ┌────────────────┐     DMA/MMIO    ┌──────────┐
//! │  Core (Stack)  │ ◄════════════════► │  WiFi Jail     │ ◄════════════► │  WiFi    │
//! │  (Consumer)    │   CommandRing      │  (Tier 2)      │   TX/RX Rings  │  HW      │
//! │                │   CompletionRing   │                │                │          │
//! │  scan()        │                    │  FW load       │                │          │
//! │  connect()     │                    │  Scan FSM      │                │          │
//! │  tx/rx()       │                    │  Assoc FSM     │                │          │
//! └────────────────┘                    └────────────────┘                └──────────┘
//! ```
//!
//! ## Firmware Yukleme (Linux iwlwifi modeli)
//!
//! ```text
//! INIT → LOAD (ucode sections) → VERIFY (checksum) → RUN (init handshake) → OPERATIONAL
//! ```
//!
//! ## 802.11 Tarama (IEEE 802.11-2024 §9.6)
//!
//! - Pasif tarama: Her kanalda beacon dinle (dwell time)
//! - Aktif tarama: Probe Request gonder, Probe Response topla
//!
//! ## Iliskilendirme (IEEE 802.11-2024 §9.3, §9.4)
//!
//! ```text
//! Authentication (Open System) → Association Request/Response → 4-Way Handshake (EAPOL-Key)
//! ```
//!
//! ## Desteklenen Standartlar
//!
//! - IEEE 802.11a/b/g/n/ac/ax/be (WiFi 6E/7)
//! - WPA2-PSK / WPA3-SAE
//! - WPA2-Enterprise (802.1X/EAP)
//! - MLO (Multi-Link Operation, 802.11be)
//!
//! ## Guvenlik
//!
//! - Tum firmware komutları sandbox icinden gonderilir
//! - MMIO register erisimi JailWorker tarafında denetlenir
//! - DMA buffer'lar izole fiziksel bolgede tahsis edilir
//! - Crash-only microreboot ile izolasyon (MINIX 3 modeli)

use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use sha1::{Digest as Sha1Digest, Sha1};
use spin::Mutex;

use crate::drivers::jail_ring::{JailChannel, JailEvent, JailOpcode, JailRequest};

// ============================================================================
// WiFi Sabitleri
// ============================================================================

/// WiFi bant turleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WifiBand {
    Band2G,
    Band5G,
    Band6G,
}

/// WiFi guvenlik protokolu
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WifiSecurity {
    Open,
    WEP,
    WPA,
    WPA2Personal,
    WPA2Enterprise,
    WPA3Personal,
    WPA3Enterprise,
}

impl WifiSecurity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Open => "Open",
            Self::WEP => "WEP",
            Self::WPA => "WPA",
            Self::WPA2Personal => "WPA2-PSK",
            Self::WPA2Enterprise => "WPA2-EAP",
            Self::WPA3Personal => "WPA3-SAE",
            Self::WPA3Enterprise => "WPA3-EAP",
        }
    }
}

/// WiFi PHY modu
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WifiPhyMode {
    Dot11B,
    Dot11G,
    Dot11N,
    Dot11AC,
    Dot11AX,
    Dot11BE,
}

/// WiFi baglantı durumu
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WifiState {
    Disconnected,
    Scanning,
    Authenticating,
    Associating,
    KeyExchange,
    Connected,
    Roaming,
}

// ============================================================================
// 802.11 MAC Header (IEEE 802.11-2024 §9.2)
// ============================================================================

/// Frame Control alanı (2 byte)
///
/// Layout:
/// | Bits  | Field              |
/// |-------|---------------------|
/// | 0-1   | Protocol Version   |
/// | 2-3   | Type (Mgmt/Ctrl/Data)|
/// | 4-7   | Subtype            |
/// | 8     | To DS              |
/// | 9     | From DS            |
/// | 10    | More Fragments     |
/// | 11    | Retry              |
/// | 12    | Power Management   |
/// | 13    | More Data          |
/// | 14    | Protected Frame    |
/// | 15    | Order              |
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameControl {
    pub raw: u16,
}

impl FrameControl {
    pub const FRAME_TYPE_MGMT: u16 = 0x0000;
    pub const FRAME_TYPE_CTRL: u16 = 0x0004;
    pub const FRAME_TYPE_DATA: u16 = 0x0008;

    // Management subtypes (Linux: IEEE80211_STYPE_*)
    pub const SUBTYPE_ASSOC_REQ: u16 = 0x0000;
    pub const SUBTYPE_ASSOC_RESP: u16 = 0x0010;
    pub const SUBTYPE_REASSOC_REQ: u16 = 0x0020;
    pub const SUBTYPE_REASSOC_RESP: u16 = 0x0030;
    pub const SUBTYPE_PROBE_REQ: u16 = 0x0040;
    pub const SUBTYPE_PROBE_RESP: u16 = 0x0050;
    pub const SUBTYPE_BEACON: u16 = 0x0080;
    pub const SUBTYPE_ATIM: u16 = 0x0090;
    pub const SUBTYPE_DISASSOC: u16 = 0x00A0;
    pub const SUBTYPE_AUTH: u16 = 0x00B0;
    pub const SUBTYPE_DEAUTH: u16 = 0x00C0;
    pub const SUBTYPE_ACTION: u16 = 0x00D0;

    // Control subtypes (Linux: IEEE80211_STYPE_*)
    pub const SUBTYPE_BACK_REQ: u16 = 0x0080;
    pub const SUBTYPE_BACK: u16 = 0x0090;
    pub const SUBTYPE_PSPOLL: u16 = 0x00A0;
    pub const SUBTYPE_RTS: u16 = 0x00B0;
    pub const SUBTYPE_CTS: u16 = 0x00C0;
    pub const SUBTYPE_ACK: u16 = 0x00D0;
    pub const SUBTYPE_CF_END: u16 = 0x00E0;
    pub const SUBTYPE_CF_END_ACK: u16 = 0x00F0;

    // Data subtypes (Linux: IEEE80211_STYPE_*)
    pub const SUBTYPE_DATA: u16 = 0x0000;
    pub const SUBTYPE_NULLFUNC: u16 = 0x0040;
    pub const SUBTYPE_QOS_DATA: u16 = 0x0080;
    pub const SUBTYPE_QOS_NULLFUNC: u16 = 0x00C0;

    pub fn new(ftype: u16, subtype: u16) -> Self {
        Self {
            raw: ftype | subtype,
        }
    }

    pub fn frame_type(&self) -> u16 {
        self.raw & 0x000C
    }

    pub fn subtype(&self) -> u16 {
        self.raw & 0x00F0
    }

    pub fn to_ds(&self) -> bool {
        (self.raw & 0x0100) != 0
    }

    pub fn from_ds(&self) -> bool {
        (self.raw & 0x0200) != 0
    }

    pub fn protected(&self) -> bool {
        (self.raw & 0x4000) != 0
    }

    pub fn is_mgmt(&self) -> bool {
        self.frame_type() == Self::FRAME_TYPE_MGMT
    }

    pub fn is_ctrl(&self) -> bool {
        self.frame_type() == Self::FRAME_TYPE_CTRL
    }

    pub fn is_data(&self) -> bool {
        self.frame_type() == Self::FRAME_TYPE_DATA
    }

    pub fn is_beacon(&self) -> bool {
        self.is_mgmt() && self.subtype() == Self::SUBTYPE_BEACON
    }

    pub fn is_probe_req(&self) -> bool {
        self.is_mgmt() && self.subtype() == Self::SUBTYPE_PROBE_REQ
    }

    pub fn is_probe_resp(&self) -> bool {
        self.is_mgmt() && self.subtype() == Self::SUBTYPE_PROBE_RESP
    }

    pub fn is_auth(&self) -> bool {
        self.is_mgmt() && self.subtype() == Self::SUBTYPE_AUTH
    }

    pub fn is_assoc_req(&self) -> bool {
        self.is_mgmt() && self.subtype() == Self::SUBTYPE_ASSOC_REQ
    }

    pub fn is_assoc_resp(&self) -> bool {
        self.is_mgmt() && self.subtype() == Self::SUBTYPE_ASSOC_RESP
    }

    pub fn is_deauth(&self) -> bool {
        self.is_mgmt() && self.subtype() == Self::SUBTYPE_DEAUTH
    }
}

/// 802.11 MAC header (minimum 24 byte)
///
/// Layout (IEEE 802.11-2024 §9.2.4):
/// | Offset | Field            | Size   |
/// |--------|-------------------|--------|
/// | 0      | Frame Control    | 2      |
/// | 2      | Duration/ID      | 2      |
/// | 4      | Address 1 (RA)   | 6      |
/// | 10     | Address 2 (TA)   | 6      |
/// | 16     | Address 3 (BSSID)| 6      |
/// | 22     | Sequence Control | 2      |
/// | 24     | Address 4 (opt)  | 6      |
/// | 24/30  | QoS Control (opt)| 2      |
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MacHeader {
    pub frame_ctrl: FrameControl,
    pub duration: u16,
    pub addr1: [u8; 6],
    pub addr2: [u8; 6],
    pub addr3: [u8; 6],
    pub seq_ctrl: u16,
}

impl MacHeader {
    pub const MIN_SIZE: usize = 24;

    pub fn new(fc: FrameControl, ra: [u8; 6], ta: [u8; 6], bssid: [u8; 6]) -> Self {
        Self {
            frame_ctrl: fc,
            duration: 0,
            addr1: ra,
            addr2: ta,
            addr3: bssid,
            seq_ctrl: 0,
        }
    }

    pub fn sequence_number(&self) -> u16 {
        (self.seq_ctrl >> 4) & 0x0FFF
    }

    pub fn fragment_number(&self) -> u8 {
        (self.seq_ctrl & 0x0F) as u8
    }

    pub fn set_sequence(&mut self, seq: u16, frag: u8) {
        self.seq_ctrl = ((seq & 0x0FFF) << 4) | (frag as u16 & 0x0F);
    }

    pub fn header_len(&self) -> usize {
        let mut len = Self::MIN_SIZE;
        if self.frame_ctrl.to_ds() && self.frame_ctrl.from_ds() {
            len += 6;
        }
        if self.frame_ctrl.is_data() && self.frame_ctrl.subtype() & 0x0008 != 0 {
            len += 2;
        }
        len
    }

    pub fn to_bytes(&self) -> [u8; Self::MIN_SIZE] {
        let mut buf = [0u8; Self::MIN_SIZE];
        buf[0..2].copy_from_slice(&self.frame_ctrl.raw.to_le_bytes());
        buf[2..4].copy_from_slice(&self.duration.to_le_bytes());
        buf[4..10].copy_from_slice(&self.addr1);
        buf[10..16].copy_from_slice(&self.addr2);
        buf[16..22].copy_from_slice(&self.addr3);
        buf[22..24].copy_from_slice(&self.seq_ctrl.to_le_bytes());
        buf
    }
}

// ============================================================================
// 802.11 Management Frame Bodies
// ============================================================================

/// Authentication frame body (IEEE 802.11-2024 §9.4.1.1)
///
/// | Field              | Size   |
/// |---------------------|--------|
/// | Auth Algorithm     | 2      |
/// | Auth Transaction   | 2      |
/// | Status Code        | 2      |
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct AuthFrameBody {
    pub auth_algorithm: u16,
    pub auth_transaction_seq: u16,
    pub status_code: u16,
}

impl AuthFrameBody {
    pub const ALGO_OPEN_SYSTEM: u16 = 0;
    pub const ALGO_SHARED_KEY: u16 = 1;
    pub const ALGO_SAE: u16 = 3;
    pub const ALGO_FILS: u16 = 4;

    pub const STATUS_SUCCESS: u16 = 0;
    pub const STATUS_REFUSED: u16 = 12;
    pub const STATUS_UNSUPP_ALGO: u16 = 17;

    pub fn open_system_request() -> Self {
        Self {
            auth_algorithm: Self::ALGO_OPEN_SYSTEM,
            auth_transaction_seq: 1,
            status_code: 0,
        }
    }

    pub fn open_system_response() -> Self {
        Self {
            auth_algorithm: Self::ALGO_OPEN_SYSTEM,
            auth_transaction_seq: 2,
            status_code: Self::STATUS_SUCCESS,
        }
    }

    pub fn sae_request() -> Self {
        Self {
            auth_algorithm: Self::ALGO_SAE,
            auth_transaction_seq: 1,
            status_code: 0,
        }
    }
}

/// Association Request frame body (IEEE 802.11-2024 §9.4.1.3)
///
/// | Field              | Size   |
/// |---------------------|--------|
/// | Capability Info    | 2      |
/// | Listen Interval    | 2      |
/// | Variable: SSID, Supported Rates, RSN, HT/VHT/HE Capabilities |
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct AssocReqBody {
    pub capability_info: u16,
    pub listen_interval: u16,
}

impl AssocReqBody {
    pub const CAP_ESS: u16 = 0x0001;
    pub const CAP_IBSS: u16 = 0x0002;
    pub const CAP_PRIVACY: u16 = 0x0010;
    pub const CAP_SHORT_PREAMBLE: u16 = 0x0020;
    pub const CAP_SPECTRUM_MGMT: u16 = 0x0100;
    pub const CAP_QOS: u16 = 0x0200;
    pub const CAP_SHORT_SLOT: u16 = 0x0400;
    pub const CAP_DSSS_OFDM: u16 = 0x2000;

    pub fn new(listen_interval: u16, privacy: bool) -> Self {
        let mut cap = Self::CAP_ESS | Self::CAP_QOS | Self::CAP_SHORT_SLOT | Self::CAP_DSSS_OFDM;
        if privacy {
            cap |= Self::CAP_PRIVACY;
        }
        Self {
            capability_info: cap,
            listen_interval,
        }
    }
}

/// Association Response frame body (IEEE 802.11-2024 §9.4.1.4)
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct AssocRespBody {
    pub capability_info: u16,
    pub status_code: u16,
    pub aid: u16,
}

impl AssocRespBody {
    pub const STATUS_SUCCESS: u16 = 0;
    pub const STATUS_REFUSED: u16 = 12;
    pub const STATUS_UNSUPP_RATES: u16 = 18;

    pub fn success(capability_info: u16, aid: u16) -> Self {
        Self {
            capability_info,
            status_code: Self::STATUS_SUCCESS,
            aid,
        }
    }
}

/// Beacon frame body (IEEE 802.11-2024 §9.4.1.2)
///
/// | Field              | Size   |
/// |---------------------|--------|
/// | Timestamp          | 8      |
/// | Beacon Interval    | 2      |
/// | Capability Info    | 2      |
/// | Variable: SSID, Supported Rates, DS Param, RSN, HT/VHT/HE Info |
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct BeaconBody {
    pub timestamp: u64,
    pub beacon_interval: u16,
    pub capability_info: u16,
}

// ============================================================================
// 802.11 IE (Information Element) Parsing
// ============================================================================

/// IE Number (IEEE 802.11-2024 §9.4.2)
pub const IE_SSID: u8 = 0;
pub const IE_SUPPORTED_RATES: u8 = 1;
pub const IE_DS_PARAM: u8 = 3;
pub const IE_RSN: u8 = 48;
pub const IE_HT_CAP: u8 = 45;
pub const IE_HT_INFO: u8 = 61;
pub const IE_VHT_CAP: u8 = 191;
pub const IE_VHT_INFO: u8 = 192;
pub const IE_HE_CAP: u8 = 255;
pub const IE_EXT_HE_CAP: u8 = 255;
pub const IE_VENDOR_SPECIFIC: u8 = 221;

/// IE parser: (tag, length, data...)
pub fn parse_ies(data: &[u8]) -> Vec<(u8, &[u8])> {
    let mut ies = Vec::new();
    let mut pos = 0;
    while pos + 1 < data.len() {
        let tag = data[pos];
        let len = data[pos + 1] as usize;
        if pos + 2 + len > data.len() {
            break;
        }
        ies.push((tag, &data[pos + 2..pos + 2 + len]));
        pos += 2 + len;
    }
    ies
}

pub fn extract_ssid(ies: &[(u8, &[u8])]) -> String {
    for &(tag, data) in ies {
        if tag == IE_SSID && !data.is_empty() {
            return String::from_utf8_lossy(data).into_owned();
        }
    }
    String::new()
}

pub fn extract_rsn_info(ies: &[(u8, &[u8])]) -> Option<WifiSecurity> {
    for &(tag, data) in ies {
        if tag == IE_RSN && data.len() >= 2 {
            let version = u16::from_le_bytes([data[0], data[1]]);
            if version != 1 {
                continue;
            }
            return Some(WifiSecurity::WPA2Personal);
        }
    }
    None
}

pub fn extract_channel(ies: &[(u8, &[u8])]) -> Option<u8> {
    for &(tag, data) in ies {
        if tag == IE_DS_PARAM && data.len() >= 1 {
            return Some(data[0]);
        }
    }
    None
}

// ============================================================================
// Firmware Loading (Linux iwlwifi modeli)
// ============================================================================

/// Firmware header format (iwlwifi ucode v2 format)
///
/// | Field        | Size   |
/// |---------------|--------|
/// | Magic        | 4      | "IWLfw"
/// | Version      | 4      |
/// | Header Size  | 4      |
/// | Section Count| 4      |
/// | Entry Point  | 4      |
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FirmwareHeader {
    pub magic: [u8; 4],
    pub version: u32,
    pub header_size: u32,
    pub section_count: u32,
    pub entry_point: u32,
}

impl FirmwareHeader {
    pub const MAGIC: [u8; 4] = [b'I', b'W', b'L', b'f'];

    pub fn validate(&self) -> bool {
        self.magic == Self::MAGIC && self.header_size >= 20 && self.section_count > 0
    }
}

/// Firmware section type
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FirmwareSectionType {
    InstUcode,
    InitUcode,
    DataType,
    PhyData,
    Regulatory,
    Debug,
}

/// Firmware section descriptor
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FirmwareSection {
    pub section_type: u32,
    pub offset: u32,
    pub size: u32,
}

/// Firmware load state machine
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FirmwareState {
    NotLoaded,
    Loading,
    Loaded,
    Verifying,
    Verified,
    Running,
    Operational,
    Error,
}

/// Firmware metadata
#[derive(Clone, Debug)]
pub struct FirmwareInfo {
    pub version: u32,
    pub build_date: u32,
    pub api_version: u32,
    pub sections: Vec<FirmwareSection>,
}

// ============================================================================
// TX/RX Ring Programming
// ============================================================================

/// TX descriptor (hardware ring entry)
///
/// | Field        | Size   |
/// |---------------|--------|
/// | Buffer Addr  | 8      |
/// | Length       | 2      |
/// | Cmd Flags    | 2      |
/// | Status       | 2      |
/// | Reserved     | 2      |
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct TxDescriptor {
    pub buffer_addr: u64,
    pub length: u16,
    pub cmd_flags: u16,
    pub status: u16,
    pub reserved: u16,
}

impl TxDescriptor {
    pub const CMD_EOP: u16 = 0x0001;
    pub const CMD_IFCS: u16 = 0x0002;
    pub const CMD_RS: u16 = 0x0008;
    pub const CMD_IC: u16 = 0x0010;
    pub const CMD_EXT: u16 = 0x0020;
    pub const CMD_RPS: u16 = 0x0040;
    pub const CMD_DEXT: u16 = 0x0080;
    pub const CMD_VLE: u16 = 0x0100;

    pub const STAT_DD: u16 = 0x0001;
    pub const STAT_EC: u16 = 0x0002;
    pub const STAT_LC: u16 = 0x0004;
    pub const STAT_TU: u16 = 0x0008;

    pub fn is_complete(&self) -> bool {
        (self.status & Self::STAT_DD) != 0
    }

    pub fn is_error(&self) -> bool {
        (self.status & (Self::STAT_EC | Self::STAT_LC | Self::STAT_TU)) != 0
    }
}

/// RX descriptor (hardware ring entry)
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct RxDescriptor {
    pub buffer_addr: u64,
    pub length: u16,
    pub pkt_type: u16,
    pub status: u16,
    pub errors: u16,
    pub vlan_tag: u16,
}

impl RxDescriptor {
    pub const STAT_DD: u16 = 0x0001;
    pub const STAT_EOP: u16 = 0x0002;
    pub const STAT_IXSM: u16 = 0x0004;
    pub const STAT_VP: u16 = 0x0008;
    pub const STAT_UDPCS: u16 = 0x0010;
    pub const STAT_TCPCS: u16 = 0x0020;

    pub const ERR_CE: u16 = 0x0001;
    pub const ERR_SE: u16 = 0x0002;
    pub const ERR_SEQ: u16 = 0x0004;
    pub const ERR_CXE: u16 = 0x0010;

    pub fn is_complete(&self) -> bool {
        (self.status & Self::STAT_DD) != 0
    }

    pub fn has_error(&self) -> bool {
        (self.errors & (Self::ERR_CE | Self::ERR_SE | Self::ERR_SEQ)) != 0
    }
}

/// TX ring state
pub struct TxRing {
    pub descriptors: Vec<TxDescriptor>,
    pub buffers: Vec<Vec<u8>>,
    pub head: AtomicU32,
    pub tail: AtomicU32,
    pub size: usize,
}

impl TxRing {
    pub fn new(size: usize) -> Self {
        assert!(size.is_power_of_two(), "TX ring size must be power of 2");
        let mut descriptors = Vec::with_capacity(size);
        let mut buffers = Vec::with_capacity(size);
        for _ in 0..size {
            descriptors.push(TxDescriptor::default());
            buffers.push(Vec::with_capacity(2304));
        }
        Self {
            descriptors,
            buffers,
            head: AtomicU32::new(0),
            tail: AtomicU32::new(0),
            size,
        }
    }

    pub fn mask(&self) -> u32 {
        (self.size - 1) as u32
    }

    pub fn is_full(&self) -> bool {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Relaxed);
        tail.wrapping_sub(head) >= self.size as u32
    }

    pub fn is_empty(&self) -> bool {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        head == tail
    }

    pub fn submit(&mut self, data: &[u8], eop: bool) -> Result<u32, ()> {
        if self.is_full() {
            return Err(());
        }
        let tail = self.tail.load(Ordering::Relaxed);
        let idx = (tail & self.mask()) as usize;

        self.buffers[idx].clear();
        self.buffers[idx].extend_from_slice(data);

        let mut flags = TxDescriptor::CMD_RS | TxDescriptor::CMD_IFCS;
        if eop {
            flags |= TxDescriptor::CMD_EOP;
        }

        self.descriptors[idx].buffer_addr = idx as u64;
        self.descriptors[idx].length = data.len() as u16;
        self.descriptors[idx].cmd_flags = flags;
        self.descriptors[idx].status = 0;

        crate::memory_barriers::smp_wmb();
        self.tail.store(tail.wrapping_add(1), Ordering::Release);

        Ok(tail)
    }

    pub fn poll_completion(&self) -> Option<u32> {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        if head == tail {
            return None;
        }

        crate::memory_barriers::smp_rmb();
        let idx = (head & self.mask()) as usize;
        let desc = &self.descriptors[idx];

        if desc.is_complete() {
            self.head.store(head.wrapping_add(1), Ordering::Release);
            Some(head)
        } else {
            None
        }
    }

    pub fn reclaim_completed(&mut self) -> usize {
        let mut count = 0;
        while let Some(_) = self.poll_completion() {
            count += 1;
        }
        count
    }
}

/// RX ring state
pub struct RxRing {
    pub descriptors: Vec<RxDescriptor>,
    pub buffers: Vec<Vec<u8>>,
    pub head: AtomicU32,
    pub tail: AtomicU32,
    pub size: usize,
}

impl RxRing {
    pub fn new(size: usize) -> Self {
        assert!(size.is_power_of_two(), "RX ring size must be power of 2");
        let mut descriptors = Vec::with_capacity(size);
        let mut buffers = Vec::with_capacity(size);
        for _ in 0..size {
            descriptors.push(RxDescriptor::default());
            buffers.push(vec![0u8; 2304]);
        }
        Self {
            descriptors,
            buffers,
            head: AtomicU32::new(0),
            tail: AtomicU32::new(0),
            size,
        }
    }

    pub fn mask(&self) -> u32 {
        (self.size - 1) as u32
    }

    pub fn is_full(&self) -> bool {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Relaxed);
        tail.wrapping_sub(head) >= self.size as u32
    }

    pub fn refill(&mut self) -> usize {
        let mut count = 0;
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        let available = (self.size as u32).wrapping_sub(tail.wrapping_sub(head)) as usize;

        for i in 0..available {
            let idx = ((tail.wrapping_add(i as u32)) & self.mask()) as usize;
            self.buffers[idx].resize(2304, 0);
            self.descriptors[idx].buffer_addr = idx as u64;
            self.descriptors[idx].length = 2304;
            self.descriptors[idx].status = 0;
            self.descriptors[idx].errors = 0;
            count += 1;
        }

        if count > 0 {
            crate::memory_barriers::smp_wmb();
            self.tail
                .store(tail.wrapping_add(count as u32), Ordering::Release);
        }
        count
    }

    pub fn poll_packet(&mut self) -> Option<Vec<u8>> {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        if head == tail {
            return None;
        }

        crate::memory_barriers::smp_rmb();
        let idx = (head & self.mask()) as usize;
        let desc = &self.descriptors[idx];

        if desc.is_complete() {
            if desc.has_error() {
                self.head.store(head.wrapping_add(1), Ordering::Release);
                return Some(Vec::new());
            }

            let len = desc.length as usize;
            let packet = self.buffers[idx][..len].to_vec();
            self.head.store(head.wrapping_add(1), Ordering::Release);
            Some(packet)
        } else {
            None
        }
    }
}

// ============================================================================
// Scan State Machine (IEEE 802.11-2024 §9.6)
// ============================================================================

/// Tarama sonucu (BSS)
#[derive(Clone, Debug)]
pub struct WifiBss {
    pub bssid: [u8; 6],
    pub ssid: String,
    pub rssi: i8,
    pub channel: u8,
    pub frequency: u16,
    pub band: WifiBand,
    pub security: WifiSecurity,
    pub phy_mode: WifiPhyMode,
    pub channel_width: u16,
}

impl WifiBss {
    pub fn bssid_str(&self) -> String {
        format!(
            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            self.bssid[0],
            self.bssid[1],
            self.bssid[2],
            self.bssid[3],
            self.bssid[4],
            self.bssid[5]
        )
    }
}

/// Kanal listesi
pub const CHANNELS_2GHZ: [u8; 14] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14];
pub const CHANNELS_5GHZ: [u8; 24] = [
    36, 40, 44, 48, 52, 56, 60, 64, 100, 104, 108, 112, 116, 120, 124, 128, 132, 136, 140, 144,
    149, 153, 157, 161,
];
pub const CHANNELS_6GHZ: [u8; 10] = [1, 5, 9, 13, 17, 21, 25, 29, 33, 37];

pub fn channel_to_frequency(channel: u8, band: WifiBand) -> u16 {
    match band {
        WifiBand::Band2G => {
            if channel <= 14 {
                2407 + channel as u16 * 5
            } else {
                2407 + 14 * 5
            }
        }
        WifiBand::Band5G => 5000 + channel as u16 * 5,
        WifiBand::Band6G => 5950 + channel as u16 * 5,
    }
}

/// Scan state machine
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanState {
    Idle,
    SwitchingChannel,
    DwellTimer,
    ProcessingBeacons,
    SendingProbeRequest,
    CollectingProbeResponses,
    Complete,
    Aborted,
}

/// Scan configuration
#[derive(Clone, Debug)]
pub struct ScanConfig {
    pub passive: bool,
    pub dwell_time_ms: u32,
    pub probe_ssid: Option<String>,
    pub bands: Vec<WifiBand>,
    pub max_channels: usize,
}

impl ScanConfig {
    pub fn default_active() -> Self {
        Self {
            passive: false,
            dwell_time_ms: 20,
            probe_ssid: None,
            bands: vec![WifiBand::Band2G, WifiBand::Band5G],
            max_channels: 38,
        }
    }

    pub fn default_passive() -> Self {
        Self {
            passive: true,
            dwell_time_ms: 110,
            probe_ssid: None,
            bands: vec![WifiBand::Band2G, WifiBand::Band5G],
            max_channels: 38,
        }
    }

    pub fn channels_for_band(&self, band: WifiBand) -> &[u8] {
        match band {
            WifiBand::Band2G => &CHANNELS_2GHZ,
            WifiBand::Band5G => &CHANNELS_5GHZ,
            WifiBand::Band6G => &CHANNELS_6GHZ,
        }
    }
}

/// Probe Request frame builder (IEEE 802.11-2024 §9.4.1.2)
pub fn build_probe_request(ssid: Option<&str>, supported_rates: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(128);

    let broadcast: [u8; 6] = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
    let zero_addr: [u8; 6] = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00];

    let fc = FrameControl::new(
        FrameControl::FRAME_TYPE_MGMT,
        FrameControl::SUBTYPE_PROBE_REQ,
    );
    let header = MacHeader::new(fc, broadcast, zero_addr, broadcast);
    frame.extend_from_slice(&header.to_bytes());

    let ssid_bytes = ssid.unwrap_or("");
    frame.push(IE_SSID);
    frame.push(ssid_bytes.len() as u8);
    frame.extend_from_slice(ssid_bytes.as_bytes());

    frame.push(IE_SUPPORTED_RATES);
    frame.push(supported_rates.len() as u8);
    frame.extend_from_slice(supported_rates);

    frame
}

/// Beacon/Probe Response parser
pub fn parse_beacon_or_probe_resp(
    data: &[u8],
) -> Option<(MacHeader, String, u8, WifiSecurity, WifiPhyMode)> {
    if data.len() < MacHeader::MIN_SIZE + 12 {
        return None;
    }

    let header = MacHeader {
        frame_ctrl: FrameControl {
            raw: u16::from_le_bytes([data[0], data[1]]),
        },
        duration: u16::from_le_bytes([data[2], data[3]]),
        addr1: [data[4], data[5], data[6], data[7], data[8], data[9]],
        addr2: [data[10], data[11], data[12], data[13], data[14], data[15]],
        addr3: [data[16], data[17], data[18], data[19], data[20], data[21]],
        seq_ctrl: u16::from_le_bytes([data[22], data[23]]),
    };

    if !header.frame_ctrl.is_beacon() && !header.frame_ctrl.is_probe_resp() {
        return None;
    }

    let body_offset = MacHeader::MIN_SIZE;
    let ies_start = body_offset + 12;

    if ies_start >= data.len() {
        return None;
    }

    let ies = parse_ies(&data[ies_start..]);
    let ssid = extract_ssid(&ies);
    let channel = extract_channel(&ies).unwrap_or(1);
    let security = extract_rsn_info(&ies).unwrap_or(WifiSecurity::Open);

    let phy_mode = determine_phy_mode(&ies);

    Some((header, ssid, channel, security, phy_mode))
}

fn determine_phy_mode(ies: &[(u8, &[u8])]) -> WifiPhyMode {
    let mut has_he = false;
    let mut has_vht = false;
    let mut has_ht = false;

    for &(tag, _) in ies {
        match tag {
            IE_HE_CAP => has_he = true,
            IE_VHT_CAP => has_vht = true,
            IE_HT_CAP => has_ht = true,
            _ => {}
        }
    }

    if has_he {
        WifiPhyMode::Dot11AX
    } else if has_vht {
        WifiPhyMode::Dot11AC
    } else if has_ht {
        WifiPhyMode::Dot11N
    } else {
        WifiPhyMode::Dot11G
    }
}

// ============================================================================
// Association State Machine (IEEE 802.11-2024 §9.3, §9.4)
// ============================================================================

/// Association state machine
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssocState {
    Idle,
    Authenticating,
    Authenticated,
    Associating,
    Associated,
    Keying,
    Complete,
    Failed,
}

/// Authentication state
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthState {
    Idle,
    RequestSent,
    ResponseReceived,
    Complete,
    Failed,
}

/// 4-Way Handshake state (WPA2/WPA3)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FourWayState {
    Idle,
    Msg1Received,
    Msg2Sent,
    Msg3Received,
    Msg4Sent,
    Complete,
    Failed,
}

/// EAPOL-Key frame body defined by IEEE 802.11i / 802.1X.
/// EAPOL header (version=1, type=3, length) ayrıdır — bu struct Key body'dir.
///
/// Layout:
/// | Field          | Size   |
/// |-----------------|--------|
/// | descriptor_type| 1      | (RSN=2, WPA=254)
/// | key_info       | 2      |
// ============================================================================
// EAPOL / EAPOL-Key (IEEE 802.1X-2004 + IEEE 802.11i)
// ============================================================================

/// EAPOL header (IEEE 802.1X-2004 §11.3.1)
/// Precedes EAPOL-Key body in all 4-way handshake frames.
/// EtherType 0x888E identifies EAPOL in the MAC frame.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct EapolHeader {
    pub version: u8,
    pub packet_type: u8,
    pub length: u16, // big-endian on wire
}

impl EapolHeader {
    pub const VERSION_802_1X_2004: u8 = 2;
    pub const VERSION_802_1X_2001: u8 = 1;
    pub const TYPE_EAPOL_KEY: u8 = 3;

    pub fn new(version: u8, packet_type: u8, length: u16) -> Self {
        Self {
            version,
            packet_type,
            length: length.to_be(),
        }
    }

    pub fn to_bytes(&self) -> [u8; 4] {
        [
            self.version,
            self.packet_type,
            (self.length >> 8) as u8,
            (self.length & 0xFF) as u8,
        ]
    }
}

/// EAPOL-Key frame body (IEEE 802.11i / WPA2 RSN Key Descriptor)
///
/// Per IEEE 802.11i §8.5.2:
/// All multi-byte fields are BIG-ENDIAN on the wire.
///
/// Layout (95 bytes + variable key data):
/// | Field          | Size | Endian |
/// |----------------|------|--------|
/// | descriptor_type| 1    | -      |
/// | key_info       | 2    | BE     |
/// | key_len        | 2    | BE     |
/// | replay_counter | 8    | BE     |
/// | nonce          | 32   | -      |
/// | key_iv         | 16   | -      |
/// | rsc            | 8    | -      |
/// | id             | 8    | -      |
/// | mic            | 16   | -      |
/// | data_len       | 2    | BE     |
/// | data           | var  | -      |
///
/// NOTE: This struct covers the fixed 95-byte portion. Key data is
/// appended separately during serialization.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct EapolKeyFrame {
    pub descriptor_type: u8,
    pub key_info: u16,
    pub key_len: u16,
    pub replay_counter: u64,
    pub nonce: [u8; 32],
    pub key_iv: [u8; 16],
    pub rsc: [u8; 8],
    pub id: [u8; 8],
    pub mic: [u8; 16],
    pub data_len: u16,
}

impl EapolKeyFrame {
    pub const FIXED_SIZE: usize = 95;
    pub const DESCRIPTOR_RSN: u8 = 2;

    // Key info bits (IEEE 802.11i §8.5.3, big-endian on wire)
    pub const KEY_INFO_VERSION_MASK: u16 = 0x0007;
    pub const KEY_INFO_VERSION_WPA: u16 = 1;
    pub const KEY_INFO_VERSION_WPA2: u16 = 2;
    pub const KEY_INFO_KEY_TYPE: u16 = 0x0008;
    pub const KEY_INFO_KEY_INDEX_MASK: u16 = 0x0030;
    pub const KEY_INFO_INSTALL: u16 = 0x0040;
    pub const KEY_INFO_ACK: u16 = 0x0080;
    pub const KEY_INFO_MIC: u16 = 0x0100;
    pub const KEY_INFO_SECURE: u16 = 0x0200;
    pub const KEY_INFO_ERROR: u16 = 0x0400;
    pub const KEY_INFO_REQUEST: u16 = 0x0800;
    pub const KEY_INFO_ENCRYPTED_DATA: u16 = 0x1000;

    /// Serialize the fixed 95-byte portion with big-endian wire format.
    /// Per IEEE 802.11i: key_info, key_len, replay_counter, data_len are big-endian.
    pub fn to_bytes(&self) -> [u8; Self::FIXED_SIZE] {
        let mut buf = [0u8; Self::FIXED_SIZE];
        buf[0] = self.descriptor_type;
        buf[1..3].copy_from_slice(&self.key_info.to_be_bytes());
        buf[3..5].copy_from_slice(&self.key_len.to_be_bytes());
        buf[5..13].copy_from_slice(&self.replay_counter.to_be_bytes());
        buf[13..45].copy_from_slice(&self.nonce);
        buf[45..61].copy_from_slice(&self.key_iv);
        buf[61..69].copy_from_slice(&self.rsc);
        buf[69..77].copy_from_slice(&self.id);
        buf[77..93].copy_from_slice(&self.mic);
        buf[93..95].copy_from_slice(&self.data_len.to_be_bytes());
        buf
    }

    /// Parse from raw bytes (big-endian wire format).
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < Self::FIXED_SIZE {
            return None;
        }
        Some(Self {
            descriptor_type: data[0],
            key_info: u16::from_be_bytes([data[1], data[2]]),
            key_len: u16::from_be_bytes([data[3], data[4]]),
            replay_counter: u64::from_be_bytes([
                data[5], data[6], data[7], data[8], data[9], data[10], data[11], data[12],
            ]),
            nonce: data[13..45].try_into().ok()?,
            key_iv: data[45..61].try_into().ok()?,
            rsc: data[61..69].try_into().ok()?,
            id: data[69..77].try_into().ok()?,
            mic: data[77..93].try_into().ok()?,
            data_len: u16::from_be_bytes([data[93], data[94]]),
        })
    }

    pub fn is_msg1(&self) -> bool {
        (self.key_info & Self::KEY_INFO_ACK) != 0
            && (self.key_info & Self::KEY_INFO_MIC) == 0
            && (self.key_info & Self::KEY_INFO_SECURE) == 0
    }

    pub fn is_msg2(&self) -> bool {
        (self.key_info & Self::KEY_INFO_ACK) == 0
            && (self.key_info & Self::KEY_INFO_MIC) != 0
            && (self.key_info & Self::KEY_INFO_SECURE) == 0
    }

    pub fn is_msg3(&self) -> bool {
        // Per IEEE 802.11i §8.5.3.3: Message 3 has ACK=1, MIC=1, Secure=1, Install=1
        (self.key_info & Self::KEY_INFO_ACK) != 0
            && (self.key_info & Self::KEY_INFO_MIC) != 0
            && (self.key_info & Self::KEY_INFO_SECURE) != 0
    }

    pub fn is_msg4(&self) -> bool {
        (self.key_info & Self::KEY_INFO_ACK) == 0
            && (self.key_info & Self::KEY_INFO_MIC) != 0
            && (self.key_info & Self::KEY_INFO_SECURE) != 0
    }

    pub fn is_install(&self) -> bool {
        (self.key_info & Self::KEY_INFO_INSTALL) != 0
    }
}

/// Association context
#[derive(Clone, Debug)]
pub struct AssocContext {
    pub state: AssocState,
    pub auth_state: AuthState,
    pub four_way_state: FourWayState,
    pub bssid: [u8; 6],
    pub ssid: String,
    pub security: WifiSecurity,
    pub aid: u16,
    pub replay_counter: u64,
    pub anonce: [u8; 32],
    pub snonce: [u8; 32],
    pub pmk: [u8; 32],
    pub ptk: Option<Ptk>,
    pub ptk_installed: bool,
    pub gtk_installed: bool,
}

impl AssocContext {
    pub fn new() -> Self {
        Self {
            state: AssocState::Idle,
            auth_state: AuthState::Idle,
            four_way_state: FourWayState::Idle,
            bssid: [0; 6],
            ssid: String::new(),
            security: WifiSecurity::Open,
            aid: 0,
            replay_counter: 0,
            anonce: [0; 32],
            snonce: [0; 32],
            pmk: [0; 32],
            ptk: None,
            ptk_installed: false,
            gtk_installed: false,
        }
    }

    pub fn reset(&mut self) {
        self.state = AssocState::Idle;
        self.auth_state = AuthState::Idle;
        self.four_way_state = FourWayState::Idle;
        self.aid = 0;
        self.replay_counter = 0;
        self.anonce = [0; 32];
        self.snonce = [0; 32];
        self.pmk = [0; 32];
        self.ptk = None;
        self.ptk_installed = false;
        self.gtk_installed = false;
    }
}

// ============================================================================
// WPA2 4-Way Handshake Crypto (IEEE 802.11i)
// ============================================================================

/// HMAC-SHA1 per RFC 2104.
/// Used for MIC computation in WPA2 4-way handshake (Key Descriptor Version 2).
fn hmac_sha1(key: &[u8], msg: &[u8]) -> [u8; 20] {
    let block_size = 64usize;
    let mut ipad = [0x36u8; 64];
    let mut opad = [0x5Cu8; 64];

    if key.len() > block_size {
        let mut hasher = Sha1::new();
        hasher.update(key);
        let hash = hasher.finalize();
        for i in 0..20 {
            ipad[i] ^= hash[i];
            opad[i] ^= hash[i];
        }
    } else {
        for i in 0..key.len() {
            ipad[i] ^= key[i];
            opad[i] ^= key[i];
        }
    }

    let mut inner_hasher = Sha1::new();
    inner_hasher.update(&ipad);
    inner_hasher.update(msg);
    let inner_hash = inner_hasher.finalize();

    let mut outer_hasher = Sha1::new();
    outer_hasher.update(&opad);
    outer_hasher.update(&inner_hash);
    let outer_hash = outer_hasher.finalize();

    let mut result = [0u8; 20];
    result.copy_from_slice(&outer_hash);
    result
}

/// PRF-HMAC-SHA1 per IEEE 802.11i §8.5.1.1.
/// PTK = PRF-X(PMK, "Pairwise key expansion", Min(AA,SPA) || Max(AA,SPA) || Min(ANonce,SNonce) || Max(ANonce,SNonce))
/// For CCMP (X=512): PTK = KCK(16) + KEK(16) + TK(16) + TMIC1(8) + TMIC2(8)
fn prf_sha1(key: &[u8], label: &str, data: &[u8], output_bits: usize) -> Vec<u8> {
    let output_bytes = (output_bits + 7) / 8;
    let mut result = Vec::with_capacity(output_bytes);

    let num_iterations = (output_bytes + 19) / 20; // ceil(output_bytes / 20)
    for i in 0..num_iterations {
        let mut msg = Vec::with_capacity(label.len() + 1 + data.len() + 1);
        msg.extend_from_slice(label.as_bytes());
        msg.push(0);
        msg.extend_from_slice(data);
        msg.push(i as u8);

        let hash = hmac_sha1(key, &msg);
        for &b in &hash {
            result.push(b);
            if result.len() >= output_bytes {
                break;
            }
        }
        if result.len() >= output_bytes {
            break;
        }
    }

    result.truncate(output_bytes);
    result
}

/// PTK (Pairwise Transient Key) derived per IEEE 802.11i §8.5.1.
#[derive(Clone, Debug)]
pub struct Ptk {
    pub kck: [u8; 16], // Key Confirmation Key (MIC)
    pub kek: [u8; 16], // Key Encryption Key (GTK encryption)
    pub tk: [u8; 16],  // Temporal Key (CCMP data encryption)
}

impl Ptk {
    pub fn derive(
        pmk: &[u8; 32],
        anonce: &[u8; 32],
        snonce: &[u8; 32],
        authenticator_addr: &[u8; 6],
        supplicant_addr: &[u8; 6],
    ) -> Self {
        // Min(AA, SPA) || Max(AA, SPA) || Min(ANonce, SNonce) || Max(ANonce, SNonce)
        let mut data = Vec::with_capacity(76);
        if authenticator_addr < supplicant_addr {
            data.extend_from_slice(authenticator_addr);
            data.extend_from_slice(supplicant_addr);
        } else {
            data.extend_from_slice(supplicant_addr);
            data.extend_from_slice(authenticator_addr);
        }
        if anonce < snonce {
            data.extend_from_slice(anonce);
            data.extend_from_slice(snonce);
        } else {
            data.extend_from_slice(snonce);
            data.extend_from_slice(anonce);
        }

        let ptk_bytes = prf_sha1(pmk, "Pairwise key expansion", &data, 512);

        let mut kck = [0u8; 16];
        let mut kek = [0u8; 16];
        let mut tk = [0u8; 16];
        kck.copy_from_slice(&ptk_bytes[0..16]);
        kek.copy_from_slice(&ptk_bytes[16..32]);
        tk.copy_from_slice(&ptk_bytes[32..48]);

        Self { kck, kek, tk }
    }
}

/// Compute MIC over EAPOL frame using HMAC-SHA1-128 (Key Descriptor Version 2).
/// Per IEEE 802.11i: MIC = HMAC-SHA1-128(KCK, EAPOL_frame) where MIC field is zeroed.
/// Returns first 16 bytes of HMAC-SHA1 output.
fn compute_mic(kck: &[u8; 16], eapol_frame: &[u8]) -> [u8; 16] {
    let hash = hmac_sha1(kck, eapol_frame);
    let mut mic = [0u8; 16];
    mic.copy_from_slice(&hash[0..16]);
    mic
}

/// Generate cryptographically random nonce using RDRAND if available,
/// falling back to a simple LCG seeded from TSC.
fn generate_nonce() -> [u8; 32] {
    let mut nonce = [0u8; 32];
    // Try RDRAND first
    #[cfg(target_arch = "x86_64")]
    {
        let mut filled = 0;
        while filled < 32 {
            let mut val: u64 = 0;
            let ok: u8;
            unsafe {
                core::arch::asm!(
                    "rdrand {}",
                    "setc {}",
                    out(reg) val,
                    lateout(reg_byte) ok,
                    options(nomem, nostack),
                );
            }
            if ok != 0 {
                let bytes = val.to_le_bytes();
                let remaining = 32 - filled;
                let copy_len = remaining.min(8);
                nonce[filled..filled + copy_len].copy_from_slice(&bytes[..copy_len]);
                filled += copy_len;
            } else {
                // RDRAND failed, fall back to TSC-based generation
                break;
            }
        }
        if filled >= 32 {
            return nonce;
        }
    }
    // Fallback: TSC-seeded LCG
    let mut state: u64 = unsafe {
        let lo: u32;
        let hi: u32;
        core::arch::asm!("rdtsc", out("eax") lo, out("edx") hi);
        ((hi as u64) << 32) | (lo as u64)
    };
    for byte in &mut nonce {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        *byte = (state >> 33) as u8;
    }
    nonce
}

// ============================================================================
// MLO (Multi-Link Operation, 802.11be)
// ============================================================================

#[derive(Clone, Debug)]
pub struct WifiMloLink {
    pub bssid: [u8; 6],
    pub band: WifiBand,
    pub channel: u8,
    pub frequency: u16,
    pub phy_mode: WifiPhyMode,
    pub channel_width: u16,
    pub rssi: i8,
    pub score: u32,
    pub estimated_mbps: u32,
}

#[derive(Clone, Debug)]
pub struct WifiMloSession {
    pub ssid: String,
    pub security: WifiSecurity,
    pub primary: WifiMloLink,
    pub secondary: Vec<WifiMloLink>,
    pub aggregate_mbps: u32,
    pub average_rssi: i8,
}

impl WifiMloSession {
    pub fn link_count(&self) -> usize {
        1 + self.secondary.len()
    }
}

// ============================================================================
// WiFi Jail Komutu (sandbox → kernel)
// ============================================================================

#[derive(Clone, Debug)]
pub enum WifiJailCommand {
    FirmwareLoad {
        version: u32,
    },
    Scan {
        passive: bool,
    },
    Connect {
        ssid: String,
        password: String,
        security: WifiSecurity,
    },
    Disconnect,
    SetPowerSave(bool),
    GetMacAddress,
    GetStats,
    GetFirmwareVersion,
    SetChannel(u8),
    SetTxPower(i8),
    TxFrame {
        data: Vec<u8>,
    },
}

#[derive(Clone, Debug)]
pub enum WifiJailResponse {
    Ok,
    Error(WifiError),
    ScanResults(Vec<WifiBss>),
    MacAddress([u8; 6]),
    Stats(WifiStats),
    FirmwareVersion(String),
    AssocComplete { aid: u16 },
    FrameTxComplete { seq: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WifiError {
    NotInitialized,
    DeviceNotFound,
    AlreadyConnected,
    NotConnected,
    AuthenticationFailed,
    AssociationFailed,
    KeyExchangeFailed,
    Timeout,
    FirmwareError,
    FirmwareLoadFailed,
    InvalidParameter,
    NoMemory,
    TxRingFull,
    RxError,
    ChannelSwitchFailed,
    JailChannelClosed,
}

// ============================================================================
// WiFi Istatistikleri
// ============================================================================

#[derive(Clone, Debug, Default)]
pub struct WifiStats {
    pub tx_packets: u64,
    pub rx_packets: u64,
    pub tx_bytes: u64,
    pub rx_bytes: u64,
    pub tx_errors: u64,
    pub rx_errors: u64,
    pub tx_retries: u64,
    pub rx_dropped: u64,
    pub signal_dbm: i8,
    pub noise_dbm: i8,
    pub connected_time: u64,
    pub firmware_load_time_ms: u32,
    pub scan_duration_ms: u32,
    pub assoc_duration_ms: u32,
    pub four_way_duration_ms: u32,
    pub key_exchange_failures: u64,
    pub channel_switches: u64,
    pub beacon_count: u64,
    pub probe_resp_count: u64,
}

// ============================================================================
// WiFi Jail Controller
// ============================================================================

pub struct WifiJailController {
    pub initialized: AtomicBool,
    pub state: Mutex<WifiState>,
    pub connected_ssid: Mutex<Option<String>>,
    pub connected_bssid: Mutex<Option<[u8; 6]>>,
    pub mac_address: Mutex<[u8; 6]>,
    pub scan_results: Mutex<Vec<WifiBss>>,
    pub mlo_session: Mutex<Option<WifiMloSession>>,
    pub stats: Mutex<WifiStats>,
    pub jail_token: AtomicU32,

    pub jail_id: u32,
    pub jail_channel: Option<JailChannel>,

    pub crash_count: AtomicU32,
    pub reboot_seq: AtomicU64,
    pub last_healthy_seq: AtomicU64,
    pub watchdog_timeout_ms: AtomicU32,
    pub last_heartbeat: AtomicU64,
    pub restart_backoff_ms: AtomicU64,
    pub max_backoff_ms: AtomicU64,
    pub consecutive_crashes: AtomicU32,

    pub firmware_state: Mutex<FirmwareState>,
    pub firmware_info: Mutex<Option<FirmwareInfo>>,

    pub tx_ring: Mutex<TxRing>,
    pub rx_ring: Mutex<RxRing>,
    pub seq_number: AtomicU32,

    pub scan_state: Mutex<ScanState>,
    pub scan_config: Mutex<Option<ScanConfig>>,
    pub current_channel: AtomicU32,
    pub current_band: Mutex<WifiBand>,

    pub assoc_ctx: Mutex<AssocContext>,
}

impl WifiJailController {
    pub const fn new() -> Self {
        Self {
            initialized: AtomicBool::new(false),
            state: Mutex::new(WifiState::Disconnected),
            connected_ssid: Mutex::new(None),
            connected_bssid: Mutex::new(None),
            mac_address: Mutex::new([0u8; 6]),
            scan_results: Mutex::new(Vec::new()),
            mlo_session: Mutex::new(None),
            stats: Mutex::new(WifiStats {
                tx_packets: 0,
                rx_packets: 0,
                tx_bytes: 0,
                rx_bytes: 0,
                tx_errors: 0,
                rx_errors: 0,
                tx_retries: 0,
                rx_dropped: 0,
                signal_dbm: 0,
                noise_dbm: 0,
                connected_time: 0,
                firmware_load_time_ms: 0,
                scan_duration_ms: 0,
                assoc_duration_ms: 0,
                four_way_duration_ms: 0,
                key_exchange_failures: 0,
                channel_switches: 0,
                beacon_count: 0,
                probe_resp_count: 0,
            }),
            jail_token: AtomicU32::new(0),
            jail_id: 0,
            jail_channel: None,
            crash_count: AtomicU32::new(0),
            reboot_seq: AtomicU64::new(0),
            last_healthy_seq: AtomicU64::new(0),
            watchdog_timeout_ms: AtomicU32::new(5000),
            last_heartbeat: AtomicU64::new(0),
            restart_backoff_ms: AtomicU64::new(100),
            max_backoff_ms: AtomicU64::new(30000),
            consecutive_crashes: AtomicU32::new(0),
            firmware_state: Mutex::new(FirmwareState::NotLoaded),
            firmware_info: Mutex::new(None),
            tx_ring: Mutex::new(TxRing {
                descriptors: Vec::new(),
                buffers: Vec::new(),
                head: AtomicU32::new(0),
                tail: AtomicU32::new(0),
                size: 256,
            }),
            rx_ring: Mutex::new(RxRing {
                descriptors: Vec::new(),
                buffers: Vec::new(),
                head: AtomicU32::new(0),
                tail: AtomicU32::new(0),
                size: 256,
            }),
            seq_number: AtomicU32::new(0),
            scan_state: Mutex::new(ScanState::Idle),
            scan_config: Mutex::new(None),
            current_channel: AtomicU32::new(0),
            current_band: Mutex::new(WifiBand::Band2G),
            assoc_ctx: Mutex::new(AssocContext {
                state: AssocState::Idle,
                auth_state: AuthState::Idle,
                four_way_state: FourWayState::Idle,
                bssid: [0; 6],
                ssid: String::new(),
                security: WifiSecurity::Open,
                aid: 0,
                replay_counter: 0,
                anonce: [0; 32],
                snonce: [0; 32],
                pmk: [0; 32],
                ptk: None,
                ptk_installed: false,
                gtk_installed: false,
            }),
        }
    }

    // ========================================================================
    // Firmware Loading (Linux iwlwifi modeli)
    // ========================================================================

    pub fn load_firmware(&self, fw_data: &[u8]) -> Result<FirmwareInfo, WifiError> {
        let mut fw_state = self.firmware_state.lock();
        *fw_state = FirmwareState::Loading;

        if fw_data.len() < core::mem::size_of::<FirmwareHeader>() {
            *fw_state = FirmwareState::Error;
            return Err(WifiError::FirmwareError);
        }

        let header = unsafe { &*(fw_data.as_ptr() as *const FirmwareHeader) };
        if !header.validate() {
            *fw_state = FirmwareState::Error;
            return Err(WifiError::FirmwareError);
        }

        let sections_start = header.header_size as usize;
        let mut sections = Vec::new();
        let mut pos = sections_start;

        for _ in 0..header.section_count {
            if pos + core::mem::size_of::<FirmwareSection>() > fw_data.len() {
                *fw_state = FirmwareState::Error;
                return Err(WifiError::FirmwareError);
            }
            let sec = unsafe { &*(fw_data[pos..].as_ptr() as *const FirmwareSection) };
            if sec.offset as usize + sec.size as usize > fw_data.len() {
                *fw_state = FirmwareState::Error;
                return Err(WifiError::FirmwareError);
            }
            sections.push(*sec);
            pos += core::mem::size_of::<FirmwareSection>();
        }

        *fw_state = FirmwareState::Verifying;

        for sec in &sections {
            let data = &fw_data[sec.offset as usize..(sec.offset + sec.size) as usize];
            let checksum: u32 = data.iter().map(|&b| b as u32).sum();
            if checksum == 0 {
                *fw_state = FirmwareState::Error;
                return Err(WifiError::FirmwareError);
            }
        }

        *fw_state = FirmwareState::Verified;

        let info = FirmwareInfo {
            version: header.version,
            build_date: 0,
            api_version: header.version & 0xFF,
            sections,
        };

        *fw_state = FirmwareState::Running;

        *self.firmware_info.lock() = Some(info.clone());

        crate::serial_println!(
            "[WiFi-Jail] Firmware loaded: v{}, {} sections, entry=0x{:x}",
            header.version,
            header.section_count,
            header.entry_point
        );

        *fw_state = FirmwareState::Operational;
        Ok(info)
    }

    pub fn firmware_state(&self) -> FirmwareState {
        *self.firmware_state.lock()
    }

    // ========================================================================
    // TX/RX Ring Programming
    // ========================================================================

    pub fn init_rings(&self) {
        *self.tx_ring.lock() = TxRing::new(256);
        *self.rx_ring.lock() = RxRing::new(256);

        let mut rx = self.rx_ring.lock();
        rx.refill();

        crate::serial_println!("[WiFi-Jail] TX/RX rings initialized (256 entries each)");
    }

    pub fn tx_frame(&self, data: &[u8]) -> Result<u32, WifiError> {
        let mut tx = self.tx_ring.lock();
        let seq = self.seq_number.fetch_add(1, Ordering::Relaxed);
        match tx.submit(data, true) {
            Ok(_) => {
                self.stats.lock().tx_packets += 1;
                self.stats.lock().tx_bytes += data.len() as u64;
                Ok(seq)
            }
            Err(_) => {
                self.stats.lock().tx_errors += 1;
                Err(WifiError::TxRingFull)
            }
        }
    }

    pub fn rx_poll(&self) -> Option<Vec<u8>> {
        let mut rx = self.rx_ring.lock();
        if let Some(packet) = rx.poll_packet() {
            if packet.is_empty() {
                self.stats.lock().rx_errors += 1;
            } else {
                self.stats.lock().rx_packets += 1;
                self.stats.lock().rx_bytes += packet.len() as u64;
            }
            Some(packet)
        } else {
            None
        }
    }

    pub fn tx_reclaim(&self) -> usize {
        let mut tx = self.tx_ring.lock();
        tx.reclaim_completed()
    }

    pub fn rx_refill(&self) -> usize {
        let mut rx = self.rx_ring.lock();
        rx.refill()
    }

    // ========================================================================
    // Scan State Machine (IEEE 802.11-2024 §9.6)
    // ========================================================================

    pub fn start_scan(&self, config: ScanConfig) -> Result<(), WifiError> {
        let mut scan_state = self.scan_state.lock();
        if *scan_state != ScanState::Idle {
            return Err(WifiError::AlreadyConnected);
        }

        *self.state.lock() = WifiState::Scanning;
        *scan_state = ScanState::SwitchingChannel;
        let passive = config.passive;
        *self.scan_config.lock() = Some(config);

        crate::serial_println!("[WiFi-Jail] Scan started (passive={})", passive);
        Ok(())
    }

    pub fn scan_step(&self) -> Result<bool, WifiError> {
        let mut scan_state = self.scan_state.lock();
        let config = self.scan_config.lock();
        let config = match config.as_ref() {
            Some(c) => c,
            None => return Err(WifiError::InvalidParameter),
        };

        match *scan_state {
            ScanState::Idle => Ok(true),
            ScanState::SwitchingChannel => {
                let ch = self.current_channel.fetch_add(1, Ordering::Relaxed);
                let mut total_channels = 0;
                for band in &config.bands {
                    total_channels += config.channels_for_band(*band).len();
                }
                if ch as usize >= total_channels {
                    *scan_state = ScanState::Complete;
                    return Ok(true);
                }

                let mut band = WifiBand::Band2G;
                let mut channel_idx = ch as usize;
                for b in &config.bands {
                    let channels = config.channels_for_band(*b);
                    if channel_idx < channels.len() {
                        band = *b;
                        let channel = channels[channel_idx];
                        self.current_channel
                            .store(channel as u32, Ordering::Release);
                        *self.current_band.lock() = band;
                        self.stats.lock().channel_switches += 1;
                        *scan_state = if config.passive {
                            ScanState::DwellTimer
                        } else {
                            ScanState::SendingProbeRequest
                        };
                        return Ok(false);
                    }
                    channel_idx -= channels.len();
                }
                *scan_state = ScanState::Complete;
                Ok(true)
            }
            ScanState::DwellTimer => {
                self.stats.lock().beacon_count += 1;
                *scan_state = ScanState::SwitchingChannel;
                Ok(false)
            }
            ScanState::SendingProbeRequest => {
                let probe_ssid = config.probe_ssid.as_deref();
                let rates: [u8; 8] = [0x82, 0x84, 0x8B, 0x96, 0x0C, 0x12, 0x18, 0x60];
                let frame = build_probe_request(probe_ssid, &rates);
                let _ = self.tx_frame(&frame);
                *scan_state = ScanState::CollectingProbeResponses;
                Ok(false)
            }
            ScanState::CollectingProbeResponses => {
                self.stats.lock().probe_resp_count += 1;
                *scan_state = ScanState::SwitchingChannel;
                Ok(false)
            }
            ScanState::ProcessingBeacons => {
                *scan_state = ScanState::SwitchingChannel;
                Ok(false)
            }
            ScanState::Complete => {
                *self.state.lock() = WifiState::Disconnected;
                Ok(true)
            }
            ScanState::Aborted => {
                *self.state.lock() = WifiState::Disconnected;
                Ok(true)
            }
        }
    }

    pub fn process_beacon(&self, data: &[u8], rssi: i8) -> Option<WifiBss> {
        if let Some((header, ssid, channel, security, phy_mode)) = parse_beacon_or_probe_resp(data)
        {
            let band = *self.current_band.lock();
            let frequency = channel_to_frequency(channel, band);
            let channel_width = match phy_mode {
                WifiPhyMode::Dot11BE => 320,
                WifiPhyMode::Dot11AX => 160,
                WifiPhyMode::Dot11AC => 80,
                WifiPhyMode::Dot11N => 40,
                _ => 20,
            };

            let bss = WifiBss {
                bssid: header.addr3,
                ssid,
                rssi,
                channel,
                frequency,
                band,
                security,
                phy_mode,
                channel_width,
            };

            let mut results = self.scan_results.lock();
            if !results.iter().any(|b| b.bssid == bss.bssid) {
                results.push(bss.clone());
            }

            Some(bss)
        } else {
            None
        }
    }

    pub fn abort_scan(&self) {
        *self.scan_state.lock() = ScanState::Aborted;
        *self.state.lock() = WifiState::Disconnected;
    }

    // ========================================================================
    // Association State Machine (IEEE 802.11-2024 §9.3, §9.4)
    // ========================================================================

    pub fn start_association(&self, bss: &WifiBss, password: &str) -> Result<(), WifiError> {
        let mut ctx = self.assoc_ctx.lock();
        ctx.reset();
        ctx.bssid = bss.bssid;
        ctx.ssid = bss.ssid.clone();
        ctx.security = bss.security;
        ctx.state = AssocState::Authenticating;

        *self.state.lock() = WifiState::Authenticating;

        crate::serial_println!(
            "[WiFi-Jail] Association started: {} security={}",
            bss.ssid,
            bss.security.as_str()
        );

        drop(ctx);
        self.send_auth_request(bss.bssid, bss.security)
    }

    fn send_auth_request(&self, bssid: [u8; 6], security: WifiSecurity) -> Result<(), WifiError> {
        let mut ctx = self.assoc_ctx.lock();
        ctx.auth_state = AuthState::RequestSent;

        let body = match security {
            WifiSecurity::WPA3Personal | WifiSecurity::WPA3Enterprise => {
                AuthFrameBody::sae_request()
            }
            _ => AuthFrameBody::open_system_request(),
        };

        let mut frame = Vec::with_capacity(64);
        let fc = FrameControl::new(FrameControl::FRAME_TYPE_MGMT, FrameControl::SUBTYPE_AUTH);
        let header = MacHeader::new(fc, bssid, *self.mac_address.lock(), bssid);
        frame.extend_from_slice(&header.to_bytes());
        frame.extend_from_slice(&unsafe {
            core::slice::from_raw_parts(
                &body as *const AuthFrameBody as *const u8,
                core::mem::size_of::<AuthFrameBody>(),
            )
        });

        let _ = self.tx_frame(&frame);
        Ok(())
    }

    pub fn process_auth_response(&self, data: &[u8]) -> Result<(), WifiError> {
        if data.len() < MacHeader::MIN_SIZE + core::mem::size_of::<AuthFrameBody>() {
            return Err(WifiError::AuthenticationFailed);
        }

        let body_offset = MacHeader::MIN_SIZE;
        let body = unsafe { &*(data[body_offset..].as_ptr() as *const AuthFrameBody) };

        if body.status_code != AuthFrameBody::STATUS_SUCCESS {
            let mut ctx = self.assoc_ctx.lock();
            ctx.auth_state = AuthState::Failed;
            ctx.state = AssocState::Failed;
            return Err(WifiError::AuthenticationFailed);
        }

        let mut ctx = self.assoc_ctx.lock();
        ctx.auth_state = AuthState::Complete;
        ctx.state = AssocState::Associating;

        drop(ctx);
        *self.state.lock() = WifiState::Associating;

        self.send_assoc_request()
    }

    fn send_assoc_request(&self) -> Result<(), WifiError> {
        let ctx = self.assoc_ctx.lock();
        let bssid = ctx.bssid;
        let privacy = ctx.security != WifiSecurity::Open;
        drop(ctx);

        let body = AssocReqBody::new(10, privacy);
        let mut frame = Vec::with_capacity(256);

        let fc = FrameControl::new(
            FrameControl::FRAME_TYPE_MGMT,
            FrameControl::SUBTYPE_ASSOC_REQ,
        );
        let header = MacHeader::new(fc, bssid, *self.mac_address.lock(), bssid);
        frame.extend_from_slice(&header.to_bytes());
        frame.extend_from_slice(&unsafe {
            core::slice::from_raw_parts(
                &body as *const AssocReqBody as *const u8,
                core::mem::size_of::<AssocReqBody>(),
            )
        });

        let ssid_bytes = self.assoc_ctx.lock().ssid.clone();
        frame.push(IE_SSID);
        frame.push(ssid_bytes.len() as u8);
        frame.extend_from_slice(ssid_bytes.as_bytes());

        let rates: [u8; 8] = [0x82, 0x84, 0x8B, 0x96, 0x0C, 0x12, 0x18, 0x60];
        frame.push(IE_SUPPORTED_RATES);
        frame.push(rates.len() as u8);
        frame.extend_from_slice(&rates);

        let _ = self.tx_frame(&frame);
        Ok(())
    }

    pub fn process_assoc_response(&self, data: &[u8]) -> Result<u16, WifiError> {
        if data.len() < MacHeader::MIN_SIZE + core::mem::size_of::<AssocRespBody>() {
            return Err(WifiError::AssociationFailed);
        }

        let body_offset = MacHeader::MIN_SIZE;
        let body = unsafe { &*(data[body_offset..].as_ptr() as *const AssocRespBody) };

        if body.status_code != AssocRespBody::STATUS_SUCCESS {
            let mut ctx = self.assoc_ctx.lock();
            ctx.state = AssocState::Failed;
            return Err(WifiError::AssociationFailed);
        }

        let mut ctx = self.assoc_ctx.lock();
        ctx.aid = body.aid & 0x3FFF;
        ctx.state = AssocState::Associated;

        if ctx.security == WifiSecurity::Open || ctx.security == WifiSecurity::WEP {
            ctx.state = AssocState::Complete;
            drop(ctx);
            self.mark_connected();
            return Ok(body.aid & 0x3FFF);
        }

        ctx.state = AssocState::Keying;
        drop(ctx);
        *self.state.lock() = WifiState::KeyExchange;

        Ok(body.aid & 0x3FFF)
    }

    pub fn process_eapol_key(&self, data: &[u8]) -> Result<(), WifiError> {
        let data = if data.len() >= MacHeader::MIN_SIZE + 4 + EapolKeyFrame::FIXED_SIZE {
            &data[MacHeader::MIN_SIZE..]
        } else {
            data
        };

        // data includes EAPOL header (4 bytes) + EAPOL-Key body (95+ bytes)
        let eapol_body_offset = 4;
        if data.len() < eapol_body_offset + EapolKeyFrame::FIXED_SIZE {
            return Err(WifiError::KeyExchangeFailed);
        }

        let key_frame = match EapolKeyFrame::from_bytes(&data[eapol_body_offset..]) {
            Some(kf) => kf,
            None => return Err(WifiError::KeyExchangeFailed),
        };

        let mut ctx = self.assoc_ctx.lock();

        match ctx.four_way_state {
            FourWayState::Idle => {
                if key_frame.is_msg1() {
                    ctx.anonce.copy_from_slice(&key_frame.nonce);
                    ctx.replay_counter = key_frame.replay_counter;
                    ctx.four_way_state = FourWayState::Msg1Received;
                    // Generate SNonce now so we can derive PTK before sending Msg2
                    ctx.snonce = generate_nonce();
                    // Derive PTK: PMK + ANonce + SNonce + AA + SPA
                    let my_mac = *self.mac_address.lock();
                    let ptk = Ptk::derive(
                        &ctx.pmk,
                        &ctx.anonce,
                        &ctx.snonce,
                        &ctx.bssid, // AA (Authenticator Address)
                        &my_mac,    // SPA (Supplicant Address)
                    );
                    ctx.ptk = Some(ptk);
                    drop(ctx);
                    self.send_eapol_msg2()
                } else {
                    Err(WifiError::KeyExchangeFailed)
                }
            }
            FourWayState::Msg2Sent => {
                if key_frame.is_msg3() {
                    ctx.anonce.copy_from_slice(&key_frame.nonce);
                    ctx.replay_counter = key_frame.replay_counter;
                    ctx.four_way_state = FourWayState::Msg3Received;
                    if key_frame.is_install() {
                        ctx.ptk_installed = true;
                    }
                    drop(ctx);
                    self.send_eapol_msg4()
                } else {
                    Err(WifiError::KeyExchangeFailed)
                }
            }
            _ => Err(WifiError::KeyExchangeFailed),
        }
    }

    fn send_eapol_msg2(&self) -> Result<(), WifiError> {
        let ctx = self.assoc_ctx.lock();
        let bssid = ctx.bssid;
        let replay = ctx.replay_counter;
        let snonce = ctx.snonce;
        let ptk = ctx.ptk.clone();
        drop(ctx);

        let ptk = match ptk {
            Some(p) => p,
            None => return Err(WifiError::KeyExchangeFailed),
        };

        // Build EAPOL-Key body with big-endian fields
        let key_frame = EapolKeyFrame {
            descriptor_type: EapolKeyFrame::DESCRIPTOR_RSN,
            key_info: EapolKeyFrame::KEY_INFO_VERSION_WPA2
                | EapolKeyFrame::KEY_INFO_KEY_TYPE
                | EapolKeyFrame::KEY_INFO_MIC,
            key_len: 16, // CCMP TK length
            replay_counter: replay + 1,
            nonce: snonce,
            key_iv: [0; 16],
            rsc: [0; 8],
            id: [0; 8],
            mic: [0; 16], // zeroed for MIC computation
            data_len: 0,
        };

        // Serialize: EAPOL header + EAPOL-Key body
        let key_bytes = key_frame.to_bytes();
        let eapol_body_len = key_bytes.len() as u16;
        let eapol_hdr = EapolHeader::new(
            EapolHeader::VERSION_802_1X_2004,
            EapolHeader::TYPE_EAPOL_KEY,
            eapol_body_len,
        );

        let mut eapol_frame = Vec::with_capacity(4 + EapolKeyFrame::FIXED_SIZE);
        eapol_frame.extend_from_slice(&eapol_hdr.to_bytes());
        eapol_frame.extend_from_slice(&key_bytes);

        // Compute MIC over entire EAPOL frame (with MIC field zeroed)
        let mic = compute_mic(&ptk.kck, &eapol_frame);
        // Write MIC into the frame at the correct offset (4 + 77 = 81)
        eapol_frame[81..97].copy_from_slice(&mic);

        // Build 802.11 data frame
        let fc = FrameControl::new(FrameControl::FRAME_TYPE_DATA, FrameControl::SUBTYPE_DATA);
        let header = MacHeader::new(fc, bssid, *self.mac_address.lock(), bssid);
        let mut frame = header.to_bytes().to_vec();
        frame.extend_from_slice(&eapol_frame);

        let _ = self.tx_frame(&frame);

        let mut ctx = self.assoc_ctx.lock();
        ctx.four_way_state = FourWayState::Msg2Sent;
        Ok(())
    }

    fn send_eapol_msg4(&self) -> Result<(), WifiError> {
        let ctx = self.assoc_ctx.lock();
        let bssid = ctx.bssid;
        let replay = ctx.replay_counter;
        let ptk = ctx.ptk.clone();
        drop(ctx);

        let ptk = match ptk {
            Some(p) => p,
            None => return Err(WifiError::KeyExchangeFailed),
        };

        let key_frame = EapolKeyFrame {
            descriptor_type: EapolKeyFrame::DESCRIPTOR_RSN,
            key_info: EapolKeyFrame::KEY_INFO_VERSION_WPA2
                | EapolKeyFrame::KEY_INFO_KEY_TYPE
                | EapolKeyFrame::KEY_INFO_MIC
                | EapolKeyFrame::KEY_INFO_SECURE,
            key_len: 0,
            replay_counter: replay + 1,
            nonce: [0; 32],
            key_iv: [0; 16],
            rsc: [0; 8],
            id: [0; 8],
            mic: [0; 16],
            data_len: 0,
        };

        let key_bytes = key_frame.to_bytes();
        let eapol_body_len = key_bytes.len() as u16;
        let eapol_hdr = EapolHeader::new(
            EapolHeader::VERSION_802_1X_2004,
            EapolHeader::TYPE_EAPOL_KEY,
            eapol_body_len,
        );

        let mut eapol_frame = Vec::with_capacity(4 + EapolKeyFrame::FIXED_SIZE);
        eapol_frame.extend_from_slice(&eapol_hdr.to_bytes());
        eapol_frame.extend_from_slice(&key_bytes);

        let mic = compute_mic(&ptk.kck, &eapol_frame);
        eapol_frame[81..97].copy_from_slice(&mic);

        let fc = FrameControl::new(FrameControl::FRAME_TYPE_DATA, FrameControl::SUBTYPE_DATA);
        let header = MacHeader::new(fc, bssid, *self.mac_address.lock(), bssid);
        let mut frame = header.to_bytes().to_vec();
        frame.extend_from_slice(&eapol_frame);

        let _ = self.tx_frame(&frame);

        let mut ctx = self.assoc_ctx.lock();
        ctx.four_way_state = FourWayState::Msg4Sent;
        ctx.state = AssocState::Complete;
        ctx.ptk_installed = true;
        ctx.gtk_installed = true;
        drop(ctx);

        self.mark_connected();
        Ok(())
    }

    fn mark_connected(&self) {
        let ctx = self.assoc_ctx.lock();
        *self.state.lock() = WifiState::Connected;
        *self.connected_ssid.lock() = Some(ctx.ssid.clone());
        *self.connected_bssid.lock() = Some(ctx.bssid);
        crate::serial_println!("[WiFi-Jail] Connected to '{}' (AID={})", ctx.ssid, ctx.aid);
    }

    pub fn disconnect(&self) -> Result<(), WifiError> {
        let state = *self.state.lock();
        if state != WifiState::Connected {
            return Err(WifiError::NotConnected);
        }

        let bssid = {
            let connected_bssid = self.connected_bssid.lock();
            match *connected_bssid {
                Some(b) => b,
                None => return Err(WifiError::NotConnected),
            }
        };

        let mut frame = Vec::with_capacity(48);
        let fc = FrameControl::new(
            FrameControl::FRAME_TYPE_MGMT,
            FrameControl::SUBTYPE_DISASSOC,
        );
        let header = MacHeader::new(fc, bssid, *self.mac_address.lock(), bssid);
        frame.extend_from_slice(&header.to_bytes());
        frame.push(0);
        frame.push(0);
        frame.push(3);
        frame.push(0);

        let ssid = self.connected_ssid.lock().clone().unwrap_or_default();

        let _ = self.tx_frame(&frame);

        *self.state.lock() = WifiState::Disconnected;
        *self.connected_ssid.lock() = None;
        *self.connected_bssid.lock() = None;
        *self.mlo_session.lock() = None;
        self.assoc_ctx.lock().reset();

        crate::serial_println!("[WiFi-Jail] Disconnected from '{}'", ssid);
        Ok(())
    }

    // ========================================================================
    // MLO Planning
    // ========================================================================

    fn estimated_link_mbps(bss: &WifiBss) -> u32 {
        let base = match bss.phy_mode {
            WifiPhyMode::Dot11B => 11,
            WifiPhyMode::Dot11G => 54,
            WifiPhyMode::Dot11N => 300,
            WifiPhyMode::Dot11AC => 866,
            WifiPhyMode::Dot11AX => 1200,
            WifiPhyMode::Dot11BE => 2400,
        };
        let width_factor = match bss.channel_width {
            20 => 1,
            40 => 2,
            80 => 4,
            160 => 8,
            320 => 16,
            _ => 1,
        };
        let band_gain = match bss.band {
            WifiBand::Band2G => 85,
            WifiBand::Band5G => 100,
            WifiBand::Band6G => 115,
        };
        let rssi_gain = (bss.rssi as i32 + 100).clamp(25, 70) as u32;
        ((base * width_factor) * band_gain as u32 * rssi_gain) / 7000
    }

    fn link_score(bss: &WifiBss) -> u32 {
        let band_score = match bss.band {
            WifiBand::Band2G => 8,
            WifiBand::Band5G => 18,
            WifiBand::Band6G => 28,
        };
        let phy_score = match bss.phy_mode {
            WifiPhyMode::Dot11B => 1,
            WifiPhyMode::Dot11G => 4,
            WifiPhyMode::Dot11N => 10,
            WifiPhyMode::Dot11AC => 18,
            WifiPhyMode::Dot11AX => 26,
            WifiPhyMode::Dot11BE => 34,
        };
        let width_score = match bss.channel_width {
            20 => 4,
            40 => 8,
            80 => 14,
            160 => 22,
            320 => 32,
            _ => 0,
        };
        let security_score = match bss.security {
            WifiSecurity::WPA3Personal | WifiSecurity::WPA3Enterprise => 12,
            WifiSecurity::WPA2Personal | WifiSecurity::WPA2Enterprise => 8,
            WifiSecurity::WPA | WifiSecurity::WEP => 2,
            WifiSecurity::Open => 0,
        };
        let signal_score = (bss.rssi as i32 + 100).clamp(0, 60) as u32;
        signal_score * 4 + band_score * 3 + phy_score * 5 + width_score * 2 + security_score
    }

    fn bss_to_link(bss: &WifiBss) -> WifiMloLink {
        WifiMloLink {
            bssid: bss.bssid,
            band: bss.band,
            channel: bss.channel,
            frequency: bss.frequency,
            phy_mode: bss.phy_mode,
            channel_width: bss.channel_width,
            rssi: bss.rssi,
            score: Self::link_score(bss),
            estimated_mbps: Self::estimated_link_mbps(bss),
        }
    }

    fn security_compatible(expected: WifiSecurity, observed: WifiSecurity) -> bool {
        if expected == observed {
            return true;
        }
        matches!(
            (expected, observed),
            (WifiSecurity::WPA3Personal, WifiSecurity::WPA2Personal)
                | (WifiSecurity::WPA3Enterprise, WifiSecurity::WPA2Enterprise)
        )
    }

    pub fn plan_mlo_for_ssid(&self, ssid: &str, security: WifiSecurity) -> Option<WifiMloSession> {
        let mut candidates: Vec<WifiBss> = self
            .scan_results
            .lock()
            .iter()
            .filter(|bss| bss.ssid == ssid && Self::security_compatible(security, bss.security))
            .cloned()
            .collect();

        if candidates.is_empty() {
            return None;
        }

        candidates.sort_by(|a, b| {
            Self::link_score(b)
                .cmp(&Self::link_score(a))
                .then_with(|| Self::estimated_link_mbps(b).cmp(&Self::estimated_link_mbps(a)))
        });

        let primary = Self::bss_to_link(&candidates[0]);
        let mut secondary = Vec::new();
        let mut used_bands = vec![primary.band];

        for candidate in candidates.iter().skip(1) {
            if used_bands.contains(&candidate.band) {
                continue;
            }
            let link = Self::bss_to_link(candidate);
            let min_secondary_mbps = (primary.estimated_mbps / 8).max(54);
            if link.estimated_mbps < min_secondary_mbps && link.score + 12 < primary.score {
                continue;
            }
            secondary.push(link.clone());
            used_bands.push(candidate.band);
            if secondary.len() == 2 {
                break;
            }
        }

        let total_raw = primary.estimated_mbps
            + secondary
                .iter()
                .map(|link| link.estimated_mbps)
                .sum::<u32>();
        let efficiency = 92u32.saturating_sub((secondary.len() as u32) * 7);
        let aggregate_mbps = total_raw * efficiency / 100;

        let total_rssi =
            primary.rssi as i32 + secondary.iter().map(|link| link.rssi as i32).sum::<i32>();
        let average_rssi = (total_rssi / (1 + secondary.len()) as i32) as i8;

        Some(WifiMloSession {
            ssid: String::from(ssid),
            security,
            primary,
            secondary,
            aggregate_mbps,
            average_rssi,
        })
    }

    // ========================================================================
    // Crash-Only Microreboot (MINIX 3 modeli)
    // ========================================================================

    pub fn crash_and_reboot(&self) -> Result<u64, WifiError> {
        let crashes = self.crash_count.fetch_add(1, Ordering::SeqCst) + 1;
        self.consecutive_crashes.fetch_add(1, Ordering::SeqCst);
        let new_seq = self.reboot_seq.fetch_add(1, Ordering::SeqCst) + 1;

        let backoff = self.calculate_backoff(crashes);

        crate::serial_println!(
            "[WiFi-Jail] Crash #{}, backoff={}ms, microreboot seq={}",
            crashes,
            backoff,
            new_seq
        );

        self.reset_internal()?;
        self.initialized.store(true, Ordering::SeqCst);
        self.last_healthy_seq.store(new_seq, Ordering::SeqCst);

        self.consecutive_crashes.store(0, Ordering::SeqCst);
        self.restart_backoff_ms.store(100, Ordering::SeqCst);

        crate::serial_println!("[WiFi-Jail] Microreboot complete, seq={}", new_seq);
        Ok(new_seq)
    }

    fn calculate_backoff(&self, crash_count: u32) -> u64 {
        let base = self.restart_backoff_ms.load(Ordering::Relaxed);
        let max = self.max_backoff_ms.load(Ordering::Relaxed);
        let backoff = base.saturating_mul(2u64.saturating_pow(crash_count.min(10) as u32));
        backoff.min(max)
    }

    fn reset_internal(&self) -> Result<(), WifiError> {
        *self.state.lock() = WifiState::Disconnected;
        *self.connected_ssid.lock() = None;
        *self.connected_bssid.lock() = None;
        *self.scan_results.lock() = Vec::new();
        *self.mlo_session.lock() = None;
        *self.scan_state.lock() = ScanState::Idle;
        *self.firmware_state.lock() = FirmwareState::NotLoaded;
        self.assoc_ctx.lock().reset();
        self.init_rings();
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
    // Jail Command Processing
    // ========================================================================

    pub fn process_command(&self, cmd: WifiJailCommand) -> WifiJailResponse {
        if !self.initialized.load(Ordering::SeqCst) {
            if !matches!(&cmd, WifiJailCommand::FirmwareLoad { .. }) {
                return WifiJailResponse::Error(WifiError::NotInitialized);
            }
        }

        match cmd {
            WifiJailCommand::FirmwareLoad { version } => {
                crate::serial_println!(
                    "[WiFi-Jail] FirmwareLoad(version={:#x}) rejected: firmware bytes required",
                    version
                );
                WifiJailResponse::Error(WifiError::FirmwareLoadFailed)
            }
            WifiJailCommand::Scan { passive } => {
                let config = if passive {
                    ScanConfig::default_passive()
                } else {
                    ScanConfig::default_active()
                };
                match self.start_scan(config) {
                    Ok(()) => {
                        while let Ok(done) = self.scan_step() {
                            if done {
                                break;
                            }
                        }
                        let results = self.scan_results.lock().clone();
                        WifiJailResponse::ScanResults(results)
                    }
                    Err(e) => WifiJailResponse::Error(e),
                }
            }
            WifiJailCommand::Connect {
                ssid,
                password,
                security,
            } => {
                let state = *self.state.lock();
                if state == WifiState::Connected {
                    return WifiJailResponse::Error(WifiError::AlreadyConnected);
                }

                let mlo_session = match self.plan_mlo_for_ssid(&ssid, security) {
                    Some(s) => s,
                    None => {
                        return WifiJailResponse::Error(WifiError::AssociationFailed);
                    }
                };

                let primary_bss = WifiBss {
                    bssid: mlo_session.primary.bssid,
                    ssid: ssid.clone(),
                    rssi: mlo_session.primary.rssi,
                    channel: mlo_session.primary.channel,
                    frequency: mlo_session.primary.frequency,
                    band: mlo_session.primary.band,
                    security,
                    phy_mode: mlo_session.primary.phy_mode,
                    channel_width: mlo_session.primary.channel_width,
                };

                match self.start_association(&primary_bss, &password) {
                    Ok(()) => {
                        crate::serial_println!(
                            "[WiFi-Jail] Association request for '{}' transmitted; waiting for AP frames",
                            ssid
                        );
                        WifiJailResponse::Error(WifiError::Timeout)
                    }
                    Err(e) => WifiJailResponse::Error(e),
                }
            }
            WifiJailCommand::Disconnect => match self.disconnect() {
                Ok(()) => WifiJailResponse::Ok,
                Err(e) => WifiJailResponse::Error(e),
            },
            WifiJailCommand::GetMacAddress => {
                WifiJailResponse::MacAddress(*self.mac_address.lock())
            }
            WifiJailCommand::GetStats => WifiJailResponse::Stats(self.stats.lock().clone()),
            WifiJailCommand::GetFirmwareVersion => {
                let info = self.firmware_info.lock();
                match info.as_ref() {
                    Some(info) => WifiJailResponse::FirmwareVersion(format!(
                        "echOS-WiFi-Jail v{}.{}.{}",
                        info.version >> 16,
                        (info.version >> 8) & 0xFF,
                        info.version & 0xFF
                    )),
                    None => {
                        WifiJailResponse::FirmwareVersion(String::from("echOS-WiFi-Jail v1.0.0"))
                    }
                }
            }
            WifiJailCommand::SetPowerSave(enable) => {
                crate::serial_println!(
                    "[WiFi-Jail] Power save: {}",
                    if enable { "ON" } else { "OFF" }
                );
                WifiJailResponse::Ok
            }
            WifiJailCommand::SetChannel(ch) => {
                self.current_channel.store(ch as u32, Ordering::Release);
                crate::serial_println!("[WiFi-Jail] Channel set: {}", ch);
                WifiJailResponse::Ok
            }
            WifiJailCommand::SetTxPower(dbm) => {
                crate::serial_println!("[WiFi-Jail] TX power: {} dBm", dbm);
                WifiJailResponse::Ok
            }
            WifiJailCommand::TxFrame { data } => match self.tx_frame(&data) {
                Ok(seq) => WifiJailResponse::FrameTxComplete { seq },
                Err(e) => WifiJailResponse::Error(e),
            },
        }
    }

    #[cfg(test)]
    fn drive_assoc_fixture_flow(
        &self,
        bss: &WifiBss,
        security: WifiSecurity,
    ) -> Result<(), WifiError> {
        let auth_frame = self.build_auth_frame(bss.bssid, security);
        let _ = self.tx_frame(&auth_frame);

        let auth_resp = self.build_auth_response(bss.bssid);
        self.process_auth_response(&auth_resp)?;

        let assoc_req = self.build_assoc_req_frame(bss.bssid, security);
        let _ = self.tx_frame(&assoc_req);

        let assoc_resp = self.build_assoc_response(bss.bssid);
        self.process_assoc_response(&assoc_resp)?;

        if security != WifiSecurity::Open && security != WifiSecurity::WEP {
            let msg1 = self.build_eapol_msg1(bss.bssid);
            self.process_eapol_key(&msg1)?;

            let msg3 = self.build_eapol_msg3(bss.bssid);
            self.process_eapol_key(&msg3)?;
        }

        Ok(())
    }

    #[cfg(test)]
    fn build_auth_frame(&self, bssid: [u8; 6], security: WifiSecurity) -> Vec<u8> {
        let body = match security {
            WifiSecurity::WPA3Personal | WifiSecurity::WPA3Enterprise => {
                AuthFrameBody::sae_request()
            }
            _ => AuthFrameBody::open_system_request(),
        };
        let mut frame = Vec::with_capacity(64);
        let fc = FrameControl::new(FrameControl::FRAME_TYPE_MGMT, FrameControl::SUBTYPE_AUTH);
        let header = MacHeader::new(fc, bssid, *self.mac_address.lock(), bssid);
        frame.extend_from_slice(&header.to_bytes());
        frame.extend_from_slice(&unsafe {
            core::slice::from_raw_parts(
                &body as *const AuthFrameBody as *const u8,
                core::mem::size_of::<AuthFrameBody>(),
            )
        });
        frame
    }

    #[cfg(test)]
    fn build_auth_response(&self, bssid: [u8; 6]) -> Vec<u8> {
        let body = AuthFrameBody::open_system_response();
        let mut frame = Vec::with_capacity(64);
        let fc = FrameControl::new(FrameControl::FRAME_TYPE_MGMT, FrameControl::SUBTYPE_AUTH);
        let header = MacHeader::new(fc, *self.mac_address.lock(), bssid, bssid);
        frame.extend_from_slice(&header.to_bytes());
        frame.extend_from_slice(&unsafe {
            core::slice::from_raw_parts(
                &body as *const AuthFrameBody as *const u8,
                core::mem::size_of::<AuthFrameBody>(),
            )
        });
        frame
    }

    #[cfg(test)]
    fn build_assoc_req_frame(&self, bssid: [u8; 6], security: WifiSecurity) -> Vec<u8> {
        let body = AssocReqBody::new(10, security != WifiSecurity::Open);
        let mut frame = Vec::with_capacity(256);
        let fc = FrameControl::new(
            FrameControl::FRAME_TYPE_MGMT,
            FrameControl::SUBTYPE_ASSOC_REQ,
        );
        let header = MacHeader::new(fc, bssid, *self.mac_address.lock(), bssid);
        frame.extend_from_slice(&header.to_bytes());
        frame.extend_from_slice(&unsafe {
            core::slice::from_raw_parts(
                &body as *const AssocReqBody as *const u8,
                core::mem::size_of::<AssocReqBody>(),
            )
        });
        let ssid = self.assoc_ctx.lock().ssid.clone();
        frame.push(IE_SSID);
        frame.push(ssid.len() as u8);
        frame.extend_from_slice(ssid.as_bytes());
        let rates: [u8; 8] = [0x82, 0x84, 0x8B, 0x96, 0x0C, 0x12, 0x18, 0x60];
        frame.push(IE_SUPPORTED_RATES);
        frame.push(rates.len() as u8);
        frame.extend_from_slice(&rates);
        frame
    }

    #[cfg(test)]
    fn build_assoc_response(&self, bssid: [u8; 6]) -> Vec<u8> {
        let body = AssocRespBody::success(AssocReqBody::CAP_ESS | AssocReqBody::CAP_QOS, 1);
        let mut frame = Vec::with_capacity(64);
        let fc = FrameControl::new(
            FrameControl::FRAME_TYPE_MGMT,
            FrameControl::SUBTYPE_ASSOC_RESP,
        );
        let header = MacHeader::new(fc, *self.mac_address.lock(), bssid, bssid);
        frame.extend_from_slice(&header.to_bytes());
        frame.extend_from_slice(&unsafe {
            core::slice::from_raw_parts(
                &body as *const AssocRespBody as *const u8,
                core::mem::size_of::<AssocRespBody>(),
            )
        });
        frame
    }

    #[cfg(test)]
    fn build_eapol_msg1(&self, bssid: [u8; 6]) -> Vec<u8> {
        let key_frame = EapolKeyFrame {
            descriptor_type: EapolKeyFrame::DESCRIPTOR_RSN,
            key_info: EapolKeyFrame::KEY_INFO_VERSION_WPA2
                | EapolKeyFrame::KEY_INFO_KEY_TYPE
                | EapolKeyFrame::KEY_INFO_ACK,
            key_len: 16,
            replay_counter: 1,
            nonce: [0xAA; 32],
            key_iv: [0; 16],
            rsc: [0; 8],
            id: [0; 8],
            mic: [0; 16],
            data_len: 0,
        };
        let key_bytes = key_frame.to_bytes();
        let eapol_body_len = key_bytes.len() as u16;
        let eapol_hdr = EapolHeader::new(
            EapolHeader::VERSION_802_1X_2004,
            EapolHeader::TYPE_EAPOL_KEY,
            eapol_body_len,
        );

        let mut frame = eapol_hdr.to_bytes().to_vec();
        frame.extend_from_slice(&key_bytes);

        let fc = FrameControl::new(FrameControl::FRAME_TYPE_DATA, FrameControl::SUBTYPE_DATA);
        let header = MacHeader::new(fc, *self.mac_address.lock(), bssid, bssid);
        let mut full_frame = header.to_bytes().to_vec();
        full_frame.extend_from_slice(&frame);
        full_frame
    }

    #[cfg(test)]
    fn build_eapol_msg3(&self, bssid: [u8; 6]) -> Vec<u8> {
        // Per IEEE 802.11i §8.5.3.3: Message 3 has ACK=1, MIC=1, Secure=1, Install=1
        let key_frame = EapolKeyFrame {
            descriptor_type: EapolKeyFrame::DESCRIPTOR_RSN,
            key_info: EapolKeyFrame::KEY_INFO_VERSION_WPA2
                | EapolKeyFrame::KEY_INFO_KEY_TYPE
                | EapolKeyFrame::KEY_INFO_ACK
                | EapolKeyFrame::KEY_INFO_MIC
                | EapolKeyFrame::KEY_INFO_SECURE
                | EapolKeyFrame::KEY_INFO_INSTALL,
            key_len: 16,
            replay_counter: 3,
            nonce: [0xBB; 32],
            key_iv: [0; 16],
            rsc: [0; 8],
            id: [0; 8],
            mic: [0; 16],
            data_len: 0,
        };
        let key_bytes = key_frame.to_bytes();
        let eapol_body_len = key_bytes.len() as u16;
        let eapol_hdr = EapolHeader::new(
            EapolHeader::VERSION_802_1X_2004,
            EapolHeader::TYPE_EAPOL_KEY,
            eapol_body_len,
        );

        let mut frame = eapol_hdr.to_bytes().to_vec();
        frame.extend_from_slice(&key_bytes);

        let fc = FrameControl::new(FrameControl::FRAME_TYPE_DATA, FrameControl::SUBTYPE_DATA);
        let header = MacHeader::new(fc, *self.mac_address.lock(), bssid, bssid);
        let mut full_frame = header.to_bytes().to_vec();
        full_frame.extend_from_slice(&frame);
        full_frame
    }

    #[cfg(test)]
    fn build_firmware_fixture(version: u32) -> Vec<u8> {
        let header_size = 20u32;
        let section_size = 12u32;
        let num_sections = 3u32;
        let total_size = (header_size + section_size * num_sections + 64) as usize;
        let mut data = vec![0u8; total_size];

        let header = FirmwareHeader {
            magic: FirmwareHeader::MAGIC,
            version,
            header_size,
            section_count: num_sections,
            entry_point: 0x1000,
        };
        let header_bytes = unsafe {
            core::slice::from_raw_parts(
                &header as *const FirmwareHeader as *const u8,
                core::mem::size_of::<FirmwareHeader>(),
            )
        };
        data[..header_size as usize].copy_from_slice(header_bytes);

        let mut pos = header_size as usize;
        for i in 0..num_sections {
            let sec = FirmwareSection {
                section_type: i,
                offset: (header_size + section_size * num_sections) + i * 16,
                size: 16,
            };
            let sec_bytes = unsafe {
                core::slice::from_raw_parts(
                    &sec as *const FirmwareSection as *const u8,
                    core::mem::size_of::<FirmwareSection>(),
                )
            };
            data[pos..pos + section_size as usize].copy_from_slice(sec_bytes);
            pos += section_size as usize;
        }

        for i in 0..num_sections {
            let offset = (header_size + section_size * num_sections) + i * 16;
            for j in 0..16usize {
                data[offset as usize + j] = (i * 16 + j as u32) as u8;
            }
        }

        data
    }

    // ========================================================================
    // Initialization
    // ========================================================================

    pub fn init(&self) -> Result<(), WifiError> {
        for dev in crate::drivers::pci::scan() {
            let is_wifi = (dev.class_code == 0x02 && dev.subclass == 0x80)
                || (dev.vendor_id == 0x8086 && dev.class_code == 0x02);

            if is_wifi {
                crate::serial_println!(
                    "[WiFi Jail] Found WiFi adapter: {:04x}:{:04x} (class={:02x}.{:02x})",
                    dev.vendor_id,
                    dev.device_id,
                    dev.class_code,
                    dev.subclass
                );

                let mut mac = self.mac_address.lock();
                *mac = [
                    0x02,
                    0x00,
                    0x00,
                    (dev.vendor_id & 0xFF) as u8,
                    (dev.device_id >> 8) as u8,
                    (dev.device_id & 0xFF) as u8,
                ];

                self.init_rings();
                self.initialized.store(true, Ordering::SeqCst);
                self.jail_token.store(0xCAFE_0001, Ordering::SeqCst);

                crate::serial_println!(
                    "[WiFi Jail] MAC: {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                    mac[0],
                    mac[1],
                    mac[2],
                    mac[3],
                    mac[4],
                    mac[5]
                );
                return Ok(());
            }
        }

        crate::serial_println!("[WiFi Jail] No WiFi adapter found");
        Err(WifiError::DeviceNotFound)
    }

    // ========================================================================
    // JailChannel Integration
    // ========================================================================

    pub fn attach_channel(&mut self, channel: JailChannel) {
        let cid = channel.channel_id;
        self.jail_channel = Some(channel);
        crate::serial_println!("[WiFi-Jail] JailChannel {} attached", cid);
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
            JailOpcode::Read => {
                let packet = self.rx_poll();
                packet.map(|p| p.len() as i64).unwrap_or(0)
            }
            JailOpcode::Write => self.tx_reclaim() as i64,
            JailOpcode::Control => {
                self.handle_control_request(req.offset as u32, req.length as u32)
            }
            JailOpcode::Reset => match self.crash_and_reboot() {
                Ok(seq) => seq as i64,
                Err(_) => -1i64,
            },
            JailOpcode::Status => {
                let state = *self.state.lock();
                (match state {
                    WifiState::Connected => 1,
                    WifiState::Scanning => 2,
                    WifiState::Authenticating => 3,
                    WifiState::Associating => 4,
                    WifiState::KeyExchange => 5,
                    _ => 0,
                }) as i64
            }
            JailOpcode::Nop => 0i64,
            JailOpcode::Flush => self.rx_refill() as i64,
        };

        JailEvent {
            request_id: req.request_id,
            result,
            data_len: if result >= 0 { result as u32 } else { 0 },
            jail_id: self.jail_id as u16,
            flags: 0,
        }
    }

    fn handle_control_request(&self, cmd: u32, param: u32) -> i64 {
        match cmd {
            0 => {
                let config = ScanConfig::default_active();
                match self.start_scan(config) {
                    Ok(()) => 0,
                    Err(_) => -1,
                }
            }
            1 => match self.disconnect() {
                Ok(()) => 0,
                Err(_) => -1,
            },
            2 => {
                self.current_channel.store(param, Ordering::Release);
                0
            }
            3 => {
                let enable = param != 0;
                crate::serial_println!(
                    "[WiFi-Jail] Power save: {}",
                    if enable { "ON" } else { "OFF" }
                );
                0
            }
            _ => -1,
        }
    }

    // ========================================================================
    // Public getters
    // ========================================================================

    pub fn get_state(&self) -> WifiState {
        *self.state.lock()
    }

    pub fn connected_ssid(&self) -> Option<String> {
        self.connected_ssid.lock().clone()
    }

    pub fn mlo_session(&self) -> Option<WifiMloSession> {
        self.mlo_session.lock().clone()
    }
}

// ============================================================================
// Global Instance
// ============================================================================

lazy_static::lazy_static! {
    pub static ref WIFI_JAIL: WifiJailController = WifiJailController::new();
}

pub fn init() {
    crate::serial_println!("[WiFi Jail] TIER 2 WiFi driver initializing...");
    match WIFI_JAIL.init() {
        Ok(()) => crate::serial_println!("[WiFi Jail] Initialization complete"),
        Err(e) => crate::serial_println!("[WiFi Jail] Init skipped: {:?}", e),
    }
}

// ============================================================================
// Host Corpus Tests (D-WIFI-01)
// ============================================================================

#[cfg(test)]
mod wifi_jail_tests {
    use super::*;

    #[test]
    fn frame_control_type_subtype_decode() {
        let fc = FrameControl::new(FrameControl::FRAME_TYPE_MGMT, FrameControl::SUBTYPE_BEACON);
        assert!(fc.is_mgmt());
        assert!(fc.is_beacon());
        assert!(!fc.is_data());
        assert!(!fc.is_ctrl());
    }

    #[test]
    fn frame_control_data_qos_decode() {
        let fc = FrameControl::new(
            FrameControl::FRAME_TYPE_DATA,
            FrameControl::SUBTYPE_QOS_DATA,
        );
        assert!(fc.is_data());
        assert!(!fc.is_mgmt());
    }

    #[test]
    fn mac_header_sequence_extraction() {
        let mut header = MacHeader::new(
            FrameControl::new(FrameControl::FRAME_TYPE_MGMT, FrameControl::SUBTYPE_BEACON),
            [0xFF; 6],
            [0xAA; 6],
            [0xBB; 6],
        );
        header.set_sequence(0x123, 5);
        assert_eq!(header.sequence_number(), 0x123);
        assert_eq!(header.fragment_number(), 5);
    }

    #[test]
    fn mac_header_to_bytes_roundtrip() {
        let header = MacHeader::new(
            FrameControl::new(FrameControl::FRAME_TYPE_MGMT, FrameControl::SUBTYPE_AUTH),
            [0x11; 6],
            [0x22; 6],
            [0x33; 6],
        );
        let bytes = header.to_bytes();
        assert_eq!(bytes.len(), 24);
        assert_eq!(bytes[0], header.frame_ctrl.raw as u8);
        assert_eq!(bytes[4..10], header.addr1);
        assert_eq!(bytes[10..16], header.addr2);
        assert_eq!(bytes[16..22], header.addr3);
    }

    #[test]
    fn auth_frame_body_open_system() {
        let req = AuthFrameBody::open_system_request();
        assert_eq!(req.auth_algorithm, AuthFrameBody::ALGO_OPEN_SYSTEM);
        assert_eq!(req.auth_transaction_seq, 1);

        let resp = AuthFrameBody::open_system_response();
        assert_eq!(resp.auth_transaction_seq, 2);
        assert_eq!(resp.status_code, AuthFrameBody::STATUS_SUCCESS);
    }

    #[test]
    fn auth_frame_body_sae() {
        let req = AuthFrameBody::sae_request();
        assert_eq!(req.auth_algorithm, AuthFrameBody::ALGO_SAE);
        assert_eq!(req.auth_transaction_seq, 1);
    }

    #[test]
    fn assoc_req_body_capability_flags() {
        let body = AssocReqBody::new(10, true);
        assert!((body.capability_info & AssocReqBody::CAP_PRIVACY) != 0);
        assert!((body.capability_info & AssocReqBody::CAP_QOS) != 0);
        assert!((body.capability_info & AssocReqBody::CAP_SHORT_SLOT) != 0);
    }

    #[test]
    fn assoc_req_body_no_privacy() {
        let body = AssocReqBody::new(5, false);
        assert!((body.capability_info & AssocReqBody::CAP_PRIVACY) == 0);
        assert_eq!(body.listen_interval, 5);
    }

    #[test]
    fn assoc_resp_body_success() {
        let resp = AssocRespBody::success(0x0210, 1);
        assert_eq!(resp.status_code, AssocRespBody::STATUS_SUCCESS);
        assert_eq!(resp.aid, 1);
        assert_eq!(resp.capability_info, 0x0210);
    }

    #[test]
    fn eapol_key_frame_msg_detection() {
        let msg1 = EapolKeyFrame {
            descriptor_type: EapolKeyFrame::DESCRIPTOR_RSN,
            key_info: EapolKeyFrame::KEY_INFO_ACK,
            key_len: 16,
            replay_counter: 1,
            nonce: [0; 32],
            key_iv: [0; 16],
            rsc: [0; 8],
            id: [0; 8],
            mic: [0; 16],
            data_len: 0,
        };
        assert!(msg1.is_msg1());
        assert!(!msg1.is_msg2());
        assert!(!msg1.is_msg3());
        assert!(!msg1.is_msg4());
        assert!(!msg1.is_install());
    }

    #[test]
    fn eapol_key_msg3_has_install() {
        let msg3 = EapolKeyFrame {
            descriptor_type: EapolKeyFrame::DESCRIPTOR_RSN,
            key_info: EapolKeyFrame::KEY_INFO_ACK
                | EapolKeyFrame::KEY_INFO_MIC
                | EapolKeyFrame::KEY_INFO_SECURE
                | EapolKeyFrame::KEY_INFO_INSTALL,
            key_len: 16,
            replay_counter: 3,
            nonce: [0; 32],
            key_iv: [0; 16],
            rsc: [0; 8],
            id: [0; 8],
            mic: [0; 16],
            data_len: 0,
        };
        assert!(msg3.is_msg3());
        assert!(msg3.is_install());
    }

    #[test]
    fn eapol_key_msg4_secure() {
        let msg4 = EapolKeyFrame {
            descriptor_type: EapolKeyFrame::DESCRIPTOR_RSN,
            key_info: EapolKeyFrame::KEY_INFO_MIC | EapolKeyFrame::KEY_INFO_SECURE,
            key_len: 0,
            replay_counter: 4,
            nonce: [0; 32],
            key_iv: [0; 16],
            rsc: [0; 8],
            id: [0; 8],
            mic: [0; 16],
            data_len: 0,
        };
        assert!(msg4.is_msg4());
    }

    #[test]
    fn parse_ies_extract_ssid() {
        let mut data = Vec::new();
        data.push(IE_SSID);
        data.push(7);
        data.extend_from_slice(b"echOS-L");
        data.push(IE_SUPPORTED_RATES);
        data.push(4);
        data.extend_from_slice(&[0x82, 0x84, 0x8B, 0x96]);

        let ies = parse_ies(&data);
        assert_eq!(ies.len(), 2);
        assert_eq!(ies[0].0, IE_SSID);
        assert_eq!(extract_ssid(&ies), "echOS-L");
    }

    #[test]
    fn parse_ies_extract_channel() {
        let mut data = Vec::new();
        data.push(IE_DS_PARAM);
        data.push(1);
        data.push(36);

        let ies = parse_ies(&data);
        assert_eq!(extract_channel(&ies), Some(36));
    }

    #[test]
    fn parse_ies_rsn_detection() {
        let mut data = Vec::new();
        data.push(IE_SSID);
        data.push(4);
        data.extend_from_slice(b"Test");
        data.push(IE_RSN);
        data.push(2);
        data.push(1);
        data.push(0);

        let ies = parse_ies(&data);
        let security = extract_rsn_info(&ies);
        assert_eq!(security, Some(WifiSecurity::WPA2Personal));
    }

    #[test]
    fn build_probe_request_frame() {
        let rates: [u8; 4] = [0x82, 0x84, 0x8B, 0x96];
        let frame = build_probe_request(Some("echOS"), &rates);
        assert!(frame.len() > MacHeader::MIN_SIZE);

        let fc = FrameControl {
            raw: u16::from_le_bytes([frame[0], frame[1]]),
        };
        assert!(fc.is_mgmt());
        assert!(fc.is_probe_req());
    }

    #[test]
    fn build_probe_request_wildcard_ssid() {
        let rates: [u8; 1] = [0x82];
        let frame = build_probe_request(None, &rates);
        assert!(frame.len() > MacHeader::MIN_SIZE);

        let ssid_offset = MacHeader::MIN_SIZE;
        assert_eq!(frame[ssid_offset], IE_SSID);
        assert_eq!(frame[ssid_offset + 1], 0);
    }

    #[test]
    fn tx_ring_submit_and_complete() {
        let mut ring = TxRing::new(8);
        let data = [0xAA, 0xBB, 0xCC, 0xDD];
        assert!(ring.submit(&data, true).is_ok());
        assert!(!ring.is_empty());

        ring.descriptors[0].status = TxDescriptor::STAT_DD;
        let completed = ring.poll_completion();
        assert!(completed.is_some());
        assert_eq!(completed.unwrap(), 0);
    }

    #[test]
    fn tx_ring_full_rejection() {
        let mut ring = TxRing::new(4);
        let data = [0x01; 64];
        for _ in 0..4 {
            assert!(ring.submit(&data, true).is_ok());
        }
        assert!(ring.submit(&data, true).is_err());
    }

    #[test]
    fn tx_ring_reclaim() {
        let mut ring = TxRing::new(8);
        let data = [0x02; 32];
        for i in 0..3 {
            ring.submit(&data, true).unwrap();
            ring.descriptors[i].status = TxDescriptor::STAT_DD;
        }
        let reclaimed = ring.reclaim_completed();
        assert_eq!(reclaimed, 3);
        assert!(ring.is_empty());
    }

    #[test]
    fn rx_ring_refill_and_poll() {
        let mut ring = RxRing::new(8);
        let refilled = ring.refill();
        assert_eq!(refilled, 8);

        ring.descriptors[0].status = RxDescriptor::STAT_DD;
        ring.descriptors[0].length = 100;
        ring.buffers[0][0..4].copy_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);

        let packet = ring.poll_packet();
        assert!(packet.is_some());
        let packet = packet.unwrap();
        assert_eq!(packet.len(), 100);
        assert_eq!(&packet[0..4], &[0xDE, 0xAD, 0xBE, 0xEF]);
    }

    #[test]
    fn rx_ring_error_handling() {
        let mut ring = RxRing::new(4);
        ring.refill();
        ring.descriptors[0].status = RxDescriptor::STAT_DD;
        ring.descriptors[0].errors = RxDescriptor::ERR_CE;
        ring.descriptors[0].length = 50;

        let packet = ring.poll_packet();
        assert!(packet.is_some());
        assert!(packet.unwrap().is_empty());
    }

    #[test]
    fn firmware_header_validation() {
        let fw = WifiJailController::build_firmware_fixture(0x010203);
        let header = unsafe { &*(fw.as_ptr() as *const FirmwareHeader) };
        assert!(header.validate());
        assert_eq!(header.version, 0x010203);
        assert_eq!(header.section_count, 3);
        assert_eq!(header.entry_point, 0x1000);
    }

    #[test]
    fn firmware_load_success() {
        let ctrl = WifiJailController::new();
        let fw = WifiJailController::build_firmware_fixture(0x020000);
        let info = ctrl.load_firmware(&fw).unwrap();
        assert_eq!(info.version, 0x020000);
        assert_eq!(info.sections.len(), 3);
        assert_eq!(ctrl.firmware_state(), FirmwareState::Operational);
    }

    #[test]
    fn firmware_load_invalid_magic() {
        let ctrl = WifiJailController::new();
        let bad_fw = vec![0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00];
        let result = ctrl.load_firmware(&bad_fw);
        assert!(result.is_err());
        assert_eq!(ctrl.firmware_state(), FirmwareState::Error);
    }

    #[test]
    fn firmware_load_too_short() {
        let ctrl = WifiJailController::new();
        let short_fw = vec![0x01, 0x02];
        let result = ctrl.load_firmware(&short_fw);
        assert!(result.is_err());
    }

    #[test]
    fn wifi_jail_controller_initial_state() {
        let ctrl = WifiJailController::new();
        assert!(!ctrl.initialized.load(Ordering::Relaxed));
        assert_eq!(ctrl.crash_count.load(Ordering::Relaxed), 0);
        assert_eq!(ctrl.firmware_state(), FirmwareState::NotLoaded);
    }

    #[test]
    fn wifi_jail_scan_state_machine() {
        let ctrl = WifiJailController::new();
        ctrl.initialized.store(true, Ordering::SeqCst);
        ctrl.init_rings();

        let config = ScanConfig::default_active();
        assert!(ctrl.start_scan(config).is_ok());
        assert_eq!(*ctrl.scan_state.lock(), ScanState::SwitchingChannel);

        while let Ok(done) = ctrl.scan_step() {
            if done {
                break;
            }
        }
        assert_eq!(*ctrl.scan_state.lock(), ScanState::Complete);
    }

    #[test]
    fn wifi_jail_process_beacon() {
        let ctrl = WifiJailController::new();
        ctrl.init_rings();
        *ctrl.current_band.lock() = WifiBand::Band5G;

        let mut beacon_data = Vec::with_capacity(128);
        let fc = FrameControl::new(FrameControl::FRAME_TYPE_MGMT, FrameControl::SUBTYPE_BEACON);
        let header = MacHeader::new(fc, [0xFF; 6], [0xAA; 6], [0xBB; 6]);
        beacon_data.extend_from_slice(&header.to_bytes());

        let beacon_body = BeaconBody {
            timestamp: 12345,
            beacon_interval: 100,
            capability_info: 0x0210,
        };
        beacon_data.extend_from_slice(&unsafe {
            core::slice::from_raw_parts(
                &beacon_body as *const BeaconBody as *const u8,
                core::mem::size_of::<BeaconBody>(),
            )
        });

        beacon_data.push(IE_SSID);
        beacon_data.push(7);
        beacon_data.extend_from_slice(b"echOS-L");
        beacon_data.push(IE_DS_PARAM);
        beacon_data.push(1);
        beacon_data.push(36);

        let bss = ctrl.process_beacon(&beacon_data, -50);
        assert!(bss.is_some());
        let bss = bss.unwrap();
        assert_eq!(bss.ssid, "echOS-L");
        assert_eq!(bss.channel, 36);
        assert_eq!(bss.band, WifiBand::Band5G);
    }

    #[test]
    fn wifi_jail_association_flow_open() {
        let ctrl = WifiJailController::new();
        ctrl.initialized.store(true, Ordering::SeqCst);
        ctrl.init_rings();

        let bss = WifiBss {
            bssid: [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0x01],
            ssid: String::from("OpenNet"),
            rssi: -60,
            channel: 6,
            frequency: 2437,
            band: WifiBand::Band2G,
            security: WifiSecurity::Open,
            phy_mode: WifiPhyMode::Dot11N,
            channel_width: 40,
        };

        assert!(ctrl.start_association(&bss, "").is_ok());
        assert_eq!(ctrl.get_state(), WifiState::Authenticating);
        assert!(ctrl
            .drive_assoc_fixture_flow(&bss, WifiSecurity::Open)
            .is_ok());
        assert_eq!(ctrl.get_state(), WifiState::Connected);
    }

    #[test]
    fn wifi_jail_association_flow_wpa2() {
        let ctrl = WifiJailController::new();
        ctrl.initialized.store(true, Ordering::SeqCst);
        ctrl.init_rings();

        let bss = WifiBss {
            bssid: [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0x02],
            ssid: String::from("SecureNet"),
            rssi: -45,
            channel: 36,
            frequency: 5180,
            band: WifiBand::Band5G,
            security: WifiSecurity::WPA2Personal,
            phy_mode: WifiPhyMode::Dot11AC,
            channel_width: 80,
        };

        assert!(ctrl.start_association(&bss, "password123").is_ok());
        let _ = ctrl.drive_assoc_fixture_flow(&bss, WifiSecurity::WPA2Personal);
        assert_eq!(ctrl.get_state(), WifiState::Connected);
        assert_eq!(ctrl.connected_ssid(), Some(String::from("SecureNet")));
    }

    #[test]
    fn wifi_jail_disconnect() {
        let ctrl = WifiJailController::new();
        ctrl.initialized.store(true, Ordering::SeqCst);
        ctrl.init_rings();

        let bss = WifiBss {
            bssid: [0xAA; 6],
            ssid: String::from("TestNet"),
            rssi: -55,
            channel: 11,
            frequency: 2462,
            band: WifiBand::Band2G,
            security: WifiSecurity::Open,
            phy_mode: WifiPhyMode::Dot11G,
            channel_width: 20,
        };

        let _ = ctrl.start_association(&bss, "");
        let _ = ctrl.drive_assoc_fixture_flow(&bss, WifiSecurity::Open);
        assert_eq!(ctrl.get_state(), WifiState::Connected);

        assert!(ctrl.disconnect().is_ok());
        assert_eq!(ctrl.get_state(), WifiState::Disconnected);
        assert_eq!(ctrl.connected_ssid(), None);
    }

    #[test]
    fn wifi_jail_tx_frame() {
        let ctrl = WifiJailController::new();
        ctrl.init_rings();

        let data = vec![0x01, 0x02, 0x03, 0x04];
        let result = ctrl.tx_frame(&data);
        assert!(result.is_ok());
        assert_eq!(ctrl.stats.lock().tx_packets, 1);
        assert_eq!(ctrl.stats.lock().tx_bytes, 4);
    }

    #[test]
    fn wifi_jail_mlo_planning() {
        let ctrl = WifiJailController::new();
        ctrl.init_rings();

        let results = vec![
            WifiBss {
                bssid: [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0x01],
                ssid: String::from("echOS-Lab"),
                rssi: -51,
                channel: 6,
                frequency: 2437,
                band: WifiBand::Band2G,
                security: WifiSecurity::WPA3Personal,
                phy_mode: WifiPhyMode::Dot11AX,
                channel_width: 40,
            },
            WifiBss {
                bssid: [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0x02],
                ssid: String::from("echOS-Lab"),
                rssi: -44,
                channel: 36,
                frequency: 5180,
                band: WifiBand::Band5G,
                security: WifiSecurity::WPA3Personal,
                phy_mode: WifiPhyMode::Dot11AX,
                channel_width: 160,
            },
        ];
        *ctrl.scan_results.lock() = results;

        let session = ctrl.plan_mlo_for_ssid("echOS-Lab", WifiSecurity::WPA3Personal);
        assert!(session.is_some());
        let session = session.unwrap();
        assert_eq!(session.ssid, "echOS-Lab");
        assert!(session.link_count() >= 2);
        assert!(session.aggregate_mbps > 0);
    }

    #[test]
    fn channel_to_frequency_conversion() {
        assert_eq!(channel_to_frequency(6, WifiBand::Band2G), 2437);
        assert_eq!(channel_to_frequency(36, WifiBand::Band5G), 5180);
        assert_eq!(channel_to_frequency(1, WifiBand::Band6G), 5955);
    }

    #[test]
    fn determine_phy_mode_from_ies() {
        let mut data = Vec::new();
        data.push(IE_HT_CAP);
        data.push(2);
        data.push(0x11);
        data.push(0x22);

        let ies = parse_ies(&data);
        let phy = determine_phy_mode(&ies);
        assert_eq!(phy, WifiPhyMode::Dot11N);
    }

    #[test]
    fn wifi_jail_process_command_scan() {
        let ctrl = WifiJailController::new();
        ctrl.initialized.store(true, Ordering::SeqCst);
        ctrl.init_rings();

        let response = ctrl.process_command(WifiJailCommand::Scan { passive: false });
        assert!(matches!(response, WifiJailResponse::ScanResults(_)));
    }

    #[test]
    fn wifi_jail_process_command_fw_load() {
        let ctrl = WifiJailController::new();
        let response = ctrl.process_command(WifiJailCommand::FirmwareLoad { version: 0x030000 });
        assert!(matches!(
            response,
            WifiJailResponse::Error(WifiError::FirmwareLoadFailed)
        ));
        assert_eq!(ctrl.firmware_state(), FirmwareState::NotLoaded);
    }

    #[test]
    fn wifi_jail_crash_and_reboot() {
        let ctrl = WifiJailController::new();
        ctrl.initialized.store(true, Ordering::SeqCst);
        ctrl.init_rings();

        let result = ctrl.crash_and_reboot();
        assert!(result.is_ok());
        assert_eq!(ctrl.crash_count.load(Ordering::Relaxed), 1);
        assert_eq!(ctrl.initialized.load(Ordering::Relaxed), true);
    }

    #[test]
    fn mac_header_addr4_length() {
        let mut header = MacHeader::new(
            FrameControl::new(FrameControl::FRAME_TYPE_DATA, FrameControl::SUBTYPE_DATA),
            [0x11; 6],
            [0x22; 6],
            [0x33; 6],
        );
        assert_eq!(header.header_len(), 24);

        header.frame_ctrl.raw |= 0x0300;
        assert_eq!(header.header_len(), 30);
    }
}
