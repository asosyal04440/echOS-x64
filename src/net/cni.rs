//! # CNI (Container Network Interface)
//!
//! echOS için container network interface implementasyonu.
//! Kubernetes ve container orchestrator'ları için ağ yapılandırması sağlar.
//!
//! ## CNI Nedir?
//!
//! CNI, container'ların ağ bağlantısını yapılandırmak için standart bir arayüz sağlar.
//! Kubernetes, Docker, ve diğer orchestrator'lar tarafından kullanılır.
//!
//! ## CNI Mimarisi
//!
//! ```text
//!  Container Runtime              CNI Plugin              Network
//!  (docker, containerd)           (bridge, flannel)      (VLAN, VXLAN)
//!         │                                │                        │
//!         │--- ADD container -------------->│                        │
//!         │    (container ID,              │                        │
//!         │     netns path,                │                        │
//!         │     CNI config)                │                        │
//!         │                                │                        │
//!         │<-- Result (IP, routes,         │                        │
//!         │    DNS, etc.) -----------------│                        │
//!         │                                │                        │
//!         │--- DEL container -------------->│                        │
//!         │    (cleanup)                   │                        │
//! ```
//!
//! ## CNI Komutları
//!
//! - **ADD**: Container'a network atama
//! - **DEL**: Container'ın network'ünü temizle
//! - **CHECK**: Container'ın network durumunu kontrol et
//! - **VERSION**: CNI plugin versiyonunu göster

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use super::{Ipv4Addr, NetError};

// ============================================================================
// CNI SABİTLERİ
// ============================================================================

/// CNI komutları
pub const CNI_COMMAND_ADD: &str = "ADD";
pub const CNI_COMMAND_DEL: &str = "DEL";
pub const CNI_COMMAND_CHECK: &str = "CHECK";
pub const CNI_COMMAND_VERSION: &str = "VERSION";

/// CNI versiyonu
pub const CNI_VERSION: &str = "0.4.0";

/// Varsayılan bridge adı
pub const DEFAULT_BRIDGE_NAME: &str = "cni0";

/// Varsayılan subnet
pub const DEFAULT_SUBNET: &str = "10.244.0.0/16";

/// Varsayılan IP aralığı başlangıcı
pub const DEFAULT_IP_START: &str = "10.244.0.2";

/// Varsayılan IP aralığı sonu
pub const DEFAULT_IP_END: &str = "10.244.255.254";

// ============================================================================
// CNI HATASI
// ============================================================================

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CniError {
    /// Geçersiz komut
    InvalidCommand,
    /// Geçersiz yapılandırma
    InvalidConfig,
    /// Container bulunamadı
    ContainerNotFound,
    /// Network bulunamadı
    NetworkNotFound,
    /// IP adresi kalmadı
    IpExhausted,
    /// İzin hatası
    PermissionDenied,
    /// Zaman aşımı
    Timeout,
    /// Genel hata
    General(String),
}

impl From<NetError> for CniError {
    fn from(err: NetError) -> Self {
        CniError::General(format!("Network error: {:?}", err))
    }
}

// ============================================================================
// CNI YAPILANDIRMASI
// ============================================================================

/// CNI yapılandırması
#[derive(Clone, Debug)]
pub struct CniConfig {
    /// CNI versiyonu
    pub cni_version: String,
    /// Container adı
    pub container_name: String,
    /// Container ID
    pub container_id: String,
    /// Network namespace yolu
    pub netns: String,
    /// Bridge adı
    pub bridge: String,
    /// IP adresi
    pub ip_address: String,
    /// Gateway
    pub gateway: String,
    /// Subnet
    pub subnet: String,
    /// DNS sunucuları
    pub dns_servers: Vec<String>,
    /// MTU
    pub mtu: i32,
    /// Ek argümanlar
    pub args: BTreeMap<String, String>,
}

