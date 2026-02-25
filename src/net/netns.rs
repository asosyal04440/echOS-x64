//! # Network Namespaces
//!
//! Container network isolation.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// NETWORK NAMESPACE
// ============================================================================

pub struct NetNamespace {
    /// Namespace ID
    pub id: u64,
    /// Namespace name
    pub name: String,
    /// Network devices in this namespace
    pub devices: Mutex<BTreeMap<String, Arc<NetDevice>>>,
    /// Loopback device
    pub loopback: Option<Arc<NetDevice>>,
    /// IP addresses
    pub addresses: Mutex<Vec<IpAddress>>,
    /// Routes
    pub routes: Mutex<Vec<Route>>,
    /// iptables
    pub iptables: crate::net::netfilter::NetfilterManager,
    /// Is active
    pub active: AtomicBool,
    /// Process count
    pub process_count: AtomicU32,
}

#[derive(Clone, Debug)]
pub struct NetDevice {
    pub name: String,
    pub ifindex: u32,
    pub mtu: u32,
    pub mac: [u8; 6],
    pub flags: AtomicU32,
    pub tx_bytes: AtomicU64,
    pub rx_bytes: AtomicU64,
    pub tx_packets: AtomicU64,
    pub rx_packets: AtomicU64,
}

impl NetDevice {
    pub fn new(name: &str, ifindex: u32) -> Self {
        Self {
            name: String::from(name),
            ifindex,
            mtu: 1500,
            mac: [0; 6],
            flags: AtomicU32::new(0),
            tx_bytes: AtomicU64::new(0),
            rx_bytes: AtomicU64::new(0),
            tx_packets: AtomicU64::new(0),
            rx_packets: AtomicU64::new(0),
        }
    }
}

#[derive(Clone, Debug)]
pub struct IpAddress {
    pub addr: u32,
    pub prefix_len: u8,
    pub ifindex: u32,
    pub scope: u8,
}

#[derive(Clone, Debug)]
pub struct Route {
    pub dst: u32,
    pub dst_len: u8,
    pub gateway: u32,
    pub ifindex: u32,
    pub metric: u32,
}

impl NetNamespace {
    pub fn new(id: u64, name: &str) -> Self {
        Self {
            id,
            name: String::from(name),
            devices: Mutex::new(BTreeMap::new()),
            loopback: None,
            addresses: Mutex::new(Vec::new()),
            routes: Mutex::new(Vec::new()),
            iptables: crate::net::netfilter::NetfilterManager::new(),
            active: AtomicBool::new(true),
            process_count: AtomicU32::new(0),
        }
    }

    /// Add device
    pub fn add_device(&self, device: Arc<NetDevice>) {
        self.devices.lock().insert(device.name.clone(), device);
    }

    /// Remove device
    pub fn remove_device(&self, name: &str) -> Option<Arc<NetDevice>> {
        self.devices.lock().remove(name)
    }

    /// Get device
    pub fn get_device(&self, name: &str) -> Option<Arc<NetDevice>> {
        self.devices.lock().get(name).cloned()
    }

    /// Add address
    pub fn add_address(&self, addr: IpAddress) {
        self.addresses.lock().push(addr);
    }

    /// Add route
    pub fn add_route(&self, route: Route) {
        self.routes.lock().push(route);
    }

    /// Lookup route for destination
    pub fn lookup_route(&self, dst: u32) -> Option<Route> {
        let routes = self.routes.lock();
        let mut best: Option<&Route> = None;
        let mut best_len = 0u8;
        
        for route in routes.iter() {
            let mask = if route.dst_len == 0 { 0 } else { !0u32 << (32 - route.dst_len) };
            if (dst & mask) == (route.dst & mask) {
                if route.dst_len >= best_len {
                    best = Some(route);
                    best_len = route.dst_len;
                }
            }
        }
        
        best.cloned()
    }

    /// Increment process count
    pub fn add_process(&self) {
        self.process_count.fetch_add(1, Ordering::SeqCst);
    }

    /// Decrement process count
    pub fn remove_process(&self) {
        self.process_count.fetch_sub(1, Ordering::SeqCst);
    }
}

// ============================================================================
// NAMESPACE MANAGER
// ============================================================================

pub struct NetNamespaceManager {
    namespaces: Mutex<BTreeMap<u64, Arc<NetNamespace>>>,
    current_ns: Mutex<u64>,
    next_id: AtomicU64,
}

impl NetNamespaceManager {
    pub const fn new() -> Self {
        Self {
            namespaces: Mutex::new(BTreeMap::new()),
            current_ns: Mutex::new(0),
            next_id: AtomicU64::new(1),
        }
    }

    /// Initialize with root namespace
    pub fn init(&self) {
        let root = Arc::new(NetNamespace::new(0, "init"));
        
        // Add loopback
        let lo = Arc::new(NetDevice::new("lo", 1));
        root.add_device(lo.clone());
        
        self.namespaces.lock().insert(0, root);
        
        crate::serial_println!("[NETNS] Initialized root network namespace");
    }

    /// Create new namespace
    pub fn create(&self, name: &str) -> Arc<NetNamespace> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let ns = Arc::new(NetNamespace::new(id, name));
        
        // Add loopback
        let lo = Arc::new(NetDevice::new("lo", 1));
        ns.add_device(lo.clone());
        
        self.namespaces.lock().insert(id, ns.clone());
        
        crate::serial_println!("[NETNS] Created namespace '{}' (id={})", name, id);
        ns
    }

    /// Delete namespace
    pub fn delete(&self, id: u64) -> bool {
        if id == 0 {
            return false; // Can't delete root
        }
        
        self.namespaces.lock().remove(&id).is_some()
    }

    /// Get namespace
    pub fn get(&self, id: u64) -> Option<Arc<NetNamespace>> {
        self.namespaces.lock().get(&id).cloned()
    }

    /// Get current namespace
    pub fn current(&self) -> Arc<NetNamespace> {
        let id = *self.current_ns.lock();
        self.get(id).unwrap()
    }

    /// Set current namespace
    pub fn set_current(&self, id: u64) -> bool {
        if self.namespaces.lock().contains_key(&id) {
            *self.current_ns.lock() = id;
            true
        } else {
            false
        }
    }

    /// Move device between namespaces
    pub fn move_device(&self, from_ns: u64, to_ns: u64, dev_name: &str) -> bool {
        let from = match self.get(from_ns) {
            Some(ns) => ns,
            None => return false,
        };
        
        let to = match self.get(to_ns) {
            Some(ns) => ns,
            None => return false,
        };
        
        if let Some(dev) = from.remove_device(dev_name) {
            to.add_device(dev);
            return true;
        }
        
        false
    }
}

lazy_static::lazy_static! {
    pub static ref NETNS_MANAGER: NetNamespaceManager = NetNamespaceManager::new();
}

// ============================================================================
// SYSCALL INTERFACE
// ============================================================================

/// unshare(CLONE_NEWNET)
pub fn sys_unshare_newnet() -> i32 {
    let ns = NETNS_MANAGER.create("unshared");
    ns.add_process();
    0
}

/// setns(fd, CLONE_NEWNET)
pub fn sys_setns_net(ns_id: u64) -> i32 {
    if NETNS_MANAGER.set_current(ns_id) {
        0
    } else {
        -22
    }
}

// ============================================================================
// INITIALIZATION
// ============================================================================

pub fn init() {
    NETNS_MANAGER.init();
}
