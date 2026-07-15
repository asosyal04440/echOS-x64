use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use super::NetError;

const DEFAULT_MAX_ENTRIES: usize = 262144;

const TCP_ESTABLISHED_TIMEOUT_MS: u64 = 432_000_000;
const TCP_SYN_SENT_TIMEOUT_MS: u64 = 60_000;
const TCP_SYN_RECV_TIMEOUT_MS: u64 = 60_000;
const TCP_FIN_WAIT_TIMEOUT_MS: u64 = 120_000;
const TCP_TIME_WAIT_TIMEOUT_MS: u64 = 120_000;
const TCP_CLOSE_WAIT_TIMEOUT_MS: u64 = 432_000_000;
const TCP_LAST_ACK_TIMEOUT_MS: u64 = 432_000_000;
const TCP_CLOSE_TIMEOUT_MS: u64 = 10_000;
const UDP_TIMEOUT_MS: u64 = 120_000;
const UDP_STREAM_TIMEOUT_MS: u64 = 180_000;
const ICMP_TIMEOUT_MS: u64 = 30_000;
const GENERIC_TIMEOUT_MS: u64 = 60_000;

const IPPROTO_ICMP: u8 = 1;
const IPPROTO_TCP: u8 = 6;
const IPPROTO_UDP: u8 = 17;

const NFPROTO_IPV4: u8 = 2;
const NFPROTO_IPV6: u8 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum IpFamily {
    Unspec = 0,
    IPv4 = 2,
    IPv6 = 10,
}

impl IpFamily {
    pub fn from_u8(v: u8) -> Self {
        match v {
            2 => Self::IPv4,
            10 => Self::IPv6,
            _ => Self::Unspec,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ConntrackTuple {
    pub src_ip: u128,
    pub dst_ip: u128,
    pub src_port: u16,
    pub dst_port: u16,
    pub protocol: u8,
    pub l3num: u8,
}

impl ConntrackTuple {
    pub fn new(
        src_ip: u128,
        dst_ip: u128,
        src_port: u16,
        dst_port: u16,
        protocol: u8,
        l3num: u8,
    ) -> Self {
        Self {
            src_ip,
            dst_ip,
            src_port,
            dst_port,
            protocol,
            l3num,
        }
    }

    pub fn ipv4(
        src: [u8; 4],
        dst: [u8; 4],
        src_port: u16,
        dst_port: u16,
        protocol: u8,
    ) -> Self {
        let mut s = [0u8; 16];
        let mut d = [0u8; 16];
        s[12..16].copy_from_slice(&src);
        d[12..16].copy_from_slice(&dst);
        Self {
            src_ip: u128::from_be_bytes(s),
            dst_ip: u128::from_be_bytes(d),
            src_port,
            dst_port,
            protocol,
            l3num: NFPROTO_IPV4,
        }
    }

    pub fn reversed(&self) -> Self {
        Self {
            src_ip: self.dst_ip,
            dst_ip: self.src_ip,
            src_port: self.dst_port,
            dst_port: self.src_port,
            protocol: self.protocol,
            l3num: self.l3num,
        }
    }

    pub fn hash64(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        let prime: u64 = 0x100000001b3;
        for &b in &self.src_ip.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(prime);
        }
        for &b in &self.dst_ip.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(prime);
        }
        for &b in &self.src_port.to_be_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(prime);
        }
        for &b in &self.dst_port.to_be_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(prime);
        }
        h ^= self.protocol as u64;
        h = h.wrapping_mul(prime);
        h ^= self.l3num as u64;
        h = h.wrapping_mul(prime);
        h
    }

    pub fn hash32(&self) -> u32 {
        self.hash64() as u32
    }

    pub fn is_ipv4(&self) -> bool {
        self.l3num == NFPROTO_IPV4
    }

    pub fn is_ipv6(&self) -> bool {
        self.l3num == NFPROTO_IPV6
    }

    pub fn src_ip_as_ipv4(&self) -> [u8; 4] {
        let b = self.src_ip.to_be_bytes();
        [b[12], b[13], b[14], b[15]]
    }

    pub fn dst_ip_as_ipv4(&self) -> [u8; 4] {
        let b = self.dst_ip.to_be_bytes();
        [b[12], b[13], b[14], b[15]]
    }
}

impl fmt::Debug for ConntrackTuple {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_ipv4() {
            let s = self.src_ip_as_ipv4();
            let d = self.dst_ip_as_ipv4();
            write!(
                f,
                "{}.{}.{}.{}:{} -> {}.{}.{}.{}:{} [proto={}]",
                s[0], s[1], s[2], s[3], self.src_port,
                d[0], d[1], d[2], d[3], self.dst_port,
                self.protocol
            )
        } else {
            write!(
                f,
                "{:032x}:{} -> {:032x}:{} [proto={}]",
                self.src_ip, self.src_port, self.dst_ip, self.dst_port, self.protocol
            )
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum ConntrackState {
    Untracked = 0,
    New = 1,
    Established = 2,
    Related = 3,
    Invalid = 4,
    Reply = 5,
    Senior = 6,
}

impl ConntrackState {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Untracked,
            1 => Self::New,
            2 => Self::Established,
            3 => Self::Related,
            4 => Self::Invalid,
            5 => Self::Reply,
            _ => Self::Senior,
        }
    }

    pub fn is_valid_transition(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::New, Self::Established)
                | (Self::New, Self::Invalid)
                | (Self::New, Self::Related)
                | (Self::Established, Self::Established)
                | (Self::Established, Self::Invalid)
                | (Self::Established, Self::Reply)
                | (Self::Related, Self::Established)
                | (Self::Related, Self::Invalid)
                | (Self::Related, Self::Related)
                | (Self::Reply, Self::Established)
                | (Self::Reply, Self::Invalid)
                | (Self::Reply, Self::Reply)
                | (Self::Invalid, Self::New)
                | (Self::Untracked, Self::Untracked)
                | (Self::Senior, Self::Established)
                | (Self::Senior, Self::Invalid)
        )
    }

    pub fn default_timeout_ms(self, protocol: u8) -> u64 {
        match self {
            Self::New | Self::Established | Self::Related | Self::Reply => match protocol {
                IPPROTO_TCP => TCP_ESTABLISHED_TIMEOUT_MS,
                IPPROTO_UDP => UDP_TIMEOUT_MS,
                IPPROTO_ICMP => ICMP_TIMEOUT_MS,
                _ => GENERIC_TIMEOUT_MS,
            },
            Self::Untracked => 0,
            Self::Invalid => 0,
            Self::Senior => GENERIC_TIMEOUT_MS,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Untracked => "UNTRACKED",
            Self::New => "NEW",
            Self::Established => "ESTABLISHED",
            Self::Related => "RELATED",
            Self::Invalid => "INVALID",
            Self::Reply => "REPLY",
            Self::Senior => "SENIOR",
        }
    }
}

impl fmt::Display for ConntrackState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum TcpConntrackState {
    SynSent = 0,
    SynRecv = 1,
    Established = 2,
    CloseWait = 3,
    LastAck = 4,
    TimeWait = 5,
    Close = 6,
    Ignore = 7,
}

