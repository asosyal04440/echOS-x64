//! # Netfilter / iptables — Paket Filtreleme ve NAT
//!
//! Bu modül, Linux'un netfilter/iptables altyapısını echOS'a taşır.
//! Ağ trafiği, beş farklı kanca noktasından (hook) geçirilir ve
//! tablolarda tanımlı kurallara göre işlenir.
//!
//! ## Temel Kavramlar
//!
//! ```text
//!  Gelen Paket
//!       |
//!  [PRE_ROUTING]  ← DNAT burada uygulanır (hedef IP değişir)
//!       |
//!   Bu makineye mi?
//!   /             \
//! Evet            Hayır
//!   |               |
//! [LOCAL_IN]   [FORWARD]   ← Ağ geçidi ise iletme kararı
//!   |               |
//! Uygulama    [POST_ROUTING] ← SNAT/MASQUERADE burada
//!   |
//! [LOCAL_OUT] ← Giden paket
//!   |
//! [POST_ROUTING]
//! ```
//!
//! ## Tablo Hiyerarşisi
//!
//! - **filter** : Paket kabul/ret kararları (INPUT, FORWARD, OUTPUT zincirleri)
//! - **nat**    : Adres çevirisi PREROUTING ve POSTROUTING zincirleriyle yapılır
//! - **mangle** : Paket başlığı değişikliği (TTL, TOS vb.) — şu an stub
//! - **raw**    : Bağlantı takibinden (conntrack) muaf tutma
//! - **security**: SELinux/AppArmor kararları

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// NETFİLTER SABİTLERİ (NETFILTER CONSTANTS)
// ============================================================================
//
// Bu sabitler, Linux çekirdeğinin `include/uapi/linux/netfilter.h` başlık
// dosyasındaki değerlerle birebir uyumludur. Böylece kullanıcı alanı araçları
// (iptables, nftables) echOS'a doğrudan sistem çağrısıyla kural ekleyebilir.

/// Netfilter kanca noktaları (hook points)
///
/// Bir paket hayatı boyunca bu kancalardan geçer.
/// Sırası önemlidir: PRE_ROUTING (0) → LOCAL_IN (1) → FORWARD (2)
///                   → LOCAL_OUT (3) → POST_ROUTING (4)
pub const NF_INET_PRE_ROUTING: u32 = 0;
pub const NF_INET_LOCAL_IN: u32 = 1;
pub const NF_INET_FORWARD: u32 = 2;
pub const NF_INET_LOCAL_OUT: u32 = 3;
pub const NF_INET_POST_ROUTING: u32 = 4;

/// Netfilter karar kodları (verdicts)
///
/// - NF_DROP   (0): Paket sessizce iptal edilir; gönderene hata bildirilmez.
/// - NF_ACCEPT (1): Paket bir sonraki kancaya veya hedefe iletilir.
/// - NF_STOLEN (2): Modül paketi aldı; artık çekirdeğe ait değil.
/// - NF_QUEUE  (3): Paket kullanıcı alanına gönderilir (NFQUEUE).
/// - NF_REPEAT (4): Kanca fonksiyonu tekrar çağrılır.
/// - NF_STOP   (5): Zincir işlemi durdurulur, paket kabul edilir.
pub const NF_DROP: u32 = 0;
pub const NF_ACCEPT: u32 = 1;
pub const NF_STOLEN: u32 = 2;
pub const NF_QUEUE: u32 = 3;
pub const NF_REPEAT: u32 = 4;
pub const NF_STOP: u32 = 5;

/// Protokol aileleri (protocol families)
///
/// Linux'ta her socket oluşturulurken AF_INET (2) veya AF_INET6 (10) seçilir.
/// Netfilter bu değere göre doğru kuralları uygular.
pub const NFPROTO_UNSPEC: u32 = 0;
pub const NFPROTO_IPV4: u32 = 2;
pub const NFPROTO_IPV6: u32 = 10;

