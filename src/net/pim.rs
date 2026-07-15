use alloc::vec;
use alloc::vec::Vec;

const PIM_VERSION: u8 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PimMode {
    Dense = 0,
    Sparse = 1,
    SourceSpecific = 3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PimMessageType {
    Hello = 0,
    Register = 1,
    RegisterStop = 2,
    JoinPrune = 3,
    Bootstrap = 4,
    Assert = 5,
    Graft = 6,
    GraftAck = 7,
    CandidateRPAdvertisement = 8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JoinType {
    Normal = 0,
    Include = 1,
    Exclude = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PruneType {
    Normal = 0,
    Include = 1,
    Exclude = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PimState {
    Down,
    Listen,
    DR,
    RP,
    FHR,
    LHR,
}

#[derive(Clone, Debug)]
pub struct PimHeader {
    pub version: u8,
    pub packet_type: u8,
    pub reserved: u8,
    pub checksum: u16,
}

impl PimHeader {
    pub fn new(packet_type: u8) -> Self {
        PimHeader {
            version: PIM_VERSION,
            packet_type,
            reserved: 0,
            checksum: 0,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(4);
        buf.push((self.version << 4) | (self.packet_type & 0x0F));
        buf.push(self.reserved);
        buf.extend_from_slice(&self.checksum.to_be_bytes());
        buf
    }

    pub fn decode(data: &[u8]) -> Option<Self> {
        if data.len() < 4 {
            return None;
        }
        let version = (data[0] >> 4) & 0x0F;
        let packet_type = data[0] & 0x0F;
        let reserved = data[1];
        let checksum = u16::from_be_bytes([data[2], data[3]]);
        Some(PimHeader {
            version,
            packet_type,
            reserved,
            checksum,
        })
    }
}

#[derive(Clone, Debug)]
pub struct PimHello {
    pub hold_time: u16,
    pub dr_priority: u32,
    pub generation_id: u32,
    pub address_list: Vec<u32>,
}

impl PimHello {
    pub fn new(hold_time: u16, dr_priority: u32, generation_id: u32) -> Self {
        PimHello {
            hold_time,
            dr_priority,
            generation_id,
            address_list: Vec::new(),
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&self.hold_time.to_be_bytes());
        buf.extend_from_slice(&self.dr_priority.to_be_bytes());
        buf.extend_from_slice(&self.generation_id.to_be_bytes());
        for addr in &self.address_list {
            buf.extend_from_slice(&addr.to_be_bytes());
        }
        buf
    }

    pub fn decode(data: &[u8]) -> Option<Self> {
        if data.len() < 10 {
            return None;
        }
        let hold_time = u16::from_be_bytes([data[0], data[1]]);
        let dr_priority = u32::from_be_bytes([data[2], data[3], data[4], data[5]]);
        let generation_id = u32::from_be_bytes([data[6], data[7], data[8], data[9]]);
        let mut address_list = Vec::new();
        let mut pos = 10;
        while pos + 4 <= data.len() {
            address_list.push(u32::from_be_bytes([
                data[pos], data[pos + 1], data[pos + 2], data[pos + 3],
            ]));
            pos += 4;
        }
        Some(PimHello {
            hold_time,
            dr_priority,
            generation_id,
            address_list,
        })
    }
}

#[derive(Clone, Debug)]
pub struct PimJoinPrune {
    pub upstream_neighbor: u32,
    pub num_groups: u16,
    pub groups: Vec<PimJoinPruneGroup>,
}

#[derive(Clone, Debug)]
pub struct PimJoinPruneGroup {
    pub multicast_group: u32,
    pub joined_sources: Vec<u32>,
    pub pruned_sources: Vec<u32>,
    pub join_type: JoinType,
    pub prune_type: PruneType,
}

impl PimJoinPrune {
    pub fn new(
        upstream_neighbor: u32,
        multicast_group: u32,
        joined_sources: Vec<u32>,
        pruned_sources: Vec<u32>,
    ) -> Self {
        PimJoinPrune {
            upstream_neighbor,
            num_groups: 1,
            groups: vec![PimJoinPruneGroup {
                multicast_group,
                joined_sources,
                pruned_sources,
                join_type: JoinType::Normal,
                prune_type: PruneType::Normal,
            }],
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&self.upstream_neighbor.to_be_bytes());
        buf.extend_from_slice(&self.num_groups.to_be_bytes());
        buf.push(0);
        buf.push(0);
        for group in &self.groups {
            buf.extend_from_slice(&group.multicast_group.to_be_bytes());
            buf.extend_from_slice(&[group.join_type as u8; 1]);
            buf.push(0);
            buf.extend_from_slice(&(group.joined_sources.len() as u16).to_be_bytes());
            buf.extend_from_slice(&[group.prune_type as u8; 1]);
            buf.push(0);
            buf.extend_from_slice(&(group.pruned_sources.len() as u16).to_be_bytes());
            for src in &group.joined_sources {
                buf.extend_from_slice(&src.to_be_bytes());
            }
            for src in &group.pruned_sources {
                buf.extend_from_slice(&src.to_be_bytes());
            }
        }
        buf
    }

    pub fn decode(data: &[u8]) -> Option<Self> {
        if data.len() < 8 {
            return None;
        }
        let upstream_neighbor = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
        let num_groups = u16::from_be_bytes([data[4], data[5]]);
        let mut pos = 8;
        let mut groups = Vec::new();
        for _ in 0..num_groups {
            if pos + 4 > data.len() {
                break;
            }
            let multicast_group = u32::from_be_bytes([
                data[pos], data[pos + 1], data[pos + 2], data[pos + 3],
            ]);
            pos += 4;
            if pos + 3 > data.len() {
                break;
            }
            let join_type_val = data[pos];
            pos += 2;
            if pos + 2 > data.len() {
                break;
            }
            let num_joined = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
            pos += 2;
            if pos + 3 > data.len() {
                break;
            }
            let prune_type_val = data[pos];
            pos += 2;
            if pos + 2 > data.len() {
                break;
            }
            let num_pruned = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
            pos += 2;
            let mut joined_sources = Vec::new();
            for _ in 0..num_joined {
                if pos + 4 > data.len() {
                    break;
                }
                joined_sources.push(u32::from_be_bytes([
                    data[pos], data[pos + 1], data[pos + 2], data[pos + 3],
                ]));
                pos += 4;
            }
            let mut pruned_sources = Vec::new();
            for _ in 0..num_pruned {
                if pos + 4 > data.len() {
                    break;
                }
                pruned_sources.push(u32::from_be_bytes([
                    data[pos], data[pos + 1], data[pos + 2], data[pos + 3],
                ]));
                pos += 4;
            }
            let join_type = match join_type_val {
                1 => JoinType::Include,
                2 => JoinType::Exclude,
                _ => JoinType::Normal,
            };
            let prune_type = match prune_type_val {
                1 => PruneType::Include,
                2 => PruneType::Exclude,
                _ => PruneType::Normal,
            };
            groups.push(PimJoinPruneGroup {
                multicast_group,
                joined_sources,
                pruned_sources,
                join_type,
                prune_type,
            });
        }
        Some(PimJoinPrune {
            upstream_neighbor,
            num_groups,
            groups,
        })
    }
}

#[derive(Clone, Debug)]
pub struct PimRegister {
    pub flags: u8,
    pub reserved: u8,
    pub checksum: u16,
    pub ip_header: Vec<u8>,
    pub data: Vec<u8>,
}

impl PimRegister {
    pub fn new() -> Self {
        PimRegister {
            flags: 0,
            reserved: 0,
            checksum: 0,
            ip_header: Vec::new(),
            data: Vec::new(),
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.push(self.flags);
        buf.push(self.reserved);
        buf.extend_from_slice(&self.checksum.to_be_bytes());
        buf.extend_from_slice(&self.ip_header);
        buf.extend_from_slice(&self.data);
        buf
    }

    pub fn decode(data: &[u8]) -> Option<Self> {
        if data.len() < 4 {
            return None;
        }
        Some(PimRegister {
            flags: data[0],
            reserved: data[1],
            checksum: u16::from_be_bytes([data[2], data[3]]),
            ip_header: if data.len() > 20 {
                data[4..20].to_vec()
            } else {
                Vec::new()
            },
            data: if data.len() > 20 {
                data[20..].to_vec()
            } else if data.len() > 4 {
                data[4..].to_vec()
            } else {
                Vec::new()
            },
        })
    }
}

#[derive(Clone, Debug)]
pub struct PimAssert {
    pub group_addr: u32,
    pub source_addr: u32,
    pub metric_preference: u32,
    pub metric: u32,
}

impl PimAssert {
    pub fn new(group_addr: u32, source_addr: u32) -> Self {
        PimAssert {
            group_addr,
            source_addr,
            metric_preference: 10,
            metric: 0,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(16);
        buf.extend_from_slice(&self.group_addr.to_be_bytes());
        buf.extend_from_slice(&self.source_addr.to_be_bytes());
        buf.extend_from_slice(&self.metric_preference.to_be_bytes());
        buf.extend_from_slice(&self.metric.to_be_bytes());
        buf
    }

    pub fn decode(data: &[u8]) -> Option<Self> {
        if data.len() < 16 {
            return None;
        }
        Some(PimAssert {
            group_addr: u32::from_be_bytes([data[0], data[1], data[2], data[3]]),
            source_addr: u32::from_be_bytes([data[4], data[5], data[6], data[7]]),
            metric_preference: u32::from_be_bytes([data[8], data[9], data[10], data[11]]),
            metric: u32::from_be_bytes([data[12], data[13], data[14], data[15]]),
        })
    }
}

#[derive(Clone, Debug)]
pub struct PimGraft {
    pub group_addr: u32,
    pub source_addr: u32,
    pub upstream_neighbor: u32,
}

impl PimGraft {
    pub fn new(group_addr: u32, source_addr: u32, upstream_neighbor: u32) -> Self {
        PimGraft {
            group_addr,
            source_addr,
            upstream_neighbor,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(12);
        buf.extend_from_slice(&self.upstream_neighbor.to_be_bytes());
        buf.extend_from_slice(&self.group_addr.to_be_bytes());
        buf.extend_from_slice(&self.source_addr.to_be_bytes());
        buf
    }

    pub fn decode(data: &[u8]) -> Option<Self> {
        if data.len() < 12 {
            return None;
        }
        Some(PimGraft {
            upstream_neighbor: u32::from_be_bytes([data[0], data[1], data[2], data[3]]),
            group_addr: u32::from_be_bytes([data[4], data[5], data[6], data[7]]),
            source_addr: u32::from_be_bytes([data[8], data[9], data[10], data[11]]),
        })
    }
}

pub fn pim_hello_create(router_id: u32) -> PimHello {
    let mut hello = PimHello::new(105, 1, router_id);
    hello.address_list.push(router_id);
    hello
}

pub fn pim_join_prune_create(
    upstream: u32,
    group: u32,
    joined: Vec<u32>,
    pruned: Vec<u32>,
) -> PimJoinPrune {
    PimJoinPrune::new(upstream, group, joined, pruned)
}

pub fn pim_register_create(data: Vec<u8>) -> PimRegister {
    let mut reg = PimRegister::new();
    reg.data = data;
    reg
}

pub fn pim_assert_create(group: u32, source: u32) -> PimAssert {
    PimAssert::new(group, source)
}

pub fn pim_state_machine(state: PimState, neighbor_up: bool, is_dr: bool, is_rp: bool) -> PimState {
    match state {
        PimState::Down => {
            if neighbor_up {
                PimState::Listen
            } else {
                PimState::Down
            }
        }
        PimState::Listen => {
            if is_dr {
                PimState::DR
            } else if neighbor_up {
                PimState::Listen
            } else {
                PimState::Down
            }
        }
        PimState::DR => {
            if !is_dr {
                PimState::Listen
            } else if is_rp {
                PimState::RP
            } else {
                PimState::DR
            }
        }
        PimState::RP => {
            if !is_rp {
                PimState::DR
            } else if !is_dr {
                PimState::Listen
            } else {
                PimState::RP
            }
        }
        PimState::FHR => {
            if !neighbor_up {
                PimState::Down
            } else {
                PimState::FHR
            }
        }
        PimState::LHR => {
            if !neighbor_up {
                PimState::Down
            } else {
                PimState::LHR
            }
        }
    }
}

pub const PIM_TYPE_HELLO: u8 = 0;
pub const PIM_TYPE_REGISTER: u8 = 1;
pub const PIM_TYPE_REGISTER_STOP: u8 = 2;
pub const PIM_TYPE_JOIN_PRUNE: u8 = 3;
pub const PIM_TYPE_BOOTSTRAP: u8 = 4;
pub const PIM_TYPE_ASSERT: u8 = 5;
pub const PIM_TYPE_GRAFT: u8 = 6;
pub const PIM_TYPE_GRAFT_ACK: u8 = 7;
pub const PIM_TYPE_CANDIDATE_RP_ADV: u8 = 8;

pub const PIM_HELLO_HOLD_TIME: u16 = 105;
pub const PIM_DEFAULT_DR_PRIORITY: u32 = 1;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pim_header_encode_decode() {
        let header = PimHeader::new(PIM_TYPE_HELLO);
        let encoded = header.encode();
        assert_eq!(encoded.len(), 4);
        let decoded = PimHeader::decode(&encoded).unwrap();
        assert_eq!(decoded.version, 2);
        assert_eq!(decoded.packet_type, 0);
    }

    #[test]
    fn test_pim_hello_encode_decode() {
        let mut hello = PimHello::new(105, 1, 0xDEADBEEF);
        hello.address_list.push(0x0A000001);
        let encoded = hello.encode();
        let decoded = PimHello::decode(&encoded).unwrap();
        assert_eq!(decoded.hold_time, 105);
        assert_eq!(decoded.dr_priority, 1);
        assert_eq!(decoded.generation_id, 0xDEADBEEF);
        assert_eq!(decoded.address_list.len(), 1);
        assert_eq!(decoded.address_list[0], 0x0A000001);
    }

    #[test]
    fn test_pim_join_prune_encode_decode() {
        let jp = PimJoinPrune::new(
            0x0A000001,
            0xE0000001,
            vec![0x0A000002, 0x0A000003],
            vec![0x0A000004],
        );
        let encoded = jp.encode();
        let decoded = PimJoinPrune::decode(&encoded).unwrap();
        assert_eq!(decoded.upstream_neighbor, 0x0A000001);
        assert_eq!(decoded.num_groups, 1);
        assert_eq!(decoded.groups[0].multicast_group, 0xE0000001);
        assert_eq!(decoded.groups[0].joined_sources.len(), 2);
        assert_eq!(decoded.groups[0].pruned_sources.len(), 1);
    }

    #[test]
    fn test_pim_register_encode_decode() {
        let mut reg = PimRegister::new();
        reg.flags = 0x80;
        reg.data = vec![0xDE, 0xAD, 0xBE, 0xEF];
        let encoded = reg.encode();
        let decoded = PimRegister::decode(&encoded).unwrap();
        assert_eq!(decoded.flags, 0x80);
        assert_eq!(decoded.data, vec![0xDE, 0xAD, 0xBE, 0xEF]);
    }

    #[test]
    fn test_pim_assert_encode_decode() {
        let assert = PimAssert::new(0xE0000001, 0x0A000002);
        let encoded = assert.encode();
        let decoded = PimAssert::decode(&encoded).unwrap();
        assert_eq!(decoded.group_addr, 0xE0000001);
        assert_eq!(decoded.source_addr, 0x0A000002);
        assert_eq!(decoded.metric_preference, 10);
    }

    #[test]
    fn test_pim_graft_encode_decode() {
        let graft = PimGraft::new(0xE0000001, 0x0A000002, 0x0A000001);
        let encoded = graft.encode();
        let decoded = PimGraft::decode(&encoded).unwrap();
        assert_eq!(decoded.group_addr, 0xE0000001);
        assert_eq!(decoded.source_addr, 0x0A000002);
        assert_eq!(decoded.upstream_neighbor, 0x0A000001);
    }

    #[test]
    fn test_pim_hello_create() {
        let hello = pim_hello_create(0x0A000001);
        assert_eq!(hello.hold_time, PIM_HELLO_HOLD_TIME);
        assert_eq!(hello.dr_priority, PIM_DEFAULT_DR_PRIORITY);
        assert_eq!(hello.generation_id, 0x0A000001);
        assert_eq!(hello.address_list.len(), 1);
    }

    #[test]
    fn test_pim_state_machine() {
        assert_eq!(
            pim_state_machine(PimState::Down, true, false, false),
            PimState::Listen
        );
        assert_eq!(
            pim_state_machine(PimState::Listen, true, true, false),
            PimState::DR
        );
        assert_eq!(
            pim_state_machine(PimState::DR, true, true, true),
            PimState::RP
        );
        assert_eq!(
            pim_state_machine(PimState::RP, true, false, true),
            PimState::Listen
        );
        assert_eq!(
            pim_state_machine(PimState::Listen, false, false, false),
            PimState::Down
        );
    }
}
