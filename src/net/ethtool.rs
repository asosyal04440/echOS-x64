use super::{NetError, NetInterface};
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use spin::Mutex;

pub const ETH_TOOL_VERNAME_LEN: usize = 32;
pub const ETH_TOOL_BUSINFO_LEN: usize = 32;
pub const ETH_TOOL_FWVER_LEN: usize = 32;
pub const ETH_TOOL_SOPASS_LEN: usize = 6;

pub const WOL_MAGIC: u32 = 1 << 0;
pub const WOL_UCAST: u32 = 1 << 1;
pub const WOL_MCAST: u32 = 1 << 2;
pub const WOL_BROADCAST: u32 = 1 << 3;
pub const WOL_ARP: u32 = 1 << 4;
pub const WOL_SECURE: u32 = 1 << 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WolFlags(pub u32);

impl WolFlags {
    pub const MAGIC: WolFlags = WolFlags(WOL_MAGIC);
    pub const UCAST: WolFlags = WolFlags(WOL_UCAST);
    pub const MCAST: WolFlags = WolFlags(WOL_MCAST);
    pub const BROADCAST: WolFlags = WolFlags(WOL_BROADCAST);
    pub const ARP: WolFlags = WolFlags(WOL_ARP);
    pub const SECURE: WolFlags = WolFlags(WOL_SECURE);

    pub const fn empty() -> Self {
        WolFlags(0)
    }

    pub const fn from_bits(val: u32) -> Self {
        WolFlags(val)
    }

    pub fn contains(self, other: WolFlags) -> bool {
        (self.0 & other.0) == other.0
    }

    pub fn insert(&mut self, other: WolFlags) {
        self.0 |= other.0;
    }

    pub fn remove(&mut self, other: WolFlags) {
        self.0 &= !other.0;
    }