/// iptables hedef (target) adları
///
/// Bir kural eşleştiğinde hangi eylemin gerçekleşeceğini belirtir.
/// MASQUERADE, SNAT, DNAT NAT eylemleridir; LOG iz kaydı için kullanılır.
pub const IPT_STANDARD_TARGET: &str = "";
pub const IPT_ACCEPT_TARGET: &str = "ACCEPT";
pub const IPT_DROP_TARGET: &str = "DROP";
pub const IPT_RETURN_TARGET: &str = "RETURN";
pub const IPT_QUEUE_TARGET: &str = "QUEUE";
pub const IPT_REJECT_TARGET: &str = "REJECT";
pub const IPT_LOG_TARGET: &str = "LOG";
pub const IPT_MASQUERADE_TARGET: &str = "MASQUERADE";
pub const IPT_DNAT_TARGET: &str = "DNAT";
pub const IPT_SNAT_TARGET: &str = "SNAT";
pub const IPT_REDIRECT_TARGET: &str = "REDIRECT";

/// iptables tablo adları
///
/// Her tablo belirli bir amaç için tasarlanmıştır.
/// En çok kullanılan "filter" ve "nat" tablolarıdır.
pub const IPTABLES_FILTER_TABLE: &str = "filter";
pub const IPTABLES_NAT_TABLE: &str = "nat";
pub const IPTABLES_MANGLE_TABLE: &str = "mangle";
pub const IPTABLES_RAW_TABLE: &str = "raw";
pub const IPTABLES_SECURITY_TABLE: &str = "security";

// ============================================================================
// IPTABLES KURAL GİRİŞİ (IPTABLES ENTRY)
// ============================================================================
//
// Bir iptables kuralı şu bileşenlerden oluşur:
//   Eşleştirici (match)  →  Hedef (target)
//
// Örnek: "kaynak IP 192.168.1.0/24 ise KABUL et"
//   src_ip = 0xC0A80100, src_mask = 0xFFFFFF00, target = ACCEPT
//
// IP adresleri u32 olarak tutulur (little-endian). Maskeleme işlemi
// bitwise AND ile yapılır: (pkt.src_ip & mask) == (rule.src_ip & mask)

/// Bir iptables kuralını temsil eden yapı.
///
/// - `src_ip` / `src_mask`    : Kaynak IP adresi ve ağ maskesi
/// - `dst_ip` / `dst_mask`    : Hedef IP adresi ve ağ maskesi
/// - `in_iface` / `out_iface` : Gelen/giden arabirim adı filtresi
/// - `proto`                  : IP protokol numarası (0=hepsi, 6=TCP, 17=UDP)
/// - `src_ports` / `dst_ports`: Port aralığı (dahil, min..=max)
/// - `tcp_flags`              : TCP bayrak maskesi (SYN, ACK, FIN vb.)
/// - `matches`                : Ek eşleştirici uzantıları (conntrack, limit vb.)
/// - `target`                 : Eşleşme durumunda uygulanacak eylem
/// - `packet_count`/`byte_count`: İstatistik sayaçları (atomik güncelleme)
#[derive(Clone, Debug)]
pub struct IptEntry {
    /// Source IP address
    pub src_ip: u32,
    /// Source mask
    pub src_mask: u32,
    /// Destination IP address
    pub dst_ip: u32,
    /// Destination mask
    pub dst_mask: u32,
    /// Input interface
    pub in_iface: String,
    /// Output interface
    pub out_iface: String,
    /// Protocol
    pub proto: u8,
    /// Source port range
    pub src_ports: (u16, u16),
    /// Destination port range
    pub dst_ports: (u16, u16),
    /// TCP flags
    pub tcp_flags: u8,
    /// Match extensions
    pub matches: Vec<IptMatch>,
    /// Target
    pub target: IptTarget,
    /// Packet count
    pub packet_count: AtomicU64,
    /// Byte count
    pub byte_count: AtomicU64,
}