impl TcpConntrackState {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::SynSent,
            1 => Self::SynRecv,
            2 => Self::Established,
            3 => Self::CloseWait,
            4 => Self::LastAck,
            5 => Self::TimeWait,
            6 => Self::Close,
            _ => Self::Ignore,
        }
    }

    pub fn timeout_ms(self) -> u64 {
        match self {
            Self::SynSent => TCP_SYN_SENT_TIMEOUT_MS,
            Self::SynRecv => TCP_SYN_RECV_TIMEOUT_MS,
            Self::Established => TCP_ESTABLISHED_TIMEOUT_MS,
            Self::CloseWait => TCP_CLOSE_WAIT_TIMEOUT_MS,
            Self::LastAck => TCP_LAST_ACK_TIMEOUT_MS,
            Self::TimeWait => TCP_TIME_WAIT_TIMEOUT_MS,
            Self::Close => TCP_CLOSE_TIMEOUT_MS,
            Self::Ignore => TCP_CLOSE_TIMEOUT_MS,
        }
    }

    pub fn is_valid_transition(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::SynSent, Self::SynRecv)
                | (Self::SynSent, Self::Close)
                | (Self::SynSent, Self::Ignore)
                | (Self::SynRecv, Self::Established)
                | (Self::SynRecv, Self::Close)
                | (Self::SynRecv, Self::Ignore)
                | (Self::Established, Self::CloseWait)
                | (Self::Established, Self::Close)
                | (Self::Established, Self::Ignore)
                | (Self::CloseWait, Self::LastAck)
                | (Self::CloseWait, Self::Close)
                | (Self::CloseWait, Self::Ignore)
                | (Self::LastAck, Self::TimeWait)
                | (Self::LastAck, Self::Close)
                | (Self::LastAck, Self::Ignore)
                | (Self::TimeWait, Self::Close)
                | (Self::TimeWait, Self::Ignore)
                | (Self::Close, Self::SynSent)
                | (Self::Close, Self::Ignore)
                | (Self::Ignore, Self::Ignore)
        )
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::SynSent => "SYN_SENT",
            Self::SynRecv => "SYN_RECV",
            Self::Established => "ESTABLISHED",
            Self::CloseWait => "CLOSE_WAIT",
            Self::LastAck => "LAST_ACK",
            Self::TimeWait => "TIME_WAIT",
            Self::Close => "CLOSE",
            Self::Ignore => "IGNORE",
        }
    }
}

impl fmt::Display for TcpConntrackState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConntrackZone {
    pub src_zone: u16,
    pub dst_zone: u16,
}

impl ConntrackZone {
    pub const DEFAULT: Self = Self {
        src_zone: 0,
        dst_zone: 0,
    };

    pub fn new(src_zone: u16, dst_zone: u16) -> Self {
        Self {
            src_zone,
            dst_zone,
        }
    }

    pub fn is_default(&self) -> bool {
        self.src_zone == 0 && self.dst_zone == 0
    }
}

#[derive(Clone, Debug)]
pub struct ConntrackEntry {
    pub tuple_orig: ConntrackTuple,
    pub tuple_reply: ConntrackTuple,
    pub state: ConntrackState,
    pub timeout_ms: u64,
    pub mark: u32,
    pub zone: ConntrackZone,
    pub helper_name: Option<String>,
    pub tcp_window_state: TcpConntrackState,
    pub created_ms: u64,
    pub last_seen_ms: u64,
    pub packets_orig: u64,
    pub bytes_orig: u64,
    pub packets_reply: u64,
    pub bytes_reply: u64,
}

impl ConntrackEntry {
    pub fn new(
        tuple_orig: ConntrackTuple,
        tuple_reply: ConntrackTuple,
        state: ConntrackState,
        current_time_ms: u64,
    ) -> Self {
        let timeout_ms = state.default_timeout_ms(tuple_orig.protocol);
        Self {
            tuple_orig,
            tuple_reply,
            state,
            timeout_ms,
            mark: 0,
            zone: ConntrackZone::DEFAULT,
            helper_name: None,
            tcp_window_state: TcpConntrackState::Close,
            created_ms: current_time_ms,
            last_seen_ms: current_time_ms,
            packets_orig: 0,
            bytes_orig: 0,
            packets_reply: 0,
            bytes_reply: 0,
        }
    }

    pub fn with_zone(
        tuple_orig: ConntrackTuple,
        tuple_reply: ConntrackTuple,
        state: ConntrackState,
        zone: ConntrackZone,
        current_time_ms: u64,
    ) -> Self {
        let timeout_ms = state.default_timeout_ms(tuple_orig.protocol);
        Self {
            tuple_orig,
            tuple_reply,
            state,
            timeout_ms,
            mark: 0,
            zone,
            helper_name: None,
            tcp_window_state: TcpConntrackState::Close,
            created_ms: current_time_ms,
            last_seen_ms: current_time_ms,
            packets_orig: 0,
            bytes_orig: 0,
            packets_reply: 0,
            bytes_reply: 0,
        }
    }

    pub fn is_expired(&self, current_time_ms: u64) -> bool {
        if self.timeout_ms == 0 {
            return false;
        }
        current_time_ms >= self.created_ms.saturating_add(self.timeout_ms)
    }

    pub fn remaining_ms(&self, current_time_ms: u64) -> u64 {
        if self.timeout_ms == 0 {
            return u64::MAX;
        }
        let deadline = self.created_ms.saturating_add(self.timeout_ms);
        deadline.saturating_sub(current_time_ms)
    }

    pub fn touch(&mut self, current_time_ms: u64) {
        self.last_seen_ms = current_time_ms;
    }

    pub fn refresh_timeout(&mut self, current_time_ms: u64) {
        self.created_ms = current_time_ms;
        self.last_seen_ms = current_time_ms;
    }

    pub fn inc_orig(&mut self, bytes: u64) {
        self.packets_orig += 1;
        self.bytes_orig += bytes;
    }

    pub fn inc_reply(&mut self, bytes: u64) {
        self.packets_reply += 1;
        self.bytes_reply += bytes;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConntrackError {
    TableFull,
    NotFound,
    InvalidTransition,
}

impl fmt::Display for ConntrackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TableFull => f.write_str("conntrack table full"),
            Self::NotFound => f.write_str("conntrack entry not found"),
            Self::InvalidTransition => f.write_str("invalid conntrack state transition"),
        }
    }
}

