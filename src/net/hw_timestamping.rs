use alloc::vec;
use alloc::vec::Vec;

macro_rules! bitflags_ts {
    ($(#[$meta:meta])* $vis:vis struct $name:ident: $ty:ty {
        $(const $flag:ident = $val:expr;)*
    }) => {
        $(#[$meta])*
        $vis struct $name($ty);

        impl $name {
            $(pub const $flag: Self = Self($val);)*

            pub const fn empty() -> Self {
                Self(0)
            }

            pub const fn from_bits_truncate(bits: $ty) -> Self {
                Self(bits)
            }

            pub const fn contains(self, other: Self) -> bool {
                (self.0 & other.0) == other.0
            }

            pub const fn bits(self) -> $ty {
                self.0
            }

            pub fn insert(&mut self, other: Self) {
                self.0 |= other.0;
            }

            pub fn remove(&mut self, other: Self) {
                self.0 &= !other.0;
            }

            pub fn is_empty(self) -> bool {
                self.0 == 0
            }

            pub fn intersects(self, other: Self) -> bool {
                (self.0 & other.0) != 0
            }
        }

        impl core::ops::BitOr for $name {
            type Output = Self;
            fn bitor(self, rhs: Self) -> Self {
                Self(self.0 | rhs.0)
            }
        }

        impl core::ops::BitAnd for $name {
            type Output = Self;
            fn bitand(self, rhs: Self) -> Self {
                Self(self.0 & rhs.0)
            }
        }

        impl core::ops::Not for $name {
            type Output = Self;
            fn not(self) -> Self {
                Self(!self.0)
            }
        }
    };
}

use super::NetError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum HwTimestampFlags {
    Software = 0x1,
    HwGen = 0x10,
    HwV2Tx = 0x20,
    HwV2Rx = 0x40,
    HwV2ComboTxRx = 0x60,
}

impl HwTimestampFlags {
    pub fn from_bits(bits: u32) -> Option<Self> {
        match bits {
            0x1 => Some(Self::Software),
            0x10 => Some(Self::HwGen),
            0x20 => Some(Self::HwV2Tx),
            0x40 => Some(Self::HwV2Rx),
            0x60 => Some(Self::HwV2ComboTxRx),
            _ => None,
        }
    }

    pub fn bits(self) -> u32 {
        self as u32
    }
}

bitflags_ts! {
    pub struct TsFlags: u32 {
        const SOFTWARE                         = 1 << 0;
        const SYS_HARDWARE                     = 1 << 1;
        const RAW_HARDWARE                     = 1 << 2;
        const ID                               = 1 << 4;
        const TX_SCHED                         = 1 << 8;
        const TX_ACK                           = 1 << 9;
        const TX_RECORD_OPTIMIZATION           = 1 << 10;
        const OPT_ID                           = 1 << 11;
        const OPT_TSONLY                       = 1 << 12;
        const OPT_STATS                        = 1 << 13;
    }
}

pub const SCM_TIMESTAMPING: u16 = 0x29;
pub const SOL_SOCKET_SO_TIMESTAMPING: i32 = 0x1046;
pub const ENOMSG: u32 = 42;

pub const SO_EE_ORIGIN_LOCAL: u8 = 0;
pub const SO_EE_ORIGIN_ICMP: u8 = 1;
pub const SO_EE_ORIGIN_ICMP6: u8 = 2;
pub const SO_EE_ORIGIN_TSTAMP: u8 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C, packed)]
pub struct SockExtendedErr {
    pub ee_errno: u32,
    pub ee_origin: u8,
    pub ee_type: u8,
    pub ee_code: u8,
    pub ee_pad: u8,
    pub ee_info: u32,
    pub ee_data: u32,
}

impl SockExtendedErr {
    pub fn new_timestamp(timestamp_type: u32) -> Self {
        Self {
            ee_errno: ENOMSG,
            ee_origin: SO_EE_ORIGIN_TSTAMP,
            ee_type: 0,
            ee_code: 0,
            ee_pad: 0,
            ee_info: timestamp_type,
            ee_data: 0,
        }
    }

    pub fn new_local(errno: u32) -> Self {
        Self {
            ee_errno: errno,
            ee_origin: SO_EE_ORIGIN_LOCAL,
            ee_type: 0,
            ee_code: 0,
            ee_pad: 0,
            ee_info: 0,
            ee_data: 0,
        }
    }

    pub fn serialize(&self, buf: &mut [u8]) -> Result<usize, NetError> {
        if buf.len() < 12 {
            return Err(NetError::BufferFull);
        }
        let bytes = unsafe {
            core::slice::from_raw_parts(
                self as *const Self as *const u8,
                12,
            )
        };
        buf[..12].copy_from_slice(bytes);
        Ok(12)
    }