impl IptEntry {
    /// Tüm trafiği kabul eden varsayılan kural oluşturur.
    ///
    /// Maske 0xFFFFFFFF olduğunda IP eşleştirmesi tam adres üzerinden yapılır.
    /// src_ip = 0 ve src_mask = 0xFFFFFFFF → yalnızca 0.0.0.0 eşleşir
    /// (bu davranış "herhangi bir kaynak IP" için özelleştirme gerektirir).
    pub fn new() -> Self {
        Self {
            src_ip: 0,
            src_mask: 0xFFFFFFFF,
            dst_ip: 0,
            dst_mask: 0xFFFFFFFF,
            in_iface: String::new(),
            out_iface: String::new(),
            proto: 0,
            src_ports: (0, 65535),
            dst_ports: (0, 65535),
            tcp_flags: 0,
            matches: Vec::new(),
            target: IptTarget::accept(),
            packet_count: AtomicU64::new(0),
            byte_count: AtomicU64::new(0),
        }
    }

    /// Paketin bu kurala uyup uymadığını sırayla kontrol eder.
    ///
    /// Kontrol sırası Linux çekirdeğiyle aynıdır:
    /// kaynak IP → hedef IP → protokol → portlar → arabirim adı
    /// Herhangi bir kontrol başarısız olursa `false` döner (kısa devre).
    pub fn matches_packet(&self, pkt: &PacketInfo) -> bool {
        // Check source IP
        if (pkt.src_ip & self.src_mask) != (self.src_ip & self.src_mask) {
            return false;
        }

        // Check destination IP
        if (pkt.dst_ip & self.dst_mask) != (self.dst_ip & self.dst_mask) {
            return false;
        }

        // Check protocol
        if self.proto != 0 && pkt.proto != self.proto {
            return false;
        }

        // Check ports
        if pkt.src_port < self.src_ports.0 || pkt.src_port > self.src_ports.1 {
            return false;
        }
        if pkt.dst_port < self.dst_ports.0 || pkt.dst_port > self.dst_ports.1 {
            return false;
        }

        // Check interface
        if !self.in_iface.is_empty() && !pkt.in_iface.starts_with(&self.in_iface) {
            return false;
        }
        if !self.out_iface.is_empty() && !pkt.out_iface.starts_with(&self.out_iface) {
            return false;
        }

        true
    }
}

/// Ek eşleştirici uzantısı (match extension).
///
/// iptables'ın `-m` seçeneğiyle yüklenen modüllere karşılık gelir.
/// Örneğin `-m conntrack --ctstate ESTABLISHED` gibi gelişmiş filtreler.
/// `data` alanı modüle özgü ham yapılandırma baytlarını taşır.
#[derive(Clone, Debug)]
pub struct IptMatch {
    pub name: String,
    pub data: Vec<u8>,
}

/// Kural hedefi (target).
///
/// Bir kural eşleştiğinde uygulanacak eylemi tanımlar.
/// - `name`    : Hedef adı ("ACCEPT", "DROP", "SNAT" vb.)
/// - `verdict` : İşlem kodu (NF_ACCEPT, NF_DROP vb.)
/// - `data`    : NAT hedeflerinde yeni IP ve port bilgisi (little-endian)
#[derive(Clone, Debug)]
pub struct IptTarget {
    pub name: String,
    pub verdict: u32,
    pub data: Vec<u8>,
}

impl IptTarget {
    /// Paketi kabul eden hedef oluşturur. Linux'ta `-j ACCEPT` ile eşdeğerdir.
    pub fn accept() -> Self {
        Self { name: String::from("ACCEPT"), verdict: NF_ACCEPT, data: Vec::new() }
    }

    /// Paketi sessizce düşüren hedef. `-j DROP` ile eşdeğerdir.
    /// Gönderici herhangi bir hata mesajı almaz (güvenlik açısından tercih edilir).
    pub fn drop() -> Self {
        Self { name: String::from("DROP"), verdict: NF_DROP, data: Vec::new() }
    }

