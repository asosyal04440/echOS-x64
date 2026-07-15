use super::{
    get_interface, register_interface, Ipv4Addr, MacAddr, NetError, NetInterface, NetStats,
};
use super::Mutex;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

pub const MACVLAN_DEFAULT_MTU: u16 = 1500;
pub const ETH_HLEN: usize = 14;

static NEXT_IFINDEX: AtomicU32 = AtomicU32::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MacvlanMode {
    Bridge,
    Private,
    Vepa,
    Passthru,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MacvlanResult {
    TxToMaster,
    TxToSlave(u32),
    TxToExternal,
    TxHairpin,
    Drop,
    DeliverLocally,
}

pub struct MacvlanDevice {
    pub parent_ifindex: u32,
    pub mode: MacvlanMode,
    pub mac_prefix: [u8; 3],
    pub ifindex: u32,
    pub mtu: u16,
    pub slaves: Vec<MacvlanSlave>,
}

impl MacvlanDevice {
    pub fn new(parent_ifindex: u32, mode: MacvlanMode) -> Self {
        let ifindex = NEXT_IFINDEX.fetch_add(1, Ordering::Relaxed);
        let mac_prefix = [
            0x02,
            ((parent_ifindex >> 8) & 0xFF) as u8,
            (parent_ifindex & 0xFF) as u8,
        ];
        MacvlanDevice {
            parent_ifindex,
            mode,
            mac_prefix,
            ifindex,
            mtu: MACVLAN_DEFAULT_MTU,
            slaves: Vec::new(),
        }
    }

    pub fn find_slave(&self, dev_ifindex: u32) -> Option<&MacvlanSlave> {
        self.slaves.iter().find(|s| s.dev_ifindex == dev_ifindex)
    }

    pub fn add_slave(&mut self, slave: MacvlanSlave) -> Result<(), &'static str> {
        match self.mode {
            MacvlanMode::Passthru => {
                if !self.slaves.is_empty() {
                    return Err("passthru mode allows only one slave");
                }
                self.slaves.push(slave);
                Ok(())
            }
            _ => {
                self.slaves.push(slave);
                Ok(())
            }
        }
    }

    pub fn remove_slave(&mut self, dev_ifindex: u32) -> bool {
        let len_before = self.slaves.len();
        self.slaves.retain(|s| s.dev_ifindex != dev_ifindex);
        self.slaves.len() < len_before
    }
}

