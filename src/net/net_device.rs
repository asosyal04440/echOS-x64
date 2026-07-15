use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU16, AtomicU32, AtomicU64, AtomicBool, Ordering};
use spin::Mutex;

use super::{Ipv4Addr, MacAddr, NetError, NetInterface, NetStats};

// ============================================================================
// NETIF_F_* Feature Flags
// ============================================================================

pub const NETIF_F_HW_CSUM: u128 = 1 << 0;
pub const NETIF_F_IP_CSUM: u128 = 1 << 1;
pub const NETIF_F_IPV6_CSUM: u128 = 1 << 2;
pub const NETIF_F_RXCSUM: u128 = 1 << 3;
pub const NETIF_F_TSO: u128 = 1 << 4;
pub const NETIF_F_TSO6: u128 = 1 << 5;
pub const NETIF_F_TSO_ECN: u128 = 1 << 6;
pub const NETIF_F_UFO: u128 = 1 << 7;
pub const NETIF_F_GSO: u128 = 1 << 8;
pub const NETIF_F_GSO_PARTIAL: u128 = 1 << 9;
pub const NETIF_F_GSO_ESP: u128 = 1 << 10;
pub const NETIF_F_GRO: u128 = 1 << 11;
pub const NETIF_F_GRO_HW: u128 = 1 << 12;
pub const NETIF_F_LRO: u128 = 1 << 13;
pub const NETIF_F_RXHASH: u128 = 1 << 14;
pub const NETIF_F_RXVLAN: u128 = 1 << 15;
pub const NETIF_F_TXVLAN: u128 = 1 << 16;
pub const NETIF_F_SG: u128 = 1 << 17;
pub const NETIF_F_HIGHDMA: u128 = 1 << 18;
pub const NETIF_F_NTUPLE: u128 = 1 << 19;
pub const NETIF_F_HW_VLAN_CTAG_FILTER: u128 = 1 << 20;
pub const NETIF_F_HW_VLAN_STAG_FILTER: u128 = 1 << 21;
pub const NETIF_F_HW_TC: u128 = 1 << 22;
pub const NETIF_F_XDP: u128 = 1 << 23;
pub const NETIF_F_HW_TLS: u128 = 1 << 24;
pub const NETIF_F_FRAGLIST: u128 = 1 << 25;
pub const NETIF_F_HW_MACSEC: u128 = 1 << 26;
pub const NETIF_F_RFS: u128 = 1 << 27;
pub const NETIF_F_RSS: u128 = 1 << 28;
pub const NETIF_F_HW_TIMESTAMPING: u128 = 1 << 29;
pub const NETIF_F_MULTI_QUEUE: u128 = 1 << 30;

pub const NETIF_F_GSO_MASK: u128 = NETIF_F_GSO
    | NETIF_F_TSO | NETIF_F_TSO6 | NETIF_F_TSO_ECN
    | NETIF_F_UFO | NETIF_F_GSO_PARTIAL | NETIF_F_GSO_ESP
    | NETIF_F_FRAGLIST;

pub const NETIF_F_ONE_FOR_ALL: u128 = NETIF_F_GSO | NETIF_F_GRO | NETIF_F_SG | NETIF_F_HIGHDMA;

pub const NETIF_F_ALL_FOR_ALL: u128 = NETIF_F_HW_CSUM | NETIF_F_IP_CSUM | NETIF_F_IPV6_CSUM
    | NETIF_F_RXCSUM | NETIF_F_TSO | NETIF_F_TSO6 | NETIF_F_UFO | NETIF_F_GSO
    | NETIF_F_GRO | NETIF_F_RXHASH | NETIF_F_SG | NETIF_F_HIGHDMA | NETIF_F_XDP
    | NETIF_F_RSS | NETIF_F_MULTI_QUEUE;

pub const NETIF_F_SOFTWARE_FEATURES: u128 = NETIF_F_GSO | NETIF_F_GRO;

// ============================================================================
// NetDeviceOps — function-pointer vtable
// ============================================================================

