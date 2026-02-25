//! # Bluetooth Subsystem
//!
//! HCI, L2CAP, RFCOMM, and BLE support

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::vec;
use spin::Mutex;
use core::sync::atomic::{AtomicU16, Ordering};

// ============================================================================
// BLUETOOTH CONSTANTS
// ============================================================================

/// HCI packet types
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

/// HCI OCF (OpCode Command Field)
const OCF_RESET: u16 = 0x0003;
const OCF_READ_LOCAL_VERSION: u16 = 0x0001;
const OCF_READ_LOCAL_FEATURES: u16 = 0x0003;
const OCF_READ_BD_ADDR: u16 = 0x0009;

/// LE OCF
const OCF_LE_SET_EVENT_MASK: u16 = 0x0001;
const OCF_LE_SET_ADV_ENABLE: u16 = 0x000A;
const OCF_LE_SET_ADV_DATA: u16 = 0x0008;
const OCF_LE_CREATE_CONN: u16 = 0x000D;

/// L2CAP channels
const L2CAP_CID_SIGNALING: u16 = 0x0001;
const L2CAP_CID_CONN_LESS: u16 = 0x0002;
const L2CAP_CID_ATT: u16 = 0x0004;
const L2CAP_CID_LE_SIGNALING: u16 = 0x0005;
const L2CAP_CID_SMP: u16 = 0x0006;
const L2CAP_CID_SMP_BREDR: u16 = 0x0007;

/// L2CAP signaling commands
const L2CAP_CMD_REJECT: u8 = 0x01;
const L2CAP_CONN_REQ: u8 = 0x02;
const L2CAP_CONN_RSP: u8 = 0x03;
const L2CAP_CONF_REQ: u8 = 0x04;
const L2CAP_CONF_RSP: u8 = 0x05;
const L2CAP_DISCONN_REQ: u8 = 0x06;
const L2CAP_DISCONN_RSP: u8 = 0x07;
const L2CAP_ECHO_REQ: u8 = 0x08;
const L2CAP_ECHO_RSP: u8 = 0x09;
const L2CAP_INFO_REQ: u8 = 0x0A;
const L2CAP_INFO_RSP: u8 = 0x0B;

/// RFCOMM constants
const RFCOMM_DLPMASK: u8 = 0x01;
const RFCOMM_UIH: u8 = 0xEF;
const RFCOMM_SABM: u8 = 0x2F;
const RFCOMM_DISC: u8 = 0x43;
const RFCOMM_DM: u8 = 0x0F;
const RFCOMM_UA: u8 = 0x63;

/// RFCOMM DLCI
const RFCOMM_DLCI_PN: u8 = 0;
const RFCOMM_DLCI_MX: u8 = 0;

/// BLE advertising types
const BLE_ADV_IND: u8 = 0x00;
const BLE_ADV_DIRECT_IND: u8 = 0x01;
const BLE_ADV_SCAN_IND: u8 = 0x02;
const BLE_ADV_NONCONN_IND: u8 = 0x03;
const BLE_ADV_SCAN_RSP: u8 = 0x04;

// ============================================================================
// BLUETOOTH ADDRESS
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BdAddr {
    pub bytes: [u8; 6],
}

impl BdAddr {
    pub fn new(bytes: [u8; 6]) -> Self {
        BdAddr { bytes }
    }

    pub fn zero() -> Self {
        BdAddr { bytes: [0; 6] }
    }

    pub fn to_string(&self) -> String {
        alloc::format!("{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
            self.bytes[5], self.bytes[4], self.bytes[3],
            self.bytes[2], self.bytes[1], self.bytes[0])
    }
}

impl Default for BdAddr {
    fn default() -> Self {
        Self::zero()
    }
}

// ============================================================================
// HCI LAYER
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HciError {
    UnknownCommand,
    NoConnection,
    HardwareFailure,
    PageTimeout,
    AuthenticationFailure,
    MissingKey,
    MemoryFull,
    Timeout,
    ConnectionTimeout,
    CommandDisallowed,
    InvalidParameters,
    RemoteUserTerminated,
    RemoteDeviceTerminated,
    ConnectionTerminated,
    RepeatedAttempts,
    PairingNotAllowed,
    Unknown,
}