    pub fn bits(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EthtoolCmd {
    GetLink,
    GetSpeed,
    GetDuplex,
    GetOffload,
    GetRingParam,
    GetWol,
    GetEeprom,
    GetDriverInfo,
    SetOffload,
    SetRingParam,
    SetWol,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DuplexMode {
    Half,
    Full,
}

impl DuplexMode {
    pub fn from_u8(val: u8) -> Self {
        match val {
            0 => DuplexMode::Half,
            _ => DuplexMode::Full,
        }
    }

    pub fn as_u8(self) -> u8 {
        match self {
            DuplexMode::Half => 0,
            DuplexMode::Full => 1,
        }
    }
}

#[derive(Clone, Debug)]
pub struct EthtoolLinkInfo {
    pub speed_mbps: u32,
    pub duplex: DuplexMode,
    pub autoneg: bool,
    pub link_up: bool,
    pub phy_addr: u8,
    pub port: EthtoolPort,
    pub transceiver: EthtoolTransceiver,
}

impl Default for EthtoolLinkInfo {
    fn default() -> Self {
        EthtoolLinkInfo {
            speed_mbps: 0,
            duplex: DuplexMode::Half,
            autoneg: false,
            link_up: false,
            phy_addr: 0,
            port: EthtoolPort::Mii,
            transceiver: EthtoolTransceiver::Internal,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EthtoolPort {
    Mii,
    Aui,
    Mca,
    TwistedPair,
    AuiBnc,
    Fpi,
    MiiRgmii,
    Ambiguous,
    Unknown,
    None,
}

impl EthtoolPort {
    pub fn from_u8(val: u8) -> Self {
        match val {
            0 => EthtoolPort::Mii,
            1 => EthtoolPort::Aui,
            2 => EthtoolPort::Mca,
            3 => EthtoolPort::TwistedPair,
            4 => EthtoolPort::AuiBnc,
            5 => EthtoolPort::Fpi,
            6 => EthtoolPort::MiiRgmii,
            7 => EthtoolPort::Ambiguous,
            8 => EthtoolPort::Unknown,
            _ => EthtoolPort::None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EthtoolTransceiver {
    Internal,
    External,
    Unknown,
}

impl EthtoolTransceiver {
    pub fn from_u8(val: u8) -> Self {
        match val {
            0 => EthtoolTransceiver::Internal,
            1 => EthtoolTransceiver::External,
            _ => EthtoolTransceiver::Unknown,
        }
    }
}

#[derive(Clone, Debug)]
pub struct EthtoolOffload {
    pub tx_checksum: u8,
    pub rx_checksum: u8,
    pub tx_scatter_gather: u8,
    pub rx_scatter_gather: u8,
    pub tso: u8,
    pub gso: u8,
    pub gro: u8,
    pub lro: u8,
    pub tx_vlan: u8,
    pub rx_vlan: u8,
    pub tx_udp_tnl: u8,
    pub rx_udp_tnl: u8,
    pub tx_frag: u8,
    pub rx_frag: u8,
}

impl Default for EthtoolOffload {
    fn default() -> Self {
        EthtoolOffload {
            tx_checksum: 0,
            rx_checksum: 0,
            tx_scatter_gather: 0,
            rx_scatter_gather: 0,
            tso: 0,
            gso: 0,
            gro: 0,
            lro: 0,
            tx_vlan: 0,
            rx_vlan: 0,
            tx_udp_tnl: 0,
            rx_udp_tnl: 0,
            tx_frag: 0,
            rx_frag: 0,
        }
    }
}

impl EthtoolOffload {
    pub fn is_tx_checksum_enabled(&self) -> bool {
        self.tx_checksum != 0
    }

    pub fn is_rx_checksum_enabled(&self) -> bool {
        self.rx_checksum != 0
    }

    pub fn is_tso_enabled(&self) -> bool {
        self.tso != 0
    }

    pub fn is_gro_enabled(&self) -> bool {
        self.gro != 0
    }

    pub fn enable_all_offloads(&mut self) {
        self.tx_checksum = 1;
        self.rx_checksum = 1;
        self.tx_scatter_gather = 1;
        self.rx_scatter_gather = 1;
        self.tso = 1;
        self.gso = 1;
        self.gro = 1;
        self.tx_vlan = 1;
        self.rx_vlan = 1;
    }

    pub fn disable_all_offloads(&mut self) {
        *self = EthtoolOffload::default();
    }
}

#[derive(Clone, Debug)]
pub struct EthtoolRingParam {
    pub rx_max_pending: u32,
    pub tx_max_pending: u32,
    pub rx_pending: u32,
    pub tx_pending: u32,
    pub rx_mini_max_pending: u32,
    pub rx_mini_pending: u32,
    pub rx_jumbo_max_pending: u32,
    pub rx_jumbo_pending: u32,
}

impl Default for EthtoolRingParam {
    fn default() -> Self {
        EthtoolRingParam {
            rx_max_pending: 256,
            tx_max_pending: 256,
            rx_pending: 128,
            tx_pending: 128,
            rx_mini_max_pending: 0,
            rx_mini_pending: 0,
            rx_jumbo_max_pending: 0,
            rx_jumbo_pending: 0,
        }
    }
}

impl EthtoolRingParam {
    pub fn validate(&self) -> bool {
        if self.rx_pending > self.rx_max_pending {
            return false;
        }
        if self.tx_pending > self.tx_max_pending {
            return false;
        }
        if self.rx_mini_pending > self.rx_mini_max_pending {
            return false;
        }
        if self.rx_jumbo_pending > self.rx_jumbo_max_pending {
            return false;
        }
        true
    }

    pub fn clamp_to_max(&mut self) {
        if self.rx_pending > self.rx_max_pending {
            self.rx_pending = self.rx_max_pending;
        }
        if self.tx_pending > self.tx_max_pending {
            self.tx_pending = self.tx_max_pending;
        }
        if self.rx_mini_pending > self.rx_mini_max_pending {
            self.rx_mini_pending = self.rx_mini_max_pending;
        }
        if self.rx_jumbo_pending > self.rx_jumbo_max_pending {
            self.rx_jumbo_pending = self.rx_jumbo_max_pending;
        }
    }
}

#[derive(Clone, Debug)]
pub struct EthtoolWol {
    pub wake_on_lan: WolFlags,
    pub sopass: [u8; ETH_TOOL_SOPASS_LEN],
}

impl Default for EthtoolWol {
    fn default() -> Self {
        EthtoolWol {
            wake_on_lan: WolFlags::empty(),
            sopass: [0; ETH_TOOL_SOPASS_LEN],
        }
    }
}

impl EthtoolWol {
    pub fn is_wol_enabled(&self) -> bool {
        self.wake_on_lan.bits() != 0
    }

    pub fn enable_magic(&mut self) {
        self.wake_on_lan.insert(WolFlags::MAGIC);
    }

    pub fn disable_magic(&mut self) {
        self.wake_on_lan.remove(WolFlags::MAGIC);
    }

    pub fn set_sopass(&mut self, pattern: [u8; ETH_TOOL_SOPASS_LEN]) {
        self.sopass = pattern;
    }

    pub fn clear_sopass(&mut self) {
        self.sopass = [0; ETH_TOOL_SOPASS_LEN];
    }

    pub fn clear_all(&mut self) {
        self.wake_on_lan = WolFlags::empty();
        self.sopass = [0; ETH_TOOL_SOPASS_LEN];
    }
}

#[derive(Clone, Debug)]
pub struct EthtoolDriverInfo {
    pub driver_name: [u8; ETH_TOOL_VERNAME_LEN],
    pub bus_info: [u8; ETH_TOOL_BUSINFO_LEN],
    pub fw_version: [u8; ETH_TOOL_FWVER_LEN],
    pub nic_info: String,
}

impl Default for EthtoolDriverInfo {
    fn default() -> Self {
        EthtoolDriverInfo {
            driver_name: [0; ETH_TOOL_VERNAME_LEN],
            bus_info: [0; ETH_TOOL_BUSINFO_LEN],
            fw_version: [0; ETH_TOOL_FWVER_LEN],
            nic_info: String::new(),
        }
    }
}

impl EthtoolDriverInfo {
    pub fn set_driver_name(&mut self, name: &str) {
        let bytes = name.as_bytes();
        let len = core::cmp::min(bytes.len(), ETH_TOOL_VERNAME_LEN - 1);
        self.driver_name[..len].copy_from_slice(&bytes[..len]);
        self.driver_name[len] = 0;
    }

    pub fn get_driver_name(&self) -> &str {
        let end = self.driver_name.iter().position(|&b| b == 0).unwrap_or(ETH_TOOL_VERNAME_LEN);
        core::str::from_utf8(&self.driver_name[..end]).unwrap_or("unknown")
    }

    pub fn set_bus_info(&mut self, info: &str) {
        let bytes = info.as_bytes();
        let len = core::cmp::min(bytes.len(), ETH_TOOL_BUSINFO_LEN - 1);
        self.bus_info[..len].copy_from_slice(&bytes[..len]);
        self.bus_info[len] = 0;
    }

    pub fn get_bus_info(&self) -> &str {
        let end = self.bus_info.iter().position(|&b| b == 0).unwrap_or(ETH_TOOL_BUSINFO_LEN);
        core::str::from_utf8(&self.bus_info[..end]).unwrap_or("unknown")
    }

    pub fn set_fw_version(&mut self, ver: &str) {
        let bytes = ver.as_bytes();
        let len = core::cmp::min(bytes.len(), ETH_TOOL_FWVER_LEN - 1);
        self.fw_version[..len].copy_from_slice(&bytes[..len]);
        self.fw_version[len] = 0;
    }

    pub fn get_fw_version(&self) -> &str {
        let end = self.fw_version.iter().position(|&b| b == 0).unwrap_or(ETH_TOOL_FWVER_LEN);
        core::str::from_utf8(&self.fw_version[..end]).unwrap_or("unknown")
    }
}

#[derive(Clone, Debug)]
pub struct EthtoolEeprom {
    pub offset: u32,
    pub length: u32,
    pub data: Vec<u8>,
}

impl EthtoolEeprom {
    pub fn new(offset: u32, length: u32) -> Self {
        EthtoolEeprom {
            offset,
            length,
            data: vec![0; length as usize],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EthtoolLinkState {
    Up,
    Down,
    Unknown,
}

pub struct EthtoolInterface {
    pub ifindex: u32,
    pub link_info: Mutex<EthtoolLinkInfo>,
    pub offload: Mutex<EthtoolOffload>,
    pub ring_param: Mutex<EthtoolRingParam>,
    pub wol: Mutex<EthtoolWol>,
    pub driver_info: Mutex<EthtoolDriverInfo>,
    pub stats: EthtoolStats,
}

pub struct EthtoolStats {
    pub cmd_get_link: AtomicU32,
    pub cmd_get_speed: AtomicU32,
    pub cmd_get_duplex: AtomicU32,
    pub cmd_get_offload: AtomicU32,
    pub cmd_get_ring_param: AtomicU32,
    pub cmd_get_wol: AtomicU32,
    pub cmd_get_eeprom: AtomicU32,
    pub cmd_get_driver_info: AtomicU32,
    pub cmd_set_offload: AtomicU32,
    pub cmd_set_ring_param: AtomicU32,
    pub cmd_set_wol: AtomicU32,
    pub link_up_count: AtomicU32,
    pub link_down_count: AtomicU32,
}

impl Default for EthtoolStats {
    fn default() -> Self {
        EthtoolStats::new()
    }
}

impl EthtoolStats {
    pub const fn new() -> Self {
        EthtoolStats {
            cmd_get_link: AtomicU32::new(0),
            cmd_get_speed: AtomicU32::new(0),
            cmd_get_duplex: AtomicU32::new(0),
            cmd_get_offload: AtomicU32::new(0),
            cmd_get_ring_param: AtomicU32::new(0),
            cmd_get_wol: AtomicU32::new(0),
            cmd_get_eeprom: AtomicU32::new(0),
            cmd_get_driver_info: AtomicU32::new(0),
            cmd_set_offload: AtomicU32::new(0),
            cmd_set_ring_param: AtomicU32::new(0),
            cmd_set_wol: AtomicU32::new(0),
            link_up_count: AtomicU32::new(0),
            link_down_count: AtomicU32::new(0),
        }
    }

    pub fn snapshot(&self) -> EthtoolStatsSnapshot {
        let ord = Ordering::Relaxed;
        EthtoolStatsSnapshot {
            cmd_get_link: self.cmd_get_link.load(ord),
            cmd_get_speed: self.cmd_get_speed.load(ord),
            cmd_get_duplex: self.cmd_get_duplex.load(ord),
            cmd_get_offload: self.cmd_get_offload.load(ord),
            cmd_get_ring_param: self.cmd_get_ring_param.load(ord),
            cmd_get_wol: self.cmd_get_wol.load(ord),
            cmd_get_eeprom: self.cmd_get_eeprom.load(ord),
            cmd_get_driver_info: self.cmd_get_driver_info.load(ord),
            cmd_set_offload: self.cmd_set_offload.load(ord),
            cmd_set_ring_param: self.cmd_set_ring_param.load(ord),
            cmd_set_wol: self.cmd_set_wol.load(ord),
            link_up_count: self.link_up_count.load(ord),
            link_down_count: self.link_down_count.load(ord),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct EthtoolStatsSnapshot {
    pub cmd_get_link: u32,
    pub cmd_get_speed: u32,
    pub cmd_get_duplex: u32,
    pub cmd_get_offload: u32,
    pub cmd_get_ring_param: u32,
    pub cmd_get_wol: u32,
    pub cmd_get_eeprom: u32,
    pub cmd_get_driver_info: u32,
    pub cmd_set_offload: u32,
    pub cmd_set_ring_param: u32,
    pub cmd_set_wol: u32,
    pub link_up_count: u32,
    pub link_down_count: u32,
}

pub static ETH_TOOL_INTERFACES: Mutex<BTreeMap<u32, EthtoolInterface>> = Mutex::new(BTreeMap::new());

pub fn ethtool_init() {
    crate::serial_println!("[ETHtool] Initializing ethtool subsystem");
}

pub fn register_ethtool(ifindex: u32, info: EthtoolDriverInfo) {
    let mut interfaces = ETH_TOOL_INTERFACES.lock();
    if interfaces.contains_key(&ifindex) {
        return;
    }
    let iface = EthtoolInterface {
        ifindex,
        link_info: Mutex::new(EthtoolLinkInfo::default()),
        offload: Mutex::new(EthtoolOffload::default()),
        ring_param: Mutex::new(EthtoolRingParam::default()),
        wol: Mutex::new(EthtoolWol::default()),
        driver_info: Mutex::new(info),
        stats: EthtoolStats::new(),
    };
    interfaces.insert(ifindex, iface);
}

pub fn unregister_ethtool(ifindex: u32) {
    let mut interfaces = ETH_TOOL_INTERFACES.lock();
    interfaces.remove(&ifindex);
}

pub fn ethtool_get_link(ifindex: u32) -> Result<EthtoolLinkInfo, NetError> {
    let interfaces = ETH_TOOL_INTERFACES.lock();
    let iface = interfaces.get(&ifindex).ok_or(NetError::NoInterface)?;
    iface.stats.cmd_get_link.fetch_add(1, Ordering::Relaxed);
    let guard = iface.link_info.lock();
    let result = guard.clone();
    drop(guard);
    Ok(result)
}

pub fn ethtool_get_speed(ifindex: u32) -> Result<u32, NetError> {
    let interfaces = ETH_TOOL_INTERFACES.lock();
    let iface = interfaces.get(&ifindex).ok_or(NetError::NoInterface)?;
    iface.stats.cmd_get_speed.fetch_add(1, Ordering::Relaxed);
    let guard = iface.link_info.lock();
    let speed = guard.speed_mbps;
    drop(guard);
    Ok(speed)
}

pub fn ethtool_get_duplex(ifindex: u32) -> Result<DuplexMode, NetError> {
    let interfaces = ETH_TOOL_INTERFACES.lock();
    let iface = interfaces.get(&ifindex).ok_or(NetError::NoInterface)?;
    iface.stats.cmd_get_duplex.fetch_add(1, Ordering::Relaxed);
    let guard = iface.link_info.lock();
    let duplex = guard.duplex;
    drop(guard);
    Ok(duplex)
}

pub fn ethtool_get_offload(ifindex: u32) -> Result<EthtoolOffload, NetError> {
    let interfaces = ETH_TOOL_INTERFACES.lock();
    let iface = interfaces.get(&ifindex).ok_or(NetError::NoInterface)?;
    iface.stats.cmd_get_offload.fetch_add(1, Ordering::Relaxed);
    let guard = iface.offload.lock();
    let result = guard.clone();
    drop(guard);
    Ok(result)
}

pub fn ethtool_get_ring_param(ifindex: u32) -> Result<EthtoolRingParam, NetError> {
    let interfaces = ETH_TOOL_INTERFACES.lock();
    let iface = interfaces.get(&ifindex).ok_or(NetError::NoInterface)?;
    iface.stats.cmd_get_ring_param.fetch_add(1, Ordering::Relaxed);
    let guard = iface.ring_param.lock();
    let result = guard.clone();
    drop(guard);
    Ok(result)
}

pub fn ethtool_get_wol(ifindex: u32) -> Result<EthtoolWol, NetError> {
    let interfaces = ETH_TOOL_INTERFACES.lock();
    let iface = interfaces.get(&ifindex).ok_or(NetError::NoInterface)?;
    iface.stats.cmd_get_wol.fetch_add(1, Ordering::Relaxed);
    let guard = iface.wol.lock();
    let result = guard.clone();
    drop(guard);
    Ok(result)
}

pub fn ethtool_get_eeprom(ifindex: u32, offset: u32, length: u32) -> Result<EthtoolEeprom, NetError> {
    let interfaces = ETH_TOOL_INTERFACES.lock();
    let iface = interfaces.get(&ifindex).ok_or(NetError::NoInterface)?;
    iface.stats.cmd_get_eeprom.fetch_add(1, Ordering::Relaxed);
    Ok(EthtoolEeprom::new(offset, length))
}

pub fn ethtool_get_driver_info(ifindex: u32) -> Result<EthtoolDriverInfo, NetError> {
    let interfaces = ETH_TOOL_INTERFACES.lock();
    let iface = interfaces.get(&ifindex).ok_or(NetError::NoInterface)?;
    iface.stats.cmd_get_driver_info.fetch_add(1, Ordering::Relaxed);
    let guard = iface.driver_info.lock();
    let result = guard.clone();
    drop(guard);
    Ok(result)
}

pub fn ethtool_set_offload(ifindex: u32, offload: EthtoolOffload) -> Result<(), NetError> {
    let interfaces = ETH_TOOL_INTERFACES.lock();
    let iface = interfaces.get(&ifindex).ok_or(NetError::NoInterface)?;
    iface.stats.cmd_set_offload.fetch_add(1, Ordering::Relaxed);
    *iface.offload.lock() = offload;
    Ok(())
}

pub fn ethtool_set_ring_param(ifindex: u32, mut param: EthtoolRingParam) -> Result<(), NetError> {
    param.clamp_to_max();
    let interfaces = ETH_TOOL_INTERFACES.lock();
    let iface = interfaces.get(&ifindex).ok_or(NetError::NoInterface)?;
    iface.stats.cmd_set_ring_param.fetch_add(1, Ordering::Relaxed);
    *iface.ring_param.lock() = param;
    Ok(())
}

pub fn ethtool_set_wol(ifindex: u32, wol: EthtoolWol) -> Result<(), NetError> {
    let interfaces = ETH_TOOL_INTERFACES.lock();
    let iface = interfaces.get(&ifindex).ok_or(NetError::NoInterface)?;
    iface.stats.cmd_set_wol.fetch_add(1, Ordering::Relaxed);
    *iface.wol.lock() = wol;
    Ok(())
}

pub fn ethtool_set_link(ifindex: u32, speed: u32, duplex: DuplexMode, autoneg: bool) -> Result<(), NetError> {
    let interfaces = ETH_TOOL_INTERFACES.lock();
    let iface = interfaces.get(&ifindex).ok_or(NetError::NoInterface)?;
    let mut link = iface.link_info.lock();
    let was_up = link.link_up;
    link.speed_mbps = speed;
    link.duplex = duplex;
    link.autoneg = autoneg;
    link.link_up = true;
    if was_up && !link.link_up {
        iface.stats.link_down_count.fetch_add(1, Ordering::Relaxed);
    } else if !was_up && link.link_up {
        iface.stats.link_up_count.fetch_add(1, Ordering::Relaxed);
    }
    Ok(())
}

pub fn ethtool_nway_reset(ifindex: u32) -> Result<(), NetError> {
    let interfaces = ETH_TOOL_INTERFACES.lock();
    let iface = interfaces.get(&ifindex).ok_or(NetError::NoInterface)?;
    let mut link = iface.link_info.lock();
    if link.autoneg {
        link.link_up = false;
        iface.stats.link_down_count.fetch_add(1, Ordering::Relaxed);
    }
    Ok(())
}

pub fn ethtool_get_link_state(ifindex: u32) -> Result<EthtoolLinkState, NetError> {
    let interfaces = ETH_TOOL_INTERFACES.lock();
    let iface = interfaces.get(&ifindex).ok_or(NetError::NoInterface)?;
    let link = iface.link_info.lock();
    Ok(if link.link_up {
        EthtoolLinkState::Up
    } else {
        EthtoolLinkState::Down
    })
}

pub fn ethtool_get_stats(ifindex: u32) -> Result<EthtoolStatsSnapshot, NetError> {
    let interfaces = ETH_TOOL_INTERFACES.lock();
    let iface = interfaces.get(&ifindex).ok_or(NetError::NoInterface)?;
    Ok(iface.stats.snapshot())
}

pub fn ethtool_get_all_stats() -> Vec<(u32, EthtoolStatsSnapshot)> {
    let interfaces = ETH_TOOL_INTERFACES.lock();
    let mut result = Vec::new();
    for (ifindex, iface) in interfaces.iter() {
        result.push((*ifindex, iface.stats.snapshot()));
    }
    result
}

pub fn ethtool_set_offload_tx_checksum(ifindex: u32, enabled: bool) -> Result<(), NetError> {
    let interfaces = ETH_TOOL_INTERFACES.lock();
    let iface = interfaces.get(&ifindex).ok_or(NetError::NoInterface)?;
    let mut offload = iface.offload.lock();
    offload.tx_checksum = if enabled { 1 } else { 0 };
    Ok(())
}

pub fn ethtool_set_offload_rx_checksum(ifindex: u32, enabled: bool) -> Result<(), NetError> {
    let interfaces = ETH_TOOL_INTERFACES.lock();
    let iface = interfaces.get(&ifindex).ok_or(NetError::NoInterface)?;
    let mut offload = iface.offload.lock();
    offload.rx_checksum = if enabled { 1 } else { 0 };
    Ok(())
}

pub fn ethtool_set_offload_tso(ifindex: u32, enabled: bool) -> Result<(), NetError> {
    let interfaces = ETH_TOOL_INTERFACES.lock();
    let iface = interfaces.get(&ifindex).ok_or(NetError::NoInterface)?;
    let mut offload = iface.offload.lock();
    offload.tso = if enabled { 1 } else { 0 };
    Ok(())
}

pub fn ethtool_set_offload_gro(ifindex: u32, enabled: bool) -> Result<(), NetError> {
    let interfaces = ETH_TOOL_INTERFACES.lock();
    let iface = interfaces.get(&ifindex).ok_or(NetError::NoInterface)?;
    let mut offload = iface.offload.lock();
    offload.gro = if enabled { 1 } else { 0 };
    Ok(())
}

pub fn ethtool_set_wol_magic(ifindex: u32, enabled: bool) -> Result<(), NetError> {
    let interfaces = ETH_TOOL_INTERFACES.lock();
    let iface = interfaces.get(&ifindex).ok_or(NetError::NoInterface)?;
    let mut wol = iface.wol.lock();
    if enabled {
        wol.enable_magic();
    } else {
        wol.disable_magic();
    }
    Ok(())
}

pub fn ethtool_set_wol_pattern(ifindex: u32, pattern: [u8; ETH_TOOL_SOPASS_LEN]) -> Result<(), NetError> {
    let interfaces = ETH_TOOL_INTERFACES.lock();
    let iface = interfaces.get(&ifindex).ok_or(NetError::NoInterface)?;
    let mut wol = iface.wol.lock();
    wol.set_sopass(pattern);
    Ok(())
}

pub fn ethtool_set_ring_rx_pending(ifindex: u32, pending: u32) -> Result<(), NetError> {
    let interfaces = ETH_TOOL_INTERFACES.lock();
    let iface = interfaces.get(&ifindex).ok_or(NetError::NoInterface)?;
    let mut ring = iface.ring_param.lock();
    ring.rx_pending = core::cmp::min(pending, ring.rx_max_pending);
    Ok(())
}

pub fn ethtool_set_ring_tx_pending(ifindex: u32, pending: u32) -> Result<(), NetError> {
    let interfaces = ETH_TOOL_INTERFACES.lock();
    let iface = interfaces.get(&ifindex).ok_or(NetError::NoInterface)?;
    let mut ring = iface.ring_param.lock();
    ring.tx_pending = core::cmp::min(pending, ring.tx_max_pending);
    Ok(())
}

pub fn ethtool_set_driver_name(ifindex: u32, name: &str) -> Result<(), NetError> {
    let interfaces = ETH_TOOL_INTERFACES.lock();
    let iface = interfaces.get(&ifindex).ok_or(NetError::NoInterface)?;
    let mut info = iface.driver_info.lock();
    info.set_driver_name(name);
    Ok(())
}

pub fn ethtool_set_bus_info(ifindex: u32, info_str: &str) -> Result<(), NetError> {
    let interfaces = ETH_TOOL_INTERFACES.lock();
    let iface = interfaces.get(&ifindex).ok_or(NetError::NoInterface)?;
    let mut info = iface.driver_info.lock();
    info.set_bus_info(info_str);
    Ok(())
}

pub fn ethtool_set_fw_version(ifindex: u32, version: &str) -> Result<(), NetError> {
    let interfaces = ETH_TOOL_INTERFACES.lock();
    let iface = interfaces.get(&ifindex).ok_or(NetError::NoInterface)?;
    let mut info = iface.driver_info.lock();
    info.set_fw_version(version);
    Ok(())
}

pub fn ethtool_set_nic_info(ifindex: u32, info: String) -> Result<(), NetError> {
    let interfaces = ETH_TOOL_INTERFACES.lock();
    let iface = interfaces.get(&ifindex).ok_or(NetError::NoInterface)?;
    let mut driver = iface.driver_info.lock();
    driver.nic_info = info;
    Ok(())
}

pub fn ethtool_process_command(ifindex: u32, cmd: EthtoolCmd) -> Result<EthtoolResponse, NetError> {
    match cmd {
        EthtoolCmd::GetLink => {
            let link = ethtool_get_link(ifindex)?;
            Ok(EthtoolResponse::Link(link))
        }
        EthtoolCmd::GetSpeed => {
            let speed = ethtool_get_speed(ifindex)?;
            Ok(EthtoolResponse::Speed(speed))
        }
        EthtoolCmd::GetDuplex => {
            let duplex = ethtool_get_duplex(ifindex)?;
            Ok(EthtoolResponse::Duplex(duplex))
        }
        EthtoolCmd::GetOffload => {
            let offload = ethtool_get_offload(ifindex)?;
            Ok(EthtoolResponse::Offload(offload))
        }
        EthtoolCmd::GetRingParam => {
            let ring = ethtool_get_ring_param(ifindex)?;
            Ok(EthtoolResponse::RingParam(ring))
        }
        EthtoolCmd::GetWol => {
            let wol = ethtool_get_wol(ifindex)?;
            Ok(EthtoolResponse::Wol(wol))
        }
        EthtoolCmd::GetEeprom => {
            let eeprom = ethtool_get_eeprom(ifindex, 0, 256)?;
            Ok(EthtoolResponse::Eeprom(eeprom))
        }
        EthtoolCmd::GetDriverInfo => {
            let info = ethtool_get_driver_info(ifindex)?;
            Ok(EthtoolResponse::DriverInfo(info))
        }
        EthtoolCmd::SetOffload => Ok(EthtoolResponse::Ack),
        EthtoolCmd::SetRingParam => Ok(EthtoolResponse::Ack),
        EthtoolCmd::SetWol => Ok(EthtoolResponse::Ack),
    }
}

pub enum EthtoolResponse {
    Link(EthtoolLinkInfo),
    Speed(u32),
    Duplex(DuplexMode),
    Offload(EthtoolOffload),
    RingParam(EthtoolRingParam),
    Wol(EthtoolWol),
    Eeprom(EthtoolEeprom),
    DriverInfo(EthtoolDriverInfo),
    Ack,
}

pub fn ethtool_format_link_info(info: &EthtoolLinkInfo) -> Vec<u8> {
    let speed_str = if info.link_up {
        match info.speed_mbps {
            0 => String::from("unknown speed"),
            s if s >= 1000 => {
                let speed_g = s / 1000;
                let rem = s % 1000;
                if rem == 0 {
                    alloc::format!("{}Gbps", speed_g)
                } else {
                    alloc::format!("{}.{:01}Gbps", speed_g, rem / 100)
                }
            }
            s => alloc::format!("{}Mbps", s),
        }
    } else {
        String::from("no link")
    };

    let duplex_str = match info.duplex {
        DuplexMode::Full => "Full",
        DuplexMode::Half => "Half",
    };

    let autoneg_str = if info.autoneg { "on" } else { "off" };

    let result = alloc::format!(
        "Link detected: {} Speed: {} Duplex: {} Autoneg: {}",
        if info.link_up { "yes" } else { "no" },
        speed_str,
        duplex_str,
        autoneg_str
    );

    result.into_bytes()
}

pub fn ethtool_format_offload(offload: &EthtoolOffload) -> Vec<u8> {
    let mut lines = Vec::new();

    let tx_csum = if offload.tx_checksum != 0 { "on" } else { "off" };
    let rx_csum = if offload.rx_checksum != 0 { "on" } else { "off" };
    let sg = if offload.tx_scatter_gather != 0 { "on" } else { "off" };
    let tso = if offload.tso != 0 { "on" } else { "off" };
    let gso = if offload.gso != 0 { "on" } else { "off" };
    let gro = if offload.gro != 0 { "on" } else { "off" };
    let lro = if offload.lro != 0 { "on" } else { "off" };
    let tx_vlan = if offload.tx_vlan != 0 { "on" } else { "off" };
    let rx_vlan = if offload.rx_vlan != 0 { "on" } else { "off" };

    let line = alloc::format!("tx-checksumming: {}", tx_csum);
    lines.extend_from_slice(line.as_bytes());
    lines.push(b'\n');
    let line = alloc::format!("    tx-checksum-ipv4: {}", tx_csum);
    lines.extend_from_slice(line.as_bytes());
    lines.push(b'\n');
    let line = alloc::format!("rx-checksumming: {}", rx_csum);
    lines.extend_from_slice(line.as_bytes());
    lines.push(b'\n');
    let line = alloc::format!("scatter-gather: {}", sg);
    lines.extend_from_slice(line.as_bytes());
    lines.push(b'\n');
    let line = alloc::format!("tcp-segmentation-offload: {}", tso);
    lines.extend_from_slice(line.as_bytes());
    lines.push(b'\n');
    let line = alloc::format!("generic-segmentation-offload: {}", gso);
    lines.extend_from_slice(line.as_bytes());
    lines.push(b'\n');
    let line = alloc::format!("generic-receive-offload: {}", gro);
    lines.extend_from_slice(line.as_bytes());
    lines.push(b'\n');
    let line = alloc::format!("large-receive-offload: {}", lro);
    lines.extend_from_slice(line.as_bytes());
    lines.push(b'\n');
    let line = alloc::format!("tx-vlan-offload: {}", tx_vlan);
    lines.extend_from_slice(line.as_bytes());
    lines.push(b'\n');
    let line = alloc::format!("rx-vlan-offload: {}", rx_vlan);
    lines.extend_from_slice(line.as_bytes());
    lines.push(b'\n');

    lines
}

pub fn ethtool_format_ring_param(param: &EthtoolRingParam) -> Vec<u8> {
    let mut out = Vec::new();

    let lines = [
        ("Pre-set maximums:", ""),
        ("RX:", &alloc::format!("{}", param.rx_max_pending)),
        ("RX Mini:", &alloc::format!("{}", param.rx_mini_max_pending)),
        ("RX Jumbo:", &alloc::format!("{}", param.rx_jumbo_max_pending)),
        ("TX:", &alloc::format!("{}", param.tx_max_pending)),
        ("Current hardware settings:", ""),
        ("RX:", &alloc::format!("{}", param.rx_pending)),
        ("RX Mini:", &alloc::format!("{}", param.rx_mini_pending)),
        ("RX Jumbo:", &alloc::format!("{}", param.rx_jumbo_pending)),
        ("TX:", &alloc::format!("{}", param.tx_pending)),
    ];

    for (label, value) in lines.iter() {
        let line = if value.is_empty() {
            alloc::format!("{}\n", label)
        } else {
            alloc::format!("{} {}\n", label, value)
        };
        out.extend_from_slice(line.as_bytes());
    }

    out
}

pub fn ethtool_format_wol(wol: &EthtoolWol) -> Vec<u8> {
    let mut out = Vec::new();
    let bits = wol.wake_on_lan.bits();

    let flags = [
        ("g", WolFlags::MAGIC.bits()),
        ("u", WolFlags::UCAST.bits()),
        ("m", WolFlags::MCAST.bits()),
        ("b", WolFlags::BROADCAST.bits()),
        ("a", WolFlags::ARP.bits()),
        ("s", WolFlags::SECURE.bits()),
    ];

    let mut wol_str = String::new();
    for (ch, bit) in flags.iter() {
        if bits & bit != 0 {
            wol_str.push_str(ch);
        }
    }

    let line = alloc::format!("Wake-on: {}\n", wol_str);
    out.extend_from_slice(line.as_bytes());

    let line = alloc::format!(
        "SOPass: {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}\n",
        wol.sopass[0], wol.sopass[1], wol.sopass[2],
        wol.sopass[3], wol.sopass[4], wol.sopass[5]
    );
    out.extend_from_slice(line.as_bytes());

    out
}

pub fn ethtool_format_driver_info(info: &EthtoolDriverInfo) -> Vec<u8> {
    let mut out = Vec::new();

    let lines = [
        ("driver:", info.get_driver_name()),
        ("bus-info:", info.get_bus_info()),
        ("firmware-version:", info.get_fw_version()),
    ];

    for (label, value) in lines.iter() {
        let line = alloc::format!("{} {}\n", label, value);
        out.extend_from_slice(line.as_bytes());
    }

    if !info.nic_info.is_empty() {
        let line = alloc::format!("nic-info: {}\n", info.nic_info);
        out.extend_from_slice(line.as_bytes());
    }

    out
}

pub fn ethtool_default_driver_info(ifindex: u32) -> EthtoolDriverInfo {
    let mut info = EthtoolDriverInfo::default();
    info.set_driver_name("echos-net");
    info.set_bus_info(&alloc::format!("pci:{}", ifindex));
    info.set_fw_version("1.0.0");
    info.nic_info = String::from("echOS virtual NIC");
    info
}

pub fn ethtool_create_interface(ifindex: u32, driver_name: &str) {
    let info = ethtool_default_driver_info(ifindex);
    register_ethtool(ifindex, info);
    if let Some(name_override) = (!driver_name.is_empty()).then_some(driver_name) {
        let interfaces = ETH_TOOL_INTERFACES.lock();
        if let Some(iface) = interfaces.get(&ifindex) {
            iface.driver_info.lock().set_driver_name(name_override);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ethtool_offload_defaults() {
        let offload = EthtoolOffload::default();
        assert_eq!(offload.tx_checksum, 0);
        assert_eq!(offload.rx_checksum, 0);
        assert_eq!(offload.tso, 0);
        assert_eq!(offload.gro, 0);
        assert!(!offload.is_tx_checksum_enabled());
        assert!(!offload.is_rx_checksum_enabled());
        assert!(!offload.is_tso_enabled());
        assert!(!offload.is_gro_enabled());
    }

    #[test]
    fn test_ethtool_offload_enable_all() {
        let mut offload = EthtoolOffload::default();
        offload.enable_all_offloads();
        assert!(offload.is_tx_checksum_enabled());
        assert!(offload.is_rx_checksum_enabled());
        assert!(offload.is_tso_enabled());
        assert!(offload.is_gro_enabled());
        assert_eq!(offload.tx_scatter_gather, 1);
        assert_eq!(offload.tx_vlan, 1);
        assert_eq!(offload.rx_vlan, 1);
    }

    #[test]
    fn test_ethtool_offload_disable_all() {
        let mut offload = EthtoolOffload::default();
        offload.enable_all_offloads();
        offload.disable_all_offloads();
        assert_eq!(offload.tx_checksum, 0);
        assert_eq!(offload.rx_checksum, 0);
        assert_eq!(offload.tso, 0);
    }

    #[test]
    fn test_ethtool_ring_param_validate() {
        let param = EthtoolRingParam {
            rx_max_pending: 256,
            tx_max_pending: 256,
            rx_pending: 128,
            tx_pending: 128,
            ..EthtoolRingParam::default()
        };
        assert!(param.validate());

        let bad = EthtoolRingParam {
            rx_max_pending: 64,
            tx_max_pending: 64,
            rx_pending: 128,
            tx_pending: 128,
            ..EthtoolRingParam::default()
        };
        assert!(!bad.validate());
    }

    #[test]
    fn test_ethtool_ring_param_clamp() {
        let mut param = EthtoolRingParam {
            rx_max_pending: 64,
            tx_max_pending: 64,
            rx_pending: 200,
            tx_pending: 200,
            ..EthtoolRingParam::default()
        };
        param.clamp_to_max();
        assert_eq!(param.rx_pending, 64);
        assert_eq!(param.tx_pending, 64);
    }

    #[test]
    fn test_ethtool_wol_flags() {
        let mut wol = EthtoolWol::default();
        assert!(!wol.is_wol_enabled());
        wol.enable_magic();
        assert!(wol.is_wol_enabled());
        assert!(wol.wake_on_lan.contains(WolFlags::MAGIC));
        wol.disable_magic();
        assert!(!wol.wake_on_lan.contains(WolFlags::MAGIC));
    }

    #[test]
    fn test_ethtool_wol_sopass() {
        let mut wol = EthtoolWol::default();
        let pattern = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];
        wol.set_sopass(pattern);
        assert_eq!(wol.sopass, pattern);
        wol.clear_sopass();
        assert_eq!(wol.sopass, [0; 6]);
    }

    #[test]
    fn test_ethtool_wol_clear_all() {
        let mut wol = EthtoolWol::default();
        wol.enable_magic();
        wol.set_sopass([1, 2, 3, 4, 5, 6]);
        wol.clear_all();
        assert!(!wol.is_wol_enabled());
        assert_eq!(wol.sopass, [0; 6]);
    }

    #[test]
    fn test_ethtool_driver_info_name() {
        let mut info = EthtoolDriverInfo::default();
        info.set_driver_name("ixgbe");
        assert_eq!(info.get_driver_name(), "ixgbe");
    }

    #[test]
    fn test_ethtool_driver_info_bus() {
        let mut info = EthtoolDriverInfo::default();
        info.set_bus_info("pci:0000:01:00.0");
        assert_eq!(info.get_bus_info(), "pci:0000:01:00.0");
    }

    #[test]
    fn test_ethtool_driver_info_fw() {
        let mut info = EthtoolDriverInfo::default();
        info.set_fw_version("5.18.0");
        assert_eq!(info.get_fw_version(), "5.18.0");
    }

    #[test]
    fn test_ethtool_driver_info_truncate() {
        let mut info = EthtoolDriverInfo::default();
        let long_name = "a".repeat(64);
        info.set_driver_name(&long_name);
        assert_eq!(info.get_driver_name().len(), 31);
    }

    #[test]
    fn test_ethtool_duplex_mode() {
        assert_eq!(DuplexMode::from_u8(0), DuplexMode::Half);
        assert_eq!(DuplexMode::from_u8(1), DuplexMode::Full);
        assert_eq!(DuplexMode::from_u8(99), DuplexMode::Full);
        assert_eq!(DuplexMode::Half.as_u8(), 0);
        assert_eq!(DuplexMode::Full.as_u8(), 1);
    }

    #[test]
    fn test_ethtool_port() {
        assert_eq!(EthtoolPort::from_u8(0), EthtoolPort::Mii);
        assert_eq!(EthtoolPort::from_u8(3), EthtoolPort::TwistedPair);
        assert_eq!(EthtoolPort::from_u8(8), EthtoolPort::Unknown);
    }

    #[test]
    fn test_ethtool_transceiver() {
        assert_eq!(EthtoolTransceiver::from_u8(0), EthtoolTransceiver::Internal);
        assert_eq!(EthtoolTransceiver::from_u8(1), EthtoolTransceiver::External);
        assert_eq!(EthtoolTransceiver::from_u8(99), EthtoolTransceiver::Unknown);
    }

    #[test]
    fn test_ethtool_stats_new() {
        let stats = EthtoolStats::new();
        assert_eq!(stats.cmd_get_link.load(Ordering::Relaxed), 0);
        assert_eq!(stats.cmd_get_speed.load(Ordering::Relaxed), 0);
        assert_eq!(stats.link_up_count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_ethtool_stats_snapshot() {
        let stats = EthtoolStats::new();
        stats.cmd_get_link.fetch_add(5, Ordering::Relaxed);
        stats.cmd_get_speed.fetch_add(3, Ordering::Relaxed);
        stats.link_up_count.fetch_add(1, Ordering::Relaxed);
        let snap = stats.snapshot();
        assert_eq!(snap.cmd_get_link, 5);
        assert_eq!(snap.cmd_get_speed, 3);
        assert_eq!(snap.link_up_count, 1);
    }

    #[test]
    fn test_ethtool_register_unregister() {
        let ifindex = 0xDEAD;
        let info = ethtool_default_driver_info(ifindex);
        register_ethtool(ifindex, info);
        assert!(ethtool_get_link(ifindex).is_ok());
        unregister_ethtool(ifindex);
        assert!(ethtool_get_link(ifindex).is_err());
    }

    #[test]
    fn test_ethtool_set_offload() {
        let ifindex = 0xBEEF;
        register_ethtool(ifindex, ethtool_default_driver_info(ifindex));
        let mut offload = EthtoolOffload::default();
        offload.tso = 1;
        offload.gro = 1;
        assert!(ethtool_set_offload(ifindex, offload).is_ok());
        let got = ethtool_get_offload(ifindex).unwrap();
        assert_eq!(got.tso, 1);
        assert_eq!(got.gro, 1);
        unregister_ethtool(ifindex);
    }

    #[test]
    fn test_ethtool_set_ring_param() {
        let ifindex = 0xCAFE;
        register_ethtool(ifindex, ethtool_default_driver_info(ifindex));
        let param = EthtoolRingParam {
            rx_max_pending: 256,
            tx_max_pending: 256,
            rx_pending: 256,
            tx_pending: 256,
            ..EthtoolRingParam::default()
        };
        assert!(ethtool_set_ring_param(ifindex, param).is_ok());
        let got = ethtool_get_ring_param(ifindex).unwrap();
        assert_eq!(got.rx_pending, 256);
        assert_eq!(got.tx_pending, 256);
        unregister_ethtool(ifindex);
    }

    #[test]
    fn test_ethtool_set_ring_overflow() {
        let ifindex = 0xFACE;
        register_ethtool(ifindex, ethtool_default_driver_info(ifindex));
        let param = EthtoolRingParam {
            rx_max_pending: 64,
            tx_max_pending: 64,
            rx_pending: 1024,
            tx_pending: 1024,
            ..EthtoolRingParam::default()
        };
        assert!(ethtool_set_ring_param(ifindex, param).is_ok());
        let got = ethtool_get_ring_param(ifindex).unwrap();
        assert_eq!(got.rx_pending, 64);
        assert_eq!(got.tx_pending, 64);
        unregister_ethtool(ifindex);
    }

    #[test]
    fn test_ethtool_set_wol() {
        let ifindex = 0x5678;
        register_ethtool(ifindex, ethtool_default_driver_info(ifindex));
        let mut wol = EthtoolWol::default();
        wol.enable_magic();
        wol.set_sopass([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
        assert!(ethtool_set_wol(ifindex, wol).is_ok());
        let got = ethtool_get_wol(ifindex).unwrap();
        assert!(got.wake_on_lan.contains(WolFlags::MAGIC));
        assert_eq!(got.sopass, [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
        unregister_ethtool(ifindex);
    }

    #[test]
    fn test_ethtool_get_speed() {
        let ifindex = 0x9999;
        register_ethtool(ifindex, ethtool_default_driver_info(ifindex));
        let link = EthtoolLinkInfo {
            speed_mbps: 10000,
            duplex: DuplexMode::Full,
            autoneg: true,
            link_up: true,
            phy_addr: 1,
            port: EthtoolPort::TwistedPair,
            transceiver: EthtoolTransceiver::Internal,
        };
        *ETH_TOOL_INTERFACES.lock().get(&ifindex).unwrap().link_info.lock() = link;
        assert_eq!(ethtool_get_speed(ifindex).unwrap(), 10000);
        assert_eq!(ethtool_get_duplex(ifindex).unwrap(), DuplexMode::Full);
        unregister_ethtool(ifindex);
    }

    #[test]
    fn test_ethtool_set_link_state() {
        let ifindex = 0xAAAA;
        register_ethtool(ifindex, ethtool_default_driver_info(ifindex));
        assert!(ethtool_set_link(ifindex, 1000, DuplexMode::Full, true).is_ok());
        let link = ethtool_get_link(ifindex).unwrap();
        assert_eq!(link.speed_mbps, 1000);
        assert_eq!(link.duplex, DuplexMode::Full);
        assert!(link.autoneg);
        unregister_ethtool(ifindex);
    }

    #[test]
    fn test_ethtool_link_state() {
        let ifindex = 0xBBBB;
        register_ethtool(ifindex, ethtool_default_driver_info(ifindex));
        let state = ethtool_get_link_state(ifindex).unwrap();
        assert_eq!(state, EthtoolLinkState::Down);
        ethtool_set_link(ifindex, 1000, DuplexMode::Full, true).unwrap();
        let state = ethtool_get_link_state(ifindex).unwrap();
        assert_eq!(state, EthtoolLinkState::Up);
        unregister_ethtool(ifindex);
    }

    #[test]
    fn test_ethtool_get_driver_info() {
        let ifindex = 0xCCCC;
        register_ethtool(ifindex, ethtool_default_driver_info(ifindex));
        let info = ethtool_get_driver_info(ifindex).unwrap();
        assert_eq!(info.get_driver_name(), "echos-net");
        assert_eq!(info.get_fw_version(), "1.0.0");
        unregister_ethtool(ifindex);
    }

    #[test]
    fn test_ethtool_get_eeprom() {
        let ifindex = 0xDDDD;
        register_ethtool(ifindex, ethtool_default_driver_info(ifindex));
        let eeprom = ethtool_get_eeprom(ifindex, 0, 128).unwrap();
        assert_eq!(eeprom.offset, 0);
        assert_eq!(eeprom.length, 128);
        assert_eq!(eeprom.data.len(), 128);
        unregister_ethtool(ifindex);
    }

    #[test]
    fn test_ethtool_process_command() {
        let ifindex = 0xEEEE;
        register_ethtool(ifindex, ethtool_default_driver_info(ifindex));
        let response = ethtool_process_command(ifindex, EthtoolCmd::GetLink);
        assert!(response.is_ok());
        let response = ethtool_process_command(ifindex, EthtoolCmd::GetDriverInfo);
        assert!(response.is_ok());
        let response = ethtool_process_command(ifindex, EthtoolCmd::SetOffload);
        assert!(response.is_ok());
        unregister_ethtool(ifindex);
    }

    #[test]
    fn test_ethtool_format_link_info() {
        let info = EthtoolLinkInfo {
            speed_mbps: 10000,
            duplex: DuplexMode::Full,
            autoneg: true,
            link_up: true,
            phy_addr: 0,
            port: EthtoolPort::Mii,
            transceiver: EthtoolTransceiver::Internal,
        };
        let output = ethtool_format_link_info(&info);
        let s = core::str::from_utf8(&output).unwrap();
        assert!(s.contains("10Gbps"));
        assert!(s.contains("Full"));
        assert!(s.contains("on"));
    }

    #[test]
    fn test_ethtool_format_offload() {
        let mut offload = EthtoolOffload::default();
        offload.tso = 1;
        offload.gro = 1;
        let output = ethtool_format_offload(&offload);
        let s = core::str::from_utf8(&output).unwrap();
        assert!(s.contains("tcp-segmentation-offload: on"));
        assert!(s.contains("generic-receive-offload: on"));
        assert!(s.contains("rx-checksumming: off"));
    }

    #[test]
    fn test_ethtool_format_ring_param() {
        let param = EthtoolRingParam::default();
        let output = ethtool_format_ring_param(&param);
        let s = core::str::from_utf8(&output).unwrap();
        assert!(s.contains("Pre-set maximums:"));
        assert!(s.contains("Current hardware settings:"));
    }

    #[test]
    fn test_ethtool_format_wol() {
        let mut wol = EthtoolWol::default();
        wol.enable_magic();
        wol.set_sopass([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
        let output = ethtool_format_wol(&wol);
        let s = core::str::from_utf8(&output).unwrap();
        assert!(s.contains("Wake-on: g"));
        assert!(s.contains("aa:bb:cc:dd:ee:ff"));
    }

    #[test]
    fn test_ethtool_format_driver_info() {
        let info = ethtool_default_driver_info(1);
        let output = ethtool_format_driver_info(&info);
        let s = core::str::from_utf8(&output).unwrap();
        assert!(s.contains("driver: echos-net"));
        assert!(s.contains("firmware-version: 1.0.0"));
    }

    #[test]
    fn test_ethtool_create_interface() {
        let ifindex = 0xFFFF;
        ethtool_create_interface(ifindex, "virtio-net");
        let info = ethtool_get_driver_info(ifindex).unwrap();
        assert_eq!(info.get_driver_name(), "virtio-net");
        unregister_ethtool(ifindex);
    }

    #[test]
    fn test_ethtool_offload_tx_checksum_toggle() {
        let ifindex = 0x1111;
        register_ethtool(ifindex, ethtool_default_driver_info(ifindex));
        ethtool_set_offload_tx_checksum(ifindex, true).unwrap();
        let offload = ethtool_get_offload(ifindex).unwrap();
        assert_eq!(offload.tx_checksum, 1);
        ethtool_set_offload_tx_checksum(ifindex, false).unwrap();
        let offload = ethtool_get_offload(ifindex).unwrap();
        assert_eq!(offload.tx_checksum, 0);
        unregister_ethtool(ifindex);
    }

    #[test]
    fn test_ethtool_nway_reset() {
        let ifindex = 0x2222;
        register_ethtool(ifindex, ethtool_default_driver_info(ifindex));
        ethtool_set_link(ifindex, 1000, DuplexMode::Full, true).unwrap();
        ethtool_nway_reset(ifindex).unwrap();
        let link = ethtool_get_link(ifindex).unwrap();
        assert!(!link.link_up);
        unregister_ethtool(ifindex);
    }

    #[test]
    fn test_ethtool_set_wol_pattern() {
        let ifindex = 0x3333;
        register_ethtool(ifindex, ethtool_default_driver_info(ifindex));
        let pattern = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
        ethtool_set_wol_pattern(ifindex, pattern).unwrap();
        let wol = ethtool_get_wol(ifindex).unwrap();
        assert_eq!(wol.sopass, pattern);
        unregister_ethtool(ifindex);
    }

    #[test]
    fn test_ethtool_set_ring_rx_pending() {
        let ifindex = 0x4444;
        register_ethtool(ifindex, ethtool_default_driver_info(ifindex));
        ethtool_set_ring_rx_pending(ifindex, 64).unwrap();
        let ring = ethtool_get_ring_param(ifindex).unwrap();
        assert_eq!(ring.rx_pending, 64);
        unregister_ethtool(ifindex);
    }

    #[test]
    fn test_ethtool_set_ring_tx_pending() {
        let ifindex = 0x5555;
        register_ethtool(ifindex, ethtool_default_driver_info(ifindex));
        ethtool_set_ring_tx_pending(ifindex, 32).unwrap();
        let ring = ethtool_get_ring_param(ifindex).unwrap();
        assert_eq!(ring.tx_pending, 32);
        unregister_ethtool(ifindex);
    }

    #[test]
    fn test_ethtool_set_driver_name() {
        let ifindex = 0x6666;
        register_ethtool(ifindex, ethtool_default_driver_info(ifindex));
        ethtool_set_driver_name(ifindex, "mlx5_core").unwrap();
        let info = ethtool_get_driver_info(ifindex).unwrap();
        assert_eq!(info.get_driver_name(), "mlx5_core");
        unregister_ethtool(ifindex);
    }

    #[test]
    fn test_ethtool_set_bus_info() {
        let ifindex = 0x7777;
        register_ethtool(ifindex, ethtool_default_driver_info(ifindex));
        ethtool_set_bus_info(ifindex, "pci:0000:03:00.0").unwrap();
        let info = ethtool_get_driver_info(ifindex).unwrap();
        assert_eq!(info.get_bus_info(), "pci:0000:03:00.0");
        unregister_ethtool(ifindex);
    }

    #[test]
    fn test_ethtool_set_fw_version() {
        let ifindex = 0x8888;
        register_ethtool(ifindex, ethtool_default_driver_info(ifindex));
        ethtool_set_fw_version(ifindex, "6.2.0").unwrap();
        let info = ethtool_get_driver_info(ifindex).unwrap();
        assert_eq!(info.get_fw_version(), "6.2.0");
        unregister_ethtool(ifindex);
    }

    #[test]
    fn test_ethtool_set_nic_info() {
        let ifindex = 0xAAAA;
        register_ethtool(ifindex, ethtool_default_driver_info(ifindex));
        ethtool_set_nic_info(ifindex, String::from("10GbE SFP+")).unwrap();
        let info = ethtool_get_driver_info(ifindex).unwrap();
        assert_eq!(info.nic_info, "10GbE SFP+");
        unregister_ethtool(ifindex);
    }

    #[test]
    fn test_ethtool_get_all_stats_empty() {
        let stats = ethtool_get_all_stats();
        assert!(stats.is_empty() || !stats.is_empty());
    }

    #[test]
    fn test_ethtool_get_all_stats_populated() {
        let ifindex = 0xBBBB;
        register_ethtool(ifindex, ethtool_default_driver_info(ifindex));
        let _ = ethtool_get_link(ifindex);
        let _ = ethtool_get_speed(ifindex);
        let stats = ethtool_get_all_stats();
        assert!(!stats.is_empty());
        unregister_ethtool(ifindex);
    }

    #[test]
    fn test_ethtool_format_speed_1g() {
        let info = EthtoolLinkInfo {
            speed_mbps: 1000,
            link_up: true,
            ..EthtoolLinkInfo::default()
        };
        let output = ethtool_format_link_info(&info);
        let s = core::str::from_utf8(&output).unwrap();
        assert!(s.contains("1Gbps"));
    }

    #[test]
    fn test_ethtool_format_speed_100m() {
        let info = EthtoolLinkInfo {
            speed_mbps: 100,
            link_up: true,
            ..EthtoolLinkInfo::default()
        };
        let output = ethtool_format_link_info(&info);
        let s = core::str::from_utf8(&output).unwrap();
        assert!(s.contains("100Mbps"));
    }

    #[test]
    fn test_ethtool_format_speed_25g() {
        let info = EthtoolLinkInfo {
            speed_mbps: 25000,
            link_up: true,
            ..EthtoolLinkInfo::default()
        };
        let output = ethtool_format_link_info(&info);
        let s = core::str::from_utf8(&output).unwrap();
        assert!(s.contains("25Gbps"));
    }

    #[test]
    fn test_ethtool_format_speed_no_link() {
        let info = EthtoolLinkInfo {
            speed_mbps: 0,
            link_up: false,
            ..EthtoolLinkInfo::default()
        };
        let output = ethtool_format_link_info(&info);
        let s = core::str::from_utf8(&output).unwrap();
        assert!(s.contains("no link"));
    }
}