#[derive(Clone, Debug)]
pub struct MacvlanSlave {
    pub parent_ifindex: u32,
    pub dev_ifindex: u32,
    pub mac_addr: MacAddr,
    pub mode: MacvlanMode,
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

impl MacvlanSlave {
    pub fn new(parent_ifindex: u32, mac_addr: MacAddr, mode: MacvlanMode) -> Self {
        let dev_ifindex = NEXT_IFINDEX.fetch_add(1, Ordering::Relaxed);
        MacvlanSlave {
            parent_ifindex,
            dev_ifindex,
            mac_addr,
            mode,
            mtu: MACVLAN_DEFAULT_MTU,
            up: true,
            name: alloc::format!("macvlan.{}", dev_ifindex),
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

pub fn macvlan_create_slave(
    parent_ifindex: u32,
    mac: MacAddr,
    mode: MacvlanMode,
) -> MacvlanSlave {
    MacvlanSlave::new(parent_ifindex, mac, mode)
}

pub fn macvlan_validate_mac(mac: MacAddr) -> bool {
    if mac == MacAddr::ZERO {
        return false;
    }
    if mac == MacAddr::BROADCAST {
        return false;
    }
    if mac.0[0] & 0x01 != 0 {
        return false;
    }
    true
}

pub fn macvlan_generate_mac(prefix: [u8; 3], index: u32) -> MacAddr {
    MacAddr([
        prefix[0],
        prefix[1],
        prefix[2],
        ((index >> 16) & 0xFF) as u8,
        ((index >> 8) & 0xFF) as u8,
        (index & 0xFF) as u8,
    ])
}

pub fn macvlan_xmit(slave: &MacvlanSlave, packet: &[u8]) -> MacvlanResult {
    if packet.len() < ETH_HLEN {
        return MacvlanResult::Drop;
    }

    let dst_mac = MacAddr([packet[0], packet[1], packet[2], packet[3], packet[4], packet[5]]);

    let is_broadcast = dst_mac == MacAddr::BROADCAST;
    let is_multicast = !is_broadcast && (dst_mac.0[0] & 0x01 != 0);
    let is_same_host = dst_mac == slave.mac_addr;

    if is_same_host {
        return MacvlanResult::TxHairpin;
    }

    match slave.mode {
        MacvlanMode::Bridge => {
            if is_broadcast || is_multicast {
                MacvlanResult::TxToMaster
            } else {
                MacvlanResult::TxToMaster
            }
        }
        MacvlanMode::Private => {
            if is_broadcast || is_multicast {
                MacvlanResult::TxToMaster
            } else {
                MacvlanResult::Drop
            }
        }
        MacvlanMode::Vepa => {
            if is_broadcast || is_multicast {
                MacvlanResult::TxHairpin
            } else {
                MacvlanResult::TxToExternal
            }
        }
        MacvlanMode::Passthru => {
            MacvlanResult::TxToMaster
        }
    }
}

pub fn macvlan_rx(slave: &MacvlanSlave, packet: &[u8]) -> bool {
    if packet.len() < ETH_HLEN {
        return false;
    }
    let dst = &packet[0..6];
    if dst == &MacAddr::BROADCAST.0 {
        return true;
    }
    if dst[0] & 0x01 != 0 {
        return slave.mode == MacvlanMode::Bridge || slave.mode == MacvlanMode::Passthru;
    }
    let dst_mac = MacAddr([dst[0], dst[1], dst[2], dst[3], dst[4], dst[5]]);
    match slave.mode {
        MacvlanMode::Bridge => true,
        MacvlanMode::Private => dst_mac == slave.mac_addr,
        MacvlanMode::Vepa => dst_mac == slave.mac_addr,
        MacvlanMode::Passthru => true,
    }
}

pub struct MacvlanInterface {
    switch: Arc<Mutex<MacvlanSwitch>>,
    slave_ifindex: u32,
    cached_name: String,
    cached_mac: MacAddr,
    ip: Ipv4Addr,
    netmask: Ipv4Addr,
    gateway: Option<Ipv4Addr>,
    up: bool,
}

impl MacvlanInterface {
    pub fn new(
        switch: Arc<Mutex<MacvlanSwitch>>,
        slave_ifindex: u32,
        mac: MacAddr,
        name: &str,
    ) -> Self {
        MacvlanInterface {
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
        switch: Arc<Mutex<MacvlanSwitch>>,
        dev_ifindex: u32,
        mac: MacAddr,
        mode: MacvlanMode,
        name: &str,
    ) -> Result<Arc<Mutex<dyn NetInterface>>, &'static str> {
        let slave_ifindex = switch.lock().add_slave(dev_ifindex, mac, mode)?;
        let iface = MacvlanInterface::new(switch, slave_ifindex, mac, name);
        let iface_arc = Arc::new(Mutex::new(iface)) as Arc<Mutex<dyn NetInterface>>;
        register_interface(iface_arc.clone());
        Ok(iface_arc)
    }
}

impl NetInterface for MacvlanInterface {
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
            MacvlanResult::Drop => Err(NetError::InvalidPacket),
            MacvlanResult::TxToExternal => {
                let sw = self.switch.lock();
                if let Some(&dev_ifindex) = sw.slave_map.get(&self.slave_ifindex) {
                    if let Some(dev) = sw.devices.get(&dev_ifindex) {
                        let parent_name = alloc::format!("eth{}", dev.parent_ifindex);
                        drop(sw);
                        if let Some(parent) = get_interface(&parent_name) {
                            let mut guard = parent.lock();
                            return guard.send(data);
                        }
                    }
                }
                Err(NetError::NoInterface)
            }
            _ => {
                let mut tx_bytes = data.len() as u64;
                let sw = self.switch.lock();
                if let Some(&dev_ifindex) = sw.slave_map.get(&self.slave_ifindex) {
                    if let Some(dev) = sw.devices.get(&dev_ifindex) {
                        if let Some(slave) = dev.find_slave(self.slave_ifindex) {
                            tx_bytes = slave.tx_bytes + tx_bytes;
                        }
                    }
                }
                drop(sw);
                let _ = tx_bytes;
                Ok(())
            }
        }
    }

    fn recv(&mut self) -> Option<Vec<u8>> {
        if !self.up {
            return None;
        }
        let sw = self.switch.lock();
        if let Some(&dev_ifindex) = sw.slave_map.get(&self.slave_ifindex) {
            if let Some(dev) = sw.devices.get(&dev_ifindex) {
                let parent_name = alloc::format!("eth{}", dev.parent_ifindex);
                drop(sw);
                if let Some(parent) = get_interface(&parent_name) {
                    let mut guard = parent.lock();
                    if let Some(pkt) = guard.recv() {
                        let sw = self.switch.lock();
                        if sw.should_deliver_to_slave(self.slave_ifindex, &pkt) {
                            let _ = pkt.len();
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
        MACVLAN_DEFAULT_MTU
    }
}

pub struct MacvlanSwitch {
    pub devices: BTreeMap<u32, MacvlanDevice>,
    pub slave_map: BTreeMap<u32, u32>,
    pub mac_table: BTreeMap<MacAddr, u32>,
}

impl MacvlanSwitch {
    pub fn new() -> Self {
        MacvlanSwitch {
            devices: BTreeMap::new(),
            slave_map: BTreeMap::new(),
            mac_table: BTreeMap::new(),
        }
    }

    pub fn create_device(&mut self, parent_ifindex: u32, mode: MacvlanMode) -> u32 {
        let dev = MacvlanDevice::new(parent_ifindex, mode);
        let ifindex = dev.ifindex;
        self.devices.insert(ifindex, dev);
        ifindex
    }

    pub fn add_slave(
        &mut self,
        dev_ifindex: u32,
        mac: MacAddr,
        mode: MacvlanMode,
    ) -> Result<u32, &'static str> {
        let slave = macvlan_create_slave(dev_ifindex, mac, mode);
        let slave_ifindex = slave.dev_ifindex;

        if let Some(dev) = self.devices.get_mut(&dev_ifindex) {
            dev.add_slave(slave)?;
            self.slave_map.insert(slave_ifindex, dev_ifindex);
            self.mac_table.insert(mac, slave_ifindex);
            Ok(slave_ifindex)
        } else {
            Err("device not found")
        }
    }

    pub fn process_tx(&self, slave_ifindex: u32, packet: &[u8]) -> MacvlanResult {
        if let Some(&dev_ifindex) = self.slave_map.get(&slave_ifindex) {
            if let Some(dev) = self.devices.get(&dev_ifindex) {
                if let Some(slave) = dev.find_slave(slave_ifindex) {
                    return macvlan_xmit(slave, packet);
                }
            }
        }
        MacvlanResult::Drop
    }

    pub fn should_deliver_to_slave(
        &self,
        slave_ifindex: u32,
        packet: &[u8],
    ) -> bool {
        if let Some(&dev_ifindex) = self.slave_map.get(&slave_ifindex) {
            if let Some(dev) = self.devices.get(&dev_ifindex) {
                if let Some(slave) = dev.find_slave(slave_ifindex) {
                    return macvlan_rx(slave, packet);
                }
            }
        }
        false
    }

    pub fn lookup_mac(&self, mac: &MacAddr) -> Option<u32> {
        self.mac_table.get(mac).copied()
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
                if let Some(slave) = dev.find_slave(slave_ifindex) {
                    self.mac_table.remove(&slave.mac_addr);
                }
                return dev.remove_slave(slave_ifindex);
            }
        }
        false
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
    fn macvlan_validate_mac_normal() {
        assert!(macvlan_validate_mac(MacAddr([0x00, 0x11, 0x22, 0x33, 0x44, 0x55])));
    }

    #[test]
    fn macvlan_validate_mac_zero() {
        assert!(!macvlan_validate_mac(MacAddr::ZERO));
    }

    #[test]
    fn macvlan_validate_mac_broadcast() {
        assert!(!macvlan_validate_mac(MacAddr::BROADCAST));
    }

    #[test]
    fn macvlan_validate_mac_multicast() {
        assert!(!macvlan_validate_mac(MacAddr([0x01, 0x00, 0x5E, 0x00, 0x00, 0x01])));
    }

    #[test]
    fn macvlan_validate_mac_unicast() {
        assert!(macvlan_validate_mac(MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x01])));
    }

    #[test]
    fn macvlan_create_slave_basic() {
        let mac = MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
        let slave = macvlan_create_slave(10, mac, MacvlanMode::Bridge);
        assert_eq!(slave.parent_ifindex, 10);
        assert_eq!(slave.mac_addr, mac);
        assert_eq!(slave.mode, MacvlanMode::Bridge);
        assert!(slave.up);
    }

    #[test]
    fn macvlan_generate_mac_unique() {
        let prefix = [0x02, 0x00, 0x01];
        let m1 = macvlan_generate_mac(prefix, 0);
        let m2 = macvlan_generate_mac(prefix, 1);
        let m3 = macvlan_generate_mac(prefix, 0x000100);
        assert_ne!(m1, m2);
        assert_ne!(m1, m3);
        assert_ne!(m2, m3);
    }

    #[test]
    fn macvlan_generate_mac_prefix() {
        let prefix = [0x02, 0xAA, 0xBB];
        let m = macvlan_generate_mac(prefix, 0x000102);
        assert_eq!(m.0[0], 0x02);
        assert_eq!(m.0[1], 0xAA);
        assert_eq!(m.0[2], 0xBB);
        assert_eq!(m.0[3], 0x00);
        assert_eq!(m.0[4], 0x01);
        assert_eq!(m.0[5], 0x02);
    }

    #[test]
    fn macvlan_xmit_bridge_unicast() {
        let mac = MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
        let slave = macvlan_create_slave(10, mac, MacvlanMode::Bridge);
        let dst = MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x02]);
        let frame = make_eth_frame(dst, mac);
        assert_eq!(macvlan_xmit(&slave, &frame), MacvlanResult::TxToMaster);
    }