impl CniConfig {
    /// JSON'dan yapılandırma yükle
    pub fn from_json(json_str: &str) -> Result<Self, CniError> {
        fn extract_string_value(json: &str, key: &str) -> Option<String> {
            let needle = format!("\"{}\"", key);
            let key_start = json.find(&needle)?;
            let value_start =
                json[key_start + needle.len()..].find(':')? + key_start + needle.len() + 1;
            let tail = json[value_start..].trim_start();
            if !tail.starts_with('"') {
                return None;
            }
            let tail = &tail[1..];
            let end = tail.find('"')?;
            Some(tail[..end].to_string())
        }

        fn extract_i32_value(json: &str, key: &str) -> Option<i32> {
            let needle = format!("\"{}\"", key);
            let key_start = json.find(&needle)?;
            let value_start =
                json[key_start + needle.len()..].find(':')? + key_start + needle.len() + 1;
            let digits = json[value_start..]
                .trim_start()
                .chars()
                .take_while(|ch| ch.is_ascii_digit() || *ch == '-')
                .collect::<String>();
            if digits.is_empty() {
                None
            } else {
                digits.parse::<i32>().ok()
            }
        }

        fn extract_string_array(json: &str, key: &str) -> Vec<String> {
            let needle = format!("\"{}\"", key);
            let Some(key_start) = json.find(&needle) else {
                return Vec::new();
            };
            let Some(array_start_rel) = json[key_start + needle.len()..].find('[') else {
                return Vec::new();
            };
            let array_start = key_start + needle.len() + array_start_rel + 1;
            let Some(array_end_rel) = json[array_start..].find(']') else {
                return Vec::new();
            };
            json[array_start..array_start + array_end_rel]
                .split(',')
                .filter_map(|entry| {
                    let trimmed = entry.trim();
                    if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
                        Some(trimmed[1..trimmed.len() - 1].to_string())
                    } else {
                        None
                    }
                })
                .collect()
        }

        fn extract_args_object(json: &str) -> BTreeMap<String, String> {
            let needle = "\"args\"";
            let Some(key_start) = json.find(needle) else {
                return BTreeMap::new();
            };
            let Some(obj_start_rel) = json[key_start + needle.len()..].find('{') else {
                return BTreeMap::new();
            };
            let obj_start = key_start + needle.len() + obj_start_rel + 1;
            let Some(obj_end_rel) = json[obj_start..].find('}') else {
                return BTreeMap::new();
            };
            let mut args = BTreeMap::new();
            for pair in json[obj_start..obj_start + obj_end_rel].split(',') {
                let mut parts = pair.splitn(2, ':');
                let Some(key) = parts.next() else { continue };
                let Some(value) = parts.next() else { continue };
                let key = key.trim().trim_matches('"');
                let value = value.trim().trim_matches('"');
                if !key.is_empty() {
                    args.insert(key.to_string(), value.to_string());
                }
            }
            args
        }

        crate::serial_println!("[CNI] Parsing config");

        let dns_servers = {
            let parsed = extract_string_array(json_str, "dnsServers");
            if parsed.is_empty() {
                vec!["8.8.8.8".to_string()]
            } else {
                parsed
            }
        };

        Ok(Self {
            cni_version: extract_string_value(json_str, "cniVersion")
                .unwrap_or_else(|| CNI_VERSION.to_string()),
            container_name: extract_string_value(json_str, "name")
                .or_else(|| extract_string_value(json_str, "containerName"))
                .unwrap_or_else(|| "container".to_string()),
            container_id: extract_string_value(json_str, "containerID")
                .unwrap_or_else(|| "123456".to_string()),
            netns: extract_string_value(json_str, "netns")
                .unwrap_or_else(|| "/var/run/netns/container".to_string()),
            bridge: extract_string_value(json_str, "bridge")
                .unwrap_or_else(|| DEFAULT_BRIDGE_NAME.to_string()),
            ip_address: extract_string_value(json_str, "ipAddress")
                .unwrap_or_else(|| DEFAULT_IP_START.to_string()),
            gateway: extract_string_value(json_str, "gateway")
                .unwrap_or_else(|| "10.244.0.1".to_string()),
            subnet: extract_string_value(json_str, "subnet")
                .unwrap_or_else(|| DEFAULT_SUBNET.to_string()),
            dns_servers,
            mtu: extract_i32_value(json_str, "mtu").unwrap_or(1500),
            args: extract_args_object(json_str),
        })
    }

    /// JSON'a dönüştür
    pub fn to_json(&self) -> String {
        format!(
            r#"{{
  "cniVersion": "{}",
  "name": "{}",
  "containerID": "{}",
  "netns": "{}",
  "bridge": "{}",
  "ipAddress": "{}",
  "gateway": "{}",
  "subnet": "{}",
  "dnsServers": {:?},
  "mtu": {}
}}"#,
            self.cni_version,
            self.container_name,
            self.container_id,
            self.netns,
            self.bridge,
            self.ip_address,
            self.gateway,
            self.subnet,
            self.dns_servers,
            self.mtu
        )
    }
}

