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
//! - **mangle** : Paket başlığı değişikliği (TTL, TOS vb.)
//! - **raw**    : Bağlantı takibinden (conntrack) muaf tutma
//! - **security**: SELinux/AppArmor kararları

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

use super::ip::{self, IpProtocol, Ipv4Packet};
use super::ipv6::{self, Ipv6Addr, Ipv6Packet};
use super::tcp::TcpHeader;
use super::udp::UdpHeader;
use super::{default_interface, Ipv4Addr, NetError};

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
pub const IPT_TTL_TARGET: &str = "TTL";
pub const IPT_TOS_TARGET: &str = "TOS";

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
#[derive(Debug)]
pub struct IptEntry {
    /// Protocol family selector (`NFPROTO_UNSPEC`, `NFPROTO_IPV4`, `NFPROTO_IPV6`)
    pub family: u32,
    /// Source IP address
    pub src_ip: u32,
    /// Source mask
    pub src_mask: u32,
    /// Destination IP address
    pub dst_ip: u32,
    /// Destination mask
    pub dst_mask: u32,
    /// Source IPv6 address
    pub src_ip6: [u8; 16],
    /// Source IPv6 mask
    pub src_mask6: [u8; 16],
    /// Destination IPv6 address
    pub dst_ip6: [u8; 16],
    /// Destination IPv6 mask
    pub dst_mask6: [u8; 16],
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
            family: NFPROTO_UNSPEC,
            src_ip: 0,
            src_mask: 0xFFFFFFFF,
            dst_ip: 0,
            dst_mask: 0xFFFFFFFF,
            src_ip6: [0; 16],
            src_mask6: [0; 16],
            dst_ip6: [0; 16],
            dst_mask6: [0; 16],
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
        if self.family != NFPROTO_UNSPEC && self.family != pkt.family {
            return false;
        }

        match pkt.family {
            NFPROTO_IPV6 => {
                if !masked_match_ipv6(&pkt.src_addr, &self.src_ip6, &self.src_mask6) {
                    return false;
                }
                if !masked_match_ipv6(&pkt.dst_addr, &self.dst_ip6, &self.dst_mask6) {
                    return false;
                }
            }
            _ => {
                if (pkt.src_ip & self.src_mask) != (self.src_ip & self.src_mask) {
                    return false;
                }
                if (pkt.dst_ip & self.dst_mask) != (self.dst_ip & self.dst_mask) {
                    return false;
                }
            }
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
        Self {
            name: String::from("ACCEPT"),
            verdict: NF_ACCEPT,
            data: Vec::new(),
        }
    }

    /// Paketi sessizce düşüren hedef. `-j DROP` ile eşdeğerdir.
    /// Gönderici herhangi bir hata mesajı almaz (güvenlik açısından tercih edilir).
    pub fn drop() -> Self {
        Self {
            name: String::from("DROP"),
            verdict: NF_DROP,
            data: Vec::new(),
        }
    }

    /// Zincirlerde kullanılan RETURN hedefi.
    /// Alt zincirden çağıran zincire geri döner.
    /// 0xFFFFFFFF özel RETURN sentinel değeri olarak kullanılır.
    pub fn return_() -> Self {
        Self {
            name: String::from("RETURN"),
            verdict: 0xFFFFFFFF,
            data: Vec::new(),
        }
    }

    /// MASQUERADE: Kaynak IP'yi çıkış arabiriminin IP'siyle değiştirir.
    /// Dinamik IP'ye sahip bağlantı paylaşımında kullanılır (ev yönlendiricisi gibi).
    pub fn masquerade() -> Self {
        Self {
            name: String::from("MASQUERADE"),
            verdict: NF_ACCEPT,
            data: Vec::new(),
        }
    }

    /// IPv6 SNAT target.
    pub fn snat_v6(ip: Ipv6Addr, port: u16) -> Self {
        let mut data = ip.as_bytes().to_vec();
        data.extend_from_slice(&port.to_le_bytes());
        Self {
            name: String::from("SNAT"),
            verdict: NF_ACCEPT,
            data,
        }
    }