pub type NdoOpenFn = Arc<dyn Fn() -> Result<(), NetError> + Send + Sync>;
pub type NdoStopFn = Arc<dyn Fn() -> Result<(), NetError> + Send + Sync>;
pub type NdoStartXmitFn = Arc<dyn Fn(&[u8]) -> Result<(), NetError> + Send + Sync>;
pub type NdoSelectQueueFn = Arc<dyn Fn(&[u8]) -> u16 + Send + Sync>;
pub type NdoGetStatsFn = Arc<dyn Fn() -> NetStats + Send + Sync>;
pub type NdoTxTimeoutFn = Arc<dyn Fn() + Send + Sync>;
pub type NdoSetRxModeFn = Arc<dyn Fn() + Send + Sync>;
pub type NdoSetMacFn = Arc<dyn Fn(MacAddr) -> Result<(), NetError> + Send + Sync>;
pub type NdoChangeMtuFn = Arc<dyn Fn(u16) -> Result<(), NetError> + Send + Sync>;
pub type NdoXdpSetupFn = Arc<dyn Fn(Option<&str>) -> Result<(), NetError> + Send + Sync>;
pub type NdoXdpXmitFn = Arc<dyn Fn(&[u8]) -> Result<(), NetError> + Send + Sync>;

pub struct NetDeviceOps {
    pub ndo_open: Option<NdoOpenFn>,
    pub ndo_stop: Option<NdoStopFn>,
    pub ndo_start_xmit: Option<NdoStartXmitFn>,
    pub ndo_select_queue: Option<NdoSelectQueueFn>,
    pub ndo_get_stats: Option<NdoGetStatsFn>,
    pub ndo_tx_timeout: Option<NdoTxTimeoutFn>,
    pub ndo_set_rx_mode: Option<NdoSetRxModeFn>,
    pub ndo_set_mac: Option<NdoSetMacFn>,
    pub ndo_change_mtu: Option<NdoChangeMtuFn>,
    pub ndo_xdp_setup: Option<NdoXdpSetupFn>,
    pub ndo_xdp_xmit: Option<NdoXdpXmitFn>,
}

impl NetDeviceOps {
    pub const fn empty() -> Self {
        NetDeviceOps {
            ndo_open: None,
            ndo_stop: None,
            ndo_start_xmit: None,
            ndo_select_queue: None,
            ndo_get_stats: None,
            ndo_tx_timeout: None,
            ndo_set_rx_mode: None,
            ndo_set_mac: None,
            ndo_change_mtu: None,
            ndo_xdp_setup: None,
            ndo_xdp_xmit: None,
        }
    }
}

// ============================================================================
// XDP Action codes (Linux kernel XDP_RETCODE values)
// ============================================================================

pub const XDP_ABORTED: u32 = 0;
pub const XDP_DROP: u32 = 1;
pub const XDP_PASS: u32 = 2;
pub const XDP_TX: u32 = 3;
pub const XDP_REDIRECT: u32 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XdpResult {
    XdpPass,
    XdpDrop,
    XdpTx,
    XdpRedirect,
    XdpAborted,
}

// ============================================================================
// XDP Mode + State
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XdpMode {
    None,
    Native,
    Offloaded,
    Generic,
}

// ============================================================================
// NetDevice — uniform NIC abstraction
// ============================================================================

pub struct NetDevice {
    pub name: String,
    pub dev_id: u32,
    pub mac: MacAddr,
    pub ip: super::Ipv4Addr,
    pub netmask: super::Ipv4Addr,
    pub gateway: Option<super::Ipv4Addr>,
    pub mtu: AtomicU16,
    pub flags: u128,
    pub state: AtomicU32,
    pub num_rx_queues: u16,
    pub num_tx_queues: u16,
    pub up: AtomicBool,
    pub promiscuous: AtomicBool,
    pub xdp_mode: Mutex<XdpMode>,
    pub xdp_prog_name: Mutex<Option<String>>,
    pub napi_instances: Mutex<Vec<super::napi::NapiInstance>>,
    pub stats: Mutex<NetStats>,
    pub ops: NetDeviceOps,
}

impl NetDevice {
    fn next_id() -> u32 {
        static NEXT_DEVICE_ID: AtomicU32 = AtomicU32::new(1);
        NEXT_DEVICE_ID.fetch_add(1, Ordering::Relaxed)
    }

    pub fn new(name: &str, mac: MacAddr, mtu: u16) -> Self {
        NetDevice {
            name: String::from(name),
            dev_id: Self::next_id(),
            mac,
            ip: super::Ipv4Addr::UNSPECIFIED,
            netmask: super::Ipv4Addr::new(255, 255, 255, 0),
            gateway: None,
            mtu: AtomicU16::new(mtu),
            flags: NETIF_F_SOFTWARE_FEATURES | NETIF_F_SG | NETIF_F_IP_CSUM,
            state: AtomicU32::new(0),
            num_rx_queues: 1,
            num_tx_queues: 1,
            up: AtomicBool::new(false),
            promiscuous: AtomicBool::new(false),
            xdp_mode: Mutex::new(XdpMode::None),
            xdp_prog_name: Mutex::new(None),
            napi_instances: Mutex::new(Vec::new()),
            stats: Mutex::new(NetStats::default()),
            ops: NetDeviceOps::empty(),
        }
    }