impl From<ConntrackError> for NetError {
    fn from(e: ConntrackError) -> Self {
        match e {
            ConntrackError::TableFull => NetError::BufferFull,
            ConntrackError::NotFound => NetError::Unknown,
            ConntrackError::InvalidTransition => NetError::InvalidParam,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ConntrackKey {
    src_ip: u128,
    dst_ip: u128,
    src_port: u16,
    dst_port: u16,
    protocol: u8,
    l3num: u8,
    src_zone: u16,
    dst_zone: u16,
}

impl ConntrackKey {
    fn from_tuple(tuple: &ConntrackTuple, zone: &ConntrackZone) -> Self {
        Self {
            src_ip: tuple.src_ip,
            dst_ip: tuple.dst_ip,
            src_port: tuple.src_port,
            dst_port: tuple.dst_port,
            protocol: tuple.protocol,
            l3num: tuple.l3num,
            src_zone: zone.src_zone,
            dst_zone: zone.dst_zone,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ConntrackStats {
    pub total_inserts: u64,
    pub total_lookups: u64,
    pub total_lookup_hits: u64,
    pub total_lookup_misses: u64,
    pub total_removes: u64,
    pub total_updates: u64,
    pub total_pruned: u64,
    pub total_table_full: u64,
    pub total_invalid_transitions: u64,
    pub expect_adds: u64,
    pub expect_hits: u64,
    pub current_entries: usize,
}

pub struct ConntrackTable {
    entries: BTreeMap<ConntrackKey, ConntrackEntry>,
    max_entries: usize,
    expects: BTreeMap<ConntrackKey, ConntrackExpect>,
    stats: ConntrackStats,
}

impl ConntrackTable {
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_MAX_ENTRIES)
    }

    pub fn with_capacity(max_entries: usize) -> Self {
        Self {
            entries: BTreeMap::new(),
            max_entries,
            expects: BTreeMap::new(),
            stats: ConntrackStats::default(),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn max_entries(&self) -> usize {
        self.max_entries
    }

    pub fn stats(&self) -> &ConntrackStats {
        &self.stats
    }

    pub fn expect_len(&self) -> usize {
        self.expects.len()
    }

    fn sync_stats(&mut self) {
        self.stats.current_entries = self.entries.len();
    }
}

pub struct ConntrackHelper {
    pub name: &'static str,
    pub protocol: u8,
    pub port: u16,
    pub expect_max: u32,
    pub udp_timeout_ms: u64,
    pub tcp_timeout_ms: u64,
}

impl ConntrackHelper {
    pub const fn new(name: &'static str, protocol: u8, port: u16, expect_max: u32) -> Self {
        Self {
            name,
            protocol,
            port,
            expect_max,
            udp_timeout_ms: UDP_TIMEOUT_MS,
            tcp_timeout_ms: TCP_ESTABLISHED_TIMEOUT_MS,
        }
    }

    pub const fn with_timeouts(
        name: &'static str,
        protocol: u8,
        port: u16,
        expect_max: u32,
        udp_timeout_ms: u64,
        tcp_timeout_ms: u64,
    ) -> Self {
        Self {
            name,
            protocol,
            port,
            expect_max,
            udp_timeout_ms,
            tcp_timeout_ms,
        }
    }
}

static CONNTRACK_HELPERS: &[ConntrackHelper] = &[
    ConntrackHelper::new("ftp", IPPROTO_TCP, 21, 8),
    ConntrackHelper::new("sip", IPPROTO_UDP, 5060, 16),
    ConntrackHelper::new("sip-tcp", IPPROTO_TCP, 5060, 16),
    ConntrackHelper::new("tftp", IPPROTO_UDP, 69, 4),
    ConntrackHelper::new("amanda", IPPROTO_TCP, 10080, 4),
    ConntrackHelper::new("h323", IPPROTO_UDP, 1719, 8),
    ConntrackHelper::new("h323-tcp", IPPROTO_TCP, 1720, 8),
    ConntrackHelper::new("rpc", IPPROTO_TCP, 111, 4),
    ConntrackHelper::new("rpc-udp", IPPROTO_UDP, 111, 4),
    ConntrackHelper::new("pptp", IPPROTO_TCP, 1723, 4),
    ConntrackHelper::new("snmp", IPPROTO_UDP, 161, 4),
    ConntrackHelper::new("snmp-trap", IPPROTO_UDP, 162, 4),
    ConntrackHelper::new("irc", IPPROTO_TCP, 6667, 8),
    ConntrackHelper::new("irc-ssl", IPPROTO_TCP, 6697, 8),
    ConntrackHelper::new("netbios-ns", IPPROTO_UDP, 137, 4),
    ConntrackHelper::new("netbios-dgm", IPPROTO_UDP, 138, 4),
    ConntrackHelper::new("netbios-ssn", IPPROTO_TCP, 139, 4),
    ConntrackHelper::with_timeouts("sane", IPPROTO_TCP, 6566, 4, 120_000, 3_600_000),
    ConntrackHelper::new("ssdp", IPPROTO_UDP, 1900, 4),
    ConntrackHelper::new("llmnr", IPPROTO_UDP, 5355, 4),
    ConntrackHelper::new("dns-tcp", IPPROTO_TCP, 53, 4),
    ConntrackHelper::new("dns-udp", IPPROTO_UDP, 53, 4),
];

pub fn conntrack_helper_lookup(protocol: u8) -> Option<&'static ConntrackHelper> {
    CONNTRACK_HELPERS.iter().find(|h| h.protocol == protocol)
}

pub fn conntrack_helper_lookup_by_name(name: &str) -> Option<&'static ConntrackHelper> {
    CONNTRACK_HELPERS.iter().find(|h| h.name == name)
}

pub fn conntrack_helper_lookup_by_port(protocol: u8, port: u16) -> Option<&'static ConntrackHelper> {
    CONNTRACK_HELPERS
        .iter()
        .find(|h| h.protocol == protocol && h.port == port)
}

pub fn conntrack_helper_list() -> &'static [ConntrackHelper] {
    CONNTRACK_HELPERS
}

#[derive(Clone, Debug)]
pub struct ConntrackExpect {
    pub tuple: ConntrackTuple,
    pub helper_name: String,
    pub timeout_ms: u64,
    pub zone: ConntrackZone,
    pub mark: u32,
    pub master_tuple: ConntrackTuple,
    pub created_ms: u64,
}

impl ConntrackExpect {
    pub fn new(
        tuple: ConntrackTuple,
        helper_name: String,
        timeout_ms: u64,
        master_tuple: ConntrackTuple,
        current_time_ms: u64,
    ) -> Self {
        Self {
            tuple,
            helper_name,
            timeout_ms,
            zone: ConntrackZone::DEFAULT,
            mark: 0,
            master_tuple,
            created_ms: current_time_ms,
        }
    }

    pub fn is_expired(&self, current_time_ms: u64) -> bool {
        if self.timeout_ms == 0 {
            return false;
        }
        current_time_ms >= self.created_ms.saturating_add(self.timeout_ms)
    }
}

pub fn conntrack_insert(
    table: &mut ConntrackTable,
    entry: ConntrackEntry,
) -> Result<(), ConntrackError> {
    if table.entries.len() >= table.max_entries {
        table.stats.total_table_full += 1;
        return Err(ConntrackError::TableFull);
    }
    let key = ConntrackKey::from_tuple(&entry.tuple_orig, &entry.zone);
    table.entries.insert(key, entry);
    table.stats.total_inserts += 1;
    table.sync_stats();
    Ok(())
}

pub fn conntrack_lookup<'a>(
    table: &'a ConntrackTable,
    tuple: &ConntrackTuple,
    zone: &ConntrackZone,
) -> Option<&'a ConntrackEntry> {
    let key = ConntrackKey::from_tuple(tuple, zone);
    table.entries.get(&key)
}

pub fn conntrack_lookup_mut<'a>(
    table: &'a mut ConntrackTable,
    tuple: &ConntrackTuple,
    zone: &ConntrackZone,
) -> Option<&'a mut ConntrackEntry> {
    let key = ConntrackKey::from_tuple(tuple, zone);
    table.stats.total_lookups += 1;
    let result = table.entries.get_mut(&key);
    if result.is_some() {
        table.stats.total_lookup_hits += 1;
    } else {
        table.stats.total_lookup_misses += 1;
    }
    result
}

pub fn conntrack_remove(
    table: &mut ConntrackTable,
    tuple: &ConntrackTuple,
    zone: &ConntrackZone,
) -> Option<ConntrackEntry> {
    let key = ConntrackKey::from_tuple(tuple, zone);
    let result = table.entries.remove(&key);
    if result.is_some() {
        table.stats.total_removes += 1;
        table.sync_stats();
    }
    result
}

pub fn conntrack_update_state(
    table: &mut ConntrackTable,
    tuple: &ConntrackTuple,
    zone: &ConntrackZone,
    new_state: ConntrackState,
    current_time_ms: u64,
) -> Result<(), ConntrackError> {
    let key = ConntrackKey::from_tuple(tuple, zone);
    let entry = table
        .entries
        .get_mut(&key)
        .ok_or(ConntrackError::NotFound)?;
    if !entry.state.is_valid_transition(new_state) {
        table.stats.total_invalid_transitions += 1;
        return Err(ConntrackError::InvalidTransition);
    }
    entry.state = new_state;
    entry.last_seen_ms = current_time_ms;
    entry.timeout_ms = new_state.default_timeout_ms(tuple.protocol);
    entry.created_ms = current_time_ms;
    table.stats.total_updates += 1;
    Ok(())
}

