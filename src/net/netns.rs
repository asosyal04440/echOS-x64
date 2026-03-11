//! # Ağ Ad Alanları (Network Namespaces)
//!
//! Konteyner tabanlı ağ yalıtımı: her ad alanı kendi bağımsız ağ yığınını
//! (arayüzler, IP adresleri, rota tabloları, iptables kuralları) tutar.
//!
//! ## Ağ Ad Alanı Nedir?
//!
//! Linux çekirdeğinde "namespace" (ad alanı), belirli çekirdek kaynaklarının
//! süreç grupları arasında yalıtılmasını sağlar. Ağ ad alanları şu kaynakları
//! yalıtır:
//!
//! - Ağ arayüzleri (eth0, lo, veth vb.)
//! - IP adresleri ve rota tabloları
//! - ARP/NDP tabloları
//! - iptables/nftables kuralları
//! - Soketler ve port numaraları
//!
//! ## Mimari Diyagramı
//!
//! ```
//!  ┌─────────────────────────────────────────────────────────────┐
//!  │                    Linux Çekirdeği                          │
//!  │                                                             │
//!  │  ┌─────────────────┐    ┌─────────────────┐                │
//!  │  │  init (kök) ns  │    │   container ns  │                │
//!  │  │                 │    │                 │                 │
//!  │  │  eth0: 192.168.1│    │  eth0: 10.0.0.1 │                │
//!  │  │  lo:   127.0.0.1│    │  lo:   127.0.0.1│                │
//!  │  │  route tablosu  │    │  route tablosu  │                 │
//!  │  │  iptables       │    │  iptables       │                 │
//!  │  │  (PID 1 vb.)    │    │  (konteyner proc│                │
//!  │  └─────────────────┘    └─────────────────┘                │
//!  │                                                             │
//!  │  Bağlantı: veth çifti (sanal ethernet tüneli)              │
//!  │  host-veth <──────────────> container-veth                 │
//!  └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Ad Alanı Oluşturma
//!
//! ```
//!  1. unshare(CLONE_NEWNET)  - Mevcut süreç yeni ns'e taşınır
//!  2. clone(CLONE_NEWNET)    - Alt süreç yeni ns'te başlatılır
//!  3. ip netns add <name>    - Adlandırılmış ns oluşturur (/var/run/netns/)
//! ```
//!
//! ## veth Çifti ile Bağlantı
//!
//! ```
//!  ip link add veth0 type veth peer name veth1
//!  ip link set veth1 netns <container_ns>
//!  -> veth0 kök ns'te, veth1 konteyner ns'te görünür
//!  -> Paketler veth çifti üzerinden iki ns arasında akar
//! ```

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// AĞ AD ALANI (NetNamespace)
// ============================================================================
//
// Her konteyner veya yalıtılmış süreç grubu için ayrı bir NetNamespace
// örneği tutulur. Sistem açılışında "init" adıyla kök ad alanı (id=0)
// oluşturulur; tüm normal süreçler bu kök ad alanında çalışır.

/// Tek bir ağ ad alanı. Her ad alanı kendi yalıtılmış ağ yığınını içerir.
pub struct NetNamespace {
    /// Ad alanı için benzersiz sayısal kimlik. Kök ad alanı her zaman 0'dır.
    pub id: u64,
    /// İnsan-okunabilir ad alanı adı (örn. "init", "container-web").
    pub name: String,
    /// Bu ad alanına ait ağ arayüzleri: ad -> NetDevice.
    /// Kilit ile eş zamanlı erişime karşı korunur.
    pub devices: Mutex<BTreeMap<String, Arc<NetDevice>>>,
    /// Geri döngü (loopback) arayüzü: 127.0.0.1 adresine bağlı özel cihaz.
    /// Her ad alanının kendi `lo` arayüzü vardır.
    pub loopback: Option<Arc<NetDevice>>,
    /// Bu ad alanına atanmış IP adresleri listesi.
    pub addresses: Mutex<Vec<IpAddress>>,
    /// Bu ad alanının rota tablosu: En uzun önek eşleşmesi (LPM) ile aranır.
    pub routes: Mutex<Vec<Route>>,
    /// Bu ad alanına özgü Netfilter (iptables) kuralları.
    pub iptables: crate::net::netfilter::NetfilterManager,
    /// Ad alanı hâlâ kullanımda mı?
    pub active: AtomicBool,
    /// Bu ad alanında kaç süreç çalışıyor (0 olunca güvenle silinebilir).
    pub process_count: AtomicU32,
}