    /// IPv6 DNAT target.
    pub fn dnat_v6(ip: Ipv6Addr, port: u16) -> Self {
        let mut data = ip.as_bytes().to_vec();
        data.extend_from_slice(&port.to_le_bytes());
        Self {
            name: String::from("DNAT"),
            verdict: NF_ACCEPT,
            data,
        }
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
            ],
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
            ],
        }
    }

    pub fn ttl(ttl: u8) -> Self {
        Self {
            name: String::from(IPT_TTL_TARGET),
            verdict: NF_ACCEPT,
            data: vec![ttl],
        }
    }

    pub fn tos(tos: u8) -> Self {
        Self {
            name: String::from(IPT_TOS_TARGET),
            verdict: NF_ACCEPT,
            data: vec![tos],
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
#[derive(Debug)]
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
                entry
                    .byte_count
                    .fetch_add(pkt.len as u64, Ordering::Relaxed);

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
                if pkt.family == NFPROTO_IPV6 {
                    pkt.has_new_src_addr = true;
                    pkt.new_src_addr = pkt.out_iface_addr;
                } else {
                    pkt.new_src_ip = pkt.out_iface_ip;
                }
                NF_ACCEPT
            }
            "SNAT" => {
                if pkt.family == NFPROTO_IPV6 && target.data.len() >= 18 {
                    pkt.has_new_src_addr = true;
                    pkt.new_src_addr.copy_from_slice(&target.data[..16]);
                    pkt.new_src_port = u16::from_le_bytes([target.data[16], target.data[17]]);
                } else if target.data.len() >= 6 {
                    let ip = u32::from_le_bytes([
                        target.data[0],
                        target.data[1],
                        target.data[2],
                        target.data[3],
                    ]);
                    let port = u16::from_le_bytes([target.data[4], target.data[5]]);
                    pkt.new_src_ip = ip;
                    pkt.new_src_addr = ipv4_to_ipv6_bytes(Ipv4Addr::from_u32(ip));
                    pkt.new_src_port = port;
                }
                NF_ACCEPT
            }
            "DNAT" => {
                if pkt.family == NFPROTO_IPV6 && target.data.len() >= 18 {
                    pkt.has_new_dst_addr = true;
                    pkt.new_dst_addr.copy_from_slice(&target.data[..16]);
                    pkt.new_dst_port = u16::from_le_bytes([target.data[16], target.data[17]]);
                } else if target.data.len() >= 6 {
                    let ip = u32::from_le_bytes([
                        target.data[0],
                        target.data[1],
                        target.data[2],
                        target.data[3],
                    ]);
                    let port = u16::from_le_bytes([target.data[4], target.data[5]]);
                    pkt.new_dst_ip = ip;
                    pkt.new_dst_addr = ipv4_to_ipv6_bytes(Ipv4Addr::from_u32(ip));
                    pkt.new_dst_port = port;
                }
                NF_ACCEPT
            }
            IPT_TTL_TARGET => {
                if let Some(ttl) = target.data.first().copied() {
                    pkt.new_ttl = ttl;
                }
                NF_ACCEPT
            }
            IPT_TOS_TARGET => {
                if let Some(tos) = target.data.first().copied() {
                    pkt.new_tos = tos;
                }
                NF_ACCEPT
            }
            "REJECT" => {
                // Send ICMP/ICMPv6 unreachable
                NF_DROP
            }
            "LOG" => {
                if pkt.family == NFPROTO_IPV6 {
                    crate::serial_println!(
                        "[IPTABLES] LOG: {}:{} -> {}:{} proto={} fam=IPv6",
                        Ipv6Addr::new(pkt.src_addr).to_string(),
                        pkt.src_port,
                        Ipv6Addr::new(pkt.dst_addr).to_string(),
                        pkt.dst_port,
                        pkt.proto
                    );
                } else {
                    crate::serial_println!(
                        "[IPTABLES] LOG: {}:{} -> {}:{} proto={} fam=IPv4",
                        pkt.src_ip,
                        pkt.src_port,
                        pkt.dst_ip,
                        pkt.dst_port,
                        pkt.proto
                    );
                }
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
#[derive(Debug)]
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
    pub family: u32,
    pub src_ip: u32,
    pub dst_ip: u32,
    pub src_addr: [u8; 16],
    pub dst_addr: [u8; 16],
    pub src_port: u16,
    pub dst_port: u16,
    pub proto: u8,
    pub in_iface: String,
    pub out_iface: String,
    pub in_iface_ip: u32,
    pub out_iface_ip: u32,
    pub in_iface_addr: [u8; 16],
    pub out_iface_addr: [u8; 16],
    pub len: usize,
    pub ttl: u8,
    pub tos: u8,
    pub new_src_ip: u32,
    pub new_dst_ip: u32,
    pub has_new_src_addr: bool,
    pub has_new_dst_addr: bool,
    pub new_src_addr: [u8; 16],
    pub new_dst_addr: [u8; 16],
    pub new_src_port: u16,
    pub new_dst_port: u16,
    pub new_ttl: u8,
    pub new_tos: u8,
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
            stats: Mutex::new(NetfilterStats {
                packets_processed: 0,
                packets_dropped: 0,
                packets_accepted: 0,
                nat_count: 0,
            }),
        }
    }

    /// Varsayılan tabloları ve zincirleri başlatır.
    ///
    /// Linux'taki varsayılan güvenlik duvarı politikasını yansıtır:
    /// - INPUT/OUTPUT: ACCEPT (varsayılan olarak izin ver)
    /// - FORWARD: DROP   (yönlendirme devre dışı — güvenli varsayılan)
    pub fn init(&self) {
        self.enabled.store(true, Ordering::SeqCst);
        {
            let mut stats = self.stats.lock();
            *stats = NetfilterStats::default();
        }
        // Filter table
        let mut filter = IptTable::new(IPTABLES_FILTER_TABLE);
        filter.add_chain(IptChain::new("INPUT", NF_INET_LOCAL_IN, NF_ACCEPT));
        filter.add_chain(IptChain::new("FORWARD", NF_INET_FORWARD, NF_DROP));
        filter.add_chain(IptChain::new("OUTPUT", NF_INET_LOCAL_OUT, NF_ACCEPT));
        self.tables
            .lock()
            .insert(String::from(IPTABLES_FILTER_TABLE), filter);

        // NAT table
        let mut nat = IptTable::new(IPTABLES_NAT_TABLE);
        nat.add_chain(IptChain::new("PREROUTING", NF_INET_PRE_ROUTING, NF_ACCEPT));
        nat.add_chain(IptChain::new(
            "POSTROUTING",
            NF_INET_POST_ROUTING,
            NF_ACCEPT,
        ));
        nat.add_chain(IptChain::new("OUTPUT", NF_INET_LOCAL_OUT, NF_ACCEPT));
        self.tables
            .lock()
            .insert(String::from(IPTABLES_NAT_TABLE), nat);

        let mut mangle = IptTable::new(IPTABLES_MANGLE_TABLE);
        mangle.add_chain(IptChain::new("PREROUTING", NF_INET_PRE_ROUTING, NF_ACCEPT));
        mangle.add_chain(IptChain::new("INPUT", NF_INET_LOCAL_IN, NF_ACCEPT));
        mangle.add_chain(IptChain::new("FORWARD", NF_INET_FORWARD, NF_ACCEPT));
        mangle.add_chain(IptChain::new("OUTPUT", NF_INET_LOCAL_OUT, NF_ACCEPT));
        mangle.add_chain(IptChain::new(
            "POSTROUTING",
            NF_INET_POST_ROUTING,
            NF_ACCEPT,
        ));
        self.tables
            .lock()
            .insert(String::from(IPTABLES_MANGLE_TABLE), mangle);

        let mut raw = IptTable::new(IPTABLES_RAW_TABLE);
        raw.add_chain(IptChain::new("PREROUTING", NF_INET_PRE_ROUTING, NF_ACCEPT));
        raw.add_chain(IptChain::new("OUTPUT", NF_INET_LOCAL_OUT, NF_ACCEPT));
        self.tables
            .lock()
            .insert(String::from(IPTABLES_RAW_TABLE), raw);

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
        let tables = self.tables.lock();
        let verdict = self.traverse_tables(&tables, pkt, hook, &mut stats);
        match verdict {
            NF_ACCEPT => stats.packets_accepted += 1,
            NF_DROP => stats.packets_dropped += 1,
            _ => {}
        }
        verdict
    }

    fn traverse_tables(
        &self,
        tables: &BTreeMap<String, IptTable>,
        pkt: &mut PacketInfo,
        hook: u32,
        stats: &mut NetfilterStats,
    ) -> u32 {
        let mut verdict = NF_ACCEPT;
        let order: &[&str] = match hook {
            NF_INET_PRE_ROUTING => &[
                IPTABLES_RAW_TABLE,
                IPTABLES_MANGLE_TABLE,
                IPTABLES_NAT_TABLE,
            ],
            NF_INET_LOCAL_IN => &[IPTABLES_MANGLE_TABLE, IPTABLES_FILTER_TABLE],
            NF_INET_FORWARD => &[IPTABLES_MANGLE_TABLE, IPTABLES_FILTER_TABLE],
            NF_INET_LOCAL_OUT => &[
                IPTABLES_RAW_TABLE,
                IPTABLES_MANGLE_TABLE,
                IPTABLES_NAT_TABLE,
                IPTABLES_FILTER_TABLE,
            ],
            NF_INET_POST_ROUTING => &[IPTABLES_MANGLE_TABLE, IPTABLES_NAT_TABLE],
            _ => &[IPTABLES_FILTER_TABLE],
        };

        for table_name in order {
            let Some(table) = tables.get(*table_name) else {
                continue;
            };
            for chain in table.chains.values() {
                if chain.hook != hook {
                    continue;
                }
                verdict = chain.traverse(pkt);
                if pkt.new_src_ip != 0
                    || pkt.new_dst_ip != 0
                    || pkt.has_new_src_addr
                    || pkt.has_new_dst_addr
                    || pkt.new_src_port != 0
                    || pkt.new_dst_port != 0
                {
                    stats.nat_count += 1;
                }
                if verdict != NF_ACCEPT {
                    return verdict;
                }
            }
        }
        verdict
    }

    /// Tablodaki zincire yeni bir kural ekler.
    /// Hata durumunda `NetfilterError` döner (tablo veya zincir bulunamazsa).
    pub fn add_rule(
        &self,
        table: &str,
        chain: &str,
        entry: IptEntry,
    ) -> Result<(), NetfilterError> {
        let mut tables = self.tables.lock();
        let tbl = tables.get_mut(table).ok_or(NetfilterError::TableNotFound)?;
        let chn = tbl
            .get_chain_mut(chain)
            .ok_or(NetfilterError::ChainNotFound)?;
        chn.add_entry(entry);
        Ok(())
    }

    /// Tablodaki zincirden belirli konumdaki kuralı siler.
    pub fn delete_rule(&self, table: &str, chain: &str, pos: usize) -> Result<(), NetfilterError> {
        let mut tables = self.tables.lock();
        let tbl = tables.get_mut(table).ok_or(NetfilterError::TableNotFound)?;
        let chn = tbl
            .get_chain_mut(chain)
            .ok_or(NetfilterError::ChainNotFound)?;
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

    /// IPv4 paketini bu yönetici üzerinden işle (global NETFILTER yerine).
    pub fn process_ipv4_packet(
        &self,
        packet: &mut [u8],
        hook: u32,
        in_iface: Option<&str>,
        out_iface: Option<&str>,
    ) -> Result<u32, NetError> {
        let ipv4 = Ipv4Packet::parse(packet)?;
        let mut info = packet_info_from_ipv4(&ipv4, in_iface, out_iface)?;
        let verdict = self.process_packet(&mut info, hook);
        if verdict == NF_ACCEPT {
            apply_packet_info_to_ipv4(packet, &info)?;
        }
        Ok(verdict)
    }

    /// IPv6 paketini bu yönetici üzerinden işle (global NETFILTER yerine).
    pub fn process_ipv6_packet(
        &self,
        packet: &mut [u8],
        hook: u32,
        in_iface: Option<&str>,
        out_iface: Option<&str>,
    ) -> Result<u32, NetError> {
        let ipv6 = Ipv6Packet::parse(packet)?;
        let mut info = packet_info_from_ipv6(&ipv6, in_iface, out_iface)?;
        let verdict = self.process_packet(&mut info, hook);
        if verdict == NF_ACCEPT {
            apply_packet_info_to_ipv6(packet, &info)?;
        }
        Ok(verdict)
    }
}

