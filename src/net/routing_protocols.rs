use alloc::vec;
use alloc::vec::Vec;

const BGP_VERSION: u8 = 4;
const BGP_DEFAULT_HOLD_TIME: u16 = 90;
const OSPF_VERSION: u8 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BgpState {
    Idle,
    Connect,
    Active,
    OpenSent,
    OpenConfirm,
    Established,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BgpEventType {
    ManualStart,
    ManualStop,
    TcpConnectionValid,
    TcpConnectionFails,
    TcpConvEstablished,
    TcpConnFails,
    HoldTimerExpires,
    KeepAliveTimerExpires,
    ReceiveOpen,
    ReceiveKeepalive,
    ReceiveUpdate,
    ReceiveNotification,
}

#[derive(Clone, Debug)]
pub enum BgpMessage {
    Open(BgpOpen),
    Update(BgpUpdate),
    Notification(BgpNotification),
    Keepalive,
}

#[derive(Clone, Debug)]
pub struct BgpOpen {
    pub version: u8,
    pub my_as: u16,
    pub hold_time: u16,
    pub bgp_id: u32,
    pub opt_params: Vec<BgpOptParam>,
}

impl BgpOpen {
    pub fn new(my_as: u16, hold_time: u16, bgp_id: u32) -> Self {
        BgpOpen {
            version: BGP_VERSION,
            my_as,
            hold_time,
            bgp_id,
            opt_params: Vec::new(),
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(29);
        buf.push(self.version);
        buf.extend_from_slice(&self.my_as.to_be_bytes());
        buf.extend_from_slice(&self.hold_time.to_be_bytes());
        buf.extend_from_slice(&self.bgp_id.to_be_bytes());
        for param in &self.opt_params {
            buf.push(param.param_type);
            buf.push(param.param_data.len() as u8);
            buf.extend_from_slice(&param.param_data);
        }
        buf
    }

    pub fn decode(data: &[u8]) -> Option<Self> {
        if data.len() < 9 {
            return None;
        }
        let version = data[0];
        let my_as = u16::from_be_bytes([data[1], data[2]]);
        let hold_time = u16::from_be_bytes([data[3], data[4]]);
        let bgp_id = u32::from_be_bytes([data[5], data[6], data[7], data[8]]);
        let mut opt_params = Vec::new();
        let mut pos = 10;
        while pos + 1 < data.len() {
            let param_type = data[pos];
            let param_len = data[pos + 1] as usize;
            pos += 2;
            if pos + param_len > data.len() {
                break;
            }
            opt_params.push(BgpOptParam {
                param_type,
                param_data: data[pos..pos + param_len].to_vec(),
            });
            pos += param_len;
        }
        Some(BgpOpen {
            version,
            my_as,
            hold_time,
            bgp_id,
            opt_params,
        })
    }
}

#[derive(Clone, Debug)]
pub struct BgpOptParam {
    pub param_type: u8,
    pub param_data: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct BgpUpdate {
    pub withdrawn_routes: Vec<BgpRoute>,
    pub path_attrs: Vec<BgpPathAttr>,
    pub nlri: Vec<BgpRoute>,
}

impl BgpUpdate {
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        let mut withdrawn_len: u16 = 0;
        for route in &self.withdrawn_routes {
            withdrawn_len += 1 + (((route.prefix_len + 7) / 8) as u16);
        }
        buf.extend_from_slice(&withdrawn_len.to_be_bytes());
        for route in &self.withdrawn_routes {
            buf.push(route.prefix_len);
            let nbytes = ((route.prefix_len + 7) / 8) as usize;
            buf.extend_from_slice(&route.prefix.to_be_bytes()[4 - nbytes..]);
        }
        let mut attr_len: u16 = 0;
        for attr in &self.path_attrs {
            attr_len += attr.encoded_len();
        }
        buf.extend_from_slice(&attr_len.to_be_bytes());
        for attr in &self.path_attrs {
            attr.encode(&mut buf);
        }
        for route in &self.nlri {
            buf.push(route.prefix_len);
            let nbytes = ((route.prefix_len + 7) / 8) as usize;
            buf.extend_from_slice(&route.prefix.to_be_bytes()[4 - nbytes..]);
        }
        buf
    }

    pub fn decode(data: &[u8]) -> Option<Self> {
        if data.len() < 4 {
            return None;
        }
        let withdrawn_len = u16::from_be_bytes([data[0], data[1]]) as usize;
        let mut pos = 2;
        let mut withdrawn_routes = Vec::new();
        let end = (pos + withdrawn_len).min(data.len());
        while pos < end {
            if pos >= data.len() {
                break;
            }
            let prefix_len = data[pos];
            pos += 1;
            let nbytes = ((prefix_len + 7) / 8) as usize;
            if pos + nbytes > data.len() {
                break;
            }
            let mut prefix_bytes = [0u8; 4];
            prefix_bytes[4 - nbytes..].copy_from_slice(&data[pos..pos + nbytes]);
            withdrawn_routes.push(BgpRoute {
                prefix: u32::from_be_bytes(prefix_bytes),
                prefix_len,
            });
            pos += nbytes;
        }
        if pos + 2 > data.len() {
            return None;
        }
        let attr_len = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
        pos += 2;
        let mut path_attrs = Vec::new();
        let attr_end = (pos + attr_len).min(data.len());
        let mut combined = BgpPathAttr::new();
        let mut has_combined = false;
        while pos < attr_end {
            if pos + 2 > data.len() {
                break;
            }
            let flags = data[pos];
            let attr_type = data[pos + 1];
            pos += 2;
            let (attr_len_val, new_pos) = if flags & 0x10 != 0 {
                if pos + 2 > data.len() {
                    break;
                }
                (u16::from_be_bytes([data[pos], data[pos + 1]]) as usize, pos + 2)
            } else {
                if pos >= data.len() {
                    break;
                }
                (data[pos] as usize, pos + 1)
            };
            pos = new_pos;
            if pos + attr_len_val > data.len() {
                break;
            }
            if let Some(attr) = BgpPathAttr::decode(attr_type, &data[pos..pos + attr_len_val]) {
                combined.merge(&attr);
                has_combined = true;
            }
            pos += attr_len_val;
        }
        if has_combined {
            path_attrs.push(combined);
        }
        let mut nlri = Vec::new();
        while pos < data.len() {
            let prefix_len = data[pos];
            pos += 1;
            let nbytes = ((prefix_len + 7) / 8) as usize;
            if pos + nbytes > data.len() {
                break;
            }
            let mut prefix_bytes = [0u8; 4];
            prefix_bytes[4 - nbytes..].copy_from_slice(&data[pos..pos + nbytes]);
            nlri.push(BgpRoute {
                prefix: u32::from_be_bytes(prefix_bytes),
                prefix_len,
            });
            pos += nbytes;
        }
        Some(BgpUpdate {
            withdrawn_routes,
            path_attrs,
            nlri,
        })
    }
}

#[derive(Clone, Debug)]
pub struct BgpRoute {
    pub prefix: u32,
    pub prefix_len: u8,
}

#[derive(Clone, Debug)]
pub struct BgpPathAttr {
    pub origin: u8,
    pub as_path: Vec<u16>,
    pub next_hop: u32,
    pub local_pref: u32,
    pub med: u32,
}

impl BgpPathAttr {
    pub fn new() -> Self {
        BgpPathAttr {
            origin: 0,
            as_path: Vec::new(),
            next_hop: 0,
            local_pref: 100,
            med: 0,
        }
    }

    pub fn encode(&self, buf: &mut Vec<u8>) {
        buf.push(0x40);
        buf.push(1);
        buf.push(1);
        buf.push(self.origin);
        buf.push(0x40);
        buf.push(2);
        let path_len = (self.as_path.len() * 2) as u8;
        buf.push(path_len + 2);
        buf.push(0x01);
        buf.push(path_len);
        for asn in &self.as_path {
            buf.extend_from_slice(&asn.to_be_bytes());
        }
        buf.push(0x40);
        buf.push(3);
        buf.push(4);
        buf.extend_from_slice(&self.next_hop.to_be_bytes());
        buf.push(0x40);
        buf.push(5);
        buf.push(4);
        buf.extend_from_slice(&self.local_pref.to_be_bytes());
        buf.push(0x40);
        buf.push(8);
        buf.push(4);
        buf.extend_from_slice(&self.med.to_be_bytes());
    }

    pub fn encoded_len(&self) -> u16 {
        let mut len: u16 = 2 + 1 + 1;
        len += 2 + 1 + 2 + (self.as_path.len() as u16 * 2);
        len += 2 + 1 + 4;
        len += 2 + 1 + 4;
        len += 2 + 1 + 4;
        len
    }

    pub fn decode(attr_type: u8, data: &[u8]) -> Option<Self> {
        let mut attr = BgpPathAttr::new();
        match attr_type {
            1 => {
                if !data.is_empty() {
                    attr.origin = data[0];
                }
            }
            2 => {
                let mut pos = 0;
                while pos + 1 < data.len() {
                    let _seg_type = data[pos];
                    let seg_len = data[pos + 1] as usize;
                    pos += 2;
                    let count = seg_len / 2;
                    for _ in 0..count {
                        if pos + 2 > data.len() {
                            break;
                        }
                        attr.as_path.push(u16::from_be_bytes([
                            data[pos],
                            data[pos + 1],
                        ]));
                        pos += 2;
                    }
                }
            }
            3 => {
                if data.len() >= 4 {
                    attr.next_hop = u32::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                    ]);
                }
            }
            5 => {
                if data.len() >= 4 {
                    attr.local_pref = u32::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                    ]);
                }
            }
            8 => {
                if data.len() >= 4 {
                    attr.med = u32::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                    ]);
                }
            }
            _ => {
                return None;
            }
        }
        Some(attr)
    }

    pub fn merge(&mut self, other: &BgpPathAttr) {
        if other.origin != 0 {
            self.origin = other.origin;
        }
        if !other.as_path.is_empty() {
            self.as_path = other.as_path.clone();
        }
        if other.next_hop != 0 {
            self.next_hop = other.next_hop;
        }
        if other.local_pref != 100 {
            self.local_pref = other.local_pref;
        }
        if other.med != 0 {
            self.med = other.med;
        }
    }
}

