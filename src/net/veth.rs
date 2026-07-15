use super::{register_interface, MacAddr, NetError, NetInterface, NetStats, Ipv4Addr};
use super::Mutex;
use alloc::sync::Arc;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

/// Bir veth ucunun durumu (çift tarafından paylaşılır)
#[derive(Debug)]
pub struct VethEnd {
    pub name: String,
    pub mac: MacAddr,
    pub mtu: u16,
    pub ip: Ipv4Addr,
    pub netmask: Ipv4Addr,
    pub gateway: Option<Ipv4Addr>,
    pub rx_queue: VecDeque<Vec<u8>>,
    pub rx_packets: u64,
    pub tx_packets: u64,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_drops: u64,
    pub up: bool,
}

impl VethEnd {
    pub fn new(name: String, mac: MacAddr, mtu: u16) -> Self {
        VethEnd {
            name,
            mac,
            mtu,
            ip: Ipv4Addr::UNSPECIFIED,
            netmask: Ipv4Addr::new(255, 255, 255, 0),
            gateway: None,
            rx_queue: VecDeque::new(),
            rx_packets: 0,
            tx_packets: 0,
            rx_bytes: 0,
            tx_bytes: 0,
            rx_drops: 0,
            up: true,
        }
    }
}

/// Bir veth çifti — iki uç, paylaşılan Mutex içinde
pub struct VethPair {
    pub name: String,
    pub ends: [VethEnd; 2],
    pub created_ticks: u64,
}

impl VethPair {
    pub fn new(name: String, mac_a: MacAddr, mac_b: MacAddr) -> Self {
        let end_a_name = alloc::format!("{}_a", name);
        let end_b_name = alloc::format!("{}_b", name);
        VethPair {
            name,
            ends: [
                VethEnd::new(end_a_name, mac_a, 1500),
                VethEnd::new(end_b_name, mac_b, 1500),
            ],
            created_ticks: crate::interrupts::get_ticks(),
        }
    }
}

pub const MAX_VETH_RX_QUEUE: usize = 256;

/// NetInterface wrapper — bir veth çiftinin bir ucunu sarar
pub struct VethInterface {
    pair: Arc<Mutex<VethPair>>,
    end_index: usize,
    cached_name: String,
    cached_mac: MacAddr,
}

impl VethInterface {
    pub fn new(pair: Arc<Mutex<VethPair>>, end_index: usize) -> Self {
        let (cached_name, cached_mac) = {
            let g = pair.lock();
            (g.ends[end_index].name.clone(), g.ends[end_index].mac)
        };
        VethInterface { pair, end_index, cached_name, cached_mac }
    }
}

impl NetInterface for VethInterface {
    fn name(&self) -> &str {
        &self.cached_name
    }
    fn mac(&self) -> MacAddr {
        self.cached_mac
    }
    fn ip(&self) -> Ipv4Addr {
        self.pair.lock().ends[self.end_index].ip
    }
    fn set_ip(&mut self, ip: Ipv4Addr) {
        self.pair.lock().ends[self.end_index].ip = ip;
    }
    fn netmask(&self) -> Ipv4Addr {
        self.pair.lock().ends[self.end_index].netmask
    }
    fn set_netmask(&mut self, mask: Ipv4Addr) {
        self.pair.lock().ends[self.end_index].netmask = mask;
    }
    fn gateway(&self) -> Option<Ipv4Addr> {
        self.pair.lock().ends[self.end_index].gateway
    }
    fn set_gateway(&mut self, gw: Ipv4Addr) {
        self.pair.lock().ends[self.end_index].gateway = Some(gw);
    }
    fn is_up(&self) -> bool {
        self.pair.lock().ends[self.end_index].up
    }
    fn set_up(&mut self, up: bool) {
        self.pair.lock().ends[self.end_index].up = up;
    }
    fn send(&mut self, data: &[u8]) -> Result<(), NetError> {
        let mut pair = self.pair.lock();
        let from = self.end_index;
        let peer = 1 - from;
        if !pair.ends[from].up {
            pair.ends[from].rx_drops += 1;
            return Err(NetError::NotUp);
        }
        if data.len() > pair.ends[from].mtu as usize {
            return Err(NetError::InvalidPacket);
        }
        pair.ends[from].tx_packets += 1;
        pair.ends[from].tx_bytes += data.len() as u64;
        if !pair.ends[peer].up {
            return Err(NetError::NotUp);
        }
        if pair.ends[peer].rx_queue.len() >= MAX_VETH_RX_QUEUE {
            pair.ends[peer].rx_drops += 1;
            return Err(NetError::BufferFull);
        }
        pair.ends[peer].rx_packets += 1;
        pair.ends[peer].rx_bytes += data.len() as u64;
        pair.ends[peer].rx_queue.push_back(data.to_vec());
        Ok(())
    }
    fn recv(&mut self) -> Option<Vec<u8>> {
        self.pair.lock().ends[self.end_index].rx_queue.pop_front()
    }
    fn stats(&self) -> NetStats {
        let e = &self.pair.lock().ends[self.end_index];
        NetStats {
            rx_packets: e.rx_packets,
            tx_packets: e.tx_packets,
            rx_bytes: e.rx_bytes,
            tx_bytes: e.tx_bytes,
            rx_errors: e.rx_drops,
            tx_errors: 0,
            rx_dropped: e.rx_drops,
            tx_dropped: 0,
        }
    }
    fn mtu(&self) -> u16 {
        self.pair.lock().ends[self.end_index].mtu
    }
}