// ============================================================================
// CNI SONUCU
// ============================================================================

/// CNI işlem sonucu
#[derive(Clone, Debug)]
pub struct CniResult {
    /// CNI versiyonu
    pub cni_version: String,
    /// Atanan IP adresleri
    pub ips: Vec<CniIpConfig>,
    /// Yönlendirme tablosu
    pub routes: Vec<CniRoute>,
    /// DNS yapılandırması
    pub dns: CniDnsConfig,
    /// Ek arayüzler
    pub interfaces: Vec<CniInterface>,
}

/// CNI IP yapılandırması
#[derive(Clone, Debug)]
pub struct CniIpConfig {
    /// IP adresi
    pub address: String,
    /// Gateway
    pub gateway: String,
    /// Arayüz adı
    pub interface: String,
}

/// CNI yönlendirme yapılandırması
#[derive(Clone, Debug)]
pub struct CniRoute {
    /// Destinasyon
    pub dst: String,
    /// Gateway
    pub gw: String,
}

/// CNI DNS yapılandırması
#[derive(Clone, Debug)]
pub struct CniDnsConfig {
    /// DNS sunucuları
    pub nameservers: Vec<String>,
    /// Arama domain'leri
    pub domain: String,
    /// Arama seçenekleri
    pub options: Vec<String>,
}

/// CNI arayüz yapılandırması
#[derive(Clone, Debug)]
pub struct CniInterface {
    /// Arayüz adı
    pub name: String,
    /// MAC adresi
    pub mac: String,
    /// Sandbox ID
    pub sandbox: String,
}

impl CniResult {
    /// Başarılı sonuç oluştur
    pub fn success(ip_address: &str, gateway: &str) -> Self {
        Self {
            cni_version: CNI_VERSION.to_string(),
            ips: vec![CniIpConfig {
                address: ip_address.to_string(),
                gateway: gateway.to_string(),
                interface: "eth0".to_string(),
            }],
            routes: vec![CniRoute {
                dst: "0.0.0.0/0".to_string(),
                gw: gateway.to_string(),
            }],
            dns: CniDnsConfig {
                nameservers: vec!["8.8.8.8".to_string()],
                domain: "cluster.local".to_string(),
                options: vec!["ndots:1".to_string()],
            },
            interfaces: vec![CniInterface {
                name: "eth0".to_string(),
                mac: "02:42:ac:11:00:02".to_string(),
                sandbox: "container".to_string(),
            }],
        }
    }

    /// JSON'a dönüştür
    pub fn to_json(&self) -> String {
        format!(
            r#"{{
  "cniVersion": "{}",
  "ips": [{{
    "address": "{}",
    "gateway": "{}",
    "interface": "{}"
  }}],
  "routes": [{{
    "dst": "{}",
    "gw": "{}"
  }}],
  "dns": {{
    "nameservers": {:?},
    "domain": "{}",
    "options": {:?}
  }},
  "interfaces": [{{
    "name": "{}",
    "mac": "{}",
    "sandbox": "{}"
  }}]
}}"#,
            self.cni_version,
            self.ips[0].address,
            self.ips[0].gateway,
            self.ips[0].interface,
            self.routes[0].dst,
            self.routes[0].gw,
            self.dns.nameservers,
            self.dns.domain,
            self.dns.options,
            self.interfaces[0].name,
            self.interfaces[0].mac,
            self.interfaces[0].sandbox
        )
    }
}