    /// Zincirlerde kullanılan RETURN hedefi.
    /// Alt zincirden çağıran zincire geri döner.
    /// 0xFFFFFFFF özel RETURN sentinel değeri olarak kullanılır.
    pub fn return_() -> Self {
        Self { name: String::from("RETURN"), verdict: 0xFFFFFFFF, data: Vec::new() }
    }

    /// MASQUERADE: Kaynak IP'yi çıkış arabiriminin IP'siyle değiştirir.
    /// Dinamik IP'ye sahip bağlantı paylaşımında kullanılır (ev yönlendiricisi gibi).
    pub fn masquerade() -> Self {
        Self { name: String::from("MASQUERADE"), verdict: NF_ACCEPT, data: Vec::new() }
    }

    /// SNAT (Source NAT): Kaynak IP ve portu sabit bir adresle değiştirir.
    /// IP ve port baytları little-endian sırayla `data` alanına yazılır.
    pub fn snat(ip: u32, port: u16) -> Self {
        Self {
            name: String::from("SNAT"),
            verdict: NF_ACCEPT,
            data: vec![
                (ip & 0xFF) as u8,
                ((ip >> 8) & 0xFF) as u8,
                ((ip >> 16) & 0xFF) as u8,
                ((ip >> 24) & 0xFF) as u8,
                (port & 0xFF) as u8,
                ((port >> 8) & 0xFF) as u8,
            ]
        }
    }

    /// DNAT (Destination NAT): Hedef IP ve portu değiştirir.
    /// Port yönlendirme (port forwarding) bu hedefle gerçekleştirilir.
    pub fn dnat(ip: u32, port: u16) -> Self {
        Self {
            name: String::from("DNAT"),
            verdict: NF_ACCEPT,
            data: vec![
                (ip & 0xFF) as u8,
                ((ip >> 8) & 0xFF) as u8,
                ((ip >> 16) & 0xFF) as u8,
                ((ip >> 24) & 0xFF) as u8,
                (port & 0xFF) as u8,
                ((port >> 8) & 0xFF) as u8,
            ]
        }
    }
}

// ============================================================================
// IPTABLES ZİNCİRİ (IPTABLES CHAIN)
// ============================================================================
//
// Bir zincir, sırayla değerlendirilen kurallar listesidir.
// Her tablo birden fazla zincir içerebilir. Zincirler:
//   - Yerleşik (built-in): INPUT, OUTPUT, FORWARD, PREROUTING, POSTROUTING
//   - Kullanıcı tanımlı : Herhangi bir isim verilebilir; JUMP ile çağrılır
//
// Bir paket zincirden geçerken:
//   1. Her kural sırayla kontrol edilir.
//   2. İlk eşleşen kuralın hedefi uygulanır.
//   3. Hiçbiri eşleşmezse zincirin politikası (policy) döner.

/// Bir iptables zincirini temsil eden yapı.
///
/// - `name`          : Zincir adı ("INPUT", "OUTPUT", "MYCHAIN" vb.)
/// - `hook`          : Hangi netfilter kancasına bağlı olduğu
/// - `policy`        : Hiçbir kural eşleşmediğinde varsayılan karar
/// - `entries`       : Sıralı kural listesi
/// - `packet_count`  : Bu zincirden geçen toplam paket sayısı
/// - `byte_count`    : Bu zincirden geçen toplam bayt sayısı
#[derive(Clone, Debug)]
pub struct IptChain {
    pub name: String,
    pub hook: u32,
    pub policy: u32,
    pub entries: Vec<IptEntry>,
    pub packet_count: AtomicU64,
    pub byte_count: AtomicU64,
}