fn default_iface_identity() -> (String, u32, Ipv6Addr) {
    if let Some(iface) = default_interface() {
        let guard = iface.lock();
        return (
            String::from(guard.name()),
            guard.ip().to_u32(),
            ipv6::link_local_from_mac(guard.mac()),
        );
    }
    (String::new(), 0, Ipv6Addr::UNSPECIFIED)
}

fn packet_info_from_ipv4(
    packet: &Ipv4Packet<'_>,
    in_iface: Option<&str>,
    out_iface: Option<&str>,
) -> Result<PacketInfo, NetError> {
    let (default_name, default_ip, default_ipv6) = default_iface_identity();
    let in_name = in_iface.unwrap_or(default_name.as_str());
    let out_name = out_iface.unwrap_or(default_name.as_str());

    let (src_port, dst_port) = match packet.header.protocol {
        IpProtocol::TCP => {
            let header = TcpHeader::parse(packet.payload)?;
            (header.src_port.0, header.dst_port.0)
        }
        IpProtocol::UDP => {
            let header = UdpHeader::parse(packet.payload)?;
            (header.src_port.0, header.dst_port.0)
        }
        _ => (0, 0),
    };

    Ok(PacketInfo {
        family: NFPROTO_IPV4,
        src_ip: packet.header.src.to_u32(),
        dst_ip: packet.header.dst.to_u32(),
        src_addr: ipv4_to_ipv6_bytes(packet.header.src),
        dst_addr: ipv4_to_ipv6_bytes(packet.header.dst),
        src_port,
        dst_port,
        proto: packet.header.protocol as u8,
        in_iface: String::from(in_name),
        out_iface: String::from(out_name),
        in_iface_ip: default_ip,
        out_iface_ip: default_ip,
        in_iface_addr: *default_ipv6.as_bytes(),
        out_iface_addr: *default_ipv6.as_bytes(),
        len: packet.header.total_length as usize,
        ttl: packet.header.ttl,
        tos: (packet.header.dscp << 2) | packet.header.ecn,
        new_src_ip: 0,
        new_dst_ip: 0,
        has_new_src_addr: false,
        has_new_dst_addr: false,
        new_src_addr: [0; 16],
        new_dst_addr: [0; 16],
        new_src_port: 0,
        new_dst_port: 0,
        new_ttl: packet.header.ttl,
        new_tos: (packet.header.dscp << 2) | packet.header.ecn,
        conntrack_state: ConntrackState::New,
    })
}