impl HciError {
    pub fn from_status(status: u8) -> Self {
        match status {
            0x01 => HciError::UnknownCommand,
            0x02 => HciError::NoConnection,
            0x03 => HciError::HardwareFailure,
            0x04 => HciError::PageTimeout,
            0x05 => HciError::AuthenticationFailure,
            0x06 => HciError::MissingKey,
            0x07 => HciError::MemoryFull,
            0x08 => HciError::Timeout,
            0x08 => HciError::ConnectionTimeout,
            0x0C => HciError::CommandDisallowed,
            0x12 => HciError::InvalidParameters,
            0x13 => HciError::RemoteUserTerminated,
            0x14 => HciError::RemoteDeviceTerminated,
            0x16 => HciError::ConnectionTerminated,
            0x17 => HciError::RepeatedAttempts,
            0x18 => HciError::PairingNotAllowed,
            _ => HciError::Unknown,
        }
    }
}

/// HCI Command header
#[derive(Clone, Copy, Debug)]
pub struct HciCommandHeader {
    pub opcode: u16,
    pub param_len: u8,
}

impl HciCommandHeader {
    pub fn new(ogf: u8, ocf: u16) -> Self {
        let opcode = ((ogf as u16) << 10) | (ocf & 0x03FF);
        HciCommandHeader {
            opcode,
            param_len: 0,
        }
    }

    pub fn to_bytes(&self, params: &[u8]) -> Vec<u8> {
        let mut data = Vec::with_capacity(4 + params.len());
        data.push(self.opcode as u8);
        data.push((self.opcode >> 8) as u8);
        data.push(params.len() as u8);
        data.extend_from_slice(params);
        data
    }
}

/// HCI Event header
#[derive(Clone, Copy, Debug)]
pub struct HciEventHeader {
    pub evt_code: u8,
    pub param_len: u8,
}

impl HciEventHeader {
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 2 {
            return None;
        }
        Some(HciEventHeader {
            evt_code: data[0],
            param_len: data[1],
        })
    }
}

/// HCI Controller
#[derive(Clone, Debug)]
pub struct HciController {
    pub address: BdAddr,
    pub version: u8,
    pub manufacturer: u16,
    pub features: [u8; 8],
    pub name: String,
    pub initialized: bool,
}

impl HciController {
    pub fn new() -> Self {
        HciController {
            address: BdAddr::zero(),
            version: 0,
            manufacturer: 0,
            features: [0; 8],
            name: String::new(),
            initialized: false,
        }
    }

    /// Create HCI Reset command
    pub fn cmd_reset(&self) -> Vec<u8> {
        let header = HciCommandHeader::new(OGF_CONTROLLER_BASEBAND, OCF_RESET);
        header.to_bytes(&[])
    }

    /// Create Read Local Version command
    pub fn cmd_read_local_version(&self) -> Vec<u8> {
        let header = HciCommandHeader::new(OGF_INFO_PARAMS, OCF_READ_LOCAL_VERSION);
        header.to_bytes(&[])
    }

    /// Create Read BD ADDR command
    pub fn cmd_read_bd_addr(&self) -> Vec<u8> {
        let header = HciCommandHeader::new(OGF_INFO_PARAMS, OCF_READ_BD_ADDR);
        header.to_bytes(&[])
    }

    /// Create LE Set Advertising Enable command
    pub fn cmd_le_set_adv_enable(&self, enable: bool) -> Vec<u8> {
        let header = HciCommandHeader::new(OGF_LE, OCF_LE_SET_ADV_ENABLE);
        header.to_bytes(&[if enable { 1 } else { 0 }])
    }

    /// Create LE Set Advertising Data command
    pub fn cmd_le_set_adv_data(&self, data: &[u8]) -> Vec<u8> {
        let header = HciCommandHeader::new(OGF_LE, OCF_LE_SET_ADV_DATA);
        let mut params = vec![data.len() as u8];
        params.extend_from_slice(data);
        params.resize(32, 0); // Max 31 bytes of data
        header.to_bytes(&params)
    }