// ============================================================================
// IP ADRESİ YÖNETİMİ
// ============================================================================

/// IP adresi havuzu
pub struct IpPool {
    /// Subnet
    subnet: String,
    /// Başlangıç IP
    start_ip: Ipv4Addr,
    /// Bitiş IP
    end_ip: Ipv4Addr,
    /// Mevcut IP
    current_ip: AtomicU64,
    /// Kullanılan IP'ler
    used_ips: Mutex<BTreeMap<u32, bool>>,
}

impl IpPool {
    /// Yeni IP havuzu oluştur
    pub fn new(subnet: &str, start_ip: &str, end_ip: &str) -> Result<Self, CniError> {
        let start_addr = parse_ipv4(start_ip).ok_or(CniError::InvalidConfig)?;
        let end_addr = parse_ipv4(end_ip).ok_or(CniError::InvalidConfig)?;

        Ok(Self {
            subnet: subnet.to_string(),
            start_ip: start_addr,
            end_ip: end_addr,
            current_ip: AtomicU64::new(0),
            used_ips: Mutex::new(BTreeMap::new()),
        })
    }

    /// IP adresi tahsis et
    pub fn allocate_ip(&self) -> Result<Ipv4Addr, CniError> {
        let mut used_ips = self.used_ips.lock();

        let start_u32 = u32::from_be_bytes(self.start_ip.0);
        let end_u32 = u32::from_be_bytes(self.end_ip.0);

        for i in 0..=(end_u32 - start_u32) {
            let ip_u32 = start_u32 + i;

            if !used_ips.contains_key(&ip_u32) {
                used_ips.insert(ip_u32, true);
                let ip_bytes = ip_u32.to_be_bytes();
                return Ok(Ipv4Addr(ip_bytes));
            }
        }

        Err(CniError::IpExhausted)
    }

    /// IP adresini serbest bırak
    pub fn release_ip(&self, ip: Ipv4Addr) -> Result<(), CniError> {
        let mut used_ips = self.used_ips.lock();
        let ip_u32 = u32::from_be_bytes(ip.0);

        if used_ips.remove(&ip_u32).is_some() {
            Ok(())
        } else {
            Err(CniError::ContainerNotFound)
        }
    }
}

/// IPv4 adresini ayrıştır
fn parse_ipv4(s: &str) -> Option<Ipv4Addr> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 4 {
        return None;
    }

    let mut bytes = [0u8; 4];
    for (i, part) in parts.iter().enumerate() {
        bytes[i] = part.parse().ok()?;
    }

    Some(Ipv4Addr(bytes))
}

// ============================================================================
// CNI PLUGIN
// ============================================================================

/// CNI plugin arayüzü
pub trait CniPlugin {
    /// Container'a network ekle
    fn add(&self, config: &CniConfig) -> Result<CniResult, CniError>;

    /// Container'ın network'ünü sil
    fn delete(&self, config: &CniConfig) -> Result<(), CniError>;

    /// Container'ın network durumunu kontrol et
    fn check(&self, config: &CniConfig) -> Result<(), CniError>;

    /// Plugin versiyonunu göster
    fn version(&self) -> Result<String, CniError>;
}

/// Bridge CNI plugin
pub struct BridgePlugin {
    /// IP havuzu
    ip_pool: IpPool,
    /// Bridge arayüzleri
    bridges: Mutex<BTreeMap<String, BridgeInterface>>,
    /// Uygulanmış container network kayıtları
    applied: Mutex<BTreeMap<String, AppliedContainerNetwork>>,
    /// Sanal arayüz kayıt defteri
    interfaces: Mutex<BTreeMap<String, VirtualInterface>>,
    /// Network namespace kayıt defteri
    namespaces: Mutex<BTreeMap<String, NetworkNamespace>>,
}

/// Bridge arayüzü
#[derive(Clone, Debug)]
struct BridgeInterface {
    /// Bridge adı
    name: String,
    /// IP adresi
    ip: Ipv4Addr,
    /// Subnet
    subnet: String,
    /// Aktif mi
    active: bool,
}