fn packet_info_from_ipv6(
    packet: &Ipv6Packet,
    in_iface: Option<&str>,
    out_iface: Option<&str>,
) -> Result<PacketInfo, NetError> {
    let (default_name, default_ip, default_ipv6) = default_iface_identity();
    let in_name = in_iface.unwrap_or(default_name.as_str());
    let out_name = out_iface.unwrap_or(default_name.as_str());
    let (proto, payload_offset) =
        ipv6::walk_extension_headers(&packet.payload, packet.header.next_header);
    if payload_offset > packet.payload.len() {
        return Err(NetError::InvalidPacket);
    }
    let payload = &packet.payload[payload_offset..];

    let (src_port, dst_port) = match proto {
        x if x == ipv6::Ipv6NextHeader::Tcp as u8 => {
            let header = TcpHeader::parse(payload)?;
            (header.src_port.0, header.dst_port.0)
        }
        x if x == ipv6::Ipv6NextHeader::Udp as u8 => {
            let header = UdpHeader::parse(payload)?;
            (header.src_port.0, header.dst_port.0)
        }
        _ => (0, 0),
    };

    Ok(PacketInfo {
        family: NFPROTO_IPV6,
        src_ip: packet
            .header
            .src
            .to_ipv4_mapped()
            .map(|ip| ip.to_u32())
            .unwrap_or(0),
        dst_ip: packet
            .header
            .dst
            .to_ipv4_mapped()
            .map(|ip| ip.to_u32())
            .unwrap_or(0),
        src_addr: *packet.header.src.as_bytes(),
        dst_addr: *packet.header.dst.as_bytes(),
        src_port,
        dst_port,
        proto,
        in_iface: String::from(in_name),
        out_iface: String::from(out_name),
        in_iface_ip: default_ip,
        out_iface_ip: default_ip,
        in_iface_addr: *default_ipv6.as_bytes(),
        out_iface_addr: *default_ipv6.as_bytes(),
        len: ipv6::Ipv6Header::SIZE + packet.payload.len(),
        ttl: packet.header.hop_limit,
        tos: packet.header.traffic_class,
        new_src_ip: 0,
        new_dst_ip: 0,
        has_new_src_addr: false,
        has_new_dst_addr: false,
        new_src_addr: [0; 16],
        new_dst_addr: [0; 16],
        new_src_port: 0,
        new_dst_port: 0,
        new_ttl: packet.header.hop_limit,
        new_tos: packet.header.traffic_class,
        conntrack_state: ConntrackState::New,
    })
}

fn apply_packet_info_to_ipv4(packet: &mut [u8], info: &PacketInfo) -> Result<(), NetError> {
    let header = ip::Ipv4Header::parse(packet)?;
    let header_len = header.header_len();
    if packet.len() < header.total_length as usize || packet.len() < header_len {
        return Err(NetError::InvalidPacket);
    }

    let current_src = header.src;
    let current_dst = header.dst;
    let new_src = if info.new_src_ip != 0 {
        Ipv4Addr::from_u32(info.new_src_ip)
    } else {
        current_src
    };
    let new_dst = if info.new_dst_ip != 0 {
        Ipv4Addr::from_u32(info.new_dst_ip)
    } else {
        current_dst
    };

    if new_src != current_src {
        packet[12..16].copy_from_slice(new_src.as_bytes());
    }
    if new_dst != current_dst {
        packet[16..20].copy_from_slice(new_dst.as_bytes());
    }
    if info.new_ttl != header.ttl {
        packet[8] = info.new_ttl;
    }
    if info.new_tos != ((header.dscp << 2) | header.ecn) {
        packet[1] = info.new_tos;
    }

    match header.protocol {
        IpProtocol::TCP => {
            if packet.len() < header.total_length as usize
                || (header.total_length as usize) < header_len + TcpHeader::MIN_SIZE
            {
                return Err(NetError::InvalidPacket);
            }
            let segment = &mut packet[header_len..header.total_length as usize];
            if info.new_src_port != 0 {
                segment[0..2].copy_from_slice(&info.new_src_port.to_be_bytes());
            }
            if info.new_dst_port != 0 {
                segment[2..4].copy_from_slice(&info.new_dst_port.to_be_bytes());
            }
            segment[16] = 0;
            segment[17] = 0;
            let checksum = super::tcp::compute_checksum(new_src, new_dst, segment);
            segment[16..18].copy_from_slice(&checksum.to_be_bytes());
        }
        IpProtocol::UDP => {
            if packet.len() < header.total_length as usize
                || (header.total_length as usize) < header_len + UdpHeader::SIZE
            {
                return Err(NetError::InvalidPacket);
            }
            let segment = &mut packet[header_len..header.total_length as usize];
            if info.new_src_port != 0 {
                segment[0..2].copy_from_slice(&info.new_src_port.to_be_bytes());
            }
            if info.new_dst_port != 0 {
                segment[2..4].copy_from_slice(&info.new_dst_port.to_be_bytes());
            }
            segment[6] = 0;
            segment[7] = 0;
            let checksum = super::udp::compute_checksum(new_src, new_dst, segment);
            segment[6..8].copy_from_slice(&checksum.to_be_bytes());
        }
        _ => {}
    }

    packet[10] = 0;
    packet[11] = 0;
    let checksum = ip::compute_checksum(&packet[..header_len]);
    packet[10..12].copy_from_slice(&checksum.to_be_bytes());
    Ok(())
}

