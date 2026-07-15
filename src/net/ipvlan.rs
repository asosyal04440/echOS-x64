use super::{
    get_interface, register_interface, Ipv4Addr, MacAddr, NetError, NetInterface, NetStats,
};
use super::Mutex;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

pub const IPVLAN_DEFAULT_MTU: u16 = 1500;
pub const ETH_HLEN: usize = 14;
pub const ETH_P_ALL: u16 = 0x0003;

static NEXT_IFINDEX: AtomicU32 = AtomicU32::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IpvlanMode {
    L2,
    L3,
    L3s,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IpvlanFlag {
    Bridge,
    Private,
    Vepa,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IpvlanResult {
    TxToMaster,
    TxToSlave(u32),
    TxToExternal,
    Drop,
    DeliverLocally,
}

pub struct IpvlanDevice {
    pub master_dev_index: u32,
    pub mode: IpvlanMode,
    pub flag: IpvlanFlag,
    pub ifindex: u32,
    pub mtu: u16,
    pub num_queues: u16,
    pub slaves: Vec<IpvlanSlave>,
}

impl IpvlanDevice {
    pub fn new(master_dev_index: u32, mode: IpvlanMode, flag: IpvlanFlag) -> Self {
        let ifindex = NEXT_IFINDEX.fetch_add(1, Ordering::Relaxed);
        IpvlanDevice {
            master_dev_index,
            mode,
            flag,
            ifindex,
            mtu: IPVLAN_DEFAULT_MTU,
            num_queues: 1,
            slaves: Vec::new(),
        }
    }

    pub fn find_slave(&self, dev_ifindex: u32) -> Option<&IpvlanSlave> {
        self.slaves.iter().find(|s| s.dev_ifindex == dev_ifindex)
    }

    pub fn add_slave(&mut self, slave: IpvlanSlave) {
        self.slaves.push(slave);
    }

    pub fn remove_slave(&mut self, dev_ifindex: u32) -> bool {
        let len_before = self.slaves.len();
        self.slaves.retain(|s| s.dev_ifindex != dev_ifindex);
        self.slaves.len() < len_before
    }
}

#[derive(Clone, Debug)]
pub struct IpvlanSlave {
    pub parent_ifindex: u32,
    pub dev_ifindex: u32,
    pub mode: IpvlanMode,
    pub flag: IpvlanFlag,
    pub mac_addr: MacAddr,
    pub mtu: u16,
    pub up: bool,
    pub name: String,
    pub ip: Ipv4Addr,
    pub netmask: Ipv4Addr,
    pub gateway: Option<Ipv4Addr>,
    pub rx_packets: u64,
    pub tx_packets: u64,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

impl IpvlanSlave {
    pub fn new(parent_ifindex: u32, mode: IpvlanMode, flag: IpvlanFlag, mac_addr: MacAddr) -> Self {
        let dev_ifindex = NEXT_IFINDEX.fetch_add(1, Ordering::Relaxed);
        IpvlanSlave {
            parent_ifindex,
            dev_ifindex,
            mode,
            flag,
            mac_addr,
            mtu: IPVLAN_DEFAULT_MTU,
            up: true,
            name: alloc::format!("ipvlan.{}", dev_ifindex),
            ip: Ipv4Addr::new(0, 0, 0, 0),
            netmask: Ipv4Addr::new(255, 255, 255, 0),
            gateway: None,
            rx_packets: 0,
            tx_packets: 0,
            rx_bytes: 0,
            tx_bytes: 0,
        }
    }
}

pub fn ipvlan_create_slave(
    parent_ifindex: u32,
    _name: &str,
    mode: IpvlanMode,
    flag: IpvlanFlag,
) -> IpvlanSlave {
    let mac = MacAddr([
        0x02,
        ((parent_ifindex >> 24) & 0xFF) as u8,
        ((parent_ifindex >> 16) & 0xFF) as u8,
        ((parent_ifindex >> 8) & 0xFF) as u8,
        (parent_ifindex & 0xFF) as u8,
        0x00,
    ]);
    IpvlanSlave::new(parent_ifindex, mode, flag, mac)
}

pub fn ipvlan_xmit(slave: &IpvlanSlave, packet: &[u8]) -> IpvlanResult {
    if packet.len() < ETH_HLEN {
        return IpvlanResult::Drop;
    }

    let is_mc = ipvlan_frame_is_multicast(packet);

    match slave.mode {
        IpvlanMode::L2 => {
            if is_mc {
                IpvlanResult::TxToMaster
            } else {
                IpvlanResult::TxToMaster
            }
        }
        IpvlanMode::L3 => {
            if is_mc {
                IpvlanResult::Drop
            } else {
                IpvlanResult::TxToMaster
            }
        }
        IpvlanMode::L3s => {
            if is_mc {
                IpvlanResult::Drop
            } else {
                IpvlanResult::TxToMaster
            }
        }
    }
}

pub fn ipvlan_frame_is_multicast(packet: &[u8]) -> bool {
    if packet.len() < ETH_HLEN {
        return false;
    }
    let dst = &packet[0..6];
    if dst == &[0xFF; 6] {
        return true;
    }
    dst[0] & 0x01 != 0
}

pub fn ipvlan_allow_cross_traffic(
    slave1: &IpvlanSlave,
    slave2: &IpvlanSlave,
) -> bool {
    if slave1.parent_ifindex != slave2.parent_ifindex {
        return false;
    }
    match slave1.flag {
        IpvlanFlag::Bridge => true,
        IpvlanFlag::Private => false,
        IpvlanFlag::Vepa => false,
    }
}

pub fn ipvlan_rx(slave: &IpvlanSlave, packet: &[u8]) -> bool {
    if packet.len() < ETH_HLEN {
        return false;
    }
    let is_mc = ipvlan_frame_is_multicast(packet);
    match slave.mode {
        IpvlanMode::L2 => true,
        IpvlanMode::L3 => !is_mc,
        IpvlanMode::L3s => true,
    }
}

pub struct IpvlanInterface {
    switch: Arc<Mutex<IpvlanSwitch>>,
    slave_ifindex: u32,
    cached_name: String,
    cached_mac: MacAddr,
    ip: Ipv4Addr,
    netmask: Ipv4Addr,
    gateway: Option<Ipv4Addr>,
    up: bool,
}

impl IpvlanInterface {
    pub fn new(
        switch: Arc<Mutex<IpvlanSwitch>>,
        slave_ifindex: u32,
        mac: MacAddr,
        name: &str,
    ) -> Self {
        IpvlanInterface {
            switch,
            slave_ifindex,
            cached_name: name.into(),
            cached_mac: mac,
            ip: Ipv4Addr::new(0, 0, 0, 0),
            netmask: Ipv4Addr::new(255, 255, 255, 0),
            gateway: None,
            up: true,
        }
    }

    pub fn register(
        switch: Arc<Mutex<IpvlanSwitch>>,
        dev_ifindex: u32,
        parent_ifindex: u32,
        mode: IpvlanMode,
        flag: IpvlanFlag,
        name: &str,
    ) -> Result<Arc<Mutex<dyn NetInterface>>, &'static str> {
        let slave_ifindex =
            switch.lock().add_slave(dev_ifindex, parent_ifindex, mode, flag)?;
        let mac = switch.lock().slave_mac(slave_ifindex);
        let iface = IpvlanInterface::new(switch, slave_ifindex, mac, name);
        let iface_arc = Arc::new(Mutex::new(iface)) as Arc<Mutex<dyn NetInterface>>;
        register_interface(iface_arc.clone());
        Ok(iface_arc)
    }
}

impl NetInterface for IpvlanInterface {
    fn name(&self) -> &str {
        &self.cached_name
    }

    fn mac(&self) -> MacAddr {
        self.cached_mac
    }

    fn ip(&self) -> Ipv4Addr {
        self.ip
    }

    fn set_ip(&mut self, ip: Ipv4Addr) {
        self.ip = ip;
    }

    fn netmask(&self) -> Ipv4Addr {
        self.netmask
    }

    fn set_netmask(&mut self, mask: Ipv4Addr) {
        self.netmask = mask;
    }

    fn gateway(&self) -> Option<Ipv4Addr> {
        self.gateway
    }

    fn set_gateway(&mut self, gw: Ipv4Addr) {
        self.gateway = Some(gw);
    }

    fn is_up(&self) -> bool {
        self.up
    }

    fn set_up(&mut self, up: bool) {
        self.up = up;
    }

    fn send(&mut self, data: &[u8]) -> Result<(), NetError> {
        if !self.up {
            return Err(NetError::NotUp);
        }
        let sw = self.switch.lock();
        let result = sw.process_tx(self.slave_ifindex, data);
        drop(sw);
        match result {
            IpvlanResult::Drop => Err(NetError::InvalidPacket),
            IpvlanResult::TxToExternal => {
                let sw = self.switch.lock();
                if let Some(&dev_ifindex) = sw.slave_map.get(&self.slave_ifindex) {
                    if let Some(dev) = sw.devices.get(&dev_ifindex) {
                        let parent_name = alloc::format!("eth{}", dev.master_dev_index);
                        drop(sw);
                        if let Some(parent) = get_interface(&parent_name) {
                            let mut guard = parent.lock();
                            return guard.send(data);
                        }
                    }
                }
                Err(NetError::NoInterface)
            }
            _ => Ok(()),
        }
    }

    fn recv(&mut self) -> Option<Vec<u8>> {
        if !self.up {
            return None;
        }
        let sw = self.switch.lock();
        if let Some(&dev_ifindex) = sw.slave_map.get(&self.slave_ifindex) {
            if let Some(dev) = sw.devices.get(&dev_ifindex) {
                let parent_name = alloc::format!("eth{}", dev.master_dev_index);
                drop(sw);
                if let Some(parent) = get_interface(&parent_name) {
                    let mut guard = parent.lock();
                    if let Some(pkt) = guard.recv() {
                        let sw = self.switch.lock();
                        if sw.should_deliver_to_slave(self.slave_ifindex, &pkt) {
                            return Some(pkt);
                        }
                    }
                }
            }
        }
        None
    }

    fn stats(&self) -> NetStats {
        let sw = self.switch.lock();
        if let Some(&dev_ifindex) = sw.slave_map.get(&self.slave_ifindex) {
            if let Some(dev) = sw.devices.get(&dev_ifindex) {
                if let Some(slave) = dev.find_slave(self.slave_ifindex) {
                    return NetStats {
                        rx_packets: slave.rx_packets,
                        tx_packets: slave.tx_packets,
                        rx_bytes: slave.rx_bytes,
                        tx_bytes: slave.tx_bytes,
                        rx_errors: 0,
                        tx_errors: 0,
                        rx_dropped: 0,
                        tx_dropped: 0,
                    };
                }
            }
        }
        NetStats::default()
    }

    fn mtu(&self) -> u16 {
        IPVLAN_DEFAULT_MTU
    }
}

pub struct IpvlanSwitch {
    pub devices: BTreeMap<u32, IpvlanDevice>,
    pub slave_map: BTreeMap<u32, u32>,
}

impl IpvlanSwitch {
    pub fn new() -> Self {
        IpvlanSwitch {
            devices: BTreeMap::new(),
            slave_map: BTreeMap::new(),
        }
    }

    pub fn create_device(
        &mut self,
        master_dev_index: u32,
        mode: IpvlanMode,
        flag: IpvlanFlag,
    ) -> u32 {
        let dev = IpvlanDevice::new(master_dev_index, mode, flag);
        let ifindex = dev.ifindex;
        self.devices.insert(ifindex, dev);
        ifindex
    }

    pub fn add_slave(
        &mut self,
        dev_ifindex: u32,
        parent_ifindex: u32,
        mode: IpvlanMode,
        flag: IpvlanFlag,
    ) -> Result<u32, &'static str> {
        let slave = ipvlan_create_slave(parent_ifindex, "", mode, flag);
        let slave_ifindex = slave.dev_ifindex;

        if let Some(dev) = self.devices.get_mut(&dev_ifindex) {
            dev.add_slave(slave);
            self.slave_map.insert(slave_ifindex, dev_ifindex);
            Ok(slave_ifindex)
        } else {
            Err("device not found")
        }
    }

    pub fn process_tx(&self, slave_ifindex: u32, packet: &[u8]) -> IpvlanResult {
        if let Some(&dev_ifindex) = self.slave_map.get(&slave_ifindex) {
            if let Some(dev) = self.devices.get(&dev_ifindex) {
                if let Some(slave) = dev.find_slave(slave_ifindex) {
                    return ipvlan_xmit(slave, packet);
                }
            }
        }
        IpvlanResult::Drop
    }

    pub fn should_deliver_to_slave(
        &self,
        slave_ifindex: u32,
        packet: &[u8],
    ) -> bool {
        if let Some(&dev_ifindex) = self.slave_map.get(&slave_ifindex) {
            if let Some(dev) = self.devices.get(&dev_ifindex) {
                if let Some(slave) = dev.find_slave(slave_ifindex) {
                    return ipvlan_rx(slave, packet);
                }
            }
        }
        false
    }

    pub fn remove_device(&mut self, dev_ifindex: u32) -> bool {
        if let Some(dev) = self.devices.get(&dev_ifindex) {
            let slave_ifindices: Vec<u32> = dev.slaves.iter().map(|s| s.dev_ifindex).collect();
            for idx in slave_ifindices {
                self.slave_map.remove(&idx);
            }
        }
        self.devices.remove(&dev_ifindex).is_some()
    }

    pub fn remove_slave(&mut self, slave_ifindex: u32) -> bool {
        if let Some(&dev_ifindex) = self.slave_map.get(&slave_ifindex) {
            self.slave_map.remove(&slave_ifindex);
            if let Some(dev) = self.devices.get_mut(&dev_ifindex) {
                return dev.remove_slave(slave_ifindex);
            }
        }
        false
    }

    pub fn slave_mac(&self, slave_ifindex: u32) -> MacAddr {
        if let Some(&dev_ifindex) = self.slave_map.get(&slave_ifindex) {
            if let Some(dev) = self.devices.get(&dev_ifindex) {
                if let Some(slave) = dev.find_slave(slave_ifindex) {
                    return slave.mac_addr;
                }
            }
        }
        MacAddr::ZERO
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_eth_frame(dst: MacAddr, src: MacAddr) -> Vec<u8> {
        let mut frame = Vec::with_capacity(ETH_HLEN + 64);
        frame.extend_from_slice(&dst.0);
        frame.extend_from_slice(&src.0);
        frame.extend_from_slice(&0x0800u16.to_be_bytes());
        frame.extend_from_slice(&[0u8; 64]);
        frame
    }

    #[test]
    fn ipvlan_create_slave_basic() {
        let slave = ipvlan_create_slave(1, "eth0", IpvlanMode::L2, IpvlanFlag::Bridge);
        assert_eq!(slave.parent_ifindex, 1);
        assert_eq!(slave.mode, IpvlanMode::L2);
        assert_eq!(slave.flag, IpvlanFlag::Bridge);
        assert!(slave.up);
    }

    #[test]
    fn ipvlan_xmit_l2_unicast() {
        let slave = ipvlan_create_slave(1, "eth0", IpvlanMode::L2, IpvlanFlag::Bridge);
        let dst = MacAddr([0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);
        let src = MacAddr([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
        let frame = make_eth_frame(dst, src);
        assert_eq!(ipvlan_xmit(&slave, &frame), IpvlanResult::TxToMaster);
    }

    #[test]
    fn ipvlan_xmit_l3_multicast_drops() {
        let slave = ipvlan_create_slave(1, "eth0", IpvlanMode::L3, IpvlanFlag::Bridge);
        let dst = MacAddr([0x01, 0x00, 0x5E, 0x00, 0x00, 0x01]);
        let src = MacAddr([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
        let frame = make_eth_frame(dst, src);
        assert_eq!(ipvlan_xmit(&slave, &frame), IpvlanResult::Drop);
    }

    #[test]
    fn ipvlan_xmit_l3s_multicast_drops() {
        let slave = ipvlan_create_slave(1, "eth0", IpvlanMode::L3s, IpvlanFlag::Bridge);
        let dst = MacAddr([0x01, 0x00, 0x5E, 0x00, 0x00, 0x01]);
        let src = MacAddr([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
        let frame = make_eth_frame(dst, src);
        assert_eq!(ipvlan_xmit(&slave, &frame), IpvlanResult::Drop);
    }

    #[test]
    fn ipvlan_xmit_l3_unicast() {
        let slave = ipvlan_create_slave(1, "eth0", IpvlanMode::L3, IpvlanFlag::Bridge);
        let dst = MacAddr([0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);
        let src = MacAddr([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
        let frame = make_eth_frame(dst, src);
        assert_eq!(ipvlan_xmit(&slave, &frame), IpvlanResult::TxToMaster);
    }

    #[test]
    fn ipvlan_xmit_short_frame_drops() {
        let slave = ipvlan_create_slave(1, "eth0", IpvlanMode::L2, IpvlanFlag::Bridge);
        let frame = [0u8; 5];
        assert_eq!(ipvlan_xmit(&slave, &frame), IpvlanResult::Drop);
    }

    #[test]
    fn ipvlan_frame_is_multicast_broadcast() {
        let frame = make_eth_frame(MacAddr([0xFF; 6]), MacAddr([0x01; 6]));
        assert!(ipvlan_frame_is_multicast(&frame));
    }

    #[test]
    fn ipvlan_frame_is_multicasticasticast() {
        let frame = make_eth_frame(
            MacAddr([0x01, 0x00, 0x5E, 0x00, 0x00, 0x01]),
            MacAddr([0xAA; 6]),
        );
        assert!(ipvlan_frame_is_multicast(&frame));
    }

    #[test]
    fn ipvlan_frame_is_not_multicast() {
        let frame = make_eth_frame(
            MacAddr([0x00, 0x11, 0x22, 0x33, 0x44, 0x55]),
            MacAddr([0xAA; 6]),
        );
        assert!(!ipvlan_frame_is_multicast(&frame));
    }

    #[test]
    fn ipvlan_frame_is_multicast_short() {
        assert!(!ipvlan_frame_is_multicast(&[0u8; 5]));
    }

    #[test]
    fn ipvlan_allow_cross_traffic_bridge() {
        let s1 = ipvlan_create_slave(1, "a", IpvlanMode::L2, IpvlanFlag::Bridge);
        let s2 = ipvlan_create_slave(1, "b", IpvlanMode::L2, IpvlanFlag::Bridge);
        assert!(ipvlan_allow_cross_traffic(&s1, &s2));
    }

    #[test]
    fn ipvlan_allow_cross_traffic_private() {
        let s1 = ipvlan_create_slave(1, "a", IpvlanMode::L2, IpvlanFlag::Private);
        let s2 = ipvlan_create_slave(1, "b", IpvlanMode::L2, IpvlanFlag::Private);
        assert!(!ipvlan_allow_cross_traffic(&s1, &s2));
    }

    #[test]
    fn ipvlan_allow_cross_traffic_vepa() {
        let s1 = ipvlan_create_slave(1, "a", IpvlanMode::L2, IpvlanFlag::Vepa);
        let s2 = ipvlan_create_slave(1, "b", IpvlanMode::L2, IpvlanFlag::Vepa);
        assert!(!ipvlan_allow_cross_traffic(&s1, &s2));
    }

    #[test]
    fn ipvlan_allow_cross_traffic_different_parents() {
        let s1 = ipvlan_create_slave(1, "a", IpvlanMode::L2, IpvlanFlag::Bridge);
        let s2 = ipvlan_create_slave(2, "b", IpvlanMode::L2, IpvlanFlag::Bridge);
        assert!(!ipvlan_allow_cross_traffic(&s1, &s2));
    }

    #[test]
    fn ipvlan_rx_l2接受了多播() {
        let slave = ipvlan_create_slave(1, "a", IpvlanMode::L2, IpvlanFlag::Bridge);
        let frame = make_eth_frame(MacAddr([0x01; 6]), MacAddr([0xAA; 6]));
        assert!(ipvlan_rx(&slave, &frame));
    }

    #[test]
    fn ipvlan_rx_l3_拒绝多播() {
        let slave = ipvlan_create_slave(1, "a", IpvlanMode::L3, IpvlanFlag::Bridge);
        let frame = make_eth_frame(MacAddr([0x01; 6]), MacAddr([0xAA; 6]));
        assert!(!ipvlan_rx(&slave, &frame));
    }

    #[test]
    fn ipvlan_rx_l3s_接受多播() {
        let slave = ipvlan_create_slave(1, "a", IpvlanMode::L3s, IpvlanFlag::Bridge);
        let frame = make_eth_frame(MacAddr([0x01; 6]), MacAddr([0xAA; 6]));
        assert!(ipvlan_rx(&slave, &frame));
    }

    #[test]
    fn ipvlan_rx_short_frame() {
        let slave = ipvlan_create_slave(1, "a", IpvlanMode::L2, IpvlanFlag::Bridge);
        assert!(!ipvlan_rx(&slave, &[0u8; 5]));
    }

    #[test]
    fn ipvlan_switch_create_and_process() {
        let mut sw = IpvlanSwitch::new();
        let dev_idx = sw.create_device(10, IpvlanMode::L2, IpvlanFlag::Bridge);
        let slave_idx = sw.add_slave(dev_idx, 10, IpvlanMode::L2, IpvlanFlag::Bridge).unwrap();

        let dst = MacAddr([0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);
        let src = MacAddr([0xAA; 6]);
        let frame = make_eth_frame(dst, src);

        assert_eq!(sw.process_tx(slave_idx, &frame), IpvlanResult::TxToMaster);
        assert!(sw.should_deliver_to_slave(slave_idx, &frame));
    }

    #[test]
    fn ipvlan_switch_add_slave_unknown_device() {
        let mut sw = IpvlanSwitch::new();
        assert!(sw.add_slave(999, 10, IpvlanMode::L2, IpvlanFlag::Bridge).is_err());
    }

    #[test]
    fn ipvlan_switch_remove_device() {
        let mut sw = IpvlanSwitch::new();
        let dev_idx = sw.create_device(10, IpvlanMode::L2, IpvlanFlag::Bridge);
        let slave_idx = sw.add_slave(dev_idx, 10, IpvlanMode::L2, IpvlanFlag::Bridge).unwrap();

        assert!(sw.remove_device(dev_idx));
        assert_eq!(sw.process_tx(slave_idx, &[0u8; 20]), IpvlanResult::Drop);
    }

    #[test]
    fn ipvlan_switch_remove_slave() {
        let mut sw = IpvlanSwitch::new();
        let dev_idx = sw.create_device(10, IpvlanMode::L2, IpvlanFlag::Bridge);
        let slave_idx = sw.add_slave(dev_idx, 10, IpvlanMode::L2, IpvlanFlag::Bridge).unwrap();

        assert!(sw.remove_slave(slave_idx));
        assert!(!sw.should_deliver_to_slave(slave_idx, &make_eth_frame(MacAddr([0x01; 6]), MacAddr([0xAA; 6]))));
    }

    #[test]
    fn ipvlan_device_add_remove_slave() {
        let mut dev = IpvlanDevice::new(10, IpvlanMode::L3, IpvlanFlag::Private);
        let s1 = ipvlan_create_slave(10, "a", IpvlanMode::L3, IpvlanFlag::Private);
        let s2 = ipvlan_create_slave(10, "b", IpvlanMode::L3, IpvlanFlag::Private);
        let s1_idx = s1.dev_ifindex;
        let s2_idx = s2.dev_ifindex;

        dev.add_slave(s1);
        dev.add_slave(s2);
        assert_eq!(dev.slaves.len(), 2);

        assert!(dev.remove_slave(s1_idx));
        assert_eq!(dev.slaves.len(), 1);
        assert!(dev.find_slave(s2_idx).is_some());
    }

    #[test]
    fn ipvlan_device_find_nonexistent() {
        let dev = IpvlanDevice::new(10, IpvlanMode::L2, IpvlanFlag::Bridge);
        assert!(dev.find_slave(999).is_none());
    }

    #[test]
    fn ipvlan_vepa_mode_xmit() {
        let slave = ipvlan_create_slave(1, "a", IpvlanMode::L2, IpvlanFlag::Vepa);
        let frame = make_eth_frame(
            MacAddr([0x00, 0x11, 0x22, 0x33, 0x44, 0x55]),
            MacAddr([0xAA; 6]),
        );
        assert_eq!(ipvlan_xmit(&slave, &frame), IpvlanResult::TxToMaster);
    }

    #[test]
    fn ipvlan_l3s_unicast_delivers() {
        let slave = ipvlan_create_slave(1, "a", IpvlanMode::L3s, IpvlanFlag::Bridge);
        let frame = make_eth_frame(
            MacAddr([0x00, 0x11, 0x22, 0x33, 0x44, 0x55]),
            MacAddr([0xAA; 6]),
        );
        assert!(ipvlan_rx(&slave, &frame));
    }

    #[test]
    fn ipvlan_mode_equality() {
        assert_eq!(IpvlanMode::L2, IpvlanMode::L2);
        assert_ne!(IpvlanMode::L2, IpvlanMode::L3);
        assert_ne!(IpvlanMode::L3, IpvlanMode::L3s);
    }

    #[test]
    fn ipvlan_flag_equality() {
        assert_eq!(IpvlanFlag::Bridge, IpvlanFlag::Bridge);
        assert_ne!(IpvlanFlag::Bridge, IpvlanFlag::Private);
        assert_ne!(IpvlanFlag::Private, IpvlanFlag::Vepa);
    }

    #[test]
    fn ipvlan_switch_process_tx_invalid_slave() {
        let sw = IpvlanSwitch::new();
        let frame = make_eth_frame(
            MacAddr([0x00, 0x11, 0x22, 0x33, 0x44, 0x55]),
            MacAddr([0xAA; 6]),
        );
        assert_eq!(sw.process_tx(999, &frame), IpvlanResult::Drop);
    }

    #[test]
    fn ipvlan_switch_deliver_invalid_slave() {
        let sw = IpvlanSwitch::new();
        let frame = make_eth_frame(
            MacAddr([0x01; 6]),
            MacAddr([0xAA; 6]),
        );
        assert!(!sw.should_deliver_to_slave(999, &frame));
    }

    #[test]
    fn ipvlan_interface_name_and_mac() {
        let sw = Arc::new(Mutex::new(IpvlanSwitch::new()));
        let dev_idx = sw.lock().create_device(1, IpvlanMode::L2, IpvlanFlag::Bridge);
        let iface = IpvlanInterface::register(sw, dev_idx, 1, IpvlanMode::L2, IpvlanFlag::Bridge, "ipvlan0").unwrap();
        let guard = iface.lock();
        assert_eq!(guard.name(), "ipvlan0");
        assert_eq!(guard.mac(), MacAddr([0x02, 0, 0, 0, 0x01, 0]));
        assert!(guard.is_up());
    }

    #[test]
    fn ipvlan_interface_ip_config() {
        let sw = Arc::new(Mutex::new(IpvlanSwitch::new()));
        let dev_idx = sw.lock().create_device(1, IpvlanMode::L2, IpvlanFlag::Bridge);
        let iface = IpvlanInterface::register(sw, dev_idx, 1, IpvlanMode::L2, IpvlanFlag::Bridge, "ipvlan1").unwrap();
        {
            let mut guard = iface.lock();
            guard.set_ip(Ipv4Addr::new(10, 0, 0, 5));
            guard.set_netmask(Ipv4Addr::new(255, 255, 255, 0));
            guard.set_gateway(Ipv4Addr::new(10, 0, 0, 1));
        }
        let guard = iface.lock();
        assert_eq!(guard.ip(), Ipv4Addr::new(10, 0, 0, 5));
        assert_eq!(guard.netmask(), Ipv4Addr::new(255, 255, 255, 0));
        assert_eq!(guard.gateway(), Some(Ipv4Addr::new(10, 0, 0, 1)));
    }

    #[test]
    fn ipvlan_interface_up_down() {
        let sw = Arc::new(Mutex::new(IpvlanSwitch::new()));
        let dev_idx = sw.lock().create_device(1, IpvlanMode::L2, IpvlanFlag::Bridge);
        let iface = IpvlanInterface::register(sw, dev_idx, 1, IpvlanMode::L2, IpvlanFlag::Bridge, "ipvlan2").unwrap();
        {
            let mut guard = iface.lock();
            assert!(guard.is_up());
            guard.set_up(false);
        }
        {
            let guard = iface.lock();
            assert!(!guard.is_up());
        }
    }

    #[test]
    fn ipvlan_interface_send_down_returns_error() {
        let sw = Arc::new(Mutex::new(IpvlanSwitch::new()));
        let dev_idx = sw.lock().create_device(1, IpvlanMode::L2, IpvlanFlag::Bridge);
        let iface = IpvlanInterface::register(sw, dev_idx, 1, IpvlanMode::L2, IpvlanFlag::Bridge, "ipvlan3").unwrap();
        {
            let mut guard = iface.lock();
            guard.set_up(false);
        }
        let mut guard = iface.lock();
        let frame = make_eth_frame(MacAddr([0x02; 6]), MacAddr([0xAA; 6]));
        assert_eq!(guard.send(&frame), Err(NetError::NotUp));
    }

    #[test]
    fn ipvlan_interface_stats_zero() {
        let sw = Arc::new(Mutex::new(IpvlanSwitch::new()));
        let dev_idx = sw.lock().create_device(1, IpvlanMode::L2, IpvlanFlag::Bridge);
        let iface = IpvlanInterface::register(sw, dev_idx, 1, IpvlanMode::L2, IpvlanFlag::Bridge, "ipvlan4").unwrap();
        let guard = iface.lock();
        let stats = guard.stats();
        assert_eq!(stats.rx_packets, 0);
        assert_eq!(stats.tx_packets, 0);
    }

    #[test]
    fn ipvlan_interface_mtu() {
        let sw = Arc::new(Mutex::new(IpvlanSwitch::new()));
        let dev_idx = sw.lock().create_device(1, IpvlanMode::L2, IpvlanFlag::Bridge);
        let iface = IpvlanInterface::register(sw, dev_idx, 1, IpvlanMode::L2, IpvlanFlag::Bridge, "ipvlan5").unwrap();
        let guard = iface.lock();
        assert_eq!(guard.mtu(), IPVLAN_DEFAULT_MTU);
    }
}