#[derive(Clone, Debug)]
struct AppliedContainerNetwork {
    container_id: String,
    bridge: String,
    host_veth: String,
    container_interface: String,
    netns: String,
    assigned_ip: Ipv4Addr,
    gateway: String,
    mtu: i32,
    active: bool,
}

#[derive(Clone, Debug)]
struct VirtualInterface {
    name: String,
    peer: Option<String>,
    bridge: Option<String>,
    netns: Option<String>,
    ip: Option<Ipv4Addr>,
    gateway: Option<Ipv4Addr>,
    mtu: i32,
    up: bool,
}

#[derive(Clone, Debug, Default)]
struct NetworkNamespace {
    path: String,
    interfaces: Vec<String>,
}

impl BridgePlugin {
    /// Yeni bridge plugin oluştur
    pub fn new() -> Result<Self, CniError> {
        let ip_pool = IpPool::new(DEFAULT_SUBNET, DEFAULT_IP_START, DEFAULT_IP_END)?;

        Ok(Self {
            ip_pool,
            bridges: Mutex::new(BTreeMap::new()),
            applied: Mutex::new(BTreeMap::new()),
            interfaces: Mutex::new(BTreeMap::new()),
            namespaces: Mutex::new(BTreeMap::new()),
        })
    }

    /// Bridge oluştur
    fn create_bridge(&self, name: &str, subnet: &str) -> Result<(), CniError> {
        let mut bridges = self.bridges.lock();

        let bridge_ip = parse_ipv4("10.244.0.1").ok_or(CniError::InvalidConfig)?;

        let bridge = BridgeInterface {
            name: name.to_string(),
            ip: bridge_ip,
            subnet: subnet.to_string(),
            active: true,
        };

        bridges.insert(name.to_string(), bridge);
        self.interfaces.lock().insert(
            name.to_string(),
            VirtualInterface {
                name: name.to_string(),
                peer: None,
                bridge: Some(name.to_string()),
                netns: None,
                ip: Some(bridge_ip),
                gateway: None,
                mtu: 1500,
                up: true,
            },
        );

        crate::serial_println!("[CNI] Created bridge {} with IP {}", name, "10.244.0.1");
        Ok(())
    }

    /// veth pair oluştur
    fn create_veth_pair(
        &self,
        container_id: &str,
        bridge_name: &str,
    ) -> Result<(String, String), CniError> {
        if container_id.len() < 8 {
            return Err(CniError::InvalidConfig);
        }
        let veth_name = format!("veth{}", &container_id[..8]);
        let peer_name = format!("eth0");

        {
            let mut interfaces = self.interfaces.lock();
            interfaces.insert(
                veth_name.clone(),
                VirtualInterface {
                    name: veth_name.clone(),
                    peer: Some(peer_name.clone()),
                    bridge: Some(bridge_name.to_string()),
                    netns: None,
                    ip: None,
                    gateway: None,
                    mtu: 1500,
                    up: true,
                },
            );
            interfaces.insert(
                peer_name.clone(),
                VirtualInterface {
                    name: peer_name.clone(),
                    peer: Some(veth_name.clone()),
                    bridge: Some(bridge_name.to_string()),
                    netns: None,
                    ip: None,
                    gateway: None,
                    mtu: 1500,
                    up: false,
                },
            );
        }

        crate::serial_println!("[CNI] Created veth pair: {} <-> {}", veth_name, peer_name);

        // veth'i bridge'e bağla
        crate::serial_println!("[CNI] Attached {} to bridge {}", veth_name, bridge_name);

        Ok((veth_name, peer_name))
    }

    /// Arayüzü network namespace'e taşı
    fn move_to_netns(&self, interface: &str, netns: &str) -> Result<(), CniError> {
        if interface.is_empty() || netns.is_empty() {
            return Err(CniError::InvalidConfig);
        }
        {
            let mut interfaces = self.interfaces.lock();
            let iface = interfaces
                .get_mut(interface)
                .ok_or(CniError::ContainerNotFound)?;
            iface.netns = Some(netns.to_string());
        }
        {
            let mut namespaces = self.namespaces.lock();
            let entry = namespaces
                .entry(netns.to_string())
                .or_insert_with(|| NetworkNamespace {
                    path: netns.to_string(),
                    interfaces: Vec::new(),
                });
            if !entry.interfaces.iter().any(|name| name == interface) {
                entry.interfaces.push(interface.to_string());
            }
        }
        crate::serial_println!("[CNI] Moved {} to netns {}", interface, netns);
        Ok(())
    }