pub fn conntrack_update_timeout(
    table: &mut ConntrackTable,
    tuple: &ConntrackTuple,
    zone: &ConntrackZone,
    new_timeout_ms: u64,
) -> Result<(), ConntrackError> {
    let key = ConntrackKey::from_tuple(tuple, zone);
    let entry = table
        .entries
        .get_mut(&key)
        .ok_or(ConntrackError::NotFound)?;
    entry.timeout_ms = new_timeout_ms;
    table.stats.total_updates += 1;
    Ok(())
}

pub fn conntrack_update_tcp_state(
    table: &mut ConntrackTable,
    tuple: &ConntrackTuple,
    zone: &ConntrackZone,
    new_tcp_state: TcpConntrackState,
    current_time_ms: u64,
) -> Result<(), ConntrackError> {
    let key = ConntrackKey::from_tuple(tuple, zone);
    let entry = table
        .entries
        .get_mut(&key)
        .ok_or(ConntrackError::NotFound)?;
    if !entry.tcp_window_state.is_valid_transition(new_tcp_state) {
        return Err(ConntrackError::InvalidTransition);
    }
    entry.tcp_window_state = new_tcp_state;
    entry.timeout_ms = new_tcp_state.timeout_ms();
    entry.created_ms = current_time_ms;
    entry.last_seen_ms = current_time_ms;
    table.stats.total_updates += 1;
    Ok(())
}

pub fn conntrack_get_or_create<'a>(
    table: &'a mut ConntrackTable,
    tuple_orig: &ConntrackTuple,
    zone: &ConntrackZone,
    current_time_ms: u64,
) -> Result<&'a mut ConntrackEntry, ConntrackError> {
    let key = ConntrackKey::from_tuple(tuple_orig, zone);
    if table.entries.contains_key(&key) {
        table.stats.total_lookups += 1;
        table.stats.total_lookup_hits += 1;
        return Ok(table.entries.get_mut(&key).unwrap());
    }
    if table.entries.len() >= table.max_entries {
        table.stats.total_table_full += 1;
        return Err(ConntrackError::TableFull);
    }
    let tuple_reply = tuple_orig.reversed();
    let entry = ConntrackEntry::with_zone(
        *tuple_orig,
        tuple_reply,
        ConntrackState::New,
        *zone,
        current_time_ms,
    );
    table.entries.insert(key, entry);
    table.stats.total_inserts += 1;
    table.sync_stats();
    Ok(table.entries.get_mut(&key).unwrap())
}

pub fn conntrack_find_by_reply<'a>(
    table: &'a ConntrackTable,
    tuple: &ConntrackTuple,
    zone: &ConntrackZone,
) -> Option<&'a ConntrackEntry> {
    table
        .entries
        .values()
        .find(|e| e.tuple_reply == *tuple && e.zone == *zone)
}

pub fn conntrack_prune_expired(table: &mut ConntrackTable, current_time_ms: u64) -> usize {
    let before = table.entries.len();
    table
        .entries
        .retain(|_, entry| !entry.is_expired(current_time_ms));
    let pruned = before - table.entries.len();
    table.stats.total_pruned += pruned as u64;
    table.sync_stats();
    pruned
}

pub fn conntrack_prune_helper_expects(table: &mut ConntrackTable, current_time_ms: u64) -> usize {
    let before = table.expects.len();
    table
        .expects
        .retain(|_, exp| !exp.is_expired(current_time_ms));
    before - table.expects.len()
}

pub fn conntrack_expect_add(
    table: &mut ConntrackTable,
    expect: ConntrackExpect,
) -> Result<(), ConntrackError> {
    let key = ConntrackKey::from_tuple(&expect.tuple, &expect.zone);
    table.expects.insert(key, expect);
    table.stats.expect_adds += 1;
    Ok(())
}

pub fn conntrack_expect_find<'a>(
    table: &'a ConntrackTable,
    tuple: &ConntrackTuple,
    zone: &ConntrackZone,
) -> Option<&'a ConntrackExpect> {
    let key = ConntrackKey::from_tuple(tuple, zone);
    table.expects.get(&key)
}

pub fn conntrack_expect_remove(
    table: &mut ConntrackTable,
    tuple: &ConntrackTuple,
    zone: &ConntrackZone,
) -> Option<ConntrackExpect> {
    let key = ConntrackKey::from_tuple(tuple, zone);
    table.expects.remove(&key)
}

pub fn conntrack_expect_find_by_master<'a>(
    table: &'a ConntrackTable,
    master_tuple: &ConntrackTuple,
    zone: &ConntrackZone,
) -> Option<&'a ConntrackExpect> {
    table
        .expects
        .values()
        .find(|e| e.master_tuple == *master_tuple && e.zone == *zone)
}

pub fn conntrack_dump(table: &ConntrackTable) -> Vec<(ConntrackTuple, ConntrackState, u64)> {
    table
        .entries
        .values()
        .map(|e| (e.tuple_orig, e.state, e.remaining_ms(0)))
        .collect()
}

pub fn conntrack_count(table: &ConntrackTable) -> usize {
    table.entries.len()
}