impl IptChain {
    /// Yeni bir zincir oluşturur.
    ///
    /// `policy` genellikle NF_ACCEPT (beyaz liste yaklaşımı) veya
    ///  NF_DROP (kara liste — güvenlik duvarı varsayılanı) olur.
    pub fn new(name: &str, hook: u32, policy: u32) -> Self {
        Self {
            name: String::from(name),
            hook,
            policy,
            entries: Vec::new(),
            packet_count: AtomicU64::new(0),
            byte_count: AtomicU64::new(0),
        }
    }

    /// Zincirin sonuna yeni bir kural ekler (append).
    /// Linux'ta `iptables -A CHAIN ...` eşdeğeridir.
    pub fn add_entry(&mut self, entry: IptEntry) {
        self.entries.push(entry);
    }

    /// Belirtilen konuma kural ekler (insert).
    /// Linux'ta `iptables -I CHAIN NUM ...` eşdeğeridir.
    pub fn insert_entry(&mut self, entry: IptEntry, pos: usize) {
        if pos <= self.entries.len() {
            self.entries.insert(pos, entry);
        }
    }

    /// Belirtilen konumdaki kuralı siler.
    /// Linux'ta `iptables -D CHAIN NUM` eşdeğeridir.
    pub fn delete_entry(&mut self, pos: usize) -> Option<IptEntry> {
        if pos < self.entries.len() {
            Some(self.entries.remove(pos))
        } else {
            None
        }
    }

    /// Paketi zincir kuralları üzerinden geçirir ve karar döndürür.
    ///
    /// İlk eşleşen kural bulunduğunda istatistikler atomik olarak güncellenir
    /// ve `execute_target()` çağrılır. Hiçbir kural eşleşmezse `self.policy` döner.
    pub fn traverse(&self, pkt: &mut PacketInfo) -> u32 {
        for entry in &self.entries {
            if entry.matches_packet(pkt) {
                entry.packet_count.fetch_add(1, Ordering::Relaxed);
                entry.byte_count.fetch_add(pkt.len as u64, Ordering::Relaxed);

                // Execute target
                return self.execute_target(&entry.target, pkt);
            }
        }

        // Return policy
        self.policy
    }

    /// Eşleşen kuralın hedefini uygular ve karar kodu döndürür.
    ///
    /// NAT hedefleri (MASQUERADE, SNAT, DNAT) burada paketin adres
    /// alanlarını değiştirir. Gerçek başlık rewrite işlemi ağ yığınının
    /// çıkış aşamasında yapılır; burada yalnızca `PacketInfo` güncellenir.
    fn execute_target(&self, target: &IptTarget, pkt: &mut PacketInfo) -> u32 {
        match target.name.as_str() {
            "ACCEPT" => NF_ACCEPT,
            "DROP" => NF_DROP,
            "RETURN" => 0xFFFFFFFF,
            "MASQUERADE" => {
                // NAT: set source to outgoing interface IP
                pkt.new_src_ip = pkt.out_iface_ip;
                NF_ACCEPT
            }
            "SNAT" => {
                if target.data.len() >= 6 {
                    let ip = u32::from_le_bytes([
                        target.data[0], target.data[1], target.data[2], target.data[3]
                    ]);
                    pkt.new_src_ip = ip;
                }
                NF_ACCEPT
            }
            "DNAT" => {
                if target.data.len() >= 6 {
                    let ip = u32::from_le_bytes([
                        target.data[0], target.data[1], target.data[2], target.data[3]
                    ]);
                    pkt.new_dst_ip = ip;
                }
                NF_ACCEPT
            }
            "REJECT" => {
                // Send ICMP/ICMPv6 unreachable
                NF_DROP
            }
            "LOG" => {
                crate::serial_println!(
                    "[IPTABLES] LOG: {}:{} -> {}:{} proto={}",
                    pkt.src_ip, pkt.src_port,
                    pkt.dst_ip, pkt.dst_port,
                    pkt.proto
                );
                NF_ACCEPT
            }
            _ => target.verdict,
        }
    }
}