    pub fn open(&self) -> Result<(), NetError> {
        if let Some(ref f) = self.ops.ndo_open {
            f()?;
        }
        self.up.store(true, Ordering::Release);
        Ok(())
    }

    pub fn stop(&self) -> Result<(), NetError> {
        if let Some(ref f) = self.ops.ndo_stop {
            f()?;
        }
        self.up.store(false, Ordering::Release);
        Ok(())
    }

    pub fn start_xmit(&self, data: &[u8]) -> Result<(), NetError> {
        if !self.up.load(Ordering::Acquire) {
            return Err(NetError::NotUp);
        }
        if let Some(ref f) = self.ops.ndo_start_xmit {
            f(data)?;
        } else {
            return Err(NetError::NotSupported);
        }
        self.stats.lock().tx_packets += 1;
        self.stats.lock().tx_bytes += data.len() as u64;
        Ok(())
    }

    pub fn select_queue(&self, data: &[u8]) -> u16 {
        match self.ops.ndo_select_queue {
            Some(ref f) => f(data),
            None => 0,
        }
    }

    pub fn tx_timeout(&self) {
        if let Some(ref f) = self.ops.ndo_tx_timeout {
            f();
        }
    }

    pub fn set_rx_mode(&self) {
        if let Some(ref f) = self.ops.ndo_set_rx_mode {
            f();
        }
    }

    pub fn set_mac(&self, mac: MacAddr) -> Result<(), NetError> {
        match self.ops.ndo_set_mac {
            Some(ref f) => f(mac),
            None => {
                Err(NetError::NotSupported)
            }
        }
    }

    pub fn change_mtu(&self, mtu: u16) -> Result<(), NetError> {
        match self.ops.ndo_change_mtu {
            Some(ref f) => {
                let mtu = mtu.max(68).min(65535);
                f(mtu)?;
                self.mtu.store(mtu, Ordering::Release);
                Ok(())
            }
            None => Err(NetError::NotSupported),
        }
    }

    pub fn attach_xdp(&self, prog_name: Option<&str>) -> Result<(), NetError> {
        if self.flags & NETIF_F_XDP == 0 {
            return Err(NetError::NotSupported);
        }
        match self.ops.ndo_xdp_setup {
            Some(ref f) => {
                f(prog_name)?;
                let mut mode = self.xdp_mode.lock();
                let mut prog = self.xdp_prog_name.lock();
                match prog_name {
                    Some(name) => {
                        *mode = XdpMode::Native;
                        *prog = Some(String::from(name));
                    }
                    None => {
                        *mode = XdpMode::None;
                        *prog = None;
                    }
                }
                Ok(())
            }
            None => Err(NetError::NotSupported),
        }
    }

    pub fn xdp_is_attached(&self) -> bool {
        self.xdp_mode.lock().clone() != XdpMode::None
    }

    pub fn get_stats(&self) -> NetStats {
        if let Some(ref f) = self.ops.ndo_get_stats {
            f()
        } else {
            self.stats.lock().clone()
        }
    }

    pub fn set_features(&mut self, feat: u128) {
        self.flags |= feat;
    }

    pub fn clear_features(&mut self, feat: u128) {
        self.flags &= !feat;
    }

    pub fn has_feature(&self, feat: u128) -> bool {
        self.flags & feat != 0
    }

    pub fn supports_gso(&self) -> bool {
        self.flags & (NETIF_F_GSO | NETIF_F_TSO) != 0
    }

    pub fn supports_gro(&self) -> bool {
        self.flags & NETIF_F_GRO != 0
    }

    pub fn supports_xdp(&self) -> bool {
        self.flags & NETIF_F_XDP != 0
    }

    pub fn supports_rss(&self) -> bool {
        self.flags & NETIF_F_RSS != 0
    }

    pub fn is_multi_queue(&self) -> bool {
        self.flags & NETIF_F_MULTI_QUEUE != 0
    }