pub fn conntrack_is_full(table: &ConntrackTable) -> bool {
    table.entries.len() >= table.max_entries
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tuple(src: u32, dst: u32, sport: u16, dport: u16, proto: u8) -> ConntrackTuple {
        ConntrackTuple::ipv4(
            src.to_be_bytes(),
            dst.to_be_bytes(),
            sport,
            dport,
            proto,
        )
    }

    fn make_entry(
        tuple: ConntrackTuple,
        state: ConntrackState,
        time: u64,
    ) -> ConntrackEntry {
        ConntrackEntry::new(tuple, tuple.reversed(), state, time)
    }

    #[test]
    fn test_tuple_hash_consistency() {
        let t1 = make_tuple(0xC0A80101, 0xC0A80102, 12345, 80, 6);
        let t2 = make_tuple(0xC0A80101, 0xC0A80102, 12345, 80, 6);
        assert_eq!(t1.hash64(), t2.hash64());
        assert_eq!(t1.hash32(), t2.hash32());
    }

    #[test]
    fn test_tuple_hash_differs_on_port() {
        let t1 = make_tuple(0xC0A80101, 0xC0A80102, 12345, 80, 6);
        let t2 = make_tuple(0xC0A80101, 0xC0A80102, 12345, 8080, 6);
        assert_ne!(t1.hash64(), t2.hash64());
    }

    #[test]
    fn test_tuple_hash_differs_on_proto() {
        let t1 = make_tuple(0xC0A80101, 0xC0A80102, 12345, 80, 6);
        let t2 = make_tuple(0xC0A80101, 0xC0A80102, 12345, 80, 17);
        assert_ne!(t1.hash64(), t2.hash64());
    }

    #[test]
    fn test_tuple_hash_differs_on_src_ip() {
        let t1 = make_tuple(0xC0A80101, 0xC0A80102, 12345, 80, 6);
        let t2 = make_tuple(0xC0A801FF, 0xC0A80102, 12345, 80, 6);
        assert_ne!(t1.hash64(), t2.hash64());
    }

    #[test]
    fn test_tuple_reversed() {
        let t = make_tuple(0xC0A80101, 0xC0A80102, 12345, 80, 6);
        let r = t.reversed();
        assert_eq!(r.src_ip, t.dst_ip);
        assert_eq!(r.dst_ip, t.src_ip);
        assert_eq!(r.src_port, t.dst_port);
        assert_eq!(r.dst_port, t.src_port);
        assert_eq!(r.protocol, t.protocol);
        assert_eq!(r.l3num, t.l3num);
    }

    #[test]
    fn test_tuple_ipv4_conversion() {
        let t = ConntrackTuple::ipv4([192, 168, 1, 1], [10, 0, 0, 1], 5000, 80, 6);
        assert_eq!(t.src_ip_as_ipv4(), [192, 168, 1, 1]);
        assert_eq!(t.dst_ip_as_ipv4(), [10, 0, 0, 1]);
        assert!(t.is_ipv4());
        assert!(!t.is_ipv6());
    }

    #[test]
    fn test_state_valid_transitions() {
        assert!(ConntrackState::New.is_valid_transition(ConntrackState::Established));
        assert!(ConntrackState::New.is_valid_transition(ConntrackState::Related));
        assert!(ConntrackState::New.is_valid_transition(ConntrackState::Invalid));
        assert!(ConntrackState::Established.is_valid_transition(ConntrackState::Established));
        assert!(ConntrackState::Established.is_valid_transition(ConntrackState::Reply));
        assert!(ConntrackState::Established.is_valid_transition(ConntrackState::Invalid));
        assert!(ConntrackState::Related.is_valid_transition(ConntrackState::Established));
        assert!(ConntrackState::Related.is_valid_transition(ConntrackState::Related));
        assert!(ConntrackState::Reply.is_valid_transition(ConntrackState::Established));
        assert!(ConntrackState::Reply.is_valid_transition(ConntrackState::Reply));
        assert!(ConntrackState::Invalid.is_valid_transition(ConntrackState::New));
    }

    #[test]
    fn test_state_invalid_transitions() {
        assert!(!ConntrackState::Invalid.is_valid_transition(ConntrackState::Established));
        assert!(!ConntrackState::Established.is_valid_transition(ConntrackState::New));
        assert!(!ConntrackState::New.is_valid_transition(ConntrackState::Reply));
        assert!(!ConntrackState::Reply.is_valid_transition(ConntrackState::New));
    }

    #[test]
    fn test_state_default_timeouts() {
        assert_eq!(
            ConntrackState::Established.default_timeout_ms(IPPROTO_TCP),
            TCP_ESTABLISHED_TIMEOUT_MS
        );
        assert_eq!(
            ConntrackState::New.default_timeout_ms(IPPROTO_UDP),
            UDP_TIMEOUT_MS
        );
        assert_eq!(
            ConntrackState::New.default_timeout_ms(IPPROTO_ICMP),
            ICMP_TIMEOUT_MS
        );
        assert_eq!(ConntrackState::Untracked.default_timeout_ms(IPPROTO_TCP), 0);
        assert_eq!(ConntrackState::Invalid.default_timeout_ms(IPPROTO_TCP), 0);
    }

    #[test]
    fn test_state_names() {
        assert_eq!(ConntrackState::New.name(), "NEW");
        assert_eq!(ConntrackState::Established.name(), "ESTABLISHED");
        assert_eq!(ConntrackState::Related.name(), "RELATED");
        assert_eq!(ConntrackState::Invalid.name(), "INVALID");
        assert_eq!(ConntrackState::Reply.name(), "REPLY");
        assert_eq!(ConntrackState::Untracked.name(), "UNTRACKED");
        assert_eq!(ConntrackState::Senior.name(), "SENIOR");
    }

    #[test]
    fn test_tcp_state_valid_transitions() {
        assert!(TcpConntrackState::SynSent.is_valid_transition(TcpConntrackState::SynRecv));
        assert!(TcpConntrackState::SynSent.is_valid_transition(TcpConntrackState::Close));
        assert!(TcpConntrackState::SynRecv.is_valid_transition(TcpConntrackState::Established));
        assert!(TcpConntrackState::SynRecv.is_valid_transition(TcpConntrackState::Close));
        assert!(TcpConntrackState::Established.is_valid_transition(TcpConntrackState::CloseWait));
        assert!(TcpConntrackState::Established.is_valid_transition(TcpConntrackState::Close));
        assert!(TcpConntrackState::CloseWait.is_valid_transition(TcpConntrackState::LastAck));
        assert!(TcpConntrackState::LastAck.is_valid_transition(TcpConntrackState::TimeWait));
        assert!(TcpConntrackState::TimeWait.is_valid_transition(TcpConntrackState::Close));
        assert!(TcpConntrackState::Close.is_valid_transition(TcpConntrackState::SynSent));
    }

    #[test]
    fn test_tcp_state_invalid_transitions() {
        assert!(!TcpConntrackState::SynSent.is_valid_transition(TcpConntrackState::Established));
        assert!(!TcpConntrackState::SynSent.is_valid_transition(TcpConntrackState::CloseWait));
        assert!(!TcpConntrackState::Established.is_valid_transition(TcpConntrackState::SynRecv));
        assert!(!TcpConntrackState::TimeWait.is_valid_transition(TcpConntrackState::Established));
        assert!(!TcpConntrackState::CloseWait.is_valid_transition(TcpConntrackState::SynSent));
    }

    #[test]
    fn test_tcp_state_timeouts() {
        assert_eq!(TcpConntrackState::SynSent.timeout_ms(), TCP_SYN_SENT_TIMEOUT_MS);
        assert_eq!(TcpConntrackState::SynRecv.timeout_ms(), TCP_SYN_RECV_TIMEOUT_MS);
        assert_eq!(TcpConntrackState::Established.timeout_ms(), TCP_ESTABLISHED_TIMEOUT_MS);
        assert_eq!(TcpConntrackState::CloseWait.timeout_ms(), TCP_CLOSE_WAIT_TIMEOUT_MS);
        assert_eq!(TcpConntrackState::LastAck.timeout_ms(), TCP_LAST_ACK_TIMEOUT_MS);
        assert_eq!(TcpConntrackState::TimeWait.timeout_ms(), TCP_TIME_WAIT_TIMEOUT_MS);
        assert_eq!(TcpConntrackState::Close.timeout_ms(), TCP_CLOSE_TIMEOUT_MS);
    }

    #[test]
    fn test_tcp_state_names() {
        assert_eq!(TcpConntrackState::SynSent.name(), "SYN_SENT");
        assert_eq!(TcpConntrackState::SynRecv.name(), "SYN_RECV");
        assert_eq!(TcpConntrackState::Established.name(), "ESTABLISHED");
        assert_eq!(TcpConntrackState::CloseWait.name(), "CLOSE_WAIT");
        assert_eq!(TcpConntrackState::LastAck.name(), "LAST_ACK");
        assert_eq!(TcpConntrackState::TimeWait.name(), "TIME_WAIT");
        assert_eq!(TcpConntrackState::Close.name(), "CLOSE");
        assert_eq!(TcpConntrackState::Ignore.name(), "IGNORE");
    }

    #[test]
    fn test_insert_lookup_remove() {
        let mut table = ConntrackTable::new();
        let tuple = make_tuple(0xC0A80101, 0xC0A80102, 12345, 80, 6);
        let entry = make_entry(tuple, ConntrackState::New, 0);

        conntrack_insert(&mut table, entry).unwrap();
        assert_eq!(table.len(), 1);

        let found = conntrack_lookup(&table, &tuple, &ConntrackZone::DEFAULT);
        assert!(found.is_some());
        assert_eq!(found.unwrap().state, ConntrackState::New);

        let removed = conntrack_remove(&mut table, &tuple, &ConntrackZone::DEFAULT);
        assert!(removed.is_some());
        assert!(table.is_empty());
    }

    #[test]
    fn test_lookup_miss() {
        let table = ConntrackTable::new();
        let tuple = make_tuple(0xC0A80101, 0xC0A80102, 12345, 80, 6);
        assert!(conntrack_lookup(&table, &tuple, &ConntrackZone::DEFAULT).is_none());
    }

    #[test]
    fn test_remove_nonexistent() {
        let mut table = ConntrackTable::new();
        let tuple = make_tuple(0xC0A80101, 0xC0A80102, 12345, 80, 6);
        assert!(conntrack_remove(&mut table, &tuple, &ConntrackZone::DEFAULT).is_none());
    }

    #[test]
    fn test_update_state_valid() {
        let mut table = ConntrackTable::new();
        let tuple = make_tuple(0xC0A80101, 0xC0A80102, 12345, 80, 6);
        let entry = make_entry(tuple, ConntrackState::New, 0);
        conntrack_insert(&mut table, entry).unwrap();

        conntrack_update_state(
            &mut table,
            &tuple,
            &ConntrackZone::DEFAULT,
            ConntrackState::Established,
            100,
        )
        .unwrap();

        let found = conntrack_lookup(&table, &tuple, &ConntrackZone::DEFAULT).unwrap();
        assert_eq!(found.state, ConntrackState::Established);
        assert_eq!(found.created_ms, 100);
        assert_eq!(
            found.timeout_ms,
            ConntrackState::Established.default_timeout_ms(IPPROTO_TCP)
        );
    }

    #[test]
    fn test_update_state_invalid_transition() {
        let mut table = ConntrackTable::new();
        let tuple = make_tuple(0xC0A80101, 0xC0A80102, 12345, 80, 6);
        let entry = make_entry(tuple, ConntrackState::New, 0);
        conntrack_insert(&mut table, entry).unwrap();

        let result = conntrack_update_state(
            &mut table,
            &tuple,
            &ConntrackZone::DEFAULT,
            ConntrackState::Reply,
            100,
        );
        assert_eq!(result, Err(ConntrackError::InvalidTransition));
    }

    #[test]
    fn test_update_state_not_found() {
        let mut table = ConntrackTable::new();
        let tuple = make_tuple(0xC0A80101, 0xC0A80102, 12345, 80, 6);
        let result = conntrack_update_state(
            &mut table,
            &tuple,
            &ConntrackZone::DEFAULT,
            ConntrackState::Established,
            100,
        );
        assert_eq!(result, Err(ConntrackError::NotFound));
    }

    #[test]
    fn test_timeout_pruning() {
        let mut table = ConntrackTable::new();
        let tuple = make_tuple(0xC0A80101, 0xC0A80102, 12345, 80, 6);
        let mut entry = make_entry(tuple, ConntrackState::New, 0);
        entry.timeout_ms = 1000;
        conntrack_insert(&mut table, entry).unwrap();

        let removed = conntrack_prune_expired(&mut table, 500);
        assert_eq!(removed, 0);
        assert_eq!(table.len(), 1);

        let removed = conntrack_prune_expired(&mut table, 1500);
        assert_eq!(removed, 1);
        assert!(table.is_empty());
    }

    #[test]
    fn test_timeout_pruning_multiple() {
        let mut table = ConntrackTable::new();

        let t1 = make_tuple(0xC0A80101, 0xC0A80102, 1000, 80, 6);
        let t2 = make_tuple(0xC0A80101, 0xC0A80102, 1001, 80, 6);
        let t3 = make_tuple(0xC0A80101, 0xC0A80102, 1002, 80, 6);

        let mut e1 = make_entry(t1, ConntrackState::New, 0);
        e1.timeout_ms = 100;
        let mut e2 = make_entry(t2, ConntrackState::New, 0);
        e2.timeout_ms = 200;
        let mut e3 = make_entry(t3, ConntrackState::New, 0);
        e3.timeout_ms = 300;

        conntrack_insert(&mut table, e1).unwrap();
        conntrack_insert(&mut table, e2).unwrap();
        conntrack_insert(&mut table, e3).unwrap();

        let removed = conntrack_prune_expired(&mut table, 150);
        assert_eq!(removed, 1);
        assert_eq!(table.len(), 2);

        let removed = conntrack_prune_expired(&mut table, 250);
        assert_eq!(removed, 1);
        assert_eq!(table.len(), 1);

        let removed = conntrack_prune_expired(&mut table, 350);
        assert_eq!(removed, 1);
        assert!(table.is_empty());
    }

    #[test]
    fn test_timeout_pruning_zero_never_expires() {
        let mut table = ConntrackTable::new();
        let tuple = make_tuple(0xC0A80101, 0xC0A80102, 12345, 80, 6);
        let mut entry = make_entry(tuple, ConntrackState::Untracked, 0);
        entry.timeout_ms = 0;
        conntrack_insert(&mut table, entry).unwrap();

        let removed = conntrack_prune_expired(&mut table, u64::MAX / 2);
        assert_eq!(removed, 0);
        assert_eq!(table.len(), 1);
    }

    #[test]
    fn test_zone_isolation() {
        let mut table = ConntrackTable::new();
        let tuple = make_tuple(0xC0A80101, 0xC0A80102, 12345, 80, 6);

        let zone1 = ConntrackZone::new(1, 1);
        let zone2 = ConntrackZone::new(2, 2);

        let e1 = ConntrackEntry::with_zone(
            tuple,
            tuple.reversed(),
            ConntrackState::New,
            zone1,
            0,
        );
        let e2 = ConntrackEntry::with_zone(
            tuple,
            tuple.reversed(),
            ConntrackState::Established,
            zone2,
            0,
        );

        conntrack_insert(&mut table, e1).unwrap();
        conntrack_insert(&mut table, e2).unwrap();
        assert_eq!(table.len(), 2);

        let found1 = conntrack_lookup(&table, &tuple, &zone1);
        let found2 = conntrack_lookup(&table, &tuple, &zone2);
        assert!(found1.is_some());
        assert!(found2.is_some());
        assert_eq!(found1.unwrap().state, ConntrackState::New);
        assert_eq!(found2.unwrap().state, ConntrackState::Established);

        let removed = conntrack_remove(&mut table, &tuple, &zone1);
        assert!(removed.is_some());
        assert_eq!(table.len(), 1);
        assert!(conntrack_lookup(&table, &tuple, &zone2).is_some());
    }

    #[test]
    fn test_zone_default_vs_nondefault() {
        let mut table = ConntrackTable::new();
        let tuple = make_tuple(0xC0A80101, 0xC0A80102, 12345, 80, 6);

        let e1 = make_entry(tuple, ConntrackState::New, 0);
        let e2 = ConntrackEntry::with_zone(
            tuple,
            tuple.reversed(),
            ConntrackState::Established,
            ConntrackZone::new(5, 5),
            0,
        );

        conntrack_insert(&mut table, e1).unwrap();
        conntrack_insert(&mut table, e2).unwrap();
        assert_eq!(table.len(), 2);

        assert!(conntrack_lookup(&table, &tuple, &ConntrackZone::DEFAULT).is_some());
        assert!(conntrack_lookup(&table, &tuple, &ConntrackZone::new(5, 5)).is_some());
    }

    #[test]
    fn test_max_capacity() {
        let mut table = ConntrackTable::with_capacity(2);

        let t1 = make_tuple(0xC0A80101, 0xC0A80102, 1000, 80, 6);
        let t2 = make_tuple(0xC0A80101, 0xC0A80102, 1001, 80, 6);
        let t3 = make_tuple(0xC0A80101, 0xC0A80102, 1002, 80, 6);

        conntrack_insert(&mut table, make_entry(t1, ConntrackState::New, 0)).unwrap();
        conntrack_insert(&mut table, make_entry(t2, ConntrackState::New, 0)).unwrap();

        let result = conntrack_insert(&mut table, make_entry(t3, ConntrackState::New, 0));
        assert_eq!(result, Err(ConntrackError::TableFull));
        assert_eq!(table.len(), 2);
        assert!(conntrack_is_full(&table));
    }

    #[test]
    fn test_max_capacity_after_remove() {
        let mut table = ConntrackTable::with_capacity(2);

        let t1 = make_tuple(0xC0A80101, 0xC0A80102, 1000, 80, 6);
        let t2 = make_tuple(0xC0A80101, 0xC0A80102, 1001, 80, 6);
        let t3 = make_tuple(0xC0A80101, 0xC0A80102, 1002, 80, 6);

        conntrack_insert(&mut table, make_entry(t1, ConntrackState::New, 0)).unwrap();
        conntrack_insert(&mut table, make_entry(t2, ConntrackState::New, 0)).unwrap();
        conntrack_remove(&mut table, &t1, &ConntrackZone::DEFAULT).unwrap();

        let result = conntrack_insert(&mut table, make_entry(t3, ConntrackState::New, 0));
        assert!(result.is_ok());
        assert_eq!(table.len(), 2);
    }

    #[test]
    fn test_helper_lookup_by_protocol() {
        assert!(conntrack_helper_lookup(IPPROTO_TCP).is_some());
        assert!(conntrack_helper_lookup(IPPROTO_UDP).is_some());
        assert!(conntrack_helper_lookup(50).is_none());
    }

    #[test]
    fn test_helper_lookup_by_name() {
        assert!(conntrack_helper_lookup_by_name("ftp").is_some());
        assert!(conntrack_helper_lookup_by_name("sip").is_some());
        assert!(conntrack_helper_lookup_by_name("nonexistent").is_none());
    }

    #[test]
    fn test_helper_lookup_by_port() {
        assert!(conntrack_helper_lookup_by_port(IPPROTO_TCP, 21).is_some());
        assert!(conntrack_helper_lookup_by_port(IPPROTO_TCP, 80).is_none());
        assert!(conntrack_helper_lookup_by_port(IPPROTO_UDP, 5060).is_some());
    }

    #[test]
    fn test_helper_list_nonempty() {
        assert!(!conntrack_helper_list().is_empty());
    }

    #[test]
    fn test_expect_add_find() {
        let mut table = ConntrackTable::new();
        let tuple = make_tuple(0xC0A80101, 0xC0A80102, 20, 12345, 6);
        let master = make_tuple(0xC0A80101, 0xC0A80102, 54321, 80, 6);

        let expect = ConntrackExpect::new(
            tuple,
            String::from("ftp"),
            30_000,
            master,
            0,
        );
        conntrack_expect_add(&mut table, expect).unwrap();

        let found = conntrack_expect_find(&table, &tuple, &ConntrackZone::DEFAULT);
        assert!(found.is_some());
        assert_eq!(found.unwrap().helper_name, "ftp");
        assert_eq!(table.expect_len(), 1);
    }

    #[test]
    fn test_expect_find_by_master() {
        let mut table = ConntrackTable::new();
        let tuple = make_tuple(0xC0A80101, 0xC0A80102, 20, 12345, 6);
        let master = make_tuple(0xC0A80101, 0xC0A80102, 54321, 80, 6);

        let expect = ConntrackExpect::new(
            tuple,
            String::from("ftp"),
            30_000,
            master,
            0,
        );
        conntrack_expect_add(&mut table, expect).unwrap();

        let found = conntrack_expect_find_by_master(&table, &master, &ConntrackZone::DEFAULT);
        assert!(found.is_some());
    }

    #[test]
    fn test_expect_remove() {
        let mut table = ConntrackTable::new();
        let tuple = make_tuple(0xC0A80101, 0xC0A80102, 20, 12345, 6);
        let master = make_tuple(0xC0A80101, 0xC0A80102, 54321, 80, 6);

        let expect = ConntrackExpect::new(
            tuple,
            String::from("ftp"),
            30_000,
            master,
            0,
        );
        conntrack_expect_add(&mut table, expect).unwrap();
        let removed = conntrack_expect_remove(&mut table, &tuple, &ConntrackZone::DEFAULT);
        assert!(removed.is_some());
        assert!(table.expects.is_empty());
    }

    #[test]
    fn test_expect_expiry() {
        let mut table = ConntrackTable::new();
        let tuple = make_tuple(0xC0A80101, 0xC0A80102, 20, 12345, 6);
        let master = make_tuple(0xC0A80101, 0xC0A80102, 54321, 80, 6);

        let expect = ConntrackExpect::new(
            tuple,
            String::from("ftp"),
            1000,
            master,
            0,
        );
        conntrack_expect_add(&mut table, expect).unwrap();

        let pruned = conntrack_prune_helper_expects(&mut table, 500);
        assert_eq!(pruned, 0);
        assert_eq!(table.expect_len(), 1);

        let pruned = conntrack_prune_helper_expects(&mut table, 1500);
        assert_eq!(pruned, 1);
        assert!(table.expects.is_empty());
    }

    #[test]
    fn test_get_or_create_creates() {
        let mut table = ConntrackTable::new();
        let tuple = make_tuple(0xC0A80101, 0xC0A80102, 12345, 80, 6);

        let entry = conntrack_get_or_create(&mut table, &tuple, &ConntrackZone::DEFAULT, 0).unwrap();
        assert_eq!(entry.state, ConntrackState::New);
        assert_eq!(table.len(), 1);
    }

    #[test]
    fn test_get_or_create_existing() {
        let mut table = ConntrackTable::new();
        let tuple = make_tuple(0xC0A80101, 0xC0A80102, 12345, 80, 6);

        let entry = conntrack_get_or_create(&mut table, &tuple, &ConntrackZone::DEFAULT, 0).unwrap();
        entry.state = ConntrackState::Established;

        let entry2 = conntrack_get_or_create(&mut table, &tuple, &ConntrackZone::DEFAULT, 100).unwrap();
        assert_eq!(entry2.state, ConntrackState::Established);
        assert_eq!(table.len(), 1);
    }

    #[test]
    fn test_find_by_reply() {
        let mut table = ConntrackTable::new();
        let tuple = make_tuple(0xC0A80101, 0xC0A80102, 12345, 80, 6);
        let reply = tuple.reversed();
        let entry = make_entry(tuple, ConntrackState::New, 0);
        conntrack_insert(&mut table, entry).unwrap();

        let found = conntrack_find_by_reply(&table, &reply, &ConntrackZone::DEFAULT);
        assert!(found.is_some());
        assert_eq!(found.unwrap().tuple_orig, tuple);
    }

    #[test]
    fn test_find_by_reply_not_found() {
        let table = ConntrackTable::new();
        let reply = make_tuple(0xC0A80102, 0xC0A80101, 80, 12345, 6);
        assert!(conntrack_find_by_reply(&table, &reply, &ConntrackZone::DEFAULT).is_none());
    }

    #[test]
    fn test_update_timeout() {
        let mut table = ConntrackTable::new();
        let tuple = make_tuple(0xC0A80101, 0xC0A80102, 12345, 80, 6);
        let entry = make_entry(tuple, ConntrackState::New, 0);
        conntrack_insert(&mut table, entry).unwrap();

        conntrack_update_timeout(&mut table, &tuple, &ConntrackZone::DEFAULT, 99999).unwrap();
        let found = conntrack_lookup(&table, &tuple, &ConntrackZone::DEFAULT).unwrap();
        assert_eq!(found.timeout_ms, 99999);
    }

    #[test]
    fn test_update_tcp_state() {
        let mut table = ConntrackTable::new();
        let tuple = make_tuple(0xC0A80101, 0xC0A80102, 12345, 80, 6);
        let mut entry = make_entry(tuple, ConntrackState::New, 0);
        entry.tcp_window_state = TcpConntrackState::SynSent;
        conntrack_insert(&mut table, entry).unwrap();

        conntrack_update_tcp_state(
            &mut table,
            &tuple,
            &ConntrackZone::DEFAULT,
            TcpConntrackState::SynRecv,
            100,
        )
        .unwrap();

        let found = conntrack_lookup(&table, &tuple, &ConntrackZone::DEFAULT).unwrap();
        assert_eq!(found.tcp_window_state, TcpConntrackState::SynRecv);
        assert_eq!(found.timeout_ms, TcpConntrackState::SynRecv.timeout_ms());
    }

    #[test]
    fn test_update_tcp_state_invalid() {
        let mut table = ConntrackTable::new();
        let tuple = make_tuple(0xC0A80101, 0xC0A80102, 12345, 80, 6);
        let mut entry = make_entry(tuple, ConntrackState::New, 0);
        entry.tcp_window_state = TcpConntrackState::SynSent;
        conntrack_insert(&mut table, entry).unwrap();

        let result = conntrack_update_tcp_state(
            &mut table,
            &tuple,
            &ConntrackZone::DEFAULT,
            TcpConntrackState::Established,
            100,
        );
        assert_eq!(result, Err(ConntrackError::InvalidTransition));
    }

    #[test]
    fn test_entry_expiry() {
        let tuple = make_tuple(0xC0A80101, 0xC0A80102, 12345, 80, 6);
        let mut entry = make_entry(tuple, ConntrackState::New, 1000);
        entry.timeout_ms = 500;

        assert!(!entry.is_expired(1200));
        assert!(entry.is_expired(1500));
        assert!(entry.is_expired(2000));
    }

    #[test]
    fn test_entry_remaining_ms() {
        let tuple = make_tuple(0xC0A80101, 0xC0A80102, 12345, 80, 6);
        let mut entry = make_entry(tuple, ConntrackState::New, 1000);
        entry.timeout_ms = 500;

        assert_eq!(entry.remaining_ms(1000), 500);
        assert_eq!(entry.remaining_ms(1250), 250);
        assert_eq!(entry.remaining_ms(1500), 0);
    }

    #[test]
    fn test_entry_touch() {
        let tuple = make_tuple(0xC0A80101, 0xC0A80102, 12345, 80, 6);
        let mut entry = make_entry(tuple, ConntrackState::New, 0);
        assert_eq!(entry.last_seen_ms, 0);

        entry.touch(100);
        assert_eq!(entry.last_seen_ms, 100);

        entry.touch(200);
        assert_eq!(entry.last_seen_ms, 200);
    }

    #[test]
    fn test_entry_stats() {
        let tuple = make_tuple(0xC0A80101, 0xC0A80102, 12345, 80, 6);
        let mut entry = make_entry(tuple, ConntrackState::New, 0);

        entry.inc_orig(100);
        entry.inc_orig(200);
        entry.inc_reply(50);

        assert_eq!(entry.packets_orig, 2);
        assert_eq!(entry.bytes_orig, 300);
        assert_eq!(entry.packets_reply, 1);
        assert_eq!(entry.bytes_reply, 50);
    }

    #[test]
    fn test_dump() {
        let mut table = ConntrackTable::new();
        let t1 = make_tuple(0xC0A80101, 0xC0A80102, 1000, 80, 6);
        let t2 = make_tuple(0xC0A80101, 0xC0A80102, 1001, 80, 6);

        conntrack_insert(&mut table, make_entry(t1, ConntrackState::New, 0)).unwrap();
        conntrack_insert(&mut table, make_entry(t2, ConntrackState::Established, 0)).unwrap();

        let dump = conntrack_dump(&table);
        assert_eq!(dump.len(), 2);
    }

    #[test]
    fn test_count_and_is_full() {
        let mut table = ConntrackTable::with_capacity(1);
        assert_eq!(conntrack_count(&table), 0);
        assert!(!conntrack_is_full(&table));

        let t = make_tuple(0xC0A80101, 0xC0A80102, 1000, 80, 6);
        conntrack_insert(&mut table, make_entry(t, ConntrackState::New, 0)).unwrap();

        assert_eq!(conntrack_count(&table), 1);
        assert!(conntrack_is_full(&table));
    }

    #[test]
    fn test_stats_tracking() {
        let mut table = ConntrackTable::with_capacity(3);
        let t1 = make_tuple(0xC0A80101, 0xC0A80102, 1000, 80, 6);
        let t2 = make_tuple(0xC0A80101, 0xC0A80102, 1001, 80, 6);

        conntrack_insert(&mut table, make_entry(t1, ConntrackState::New, 0)).unwrap();
        conntrack_insert(&mut table, make_entry(t2, ConntrackState::New, 0)).unwrap();

        let stats = table.stats();
        assert_eq!(stats.total_inserts, 2);
        assert_eq!(stats.current_entries, 2);

        conntrack_remove(&mut table, &t1, &ConntrackZone::DEFAULT).unwrap();
        let stats = table.stats();
        assert_eq!(stats.total_removes, 1);
        assert_eq!(stats.current_entries, 1);
    }

    #[test]
    fn test_tuple_debug_format_ipv4() {
        let t = ConntrackTuple::ipv4([192, 168, 1, 1], [10, 0, 0, 1], 5000, 80, 6);
        let debug_str = alloc::format!("{:?}", t);
        assert!(debug_str.contains("192.168.1.1:5000"));
        assert!(debug_str.contains("10.0.0.1:80"));
    }

    #[test]
    fn test_zone_default_check() {
        assert!(ConntrackZone::DEFAULT.is_default());
        assert!(ConntrackZone::new(0, 0).is_default());
        assert!(!ConntrackZone::new(1, 0).is_default());
        assert!(!ConntrackZone::new(0, 1).is_default());
    }

    #[test]
    fn test_conntrack_error_display() {
        let _ = alloc::format!("{}", ConntrackError::TableFull);
        let _ = alloc::format!("{}", ConntrackError::NotFound);
        let _ = alloc::format!("{}", ConntrackError::InvalidTransition);
    }

    #[test]
    fn test_tcp_full_handshake_simulation() {
        let mut table = ConntrackTable::new();
        let tuple = make_tuple(0xC0A80101, 0xC0A80102, 12345, 80, 6);
        let mut entry = make_entry(tuple, ConntrackState::New, 0);
        entry.tcp_window_state = TcpConntrackState::Close;
        conntrack_insert(&mut table, entry).unwrap();

        conntrack_update_tcp_state(
            &mut table,
            &tuple,
            &ConntrackZone::DEFAULT,
            TcpConntrackState::SynSent,
            100,
        )
        .unwrap();

        conntrack_update_tcp_state(
            &mut table,
            &tuple,
            &ConntrackZone::DEFAULT,
            TcpConntrackState::SynRecv,
            150,
        )
        .unwrap();

        conntrack_update_tcp_state(
            &mut table,
            &tuple,
            &ConntrackZone::DEFAULT,
            TcpConntrackState::Established,
            200,
        )
        .unwrap();

        conntrack_update_state(
            &mut table,
            &tuple,
            &ConntrackZone::DEFAULT,
            ConntrackState::Established,
            200,
        )
        .unwrap();

        let found = conntrack_lookup(&table, &tuple, &ConntrackZone::DEFAULT).unwrap();
        assert_eq!(found.tcp_window_state, TcpConntrackState::Established);
        assert_eq!(found.state, ConntrackState::Established);
    }

    #[test]
    fn test_tcp_close_simulation() {
        let mut table = ConntrackTable::new();
        let tuple = make_tuple(0xC0A80101, 0xC0A80102, 12345, 80, 6);
        let mut entry = make_entry(tuple, ConntrackState::Established, 0);
        entry.tcp_window_state = TcpConntrackState::Established;
        conntrack_insert(&mut table, entry).unwrap();

        let states = [
            TcpConntrackState::CloseWait,
            TcpConntrackState::LastAck,
            TcpConntrackState::TimeWait,
            TcpConntrackState::Close,
        ];

        let mut time = 100u64;
        for &s in &states {
            conntrack_update_tcp_state(
                &mut table,
                &tuple,
                &ConntrackZone::DEFAULT,
                s,
                time,
            )
            .unwrap();
            time += 50;
        }

        let found = conntrack_lookup(&table, &tuple, &ConntrackZone::DEFAULT).unwrap();
        assert_eq!(found.tcp_window_state, TcpConntrackState::Close);
    }
}