#[derive(Clone, Debug)]
pub struct BgpNotification {
    pub error_code: u8,
    pub error_subcode: u8,
    pub data: Vec<u8>,
}

impl BgpNotification {
    pub fn new(error_code: u8, error_subcode: u8) -> Self {
        BgpNotification {
            error_code,
            error_subcode,
            data: Vec::new(),
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(2 + self.data.len());
        buf.push(self.error_code);
        buf.push(self.error_subcode);
        buf.extend_from_slice(&self.data);
        buf
    }

    pub fn decode(data: &[u8]) -> Option<Self> {
        if data.len() < 2 {
            return None;
        }
        Some(BgpNotification {
            error_code: data[0],
            error_subcode: data[1],
            data: data[2..].to_vec(),
        })
    }
}

pub const BGP_ERR_MSG_HEADER: u8 = 1;
pub const BGP_ERR_OPEN: u8 = 2;
pub const BGP_ERR_UPDATE: u8 = 3;
pub const BGP_ERR_HOLD_TIMER: u8 = 4;
pub const BGP_ERR_FSM: u8 = 5;
pub const BGP_ERR_CEASE: u8 = 6;

pub fn bgp_state_machine(state: BgpState, event: BgpEventType) -> BgpState {
    match (state, event) {
        (BgpState::Idle, BgpEventType::ManualStart) => BgpState::Connect,
        (BgpState::Connect, BgpEventType::TcpConvEstablished) => {
            BgpState::Active
        }
        (BgpState::Connect, BgpEventType::TcpConnectionFails) => BgpState::Idle,
        (BgpState::Connect, BgpEventType::ManualStop) => BgpState::Idle,
        (BgpState::Active, BgpEventType::TcpConvEstablished) => {
            BgpState::Active
        }
        (BgpState::Active, BgpEventType::TcpConnectionFails) => BgpState::Idle,
        (BgpState::Active, BgpEventType::ManualStop) => BgpState::Idle,
        (BgpState::Active, BgpEventType::ReceiveOpen) => BgpState::OpenSent,
        (BgpState::OpenSent, BgpEventType::ReceiveOpen) => BgpState::OpenConfirm,
        (BgpState::OpenSent, BgpEventType::HoldTimerExpires) => {
            BgpState::Idle
        }
        (BgpState::OpenSent, BgpEventType::ReceiveNotification) => {
            BgpState::Idle
        }
        (BgpState::OpenSent, BgpEventType::ManualStop) => BgpState::Idle,
        (BgpState::OpenConfirm, BgpEventType::ReceiveKeepalive) => {
            BgpState::Established
        }
        (BgpState::OpenConfirm, BgpEventType::HoldTimerExpires) => {
            BgpState::Idle
        }
        (BgpState::OpenConfirm, BgpEventType::ReceiveNotification) => {
            BgpState::Idle
        }
        (BgpState::Established, BgpEventType::ReceiveUpdate) => {
            BgpState::Established
        }
        (BgpState::Established, BgpEventType::ReceiveKeepalive) => {
            BgpState::Established
        }
        (BgpState::Established, BgpEventType::HoldTimerExpires) => {
            BgpState::Idle
        }
        (BgpState::Established, BgpEventType::ReceiveNotification) => {
            BgpState::Idle
        }
        (BgpState::Established, BgpEventType::ManualStop) => {
            BgpState::Idle
        }
        (BgpState::Established, BgpEventType::TcpConnectionFails) => {
            BgpState::Idle
        }
        _ => state,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OspfState {
    Down,
    Init,
    TwoWay,
    ExStart,
    Exchange,
    Loading,
    Full,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OspfPacketType {
    Hello = 1,
    DatabaseDescription = 2,
    LinkStateRequest = 3,
    LinkStateUpdate = 4,
    LinkStateAck = 5,
}

#[derive(Clone, Debug)]
pub struct OspfHeader {
    pub version: u8,
    pub packet_type: u8,
    pub length: u16,
    pub router_id: u32,
    pub area_id: u32,
    pub checksum: u16,
    pub auth_type: u16,
}

impl OspfHeader {
    pub fn new(packet_type: u8, router_id: u32, area_id: u32) -> Self {
        OspfHeader {
            version: OSPF_VERSION,
            packet_type,
            length: 0,
            router_id,
            area_id,
            checksum: 0,
            auth_type: 0,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(24);
        buf.push(self.version);
        buf.push(self.packet_type);
        buf.extend_from_slice(&self.length.to_be_bytes());
        buf.extend_from_slice(&self.router_id.to_be_bytes());
        buf.extend_from_slice(&self.area_id.to_be_bytes());
        buf.extend_from_slice(&self.checksum.to_be_bytes());
        buf.extend_from_slice(&self.auth_type.to_be_bytes());
        buf.extend_from_slice(&[0u8; 8]); // auth data
        buf
    }

    pub fn decode(data: &[u8]) -> Option<Self> {
        if data.len() < 24 {
            return None;
        }
        Some(OspfHeader {
            version: data[0],
            packet_type: data[1],
            length: u16::from_be_bytes([data[2], data[3]]),
            router_id: u32::from_be_bytes([data[4], data[5], data[6], data[7]]),
            area_id: u32::from_be_bytes([data[8], data[9], data[10], data[11]]),
            checksum: u16::from_be_bytes([data[12], data[13]]),
            auth_type: u16::from_be_bytes([data[14], data[15]]),
        })
    }
}

#[derive(Clone, Debug)]
pub struct OspfHello {
    pub network_mask: u32,
    pub hello_interval: u16,
    pub options: u8,
    pub router_priority: u8,
    pub dead_interval: u32,
    pub designated_router: u32,
    pub backup_designated_router: u32,
    pub neighbors: Vec<u32>,
}

impl OspfHello {
    pub fn new(
        network_mask: u32,
        hello_interval: u16,
        dead_interval: u32,
        router_id: u32,
    ) -> Self {
        OspfHello {
            network_mask,
            hello_interval,
            options: 0x02,
            router_priority: 1,
            dead_interval,
            designated_router: 0,
            backup_designated_router: 0,
            neighbors: vec![router_id],
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(20 + self.neighbors.len() * 4);
        buf.extend_from_slice(&self.network_mask.to_be_bytes());
        buf.extend_from_slice(&self.hello_interval.to_be_bytes());
        buf.push(self.options);
        buf.push(self.router_priority);
        buf.extend_from_slice(&self.dead_interval.to_be_bytes());
        buf.extend_from_slice(&self.designated_router.to_be_bytes());
        buf.extend_from_slice(&self.backup_designated_router.to_be_bytes());
        for neighbor in &self.neighbors {
            buf.extend_from_slice(&neighbor.to_be_bytes());
        }
        buf
    }

    pub fn decode(data: &[u8]) -> Option<Self> {
        if data.len() < 20 {
            return None;
        }
        let network_mask = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
        let hello_interval = u16::from_be_bytes([data[4], data[5]]);
        let options = data[6];
        let router_priority = data[7];
        let dead_interval = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
        let designated_router = u32::from_be_bytes([data[12], data[13], data[14], data[15]]);
        let backup_designated_router =
            u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
        let mut neighbors = Vec::new();
        let mut pos = 20;
        while pos + 4 <= data.len() {
            neighbors.push(u32::from_be_bytes([
                data[pos], data[pos + 1], data[pos + 2], data[pos + 3],
            ]));
            pos += 4;
        }
        Some(OspfHello {
            network_mask,
            hello_interval,
            options,
            router_priority,
            dead_interval,
            designated_router,
            backup_designated_router,
            neighbors,
        })
    }
}

#[derive(Clone, Debug)]
pub struct OspfDatabaseDescription {
    pub interface_mtu: u16,
    pub options: u8,
    pub flags: u8,
    pub dd_sequence: u32,
    pub lsa_headers: Vec<OspfLsaHeader>,
}

impl OspfDatabaseDescription {
    pub fn new(dd_sequence: u32) -> Self {
        OspfDatabaseDescription {
            interface_mtu: 1500,
            options: 0x02,
            flags: 0x07,
            dd_sequence,
            lsa_headers: Vec::new(),
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(8 + self.lsa_headers.len() * 20);
        buf.extend_from_slice(&self.interface_mtu.to_be_bytes());
        buf.push(self.options);
        buf.push(self.flags);
        buf.extend_from_slice(&self.dd_sequence.to_be_bytes());
        for header in &self.lsa_headers {
            header.encode(&mut buf);
        }
        buf
    }

    pub fn decode(data: &[u8]) -> Option<Self> {
        if data.len() < 8 {
            return None;
        }
        let interface_mtu = u16::from_be_bytes([data[0], data[1]]);
        let options = data[2];
        let flags = data[3];
        let dd_sequence = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
        let mut lsa_headers = Vec::new();
        let mut pos = 8;
        while pos + 20 <= data.len() {
            if let Some(h) = OspfLsaHeader::decode(&data[pos..pos + 20]) {
                lsa_headers.push(h);
            }
            pos += 20;
        }
        Some(OspfDatabaseDescription {
            interface_mtu,
            options,
            flags,
            dd_sequence,
            lsa_headers,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OspfLsaType {
    Router = 1,
    Network = 2,
    Summary = 3,
    ASExternal = 5,
}

#[derive(Clone, Debug)]
pub struct OspfLsaHeader {
    pub age: u16,
    pub options: u8,
    pub lsa_type: u8,
    pub id: u32,
    pub adv_router: u32,
    pub sequence_num: u32,
}

impl OspfLsaHeader {
    pub fn new(lsa_type: u8, id: u32, adv_router: u32) -> Self {
        OspfLsaHeader {
            age: 0,
            options: 0x02,
            lsa_type,
            id,
            adv_router,
            sequence_num: 0x80000001,
        }
    }

    pub fn encode(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.age.to_be_bytes());
        buf.push(self.options);
        buf.push(self.lsa_type);
        buf.extend_from_slice(&self.id.to_be_bytes());
        buf.extend_from_slice(&self.adv_router.to_be_bytes());
        buf.extend_from_slice(&self.sequence_num.to_be_bytes());
        buf.extend_from_slice(&[0, 0, 0, 0]);
    }

    pub fn decode(data: &[u8]) -> Option<Self> {
        if data.len() < 20 {
            return None;
        }
        Some(OspfLsaHeader {
            age: u16::from_be_bytes([data[0], data[1]]),
            options: data[2],
            lsa_type: data[3],
            id: u32::from_be_bytes([data[4], data[5], data[6], data[7]]),
            adv_router: u32::from_be_bytes([data[8], data[9], data[10], data[11]]),
            sequence_num: u32::from_be_bytes([data[12], data[13], data[14], data[15]]),
        })
    }
}

#[derive(Clone, Debug)]
pub struct OspfLsa {
    pub age: u16,
    pub options: u8,
    pub lsa_type: u8,
    pub id: u32,
    pub adv_router: u32,
    pub sequence_num: u32,
    pub data: Vec<u8>,
}

impl OspfLsa {
    pub fn new(lsa_type: u8, id: u32, adv_router: u32) -> Self {
        OspfLsa {
            age: 0,
            options: 0x02,
            lsa_type,
            id,
            adv_router,
            sequence_num: 0x80000001,
            data: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct OspfLinkStateRequest {
    pub lsa_type: u32,
    pub link_state_id: u32,
    pub adv_router: u32,
}

#[derive(Clone, Debug)]
pub struct OspfLinkStateUpdate {
    pub lsa_count: u32,
    pub lsas: Vec<OspfLsa>,
}

#[derive(Clone, Debug)]
pub struct OspfLinkStateAck {
    pub lsa_headers: Vec<OspfLsaHeader>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OspfNeighborEvent {
    HelloReceived,
    TwoWayReceived,
    NegotiationDone,
    ExchangeDone,
    BadLSA,
    BadDatabaseSummary,
    BadLSRequest,
    LSALoadDone,
    NeighborDown,
    InactivityTimer,
}

pub const OSPF_HELLO_INTERVAL: u16 = 10;
pub const OSPF_DEAD_INTERVAL: u32 = 40;
pub const OSPF_WAIT_INTERVAL: u32 = 40;
pub const OSPF_RXMT_INTERVAL: u32 = 5;
pub const OSPF_INFTRANS_DELAY: u32 = 1;

pub fn ospf_neighbor_state_machine(state: OspfState, event: OspfNeighborEvent) -> OspfState {
    match (state, event) {
        (OspfState::Down, OspfNeighborEvent::HelloReceived) => OspfState::Init,
        (OspfState::Init, OspfNeighborEvent::TwoWayReceived) => OspfState::TwoWay,
        (OspfState::TwoWay, OspfNeighborEvent::NegotiationDone) => {
            OspfState::ExStart
        }
        (OspfState::ExStart, OspfNeighborEvent::ExchangeDone) => {
            OspfState::Exchange
        }
        (OspfState::Exchange, OspfNeighborEvent::LSALoadDone) => {
            OspfState::Loading
        }
        (OspfState::Loading, OspfNeighborEvent::LSALoadDone) => OspfState::Full,
        (OspfState::Full, OspfNeighborEvent::BadLSA) => OspfState::Exchange,
        (OspfState::Full, OspfNeighborEvent::BadDatabaseSummary) => {
            OspfState::Exchange
        }
        (OspfState::Full, OspfNeighborEvent::BadLSRequest) => OspfState::Loading,
        (OspfState::Full, OspfNeighborEvent::NeighborDown) => OspfState::Down,
        (OspfState::Full, OspfNeighborEvent::InactivityTimer) => OspfState::Down,
        (OspfState::Exchange, OspfNeighborEvent::NeighborDown) => OspfState::Down,
        (OspfState::Exchange, OspfNeighborEvent::BadLSA) => OspfState::Down,
        (OspfState::Loading, OspfNeighborEvent::NeighborDown) => OspfState::Down,
        (OspfState::ExStart, OspfNeighborEvent::NeighborDown) => OspfState::Down,
        (OspfState::TwoWay, OspfNeighborEvent::NeighborDown) => OspfState::Down,
        (OspfState::Init, OspfNeighborEvent::NeighborDown) => OspfState::Down,
        _ => state,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bgp_open_encode_decode() {
        let open = BgpOpen::new(65000, 90, 0x01020304);
        let encoded = open.encode();
        assert_eq!(encoded[0], 4);
        assert_eq!(u16::from_be_bytes([encoded[1], encoded[2]]), 65000);
        let decoded = BgpOpen::decode(&encoded).unwrap();
        assert_eq!(decoded.version, 4);
        assert_eq!(decoded.my_as, 65000);
        assert_eq!(decoded.hold_time, 90);
        assert_eq!(decoded.bgp_id, 0x01020304);
    }

    #[test]
    fn test_bgp_state_machine() {
        assert_eq!(
            bgp_state_machine(BgpState::Idle, BgpEventType::ManualStart),
            BgpState::Connect
        );
        assert_eq!(
            bgp_state_machine(BgpState::Connect, BgpEventType::TcpConvEstablished),
            BgpState::Active
        );
        assert_eq!(
            bgp_state_machine(BgpState::Active, BgpEventType::ReceiveOpen),
            BgpState::OpenSent
        );
        assert_eq!(
            bgp_state_machine(BgpState::OpenSent, BgpEventType::ReceiveOpen),
            BgpState::OpenConfirm
        );
        assert_eq!(
            bgp_state_machine(
                BgpState::OpenConfirm,
                BgpEventType::ReceiveKeepalive
            ),
            BgpState::Established
        );
    }

    #[test]
    fn test_bgp_notification_encode_decode() {
        let notif = BgpNotification::new(BGP_ERR_HOLD_TIMER, 0);
        let encoded = notif.encode();
        let decoded = BgpNotification::decode(&encoded).unwrap();
        assert_eq!(decoded.error_code, BGP_ERR_HOLD_TIMER);
        assert_eq!(decoded.error_subcode, 0);
    }

    #[test]
    fn test_ospf_header_encode_decode() {
        let header = OspfHeader::new(1, 0x01020304, 0x0A0B0C0D);
        let encoded = header.encode();
        assert_eq!(encoded.len(), 24);
        let decoded = OspfHeader::decode(&encoded).unwrap();
        assert_eq!(decoded.version, 2);
        assert_eq!(decoded.packet_type, 1);
        assert_eq!(decoded.router_id, 0x01020304);
        assert_eq!(decoded.area_id, 0x0A0B0C0D);
    }

    #[test]
    fn test_ospf_hello_encode_decode() {
        let hello = OspfHello::new(0xFFFFFF00, 10, 40, 0x01020304);
        let encoded = hello.encode();
        let decoded = OspfHello::decode(&encoded).unwrap();
        assert_eq!(decoded.network_mask, 0xFFFFFF00);
        assert_eq!(decoded.hello_interval, 10);
        assert_eq!(decoded.dead_interval, 40);
        assert_eq!(decoded.neighbors.len(), 1);
        assert_eq!(decoded.neighbors[0], 0x01020304);
    }

    #[test]
    fn test_ospf_neighbor_state_machine() {
        assert_eq!(
            ospf_neighbor_state_machine(OspfState::Down, OspfNeighborEvent::HelloReceived),
            OspfState::Init
        );
        assert_eq!(
            ospf_neighbor_state_machine(OspfState::Init, OspfNeighborEvent::TwoWayReceived),
            OspfState::TwoWay
        );
        assert_eq!(
            ospf_neighbor_state_machine(
                OspfState::TwoWay,
                OspfNeighborEvent::NegotiationDone
            ),
            OspfState::ExStart
        );
        assert_eq!(
            ospf_neighbor_state_machine(OspfState::ExStart, OspfNeighborEvent::ExchangeDone),
            OspfState::Exchange
        );
        assert_eq!(
            ospf_neighbor_state_machine(OspfState::Exchange, OspfNeighborEvent::LSALoadDone),
            OspfState::Loading
        );
        assert_eq!(
            ospf_neighbor_state_machine(OspfState::Loading, OspfNeighborEvent::LSALoadDone),
            OspfState::Full
        );
    }

    #[test]
    fn test_bgp_update_encode_decode() {
        let update = BgpUpdate {
            withdrawn_routes: Vec::new(),
            path_attrs: vec![BgpPathAttr {
                origin: 0,
                as_path: vec![65000, 65001],
                next_hop: 0x0A000001,
                local_pref: 100,
                med: 0,
            }],
            nlri: vec![BgpRoute {
                prefix: 0xC0A80000,
                prefix_len: 24,
            }],
        };
        let encoded = update.encode();
        let decoded = BgpUpdate::decode(&encoded).unwrap();
        assert_eq!(decoded.path_attrs.len(), 1);
        assert_eq!(decoded.path_attrs[0].as_path, vec![65000, 65001]);
        assert_eq!(decoded.nlri.len(), 1);
        assert_eq!(decoded.nlri[0].prefix_len, 24);
    }

    #[test]
    fn test_ospf_lsa_header_encode_decode() {
        let header = OspfLsaHeader::new(1, 0x01010101, 0x02020202);
        let mut buf = Vec::new();
        header.encode(&mut buf);
        assert_eq!(buf.len(), 20);
        let decoded = OspfLsaHeader::decode(&buf).unwrap();
        assert_eq!(decoded.lsa_type, 1);
        assert_eq!(decoded.id, 0x01010101);
        assert_eq!(decoded.adv_router, 0x02020202);
    }

    #[test]
    fn test_ospf_db_desc_encode_decode() {
        let mut dd = OspfDatabaseDescription::new(1);
        dd.lsa_headers
            .push(OspfLsaHeader::new(1, 0x01010101, 0x02020202));
        let encoded = dd.encode();
        let decoded = OspfDatabaseDescription::decode(&encoded).unwrap();
        assert_eq!(decoded.dd_sequence, 1);
        assert_eq!(decoded.lsa_headers.len(), 1);
    }
}