    pub fn deserialize(data: &[u8]) -> Result<Self, NetError> {
        if data.len() < 12 {
            return Err(NetError::InvalidPacket);
        }
        let mut err = SockExtendedErr {
            ee_errno: 0,
            ee_origin: 0,
            ee_type: 0,
            ee_code: 0,
            ee_pad: 0,
            ee_info: 0,
            ee_data: 0,
        };
        let ptr = &mut err as *mut Self as *mut u8;
        unsafe {
            core::ptr::copy_nonoverlapping(data.as_ptr(), ptr, 12);
        }
        Ok(err)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct ScmTimestamping {
    pub ts: [PtpTime; 3],
}

impl ScmTimestamping {
    pub fn new_sw(sw: PtpTime) -> Self {
        Self {
            ts: [sw, PtpTime::default(), PtpTime::default()],
        }
    }

    pub fn new_hw(hw: PtpTime) -> Self {
        Self {
            ts: [PtpTime::default(), PtpTime::default(), hw],
        }
    }

    pub fn new_sw_and_hw(sw: PtpTime, hw: PtpTime) -> Self {
        Self {
            ts: [sw, PtpTime::default(), hw],
        }
    }

    pub fn serialize(&self, buf: &mut [u8]) -> Result<usize, NetError> {
        let needed = core::mem::size_of::<Self>();
        if buf.len() < needed {
            return Err(NetError::BufferFull);
        }
        let bytes = unsafe {
            core::slice::from_raw_parts(
                self as *const Self as *const u8,
                needed,
            )
        };
        buf[..needed].copy_from_slice(bytes);
        Ok(needed)
    }

    pub fn deserialize(data: &[u8]) -> Result<Self, NetError> {
        let needed = core::mem::size_of::<Self>();
        if data.len() < needed {
            return Err(NetError::InvalidPacket);
        }
        let mut sts = ScmTimestamping {
            ts: [PtpTime::default(); 3],
        };
        let ptr = &mut sts as *mut Self as *mut u8;
        unsafe {
            core::ptr::copy_nonoverlapping(data.as_ptr(), ptr, needed);
        }
        Ok(sts)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct PtpTime {
    pub seconds: u64,
    pub nanoseconds: u32,
}

const NSEC_PER_SEC: u64 = 1_000_000_000;

impl PtpTime {
    pub const fn new(seconds: u64, nanoseconds: u32) -> Self {
        Self { seconds, nanoseconds }
    }

    pub const fn zero() -> Self {
        Self { seconds: 0, nanoseconds: 0 }
    }

    pub fn add_ns(self, ns: i64) -> PtpTime {
        if ns >= 0 {
            let add_sec = (ns as u64) / NSEC_PER_SEC;
            let add_nsec = (ns as u64) % NSEC_PER_SEC;
            let total_nsec = self.nanoseconds as u64 + add_nsec;
            if total_nsec >= NSEC_PER_SEC {
                PtpTime {
                    seconds: self.seconds + add_sec + 1,
                    nanoseconds: (total_nsec - NSEC_PER_SEC) as u32,
                }
            } else {
                PtpTime {
                    seconds: self.seconds + add_sec,
                    nanoseconds: total_nsec as u32,
                }
            }
        } else {
            let sub = (-ns) as u64;
            let sub_sec = sub / NSEC_PER_SEC;
            let sub_nsec = sub % NSEC_PER_SEC;
            let total_sec = self.seconds + add_ns_seconds(self.seconds, sub_sec);
            let ns_borrow = if self.nanoseconds as u64 >= sub_nsec {
                self.nanoseconds as u64 - sub_nsec
            } else {
                return borrow_and_sub(self.seconds, self.nanoseconds, sub);
            };
            if ns_borrow < NSEC_PER_SEC {
                PtpTime {
                    seconds: total_sec,
                    nanoseconds: ns_borrow as u32,
                }
            } else {
                PtpTime {
                    seconds: total_sec + 1,
                    nanoseconds: (ns_borrow - NSEC_PER_SEC) as u32,
                }
            }
        }
    }

    pub fn sub_time(self, other: PtpTime) -> i64 {
        let self_ns = self.seconds as i128 * NSEC_PER_SEC as i128 + self.nanoseconds as i128;
        let other_ns = other.seconds as i128 * NSEC_PER_SEC as i128 + other.nanoseconds as i128;
        (self_ns - other_ns) as i64
    }

    pub fn to_ns(self) -> u128 {
        self.seconds as u128 * NSEC_PER_SEC as u128 + self.nanoseconds as u128
    }

    pub fn from_ns(total: u128) -> Self {
        let sec = (total / NSEC_PER_SEC as u128) as u64;
        let nsec = (total % NSEC_PER_SEC as u128) as u32;
        PtpTime { seconds: sec, nanoseconds: nsec }
    }

    pub fn serialize(&self, buf: &mut [u8]) -> Result<usize, NetError> {
        if buf.len() < 12 {
            return Err(NetError::BufferFull);
        }
        buf[..8].copy_from_slice(&self.seconds.to_ne_bytes());
        buf[8..12].copy_from_slice(&self.nanoseconds.to_ne_bytes());
        Ok(12)
    }

    pub fn deserialize(data: &[u8]) -> Result<Self, NetError> {
        if data.len() < 12 {
            return Err(NetError::InvalidPacket);
        }
        let seconds = u64::from_ne_bytes(data[..8].try_into().map_err(|_| NetError::InvalidPacket)?);
        let nanoseconds = u32::from_ne_bytes(data[8..12].try_into().map_err(|_| NetError::InvalidPacket)?);
        Ok(PtpTime { seconds, nanoseconds })
    }
}

fn add_ns_seconds(_current_sec: u64, sub_sec: u64) -> u64 {
    sub_sec
}

fn borrow_and_sub(seconds: u64, nanoseconds: u32, sub: u64) -> PtpTime {
    let total_ns = seconds as i128 * NSEC_PER_SEC as i128 + nanoseconds as i128 - sub as i128;
    if total_ns < 0 {
        PtpTime::zero()
    } else {
        let ns = total_ns as u64;
        PtpTime {
            seconds: (ns / NSEC_PER_SEC) as u64,
            nanoseconds: (ns % NSEC_PER_SEC) as u32,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum TxTsType {
    Off = 0,
    On = 1,
    OnestepP2p = 2,
    Onestep = 3,
}

impl TxTsType {
    pub fn from_u32(val: u32) -> Option<Self> {
        match val {
            0 => Some(Self::Off),
            1 => Some(Self::On),
            2 => Some(Self::OnestepP2p),
            3 => Some(Self::Onestep),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum RxTsFilter {
    None = 0,
    PtpV1L4Event = 1,
    PtpV1L4Sync = 2,
    PtpV1L4DelayReq = 3,
    PtpV2L4Event = 4,
    PtpV2L4Sync = 5,
    PtpV2L4DelayReq = 6,
    PtpV2L2Event = 7,
    PtpV2L2Sync = 8,
    PtpV2L2DelayReq = 9,
    PtpV2Event = 10,
    PtpV2Sync = 11,
    PtpV2DelayReq = 12,
    All = 0xFFFF,
}

impl RxTsFilter {
    pub fn from_u32(val: u32) -> Option<Self> {
        match val {
            0 => Some(Self::None),
            1 => Some(Self::PtpV1L4Event),
            2 => Some(Self::PtpV1L4Sync),
            3 => Some(Self::PtpV1L4DelayReq),
            4 => Some(Self::PtpV2L4Event),
            5 => Some(Self::PtpV2L4Sync),
            6 => Some(Self::PtpV2L4DelayReq),
            7 => Some(Self::PtpV2L2Event),
            8 => Some(Self::PtpV2L2Sync),
            9 => Some(Self::PtpV2L2DelayReq),
            10 => Some(Self::PtpV2Event),
            11 => Some(Self::PtpV2Sync),
            12 => Some(Self::PtpV2DelayReq),
            0xFFFF => Some(Self::All),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HwTimestampCaps {
    pub tx_types: Vec<TxTsType>,
    pub rx_filters: Vec<RxTsFilter>,
}

impl HwTimestampCaps {
    pub fn new() -> Self {
        Self {
            tx_types: Vec::new(),
            rx_filters: Vec::new(),
        }
    }

    pub fn supports_tx(&self, tx: TxTsType) -> bool {
        self.tx_types.contains(&tx)
    }

    pub fn supports_rx(&self, rx: RxTsFilter) -> bool {
        self.rx_filters.contains(&rx)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PtpClockMode {
    FreqAdj,
    PhaseAdj,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PtpClock {
    pub clock_id: u32,
    pub counter: u64,
    pub mult: u32,
    pub shift: u32,
    pub mode: PtpClockMode,
}

impl PtpClock {
    pub fn new(clock_id: u32) -> Self {
        Self {
            clock_id,
            counter: 0,
            mult: 1 << 16,
            shift: 0,
            mode: PtpClockMode::FreqAdj,
        }
    }

    pub fn with_mode(clock_id: u32, mode: PtpClockMode) -> Self {
        Self {
            clock_id,
            counter: 0,
            mult: 1 << 16,
            shift: 0,
            mode,
        }
    }
}

pub struct PtpClockRegistry {
    clocks: Vec<PtpClock>,
}

static mut PTP_REGISTRY: Option<PtpClockRegistry> = None;

impl PtpClockRegistry {
    fn get() -> &'static mut PtpClockRegistry {
        unsafe {
            if PTP_REGISTRY.is_none() {
                PTP_REGISTRY = Some(PtpClockRegistry { clocks: Vec::new() });
            }
            PTP_REGISTRY.as_mut().unwrap()
        }
    }

    pub fn register(&mut self, clock: PtpClock) {
        if !self.clocks.iter().any(|c| c.clock_id == clock.clock_id) {
            self.clocks.push(clock);
        }
    }

    pub fn get_clock(&self, clock_id: u32) -> Option<&PtpClock> {
        self.clocks.iter().find(|c| c.clock_id == clock_id)
    }

    pub fn get_clock_mut(&mut self, clock_id: u32) -> Option<&mut PtpClock> {
        self.clocks.iter_mut().find(|c| c.clock_id == clock_id)
    }

    pub fn unregister(&mut self, clock_id: u32) -> bool {
        let len_before = self.clocks.len();
        self.clocks.retain(|c| c.clock_id != clock_id);
        self.clocks.len() < len_before
    }
}

pub fn register_ptp_clock(clock: PtpClock) {
    PtpClockRegistry::get().register(clock);
}

pub fn unregister_ptp_clock(clock_id: u32) -> bool {
    PtpClockRegistry::get().unregister(clock_id)
}

pub fn ptp_clock_gettime(clock_id: u32) -> Result<PtpTime, NetError> {
    let registry = PtpClockRegistry::get();
    let clock = registry.get_clock(clock_id).ok_or(NetError::InvalidParam)?;
    Ok(PtpTime {
        seconds: clock.counter,
        nanoseconds: 0,
    })
}

pub fn ptp_clock_settime(clock_id: u32, time: PtpTime) -> Result<(), NetError> {
    let registry = PtpClockRegistry::get();
    let clock = registry.get_clock_mut(clock_id).ok_or(NetError::InvalidParam)?;
    clock.counter = time.seconds;
    Ok(())
}

pub fn ptp_clock_adjtime(clock_id: u32, delta_ns: i64) -> Result<(), NetError> {
    let registry = PtpClockRegistry::get();
    let clock = registry.get_clock_mut(clock_id).ok_or(NetError::InvalidParam)?;
    match clock.mode {
        PtpClockMode::FreqAdj => {
            let delta_sec = (delta_ns.unsigned_abs() / NSEC_PER_SEC) as u64;
            if delta_ns >= 0 {
                clock.counter = clock.counter.wrapping_add(delta_sec);
            } else {
                clock.counter = clock.counter.wrapping_sub(delta_sec);
            }
        }
        PtpClockMode::PhaseAdj => {
            let time = PtpTime {
                seconds: clock.counter,
                nanoseconds: 0,
            };
            let adjusted = time.add_ns(delta_ns);
            clock.counter = adjusted.seconds;
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HwTimestampConfig {
    pub tx_type: TxTsType,
    pub rx_filter: RxTsFilter,
    pub v1_events: bool,
    pub v2_events: bool,
    pub fip_events: bool,
}

impl HwTimestampConfig {
    pub fn disabled() -> Self {
        Self {
            tx_type: TxTsType::Off,
            rx_filter: RxTsFilter::None,
            v1_events: false,
            v2_events: false,
            fip_events: false,
        }
    }

    pub fn is_disabled(&self) -> bool {
        self.tx_type == TxTsType::Off && self.rx_filter == RxTsFilter::None
    }
}

pub fn hwtstamp_validate_config(config: HwTimestampConfig) -> Result<HwTimestampConfig, NetError> {
    let mut out = config;
    match out.tx_type {
        TxTsType::Off | TxTsType::On | TxTsType::OnestepP2p | TxTsType::Onestep => {}
        _ => return Err(NetError::InvalidParam),
    }
    match out.rx_filter {
        RxTsFilter::None | RxTsFilter::PtpV1L4Event | RxTsFilter::PtpV1L4Sync
        | RxTsFilter::PtpV1L4DelayReq | RxTsFilter::PtpV2L4Event | RxTsFilter::PtpV2L4Sync
        | RxTsFilter::PtpV2L4DelayReq | RxTsFilter::PtpV2L2Event | RxTsFilter::PtpV2L2Sync
        | RxTsFilter::PtpV2L2DelayReq | RxTsFilter::PtpV2Event | RxTsFilter::PtpV2Sync
        | RxTsFilter::PtpV2DelayReq | RxTsFilter::All => {}
        _ => return Err(NetError::InvalidParam),
    }
    if out.rx_filter != RxTsFilter::None {
        match out.rx_filter {
            RxTsFilter::PtpV1L4Event | RxTsFilter::PtpV1L4Sync | RxTsFilter::PtpV1L4DelayReq => {
                out.v1_events = true;
                out.v2_events = false;
            }
            RxTsFilter::PtpV2L4Event | RxTsFilter::PtpV2L4Sync | RxTsFilter::PtpV2L4DelayReq
            | RxTsFilter::PtpV2L2Event | RxTsFilter::PtpV2L2Sync | RxTsFilter::PtpV2L2DelayReq
            | RxTsFilter::PtpV2Event | RxTsFilter::PtpV2Sync | RxTsFilter::PtpV2DelayReq => {
                out.v1_events = false;
                out.v2_events = true;
            }
            RxTsFilter::All => {
                out.v1_events = true;
                out.v2_events = true;
            }
            _ => {}
        }
    }
    if out.tx_type != TxTsType::Off && out.rx_filter == RxTsFilter::None {
        return Err(NetError::InvalidParam);
    }
    Ok(out)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HwTimestampMeta {
    pub hwtstamp: PtpTime,
    pub sw_timestamp: bool,
    pub hw_timestamp: bool,
    pub tx_flags: u32,
}

pub fn extract_hardware_timestamp(packet: &[u8], hw_timestamp: &HwTimestampMeta) -> Option<PtpTime> {
    if packet.len() < 42 {
        return None;
    }
    if !hw_timestamp.hw_timestamp {
        return None;
    }
    let ether_type = u16::from_be_bytes([packet[12], packet[13]]);
    match ether_type {
        0x0800 => extract_ipv4_timestamp(packet, hw_timestamp),
        0x86DD => extract_ipv6_timestamp(packet, hw_timestamp),
        0x8100 => {
            if packet.len() < 46 {
                return None;
            }
            let inner_type = u16::from_be_bytes([packet[16], packet[17]]);
            match inner_type {
                0x0800 => extract_ipv4_timestamp(packet, hw_timestamp),
                0x86DD => extract_ipv6_timestamp(packet, hw_timestamp),
                _ => None,
            }
        }
        _ => None,
    }
}

fn extract_ipv4_timestamp(packet: &[u8], hw_timestamp: &HwTimestampMeta) -> Option<PtpTime> {
    if packet.len() < 42 {
        return None;
    }
    let ip_header_len = ((packet[14] & 0x0F) as usize) * 4;
    let ip_total_offset = 14 + ip_header_len;
    if packet.len() < ip_total_offset + 8 {
        return None;
    }
    let protocol = packet[23];
    let udp_offset = ip_total_offset;
    match protocol {
        17 => {
            if packet.len() < udp_offset + 8 {
                return None;
            }
            let dst_port = u16::from_be_bytes([packet[udp_offset + 2], packet[udp_offset + 3]]);
            if dst_port == 319 || dst_port == 320 {
                Some(hw_timestamp.hwtstamp)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn extract_ipv6_timestamp(packet: &[u8], hw_timestamp: &HwTimestampMeta) -> Option<PtpTime> {
    if packet.len() < 54 {
        return None;
    }
    let next_header = packet[20];
    let mut offset = 40;
    let mut hdr = next_header;
    loop {
        match hdr {
            17 => {
                if packet.len() < offset + 8 {
                    return None;
                }
                let dst_port = u16::from_be_bytes([packet[offset + 2], packet[offset + 3]]);
                if dst_port == 319 || dst_port == 320 {
                    return Some(hw_timestamp.hwtstamp);
                }
                return None;
            }
            44 => {
                offset += 8;
                if offset >= packet.len() {
                    return None;
                }
                hdr = packet[offset];
                offset += 1;
            }
            60 => {
                offset += 2;
                if offset >= packet.len() {
                    return None;
                }
                hdr = packet[offset];
                offset += 1;
            }
            59 | 0 => return None,
            _ => {
                offset += 40;
                if offset >= packet.len() {
                    return None;
                }
                hdr = packet[offset];
                offset += 1;
            }
        }
    }
}

pub fn insert_software_timestamp(packet: &mut [u8], ptp_time: PtpTime) -> Result<(), NetError> {
    if packet.len() < 42 {
        return Err(NetError::InvalidPacket);
    }
    let ether_type = u16::from_be_bytes([packet[12], packet[13]]);
    match ether_type {
        0x0800 => insert_ipv4_software_timestamp(packet, ptp_time),
        0x86DD => insert_ipv6_software_timestamp(packet, ptp_time),
        0x8100 => {
            if packet.len() < 46 {
                return Err(NetError::InvalidPacket);
            }
            let inner_type = u16::from_be_bytes([packet[16], packet[17]]);
            match inner_type {
                0x0800 => insert_ipv4_software_timestamp(packet, ptp_time),
                0x86DD => insert_ipv6_software_timestamp(packet, ptp_time),
                _ => Err(NetError::NotSupported),
            }
        }
        _ => Err(NetError::NotSupported),
    }
}

fn insert_ipv4_software_timestamp(packet: &mut [u8], ptp_time: PtpTime) -> Result<(), NetError> {
    if packet.len() < 42 {
        return Err(NetError::InvalidPacket);
    }
    let ip_header_len = ((packet[14] & 0x0F) as usize) * 4;
    let ip_total_offset = 14 + ip_header_len;
    if packet.len() < ip_total_offset + 8 {
        return Err(NetError::InvalidPacket);
    }
    let protocol = packet[23];
    let udp_offset = ip_total_offset;
    match protocol {
        17 => {
            if packet.len() < udp_offset + 8 {
                return Err(NetError::InvalidPacket);
            }
            let dst_port = u16::from_be_bytes([packet[udp_offset + 2], packet[udp_offset + 3]]);
            if dst_port == 319 || dst_port == 320 {
                let ptp_offset = udp_offset + 8;
                if packet.len() >= ptp_offset + 10 {
                    let seconds = ptp_time.seconds;
                    let nanos = ptp_time.nanoseconds;
                    packet[ptp_offset + 4..ptp_offset + 6].copy_from_slice(&((seconds >> 32) as u16).to_be_bytes());
                    packet[ptp_offset + 6..ptp_offset + 10].copy_from_slice(&(seconds as u32).to_be_bytes());
                    packet[ptp_offset + 10..ptp_offset + 14].copy_from_slice(&nanos.to_be_bytes());
                    Ok(())
                } else {
                    Err(NetError::BufferFull)
                }
            } else {
                Ok(())
            }
        }
        _ => Ok(()),
    }
}

fn insert_ipv6_software_timestamp(packet: &mut [u8], ptp_time: PtpTime) -> Result<(), NetError> {
    if packet.len() < 54 {
        return Err(NetError::InvalidPacket);
    }
    let next_header = packet[20];
    let mut offset = 40;
    let mut hdr = next_header;
    loop {
        match hdr {
            17 => {
                if packet.len() < offset + 8 {
                    return Err(NetError::InvalidPacket);
                }
                let dst_port = u16::from_be_bytes([packet[offset + 2], packet[offset + 3]]);
                if dst_port == 319 || dst_port == 320 {
                    let ptp_offset = offset + 8;
                    if packet.len() >= ptp_offset + 10 {
                        let seconds = ptp_time.seconds;
                        let nanos = ptp_time.nanoseconds;
                        packet[ptp_offset + 4..ptp_offset + 6].copy_from_slice(&((seconds >> 32) as u16).to_be_bytes());
                        packet[ptp_offset + 6..ptp_offset + 10].copy_from_slice(&(seconds as u32).to_be_bytes());
                        packet[ptp_offset + 10..ptp_offset + 12].copy_from_slice(&nanos.to_be_bytes());
                        return Ok(());
                    } else {
                        return Err(NetError::BufferFull);
                    }
                }
                return Ok(());
            }
            44 => {
                offset += 8;
                if offset >= packet.len() {
                    return Err(NetError::InvalidPacket);
                }
                hdr = packet[offset];
                offset += 1;
            }
            60 => {
                offset += 2;
                if offset >= packet.len() {
                    return Err(NetError::InvalidPacket);
                }
                hdr = packet[offset];
                offset += 1;
            }
            59 | 0 => return Ok(()),
            _ => {
                offset += 40;
                if offset >= packet.len() {
                    return Err(NetError::InvalidPacket);
                }
                hdr = packet[offset];
                offset += 1;
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HwtstampConfigRaw {
    pub flags: u32,
    pub tx_type: u32,
    pub rx_filter: u32,
}

impl HwtstampConfigRaw {
    pub fn serialize(&self, buf: &mut [u8]) -> Result<usize, NetError> {
        if buf.len() < 12 {
            return Err(NetError::BufferFull);
        }
        buf[..4].copy_from_slice(&self.flags.to_ne_bytes());
        buf[4..8].copy_from_slice(&self.tx_type.to_ne_bytes());
        buf[8..12].copy_from_slice(&self.rx_filter.to_ne_bytes());
        Ok(12)
    }

    pub fn deserialize(data: &[u8]) -> Result<Self, NetError> {
        if data.len() < 12 {
            return Err(NetError::InvalidPacket);
        }
        Ok(Self {
            flags: u32::from_ne_bytes(data[..4].try_into().map_err(|_| NetError::InvalidPacket)?),
            tx_type: u32::from_ne_bytes(data[4..8].try_into().map_err(|_| NetError::InvalidPacket)?),
            rx_filter: u32::from_ne_bytes(data[8..12].try_into().map_err(|_| NetError::InvalidPacket)?),
        })
    }

    pub fn to_config(&self) -> Result<HwTimestampConfig, NetError> {
        let tx = TxTsType::from_u32(self.tx_type).ok_or(NetError::InvalidParam)?;
        let rx = RxTsFilter::from_u32(self.rx_filter).ok_or(NetError::InvalidParam)?;
        let config = HwTimestampConfig {
            tx_type: tx,
            rx_filter: rx,
            v1_events: false,
            v2_events: false,
            fip_events: false,
        };
        hwtstamp_validate_config(config)
    }
}

pub const PTP_CLOCK_EXTTS: u32 = 0;
pub const PTP_CLOCK_PPS: u32 = 1;

pub const PTP_PIN_FUNCTION_NONE: u32 = 0;
pub const PTP_PIN_FUNCTION_PEROUT: u32 = 1;
pub const PTP_PIN_FUNCTION_EXTTS: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PtpPinDesc {
    pub index: u32,
    pub func: u32,
    pub chan: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PtpExttsEvent {
    pub index: u32,
    pub timestamp: PtpTime,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PtpSysOffset {
    pub sec: u64,
    pub nsec: u32,
    pub pin: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PtpSysOffsetPrecise {
    pub device_time: PtpTime,
    pub sys_time: PtpTime,
}

impl PtpSysOffsetPrecise {
    pub fn estimate_offset(&self) -> i64 {
        self.sys_time.sub_time(self.device_time)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SkbSharedHwtstamps {
    pub hwtstamp: PtpTime,
    pub swstamp: Option<PtpTime>,
}

impl SkbSharedHwtstamps {
    pub fn new(hwtstamp: PtpTime) -> Self {
        Self {
            hwtstamp,
            swstamp: None,
        }
    }

    pub fn with_swstamp(mut self, swstamp: PtpTime) -> Self {
        self.swstamp = Some(swstamp);
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScmTstamp {
    Snd = 0,
    SndSw = 1,
    SndHw = 2,
    RcvSw = 3,
    RcvHw = 4,
}

impl ScmTstamp {
    pub fn from_u32(val: u32) -> Option<Self> {
        match val {
            0 => Some(Self::Snd),
            1 => Some(Self::SndSw),
            2 => Some(Self::SndHw),
            3 => Some(Self::RcvSw),
            4 => Some(Self::RcvHw),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimestampNotification {
    pub err: SockExtendedErr,
    pub sts: ScmTimestamping,
}

impl TimestampNotification {
    pub fn new_tx_sw(ts: PtpTime, tskey: u32) -> Self {
        let mut err = SockExtendedErr::new_timestamp(ScmTstamp::SndSw as u32);
        err.ee_data = tskey;
        Self {
            err,
            sts: ScmTimestamping::new_sw(ts),
        }
    }

    pub fn new_tx_hw(ts: PtpTime, tskey: u32) -> Self {
        let mut err = SockExtendedErr::new_timestamp(ScmTstamp::SndHw as u32);
        err.ee_data = tskey;
        Self {
            err,
            sts: ScmTimestamping::new_hw(ts),
        }
    }

    pub fn new_rx_sw(ts: PtpTime) -> Self {
        Self {
            err: SockExtendedErr::new_timestamp(ScmTstamp::RcvSw as u32),
            sts: ScmTimestamping::new_sw(ts),
        }
    }

    pub fn new_rx_hw(ts: PtpTime) -> Self {
        Self {
            err: SockExtendedErr::new_timestamp(ScmTstamp::RcvHw as u32),
            sts: ScmTimestamping::new_hw(ts),
        }
    }

    pub fn serialize(&self, buf: &mut [u8]) -> Result<usize, NetError> {
        let needed = 12 + core::mem::size_of::<ScmTimestamping>();
        if buf.len() < needed {
            return Err(NetError::BufferFull);
        }
        let err_size = self.err.serialize(buf)?;
        let sts_size = self.sts.serialize(&mut buf[err_size..])?;
        Ok(err_size + sts_size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ptp_time_add_ns_positive_within_second() {
        let t = PtpTime::new(100, 500_000_000);
        let result = t.add_ns(300_000_000);
        assert_eq!(result.seconds, 100);
        assert_eq!(result.nanoseconds, 800_000_000);
    }

    #[test]
    fn ptp_time_add_ns_wraparound() {
        let t = PtpTime::new(100, 900_000_000);
        let result = t.add_ns(200_000_000);
        assert_eq!(result.seconds, 101);
        assert_eq!(result.nanoseconds, 100_000_000);
    }

    #[test]
    fn ptp_time_add_ns_large_wraparound() {
        let t = PtpTime::new(100, 999_999_999);
        let result = t.add_ns(1);
        assert_eq!(result.seconds, 101);
        assert_eq!(result.nanoseconds, 0);
    }

    #[test]
    fn ptp_time_add_ns_exact_second_boundary() {
        let t = PtpTime::new(50, 0);
        let result = t.add_ns(1_000_000_000);
        assert_eq!(result.seconds, 51);
        assert_eq!(result.nanoseconds, 0);
    }

    #[test]
    fn ptp_time_add_ns_zero() {
        let t = PtpTime::new(42, 123_456_789);
        let result = t.add_ns(0);
        assert_eq!(result.seconds, 42);
        assert_eq!(result.nanoseconds, 123_456_789);
    }

    #[test]
    fn ptp_time_add_ns_negative() {
        let t = PtpTime::new(100, 500_000_000);
        let result = t.add_ns(-200_000_000);
        assert_eq!(result.seconds, 100);
        assert_eq!(result.nanoseconds, 300_000_000);
    }

    #[test]
    fn ptp_time_add_ns_negative_borrow() {
        let t = PtpTime::new(100, 100_000_000);
        let result = t.add_ns(-200_000_000);
        assert_eq!(result.seconds, 99);
        assert_eq!(result.nanoseconds, 900_000_000);
    }

    #[test]
    fn ptp_time_sub_time() {
        let a = PtpTime::new(100, 500_000_000);
        let b = PtpTime::new(100, 300_000_000);
        assert_eq!(a.sub_time(b), 200_000_000);
    }

    #[test]
    fn ptp_time_sub_time_cross_second() {
        let a = PtpTime::new(101, 100_000_000);
        let b = PtpTime::new(100, 900_000_000);
        assert_eq!(a.sub_time(b), 200_000_000);
    }

    #[test]
    fn ts_flags_parsing() {
        let flags = TsFlags::from_bits_truncate(
            TsFlags::SOFTWARE.bits() | TsFlags::OPT_ID.bits() | TsFlags::OPT_TSONLY.bits(),
        );
        let b = flags.bits();
        assert!(b & TsFlags::SOFTWARE.bits() == TsFlags::SOFTWARE.bits());
        assert!(b & TsFlags::OPT_ID.bits() == TsFlags::OPT_ID.bits());
        assert!(b & TsFlags::OPT_TSONLY.bits() == TsFlags::OPT_TSONLY.bits());
        assert!(b & TsFlags::RAW_HARDWARE.bits() == 0);
    }

    #[test]
    fn ts_flags_tx_generation_mask() {
        let tx_gen = TsFlags::from_bits_truncate(
            TsFlags::TX_SCHED.bits() | TsFlags::TX_ACK.bits(),
        );
        let b = tx_gen.bits();
        assert!(b & TsFlags::TX_SCHED.bits() != 0);
        assert!(b & TsFlags::TX_ACK.bits() != 0);
        assert!(b & TsFlags::SOFTWARE.bits() == 0);
    }

    #[test]
    fn ts_flags_empty() {
        let flags = TsFlags::empty();
        let b = flags.bits();
        assert!(b == 0);
        assert_eq!(b, 0);
    }

    #[test]
    fn hwtstamp_config_valid() {
        let config = HwTimestampConfig {
            tx_type: TxTsType::On,
            rx_filter: RxTsFilter::PtpV2L4Event,
            v1_events: false,
            v2_events: false,
            fip_events: false,
        };
        let validated = hwtstamp_validate_config(config).unwrap();
        assert_eq!(validated.tx_type, TxTsType::On);
        assert_eq!(validated.rx_filter, RxTsFilter::PtpV2L4Event);
        assert!(!validated.v1_events);
        assert!(validated.v2_events);
    }

    #[test]
    fn hwtstamp_config_disabled() {
        let config = HwTimestampConfig::disabled();
        let validated = hwtstamp_validate_config(config).unwrap();
        assert!(validated.is_disabled());
    }

    #[test]
    fn hwtstamp_config_v1_filter() {
        let config = HwTimestampConfig {
            tx_type: TxTsType::On,
            rx_filter: RxTsFilter::PtpV1L4Event,
            v1_events: false,
            v2_events: false,
            fip_events: false,
        };
        let validated = hwtstamp_validate_config(config).unwrap();
        assert!(validated.v1_events);
        assert!(!validated.v2_events);
    }

    #[test]
    fn hwtstamp_config_all_filter() {
        let config = HwTimestampConfig {
            tx_type: TxTsType::On,
            rx_filter: RxTsFilter::All,
            v1_events: false,
            v2_events: false,
            fip_events: false,
        };
        let validated = hwtstamp_validate_config(config).unwrap();
        assert!(validated.v1_events);
        assert!(validated.v2_events);
    }

    #[test]
    fn hwtstamp_config_tx_without_rx_fails() {
        let config = HwTimestampConfig {
            tx_type: TxTsType::On,
            rx_filter: RxTsFilter::None,
            v1_events: false,
            v2_events: false,
            fip_events: false,
        };
        assert_eq!(hwtstamp_validate_config(config), Err(NetError::InvalidParam));
    }

    #[test]
    fn hwtstamp_config_raw_roundtrip() {
        let raw = HwtstampConfigRaw {
            flags: 0,
            tx_type: TxTsType::On as u32,
            rx_filter: RxTsFilter::PtpV2L4Event as u32,
        };
        let mut buf = [0u8; 12];
        let size = raw.serialize(&mut buf).unwrap();
        assert_eq!(size, 12);
        let deserialized = HwtstampConfigRaw::deserialize(&buf).unwrap();
        assert_eq!(deserialized.flags, 0);
        assert_eq!(deserialized.tx_type, TxTsType::On as u32);
        assert_eq!(deserialized.rx_filter, RxTsFilter::PtpV2L4Event as u32);
    }

    #[test]
    fn sock_extended_err_timestamp() {
        let err = SockExtendedErr::new_timestamp(ScmTstamp::SndHw as u32);
        let mut buf = [0u8; 12];
        let size = err.serialize(&mut buf).unwrap();
        assert_eq!(size, 12);
        let ee_errno = err.ee_errno;
        let ee_origin = err.ee_origin;
        let ee_info = err.ee_info;
        assert_eq!(ee_errno, ENOMSG);
        assert_eq!(ee_origin, SO_EE_ORIGIN_TSTAMP);
        assert_eq!(ee_info, 2);
        let deserialized = SockExtendedErr::deserialize(&buf).unwrap();
        let de_errno = deserialized.ee_errno;
        let de_origin = deserialized.ee_origin;
        let de_info = deserialized.ee_info;
        assert_eq!(de_errno, ENOMSG);
        assert_eq!(de_origin, SO_EE_ORIGIN_TSTAMP);
        assert_eq!(de_info, 2);
    }

    #[test]
    fn sock_extended_err_local() {
        let err = SockExtendedErr::new_local(111);
        let ee_errno = err.ee_errno;
        let ee_origin = err.ee_origin;
        assert_eq!(ee_errno, 111);
        assert_eq!(ee_origin, SO_EE_ORIGIN_LOCAL);
    }

    #[test]
    fn scm_timestamping_new_sw() {
        let sts = ScmTimestamping::new_sw(PtpTime::new(100, 200));
        assert_eq!(sts.ts[0], PtpTime::new(100, 200));
        assert_eq!(sts.ts[1], PtpTime::default());
        assert_eq!(sts.ts[2], PtpTime::default());
    }

    #[test]
    fn scm_timestamping_new_hw() {
        let sts = ScmTimestamping::new_hw(PtpTime::new(200, 300));
        assert_eq!(sts.ts[0], PtpTime::default());
        assert_eq!(sts.ts[2], PtpTime::new(200, 300));
    }

    #[test]
    fn scm_timestamping_roundtrip() {
        let sts = ScmTimestamping::new_sw_and_hw(PtpTime::new(10, 20), PtpTime::new(30, 40));
        let mut buf = [0u8; 48];
        let size = sts.serialize(&mut buf).unwrap();
        assert_eq!(size, 48);
        let deserialized = ScmTimestamping::deserialize(&buf).unwrap();
        assert_eq!(deserialized.ts[0], PtpTime::new(10, 20));
        assert_eq!(deserialized.ts[2], PtpTime::new(30, 40));
    }

    #[test]
    fn ptp_time_to_ns_and_back() {
        let t = PtpTime::new(123, 456_789_012);
        let ns = t.to_ns();
        let t2 = PtpTime::from_ns(ns);
        assert_eq!(t, t2);
    }

    #[test]
    fn ptp_clock_adjtime_freq() {
        let mut clock = PtpClock::with_mode(1, PtpClockMode::FreqAdj);
        clock.counter = 1000;
        register_ptp_clock(clock);
        ptp_clock_adjtime(1, 5000).unwrap();
        let time = ptp_clock_gettime(1).unwrap();
        assert_eq!(time.seconds, 1000);
        unregister_ptp_clock(1);
    }

    #[test]
    fn ptp_clock_set_get_time() {
        let clock = PtpClock::new(99);
        register_ptp_clock(clock);
        ptp_clock_settime(99, PtpTime::new(42, 0)).unwrap();
        let time = ptp_clock_gettime(99).unwrap();
        assert_eq!(time.seconds, 42);
        assert_eq!(time.nanoseconds, 0);
        unregister_ptp_clock(99);
    }

    #[test]
    fn notification_tx_sw() {
        let notif = TimestampNotification::new_tx_sw(PtpTime::new(10, 20), 42);
        let err = notif.err;
        let ee_errno = err.ee_errno;
        let ee_origin = err.ee_origin;
        let ee_info = err.ee_info;
        let ee_data = err.ee_data;
        assert_eq!(ee_errno, ENOMSG);
        assert_eq!(ee_origin, SO_EE_ORIGIN_TSTAMP);
        assert_eq!(ee_info, 1);
        assert_eq!(ee_data, 42);
        assert_eq!(notif.sts.ts[0], PtpTime::new(10, 20));
    }

    #[test]
    fn notification_tx_hw() {
        let notif = TimestampNotification::new_tx_hw(PtpTime::new(50, 60), 7);
        let err = notif.err;
        let ee_info = err.ee_info;
        let ee_data = err.ee_data;
        assert_eq!(ee_info, 2);
        assert_eq!(ee_data, 7);
        assert_eq!(notif.sts.ts[2], PtpTime::new(50, 60));
    }

    #[test]
    fn notification_serialize() {
        let notif = TimestampNotification::new_tx_sw(PtpTime::new(1, 2), 3);
        let mut buf = [0u8; 60];
        let size = notif.serialize(&mut buf).unwrap();
        assert!(size > 0);
    }

    #[test]
    fn skb_shared_hwtstamps() {
        let hws = SkbSharedHwtstamps::new(PtpTime::new(5, 6));
        assert_eq!(hws.hwtstamp, PtpTime::new(5, 6));
        assert!(hws.swstamp.is_none());
        let hws = hws.with_swstamp(PtpTime::new(7, 8));
        assert_eq!(hws.swstamp, Some(PtpTime::new(7, 8)));
    }

    #[test]
    fn hw_timestamp_flags_from_bits() {
        assert_eq!(HwTimestampFlags::from_bits(0x1), Some(HwTimestampFlags::Software));
        assert_eq!(HwTimestampFlags::from_bits(0x10), Some(HwTimestampFlags::HwGen));
        assert_eq!(HwTimestampFlags::from_bits(0x20), Some(HwTimestampFlags::HwV2Tx));
        assert_eq!(HwTimestampFlags::from_bits(0x40), Some(HwTimestampFlags::HwV2Rx));
        assert_eq!(HwTimestampFlags::from_bits(0x60), Some(HwTimestampFlags::HwV2ComboTxRx));
        assert_eq!(HwTimestampFlags::from_bits(0xFF), None);
    }

    #[test]
    fn ptp_clock_invalid() {
        assert_eq!(ptp_clock_gettime(9999), Err(NetError::InvalidParam));
        assert_eq!(ptp_clock_settime(9999, PtpTime::new(0, 0)), Err(NetError::InvalidParam));
    }

    #[test]
    fn scm_tstamp_values() {
        assert_eq!(ScmTstamp::from_u32(0), Some(ScmTstamp::Snd));
        assert_eq!(ScmTstamp::from_u32(1), Some(ScmTstamp::SndSw));
        assert_eq!(ScmTstamp::from_u32(2), Some(ScmTstamp::SndHw));
        assert_eq!(ScmTstamp::from_u32(3), Some(ScmTstamp::RcvSw));
        assert_eq!(ScmTstamp::from_u32(4), Some(ScmTstamp::RcvHw));
        assert_eq!(ScmTstamp::from_u32(5), None);
    }

    #[test]
    fn tx_rx_type_conversions() {
        assert_eq!(TxTsType::from_u32(0), Some(TxTsType::Off));
        assert_eq!(TxTsType::from_u32(1), Some(TxTsType::On));
        assert_eq!(TxTsType::from_u32(2), Some(TxTsType::OnestepP2p));
        assert_eq!(TxTsType::from_u32(3), Some(TxTsType::Onestep));
        assert_eq!(TxTsType::from_u32(4), None);
        assert_eq!(RxTsFilter::from_u32(0), Some(RxTsFilter::None));
        assert_eq!(RxTsFilter::from_u32(0xFFFF), Some(RxTsFilter::All));
    }

    #[test]
    fn hwtstamp_caps() {
        let mut caps = HwTimestampCaps::new();
        caps.tx_types.push(TxTsType::On);
        caps.rx_filters.push(RxTsFilter::PtpV2Event);
        assert!(caps.supports_tx(TxTsType::On));
        assert!(!caps.supports_tx(TxTsType::Off));
        assert!(caps.supports_rx(RxTsFilter::PtpV2Event));
        assert!(!caps.supports_rx(RxTsFilter::All));
    }

    #[test]
    fn ptp_time_zero() {
        let t = PtpTime::zero();
        assert_eq!(t.seconds, 0);
        assert_eq!(t.nanoseconds, 0);
    }

    #[test]
    fn ptp_time_serialize_roundtrip() {
        let t = PtpTime::new(0xDEAD_BEEF_CAFE_BABE, 999_999_999);
        let mut buf = [0u8; 12];
        t.serialize(&mut buf).unwrap();
        let t2 = PtpTime::deserialize(&buf).unwrap();
        assert_eq!(t, t2);
    }

    #[test]
    fn ptp_sys_offset_precise() {
        let offset = PtpSysOffsetPrecise {
            device_time: PtpTime::new(100, 0),
            sys_time: PtpTime::new(100, 500_000_000),
        };
        assert_eq!(offset.estimate_offset(), 500_000_000);
    }

    #[test]
    fn ptp_time_add_ns_multiple_wraparounds() {
        let mut t = PtpTime::new(0, 0);
        for _ in 0..1_000_000 {
            t = t.add_ns(999_999_999);
        }
        assert!(t.seconds > 0);
        assert!(t.nanoseconds < 1_000_000_000);
    }

    #[test]
    fn hw_timestamp_flags_bits() {
        assert_eq!(HwTimestampFlags::Software.bits(), 0x1);
        assert_eq!(HwTimestampFlags::HwGen.bits(), 0x10);
        assert_eq!(HwTimestampFlags::HwV2ComboTxRx.bits(), 0x60);
    }

    #[test]
    fn extract_hw_timestamp_short_packet() {
        let packet = [0u8; 10];
        let meta = HwTimestampMeta {
            hwtstamp: PtpTime::new(1, 2),
            sw_timestamp: false,
            hw_timestamp: true,
            tx_flags: 0,
        };
        assert!(extract_hardware_timestamp(&packet, &meta).is_none());
    }

    #[test]
    fn extract_hw_timestamp_disabled() {
        let mut packet = vec![0u8; 60];
        packet[12] = 0x08;
        packet[13] = 0x00;
        let meta = HwTimestampMeta {
            hwtstamp: PtpTime::new(1, 2),
            sw_timestamp: false,
            hw_timestamp: false,
            tx_flags: 0,
        };
        assert!(extract_hardware_timestamp(&packet, &meta).is_none());
    }

    #[test]
    fn extract_hw_timestamp_ptp_udp() {
        let mut packet = vec![0u8; 80];
        packet[12] = 0x08;
        packet[13] = 0x00;
        packet[14] = 0x45;
        packet[23] = 17;
        let udp_offset = 34;
        let dst_port = 319u16;
        packet[udp_offset + 2] = (dst_port >> 8) as u8;
        packet[udp_offset + 3] = (dst_port & 0xFF) as u8;
        let meta = HwTimestampMeta {
            hwtstamp: PtpTime::new(42, 43),
            sw_timestamp: false,
            hw_timestamp: true,
            tx_flags: 0,
        };
        let ts = extract_hardware_timestamp(&packet, &meta);
        assert_eq!(ts, Some(PtpTime::new(42, 43)));
    }

    #[test]
    fn insert_sw_timestamp_short_packet() {
        let mut packet = [0u8; 10];
        assert!(insert_software_timestamp(&mut packet, PtpTime::new(0, 0)).is_err());
    }

    #[test]
    fn insert_sw_timestamp_non_ptp() {
        let mut packet = vec![0u8; 60];
        packet[12] = 0x08;
        packet[13] = 0x00;
        let result = insert_software_timestamp(&mut packet, PtpTime::new(1, 2));
        assert_eq!(result, Ok(()));
    }

    #[test]
    fn insert_sw_timestamp_ptp() {
        let mut packet = vec![0u8; 80];
        packet[12] = 0x08;
        packet[13] = 0x00;
        packet[14] = 0x45;
        packet[23] = 17;
        let udp_offset = 34;
        let dst_port = 319u16;
        packet[udp_offset + 2] = (dst_port >> 8) as u8;
        packet[udp_offset + 3] = (dst_port & 0xFF) as u8;
        let result = insert_software_timestamp(&mut packet, PtpTime::new(100, 200));
        assert!(result.is_ok());
        let ptp_offset = udp_offset + 8;
        let sec_hi = u16::from_be_bytes([packet[ptp_offset + 4], packet[ptp_offset + 5]]);
        let sec_lo = u32::from_be_bytes([
            packet[ptp_offset + 6],
            packet[ptp_offset + 7],
            packet[ptp_offset + 8],
            packet[ptp_offset + 9],
        ]);
        let seconds = ((sec_hi as u64) << 32) | sec_lo as u64;
        assert_eq!(seconds, 100);
    }
}