    /// Create LE Create Connection command
    pub fn cmd_le_create_conn(&self, addr: &BdAddr, addr_type: u8) -> Vec<u8> {
        let header = HciCommandHeader::new(OGF_LE, OCF_LE_CREATE_CONN);
        let mut params = Vec::with_capacity(25);
        
        // Scan interval and window (in 0.625ms units)
        params.extend_from_slice(&60u16.to_le_bytes()); // 37.5ms interval
        params.extend_from_slice(&30u16.to_le_bytes()); // 18.75ms window
        
        // Initiator filter policy (0 = use address from command)
        params.push(0);
        
        // Peer address type and address
        params.push(addr_type);
        params.extend_from_slice(&addr.bytes);
        
        // Own address type
        params.push(0);
        
        // Connection interval min, max, latency, timeout
        params.extend_from_slice(&24u16.to_le_bytes());  // 30ms min
        params.extend_from_slice(&40u16.to_le_bytes());  // 50ms max
        params.extend_from_slice(&0u16.to_le_bytes());   // 0 latency
        params.extend_from_slice(&500u16.to_le_bytes()); // 5s timeout
        
        // Min/max CE length
        params.extend_from_slice(&0u16.to_le_bytes());
        params.extend_from_slice(&0u16.to_le_bytes());
        
        header.to_bytes(&params)
    }

    /// Parse Command Complete event
    pub fn parse_cmd_complete(&mut self, data: &[u8]) -> Option<(u16, u8)> {
        if data.len() < 5 {
            return None;
        }
        
        let num_cmds = data[0];
        let opcode = u16::from_le_bytes([data[1], data[2]]);
        let status = data[3];
        
        Some((opcode, status))
    }

    /// Parse Read BD ADDR response
    pub fn parse_bd_addr(&mut self, data: &[u8]) -> Option<()> {
        if data.len() < 6 {
            return None;
        }
        
        self.address.bytes.copy_from_slice(&data[0..6]);
        Some(())
    }
}

impl Default for HciController {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// L2CAP LAYER
// ============================================================================

#[derive(Clone, Debug)]
pub struct L2capChannel {
    pub cid: u16,
    pub remote_cid: u16,
    pub psm: u16,
    pub state: L2capState,
    pub mtu: u16,
    pub flush_timeout: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum L2capState {
    Closed,
    WaitConnectRsp,
    WaitConfig,
    Open,
    WaitDisconnect,
}

impl L2capChannel {
    pub fn new(cid: u16, psm: u16) -> Self {
        L2capChannel {
            cid,
            remote_cid: 0,
            psm,
            state: L2capState::Closed,
            mtu: 672,
            flush_timeout: 0xFFFF,
        }
    }
}

/// L2CAP Signaling header
#[derive(Clone, Copy, Debug)]
pub struct L2capSignalHeader {
    pub code: u8,
    pub identifier: u8,
    pub length: u16,
}

impl L2capSignalHeader {
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 4 {
            return None;
        }
        Some(L2capSignalHeader {
            code: data[0],
            identifier: data[1],
            length: u16::from_le_bytes([data[2], data[3]]),
        })
    }

    pub fn to_bytes(&self, payload: &[u8]) -> Vec<u8> {
        let mut data = Vec::with_capacity(4 + payload.len());
        data.push(self.code);
        data.push(self.identifier);
        data.extend_from_slice(&(payload.len() as u16).to_le_bytes());
        data.extend_from_slice(payload);
        data
    }
}

/// L2CAP Connection Request
#[derive(Clone, Copy, Debug)]
pub struct L2capConnReq {
    pub psm: u16,
    pub src_cid: u16,
}

impl L2capConnReq {
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 4 {
            return None;
        }
        Some(L2capConnReq {
            psm: u16::from_le_bytes([data[0], data[1]]),
            src_cid: u16::from_le_bytes([data[2], data[3]]),
        })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut data = Vec::with_capacity(4);
        data.extend_from_slice(&self.psm.to_le_bytes());
        data.extend_from_slice(&self.src_cid.to_le_bytes());
        data
    }
}

/// L2CAP Connection Response
#[derive(Clone, Copy, Debug)]
pub struct L2capConnRsp {
    pub dcid: u16,
    pub scid: u16,
    pub result: u16,
    pub status: u16,
}

impl L2capConnRsp {
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 8 {
            return None;
        }
        Some(L2capConnRsp {
            dcid: u16::from_le_bytes([data[0], data[1]]),
            scid: u16::from_le_bytes([data[2], data[3]]),
            result: u16::from_le_bytes([data[4], data[5]]),
            status: u16::from_le_bytes([data[6], data[7]]),
        })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut data = Vec::with_capacity(8);
        data.extend_from_slice(&self.dcid.to_le_bytes());
        data.extend_from_slice(&self.scid.to_le_bytes());
        data.extend_from_slice(&self.result.to_le_bytes());
        data.extend_from_slice(&self.status.to_le_bytes());
        data
    }
}