    pub fn add_napi(&self, napi: super::napi::NapiInstance) {
        self.napi_instances.lock().push(napi);
    }

    pub fn set_queue_count(&mut self, rx: u16, tx: u16) {
        self.num_rx_queues = rx;
        self.num_tx_queues = tx;
        if rx > 1 || tx > 1 {
            self.flags |= NETIF_F_MULTI_QUEUE;
        }
    }

    pub fn setup_multi_queue_napis(&self, budget: u32) {
        let current = self.napi_instances.lock().len() as u16;
        if current >= self.num_rx_queues {
            return;
        }
        for qid in current..self.num_rx_queues {
            let napi = super::napi::create_queue_napi(&self.name, qid, budget);
            napi.enable();
            self.napi_instances.lock().push((*napi).clone());
        }
    }

    pub fn setup_multi_queue_with_napi(&mut self, rx: u16, tx: u16, budget: u32) {
        self.num_rx_queues = rx;
        self.num_tx_queues = tx;
        if rx > 1 || tx > 1 {
            self.flags |= NETIF_F_MULTI_QUEUE;
        }
        self.setup_multi_queue_napis(budget);
    }

    pub fn enqueue_rx_multi_queue(&self, data: Vec<u8>, queue_id: u16) -> bool {
        let instances = self.napi_instances.lock();
        for napi in instances.iter() {
            if napi.queue_id == queue_id {
                napi.rx_enqueue(data);
                return true;
            }
        }
        false
    }

    pub fn run_xdp_on_device(&self, packet: &[u8]) -> XdpResult {
        if !self.supports_xdp() {
            return XdpResult::XdpPass;
        }
        let prog = self.xdp_prog_name.lock();
        if prog.is_none() {
            return XdpResult::XdpPass;
        }
        drop(prog);
        match super::ebpf::run_xdp(packet) {
            Ok(action) => match action as u32 {
                XDP_DROP => {
                    self.stats.lock().rx_dropped += 1;
                    XdpResult::XdpDrop
                }
                XDP_PASS => XdpResult::XdpPass,
                XDP_TX => {
                    if self.xdp_xmit_local(packet).is_ok() {
                        XdpResult::XdpTx
                    } else {
                        XdpResult::XdpDrop
                    }
                }
                XDP_REDIRECT => XdpResult::XdpRedirect,
                _ => XdpResult::XdpAborted,
            },
            Err(_) => XdpResult::XdpPass,
        }
    }

    pub fn xdp_xmit_local(&self, packet: &[u8]) -> Result<(), NetError> {
        if let Some(ref f) = self.ops.ndo_xdp_xmit {
            f(packet)
        } else if let Some(ref f) = self.ops.ndo_start_xmit {
            f(packet)
        } else {
            Err(NetError::NotSupported)
        }
    }
}

// ============================================================================
// NetDeviceManager — global device registry
// ============================================================================

pub struct NetDeviceManager {
    devices: Mutex<Vec<Arc<NetDevice>>>,
}

impl NetDeviceManager {
    pub const fn new() -> Self {
        NetDeviceManager {
            devices: Mutex::new(Vec::new()),
        }
    }

    pub fn register(&self, dev: Arc<NetDevice>) {
        self.devices.lock().push(dev);
    }

    pub fn unregister(&self, name: &str) -> bool {
        let mut devices = self.devices.lock();
        let len_before = devices.len();
        devices.retain(|d| d.name != name);
        devices.len() < len_before
    }

    pub fn get(&self, name: &str) -> Option<Arc<NetDevice>> {
        let devices = self.devices.lock();
        for d in devices.iter() {
            if d.name == name {
                return Some(d.clone());
            }
        }
        None
    }

    pub fn get_by_id(&self, id: u32) -> Option<Arc<NetDevice>> {
        let devices = self.devices.lock();
        for d in devices.iter() {
            if d.dev_id == id {
                return Some(d.clone());
            }
        }
        None
    }

    pub fn all(&self) -> Vec<Arc<NetDevice>> {
        self.devices.lock().clone()
    }

    pub fn len(&self) -> usize {
        self.devices.lock().len()
    }