fn apply_packet_info_to_ipv6(packet: &mut [u8], info: &PacketInfo) -> Result<(), NetError> {
    let mut header = ipv6::Ipv6Header::parse(packet)?;

    let new_src = if info.has_new_src_addr {
        Ipv6Addr::new(info.new_src_addr)
    } else {
        header.src
    };
    let new_dst = if info.has_new_dst_addr {
        Ipv6Addr::new(info.new_dst_addr)
    } else {
        header.dst
    };

    header.src = new_src;
    header.dst = new_dst;
    header.hop_limit = info.new_ttl;
    header.traffic_class = info.new_tos;
    header.serialize(packet)?;

    let (next_header, payload_offset) =
        ipv6::walk_extension_headers(&packet[ipv6::Ipv6Header::SIZE..], header.next_header);
    let segment_offset = ipv6::Ipv6Header::SIZE + payload_offset;
    if segment_offset > packet.len() {
        return Err(NetError::InvalidPacket);
    }
    let segment = &mut packet[segment_offset..];

    match next_header {
        x if x == ipv6::Ipv6NextHeader::Tcp as u8 => {
            if segment.len() < TcpHeader::MIN_SIZE {
                return Err(NetError::InvalidPacket);
            }
            if info.new_src_port != 0 {
                segment[0..2].copy_from_slice(&info.new_src_port.to_be_bytes());
            }
            if info.new_dst_port != 0 {
                segment[2..4].copy_from_slice(&info.new_dst_port.to_be_bytes());
            }
            segment[16] = 0;
            segment[17] = 0;
            let checksum = super::tcp::compute_checksum_v6(new_src, new_dst, segment);
            segment[16..18].copy_from_slice(&checksum.to_be_bytes());
        }
        x if x == ipv6::Ipv6NextHeader::Udp as u8 => {
            if segment.len() < UdpHeader::SIZE {
                return Err(NetError::InvalidPacket);
            }
            if info.new_src_port != 0 {
                segment[0..2].copy_from_slice(&info.new_src_port.to_be_bytes());
            }
            if info.new_dst_port != 0 {
                segment[2..4].copy_from_slice(&info.new_dst_port.to_be_bytes());
            }
            segment[6] = 0;
            segment[7] = 0;
            let checksum = super::udp::compute_checksum_v6(new_src, new_dst, segment);
            segment[6..8].copy_from_slice(&checksum.to_be_bytes());
        }
        _ => {}
    }

    Ok(())
}

pub fn process_ipv4_packet(
    packet: &mut [u8],
    hook: u32,
    in_iface: Option<&str>,
    out_iface: Option<&str>,
) -> Result<u32, NetError> {
    let ipv4 = Ipv4Packet::parse(packet)?;
    let mut info = packet_info_from_ipv4(&ipv4, in_iface, out_iface)?;
    let verdict = NETFILTER.process_packet(&mut info, hook);
    if verdict == NF_ACCEPT {
        apply_packet_info_to_ipv4(packet, &info)?;
    }
    Ok(verdict)
}

pub fn process_ipv6_packet(
    packet: &mut [u8],
    hook: u32,
    in_iface: Option<&str>,
    out_iface: Option<&str>,
) -> Result<u32, NetError> {
    let ipv6 = Ipv6Packet::parse(packet)?;
    let mut info = packet_info_from_ipv6(&ipv6, in_iface, out_iface)?;
    let verdict = NETFILTER.process_packet(&mut info, hook);
    if verdict == NF_ACCEPT {
        apply_packet_info_to_ipv6(packet, &info)?;
    }
    Ok(verdict)
}

fn ipv4_to_ipv6_bytes(ip: Ipv4Addr) -> [u8; 16] {
    let octets = *ip.as_bytes();
    let mut addr = [0u8; 16];
    addr[10] = 0xff;
    addr[11] = 0xff;
    addr[12..16].copy_from_slice(&octets);
    addr
}