static VETH_PAIRS: Mutex<BTreeMap<String, Arc<Mutex<VethPair>>>> = Mutex::new(BTreeMap::new());

static VETH_STATS: VethStats = VethStats::new();
struct VethStats {
    pairs: AtomicU32,
    frames_passed: AtomicU32,
    drops: AtomicU32,
}
impl VethStats {
    const fn new() -> Self {
        VethStats { pairs: AtomicU32::new(0), frames_passed: AtomicU32::new(0), drops: AtomicU32::new(0) }
    }
}

pub fn create_pair(name: &str, mac_a: MacAddr, mac_b: MacAddr) -> Result<(String, String), &'static str> {
    let mut pairs = VETH_PAIRS.lock();
    if pairs.contains_key(name) {
        return Err("already exists");
    }
    let pair = Arc::new(Mutex::new(VethPair::new(String::from(name), mac_a, mac_b)));
    let end_a_name;
    let end_b_name;
    {
        let guard = pair.lock();
        end_a_name = guard.ends[0].name.clone();
        end_b_name = guard.ends[1].name.clone();
    }
    pairs.insert(String::from(name), pair.clone());
    VETH_STATS.pairs.fetch_add(1, Ordering::Relaxed);
    register_interface(Arc::new(Mutex::new(VethInterface::new(pair.clone(), 0))));
    register_interface(Arc::new(Mutex::new(VethInterface::new(pair, 1))));
    crate::serial_println!("[VETH] created {} ({} <-> {})", name, end_a_name, end_b_name);
    Ok((end_a_name, end_b_name))
}

pub fn destroy(name: &str) -> Result<(), &'static str> {
    let mut pairs = VETH_PAIRS.lock();
    pairs.remove(name).ok_or("not found")?;
    VETH_STATS.pairs.fetch_sub(1, Ordering::Relaxed);
    Ok(())
}

pub fn lookup_pair(end_name: &str) -> Option<String> {
    VETH_PAIRS.lock().values().find_map(|p| {
        let g = p.lock();
        if g.ends[0].name == end_name || g.ends[1].name == end_name {
            Some(g.name.clone())
        } else { None }
    })
}

pub fn push_to_peer(pair_name: &str, from_end: usize, frame: Vec<u8>) -> Result<(), &'static str> {
    let mut pairs = VETH_PAIRS.lock();
    let pair = pairs.get_mut(pair_name).ok_or("pair not found")?;
    let mut guard = pair.lock();
    let peer = 1 - from_end;
    if !guard.ends[from_end].up || !guard.ends[peer].up { return Err("end down"); }
    if frame.len() > guard.ends[from_end].mtu as usize { return Err("mtu exceeded"); }
    if guard.ends[peer].rx_queue.len() >= MAX_VETH_RX_QUEUE { return Err("queue full"); }
    guard.ends[from_end].tx_packets += 1;
    guard.ends[from_end].tx_bytes += frame.len() as u64;
    guard.ends[peer].rx_packets += 1;
    guard.ends[peer].rx_bytes += frame.len() as u64;
    guard.ends[peer].rx_queue.push_back(frame);
    VETH_STATS.frames_passed.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

pub fn pop(pair_name: &str, end: usize) -> Option<Vec<u8>> {
    VETH_PAIRS.lock().get(pair_name).and_then(|p| p.lock().ends[end].rx_queue.pop_front())
}