    pub fn has(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    pub fn first(&self) -> Option<Arc<NetDevice>> {
        self.devices.lock().first().cloned()
    }

    pub fn find_by_ip(&self, ip: super::Ipv4Addr) -> Option<Arc<NetDevice>> {
        let devices = self.devices.lock();
        for d in devices.iter() {
            if d.ip == ip {
                return Some(d.clone());
            }
        }
        None
    }

    pub fn iter(&self) -> Vec<Arc<NetDevice>> {
        self.devices.lock().clone()
    }
}

pub static NET_DEVICE_MANAGER: NetDeviceManager = NetDeviceManager::new();

pub fn init() {
    crate::serial_println!("[NETDEV] NetDevice manager initialized");
}

pub fn register_device(dev: Arc<NetDevice>) {
    NET_DEVICE_MANAGER.register(dev);
}

pub fn get_device(name: &str) -> Option<Arc<NetDevice>> {
    NET_DEVICE_MANAGER.get(name)
}

pub fn get_all_devices() -> Vec<Arc<NetDevice>> {
    NET_DEVICE_MANAGER.all()
}

// ============================================================================
// Helpers: wrap existing NetInterface into NetDevice with default ops
// ============================================================================

pub fn wrap_interface(
    iface: &dyn super::NetInterface,
) -> NetDevice {
    let mtu = iface.mtu();
    let mac = iface.mac();
    NetDevice::new(iface.name(), mac, mtu)
}

pub fn default_ops_for_interface(
    dev: &Arc<NetDevice>,
    iface: &Arc<Mutex<dyn NetInterface>>,
) -> NetDeviceOps {
    let dev_name = String::from(&dev.name);
    let iface_clone = iface.clone();

    let ndo_open = Some(Arc::new(move || {
        let mut guard = iface_clone.lock();
        guard.set_up(true);
        crate::serial_println!("[NETDEV] {} opened", dev_name);
        Ok(())
    }) as NdoOpenFn);

    let iface_clone2 = iface.clone();
    let dev_name2 = String::from(&dev.name);
    let ndo_stop = Some(Arc::new(move || {
        let mut guard = iface_clone2.lock();
        guard.set_up(false);
        crate::serial_println!("[NETDEV] {} stopped", dev_name2);
        Ok(())
    }) as NdoStopFn);

    let iface_clone3 = iface.clone();
    let ndo_start_xmit = Some(Arc::new(move |data: &[u8]| {
        let mut guard = iface_clone3.lock();
        guard.send(data)
    }) as NdoStartXmitFn);

    let iface_clone4 = iface.clone();
    let ndo_get_stats = Some(Arc::new(move || {
        let guard = iface_clone4.lock();
        guard.stats()
    }) as NdoGetStatsFn);

    let ndo_set_mac = Some(Arc::new(move |_mac: MacAddr| {
        Ok(())
    }) as NdoSetMacFn);

    let ndo_select_queue = Some(Arc::new(move |data: &[u8]| -> u16 {
        if data.len() < 14 { return 0; }
        let hash = u16::from_be_bytes([data[12], data[13]]);
        hash % 4
    }) as NdoSelectQueueFn);

    NetDeviceOps {
        ndo_open,
        ndo_stop,
        ndo_start_xmit,
        ndo_select_queue: None,
        ndo_get_stats,
        ndo_tx_timeout: None,
        ndo_set_rx_mode: None,
        ndo_set_mac,
        ndo_change_mtu: None,
        ndo_xdp_setup: None,
        ndo_xdp_xmit: None,
    }
}

pub fn register_interface_as_device(
    iface: Arc<Mutex<dyn NetInterface>>,
) -> Arc<NetDevice> {
    let guard = iface.lock();
    let mut dev = wrap_interface(&*guard);
    drop(guard);

    let dev = Arc::new(dev);
    let ops = default_ops_for_interface(&dev, &iface);
    unsafe {
        let ptr = &dev as *const Arc<NetDevice> as *mut NetDevice;
        (*ptr).ops = ops;
    }

    NET_DEVICE_MANAGER.register(dev.clone());
    dev
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

    #[test]
    fn net_device_create() {
        let dev = NetDevice::new("eth0", MacAddr::new([0x52, 0x54, 0x00, 0x12, 0x34, 0x56]), 1500);
        assert_eq!(dev.name, "eth0");
        assert!(!dev.up.load(Ordering::Acquire));
        assert_eq!(dev.mtu.load(Ordering::Acquire), 1500);
        assert_eq!(dev.num_rx_queues, 1);
        assert_eq!(dev.num_tx_queues, 1);
        assert!(dev.dev_id > 0);
    }

    #[test]
    fn net_device_feature_flags() {
        let mut dev = NetDevice::new("eth0", MacAddr::ZERO, 1500);
        assert!(dev.has_feature(NETIF_F_SOFTWARE_FEATURES));
        assert!(!dev.has_feature(NETIF_F_XDP));

        dev.set_features(NETIF_F_XDP | NETIF_F_RSS | NETIF_F_MULTI_QUEUE);
        assert!(dev.supports_xdp());
        assert!(dev.supports_rss());
        assert!(dev.is_multi_queue());

        dev.clear_features(NETIF_F_XDP);
        assert!(!dev.supports_xdp());
    }

    #[test]
    fn net_device_open_stop() {
        let dev = Arc::new(NetDevice::new("test", MacAddr::ZERO, 1500));

        let opened = Arc::new(AtomicBool::new(false));
        let stopped = Arc::new(AtomicBool::new(false));
        let o = opened.clone();
        let s = stopped.clone();

        unsafe {
            let ptr = &dev as *const Arc<NetDevice> as *mut NetDevice;
            (*ptr).ops.ndo_open = Some(Arc::new(move || {
                o.store(true, Ordering::Release);
                Ok(())
            }) as NdoOpenFn);
            (*ptr).ops.ndo_stop = Some(Arc::new(move || {
                s.store(true, Ordering::Release);
                Ok(())
            }) as NdoStopFn);
        }

        dev.open().unwrap();
        assert!(opened.load(Ordering::Acquire));
        assert!(dev.up.load(Ordering::Acquire));

        dev.stop().unwrap();
        assert!(stopped.load(Ordering::Acquire));
        assert!(!dev.up.load(Ordering::Acquire));
    }

    #[test]
    fn net_device_start_xmit() {
        let dev = Arc::new(NetDevice::new("test", MacAddr::ZERO, 1500));
        let xmit_called = Arc::new(AtomicBool::new(false));
        let xc = xmit_called.clone();

        unsafe {
            let ptr = &dev as *const Arc<NetDevice> as *mut NetDevice;
            (*ptr).ops.ndo_start_xmit = Some(Arc::new(move |_data: &[u8]| {
                xc.store(true, Ordering::Release);
                Ok(())
            }) as NdoStartXmitFn);
        }

        assert_eq!(dev.start_xmit(&[0u8; 64]), Err(NetError::NotUp));

        dev.up.store(true, Ordering::Release);
        dev.start_xmit(&[0u8; 64]).unwrap();
        assert!(xmit_called.load(Ordering::Acquire));
    }

    #[test]
    fn net_device_select_queue() {
        let mut dev = NetDevice::new("test", MacAddr::ZERO, 1500);
        dev.set_queue_count(4, 4);
        assert!(dev.is_multi_queue());

        let select_called = Arc::new(AtomicU32::new(0));
        let sc = select_called.clone();

        dev.ops.ndo_select_queue = Some(Arc::new(move |_data: &[u8]| {
            sc.fetch_add(1, Ordering::Release);
            2u16
        }) as NdoSelectQueueFn);

        assert_eq!(dev.select_queue(&[0u8; 64]), 2);
        assert_eq!(select_called.load(Ordering::Acquire), 1);
    }

    #[test]
    fn net_device_xdp_attach() {
        let mut dev = NetDevice::new("test", MacAddr::ZERO, 1500);
        dev.set_features(NETIF_F_XDP);
        assert!(dev.supports_xdp());

        let setup_called = Arc::new(AtomicBool::new(false));
        let sc = setup_called.clone();

        dev.ops.ndo_xdp_setup = Some(Arc::new(move |prog: Option<&str>| {
            sc.store(true, Ordering::Release);
            Ok(())
        }) as NdoXdpSetupFn);

        dev.attach_xdp(Some("xdp-drop")).unwrap();
        assert!(setup_called.load(Ordering::Acquire));
        assert!(dev.xdp_is_attached());

        dev.attach_xdp(None).unwrap();
        assert!(!dev.xdp_is_attached());
    }

    #[test]
    fn net_device_xdp_not_supported() {
        let dev = NetDevice::new("test", MacAddr::ZERO, 1500);
        assert!(!dev.supports_xdp());
        assert_eq!(dev.attach_xdp(Some("xdp-prog")), Err(NetError::NotSupported));
    }

    #[test]
    fn net_device_mtu_change() {
        let dev = Arc::new(NetDevice::new("test", MacAddr::ZERO, 1500));
        let mtu_called = Arc::new(AtomicU16::new(0));
        let mc = mtu_called.clone();

        unsafe {
            let ptr = &dev as *const Arc<NetDevice> as *mut NetDevice;
            (*ptr).ops.ndo_change_mtu = Some(Arc::new(move |mtu: u16| {
                mc.store(mtu, Ordering::Release);
                Ok(())
            }) as NdoChangeMtuFn);
        }

        dev.change_mtu(9000).unwrap();
        assert_eq!(mtu_called.load(Ordering::Acquire), 9000);
        assert_eq!(dev.mtu.load(Ordering::Acquire), 9000);
    }

    #[test]
    fn net_device_manager_register() {
        let mgr = NetDeviceManager::new();

        let d1 = Arc::new(NetDevice::new("eth0", MacAddr::ZERO, 1500));
        let d2 = Arc::new(NetDevice::new("lo", MacAddr::ZERO, 65535));

        mgr.register(d1);
        mgr.register(d2);
        assert_eq!(mgr.len(), 2);
        assert!(mgr.has("eth0"));
        assert!(mgr.has("lo"));

        let eth0 = mgr.get("eth0").unwrap();
        assert_eq!(eth0.name, "eth0");

        assert!(mgr.unregister("eth0"));
        assert_eq!(mgr.len(), 1);
        assert!(!mgr.has("eth0"));
    }

    #[test]
    fn net_device_set_queue_count() {
        let mut dev = NetDevice::new("multi", MacAddr::ZERO, 1500);
        assert_eq!(dev.num_rx_queues, 1);
        assert!(!dev.is_multi_queue());

        dev.set_queue_count(8, 8);
        assert_eq!(dev.num_rx_queues, 8);
        assert_eq!(dev.num_tx_queues, 8);
        assert!(dev.is_multi_queue());
    }

    #[test]
    fn net_device_gso_gro_helpers() {
        let mut dev = NetDevice::new("test", MacAddr::ZERO, 1500);
        assert!(dev.supports_gso());
        assert!(dev.supports_gro());

        dev.clear_features(NETIF_F_GSO | NETIF_F_GRO);
        assert!(!dev.supports_gso());
        assert!(!dev.supports_gro());
    }

    #[test]
    fn net_device_stats() {
        let dev = NetDevice::new("test", MacAddr::ZERO, 1500);
        dev.stats.lock().tx_packets = 42;
        dev.stats.lock().tx_bytes = 4096;

        let s = dev.get_stats();
        assert_eq!(s.tx_packets, 42);
        assert_eq!(s.tx_bytes, 4096);
    }

    #[test]
    fn net_device_unique_ids() {
        let d1 = NetDevice::new("a", MacAddr::ZERO, 1500);
        let d2 = NetDevice::new("b", MacAddr::ZERO, 1500);
        assert_ne!(d1.dev_id, d2.dev_id);
    }

    #[test]
    fn net_device_multi_queue_flag_via_set_queue_count() {
        let mut dev = NetDevice::new("test", MacAddr::ZERO, 1500);
        dev.set_queue_count(4, 2);
        assert!(dev.has_feature(NETIF_F_MULTI_QUEUE));
    }

    #[test]
    fn net_device_xdp_action_constants() {
        assert_eq!(XDP_ABORTED, 0);
        assert_eq!(XDP_DROP, 1);
        assert_eq!(XDP_PASS, 2);
        assert_eq!(XDP_TX, 3);
        assert_eq!(XDP_REDIRECT, 4);
    }

    #[test]
    fn net_device_run_xdp_no_xdp_support() {
        let dev = NetDevice::new("test", MacAddr::ZERO, 1500);
        assert!(!dev.supports_xdp());
        let result = dev.run_xdp_on_device(&[0u8; 64]);
        assert_eq!(result, XdpResult::XdpPass);
    }

    #[test]
    fn net_device_run_xdp_no_program() {
        let mut dev = NetDevice::new("test", MacAddr::ZERO, 1500);
        dev.set_features(NETIF_F_XDP);
        assert!(dev.supports_xdp());
        let result = dev.run_xdp_on_device(&[0u8; 64]);
        assert_eq!(result, XdpResult::XdpPass);
    }

    #[test]
    fn net_device_xdp_xmit_local() {
        let dev = Arc::new(NetDevice::new("test", MacAddr::ZERO, 1500));
        let xmit_called = Arc::new(AtomicBool::new(false));
        let xc = xmit_called.clone();

        unsafe {
            let ptr = &dev as *const Arc<NetDevice> as *mut NetDevice;
            (*ptr).ops.ndo_xdp_xmit = Some(Arc::new(move |_data: &[u8]| {
                xc.store(true, Ordering::Release);
                Ok(())
            }) as NdoXdpXmitFn);
        }

        dev.up.store(true, Ordering::Release);
        dev.xdp_xmit_local(&[0u8; 64]).unwrap();
        assert!(xmit_called.load(Ordering::Acquire));
    }

    #[test]
    fn net_device_xdp_xmit_local_fallback_to_start_xmit() {
        let dev = Arc::new(NetDevice::new("test", MacAddr::ZERO, 1500));
        let xmit_called = Arc::new(AtomicBool::new(false));
        let xc = xmit_called.clone();

        unsafe {
            let ptr = &dev as *const Arc<NetDevice> as *mut NetDevice;
            (*ptr).ops.ndo_start_xmit = Some(Arc::new(move |_data: &[u8]| {
                xc.store(true, Ordering::Release);
                Ok(())
            }) as NdoStartXmitFn);
        }

        dev.up.store(true, Ordering::Release);
        dev.xdp_xmit_local(&[0u8; 64]).unwrap();
        assert!(xmit_called.load(Ordering::Acquire));
    }

    #[test]
    fn net_device_first_and_find_by_ip() {
        let mgr = NetDeviceManager::new();

        let mut d1 = NetDevice::new("eth0", MacAddr::ZERO, 1500);
        d1.ip = super::Ipv4Addr::new(192, 168, 1, 10);
        let d1 = Arc::new(d1);

        let mut d2 = NetDevice::new("eth1", MacAddr::ZERO, 1500);
        d2.ip = super::Ipv4Addr::new(10, 0, 0, 1);
        let d2 = Arc::new(d2);

        mgr.register(d1);
        mgr.register(d2);

        let first = mgr.first().unwrap();
        assert_eq!(first.name, "eth0");

        let by_ip = mgr.find_by_ip(super::Ipv4Addr::new(10, 0, 0, 1)).unwrap();
        assert_eq!(by_ip.name, "eth1");

        let none = mgr.find_by_ip(super::Ipv4Addr::new(1, 2, 3, 4));
        assert!(none.is_none());
    }

    #[test]
    fn net_device_multi_queue_napi_setup() {
        let mut dev = NetDevice::new("multi-q", MacAddr::ZERO, 1500);
        dev.setup_multi_queue_with_napi(4, 4, 64);
        assert!(dev.is_multi_queue());
        assert_eq!(dev.num_rx_queues, 4);
        assert_eq!(dev.num_tx_queues, 4);

        dev.enqueue_rx_multi_queue(vec![1u8; 64], 0);
        dev.enqueue_rx_multi_queue(vec![2u8; 64], 3);

        let instances = dev.napi_instances.lock();
        assert!(!instances.is_empty());
        let q0_has_pkt = instances.iter().any(|n| n.queue_id == 0 && n.rx_queue_len() == 1);
        assert!(q0_has_pkt);
    }

    #[test]
    fn net_device_ndo_select_queue_hash_based() {
        let dev = Arc::new(NetDevice::new("hash-test", MacAddr::ZERO, 1500));

        unsafe {
            let ptr = &dev as *const Arc<NetDevice> as *mut NetDevice;
            (*ptr).ops.ndo_select_queue = Some(Arc::new(
                move |data: &[u8]| -> u16 {
                    if data.len() < 14 { return 0; }
                    let hash = u16::from_be_bytes([data[12], data[13]]);
                    hash % 4
                }
            ) as NdoSelectQueueFn);
        }

        let pkt = vec![0u8; 64];
        let q = dev.select_queue(&pkt);
        assert!(q < 4);

        let pkt2 = vec![0xFFu8; 64];
        let q2 = dev.select_queue(&pkt2);
        assert!(q2 < 4);
    }

    #[test]
    fn net_device_enqueue_specific_queue() {
        let dev = Arc::new(NetDevice::new("enq-test", MacAddr::ZERO, 1500));
        unsafe {
            let ptr = &dev as *const Arc<NetDevice> as *mut NetDevice;
            (*ptr).setup_multi_queue_with_napi(3, 1, 64);
        }

        let ok = dev.enqueue_rx_multi_queue(vec![10u8; 64], 1);
        assert!(ok);

        let bad = dev.enqueue_rx_multi_queue(vec![0u8; 64], 99);
        assert!(!bad);
    }
}