/// Ağ arayüzü (NIC): fiziksel veya sanal bir ağ kartını temsil eder.
/// Örnekler: eth0 (fiziksel), lo (loopback), veth0 (sanal/tünel), docker0 (köprü).
#[derive(Clone, Debug)]
pub struct NetDevice {
    /// Arayüz adı (örn. "eth0", "lo", "veth0").
    pub name: String,
    /// Çekirdek arayüz indeksi (ifindex); `ip link show` çıktısındaki sayı.
    pub ifindex: u32,
    /// Maksimum Aktarım Birimi: tek seferde gönderilebilecek en büyük frame (byte).
    /// Ethernet için tipik değer 1500; jumbo frame için 9000.
    pub mtu: u32,
    /// MAC adresi: 6-byte donanım adresi (ARP katmanı için gerekli).
    pub mac: [u8; 6],
    /// Arayüz bayrakları: IFF_UP, IFF_RUNNING, IFF_LOOPBACK vb.
    pub flags: AtomicU32,
    /// Toplam gönderilen byte sayısı (istatistik).
    pub tx_bytes: AtomicU64,
    /// Toplam alınan byte sayısı (istatistik).
    pub rx_bytes: AtomicU64,
    /// Toplam gönderilen paket sayısı.
    pub tx_packets: AtomicU64,
    /// Toplam alınan paket sayısı.
    pub rx_packets: AtomicU64,
}

impl NetDevice {
    /// Belirtilen ad ve ifindex ile yeni bir ağ arayüzü oluşturur.
    /// MTU varsayılan olarak 1500 (standart Ethernet), MAC sıfırlanmış.
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

/// Bir ağ arayüzüne atanmış IPv4 adresi ve ağ maskesi.
/// Örnek: 192.168.1.10/24 için addr=0xC0A8010A, prefix_len=24.
#[derive(Clone, Debug)]
pub struct IpAddress {
    /// IPv4 adresi (big-endian 32-bit tamsayı olarak).
    pub addr: u32,
    /// CIDR önek uzunluğu (0-32). Ağ maskesini belirler.
    /// /24 -> 255.255.255.0, /16 -> 255.255.0.0
    pub prefix_len: u8,
    /// Bu adresin bağlı olduğu arayüzün ifindex'i.
    pub ifindex: u32,
    /// Kapsam: 0=global, 253=bağlantı-yerel, 254=host, 255=yok.
    pub scope: u8,
}

/// Rota tablosu girişi. En Uzun Önek Eşleşmesi (LPM) ile seçilir.
///
/// Arama: `(dst & mask) == (rota.dst & mask)` eşleşmelerinden
/// en uzun dst_len'e sahip rota seçilir (daha özgül → önce gelir).
#[derive(Clone, Debug)]
pub struct Route {
    /// Hedef ağ adresi (big-endian).
    pub dst: u32,
    /// Hedef ağın CIDR önek uzunluğu. 0 = varsayılan rota (0.0.0.0/0).
    pub dst_len: u8,
    /// Sonraki atlama (next-hop) IP adresi. 0 ise doğrudan bağlı.
    pub gateway: u32,
    /// Çıkış arayüzünün ifindex'i.
    pub ifindex: u32,
    /// Rota metriği: eşit önekli rotalar arasında düşük metrik tercih edilir.
    pub metric: u32,
}

impl NetNamespace {
    /// Belirtilen ID ve isimle yeni bir ağ ad alanı oluşturur.
    /// Netfilter yöneticisi boş kurallarla başlatılır.
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

    /// Bu ad alanına bir ağ arayüzü ekler.
    /// Cihaz adına göre haritaya yerleştirilir.
    pub fn add_device(&self, device: Arc<NetDevice>) {
        self.devices.lock().insert(device.name.clone(), device);
    }

    /// Bu ad alanından bir ağ arayüzünü kaldırır ve döndürür.
    pub fn remove_device(&self, name: &str) -> Option<Arc<NetDevice>> {
        self.devices.lock().remove(name)
    }

    /// Ad ile arayüzü arar. Docker/Podman gibi araçlar konteyner içinde
    /// `eth0` aramak için bu fonksiyonu kullanır.
    pub fn get_device(&self, name: &str) -> Option<Arc<NetDevice>> {
        self.devices.lock().get(name).cloned()
    }

    /// Bu ad alanına IP adresi atar (örn. `ip addr add 10.0.0.1/24 dev eth0`).
    pub fn add_address(&self, addr: IpAddress) {
        self.addresses.lock().push(addr);
    }

    /// Bu ad alanının rota tablosuna yeni bir rota ekler.
    pub fn add_route(&self, route: Route) {
        self.routes.lock().push(route);
    }