pub fn stats(pair_name: &str, end: usize) -> Option<VethEnd> {
    let pairs = VETH_PAIRS.lock();
    let p = pairs.get(pair_name)?;
    let g = p.lock();
    Some(VethEnd {
        name: g.ends[end].name.clone(),
        mac: g.ends[end].mac,
        mtu: g.ends[end].mtu,
        ip: g.ends[end].ip,
        netmask: g.ends[end].netmask,
        gateway: g.ends[end].gateway,
        rx_queue: VecDeque::new(),
        rx_packets: g.ends[end].rx_packets,
        tx_packets: g.ends[end].tx_packets,
        rx_bytes: g.ends[end].rx_bytes,
        tx_bytes: g.ends[end].tx_bytes,
        rx_drops: g.ends[end].rx_drops,
        up: g.ends[end].up,
    })
}

pub fn set_up(pair_name: &str, end: usize, up: bool) -> Result<(), &'static str> {
    let pairs = VETH_PAIRS.lock();
    let pair = pairs.get(pair_name).ok_or("not found")?;
    pair.lock().ends[end].up = up;
    Ok(())
}

pub fn global_stats() -> (u32, u32, u32) {
    (VETH_STATS.pairs.load(Ordering::Relaxed), VETH_STATS.frames_passed.load(Ordering::Relaxed), VETH_STATS.drops.load(Ordering::Relaxed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::NetInterface;

    #[test]
    fn frame_passed_from_a_to_b() {
        let pair = Arc::new(Mutex::new(VethPair::new("test".into(), MacAddr([1; 6]), MacAddr([2; 6]))));
        let mut iface_a = VethInterface::new(pair.clone(), 0);
        let mut iface_b = VethInterface::new(pair, 1);
        iface_a.send(&[1, 2, 3, 4]).unwrap();
        let received = iface_b.recv().unwrap();
        assert_eq!(received, vec![1, 2, 3, 4]);
    }

    #[test]
    fn frame_passed_from_b_to_a() {
        let pair = Arc::new(Mutex::new(VethPair::new("test".into(), MacAddr([1; 6]), MacAddr([2; 6]))));
        let mut iface_a = VethInterface::new(pair.clone(), 0);
        let mut iface_b = VethInterface::new(pair, 1);
        iface_b.send(&[0xAA, 0xBB]).unwrap();
        let received = iface_a.recv().unwrap();
        assert_eq!(received, vec![0xAA, 0xBB]);
    }

    #[test]
    fn rx_queue_full_drops() {
        let pair = Arc::new(Mutex::new(VethPair::new("test".into(), MacAddr([1; 6]), MacAddr([2; 6]))));
        let mut iface_a = VethInterface::new(pair.clone(), 0);
        for i in 0..MAX_VETH_RX_QUEUE {
            iface_a.send(&[1; 64]).unwrap();
        }
        let r = iface_a.send(&[1; 64]);
        assert!(r.is_err());
    }

    #[test]
    fn down_end_drops_frame() {
        let pair = Arc::new(Mutex::new(VethPair::new("test".into(), MacAddr([1; 6]), MacAddr([2; 6]))));
        let mut iface_a = VethInterface::new(pair.clone(), 0);
        iface_a.set_up(false);
        let r = iface_a.send(&[1, 2, 3]);
        assert!(r.is_err());
    }

    #[test]
    fn veth_interface_insert_matches_name() {
        let pair = Arc::new(Mutex::new(VethPair::new("test".into(), MacAddr([1; 6]), MacAddr([2; 6]))));
        let iface_a = VethInterface::new(pair.clone(), 0);
        let iface_b = VethInterface::new(pair, 1);
        assert_eq!(iface_a.name(), "test_a");
        assert_eq!(iface_b.name(), "test_b");
        assert_eq!(iface_a.mac(), MacAddr([1; 6]));
        assert_eq!(iface_b.mac(), MacAddr([2; 6]));
    }

    #[test]
    fn create_pair_global_api() {
        let result = create_pair("test_pair", MacAddr([0xAA; 6]), MacAddr([0xBB; 6]));
        assert!(result.is_ok());
        let (name_a, name_b) = result.unwrap();
        assert_eq!(name_a, "test_pair_a");
        assert_eq!(name_b, "test_pair_b");
        assert!(lookup_pair("test_pair_a").is_some());
        destroy("test_pair").unwrap();
    }
}