    #[test]
    fn macvlan_xmit_bridge_broadcast() {
        let mac = MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
        let slave = macvlan_create_slave(10, mac, MacvlanMode::Bridge);
        let frame = make_eth_frame(MacAddr::BROADCAST, mac);
        assert_eq!(macvlan_xmit(&slave, &frame), MacvlanResult::TxToMaster);
    }

    #[test]
    fn macvlan_xmit_private_unicast_drops() {
        let mac = MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
        let slave = macvlan_create_slave(10, mac, MacvlanMode::Private);
        let dst = MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x02]);
        let frame = make_eth_frame(dst, mac);
        assert_eq!(macvlan_xmit(&slave, &frame), MacvlanResult::Drop);
    }

    #[test]
    fn macvlan_xmit_private_broadcast() {
        let mac = MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
        let slave = macvlan_create_slave(10, mac, MacvlanMode::Private);
        let frame = make_eth_frame(MacAddr::BROADCAST, mac);
        assert_eq!(macvlan_xmit(&slave, &frame), MacvlanResult::TxToMaster);
    }

    #[test]
    fn macvlan_xmit_vepa_unicast() {
        let mac = MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
        let slave = macvlan_create_slave(10, mac, MacvlanMode::Vepa);
        let dst = MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x02]);
        let frame = make_eth_frame(dst, mac);
        assert_eq!(macvlan_xmit(&slave, &frame), MacvlanResult::TxToExternal);
    }

    #[test]
    fn macvlan_xmit_vepa_broadcast_hairpin() {
        let mac = MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
        let slave = macvlan_create_slave(10, mac, MacvlanMode::Vepa);
        let frame = make_eth_frame(MacAddr::BROADCAST, mac);
        assert_eq!(macvlan_xmit(&slave, &frame), MacvlanResult::TxHairpin);
    }

    #[test]
    fn macvlan_xmit_passthru() {
        let mac = MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
        let slave = macvlan_create_slave(10, mac, MacvlanMode::Passthru);
        let dst = MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x02]);
        let frame = make_eth_frame(dst, mac);
        assert_eq!(macvlan_xmit(&slave, &frame), MacvlanResult::TxToMaster);
    }

    #[test]
    fn macvlan_xmit_hairpin() {
        let mac = MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
        let slave = macvlan_create_slave(10, mac, MacvlanMode::Bridge);
        let frame = make_eth_frame(mac, mac);
        assert_eq!(macvlan_xmit(&slave, &frame), MacvlanResult::TxHairpin);
    }

    #[test]
    fn macvlan_xmit_short_frame() {
        let mac = MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
        let slave = macvlan_create_slave(10, mac, MacvlanMode::Bridge);
        assert_eq!(macvlan_xmit(&slave, &[0u8; 5]), MacvlanResult::Drop);
    }

    #[test]
    fn macvlan_rx_bridge接受了所有单播() {
        let mac = MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
        let slave = macvlan_create_slave(10, mac, MacvlanMode::Bridge);
        let dst = MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x02]);
        let frame = make_eth_frame(dst, MacAddr([0xAA; 6]));
        assert!(macvlan_rx(&slave, &frame));
    }

    #[test]
    fn macvlan_rx_private_仅接受自己的单播() {
        let mac = MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
        let slave = macvlan_create_slave(10, mac, MacvlanMode::Private);
        let frame_to_me = make_eth_frame(mac, MacAddr([0xAA; 6]));
        assert!(macvlan_rx(&slave, &frame_to_me));

        let frame_other = make_eth_frame(
            MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x02]),
            MacAddr([0xAA; 6]),
        );
        assert!(!macvlan_rx(&slave, &frame_other));
    }

    #[test]
    fn macvlan_rx_vepa_仅接受自己的单播() {
        let mac = MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
        let slave = macvlan_create_slave(10, mac, MacvlanMode::Vepa);
        let frame_to_me = make_eth_frame(mac, MacAddr([0xAA; 6]));
        assert!(macvlan_rx(&slave, &frame_to_me));

        let frame_other = make_eth_frame(
            MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x02]),
            MacAddr([0xAA; 6]),
        );
        assert!(!macvlan_rx(&slave, &frame_other));
    }

    #[test]
    fn macvlan_rx_passthru接受了所有单播() {
        let mac = MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
        let slave = macvlan_create_slave(10, mac, MacvlanMode::Passthru);
        let dst = MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x02]);
        let frame = make_eth_frame(dst, MacAddr([0xAA; 6]));
        assert!(macvlan_rx(&slave, &frame));
    }

    #[test]
    fn macvlan_rx_short_frame() {
        let mac = MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
        let slave = macvlan_create_slave(10, mac, MacvlanMode::Bridge);
        assert!(!macvlan_rx(&slave, &[0u8; 5]));
    }

    #[test]
    fn macvlan_rx_private_multicast() {
        let mac = MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
        let slave = macvlan_create_slave(10, mac, MacvlanMode::Private);
        let frame = make_eth_frame(MacAddr([0x01; 6]), MacAddr([0xAA; 6]));
        assert!(!macvlan_rx(&slave, &frame));
    }

    #[test]
    fn macvlan_rx_vepa_multicast_does_not_deliver() {
        let mac = MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
        let slave = macvlan_create_slave(10, mac, MacvlanMode::Vepa);
        let frame = make_eth_frame(MacAddr([0x01; 6]), MacAddr([0xAA; 6]));
        assert!(!macvlan_rx(&slave, &frame));
    }

    #[test]
    fn macvlan_switch_create_and_process() {
        let mut sw = MacvlanSwitch::new();
        let dev_idx = sw.create_device(10, MacvlanMode::Bridge);
        let mac = MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
        let slave_idx = sw.add_slave(dev_idx, mac, MacvlanMode::Bridge).unwrap();

        let dst = MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x02]);
        let frame = make_eth_frame(dst, mac);

        assert_eq!(sw.process_tx(slave_idx, &frame), MacvlanResult::TxToMaster);
        assert!(sw.should_deliver_to_slave(slave_idx, &frame));
    }

    #[test]
    fn macvlan_switch_add_slave_unknown_device() {
        let mut sw = MacvlanSwitch::new();
        let mac = MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
        assert!(sw.add_slave(999, mac, MacvlanMode::Bridge).is_err());
    }

    #[test]
    fn macvlan_switch_passthru_one_slave() {
        let mut sw = MacvlanSwitch::new();
        let dev_idx = sw.create_device(10, MacvlanMode::Passthru);
        let mac1 = MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
        assert!(sw.add_slave(dev_idx, mac1, MacvlanMode::Passthru).is_ok());

        let mac2 = MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x02]);
        assert!(sw.add_slave(dev_idx, mac2, MacvlanMode::Passthru).is_err());
    }

    #[test]
    fn macvlan_switch_remove_device() {
        let mut sw = MacvlanSwitch::new();
        let dev_idx = sw.create_device(10, MacvlanMode::Bridge);
        let mac = MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
        let slave_idx = sw.add_slave(dev_idx, mac, MacvlanMode::Bridge).unwrap();

        assert!(sw.remove_device(dev_idx));
        assert_eq!(sw.process_tx(slave_idx, &[0u8; 20]), MacvlanResult::Drop);
    }

    #[test]
    fn macvlan_switch_remove_slave() {
        let mut sw = MacvlanSwitch::new();
        let dev_idx = sw.create_device(10, MacvlanMode::Bridge);
        let mac = MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
        let slave_idx = sw.add_slave(dev_idx, mac, MacvlanMode::Bridge).unwrap();

        assert!(sw.remove_slave(slave_idx));
        assert!(sw.lookup_mac(&mac).is_none());
    }

    #[test]
    fn macvlan_switch_lookup_mac() {
        let mut sw = MacvlanSwitch::new();
        let dev_idx = sw.create_device(10, MacvlanMode::Bridge);
        let mac = MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
        let slave_idx = sw.add_slave(dev_idx, mac, MacvlanMode::Bridge).unwrap();

        assert_eq!(sw.lookup_mac(&mac), Some(slave_idx));
        assert_eq!(sw.lookup_mac(&MacAddr([0x02; 6])), None);
    }

    #[test]
    fn macvlan_switch_process_tx_invalid_slave() {
        let sw = MacvlanSwitch::new();
        let frame = make_eth_frame(MacAddr([0x02; 6]), MacAddr([0xAA; 6]));
        assert_eq!(sw.process_tx(999, &frame), MacvlanResult::Drop);
    }

    #[test]
    fn macvlan_switch_deliver_invalid_slave() {
        let sw = MacvlanSwitch::new();
        let frame = make_eth_frame(MacAddr([0x02; 6]), MacAddr([0xAA; 6]));
        assert!(!sw.should_deliver_to_slave(999, &frame));
    }

    #[test]
    fn macvlan_device_add_remove_slave() {
        let mut dev = MacvlanDevice::new(10, MacvlanMode::Bridge);
        let s1 = macvlan_create_slave(10, MacAddr([0x02, 0, 0, 0, 0, 1]), MacvlanMode::Bridge);
        let s2 = macvlan_create_slave(10, MacAddr([0x02, 0, 0, 0, 0, 2]), MacvlanMode::Bridge);
        let s1_idx = s1.dev_ifindex;
        let s2_idx = s2.dev_ifindex;

        assert!(dev.add_slave(s1).is_ok());
        assert!(dev.add_slave(s2).is_ok());
        assert_eq!(dev.slaves.len(), 2);

        assert!(dev.remove_slave(s1_idx));
        assert_eq!(dev.slaves.len(), 1);
        assert!(dev.find_slave(s2_idx).is_some());
    }

    #[test]
    fn macvlan_device_passthru_rejects_second() {
        let mut dev = MacvlanDevice::new(10, MacvlanMode::Passthru);
        let s1 = macvlan_create_slave(10, MacAddr([0x02, 0, 0, 0, 0, 1]), MacvlanMode::Passthru);
        let s2 = macvlan_create_slave(10, MacAddr([0x02, 0, 0, 0, 0, 2]), MacvlanMode::Passthru);

        assert!(dev.add_slave(s1).is_ok());
        assert!(dev.add_slave(s2).is_err());
    }

    #[test]
    fn macvlan_mode_equality() {
        assert_eq!(MacvlanMode::Bridge, MacvlanMode::Bridge);
        assert_ne!(MacvlanMode::Bridge, MacvlanMode::Private);
        assert_ne!(MacvlanMode::Vepa, MacvlanMode::Passthru);
    }

    #[test]
    fn macvlan_rx_bridge_broadcast() {
        let mac = MacAddr([0x02, 0, 0, 0, 0, 1]);
        let slave = macvlan_create_slave(10, mac, MacvlanMode::Bridge);
        let frame = make_eth_frame(MacAddr::BROADCAST, mac);
        assert!(macvlan_rx(&slave, &frame));
    }

    #[test]
    fn macvlan_rx_passthru接受了广播() {
        let mac = MacAddr([0x02, 0, 0, 0, 0, 1]);
        let slave = macvlan_create_slave(10, mac, MacvlanMode::Passthru);
        let frame = make_eth_frame(MacAddr::BROADCAST, mac);
        assert!(macvlan_rx(&slave, &frame));
    }

    #[test]
    fn macvlan_xmit_vepa_multicast_hairpin() {
        let mac = MacAddr([0x02, 0, 0, 0, 0, 1]);
        let slave = macvlan_create_slave(10, mac, MacvlanMode::Vepa);
        let mc = MacAddr([0x01, 0x00, 0x5E, 0x00, 0x00, 0x01]);
        let frame = make_eth_frame(mc, mac);
        assert_eq!(macvlan_xmit(&slave, &frame), MacvlanResult::TxHairpin);
    }

    #[test]
    fn macvlan_xmit_private_multicast() {
        let mac = MacAddr([0x02, 0, 0, 0, 0, 1]);
        let slave = macvlan_create_slave(10, mac, MacvlanMode::Private);
        let mc = MacAddr([0x01, 0x00, 0x5E, 0x00, 0x00, 0x01]);
        let frame = make_eth_frame(mc, mac);
        assert_eq!(macvlan_xmit(&slave, &frame), MacvlanResult::TxToMaster);
    }

    #[test]
    fn macvlan_interface_name_and_mac() {
        let sw = Arc::new(Mutex::new(MacvlanSwitch::new()));
        let dev_idx = sw.lock().create_device(1, MacvlanMode::Bridge);
        let mac = MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
        let iface = MacvlanInterface::register(sw, dev_idx, mac, MacvlanMode::Bridge, "macvlan0").unwrap();
        let guard = iface.lock();
        assert_eq!(guard.name(), "macvlan0");
        assert_eq!(guard.mac(), mac);
        assert!(guard.is_up());
    }

    #[test]
    fn macvlan_interface_ip_config() {
        let sw = Arc::new(Mutex::new(MacvlanSwitch::new()));
        let dev_idx = sw.lock().create_device(1, MacvlanMode::Bridge);
        let mac = MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
        let iface = MacvlanInterface::register(sw, dev_idx, mac, MacvlanMode::Bridge, "macvlan1").unwrap();
        {
            let mut guard = iface.lock();
            guard.set_ip(Ipv4Addr::new(192, 168, 1, 10));
            guard.set_netmask(Ipv4Addr::new(255, 255, 255, 0));
            guard.set_gateway(Ipv4Addr::new(192, 168, 1, 1));
        }
        let guard = iface.lock();
        assert_eq!(guard.ip(), Ipv4Addr::new(192, 168, 1, 10));
        assert_eq!(guard.netmask(), Ipv4Addr::new(255, 255, 255, 0));
        assert_eq!(guard.gateway(), Some(Ipv4Addr::new(192, 168, 1, 1)));
    }

    #[test]
    fn macvlan_interface_up_down() {
        let sw = Arc::new(Mutex::new(MacvlanSwitch::new()));
        let dev_idx = sw.lock().create_device(1, MacvlanMode::Bridge);
        let mac = MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
        let iface = MacvlanInterface::register(sw, dev_idx, mac, MacvlanMode::Bridge, "macvlan2").unwrap();
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
    fn macvlan_interface_send_down_returns_error() {
        let sw = Arc::new(Mutex::new(MacvlanSwitch::new()));
        let dev_idx = sw.lock().create_device(1, MacvlanMode::Bridge);
        let mac = MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
        let iface = MacvlanInterface::register(sw, dev_idx, mac, MacvlanMode::Bridge, "macvlan3").unwrap();
        {
            let mut guard = iface.lock();
            guard.set_up(false);
        }
        let mut guard = iface.lock();
        let frame = make_eth_frame(MacAddr([0x02; 6]), MacAddr([0xAA; 6]));
        assert_eq!(guard.send(&frame), Err(NetError::NotUp));
    }

    #[test]
    fn macvlan_interface_stats_zero() {
        let sw = Arc::new(Mutex::new(MacvlanSwitch::new()));
        let dev_idx = sw.lock().create_device(1, MacvlanMode::Bridge);
        let mac = MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
        let iface = MacvlanInterface::register(sw, dev_idx, mac, MacvlanMode::Bridge, "macvlan4").unwrap();
        let guard = iface.lock();
        let stats = guard.stats();
        assert_eq!(stats.rx_packets, 0);
        assert_eq!(stats.tx_packets, 0);
    }

    #[test]
    fn macvlan_interface_mtu() {
        let sw = Arc::new(Mutex::new(MacvlanSwitch::new()));
        let dev_idx = sw.lock().create_device(1, MacvlanMode::Bridge);
        let mac = MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
        let iface = MacvlanInterface::register(sw, dev_idx, mac, MacvlanMode::Bridge, "macvlan5").unwrap();
        let guard = iface.lock();
        assert_eq!(guard.mtu(), MACVLAN_DEFAULT_MTU);
    }
}