    /// Arayüzü yapılandır
    fn configure_interface(
        &self,
        interface: &str,
        ip: &str,
        gateway: &str,
    ) -> Result<(), CniError> {
        if interface.is_empty() || parse_ipv4(ip).is_none() || parse_ipv4(gateway).is_none() {
            return Err(CniError::InvalidConfig);
        }
        let parsed_ip = parse_ipv4(ip).ok_or(CniError::InvalidConfig)?;
        let parsed_gateway = parse_ipv4(gateway).ok_or(CniError::InvalidConfig)?;
        let mut interfaces = self.interfaces.lock();
        let iface = interfaces
            .get_mut(interface)
            .ok_or(CniError::ContainerNotFound)?;
        iface.ip = Some(parsed_ip);
        iface.gateway = Some(parsed_gateway);
        iface.mtu = 1500;
        iface.up = true;
        crate::serial_println!(
            "[CNI] Configured {} with IP {} and gateway {}",
            interface,
            ip,
            gateway
        );
        Ok(())
    }

    fn rollback_add(
        &self,
        container_id: &str,
        ip: Option<Ipv4Addr>,
        host_veth: Option<&str>,
        container_interface: Option<&str>,
        netns: Option<&str>,
    ) {
        if let Some(ip) = ip {
            let _ = self.ip_pool.release_ip(ip);
        }
        if let Some(host_veth) = host_veth {
            crate::serial_println!("[CNI] Rolled back interface {}", host_veth);
            self.interfaces.lock().remove(host_veth);
        }
        if let Some(container_interface) = container_interface {
            self.interfaces.lock().remove(container_interface);
        }
        if let Some(netns) = netns {
            if let Some(namespace) = self.namespaces.lock().get_mut(netns) {
                if let Some(container_interface) = container_interface {
                    namespace
                        .interfaces
                        .retain(|iface| iface != container_interface);
                }
            }
        }
        self.applied.lock().remove(container_id);
    }
}

impl CniPlugin for BridgePlugin {
    fn add(&self, config: &CniConfig) -> Result<CniResult, CniError> {
        crate::serial_println!("[CNI] ADD container: {}", config.container_id);

        // Bridge'i kontrol et
        if !self.bridges.lock().contains_key(&config.bridge) {
            self.create_bridge(&config.bridge, &config.subnet)?;
        }

        // IP adresi tahsis et
        let ip = self.ip_pool.allocate_ip()?;
        let ip_str = format!("{}.{}.{}.{}", ip.0[0], ip.0[1], ip.0[2], ip.0[3]);

        // veth pair oluştur
        let (host_veth, container_interface) =
            match self.create_veth_pair(&config.container_id, &config.bridge) {
                Ok(veth) => veth,
                Err(err) => {
                    self.rollback_add(&config.container_id, Some(ip), None, None, None);
                    return Err(err);
                }
            };

        // Arayüzü namespace'e taşı
        if let Err(err) = self.move_to_netns(&container_interface, &config.netns) {
            self.rollback_add(
                &config.container_id,
                Some(ip),
                Some(&host_veth),
                Some(&container_interface),
                Some(&config.netns),
            );
            return Err(err);
        }

        // Arayüzü yapılandır
        if let Err(err) = self.configure_interface(&container_interface, &ip_str, &config.gateway) {
            self.rollback_add(
                &config.container_id,
                Some(ip),
                Some(&host_veth),
                Some(&container_interface),
                Some(&config.netns),
            );
            return Err(err);
        }

        self.applied.lock().insert(
            config.container_id.clone(),
            AppliedContainerNetwork {
                container_id: config.container_id.clone(),
                bridge: config.bridge.clone(),
                host_veth: host_veth.clone(),
                container_interface: container_interface.clone(),
                netns: config.netns.clone(),
                assigned_ip: ip,
                gateway: config.gateway.clone(),
                mtu: config.mtu,
                active: true,
            },
        );

        crate::serial_println!(
            "[CNI] Assigned IP {} to container {}",
            ip_str,
            config.container_id
        );

        Ok(CniResult::success(&ip_str, &config.gateway))
    }