    /// Hedef IP adresi için en uygun rotayı bulur (En Uzun Önek Eşleşmesi).
    ///
    /// Algoritma:
    /// 1. Tüm rota girişlerini tara
    /// 2. Her rota için: mask = !0 << (32 - dst_len)
    /// 3. (dst & mask) == (rota.dst & mask) ise eşleşme var
    /// 4. Eşleşenler arasından en uzun dst_len'e sahip rotayı seç
    pub fn lookup_route(&self, dst: u32) -> Option<Route> {
        let routes = self.routes.lock();
        let mut best: Option<&Route> = None;
        let mut best_len = 0u8;

        for route in routes.iter() {
            // dst_len=0 ise varsayılan rota -> maske 0 (her adresle eşleşir)
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

    /// Bu ad alanında yeni bir süreç başladığında çağrılır.
    pub fn add_process(&self) {
        self.process_count.fetch_add(1, Ordering::SeqCst);
    }

    /// Bu ad alanından bir süreç çıkış yaptığında çağrılır.
    /// Sayaç 0 olursa ad alanı güvenle silinebilir.
    pub fn remove_process(&self) {
        self.process_count.fetch_sub(1, Ordering::SeqCst);
    }
}

// ============================================================================
// AD ALANI YÖNETİCİSİ (NetNamespaceManager)
// ============================================================================
//
// Sistemdeki tüm ağ ad alanlarını merkezi olarak yönetir.
// Docker/Podman gibi konteyner çalışma ortamları bu yapıyı kullanır.
//
// Veri yapıları:
//   namespaces : id -> Arc<NetNamespace>   (tüm ad alanlarının haritası)
//   current_ns : aktif ad alanının ID'si   (süreç için geçerli ns)
//   next_id    : sonraki ns için ID üreteci

/// Tüm ağ ad alanlarını yöneten merkezi yönetici.
pub struct NetNamespaceManager {
    /// ID -> ad alanı haritası; kilit ile korunur.
    namespaces: Mutex<BTreeMap<u64, Arc<NetNamespace>>>,
    /// Şu anda aktif olan ad alanının ID'si.
    current_ns: Mutex<u64>,
    /// Monoton artan yeni ad alanı ID üreteci.
    next_id: AtomicU64,
}

impl NetNamespaceManager {
    /// Derleme-zamanı sabit başlatıcı (static değişken için gerekli).
    pub const fn new() -> Self {
        Self {
            namespaces: Mutex::new(BTreeMap::new()),
            current_ns: Mutex::new(0),
            next_id: AtomicU64::new(1),
        }
    }

    /// Kök (init) ağ ad alanını oluşturur.
    /// Her ad alanı otomatik olarak bir `lo` arayüzü ile başlatılır.
    pub fn init(&self) {
        let root = Arc::new(NetNamespace::new(0, "init"));

        // Loopback arayüzü: 127.0.0.1 paketeleri göndermek için gerekli
        let lo = Arc::new(NetDevice::new("lo", 1));
        root.add_device(lo.clone());

        self.namespaces.lock().insert(0, root);

        crate::serial_println!("[NETNS] Initialized root network namespace");
    }

    /// Yeni bir ağ ad alanı oluşturur.
    /// Loopback arayüzü otomatik eklenir (her ns kendi 127.0.0.1'ine sahip).
    pub fn create(&self, name: &str) -> Arc<NetNamespace> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let ns = Arc::new(NetNamespace::new(id, name));

        // Her yeni ad alanı kendi loopback arayüzünü alır
        let lo = Arc::new(NetDevice::new("lo", 1));
        ns.add_device(lo.clone());

        self.namespaces.lock().insert(id, ns.clone());

        crate::serial_println!("[NETNS] Created namespace '{}' (id={})", name, id);
        ns
    }

    /// Ad alanını siler. Kök ad alanı (id=0) silinemez.
    pub fn delete(&self, id: u64) -> bool {
        if id == 0 {
            return false; // Kök ad alanı asla silinemez
        }

        self.namespaces.lock().remove(&id).is_some()
    }

    /// Verilen ID ile ad alanını döndürür.
    pub fn get(&self, id: u64) -> Option<Arc<NetNamespace>> {
        self.namespaces.lock().get(&id).cloned()
    }

    /// Şu anda aktif olan (geçerli süreç için) ağ ad alanını döndürür.
    pub fn current(&self) -> Arc<NetNamespace> {
        let id = *self.current_ns.lock();
        self.get(id).unwrap()
    }

    /// Aktif ağ ad alanını değiştirir.
    /// `setns(fd, CLONE_NEWNET)` sistem çağrısının karşılığıdır.
    /// Başarıda true, ad alanı bulunamazsa false döner.
    pub fn set_current(&self, id: u64) -> bool {
        if self.namespaces.lock().contains_key(&id) {
            *self.current_ns.lock() = id;
            true
        } else {
            false
        }
    }

    /// Bir ağ arayüzünü bir ad alanından diğerine taşır.
    /// Docker bunu `ip link set eth0 netns <container>` ile gerçekleştirir.
    /// veth çifti oluşturulurken bir uç konteyner ns'e bu yolla taşınır.
    pub fn move_device(&self, from_ns: u64, to_ns: u64, dev_name: &str) -> bool {
        let from = match self.get(from_ns) {
            Some(ns) => ns,
            None => return false,
        };

        let to = match self.get(to_ns) {
            Some(ns) => ns,
            None => return false,
        };

        // Kaynak ns'den çıkar, hedef ns'e ekle
        if let Some(dev) = from.remove_device(dev_name) {
            to.add_device(dev);
            return true;
        }

        false
    }
}

lazy_static::lazy_static! {
    /// Sistemin global ağ ad alanı yöneticisi.
    pub static ref NETNS_MANAGER: NetNamespaceManager = NetNamespaceManager::new();
}

// ============================================================================
// SİSTEM ÇAĞRISI ARABIRIMI (Syscall Interface)
// ============================================================================
//
// POSIX/Linux uyumlu sistem çağrısı köprüleri:
//
//   unshare(CLONE_NEWNET)          -> sys_unshare_newnet()
//   setns(fd, CLONE_NEWNET)        -> sys_setns_net()

/// `unshare(CLONE_NEWNET)` sistem çağrısı:
/// Çağıran süreç için yeni, yalıtılmış bir ağ ad alanı oluşturur.
/// Süreç artık kendi arayüzlerine ve rota tablosuna sahip olur.
pub fn sys_unshare_newnet() -> i32 {
    let ns = NETNS_MANAGER.create("unshared");
    ns.add_process();
    0 // Başarı
}

/// `setns(fd, CLONE_NEWNET)` sistem çağrısı:
/// Çağıran süreci mevcut bir ağ ad alanına taşır.
/// Başarıda 0, geçersiz ns_id için -22 (EINVAL) döner.
pub fn sys_setns_net(ns_id: u64) -> i32 {
    if NETNS_MANAGER.set_current(ns_id) {
        0
    } else {
        -22 // EINVAL
    }
}

// ============================================================================
// BAŞLATMA (Initialization)
// ============================================================================

/// Ağ ad alanı alt sistemini başlatır: kök "init" ad alanını oluşturur.
pub fn init() {
    NETNS_MANAGER.init();
}

// ============================================================================
// VETH (Virtual Ethernet Pair) DESTEĞİ
// ============================================================================

/// Veth pair oluşturur — iki sanal ağ arayüzü birbirine bağlanır
///
/// Container networking temel taşı:
/// - veth0 → host namespace'de kalır
/// - veth1 → container namespace'e taşınır
///
/// ```text
/// Host NS:  veth0 ◄──────► veth1 :Container NS
/// ```
pub fn create_veth_pair(
    ns1_id: u64,
    name1: &str,
    ns2_id: u64,
    name2: &str,
) -> Result<(), &'static str> {
    let ns1 = NETNS_MANAGER.get(ns1_id).ok_or("NS1 not found")?;
    let ns2 = NETNS_MANAGER.get(ns2_id).ok_or("NS2 not found")?;

    let ifindex1 = alloc_ifindex();
    let ifindex2 = alloc_ifindex();

    let dev1 = Arc::new(NetDevice::new(name1, ifindex1));
    let dev2 = Arc::new(NetDevice::new(name2, ifindex2));

    ns1.add_device(dev1);
    ns2.add_device(dev2);

    crate::serial_println!(
        "[NetNS] veth pair created: {}(ns{}) <-> {}(ns{})",
        name1, ns1_id, name2, ns2_id
    );

    Ok(())
}

/// Interface index sayacı
static NEXT_IFINDEX: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(100);

fn alloc_ifindex() -> u32 {
    NEXT_IFINDEX.fetch_add(1, core::sync::atomic::Ordering::Relaxed)
}

/// Namespace'deki tüm interface'leri listeler
pub fn list_interfaces(ns_id: u64) -> Vec<String> {
    if let Some(ns) = NETNS_MANAGER.get(ns_id) {
        ns.devices.lock().keys().cloned().collect()
    } else {
        Vec::new()
    }
}

/// Namespace sayısını döner
pub fn namespace_count() -> usize {
    NETNS_MANAGER.namespaces.lock().len()
}