// ============================================================================
// IPTABLES TABLOSU (IPTABLES TABLE)
// ============================================================================
//
// Bir tablo, ilgili zincirleri gruplar. `BTreeMap` kullanılmasının nedeni
// deterministik sıralama (B-tree) sağlaması ve no_std ortamında
// `HashMap`'in aksine varsayılan hasher gerektirmemesidir.

/// Bir iptables tablosunu temsil eden yapı.
///
/// - `name`   : Tablo adı ("filter", "nat", "mangle" vb.)
/// - `chains` : Tablo içindeki zincirler (zincir adına göre anahtar-değer)
#[derive(Clone, Debug)]
pub struct IptTable {
    pub name: String,
    pub chains: BTreeMap<String, IptChain>,
}

impl IptTable {
    /// Yeni bir boş tablo oluşturur; zincirler daha sonra eklenir.
    pub fn new(name: &str) -> Self {
        Self {
            name: String::from(name),
            chains: BTreeMap::new(),
        }
    }

    /// Tabloya yeni bir zincir ekler.
    /// Aynı isimde zincir varsa üzerine yazılır.
    pub fn add_chain(&mut self, chain: IptChain) {
        self.chains.insert(chain.name.clone(), chain);
    }

    /// İsme göre zincir döndürür (salt okunur).
    pub fn get_chain(&self, name: &str) -> Option<&IptChain> {
        self.chains.get(name)
    }

    /// İsme göre zincir döndürür (değiştirilebilir).
    pub fn get_chain_mut(&mut self, name: &str) -> Option<&mut IptChain> {
        self.chains.get_mut(name)
    }
}

// ============================================================================
// PAKET BİLGİSİ (PACKET INFO)
// ============================================================================
//
// `PacketInfo`, bir IP paketini filtre motorunun anlayacağı biçime dönüştürür.
// IP başlığı çözümlendikten sonra bu yapı doldurulur ve tüm zincirler
// bu yapı üzerinden karara varır. NAT eylemlerinde `new_src_ip`/`new_dst_ip`
// alanları güncellenir; asıl başlık rewrite çıkış aşamasında yazılır.

/// Bir IP paketinin filtre motoruna sunulan temsili.
///
/// - `src_ip`/`dst_ip`           : Orijinal kaynak/hedef IPv4 adresi (u32, network byte order)
/// - `src_port`/`dst_port`       : TCP/UDP bağlantı noktaları
/// - `proto`                     : IP protokol numarası (6=TCP, 17=UDP, 1=ICMP)
/// - `in_iface`/`out_iface`      : Paketin geldiği/gideceği arabirim adı
/// - `in_iface_ip`/`out_iface_ip`: NAT için arabirim IP adresleri
/// - `len`                       : Paket boyutu (istatistik için)
/// - `new_src_ip`/`new_dst_ip`   : NAT sonrasında uygulanacak yeni adresler
/// - `new_src_port`/`new_dst_port`: NAT sonrasında uygulanacak yeni portlar
/// - `conntrack_state`           : Bağlantı izleme durumu (yeni/kuruldu/ilgili)
#[derive(Clone, Debug)]
pub struct PacketInfo {
    pub src_ip: u32,
    pub dst_ip: u32,
    pub src_port: u16,
    pub dst_port: u16,
    pub proto: u8,
    pub in_iface: String,
    pub out_iface: String,
    pub in_iface_ip: u32,
    pub out_iface_ip: u32,
    pub len: usize,
    pub new_src_ip: u32,
    pub new_dst_ip: u32,
    pub new_src_port: u16,
    pub new_dst_port: u16,
    pub conntrack_state: ConntrackState,
}