    fn delete(&self, config: &CniConfig) -> Result<(), CniError> {
        crate::serial_println!("[CNI] DEL container: {}", config.container_id);

        let applied = self
            .applied
            .lock()
            .remove(&config.container_id)
            .ok_or(CniError::ContainerNotFound)?;
        self.ip_pool.release_ip(applied.assigned_ip)?;
        self.interfaces.lock().remove(&applied.host_veth);
        self.interfaces.lock().remove(&applied.container_interface);
        if let Some(namespace) = self.namespaces.lock().get_mut(&applied.netns) {
            namespace
                .interfaces
                .retain(|iface| iface != &applied.container_interface);
        }

        // veth arayüzünü sil
        crate::serial_println!("[CNI] Deleted interface {}", applied.host_veth);

        Ok(())
    }

    fn check(&self, config: &CniConfig) -> Result<(), CniError> {
        crate::serial_println!("[CNI] CHECK container: {}", config.container_id);

        let applied = self
            .applied
            .lock()
            .get(&config.container_id)
            .cloned()
            .ok_or(CniError::ContainerNotFound)?;

        if !applied.active
            || applied.netns != config.netns
            || applied.bridge != config.bridge
            || applied.gateway != config.gateway
            || applied.mtu != config.mtu
            || applied.container_interface != "eth0"
        {
            return Err(CniError::ContainerNotFound);
        }

        let bridges = self.bridges.lock();
        let bridge = bridges
            .get(&applied.bridge)
            .ok_or(CniError::NetworkNotFound)?;
        if !bridge.active || bridge.subnet != config.subnet {
            return Err(CniError::NetworkNotFound);
        }

        let interfaces = self.interfaces.lock();
        let host_veth = interfaces
            .get(&applied.host_veth)
            .ok_or(CniError::ContainerNotFound)?;
        let container_iface = interfaces
            .get(&applied.container_interface)
            .ok_or(CniError::ContainerNotFound)?;
        if host_veth.bridge.as_deref() != Some(applied.bridge.as_str())
            || host_veth.peer.as_deref() != Some(applied.container_interface.as_str())
            || !host_veth.up
            || container_iface.netns.as_deref() != Some(applied.netns.as_str())
            || container_iface.ip != Some(applied.assigned_ip)
            || container_iface.gateway != parse_ipv4(&config.gateway)
            || !container_iface.up
        {
            return Err(CniError::ContainerNotFound);
        }

        let namespaces = self.namespaces.lock();
        let namespace = namespaces
            .get(&applied.netns)
            .ok_or(CniError::ContainerNotFound)?;
        if namespace.path != applied.netns
            || !namespace
                .interfaces
                .iter()
                .any(|iface| iface == &applied.container_interface)
        {
            return Err(CniError::ContainerNotFound);
        }

        let used_ips = self.ip_pool.used_ips.lock();
        let ip_u32 = u32::from_be_bytes(applied.assigned_ip.0);
        if used_ips.contains_key(&ip_u32) {
            Ok(())
        } else {
            Err(CniError::ContainerNotFound)
        }
    }

    fn version(&self) -> Result<String, CniError> {
        Ok(format!(
            r#"{{
  "cniVersion": "{}",
  "supportedVersions": ["0.1.0", "0.2.0", "0.3.0", "0.4.0"]
}}"#,
            CNI_VERSION
        ))
    }
}

// ============================================================================
// CNI YÖNETİCİSİ
// ============================================================================

/// CNI yöneticisi
pub struct CniManager {
    /// Plugin'ler
    plugins: BTreeMap<String, Box<dyn CniPlugin>>,
    /// Varsayılan plugin
    default_plugin: String,
}