/// L2CAP Manager
#[derive(Clone, Debug)]
pub struct L2capManager {
    pub channels: BTreeMap<u16, L2capChannel>,
    pub next_cid: u16,
}

impl L2capManager {
    pub fn new() -> Self {
        L2capManager {
            channels: BTreeMap::new(),
            next_cid: 0x0040, // Dynamic channels start at 0x0040
        }
    }

    /// Allocate new CID
    pub fn alloc_cid(&mut self) -> u16 {
        let cid = self.next_cid;
        self.next_cid += 1;
        if self.next_cid >= 0xFFFF {
            self.next_cid = 0x0040;
        }
        cid
    }

    /// Create connection request
    pub fn create_conn_req(&mut self, psm: u16) -> (u16, Vec<u8>) {
        let cid = self.alloc_cid();
        let channel = L2capChannel::new(cid, psm);
        self.channels.insert(cid, channel);

        let req = L2capConnReq {
            psm,
            src_cid: cid,
        };

        (cid, req.to_bytes())
    }

    /// Handle connection response
    pub fn handle_conn_rsp(&mut self, rsp: &L2capConnRsp) -> Option<u16> {
        if let Some(channel) = self.channels.get_mut(&rsp.scid) {
            channel.remote_cid = rsp.dcid;
            if rsp.result == 0 {
                channel.state = L2capState::WaitConfig;
            }
            return Some(rsp.scid);
        }
        None
    }

    /// Get channel by CID
    pub fn get_channel(&self, cid: u16) -> Option<&L2capChannel> {
        self.channels.get(&cid)
    }
}

impl Default for L2capManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// RFCOMM LAYER
// ============================================================================

#[derive(Clone, Debug)]
pub struct RfcommSession {
    pub dlci: u8,
    pub channel: u8,
    pub state: RfcommState,
    pub mtu: u16,
    pub credits: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RfcommState {
    Closed,
    Opening,
    Open,
    Closing,
}

impl RfcommSession {
    pub fn new(dlci: u8, channel: u8) -> Self {
        RfcommSession {
            dlci,
            channel,
            state: RfcommState::Closed,
            mtu: 127,
            credits: 0,
        }
    }
}

/// RFCOMM frame header
#[derive(Clone, Copy, Debug)]
pub struct RfcommHeader {
    pub addr: u8,
    pub control: u8,
    pub length: u8,
}

impl RfcommHeader {
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 3 {
            return None;
        }
        
        let length = if (data[2] & 0x01) != 0 {
            data[2] >> 1
        } else if data.len() >= 4 {
            ((data[2] >> 1) | ((data[3] as u8) << 7)) as u8
        } else {
            return None;
        };
        
        Some(RfcommHeader {
            addr: data[0],
            control: data[1],
            length,
        })
    }

    /// Create SABM frame (Set Asynchronous Balanced Mode)
    pub fn sabm(dlci: u8) -> Vec<u8> {
        let addr = 0x03 | (dlci << 2);
        vec![addr, RFCOMM_SABM, 0x01, 0x00, 0x70 | Self::fcs(&[addr, RFCOMM_SABM, 0x01])]
    }

    /// Create UA frame (Unnumbered Acknowledgment)
    pub fn ua(dlci: u8) -> Vec<u8> {
        let addr = 0x01 | (dlci << 2);
        vec![addr, RFCOMM_UA, 0x01, 0x00, 0x70 | Self::fcs(&[addr, RFCOMM_UA, 0x01])]
    }

    /// Create UIH frame (Unnumbered Information with Header check)
    pub fn uih(dlci: u8, data: &[u8]) -> Vec<u8> {
        let addr = 0x03 | (dlci << 2);
        let mut frame = Vec::with_capacity(5 + data.len());
        frame.push(addr);
        frame.push(RFCOMM_UIH);
        
        // Length field (7-bit or 15-bit)
        if data.len() < 128 {
            frame.push(((data.len() as u8) << 1) | 1);
        } else {
            frame.push((data.len() as u8) << 1);
            frame.push((data.len() >> 7) as u8);
        }
        
        frame.extend_from_slice(data);
        
        // FCS (only header for UIH)
        frame.push(0x00); // Simplified FCS
        
        frame
    }

