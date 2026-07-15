use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, AtomicU64, AtomicBool, Ordering};
use spin::Mutex;
use lazy_static::lazy_static;

pub const WIPHY_MAX: usize = 32;
pub const WDEV_MAX: usize = 64;
pub const STA_MAX: usize = 256;
pub const BSS_CACHE_MAX: usize = 512;
pub const MAC_LEN: usize = 6;
pub const WIPHY_NAME_MAX: usize = 64;
pub const IFNAME_MAX: usize = 16;
pub const SSID_MAX_LEN: usize = 32;

pub const AES_CMAC_128: u32 = 0x000F_AC04;
pub const AES_CCMP: u32 = 0x000F_AC04;
pub const BIP_CMAC_128: u32 = 0x000F_AC06;
pub const GCMP_128: u32 = 0x000F_AC08;
pub const GCMP_256: u32 = 0x000F_AC09;
pub const CCMP_256: u32 = 0x000F_AC0A;
pub const BIP_GMAC_128: u32 = 0x000F_AC0B;
pub const BIP_GMAC_256: u32 = 0x000F_AC0C;
pub const BIP_CMAC_256: u32 = 0x000F_AC0D;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Nl80211Band {
    Band2GHz = 0,
    Band5GHz = 1,
    Band60GHz = 2,
    Band6GHz = 3,
    BandS1GHz = 4,
    BandLC = 5,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Nl80211Iftype {
    Unspecified = 0,
    Adhoc = 1,
    Station = 2,
    Ap = 3,
    ApVlan = 4,
    Wds = 5,
    Monitor = 6,
    MeshPoint = 7,
    P2pClient = 8,
    P2pGo = 9,
    P2pDevice = 10,
    Ocb = 11,
    Nan = 12,
}

#[derive(Clone, Debug)]
pub struct Wiphy {
    pub id: u32,
    pub name: alloc::string::String,
    pub mac: [u8; MAC_LEN],
    pub generation: u32,
    pub feature_flags: u32,
    pub ext_features: Vec<u8>,
    pub cipher_suites: Vec<u32>,
    pub supported_iftypes: u32,
    pub supported_commands: Vec<u32>,
    pub max_scan_ssids: u8,
    pub max_sched_scan_ssids: u8,
    pub max_match_sets: u8,
    pub max_remain_on_channel: u32,
    pub max_scan_ie_len: u16,
    pub max_sched_scan_ie_len: u16,
    pub max_num_pmkids: u8,
    pub max_csa_counters: u8,
    pub coverage_class: u8,
    pub frag_threshold: u32,
    pub rts_threshold: u32,
    pub retry_short: u8,
    pub retry_long: u8,
    pub antenna_tx: u32,
    pub antenna_rx: u32,
    pub antenna_avail_tx: u32,
    pub antenna_avail_rx: u32,
    pub bands: [Option<BandInfo>; 6],
    pub interfaces: BTreeMap<u32, u32>,
    pub self_managed_reg: bool,
}

#[derive(Clone, Debug)]
pub struct BandInfo {
    pub freqs: Vec<FrequencyInfo>,
    pub rates: Vec<BitrateInfo>,
    pub ht_mcs_set: Option<[u8; 128]>,
    pub ht_capa: u16,
    pub ht_ampdu_factor: u8,
    pub ht_ampdu_density: u8,
    pub vht_mcs_set: Option<[u8; 8]>,
    pub vht_capa: u32,
}

#[derive(Clone, Debug)]
pub struct FrequencyInfo {
    pub freq: u32,
    pub disabled: bool,
    pub no_ir: bool,
    pub radar: bool,
    pub max_tx_power: u32,
    pub dfs_state: u32,
}

#[derive(Clone, Debug)]
pub struct BitrateInfo {
    pub rate: u32,
    pub short_preamble: bool,
}

#[derive(Clone, Debug)]
pub struct WirelessInterface {
    pub ifindex: u32,
    pub wdev: u64,
    pub wiphy_id: u32,
    pub iftype: Nl80211Iftype,
    pub name: alloc::string::String,
    pub mac: [u8; MAC_LEN],
    pub generation: u32,
    pub four_addr: bool,
    pub txq_limit: u32,
    pub txq_memory_limit: u32,
    pub txq_quantum: u32,
}

#[derive(Clone, Debug)]
pub struct StationInfo {
    pub mac: [u8; MAC_LEN],
    pub ifindex: u32,
    pub generation: u32,
    pub aid: u16,
    pub flags: u32,
    pub listen_interval: u16,
    pub supported_rates: Vec<u8>,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_packets: u64,
    pub tx_packets: u64,
    pub signal: u32,
    pub beacon_signal: u32,
    pub tx_retries: u32,
    pub tx_failed: u32,
    pub beacon_loss: u32,
    pub inactive_ms: u32,
    pub connected_time: u32,
}

#[derive(Clone, Debug)]
pub struct BssEntry {
    pub bssid: [u8; MAC_LEN],
    pub freq: u32,
    pub beacon_interval: u16,
    pub capability: u16,
    pub signal: u32,
    pub ie: Vec<u8>,
    pub ssid: Vec<u8>,
    pub generation: u32,
    pub timestamp: u64,
}

lazy_static! {
    pub(crate) static ref WIPHY_REGISTRY: Arc<Mutex<BTreeMap<u32, Arc<Mutex<Wiphy>>>>> = {
        Arc::new(Mutex::new(BTreeMap::new()))
    };
    pub(crate) static ref WDEV_REGISTRY: Arc<Mutex<BTreeMap<u64, Arc<Mutex<WirelessInterface>>>>> = {
        Arc::new(Mutex::new(BTreeMap::new()))
    };
    pub(crate) static ref STATION_REGISTRY: Arc<Mutex<BTreeMap<(u32, [u8; MAC_LEN]), Arc<Mutex<StationInfo>>>>> = {
        Arc::new(Mutex::new(BTreeMap::new()))
    };
    pub(crate) static ref BSS_CACHE: Arc<Mutex<Vec<BssEntry>>> = {
        Arc::new(Mutex::new(Vec::new()))
    };
    static ref NEXT_WIPHY_ID: AtomicU32 = AtomicU32::new(1);
    static ref NEXT_WDEV: AtomicU64 = AtomicU64::new(1);
    pub(crate) static ref GLOBAL_GENERATION: AtomicU32 = AtomicU32::new(1);
}

pub fn get_wiphy(id: u32) -> Option<Arc<Mutex<Wiphy>>> {
    WIPHY_REGISTRY.lock().get(&id).cloned()
}

pub fn get_or_create_wiphy(id: u32, name: &str) -> Arc<Mutex<Wiphy>> {
    let mut registry = WIPHY_REGISTRY.lock();
    if let Some(w) = registry.get(&id) {
        return w.clone();
    }
    let wiphy = Arc::new(Mutex::new(Wiphy {
        id,
        name: alloc::string::String::from(name),
        mac: [0x02, 0x00, 0x00, 0x00, 0x00, 0x01],
        generation: 1,
        feature_flags: 0,
        ext_features: Vec::new(),
        cipher_suites: vec![AES_CCMP, AES_CMAC_128, BIP_CMAC_128, GCMP_128],
        supported_iftypes: (1u32 << Nl80211Iftype::Station as u32)
            | (1u32 << Nl80211Iftype::Ap as u32)
            | (1u32 << Nl80211Iftype::Monitor as u32),
        supported_commands: Vec::new(),
        max_scan_ssids: 10,
        max_sched_scan_ssids: 10,
        max_match_sets: 10,
        max_remain_on_channel: 5000,
        max_scan_ie_len: 500,
        max_sched_scan_ie_len: 500,
        max_num_pmkids: 32,
        max_csa_counters: 1,
        coverage_class: 0,
        frag_threshold: 2346,
        rts_threshold: 2347,
        retry_short: 7,
        retry_long: 4,
        antenna_tx: 1,
        antenna_rx: 1,
        antenna_avail_tx: 1,
        antenna_avail_rx: 1,
        bands: Default::default(),
        interfaces: BTreeMap::new(),
        self_managed_reg: false,
    }));
    registry.insert(id, wiphy.clone());
    wiphy
}

pub fn get_interface(ifindex: u32) -> Option<Arc<Mutex<WirelessInterface>>> {
    for wdev in WDEV_REGISTRY.lock().values() {
        let iface = wdev.lock();
        if iface.ifindex == ifindex {
            return Some(wdev.clone());
        }
    }
    None
}

pub fn get_interface_by_wdev(wdev_id: u64) -> Option<Arc<Mutex<WirelessInterface>>> {
    WDEV_REGISTRY.lock().get(&wdev_id).cloned()
}

static NEXT_IFINDEX: AtomicU32 = AtomicU32::new(1);

pub fn create_interface(
    wiphy_id: u32,
    name: &str,
    iftype: Nl80211Iftype,
    mac: [u8; MAC_LEN],
) -> Arc<Mutex<WirelessInterface>> {
    let ifindex = NEXT_IFINDEX.fetch_add(1, Ordering::Relaxed);
    let wdev = NEXT_WDEV.fetch_add(1, Ordering::Relaxed);
    let gen = GLOBAL_GENERATION.fetch_add(1, Ordering::Relaxed);
    let iface = Arc::new(Mutex::new(WirelessInterface {
        ifindex,
        wdev,
        wiphy_id,
        iftype,
        name: alloc::string::String::from(name),
        mac,
        generation: gen,
        four_addr: false,
        txq_limit: 0,
        txq_memory_limit: 0,
        txq_quantum: 0,
    }));
    WDEV_REGISTRY.lock().insert(wdev, iface.clone());
    {
        let w = get_or_create_wiphy(wiphy_id, "phy0");
        w.lock().interfaces.insert(wdev as u32, ifindex);
    }
    iface
}

pub fn delete_interface(wdev: u64) -> bool {
    if let Some(iface) = WDEV_REGISTRY.lock().remove(&wdev) {
        let wiphy_id = iface.lock().wiphy_id;
        if let Some(w) = get_wiphy(wiphy_id) {
            w.lock().interfaces.remove(&(wdev as u32));
        }
        GLOBAL_GENERATION.fetch_add(1, Ordering::Relaxed);
        true
    } else {
        false
    }
}

pub fn get_station(ifindex: u32, mac: &[u8; MAC_LEN]) -> Option<Arc<Mutex<StationInfo>>> {
    STATION_REGISTRY.lock().get(&(ifindex, *mac)).cloned()
}

pub fn get_or_create_station(ifindex: u32, mac: [u8; MAC_LEN]) -> Arc<Mutex<StationInfo>> {
    let mut reg = STATION_REGISTRY.lock();
    let entry = reg.entry((ifindex, mac));
    Arc::new(Mutex::new(StationInfo {
        mac,
        ifindex,
        generation: GLOBAL_GENERATION.fetch_add(1, Ordering::Relaxed),
        aid: 0,
        flags: 0,
        listen_interval: 100,
        supported_rates: Vec::new(),
        rx_bytes: 0,
        tx_bytes: 0,
        rx_packets: 0,
        tx_packets: 0,
        signal: 0,
        beacon_signal: 0,
        tx_retries: 0,
        tx_failed: 0,
        beacon_loss: 0,
        inactive_ms: 0,
        connected_time: 0,
    }))
}

pub fn add_bss_entry(bss: BssEntry) {
    let mut cache = BSS_CACHE.lock();
    if cache.len() >= BSS_CACHE_MAX {
        cache.remove(0);
    }
    cache.push(bss);
    GLOBAL_GENERATION.fetch_add(1, Ordering::Relaxed);
}

pub fn get_generation() -> u32 {
    GLOBAL_GENERATION.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wiphy_create_and_get() {
        let w = get_or_create_wiphy(1, "phy0");
        assert_eq!(w.lock().name, "phy0");
        assert_eq!(w.lock().id, 1);
        let w2 = get_wiphy(1);
        assert!(w2.is_some());
    }

    #[test]
    fn test_interface_create_delete() {
        let iface = create_interface(1, "wlan0", Nl80211Iftype::Station, [0x02; 6]);
        assert!(iface.lock().ifindex > 0);
        assert!(get_interface(iface.lock().ifindex).is_some());
        assert!(delete_interface(iface.lock().wdev));
        assert!(get_interface(iface.lock().ifindex).is_none());
    }

    #[test]
    fn test_station_create() {
        let mac = [0xaa; 6];
        let sta = get_or_create_station(1, mac);
        assert_eq!(sta.lock().mac, mac);
        let sta2 = get_station(1, &mac);
        assert!(sta2.is_some());
    }
}