/// Bağlantı takip durumu (Connection tracking state).
///
/// Linux'ta `-m conntrack --ctstate` ile eşleştirilen bu durum,
/// güvenlik duvarının durum bilgili (stateful) kararlar almasını sağlar.
/// - `New`         : İlk paket (henüz bağlantı kurulmadı)
/// - `Established` : Çift yönlü trafik görüldü
/// - `Related`     : FTP veri bağlantısı gibi ilgili ancak farklı bağlantı
/// - `Invalid`     : Hiçbir durumla eşleşmeyen bozuk/sahte paket
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConntrackState {
    New,
    Established,
    Related,
    Invalid,
}

// ============================================================================
// NETFİLTER YÖNETİCİSİ (NETFILTER MANAGER)
// ============================================================================
//
// `NetfilterManager`, tüm tabloları ve istatistikleri tek bir global yapıda
// toplar. `lazy_static!` ile çekirdek başlangıcında tek seferlik oluşturulur.
//
// Thread güvenliği:
//   - `tables` : `Mutex<BTreeMap>` — aynı anda yalnızca bir çekirdek kuralları değiştirebilir
//   - `enabled`: `AtomicBool`      — okuma/yazma için kilit gerekmez; uygun çekirdek çağrısı
//   - `stats`  : `Mutex<...>`      — sayaç güncellemeleri kilitli bölgede yapılır

/// Netfilter alt sisteminin merkezi yönetici yapısı.
pub struct NetfilterManager {
    tables: Mutex<BTreeMap<String, IptTable>>,
    enabled: AtomicBool,
    stats: Mutex<NetfilterStats>,
}

/// Netfilter istatistikleri.
///
/// Bu sayaçlar `iptables -L -v` çıktısındaki paket/bayt bilgilerine karşılık gelir.
#[derive(Clone, Debug, Default)]
pub struct NetfilterStats {
    pub packets_processed: u64,
    pub packets_dropped: u64,
    pub packets_accepted: u64,
    pub nat_count: u64,
}

impl NetfilterManager {
    /// Sabit fonksiyon (const fn) ile derleme zamanında örnek oluşturur.
    /// Bu sayede `lazy_static!` makrosu çalışma zamanı başlatması olmadan
    /// global yapıyı tanımlayabilir.
    pub const fn new() -> Self {
        Self {
            tables: Mutex::new(BTreeMap::new()),
            enabled: AtomicBool::new(true),
            stats: Mutex::new(NetfilterStats::default()),
        }
    }

    /// Varsayılan tabloları ve zincirleri başlatır.
    ///
    /// Linux'taki varsayılan güvenlik duvarı politikasını yansıtır:
    /// - INPUT/OUTPUT: ACCEPT (varsayılan olarak izin ver)
    /// - FORWARD: DROP   (yönlendirme devre dışı — güvenli varsayılan)
    pub fn init(&self) {
        // Filter table
        let mut filter = IptTable::new(IPTABLES_FILTER_TABLE);
        filter.add_chain(IptChain::new("INPUT", NF_INET_LOCAL_IN, NF_ACCEPT));
        filter.add_chain(IptChain::new("FORWARD", NF_INET_FORWARD, NF_DROP));
        filter.add_chain(IptChain::new("OUTPUT", NF_INET_LOCAL_OUT, NF_ACCEPT));
        self.tables.lock().insert(String::from(IPTABLES_FILTER_TABLE), filter);

        // NAT table
        let mut nat = IptTable::new(IPTABLES_NAT_TABLE);
        nat.add_chain(IptChain::new("PREROUTING", NF_INET_PRE_ROUTING, NF_ACCEPT));
        nat.add_chain(IptChain::new("POSTROUTING", NF_INET_POST_ROUTING, NF_ACCEPT));
        nat.add_chain(IptChain::new("OUTPUT", NF_INET_LOCAL_OUT, NF_ACCEPT));
        self.tables.lock().insert(String::from(IPTABLES_NAT_TABLE), nat);

        crate::serial_println!("[NETFILTER] Initialized iptables");
    }