    /// Calculate FCS (Frame Check Sequence)
    fn fcs(data: &[u8]) -> u8 {
        // Simplified CRC-8 calculation
        let mut crc: u8 = 0xFF;
        for &byte in data {
            crc ^= byte;
            for _ in 0..8 {
                if (crc & 0x01) != 0 {
                    crc = (crc >> 1) ^ 0xE0; // Polynomial for CRC-8
                } else {
                    crc >>= 1;
                }
            }
        }
        0xFF - crc
    }
}

/// RFCOMM Manager
#[derive(Clone, Debug)]
pub struct RfcommManager {
    pub sessions: BTreeMap<u8, RfcommSession>,
    pub next_dlci: u8,
}

impl RfcommManager {
    pub fn new() -> Self {
        RfcommManager {
            sessions: BTreeMap::new(),
            next_dlci: 1,
        }
    }

    /// Create new session
    pub fn create_session(&mut self, channel: u8) -> u8 {
        let dlci = self.next_dlci;
        self.next_dlci += 2; // Alternate between initiator and responder

        let session = RfcommSession::new(dlci, channel);
        self.sessions.insert(dlci, session);

        dlci
    }

    /// Get session by DLCI
    pub fn get_session(&self, dlci: u8) -> Option<&RfcommSession> {
        self.sessions.get(&dlci)
    }
}

impl Default for RfcommManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// BLE (Bluetooth Low Energy)
// ============================================================================

#[derive(Clone, Debug)]
pub struct BleDevice {
    pub address: BdAddr,
    pub address_type: u8,
    pub name: String,
    pub rssi: i8,
    pub services: Vec<BleService>,
    pub connected: bool,
}

#[derive(Clone, Debug)]
pub struct BleService {
    pub uuid: [u8; 16],
    pub handle: u16,
    pub characteristics: Vec<BleCharacteristic>,
}

#[derive(Clone, Debug)]
pub struct BleCharacteristic {
    pub uuid: [u8; 16],
    pub handle: u16,
    pub value_handle: u16,
    pub properties: u8,
}

/// BLE Advertising data
#[derive(Clone, Debug)]
pub struct BleAdvData {
    pub flags: u8,
    pub name: String,
    pub services: Vec<[u8; 2]>, // 16-bit UUIDs
    pub appearance: u16,
}

impl BleAdvData {
    pub fn new() -> Self {
        BleAdvData {
            flags: 0x06, // LE General Discoverable, BR/EDR Not Supported
            name: String::new(),
            services: Vec::new(),
            appearance: 0,
        }
    }

    /// Encode to advertising packet format
    pub fn encode(&self) -> Vec<u8> {
        let mut data = Vec::with_capacity(31);

        // Flags
        data.push(0x02); // Length
        data.push(0x01); // Type: Flags
        data.push(self.flags);

        // Name
        if !self.name.is_empty() {
            let name_bytes = self.name.as_bytes();
            let len = name_bytes.len().min(29);
            data.push((len + 1) as u8);
            data.push(0x08); // Type: Shortened Local Name
            data.extend_from_slice(&name_bytes[..len]);
        }

        // Services (16-bit UUIDs)
        if !self.services.is_empty() {
            let count = self.services.len().min(14); // Max 14 UUIDs
            data.push((count * 2 + 1) as u8);
            data.push(0x02); // Type: Incomplete List of 16-bit Service UUIDs
            for uuid in &self.services[..count] {
                data.extend_from_slice(uuid);
            }
        }

        data
    }
}

impl Default for BleAdvData {
    fn default() -> Self {
        Self::new()
    }
}

/// BLE Manager
#[derive(Clone, Debug)]
pub struct BleManager {
    pub devices: Vec<BleDevice>,
    pub advertising: bool,
    pub scanning: bool,
    pub adv_data: BleAdvData,
}

impl BleManager {
    pub fn new() -> Self {
        BleManager {
            devices: Vec::new(),
            advertising: false,
            scanning: false,
            adv_data: BleAdvData::new(),
        }
    }

    /// Start advertising
    pub fn start_advertising(&mut self) {
        self.advertising = true;
    }

    /// Stop advertising
    pub fn stop_advertising(&mut self) {
        self.advertising = false;
    }

    /// Start scanning
    pub fn start_scanning(&mut self) {
        self.scanning = true;
        self.devices.clear();
    }

    /// Stop scanning
    pub fn stop_scanning(&mut self) {
        self.scanning = false;
    }