impl CniManager {
    /// Yeni CNI yöneticisi oluştur
    pub fn new() -> Self {
        let mut manager = Self {
            plugins: BTreeMap::new(),
            default_plugin: "bridge".to_string(),
        };

        // Bridge plugin'i ekle
        if let Ok(bridge_plugin) = BridgePlugin::new() {
            manager
                .plugins
                .insert("bridge".to_string(), Box::new(bridge_plugin));
        }

        manager
    }

    /// Plugin ekle
    pub fn add_plugin(&mut self, name: &str, plugin: Box<dyn CniPlugin>) {
        self.plugins.insert(name.to_string(), plugin);
    }

    /// Varsayılan plugin'i ayarla
    pub fn set_default_plugin(&mut self, name: &str) {
        self.default_plugin = name.to_string();
    }

    fn resolve_plugin(&self, config: &CniConfig) -> Result<&dyn CniPlugin, CniError> {
        self.plugins
            .get(&config.bridge)
            .or_else(|| self.plugins.get(&self.default_plugin))
            .map(|plugin| plugin.as_ref())
            .ok_or(CniError::NetworkNotFound)
    }

    /// CNI komutu çalıştır
    pub fn run_command(&self, command: &str, config: &CniConfig) -> Result<CniResult, CniError> {
        let plugin = self.resolve_plugin(config)?;

        match command {
            CNI_COMMAND_ADD => plugin.add(config),
            CNI_COMMAND_DEL | CNI_COMMAND_CHECK | CNI_COMMAND_VERSION => {
                Err(CniError::InvalidCommand)
            }
            _ => Err(CniError::InvalidCommand),
        }
    }

    pub fn run_delete(&self, config: &CniConfig) -> Result<(), CniError> {
        self.resolve_plugin(config)?.delete(config)
    }

    pub fn run_check(&self, config: &CniConfig) -> Result<(), CniError> {
        self.resolve_plugin(config)?.check(config)
    }

    pub fn run_version(&self, config: &CniConfig) -> Result<String, CniError> {
        self.resolve_plugin(config)?.version()
    }
}

impl Default for CniManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// MODÜL BAŞLATMA
// ============================================================================

/// CNI modülünü başlat
pub fn init() {
    crate::serial_println!("[CNI] CNI module initialized");
}

/// CNI komutunu çalıştır (standalone)
pub fn run_cni_command(command: &str, config_json: &str) -> Result<String, CniError> {
    let config = CniConfig::from_json(config_json)?;
    let manager = CniManager::new();

    match command {
        CNI_COMMAND_ADD => {
            let result = manager.run_command(command, &config)?;
            Ok(result.to_json())
        }
        CNI_COMMAND_DEL => {
            manager.run_delete(&config)?;
            Ok(r#"{"code": 0}"#.to_string())
        }
        CNI_COMMAND_CHECK => {
            manager.run_check(&config)?;
            Ok(r#"{"code": 0}"#.to_string())
        }
        CNI_COMMAND_VERSION => {
            let version = manager.run_version(&config)?;
            Ok(format!("{{\"code\": 0, \"result\": {}}}", version))
        }
        _ => Err(CniError::InvalidCommand),
    }
}

/// Test CNI işlemi
pub fn test_cni_add() -> Result<String, CniError> {
    let config = CniConfig {
        cni_version: CNI_VERSION.to_string(),
        container_name: "test-container".to_string(),
        container_id: "123456789".to_string(),
        netns: "/var/run/netns/test".to_string(),
        bridge: DEFAULT_BRIDGE_NAME.to_string(),
        ip_address: "".to_string(), // Otomatik atanacak
        gateway: "10.244.0.1".to_string(),
        subnet: DEFAULT_SUBNET.to_string(),
        dns_servers: vec!["8.8.8.8".to_string()],
        mtu: 1500,
        args: BTreeMap::new(),
    };

    let manager = CniManager::new();
    let result = manager.run_command(CNI_COMMAND_ADD, &config)?;

    Ok(result.to_json())
}

impl core::fmt::Display for CniResult {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.to_json())
    }
}