    /// Paketi belirtilen kanca noktasındaki zincirlerden geçirir ve karar döndürür.
    ///
    /// Netfilter devre dışıysa (`enabled = false`) tüm paketler kabul edilir.
    /// Bu mekanizma, kural değişiklikleri sırasında geçici olarak kullanılabilir.
    pub fn process_packet(&self, pkt: &mut PacketInfo, hook: u32) -> u32 {
        if !self.enabled.load(Ordering::SeqCst) {
            return NF_ACCEPT;
        }

        let mut stats = self.stats.lock();
        stats.packets_processed += 1;

        // Process through filter table
        if let Some(table) = self.tables.lock().get(IPTABLES_FILTER_TABLE) {
            for chain in table.chains.values() {
                if chain.hook == hook {
                    let verdict = chain.traverse(pkt);
                    match verdict {
                        NF_ACCEPT => stats.packets_accepted += 1,
                        NF_DROP => stats.packets_dropped += 1,
                        _ => {}
                    }
                    return verdict;
                }
            }
        }

        NF_ACCEPT
    }

    /// Tablodaki zincire yeni bir kural ekler.
    /// Hata durumunda `NetfilterError` döner (tablo veya zincir bulunamazsa).
    pub fn add_rule(&self, table: &str, chain: &str, entry: IptEntry) -> Result<(), NetfilterError> {
        let mut tables = self.tables.lock();
        let tbl = tables.get_mut(table).ok_or(NetfilterError::TableNotFound)?;
        let chn = tbl.get_chain_mut(chain).ok_or(NetfilterError::ChainNotFound)?;
        chn.add_entry(entry);
        Ok(())
    }

    /// Tablodaki zincirden belirli konumdaki kuralı siler.
    pub fn delete_rule(&self, table: &str, chain: &str, pos: usize) -> Result<(), NetfilterError> {
        let mut tables = self.tables.lock();
        let tbl = tables.get_mut(table).ok_or(NetfilterError::TableNotFound)?;
        let chn = tbl.get_chain_mut(chain).ok_or(NetfilterError::ChainNotFound)?;
        chn.delete_entry(pos);
        Ok(())
    }

    /// O ana kadar birikmiş istatistikleri döndürür.
    pub fn get_stats(&self) -> NetfilterStats {
        self.stats.lock().clone()
    }

    /// Netfilter'ı etkinleştirir ya da devre dışı bırakır.
    /// SeqCst (Sequentially Consistent) sıralama garantisi tüm çekirdeklerin
    /// değişikliği hemen görmesini sağlar.
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::SeqCst);
    }
}

// `lazy_static!` makrosu, `static` değişkenlerde çalışma zamanı başlatıcı
// kullanmayı sağlar. NETFILTER sabiti tüm modüllerden erişilebilir global
// netfilter yöneticisidir.
lazy_static::lazy_static! {
    pub static ref NETFILTER: NetfilterManager = NetfilterManager::new();
}

// ============================================================================
// HATA TİPİ (ERROR TYPE)
// ============================================================================

/// Netfilter işlem hataları.
///
/// - `TableNotFound`    : Belirtilen tablo adı (`filter`, `nat` vb.) bulunamadı
/// - `ChainNotFound`    : Belirtilen zincir adı o tabloda yok
/// - `InvalidRule`      : Kural yapısı geçersiz veya tutarsız
/// - `PermissionDenied` : Yetkisiz erişim (kullanıcı modu güvenlik duvarı yönetimi için)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetfilterError {
    TableNotFound,
    ChainNotFound,
    InvalidRule,
    PermissionDenied,
}

// ============================================================================
// BAŞLATMA (INITIALIZATION)
// ============================================================================

/// Netfilter alt sistemini başlatır.
///
/// Çekirdek önyükleme sırasında `net::init()` tarafından çağrılır.
/// Bu fonksiyon `NETFILTER.init()` aracılığıyla varsayılan tabloları kurar.
pub fn init() {
    NETFILTER.init();
}