fn masked_match_ipv6(packet: &[u8; 16], rule: &[u8; 16], mask: &[u8; 16]) -> bool {
    for idx in 0..16 {
        if (packet[idx] & mask[idx]) != (rule[idx] & mask[idx]) {
            return false;
        }
    }
    true
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

#[cfg(test)]
mod tests {
    use super::super::ip::Ipv4Packet;
    use super::super::ipv6::{Ipv6Addr, Ipv6Header, Ipv6NextHeader, Ipv6Packet};
    use super::super::udp::{self, UdpHeader};
    use super::*;

    fn pkt() -> PacketInfo {
        PacketInfo {
            family: NFPROTO_IPV4,
            src_ip: 0x0a000201,
            dst_ip: 0x0a000202,
            src_addr: ipv4_to_ipv6_bytes(Ipv4Addr::new(10, 0, 2, 1)),
            dst_addr: ipv4_to_ipv6_bytes(Ipv4Addr::new(10, 0, 2, 2)),
            src_port: 1234,
            dst_port: 80,
            proto: 6,
            in_iface: String::from("eth0"),
            out_iface: String::from("eth1"),
            in_iface_ip: 0x0a000201,
            out_iface_ip: 0x0a000203,
            in_iface_addr: *Ipv6Addr::from_segments([0xfe80, 0, 0, 0, 0, 0, 0, 1]).as_bytes(),
            out_iface_addr: *Ipv6Addr::from_segments([0xfe80, 0, 0, 0, 0, 0, 0, 2]).as_bytes(),
            len: 128,
            ttl: 64,
            tos: 0,
            new_src_ip: 0,
            new_dst_ip: 0,
            has_new_src_addr: false,
            has_new_dst_addr: false,
            new_src_addr: [0; 16],
            new_dst_addr: [0; 16],
            new_src_port: 0,
            new_dst_port: 0,
            new_ttl: 64,
            new_tos: 0,
            conntrack_state: ConntrackState::New,
        }
    }

    #[test]
    fn netfilter_init_installs_mangle_and_raw_tables() {
        let manager = NetfilterManager::new();
        manager.init();
        let tables = manager.tables.lock();
        assert!(tables.contains_key(IPTABLES_MANGLE_TABLE));
        assert!(tables.contains_key(IPTABLES_RAW_TABLE));
        assert!(tables[IPTABLES_MANGLE_TABLE]
            .chains
            .contains_key("POSTROUTING"));
        assert!(tables[IPTABLES_RAW_TABLE].chains.contains_key("PREROUTING"));
    }

    #[test]
    fn mangle_table_updates_ttl_and_tos_before_filter() {
        let manager = NetfilterManager::new();
        manager.init();
        let mut rule = IptEntry::new();
        rule.src_mask = 0;
        rule.dst_mask = 0;
        rule.target = IptTarget::ttl(32);
        manager
            .add_rule(IPTABLES_MANGLE_TABLE, "PREROUTING", rule)
            .unwrap();

        let mut packet = pkt();
        let verdict = manager.process_packet(&mut packet, NF_INET_PRE_ROUTING);
        assert_eq!(verdict, NF_ACCEPT);
        assert_eq!(packet.new_ttl, 32);
    }

    #[test]
    fn mangle_table_updates_tos_before_filter() {
        let manager = NetfilterManager::new();
        manager.init();
        let mut rule = IptEntry::new();
        rule.src_mask = 0;
        rule.dst_mask = 0;
        rule.target = IptTarget::tos(0x2e);
        manager
            .add_rule(IPTABLES_MANGLE_TABLE, "PREROUTING", rule)
            .unwrap();

        let mut packet = pkt();
        let verdict = manager.process_packet(&mut packet, NF_INET_PRE_ROUTING);
        assert_eq!(verdict, NF_ACCEPT);
        assert_eq!(packet.new_tos, 0x2e);
    }

    fn build_udp_ipv4_packet(
        src: Ipv4Addr,
        dst: Ipv4Addr,
        src_port: u16,
        dst_port: u16,
        payload: &[u8],
    ) -> Vec<u8> {
        let mut segment = vec![0u8; UdpHeader::SIZE + payload.len()];
        let mut header = UdpHeader::new(
            super::super::Port(src_port),
            super::super::Port(dst_port),
            segment.len() as u16,
        );
        header.serialize(&mut segment).unwrap();
        segment[UdpHeader::SIZE..].copy_from_slice(payload);
        header.checksum = udp::compute_checksum(src, dst, &segment);
        header.serialize(&mut segment).unwrap();

        let packet = Ipv4Packet::new(src, dst, IpProtocol::UDP, &segment);
        let mut buf = vec![0u8; 128];
        let len = packet.serialize(&mut buf).unwrap();
        buf.truncate(len);
        buf
    }

    fn build_udp_ipv6_packet(
        src: Ipv6Addr,
        dst: Ipv6Addr,
        src_port: u16,
        dst_port: u16,
        payload: &[u8],
    ) -> Vec<u8> {
        let mut segment = vec![0u8; UdpHeader::SIZE + payload.len()];
        let mut header = UdpHeader::new(
            super::super::Port(src_port),
            super::super::Port(dst_port),
            segment.len() as u16,
        );
        header.serialize(&mut segment).unwrap();
        segment[UdpHeader::SIZE..].copy_from_slice(payload);
        header.checksum = udp::compute_checksum_v6(src, dst, &segment);
        header.serialize(&mut segment).unwrap();

        let header = Ipv6Header::new(src, dst, Ipv6NextHeader::Udp as u8, segment.len() as u16);
        Ipv6Packet::new(header, &segment).serialize()
    }

    #[test]
    fn process_ipv4_packet_rewrites_udp_tuple_and_checksums() {
        let manager = NetfilterManager::new();
        manager.init();

        let mut dnat = IptEntry::new();
        dnat.src_mask = 0;
        dnat.dst_mask = 0;
        dnat.target = IptTarget::dnat(Ipv4Addr::new(10, 0, 2, 42).to_u32(), 5353);
        manager
            .add_rule(IPTABLES_NAT_TABLE, "PREROUTING", dnat)
            .unwrap();

        let mut mangle = IptEntry::new();
        mangle.src_mask = 0;
        mangle.dst_mask = 0;
        mangle.target = IptTarget::ttl(48);
        manager
            .add_rule(IPTABLES_MANGLE_TABLE, "PREROUTING", mangle)
            .unwrap();

        let mut packet = build_udp_ipv4_packet(
            Ipv4Addr::new(10, 0, 2, 15),
            Ipv4Addr::new(8, 8, 8, 8),
            12345,
            53,
            b"dns",
        );
        let verdict = manager
            .process_ipv4_packet(&mut packet, NF_INET_PRE_ROUTING, Some("eth0"), Some("eth0"))
            .unwrap();
        assert_eq!(verdict, NF_ACCEPT);

        let parsed = Ipv4Packet::parse(&packet).unwrap();
        assert_eq!(parsed.header.dst, Ipv4Addr::new(10, 0, 2, 42));
        assert_eq!(parsed.header.ttl, 48);
        assert_eq!(
            parsed.header.checksum,
            u16::from_be_bytes([packet[10], packet[11]])
        );
        let udp = UdpHeader::parse(parsed.payload).unwrap();
        assert_eq!(udp.dst_port.0, 5353);
        assert!(udp::verify_checksum(
            parsed.header.src,
            parsed.header.dst,
            parsed.payload
        ));
    }

    #[test]
    fn process_ipv4_packet_postrouting_snat_rewrites_source_tuple() {
        let manager = NetfilterManager::new();
        manager.init();

        let mut snat = IptEntry::new();
        snat.src_mask = 0;
        snat.dst_mask = 0;
        snat.target = IptTarget::snat(Ipv4Addr::new(10, 0, 2, 99).to_u32(), 40000);
        manager
            .add_rule(IPTABLES_NAT_TABLE, "POSTROUTING", snat)
            .unwrap();

        let mut packet = build_udp_ipv4_packet(
            Ipv4Addr::new(10, 0, 2, 15),
            Ipv4Addr::new(1, 1, 1, 1),
            12345,
            53,
            b"udp",
        );
        let verdict = manager
            .process_ipv4_packet(&mut packet, NF_INET_POST_ROUTING, Some("eth0"), Some("eth0"))
            .unwrap();
        assert_eq!(verdict, NF_ACCEPT);

        let parsed = Ipv4Packet::parse(&packet).unwrap();
        assert_eq!(parsed.header.src, Ipv4Addr::new(10, 0, 2, 99));
        let udp = UdpHeader::parse(parsed.payload).unwrap();
        assert_eq!(udp.src_port.0, 40000);
        assert!(udp::verify_checksum(
            parsed.header.src,
            parsed.header.dst,
            parsed.payload
        ));
    }

    #[test]
    fn process_ipv6_packet_rewrites_udp_tuple_and_checksums() {
        let manager = NetfilterManager::new();
        manager.init();

        let rewritten_dst = Ipv6Addr::from_segments([0x2001, 0xdb8, 0, 0, 0, 0, 0, 0x99]);
        let rewritten_src = Ipv6Addr::from_segments([0x2001, 0xdb8, 0, 0, 0, 0, 0, 0x77]);

        let mut dnat = IptEntry::new();
        dnat.family = NFPROTO_IPV6;
        dnat.src_mask6 = [0; 16];
        dnat.dst_mask6 = [0; 16];
        dnat.target = IptTarget::dnat_v6(rewritten_dst, 5353);
        manager
            .add_rule(IPTABLES_NAT_TABLE, "PREROUTING", dnat)
            .unwrap();

        let mut mangle = IptEntry::new();
        mangle.family = NFPROTO_IPV6;
        mangle.src_mask6 = [0; 16];
        mangle.dst_mask6 = [0; 16];
        mangle.target = IptTarget::ttl(48);
        manager
            .add_rule(IPTABLES_MANGLE_TABLE, "PREROUTING", mangle)
            .unwrap();

        let mut snat = IptEntry::new();
        snat.family = NFPROTO_IPV6;
        snat.src_mask6 = [0; 16];
        snat.dst_mask6 = [0; 16];
        snat.target = IptTarget::snat_v6(rewritten_src, 4242);
        manager
            .add_rule(IPTABLES_NAT_TABLE, "POSTROUTING", snat)
            .unwrap();

        let mut packet = build_udp_ipv6_packet(
            Ipv6Addr::from_segments([0x2001, 0xdb8, 0, 0, 0, 0, 0, 0x15]),
            Ipv6Addr::from_segments([0x2001, 0xdb8, 0, 0, 0, 0, 0, 0x53]),
            12345,
            53,
            b"dns6",
        );
        let verdict = manager
            .process_ipv6_packet(&mut packet, NF_INET_PRE_ROUTING, Some("eth0"), Some("eth0"))
            .unwrap();
        assert_eq!(verdict, NF_ACCEPT);

        let mut parsed = Ipv6Packet::parse(&packet).unwrap();
        assert_eq!(parsed.header.dst, rewritten_dst);
        assert_eq!(parsed.header.hop_limit, 48);
        let udp = UdpHeader::parse(&parsed.payload).unwrap();
        assert_eq!(udp.dst_port.0, 5353);
        assert!(udp::verify_checksum_v6(
            parsed.header.src,
            parsed.header.dst,
            &parsed.payload
        ));

        let verdict = manager
            .process_ipv6_packet(
                &mut packet,
                NF_INET_POST_ROUTING,
                Some("eth0"),
                Some("eth0"),
            )
            .unwrap();
        assert_eq!(verdict, NF_ACCEPT);

        parsed = Ipv6Packet::parse(&packet).unwrap();
        assert_eq!(parsed.header.src, rewritten_src);
        let udp = UdpHeader::parse(&parsed.payload).unwrap();
        assert_eq!(udp.src_port.0, 4242);
        assert!(udp::verify_checksum_v6(
            parsed.header.src,
            parsed.header.dst,
            &parsed.payload
        ));
    }
}

// ============================================================================
// CONNTRACK — Bağlantı Takibi (Connection Tracking)
// ============================================================================
//
// Conntrack, netfilter'ın datagram-tabanlı IP trafiğini mantıksal bağlantılar
// olarak modellemesini sağlar. NAT, stateful firewall ve rate limiting için
// temel altyapıdır.

/// Bağlantı durumu
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnState {
    /// Yeni bağlantı (ilk paket görüldü)
    New,
    /// Bağlantı kuruldu (çift yönlü trafik)
    Established,
    /// İlişkili bağlantı (FTP data vs.)
    Related,
    /// Geçersiz paket
    Invalid,
    /// Bağlantı kapanıyor (FIN/RST görüldü)
    Closing,
    /// Zaman aşımı ile silindi
    TimedOut,
}

/// Bağlantı yönü
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnDirection {
    /// Orijinal yön (istemci → sunucu)
    Original,
    /// Yanıt yönü (sunucu → istemci)
    Reply,
}

/// L4 protokol bilgisi
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnProto {
    Tcp {
        src_port: u16,
        dst_port: u16,
        /// TCP durum takibi
        tcp_state: ConnTcpState,
    },
    Udp {
        src_port: u16,
        dst_port: u16,
    },
    Icmp {
        icmp_type: u8,
        icmp_code: u8,
        icmp_id: u16,
    },
    Other(u8),
}

/// Conntrack TCP durum makinesi
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnTcpState {
    None,
    SynSent,
    SynRecv,
    Established,
    FinWait,
    CloseWait,
    LastAck,
    TimeWait,
    Close,
}

/// Tek bir bağlantı takip kaydı
#[derive(Debug, Clone)]
pub struct ConntrackEntry {
    /// Benzersiz bağlantı ID
    pub id: u64,
    /// Kaynak IP (IPv4 olarak u32, big-endian)
    pub src_ip: u32,
    /// Hedef IP
    pub dst_ip: u32,
    /// Protokol bilgisi
    pub proto: ConnProto,
    /// Bağlantı durumu
    pub state: ConnState,
    /// Orijinal yöndeki paket sayısı
    pub packets_orig: u64,
    /// Yanıt yönündeki paket sayısı
    pub packets_reply: u64,
    /// Orijinal yöndeki bayt sayısı
    pub bytes_orig: u64,
    /// Yanıt yönündeki bayt sayısı
    pub bytes_reply: u64,
    /// Oluşturma zamanı (TSC)
    pub created_tsc: u64,
    /// Son paket zamanı (TSC)
    pub last_seen_tsc: u64,
    /// Zaman aşımı süresi (TSC tick)
    pub timeout_ticks: u64,
    /// NAT bilgisi (varsa)
    pub nat: Option<ConnNat>,
    /// Mark (nfmark)
    pub mark: u32,
}

/// NAT çeviri bilgisi
#[derive(Debug, Clone, Copy)]
pub struct ConnNat {
    /// Çevrilmiş kaynak IP
    pub translated_src: u32,
    /// Çevrilmiş hedef IP
    pub translated_dst: u32,
    /// Çevrilmiş kaynak port
    pub translated_sport: u16,
    /// Çevrilmiş hedef port
    pub translated_dport: u16,
}

/// Conntrack tablosu — tüm aktif bağlantıları tutar
pub struct ConntrackTable {
    /// Bağlantılar (ID → Entry)
    entries: Mutex<BTreeMap<u64, ConntrackEntry>>,
    /// Sonraki ID
    next_id: AtomicU64,
    /// Toplam kayıt limiti
    max_entries: usize,
    /// TCP timeout (saniye cinsinden TSC — varsayılan 5 dakika)
    tcp_timeout: u64,
    /// UDP timeout
    udp_timeout: u64,
    /// ICMP timeout
    icmp_timeout: u64,
    /// Aktif mi
    enabled: AtomicBool,
}

impl ConntrackTable {
    pub const fn new() -> Self {
        Self {
            entries: Mutex::new(BTreeMap::new()),
            next_id: AtomicU64::new(1),
            max_entries: 65536,
            tcp_timeout: 300_000_000_000, // ~5 dakika @ 1GHz TSC
            udp_timeout: 30_000_000_000,  // ~30 saniye
            icmp_timeout: 30_000_000_000, // ~30 saniye
            enabled: AtomicBool::new(true),
        }
    }

    /// Yeni bağlantı kaydeder veya mevcut kaydı günceller.
    pub fn track_packet(
        &self,
        src_ip: u32,
        dst_ip: u32,
        proto: ConnProto,
        current_tsc: u64,
        pkt_len: u64,
    ) -> (u64, ConnState) {
        if !self.enabled.load(Ordering::Relaxed) {
            return (0, ConnState::Invalid);
        }

        let mut entries = self.entries.lock();

        // Mevcut bağlantı ara (kaynak→hedef yönünde)
        for entry in entries.values_mut() {
            if entry.src_ip == src_ip
                && entry.dst_ip == dst_ip
                && Self::proto_match(&entry.proto, &proto)
            {
                // Orijinal yönde paket
                entry.packets_orig += 1;
                entry.bytes_orig += pkt_len;
                entry.last_seen_tsc = current_tsc;
                Self::advance_tcp_state(&mut entry.proto, &proto, ConnDirection::Original);
                if entry.state == ConnState::New && entry.packets_reply > 0 {
                    entry.state = ConnState::Established;
                }
                return (entry.id, entry.state);
            }
            if entry.src_ip == dst_ip
                && entry.dst_ip == src_ip
                && Self::proto_match_reverse(&entry.proto, &proto)
            {
                // Yanıt yönünde paket
                entry.packets_reply += 1;
                entry.bytes_reply += pkt_len;
                entry.last_seen_tsc = current_tsc;
                Self::advance_tcp_state(&mut entry.proto, &proto, ConnDirection::Reply);
                if entry.state == ConnState::New {
                    entry.state = ConnState::Established;
                }
                return (entry.id, entry.state);
            }
        }

        // Yeni bağlantı
        if entries.len() >= self.max_entries {
            return (0, ConnState::Invalid);
        }

        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let timeout = match proto {
            ConnProto::Tcp { .. } => self.tcp_timeout,
            ConnProto::Udp { .. } => self.udp_timeout,
            ConnProto::Icmp { .. } => self.icmp_timeout,
            ConnProto::Other(_) => self.udp_timeout,
        };
        let entry = ConntrackEntry {
            id,
            src_ip,
            dst_ip,
            proto,
            state: ConnState::New,
            packets_orig: 1,
            packets_reply: 0,
            bytes_orig: pkt_len,
            bytes_reply: 0,
            created_tsc: current_tsc,
            last_seen_tsc: current_tsc,
            timeout_ticks: timeout,
            nat: None,
            mark: 0,
        };
        entries.insert(id, entry);
        (id, ConnState::New)
    }

    /// Protokol eşleştirme (aynı yön).
    fn proto_match(existing: &ConnProto, incoming: &ConnProto) -> bool {
        match (existing, incoming) {
            (
                ConnProto::Tcp {
                    src_port: s1,
                    dst_port: d1,
                    ..
                },
                ConnProto::Tcp {
                    src_port: s2,
                    dst_port: d2,
                    ..
                },
            ) => s1 == s2 && d1 == d2,
            (
                ConnProto::Udp {
                    src_port: s1,
                    dst_port: d1,
                },
                ConnProto::Udp {
                    src_port: s2,
                    dst_port: d2,
                },
            ) => s1 == s2 && d1 == d2,
            (ConnProto::Icmp { icmp_id: id1, .. }, ConnProto::Icmp { icmp_id: id2, .. }) => {
                id1 == id2
            }
            _ => false,
        }
    }

    /// Ters yön eşleştirme.
    fn proto_match_reverse(existing: &ConnProto, incoming: &ConnProto) -> bool {
        match (existing, incoming) {
            (
                ConnProto::Tcp {
                    src_port: s1,
                    dst_port: d1,
                    ..
                },
                ConnProto::Tcp {
                    src_port: s2,
                    dst_port: d2,
                    ..
                },
            ) => s1 == d2 && d1 == s2,
            (
                ConnProto::Udp {
                    src_port: s1,
                    dst_port: d1,
                },
                ConnProto::Udp {
                    src_port: s2,
                    dst_port: d2,
                },
            ) => s1 == d2 && d1 == s2,
            (ConnProto::Icmp { icmp_id: id1, .. }, ConnProto::Icmp { icmp_id: id2, .. }) => {
                id1 == id2
            }
            _ => false,
        }
    }

    /// TCP durum geçişini ilerletir.
    fn advance_tcp_state(existing: &mut ConnProto, _incoming: &ConnProto, _dir: ConnDirection) {
        if let ConnProto::Tcp { tcp_state, .. } = existing {
            match *tcp_state {
                ConnTcpState::None => *tcp_state = ConnTcpState::SynSent,
                ConnTcpState::SynSent => *tcp_state = ConnTcpState::SynRecv,
                ConnTcpState::SynRecv => *tcp_state = ConnTcpState::Established,
                ConnTcpState::Established => {}
                ConnTcpState::FinWait => *tcp_state = ConnTcpState::CloseWait,
                ConnTcpState::CloseWait => *tcp_state = ConnTcpState::LastAck,
                ConnTcpState::LastAck => *tcp_state = ConnTcpState::TimeWait,
                ConnTcpState::TimeWait => *tcp_state = ConnTcpState::Close,
                ConnTcpState::Close => {}
            }
        }
    }

    /// Zaman aşımına uğramış bağlantıları temizler.
    pub fn gc(&self, current_tsc: u64) -> usize {
        let mut entries = self.entries.lock();
        let before = entries.len();
        entries.retain(|_, e| current_tsc.saturating_sub(e.last_seen_tsc) < e.timeout_ticks);
        before - entries.len()
    }

    /// Toplam bağlantı sayısı.
    pub fn count(&self) -> usize {
        self.entries.lock().len()
    }

    /// Tüm bağlantıları listeler.
    pub fn list(&self) -> Vec<ConntrackEntry> {
        self.entries.lock().values().cloned().collect()
    }

    /// NAT bilgisi atar.
    pub fn set_nat(&self, conn_id: u64, nat: ConnNat) {
        if let Some(entry) = self.entries.lock().get_mut(&conn_id) {
            entry.nat = Some(nat);
        }
    }
}

lazy_static::lazy_static! {
    pub static ref CONNTRACK: ConntrackTable = ConntrackTable::new();
}