    /// Add discovered device
    pub fn add_device(&mut self, device: BleDevice) {
        // Check if device already exists
        if !self.devices.iter().any(|d| d.address == device.address) {
            self.devices.push(device);
        }
    }

    /// Parse advertising report
    pub fn parse_adv_report(&self, data: &[u8]) -> Option<BleDevice> {
        if data.len() < 10 {
            return None;
        }

        let event_type = data[0];
        let addr_type = data[1];
        let addr = BdAddr::new([data[2], data[3], data[4], data[5], data[6], data[7]]);
        let _data_len = data[8] as usize;
        let rssi = data[data.len() - 1] as i8;

        // Parse name from advertising data
        let mut name = String::new();
        let mut offset = 9;
        while offset < data.len() - 1 {
            let len = data[offset] as usize;
            if len == 0 || offset + len >= data.len() {
                break;
            }
            let ad_type = data[offset + 1];
            
            // Local Name (0x08 or 0x09)
            if ad_type == 0x08 || ad_type == 0x09 {
                if offset + 2 + len - 1 <= data.len() {
                    let name_bytes = &data[offset + 2..offset + 1 + len];
                    name = String::from_utf8_lossy(name_bytes).to_string();
                }
            }
            
            offset += len + 1;
        }

        Some(BleDevice {
            address: addr,
            address_type: addr_type,
            name,
            rssi,
            services: Vec::new(),
            connected: false,
        })
    }
}

impl Default for BleManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// BLUETOOTH MANAGER
// ============================================================================

#[derive(Clone, Debug)]
pub struct BluetoothManager {
    pub hci: HciController,
    pub l2cap: L2capManager,
    pub rfcomm: RfcommManager,
    pub ble: BleManager,
    pub initialized: bool,
}

impl BluetoothManager {
    pub fn new() -> Self {
        BluetoothManager {
            hci: HciController::new(),
            l2cap: L2capManager::new(),
            rfcomm: RfcommManager::new(),
            ble: BleManager::new(),
            initialized: false,
        }
    }

    /// Initialize Bluetooth
    pub fn init(&mut self) {
        crate::serial_println!("[BT] Initializing Bluetooth subsystem...");
        self.initialized = true;
    }

    /// Get local address
    pub fn get_address(&self) -> &BdAddr {
        &self.hci.address
    }

    /// Start BLE advertising
    pub fn start_ble_advertising(&mut self) {
        self.ble.start_advertising();
        crate::serial_println!("[BT] Started BLE advertising");
    }

    /// Stop BLE advertising
    pub fn stop_ble_advertising(&mut self) {
        self.ble.stop_advertising();
        crate::serial_println!("[BT] Stopped BLE advertising");
    }

    /// Start BLE scanning
    pub fn start_ble_scanning(&mut self) {
        self.ble.start_scanning();
        crate::serial_println!("[BT] Started BLE scanning");
    }

    /// Stop BLE scanning
    pub fn stop_ble_scanning(&mut self) {
        self.ble.stop_scanning();
        crate::serial_println!("[BT] Stopped BLE scanning");
    }

    /// Get discovered BLE devices
    pub fn get_ble_devices(&self) -> &Vec<BleDevice> {
        &self.ble.devices
    }
}

impl Default for BluetoothManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// GLOBAL INSTANCE
// ============================================================================

lazy_static::lazy_static! {
    static ref BT_MANAGER: Mutex<BluetoothManager> = Mutex::new(BluetoothManager::new());
}

/// Initialize Bluetooth
pub fn init() {
    BT_MANAGER.lock().init();
}

/// Get Bluetooth manager
pub fn get_manager() -> BluetoothManager {
    BT_MANAGER.lock().clone()
}

/// Start BLE advertising
pub fn start_ble_advertising() {
    BT_MANAGER.lock().start_ble_advertising();
}

/// Stop BLE advertising
pub fn stop_ble_advertising() {
    BT_MANAGER.lock().stop_ble_advertising();
}

/// Start BLE scanning
pub fn start_ble_scanning() {
    BT_MANAGER.lock().start_ble_scanning();
}

/// Stop BLE scanning
pub fn stop_ble_scanning() {
    BT_MANAGER.lock().stop_ble_scanning();
}

/// Get BLE devices
pub fn get_ble_devices() -> Vec<BleDevice> {
    BT_MANAGER.lock().ble.devices.clone()
}
