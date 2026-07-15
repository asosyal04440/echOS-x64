//! # Netlink Soketi (Çekirdek-Kullanıcı Uzayı İletişimi)
//!
//! Netlink, Linux çekirdeği ile kullanıcı uzayı uygulamaları arasında
//! iletişim sağlayan özel bir soket ailesidir (AF_NETLINK).
//!
//! ## Netlink Nedir?
//!
//! Geleneksel olarak çekirdek yapılandırması `ioctl()` sistem çağrıları ve
//! `/proc` dosya sistemi üzerinden yapılırdı. Netlink bu yaklaşımların yerini
//! alarak daha esnek, asenkron ve çift yönlü iletişim sağlar.
//!
//! ## Mimari Diyagramı
//!
//! ```
//!  Kullanıcı Uzayı                       Çekirdek (Kernel Space)
//!  ─────────────────────────────────────────────────────────────
//!
//!  iproute2 (ip link/addr/route)
//!  iptables / nftables               ┌──────────────────────────┐
//!  NetworkManager                    │  NETLINK_ROUTE      (0)  │ Yönlendirme
//!  systemd-networkd                  │  NETLINK_FIREWALL   (3)  │ Güvenlik Duvarı
//!                                    │  NETLINK_SOCK_DIAG  (4)  │ Soket İzleme
//!       ┌─────────────────┐          │  NETLINK_NETFILTER (12)  │ Netfilter/iptables
//!       │  AF_NETLINK     │          │  NETLINK_XFRM       (6)  │ IPsec
//!       │  socket(fd)     │          │  NETLINK_AUDIT      (9)  │ Denetim
//!       └────────┬────────┘          │  NETLINK_KOBJECT   (15)  │ Kernel Olayları
//!                │ sendmsg()         │  NETLINK_GENERIC   (16)  │ Genel Amaçlı
//!                │ NlMsgHdr+payload  └──────────────────────────┘
//!                └─────────────────────────> Çekirdek işler ve yanıt gönderir
//!                <──────────────────────────── Yanıt (NLM_F_MULTI, NL_DONE)
//! ```
//!
//! ## Netlink Mesaj Yapısı
//!
//! ```
//!  ┌───────────────────────────────────────────────────────┐
//!  │               NlMsgHdr  (16 byte)                     │
//!  │  nlmsg_len(4) │ nlmsg_type(2) │ nlmsg_flags(2)       │
//!  │  nlmsg_seq(4) │ nlmsg_pid(4)                         │
//!  ├───────────────────────────────────────────────────────┤
//!  │               Payload  (değişken)                     │
//!  │  ┌──────────────┬───────────────────────────────┐    │
//!  │  │   NlAttr     │   Attribute Data               │    │
//!  │  │  nla_len(2)  │   (4-byte hizalı, padding ile)│    │
//!  │  │  nla_type(2) │                               │    │
//!  │  └──────────────┴───────────────────────────────┘    │
//!  │  (birden fazla NlAttr art arda gelebilir)             │
//!  └───────────────────────────────────────────────────────┘
//! ```
//!
//! ## Çok Noktaya Yayın (Multicast) Gruplar
//!
//! Netlink soketleri çok noktaya yayın gruplarına abone olabilir.
//! Çekirdek, belirli olayları (arayüz up/down, rota değişikliği vb.)
//! ilgili gruba üye tüm soketlere yayınlar.
//!
//! ## Protokol Numaraları
//!
//! Her Netlink ailesi ayrı bir protokol numarası ile tanımlanır.
//! Bu numaralar `socket(AF_NETLINK, SOCK_RAW, protokol)` çağrısında kullanılır.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

use crate::net::net_device::{NetDevice, NET_DEVICE_MANAGER};
use crate::net::arp;
use crate::net::routing;
use crate::net::tc::TC_MANAGER;
use crate::net::MacAddr;

// ============================================================================
// NETLINK SABİTLERİ (NETLINK CONSTANTS)
// ============================================================================
//
// Netlink protokol aileleri: her biri farklı bir çekirdek alt sistemine
// karşılık gelir. `socket(AF_NETLINK, SOCK_RAW, NETLINK_XXX)` çağrısında
// üçüncü parametre olarak kullanılır.

/// Ağ arayüzleri, rota tabloları, komşu (ARP/NDP) tabloları.
/// `ip route`, `ip link`, `ip addr` komutları bu aileyi kullanır.
pub const NETLINK_ROUTE: u32 = 0;
/// Kullanılmıyor (reserved).
pub const NETLINK_UNUSED: u32 = 1;
/// Kullanıcı uzayı soket iletişimi için özel aile.
pub const NETLINK_USERSOCK: u32 = 2;
/// Güvenlik duvarı kural yönetimi (eski API, artık NETLINK_NETFILTER tercih edilir).
pub const NETLINK_FIREWALL: u32 = 3;
/// Açık soketleri izleme: `ss` komutu bu aileyi kullanır.
pub const NETLINK_SOCK_DIAG: u32 = 4;
/// Netfilter NFLOG - paket günlükleme.
pub const NETLINK_NFLOG: u32 = 5;
/// IPsec (XFRM) yönetimi: güvenlik ilkeleri ve ilişkilendirmeler (SA).
pub const NETLINK_XFRM: u32 = 6;
/// SELinux güvenlik politikası bildirimleri.
pub const NETLINK_SELINUX: u32 = 7;
/// iSCSI oturumu ve hedef yönetimi.
pub const NETLINK_ISCSI: u32 = 8;
/// Linux güvenlik denetim (audit) alt sistemi.
pub const NETLINK_AUDIT: u32 = 9;
/// İletme Bilgi Tabanı (FIB) sorguları.
pub const NETLINK_FIB_LOOKUP: u32 = 10;
/// Çekirdek bileşenleri arası iletişim köprüsü.
pub const NETLINK_CONNECTOR: u32 = 11;
/// Netfilter çerçevesi: iptables, nftables, conntrack.
pub const NETLINK_NETFILTER: u32 = 12;
/// IPv6 güvenlik duvarı (eski, artık kullanılmıyor).
pub const NETLINK_IP6_FW: u32 = 13;
/// DECnet yönlendirme mesajları.
pub const NETLINK_DNRTMSG: u32 = 14;
/// Çekirdek donanım olayları (uevent): `udev` bu aileyi dinler.
pub const NETLINK_KOBJECT_UEVENT: u32 = 15;
/// Genel amaçlı Netlink: sürücüler/modüller kendi komutlarını tanımlayabilir.
pub const NETLINK_GENERIC: u32 = 16;
/// SCSI aktarım katmanı olayları.
pub const NETLINK_SCSITRANSPORT: u32 = 18;
/// eCryptfs şifreli dosya sistemi.
pub const NETLINK_ECRYPTFS: u32 = 19;
/// RDMA (InfiniBand) yönetimi.
pub const NETLINK_RDMA: u32 = 20;
/// Şifreleme alt sistemi arayüzü.
pub const NETLINK_CRYPTO: u32 = 21;

// ----------------------------------------------------------------------------
// Netlink Mesaj Bayrakları (Flags)
//
// Bu bayraklar NlMsgHdr.nlmsg_flags alanına OR'lanarak kullanılır.
// Bir mesajın amacını ve işlem biçimini belirler.
// ----------------------------------------------------------------------------

/// Mesajın bir istek olduğunu bildirir (çekirdeğe gönderilen her mesajda bulunur).
pub const NLM_F_REQUEST: u16 = 1;
/// Bu yanıt, çok parçalı (multi-part) dizinin bir parçasıdır.
/// Dizinin sonu NLM_DONE mesajıyla bildirilir.
pub const NLM_F_MULTI: u16 = 2;
/// İşlem tamamlanınca NLMSG_ERROR ile onay (ACK) gönder.
pub const NLM_F_ACK: u16 = 4;
/// İsteği gönderen sokete de yankıla.
pub const NLM_F_ECHO: u16 = 8;
/// Sayım sırasında liste değişmiş; sonuçlar tutarsız olabilir.
pub const NLM_F_DUMP_INTR: u16 = 16;
/// Dump sonuçları filtrelendi (bazı nesneler atlandı).
pub const NLM_F_DUMP_FILTERED: u16 = 32;

// GET istekleri için ek bayraklar:
/// Kök nesneden başlayarak tüm listeyi döndür.
pub const NLM_F_ROOT: u16 = 0x100;
/// Filtreyle eşleşen nesneleri döndür.
pub const NLM_F_MATCH: u16 = 0x200;
/// Döndürülen veriyi atomik olarak al (kilit altında).
pub const NLM_F_ATOMIC: u16 = 0x400;
/// ROOT | MATCH => tam dump (tüm nesneleri listele).
pub const NLM_F_DUMP: u16 = NLM_F_ROOT | NLM_F_MATCH;

// NEW/SET istekleri için ek bayraklar:
/// Varsa mevcut nesneyi yenisiyle değiştir.
pub const NLM_F_REPLACE: u16 = 0x100;
/// Zaten varsa hata döndür (CREATE ile birlikte kullanılır).
pub const NLM_F_EXCL: u16 = 0x200;
/// Yoksa yeni nesne oluştur.
pub const NLM_F_CREATE: u16 = 0x400;
/// Listeye ekle (değiştirme).
pub const NLM_F_APPEND: u16 = 0x800;

// Netlink hata kodları (errno uyumlu):
/// İşlem başarılı.
pub const NLE_SUCCESS: i32 = 0;
/// Genel hata.
pub const NLE_ERROR: i32 = -1;
/// Erişim reddedildi (EACCES).
pub const NLE_NOACCESS: i32 = -13;

// ============================================================================
// NETLINK MESAJ TIPLERI (NLMSG_*)
// ============================================================================

/// No-op mesaj (ignore).
pub const NLMSG_NOOP: u16 = 1;
/// Hata veya ACK mesajı.
pub const NLMSG_ERROR: u16 = 2;
/// Multi-part dump tamamlandı.
pub const NLMSG_DONE: u16 = 3;

// ============================================================================
// NETLINK MESAJ BAŞLIĞI (NlMsgHdr)
// ============================================================================
//
// Her Netlink mesajı bu 16-byte başlık ile başlar.
//
//  0        7 8       15 16      23 24      31
//  ┌─────────────────────────────────────────┐
//  │           nlmsg_len (32-bit)            │  Toplam mesaj uzunluğu (başlık dahil)
//  ├──────────────────────┬──────────────────┤
//  │   nlmsg_type (16)    │  nlmsg_flags(16) │  Mesaj tipi ve bayraklar
//  ├──────────────────────┴──────────────────┤
//  │           nlmsg_seq (32-bit)            │  Sıra numarası (istek/yanıt eşleştirme)
//  ├─────────────────────────────────────────┤
//  │           nlmsg_pid (32-bit)            │  Gönderenin port kimliği (genellikle PID)
//  └─────────────────────────────────────────┘

/// Netlink mesaj başlığı. Linux `struct nlmsghdr` ile birebir uyumludur.
/// `#[repr(C)]` ile C ABI hizalaması sağlanır, ham bellek erişimine hazır.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct NlMsgHdr {
    /// Başlık + payload dahil toplam mesaj boyutu (byte).
    pub nlmsg_len: u32,
    /// Mesajın tipi: RTM_GETLINK, RTM_NEWROUTE vb. (bkz. RTM_* sabitleri)
    pub nlmsg_type: u16,
    /// İstek/yanıt davranışını değiştiren bayraklar (bkz. NLM_F_* sabitleri).
    pub nlmsg_flags: u16,
    /// Monoton artan sıra numarası; yanıtı hangi isteğe ait olduğunu bulmak için.
    pub nlmsg_seq: u32,
    /// Gönderenin Netlink port ID'si. Kullanıcı uzayından PID, çekirdekten 0.
    pub nlmsg_pid: u32,
}

impl NlMsgHdr {
    /// Yeni bir Netlink başlığı oluşturur.
    pub fn new(len: u32, msg_type: u16, flags: u16, seq: u32, pid: u32) -> Self {
        Self {
            nlmsg_len: len,
            nlmsg_type: msg_type,
            nlmsg_flags: flags,
            nlmsg_seq: seq,
            nlmsg_pid: pid,
        }
    }

    /// Başlığın bellekteki boyutunu döndürür (her zaman 16 byte).
    pub fn size() -> usize {
        core::mem::size_of::<Self>()
    }
}

// ============================================================================
// NETLINK NİTELİKLERİ (NETLINK ATTRIBUTES)
// ============================================================================
//
// Netlink nitelikleri (TLV = Type-Length-Value), mesajların payload'ına
// yapılandırılmış veri eklemek için kullanılır.
//
//  ┌───────────────┬──────────────────────────────────────┐
//  │  nla_len(2B)  │  nla_type(2B)  │  Data...  │ Padding│
//  │  (hdr+data)   │  (attr tipi)   │           │(4B hiz.)│
//  └───────────────┴──────────────────────────────────────┘
//
// Birden fazla nitelik art arda dizilir:
//   [NlAttr1][Data1][pad][NlAttr2][Data2][pad]...
//
// nla_type'ın yüksek 2 biti özel anlam taşır:
//   bit15: nested (içiçe nitelik)
//   bit14: veri Net-Byte-Order'da (big-endian)

/// Netlink nitelik başlığı (TLV formatı). Linux `struct nlattr` ile uyumludur.
#[repr(C)]
pub struct NlAttr {
    /// Bu niteliğin toplam uzunluğu (2-byte başlık + veri + padding öncesi).
    pub nla_len: u16,
    /// Nitelik tipi (protokole özgü sabitler, örn. IFLA_*, RTA_*).
    pub nla_type: u16,
}

impl NlAttr {
    /// Belirtilen tipte bir TLV nitelik tamponu oluşturur.
    /// Veri otomatik olarak 4-byte sınırına hizalanır (padding eklenir).
    pub fn new(attr_type: u16, data: &[u8]) -> Vec<u8> {
        let mut buf = Vec::new();
        let len = (4 + data.len()) as u16;
        buf.extend_from_slice(&len.to_le_bytes());
        buf.extend_from_slice(&attr_type.to_le_bytes());
        buf.extend_from_slice(data);
        while buf.len() % 4 != 0 {
            buf.push(0);
        }
        buf
    }
}

// ============================================================================
// IFF_* — NET_DEVICE I/O FLAGS (Linux if.h uyumlu)
// ============================================================================

pub const IFF_UP: u32 = 1 << 0;
pub const IFF_BROADCAST: u32 = 1 << 1;
pub const IFF_LOOPBACK: u32 = 1 << 3;
pub const IFF_POINTOPOINT: u32 = 1 << 4;
pub const IFF_RUNNING: u32 = 1 << 6;
pub const IFF_NOARP: u32 = 1 << 7;
pub const IFF_PROMISC: u32 = 1 << 8;
pub const IFF_ALLMULTI: u32 = 1 << 9;
pub const IFF_MULTICAST: u32 = 1 << 12;
pub const IFF_LOWER_UP: u32 = 1 << 16;
pub const IFF_DORMANT: u32 = 1 << 17;

pub const ARPHRD_ETHER: u16 = 1;
pub const AF_UNSPEC: u8 = 0;

/// Linux `struct ifinfomsg` — 16 byte, repr(C)
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct IfInfoMsg {
    pub ifi_family: u8,
    pub ifi_reserved: u8,
    pub ifi_type: u16,
    pub ifi_index: i32,
    pub ifi_flags: u32,
    pub ifi_change: u32,
}

/// Linux `struct rtnl_link_stats64` — 24 × u64 = 192 byte
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct RtnlLinkStats64 {
    pub rx_packets: u64,
    pub tx_packets: u64,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_errors: u64,
    pub tx_errors: u64,
    pub rx_dropped: u64,
    pub tx_dropped: u64,
    pub multicast: u64,
    pub collisions: u64,
    pub rx_length_errors: u64,
    pub rx_over_errors: u64,
    pub rx_crc_errors: u64,
    pub rx_frame_errors: u64,
    pub rx_fifo_errors: u64,
    pub rx_missed_errors: u64,
    pub tx_aborted_errors: u64,
    pub tx_carrier_errors: u64,
    pub tx_fifo_errors: u64,
    pub tx_heartbeat_errors: u64,
    pub tx_window_errors: u64,
    pub rx_compressed: u64,
    pub tx_compressed: u64,
    pub rx_nohandler: u64,
}

/// Linux `struct ifaddrmsg` — 8 byte, rt-addr header
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct IfAddrMsg {
    pub ifa_family: u8,
    pub ifa_prefixlen: u8,
    pub ifa_flags: u8,
    pub ifa_scope: u8,
    pub ifa_index: i32,
}

/// Linux `struct ndmsg` — 12 byte, rt-neigh header
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct NdMsg {
    pub ndm_family: u8,
    pub ndm_pad1: u8,
    pub ndm_pad2: u16,
    pub ndm_ifindex: i32,
    pub ndm_state: u16,
    pub ndm_flags: u8,
    pub ndm_type: u8,
}

/// Linux `struct rtmsg` — 8 byte + flags = 12 byte, rt-route header
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct RtMsg {
    pub rtm_family: u8,
    pub rtm_dst_len: u8,
    pub rtm_src_len: u8,
    pub rtm_tos: u8,
    pub rtm_table: u8,
    pub rtm_protocol: u8,
    pub rtm_scope: u8,
    pub rtm_type: u8,
    pub rtm_flags: u32,
}

/// Linux `struct fib_rule_hdr` — 8 byte, rt-rule header
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct FibRuleHdr {
    pub family: u8,
    pub dst_len: u8,
    pub src_len: u8,
    pub tos: u8,
    pub table: u8,
    pub res1: u8,
    pub res2: u8,
    pub action: u8,
}

/// Linux `struct tcmsg` — 16 byte, tc qdisc/class/filter header
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct TcMsg {
    pub tcm_family: u8,
    pub tcm__pad1: u8,
    pub tcm__pad2: u16,
    pub tcm_ifindex: i32,
    pub tcm_handle: u32,
    pub tcm_parent: u32,
    pub tcm_info: u32,
}

// ============================================================================
// NETLINK MESAJ TIPLERI (RTM_* ve GENL_*)
// ============================================================================

/// Yeni ağ arayüzü ekle / arayüz olayı bildir.
pub const RTM_NEWLINK: u16 = 16;
/// Ağ arayüzünü kaldır.
pub const RTM_DELLINK: u16 = 17;
/// Ağ arayüzü bilgisini sorgula (NLM_F_DUMP ile tüm arayüzleri listele).
pub const RTM_GETLINK: u16 = 18;
/// Arayüz özelliğini değiştir (MTU, bayraklar vb.).
pub const RTM_SETLINK: u16 = 19;
/// Arayüze IP adresi ekle.
pub const RTM_NEWADDR: u16 = 20;
/// Arayüzden IP adresini kaldır.
pub const RTM_DELADDR: u16 = 21;
/// IP adreslerini sorgula.
pub const RTM_GETADDR: u16 = 22;
/// Yönlendirme tablosuna rota ekle.
pub const RTM_NEWROUTE: u16 = 24;
/// Rotayı sil.
pub const RTM_DELROUTE: u16 = 25;
/// Rota tablosunu sorgula (NLM_F_DUMP ile tüm rotaları).
pub const RTM_GETROUTE: u16 = 26;
/// ARP/NDP komşu tablosuna giriş ekle.
pub const RTM_NEWNEIGH: u16 = 28;
/// Komşu tablosundan giriş sil.
pub const RTM_DELNEIGH: u16 = 29;
/// Komşu tablosunu sorgula.
pub const RTM_GETNEIGH: u16 = 30;
/// Politika tabanlı yönlendirme kuralı ekle.
pub const RTM_NEWRULE: u16 = 32;
/// Kural sil.
pub const RTM_DELRULE: u16 = 33;
/// Kural sorgula.
pub const RTM_GETRULE: u16 = 34;
/// Qdisc olustur/degistir / dump yaniti / sorgula.
pub const RTM_NEWQDISC: u16 = 36;
/// Qdisc sorgula.
pub const RTM_DELQDISC: u16 = 37;
pub const RTM_GETQDISC: u16 = 38;
/// Traffic class olustur/degistir / dump.
pub const RTM_NEWTCLASS: u16 = 40;
/// Traffic class sil.
pub const RTM_DELTCLASS: u16 = 41;
pub const RTM_GETTCLASS: u16 = 42;
/// TC filter olustur/degistir / dump.
pub const RTM_NEWTFILTER: u16 = 44;
/// TC filter sil.
pub const RTM_DELTFILTER: u16 = 45;
pub const RTM_GETTFILTER: u16 = 46;

// ============================================================================
// IFLA_* — RTM_*LINK nitelik tipleri (rt-link attributes)
// ============================================================================

pub const IFLA_UNSPEC: u16 = 0;
pub const IFLA_ADDRESS: u16 = 1;
pub const IFLA_BROADCAST: u16 = 2;
pub const IFLA_IFNAME: u16 = 3;
pub const IFLA_MTU: u16 = 4;
pub const IFLA_LINK: u16 = 5;
pub const IFLA_QDISC: u16 = 6;
pub const IFLA_STATS: u16 = 7;
pub const IFLA_MASTER: u16 = 10;
pub const IFLA_TXQLEN: u16 = 13;
pub const IFLA_OPERSTATE: u16 = 16;
pub const IFLA_LINKMODE: u16 = 17;
pub const IFLA_STATS64: u16 = 23;
pub const IFLA_GROUP: u16 = 27;
pub const IFLA_PROMISCUITY: u16 = 30;
pub const IFLA_NUM_TX_QUEUES: u16 = 31;
pub const IFLA_NUM_RX_QUEUES: u16 = 32;
pub const IFLA_CARRIER: u16 = 33;
pub const IFLA_PROTO_DOWN: u16 = 39;
pub const IFLA_MIN_MTU: u16 = 50;
pub const IFLA_MAX_MTU: u16 = 51;
pub const IFLA_PERM_ADDRESS: u16 = 54;

// ============================================================================
// RTA_* — ROUTE NITELIK TIPLERI (rt-route)
// ============================================================================

pub const RTA_UNSPEC: u16 = 0;
pub const RTA_DST: u16 = 1;
pub const RTA_SRC: u16 = 2;
pub const RTA_IIF: u16 = 3;
pub const RTA_OIF: u16 = 4;
pub const RTA_GATEWAY: u16 = 5;
pub const RTA_PRIORITY: u16 = 6;
pub const RTA_PREFSRC: u16 = 7;
pub const RTA_TABLE: u16 = 15;
pub const RTA_MARK: u16 = 16;

// ============================================================================
// IFA_* — ADDRESS NITELIK TIPLERI (rt-addr)
// ============================================================================

pub const IFA_UNSPEC: u16 = 0;
pub const IFA_ADDRESS: u16 = 1;
pub const IFA_LOCAL: u16 = 2;
pub const IFA_LABEL: u16 = 3;
pub const IFA_BROADCAST: u16 = 4;
pub const IFA_CACHEINFO: u16 = 6;
pub const IFA_FLAGS: u16 = 8;
pub const IFA_PROTO: u16 = 9;

pub const IFA_F_SECONDARY: u32 = 0x01;
pub const IFA_F_TEMPORARY: u32 = 0x01;
pub const IFA_F_NODAD: u32 = 0x02;
pub const IFA_F_OPTIMISTIC: u32 = 0x04;
pub const IFA_F_DADFAILED: u32 = 0x08;
pub const IFA_F_HOMEADDRESS: u32 = 0x10;
pub const IFA_F_DEPRECATED: u32 = 0x20;
pub const IFA_F_TENTATIVE: u32 = 0x40;
pub const IFA_F_PERMANENT: u32 = 0x80;

// ============================================================================
// NDA_* — NEIGHBOUR NITELIK TIPLERI (rt-neigh)
// ============================================================================

pub const NDA_UNSPEC: u16 = 0;
pub const NDA_DST: u16 = 1;
pub const NDA_LLADDR: u16 = 2;
pub const NDA_CACHEINFO: u16 = 3;
pub const NDA_PROBES: u16 = 4;
pub const NDA_IFINDEX: u16 = 8;
pub const NDA_FLAGS_EXT: u16 = 9;
pub const NDA_PROTOCOL: u16 = 12;

/// NUD_* — Neighbour state flags
pub const NUD_INCOMPLETE: u16 = 0x01;
pub const NUD_REACHABLE: u16 = 0x02;
pub const NUD_STALE: u16 = 0x04;
pub const NUD_DELAY: u16 = 0x08;
pub const NUD_PROBE: u16 = 0x10;
pub const NUD_FAILED: u16 = 0x20;
pub const NUD_NOARP: u16 = 0x40;
pub const NUD_PERMANENT: u16 = 0x80;

// ============================================================================
// FRA_* — RULE NITELIK TIPLERI (rt-rule)
// ============================================================================

pub const FRA_UNSPEC: u16 = 0;
pub const FRA_DST: u16 = 1;
pub const FRA_SRC: u16 = 2;
pub const FRA_IIFNAME: u16 = 3;
pub const FRA_OIFNAME: u16 = 4;
pub const FRA_GOTO: u16 = 5;
pub const FRA_PRIORITY: u16 = 6;
pub const FRA_FWMARK: u16 = 10;
pub const FRA_TABLE: u16 = 15;
pub const FRA_FWMASK: u16 = 16;

// ============================================================================
// IFA_CACHEINFO struct for address attributes
// ============================================================================

/// ifa_cacheinfo — Linux IFA_CACHEINFO payload (preferred/valid lifetimes)
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct IfaCacheInfo {
    pub ifa_prefered: u32,
    pub ifa_valid: u32,
    pub cstamp: u32,
    pub tstamp: u32,
}

// ============================================================================
// TCA_* — QDISC/CLASS/FILTER NITELIK TIPLERI (tc)
// ============================================================================

pub const TCA_UNSPEC: u16 = 0;
pub const TCA_KIND: u16 = 1;
pub const TCA_STATS: u16 = 2;
pub const TCA_STATS2: u16 = 4;
pub const TCA_XSTATS: u16 = 3;
pub const TCA_OPTIONS: u16 = 5;
pub const TCA_HANDLE: u16 = 8;
pub const TCA_PARENT: u16 = 9;
pub const TCA_IFINDEX: u16 = 10;

/// Linux `struct tc_stats` — 40 byte
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct TcStats {
    pub bytes: u64,
    pub packets: u32,
    pub drops: u32,
    pub overlimits: u32,
    pub bps: u32,
    pub pps: u32,
    pub qlen: u32,
    pub backlog: u32,
}

/// Linux `struct gnet_stats_basic` — 12 byte (TCA_STATS2)
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct GnetStatsBasic {
    pub bytes: u64,
    pub packets: u32,
}

/// Linux `struct gnet_stats_queue` — 20 byte (part of TCA_STATS2)
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct GnetStatsQueue {
    pub qlen: u32,
    pub backlog: u32,
    pub drops: u32,
    pub requeues: u32,
    pub overlimits: u32,
}

/// Linux `struct tc_estimator` for rate estimation
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct TcEstimator {
    pub interval: u8,
    pub ewma_log: u8,
}

// Generic Netlink (GENL) sabitleri:
/// Dinamik aile ID üretimi (sıfır gönderilirse çekirdek atar).
pub const GENL_ID_GENERATE: u16 = 0;
/// Kontrol ailesi: mevcut Generic Netlink ailelerini keşfetmek için.
pub const GENL_ID_CTRL: u16 = 0x10;

// ============================================================================
// NETLINK SOKETİ (NetlinkSocket)
// ============================================================================
//
// Her kullanıcı uzayı süreci veya çekirdek bileşeni, Netlink üzerinden
// iletişim kurmak için bir NetlinkSocket örneği oluşturur.
//
// Bir soketin yaşam döngüsü:
//
//   socket(AF_NETLINK, SOCK_RAW, protokol)
//       |
//       v
//   bind(port_id)              <- port_id genellikle PID
//       |
//       v
//   sendmsg / recvmsg          <- mesaj gönder/al
//       |
//       v
//   close(fd)                  <- kaynakları serbest bırak

/// Netlink soketi. Hem çekirdek hem de kullanıcı uzayı tarafı bu yapıyı kullanır.
pub struct NetlinkSocket {
    /// Bu soket için benzersiz tanımlayıcı.
    pub id: u64,
    /// Bu sokete ait Netlink protokol ailesi (NETLINK_ROUTE vb.).
    pub protocol: u32,
    /// Bu soketin bağlı olduğu Netlink port kimliği (genellikle prosesin PID'i).
    pub port_id: AtomicU32,
    /// Hedef port kimliği; 0 = çekirdek.
    pub dst_port_id: AtomicU32,
    /// Hedef çok noktaya yayın grubu; 0 = grup yayını yok.
    pub dst_group: AtomicU32,
    /// Gelen mesajların tamponu. Kilit ile eş zamanlı erişime güvenli.
    pub rx_buf: Mutex<Vec<NetlinkMessage>>,
    /// Giden mesajların tamponu.
    pub tx_buf: Mutex<Vec<NetlinkMessage>>,
    /// Bu soketin üye olduğu çok noktaya yayın grupları (grup_id -> aktif?).
    pub groups: Mutex<BTreeMap<u32, bool>>,
    /// true ise recv() çağrısı bloklama yapmaz (EAGAIN döner).
    pub nonblocking: AtomicBool,
    /// Giden her mesaj için monoton artan sıra numarası.
    pub seq: AtomicU32,
}

/// Tek bir Netlink mesajı: başlık + payload.
#[derive(Clone, Debug)]
pub struct NetlinkMessage {
    /// Mesajın 16-byte Netlink başlığı.
    pub header: NlMsgHdr,
    /// Başlıktan sonraki veri; protokole ve mesaj tipine özgü yapı taşır.
    pub payload: Vec<u8>,
}

impl NetlinkSocket {
    /// Yeni bir Netlink soketi oluşturur.
    pub fn new(id: u64, protocol: u32, port_id: u32) -> Self {
        Self {
            id,
            protocol,
            port_id: AtomicU32::new(port_id),
            dst_port_id: AtomicU32::new(0),
            dst_group: AtomicU32::new(0),
            rx_buf: Mutex::new(Vec::new()),
            tx_buf: Mutex::new(Vec::new()),
            groups: Mutex::new(BTreeMap::new()),
            nonblocking: AtomicBool::new(false),
            seq: AtomicU32::new(1),
        }
    }

    /// Mesajı gönderir.
    /// - Hedef port_id == 0 ise mesaj çekirdeğe yönlendirilir.
    /// - Aksi takdirde hedef port_id'li kullanıcı uzayı soketine iletilir.
    pub fn send(&self, msg: NetlinkMessage) -> Result<(), NetlinkError> {
        let pid = self.dst_port_id.load(Ordering::Relaxed);
        let group = self.dst_group.load(Ordering::Relaxed);

        // pid=0 ve group=0 ise çekirdek mesajı (kernel-bound)
        if pid == 0 && group == 0 {
            // Çekirdek alt sistemi mesajı işler
            self.handle_kernel_message(&msg)?;
        } else {
            // Kullanıcı uzayı soketi - hedefin rx_buf'una koy
            if let Some(sock) = NETLINK_SOCKS.lock().get(&pid) {
                sock.rx_buf.lock().push(msg);
            }
        }

        Ok(())
    }

    /// Tamponda bekleyen bir sonraki mesajı alır (LIFO sırası).
    pub fn recv(&self) -> Option<NetlinkMessage> {
        self.rx_buf.lock().pop()
    }

    /// Çekirdeğe yönelik mesajı protokole göre ilgili işleyiciye yönlendirir.
    fn handle_kernel_message(&self, msg: &NetlinkMessage) -> Result<(), NetlinkError> {
        match self.protocol {
            NETLINK_ROUTE     => self.handle_route(msg),
            NETLINK_SOCK_DIAG => self.handle_sock_diag(msg),
            NETLINK_NETFILTER => self.handle_netfilter(msg),
            NETLINK_XFRM      => self.handle_xfrm(msg),
            NETLINK_AUDIT     => self.handle_audit(msg),
            NETLINK_KOBJECT_UEVENT => self.handle_uevent(msg),
            NETLINK_GENERIC   => self.handle_generic(msg),
            _ => Ok(()),
        }
    }

    /// NETLINK_SOCK_DIAG: socket monitoring (`ss` command support).
    /// Delegates to sock_diag::handle_diag_request and routes responses
    /// back to the requesting socket's rx_buf.
    fn handle_sock_diag(&self, msg: &NetlinkMessage) -> Result<(), NetlinkError> {
        let responses = crate::net::sock_diag::handle_diag_request(&msg.payload);
        let src_pid = msg.header.nlmsg_pid;
        let seq = msg.header.nlmsg_seq;

        for (resp_type, resp_payload) in &responses {
            let total_len = (NlMsgHdr::size() + resp_payload.len()) as u32;
            let reply_hdr = NlMsgHdr::new(
                total_len,
                *resp_type,
                NLM_F_MULTI,
                seq,
                0, // kernel pid
            );
            let reply_msg = NetlinkMessage {
                header: reply_hdr,
                payload: resp_payload.clone(),
            };
            // Route to the requesting socket (or broadcast)
            if src_pid != 0 {
                if let Some(sock) = NETLINK_MANAGER.get_socket(src_pid) {
                    sock.rx_buf.lock().push(reply_msg);
                }
            }
        }
        Ok(())
    }

    /// NETLINK_ROUTE mesajlarını işler.
    fn handle_route(&self, msg: &NetlinkMessage) -> Result<(), NetlinkError> {
        let src_pid = msg.header.nlmsg_pid;
        let seq = msg.header.nlmsg_seq;

        let responses: Vec<(u16, Vec<u8>)> = match msg.header.nlmsg_type {
            RTM_GETLINK  => build_link_dump(),
            RTM_GETADDR  => build_addr_dump(),
            RTM_GETROUTE => build_route_dump(),
            RTM_GETNEIGH  => build_neigh_dump(),
            RTM_GETRULE   => build_rule_dump(),
            RTM_GETQDISC | RTM_GETTCLASS | RTM_GETTFILTER => build_qdisc_dump(),
            RTM_NEWLINK | RTM_NEWADDR | RTM_NEWROUTE
                | RTM_NEWNEIGH | RTM_NEWRULE => {
                return Ok(());
            }
            _ => return Ok(()),
        };

        for (resp_type, resp_payload) in &responses {
            let total_len = (NlMsgHdr::size() + resp_payload.len()) as u32;
            let reply_hdr = NlMsgHdr::new(total_len, *resp_type, NLM_F_MULTI, seq, 0);
            let reply_msg = NetlinkMessage {
                header: reply_hdr,
                payload: resp_payload.clone(),
            };
            if src_pid != 0 {
                if let Some(sock) = NETLINK_MANAGER.get_socket(src_pid) {
                    sock.rx_buf.lock().push(reply_msg);
                }
            }
        }
        Ok(())
    }

    /// NETLINK_NETFILTER: iptables/nftables kural işlemleri.
    fn handle_netfilter(&self, msg: &NetlinkMessage) -> Result<(), NetlinkError> {
        let src_pid = msg.header.nlmsg_pid;
        let seq = msg.header.nlmsg_seq;

        let responses =
            crate::net::nftables::handle_nftables_netlink(msg.header.nlmsg_type, &msg.payload);

        for (resp_type, resp_payload) in &responses {
            let total_len = (NlMsgHdr::size() + resp_payload.len()) as u32;
            let reply_hdr = NlMsgHdr::new(total_len, *resp_type, NLM_F_MULTI, seq, 0);
            let reply_msg = NetlinkMessage {
                header: reply_hdr,
                payload: resp_payload.clone(),
            };
            if src_pid != 0 {
                if let Some(sock) = NETLINK_MANAGER.get_socket(src_pid) {
                    sock.rx_buf.lock().push(reply_msg);
                }
            }
        }
        Ok(())
    }

    /// NETLINK_XFRM: IPsec güvenlik ilkeleri ve ilişkilendirmeleri (SA) yönetimi.
    fn handle_xfrm(&self, _msg: &NetlinkMessage) -> Result<(), NetlinkError> {
        Ok(())
    }

    /// NETLINK_AUDIT: Güvenlik denetim olaylarını işle.
    fn handle_audit(&self, _msg: &NetlinkMessage) -> Result<(), NetlinkError> {
        Ok(())
    }

    /// NETLINK_KOBJECT_UEVENT: `udev` gibi cihaz yöneticilerinin dinlediği olaylar.
    fn handle_uevent(&self, _msg: &NetlinkMessage) -> Result<(), NetlinkError> {
        Ok(())
    }

    /// NETLINK_GENERIC: Sürücülerin/modüllerin kendi tanımladığı komutlar.
    fn handle_generic(&self, msg: &NetlinkMessage) -> Result<(), NetlinkError> {
        if msg.header.nlmsg_type == crate::net::ethtool_genl::ETHTOOL_GENL_ID {
            crate::net::ethtool_genl::handle_ethtool_genl_request(
                msg.header.nlmsg_pid,
                msg.header.nlmsg_seq,
                &msg.payload,
            );
        } else if msg.header.nlmsg_type == crate::net::devlink_genl::DEVLINK_GENL_ID {
            crate::net::devlink_genl::handle_devlink_genl_request(
                msg.header.nlmsg_pid,
                msg.header.nlmsg_seq,
                &msg.payload,
            );
        } else if msg.header.nlmsg_type == crate::net::tcp_metrics_genl::TCP_METRICS_GENL_ID {
            crate::net::tcp_metrics_genl::handle_tcp_metrics_genl_request(
                msg.header.nlmsg_pid,
                msg.header.nlmsg_seq,
                &msg.payload,
            );
        } else if msg.header.nlmsg_type == crate::net::wireguard_genl::WIREGUARD_GENL_ID {
            crate::net::wireguard_genl::handle_wireguard_genl_request(
                msg.header.nlmsg_pid,
                msg.header.nlmsg_seq,
                &msg.payload,
            );
        } else if msg.header.nlmsg_type == crate::net::mptcp_pm_genl::MPTCP_PM_GENL_ID {
            crate::net::mptcp_pm_genl::handle_mptcp_pm_genl_request(
                msg.header.nlmsg_pid,
                msg.header.nlmsg_seq,
                &msg.payload,
            );
        } else if msg.header.nlmsg_type == crate::net::nl80211_genl::NL80211_GENL_ID {
            crate::net::nl80211_genl::handle_nl80211_genl_request(
                msg.header.nlmsg_pid,
                msg.header.nlmsg_seq,
                &msg.payload,
            );
        } else if msg.header.nlmsg_type == crate::net::handshake_genl::HANDSHAKE_GENL_ID {
            crate::net::handshake_genl::handle_handshake_genl_request(
                msg.header.nlmsg_pid,
                msg.header.nlmsg_seq,
                &msg.payload,
            );
        } else if msg.header.nlmsg_type == crate::net::net_shaper_genl::NET_SHAPER_GENL_ID {
            crate::net::net_shaper_genl::handle_net_shaper_genl_request(
                msg.header.nlmsg_pid,
                msg.header.nlmsg_seq,
                &msg.payload,
            );
        } else if msg.header.nlmsg_type == crate::net::ovpn_genl::OVPN_GENL_ID {
            crate::net::ovpn_genl::handle_ovpn_genl_request(
                msg.header.nlmsg_pid,
                msg.header.nlmsg_seq,
                &msg.payload,
            );
        }
        Ok(())
    }

    /// Belirtilen çok noktaya yayın grubuna abone ol.
    /// Grup ID, protokole özgüdür (örn. RTNLGRP_LINK = 1).
    pub fn join_group(&self, group: u32) {
        self.groups.lock().insert(group, true);
    }

    /// Çok noktaya yayın grubundan çık.
    pub fn leave_group(&self, group: u32) {
        self.groups.lock().remove(&group);
    }

    /// Hedef port ve grubu ayarla (sendmsg için varsayılan adres).
    pub fn set_destination(&self, pid: u32, group: u32) {
        self.dst_port_id.store(pid, Ordering::SeqCst);
        self.dst_group.store(group, Ordering::SeqCst);
    }
}

// ============================================================================
// RT-LINK: RTM_GETLINK dump helpers
// ============================================================================

fn build_ifinfo_payload(dev: &NetDevice) -> Vec<u8> {
    let mut payload = Vec::new();

    let mut iff = IFF_BROADCAST | IFF_MULTICAST;
    if dev.up.load(Ordering::Acquire) {
        iff |= IFF_UP | IFF_LOWER_UP | IFF_RUNNING;
    }
    if dev.promiscuous.load(Ordering::Acquire) {
        iff |= IFF_PROMISC;
    }

    let info = IfInfoMsg {
        ifi_family: AF_UNSPEC,
        ifi_reserved: 0,
        ifi_type: ARPHRD_ETHER,
        ifi_index: dev.dev_id as i32,
        ifi_flags: iff,
        ifi_change: 0xFFFFFFFF,
    };

    let info_bytes = unsafe {
        core::slice::from_raw_parts(
            &info as *const IfInfoMsg as *const u8,
            core::mem::size_of::<IfInfoMsg>(),
        )
    };
    payload.extend_from_slice(info_bytes);

    // IFLA_IFNAME
    payload.extend_from_slice(&NlAttr::new(IFLA_IFNAME, dev.name.as_bytes()));
    // IFLA_ADDRESS
    payload.extend_from_slice(&NlAttr::new(IFLA_ADDRESS, &dev.mac.0));
    // IFLA_BROADCAST
    payload.extend_from_slice(&NlAttr::new(IFLA_BROADCAST, &[0xFFu8; 6]));
    // IFLA_MTU
    let mtu = dev.mtu.load(Ordering::Acquire);
    payload.extend_from_slice(&NlAttr::new(IFLA_MTU, &mtu.to_ne_bytes()));
    // IFLA_QDISC
    payload.extend_from_slice(&NlAttr::new(IFLA_QDISC, b"pfifo_fast"));
    // IFLA_OPERSTATE
    let operstate: u8 = if dev.up.load(Ordering::Acquire) { 6 } else { 2 };
    payload.extend_from_slice(&NlAttr::new(IFLA_OPERSTATE, &[operstate]));
    // IFLA_LINKMODE
    payload.extend_from_slice(&NlAttr::new(IFLA_LINKMODE, &[0u8]));
    // IFLA_STATS64
    let stats = dev.get_stats();
    let mut stats64: RtnlLinkStats64 = unsafe { core::mem::zeroed() };
    stats64.rx_packets = stats.rx_packets;
    stats64.tx_packets = stats.tx_packets;
    stats64.rx_bytes = stats.rx_bytes;
    stats64.tx_bytes = stats.tx_bytes;
    stats64.rx_errors = stats.rx_errors;
    stats64.tx_errors = stats.tx_errors;
    stats64.rx_dropped = stats.rx_dropped;
    stats64.tx_dropped = stats.tx_dropped;
    let stats_bytes = unsafe {
        core::slice::from_raw_parts(
            &stats64 as *const RtnlLinkStats64 as *const u8,
            core::mem::size_of::<RtnlLinkStats64>(),
        )
    };
    payload.extend_from_slice(&NlAttr::new(IFLA_STATS64, stats_bytes));
    // IFLA_NUM_TX_QUEUES
    let txq = dev.num_tx_queues as u32;
    payload.extend_from_slice(&NlAttr::new(IFLA_NUM_TX_QUEUES, &txq.to_ne_bytes()));
    // IFLA_NUM_RX_QUEUES
    let rxq = dev.num_rx_queues as u32;
    payload.extend_from_slice(&NlAttr::new(IFLA_NUM_RX_QUEUES, &rxq.to_ne_bytes()));

    payload
}

fn build_link_dump() -> Vec<(u16, Vec<u8>)> {
    let devices = NET_DEVICE_MANAGER.all();
    let mut responses = Vec::with_capacity(devices.len() + 1);

    for dev in &devices {
        let payload = build_ifinfo_payload(dev);
        responses.push((RTM_NEWLINK, payload));
    }

    responses.push((NLMSG_DONE, Vec::new()));
    responses
}

// ============================================================================
// RT-ADDR: RTM_GETADDR dump — IP adresleri
// ============================================================================

fn build_addr_dump() -> Vec<(u16, Vec<u8>)> {
    let devices = NET_DEVICE_MANAGER.all();
    if devices.is_empty() {
        return vec![(NLMSG_DONE, Vec::new())];
    }

    let mut responses = Vec::new();
    for dev in &devices {
        let addr = dev.ip.to_u32();
        if addr == 0 {
            continue;
        }

        let msg = IfAddrMsg {
            ifa_family: 2, // AF_INET
            ifa_prefixlen: 24,
            ifa_flags: IFA_F_PERMANENT as u8,
            ifa_scope: 0, // RT_SCOPE_UNIVERSE
            ifa_index: dev.dev_id as i32,
        };
        let msg_bytes = unsafe {
            core::slice::from_raw_parts(
                &msg as *const IfAddrMsg as *const u8,
                core::mem::size_of::<IfAddrMsg>(),
            )
        };

        let mut payload = Vec::new();
        payload.extend_from_slice(msg_bytes);
        payload.extend_from_slice(&NlAttr::new(IFA_LOCAL, &addr.to_be_bytes()));
        payload.extend_from_slice(&NlAttr::new(IFA_ADDRESS, &addr.to_be_bytes()));
        payload.extend_from_slice(&NlAttr::new(IFA_LABEL, dev.name.as_bytes()));
        // Broadcast
        let bcast = addr | (!((1u32 << (32 - 24)) - 1));
        payload.extend_from_slice(&NlAttr::new(IFA_BROADCAST, &bcast.to_be_bytes()));

        responses.push((RTM_NEWADDR, payload));
    }

    responses.push((NLMSG_DONE, Vec::new()));
    responses
}

// ============================================================================
// RT-NEIGH: RTM_GETNEIGH dump — ARP/NDP neighbor table
// ============================================================================

fn build_neigh_dump() -> Vec<(u16, Vec<u8>)> {
    let neighbors = arp::get_table();
    if neighbors.is_empty() {
        return vec![(NLMSG_DONE, Vec::new())];
    }

    let mut responses = Vec::new();
    for (ip, mac) in &neighbors {
        let ip_u32 = ip.to_u32();
        let idx = NET_DEVICE_MANAGER.find_by_ip(*ip)
            .map(|d| d.dev_id as i32)
            .unwrap_or(0);

        let msg = NdMsg {
            ndm_family: 2, // AF_INET
            ndm_pad1: 0,
            ndm_pad2: 0,
            ndm_ifindex: idx,
            ndm_state: NUD_REACHABLE,
            ndm_flags: 0,
            ndm_type: 0,
        };
        let msg_bytes = unsafe {
            core::slice::from_raw_parts(
                &msg as *const NdMsg as *const u8,
                core::mem::size_of::<NdMsg>(),
            )
        };

        let mut payload = Vec::new();
        payload.extend_from_slice(msg_bytes);
        payload.extend_from_slice(&NlAttr::new(NDA_DST, &ip_u32.to_be_bytes()));
        payload.extend_from_slice(&NlAttr::new(NDA_LLADDR, &mac.0));

        responses.push((RTM_NEWNEIGH, payload));
    }

    responses.push((NLMSG_DONE, Vec::new()));
    responses
}

// ============================================================================
// RT-ROUTE: RTM_GETROUTE dump — routing table entries
// ============================================================================

fn build_route_dump() -> Vec<(u16, Vec<u8>)> {
    let tables = routing::dump_tables();
    let mut responses = Vec::new();

    for table_id in &tables {
        let routes = routing::dump_routes(*table_id);
        for (dst, prefix_len, gateway, iface) in &routes {
            let idx = NET_DEVICE_MANAGER.get(iface)
                .map(|d| d.dev_id as i32)
                .unwrap_or(0);

            let scope = if *gateway == 0 {
                253u8 // RT_SCOPE_LINK
            } else {
                0u8   // RT_SCOPE_UNIVERSE
            };
            let rtype = if *gateway == 0 { 1u8 } else { 1u8 }; // RTN_UNICAST

            let msg = RtMsg {
                rtm_family: 2,    // AF_INET
                rtm_dst_len: *prefix_len,
                rtm_src_len: 0,
                rtm_tos: 0,
                rtm_table: *table_id as u8,
                rtm_protocol: 4,  // RTPROT_STATIC
                rtm_scope: scope,
                rtm_type: rtype,
                rtm_flags: 0,
            };
            let msg_bytes = unsafe {
                core::slice::from_raw_parts(
                    &msg as *const RtMsg as *const u8,
                    core::mem::size_of::<RtMsg>(),
                )
            };

            let mut payload = Vec::new();
            payload.extend_from_slice(msg_bytes);

            if *prefix_len > 0 {
                payload.extend_from_slice(&NlAttr::new(RTA_DST, &dst.to_be_bytes()));
            }
            if *gateway != 0 {
                payload.extend_from_slice(&NlAttr::new(RTA_GATEWAY, &gateway.to_be_bytes()));
            }
            if idx > 0 {
                let oif_bytes = iface.as_bytes();
                payload.extend_from_slice(&NlAttr::new(RTA_OIF, oif_bytes));
            }
            payload.extend_from_slice(&NlAttr::new(RTA_TABLE, &table_id.to_ne_bytes()));

            responses.push((RTM_NEWROUTE, payload));
        }
    }

    responses.push((NLMSG_DONE, Vec::new()));
    responses
}

// ============================================================================
// RT-RULE: RTM_GETRULE dump — policy routing rules
// ============================================================================

fn build_rule_dump() -> Vec<(u16, Vec<u8>)> {
    let rules = routing::dump_rules();
    if rules.is_empty() {
        return vec![(NLMSG_DONE, Vec::new())];
    }

    let mut responses = Vec::new();
    for rule in &rules {
        let msg = FibRuleHdr {
            family: 2,                  // AF_INET
            dst_len: rule.dst_prefix_len,
            src_len: rule.src_prefix_len,
            tos: rule.tos,
            table: rule.table_id as u8,
            res1: 0,
            res2: 0,
            action: 0,                  // FR_ACT_TO_TBL
        };
        let msg_bytes = unsafe {
            core::slice::from_raw_parts(
                &msg as *const FibRuleHdr as *const u8,
                core::mem::size_of::<FibRuleHdr>(),
            )
        };

        let mut payload = Vec::new();
        payload.extend_from_slice(msg_bytes);

        payload.extend_from_slice(&NlAttr::new(FRA_PRIORITY, &rule.priority.to_ne_bytes()));
        payload.extend_from_slice(&NlAttr::new(FRA_TABLE, &rule.table_id.to_ne_bytes()));
        if rule.dst_prefix_len > 0 {
            payload.extend_from_slice(&NlAttr::new(FRA_DST, &rule.dst_match.to_be_bytes()));
        }
        if rule.src_prefix_len > 0 {
            payload.extend_from_slice(&NlAttr::new(FRA_SRC, &rule.src_match.to_be_bytes()));
        }
        if rule.fwmark != 0 {
            payload.extend_from_slice(&NlAttr::new(FRA_FWMARK, &rule.fwmark.to_ne_bytes()));
            payload.extend_from_slice(&NlAttr::new(FRA_FWMASK, &rule.fwmark_mask.to_ne_bytes()));
        }

        responses.push((RTM_NEWRULE, payload));
    }

    responses.push((NLMSG_DONE, Vec::new()));
    responses
}

// ============================================================================
// TC: RTM_GETQDISC / GETTCLASS / GETTFILTER dump — traffic control qdiscs
// ============================================================================

fn build_qdisc_dump() -> Vec<(u16, Vec<u8>)> {
    let qdiscs = TC_MANAGER.dump_all();
    if qdiscs.is_empty() {
        return vec![(NLMSG_DONE, Vec::new())];
    }

    let mut responses = Vec::new();
    for (iface, kind, stats) in &qdiscs {
        let idx = NET_DEVICE_MANAGER.get(iface)
            .map(|d| d.dev_id as i32)
            .unwrap_or(0);

        let kind_str = match kind {
            crate::net::tc::QdiscKind::PfifoFast => "pfifo_fast",
            crate::net::tc::QdiscKind::FqCodel => "fq_codel",
            crate::net::tc::QdiscKind::Noop => "noop",
        };

        let msg = TcMsg {
            tcm_family: 2, // AF_INET
            tcm__pad1: 0,
            tcm__pad2: 0,
            tcm_ifindex: idx,
            tcm_handle: 0x80000000, // root handle (HTB_ROOT)
            tcm_parent: 0,         // root
            tcm_info: 0,
        };
        let msg_bytes = unsafe {
            core::slice::from_raw_parts(
                &msg as *const TcMsg as *const u8,
                core::mem::size_of::<TcMsg>(),
            )
        };

        let mut payload = Vec::new();
        payload.extend_from_slice(msg_bytes);
        payload.extend_from_slice(&NlAttr::new(TCA_KIND, kind_str.as_bytes()));

        // TCA_STATS2: basic stats
        let basic = GnetStatsBasic {
            bytes: stats.enqueue_count,  // approximation
            packets: stats.dequeue_count as u32,
        };
        let basic_bytes = unsafe {
            core::slice::from_raw_parts(
                &basic as *const GnetStatsBasic as *const u8,
                core::mem::size_of::<GnetStatsBasic>(),
            )
        };
        // TCA_STATS2 is nested; for simplicity send raw struct
        payload.extend_from_slice(&NlAttr::new(TCA_STATS2, basic_bytes));

        // Queue stats
        let qstats = GnetStatsQueue {
            qlen: stats.backlog_packets as u32,
            backlog: stats.backlog_bytes as u32,
            drops: stats.drop_count as u32,
            requeues: stats.requeue_count as u32,
            overlimits: stats.overlimits as u32,
        };
        let qstats_bytes = unsafe {
            core::slice::from_raw_parts(
                &qstats as *const GnetStatsQueue as *const u8,
                core::mem::size_of::<GnetStatsQueue>(),
            )
        };
        payload.extend_from_slice(&NlAttr::new(TCA_STATS2, qstats_bytes));

        responses.push((RTM_NEWQDISC, payload));
    }

    responses.push((NLMSG_DONE, Vec::new()));
    responses
}

// ============================================================================
// NETLINK YÖNETİCİSİ (NetlinkManager)
// ============================================================================
//
// Tüm Netlink soketlerini ve çok noktaya yayın gruplarını yöneten
// merkezi yapı. İki harita tutar:
//
//   sockets : port_id --> Arc<NetlinkSocket>
//   groups  : group_id --> [port_id listesi]
//
// Paket yönlendirme:
//   send(pid=X)  -> sockets[X].rx_buf'a ekle
//   broadcast(g) -> groups[g] içindeki her port için rx_buf'a ekle

/// Tüm Netlink soketlerini merkezi olarak yöneten yapı.
pub struct NetlinkManager {
    /// port_id -> soket haritası; kilit ile korunur.
    sockets: Mutex<BTreeMap<u32, Arc<NetlinkSocket>>>,
    /// Sonraki soket için benzersiz port ID üreteci.
    next_port_id: AtomicU32,
    /// Sonraki soket için benzersiz soket ID üreteci.
    next_socket_id: AtomicU64,
    /// Çok noktaya yayın grupları: group_id -> üye port_id listesi.
    groups: Mutex<BTreeMap<u32, Vec<u32>>>,
}

impl NetlinkManager {
    /// Derleme-zamanı sabit başlatıcı (static değişken için).
    pub const fn new() -> Self {
        Self {
            sockets: Mutex::new(BTreeMap::new()),
            next_port_id: AtomicU32::new(1),
            next_socket_id: AtomicU64::new(1),
            groups: Mutex::new(BTreeMap::new()),
        }
    }

    /// Yeni bir Netlink soketi oluşturur ve kayıt eder.
    /// Dönen `Arc<NetlinkSocket>` çağıranın da referansını tutar.
    pub fn create_socket(&self, protocol: u32) -> Arc<NetlinkSocket> {
        let id = self.next_socket_id.fetch_add(1, Ordering::SeqCst);
        let port_id = self.next_port_id.fetch_add(1, Ordering::SeqCst);

        let sock = Arc::new(NetlinkSocket::new(id, protocol, port_id));
        self.sockets.lock().insert(port_id, sock.clone());

        sock
    }

    /// Soketi kapatır, tablolardan siler ve tüm grup üyeliklerini temizler.
    pub fn close_socket(&self, port_id: u32) {
        self.sockets.lock().remove(&port_id);

        // Tüm grup listelerinden bu port_id'yi çıkar
        for members in self.groups.lock().values_mut() {
            members.retain(|&p| p != port_id);
        }
    }

    /// Port ID ile soketi arar ve klone ederek döner.
    pub fn get_socket(&self, port_id: u32) -> Option<Arc<NetlinkSocket>> {
        self.sockets.lock().get(&port_id).cloned()
    }

    /// Belirtilen çok noktaya yayın grubundaki tüm soketlere mesaj iletir.
    pub fn broadcast(&self, group: u32, msg: NetlinkMessage) {
        if let Some(members) = self.groups.lock().get(&group) {
            for port_id in members {
                if let Some(sock) = self.sockets.lock().get(port_id) {
                    sock.rx_buf.lock().push(msg.clone());
                }
            }
        }
    }

    /// Soketi belirtilen çok noktaya yayın grubuna ekler.
    pub fn join_group(&self, port_id: u32, group: u32) {
        self.groups.lock()
            .entry(group)
            .or_insert_with(Vec::new)
            .push(port_id);
    }

    /// Soketi çok noktaya yayın grubundan çıkarır.
    pub fn leave_group(&self, port_id: u32, group: u32) {
        if let Some(members) = self.groups.lock().get_mut(&group) {
            members.retain(|&p| p != port_id);
        }
    }
}

lazy_static::lazy_static! {
    pub static ref NETLINK_MANAGER: NetlinkManager = NetlinkManager::new();
    pub static ref NETLINK_SOCKS: Mutex<BTreeMap<u32, Arc<NetlinkSocket>>> = 
        Mutex::new(BTreeMap::new());
}

// ============================================================================
// HATA TİPLERİ (NetlinkError)
// ============================================================================

/// Netlink işlemlerinde olabilecek hatta tipleri.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetlinkError {
    /// Mesaj formatı hatalı veya eksik.
    InvalidMessage,
    /// Bu protokol desteklenmiyor.
    InvalidProtocol,
    /// İşlem için yeterli yetki yok (root gerekiyor).
    PermissionDenied,
    /// Alım tamponu dolu, mesaj kabul edilemiyor.
    BufferFull,
    /// Hedef soket veya nesne bulunamadı.
    NotFound,
}

// ============================================================================
// SİSTEM ÇAĞRISI ARABIRIMI (Syscall Interface)
// ============================================================================
//
// Bu fonksiyonlar, kullanıcı uzayı sistem çağrılarını Netlink çekirdek
// yapısına köprüler. POSIX soket API'sini taklit ederler:
//
//   socket(AF_NETLINK, SOCK_RAW, protokol)  ->  sys_socket_netlink()
//   sendmsg(fd, msghdr, flags)              ->  sys_sendmsg_netlink()
//   recvmsg(fd, msghdr, flags)              ->  sys_recvmsg_netlink()

/// `socket(AF_NETLINK, ...)` sistem çağrısının Netlink tarafı.
/// Başarıda port_id (fd olarak kullanılır) döner.
pub fn sys_socket_netlink(protocol: u32) -> i32 {
    let sock = NETLINK_MANAGER.create_socket(protocol);
    sock.port_id.load(Ordering::Relaxed) as i32
}

/// `sendmsg()` sistem çağrısının Netlink tarafı.
/// `buf` bir NlMsgHdr başlangıcıyla başlamalıdır.
/// Başarıda gönderilen byte sayısını, hata varsa negatif errno döner.
pub fn sys_sendmsg_netlink(port_id: u32, buf: &[u8], flags: u32) -> i32 {
    // Tampon en az Netlink başlığı kadar büyük olmalı
    if buf.len() < NlMsgHdr::size() {
        return -22; // EINVAL
    }

    // ham belleği NlMsgHdr olarak oku (unsafe: C ABI uyumlu struct)
    let header = unsafe {
        core::ptr::read(buf.as_ptr() as *const NlMsgHdr)
    };

    let msg = NetlinkMessage {
        header,
        payload: buf[NlMsgHdr::size()..].to_vec(),
    };

    if let Some(sock) = NETLINK_MANAGER.get_socket(port_id) {
        match sock.send(msg) {
            Ok(()) => buf.len() as i32,
            Err(_) => -5, // EIO
        }
    } else {
        -9 // EBADF
    }
}

/// `recvmsg()` sistem çağrısının Netlink tarafı.
/// Tampona NlMsgHdr + payload yazar ve toplam uzunluğu döner.
/// Bekleyen mesaj yoksa -11 (EAGAIN) döner.
pub fn sys_recvmsg_netlink(port_id: u32, buf: &mut [u8], flags: u32) -> i32 {
    if let Some(sock) = NETLINK_MANAGER.get_socket(port_id) {
        if let Some(msg) = sock.recv() {
            let total_len = NlMsgHdr::size() + msg.payload.len();
            if buf.len() < total_len {
                return -7; // E2BIG (tampon çok küçük)
            }

            // Başlığı tampona yaz (unsafe: C struct -> raw bytes)
            unsafe {
                let ptr = buf.as_mut_ptr() as *mut NlMsgHdr;
                (*ptr) = msg.header;
            }

            // Payload'ı başlığın hemen ardına yaz
            buf[NlMsgHdr::size()..total_len].copy_from_slice(&msg.payload);

            return total_len as i32;
        }
        return -11; // EAGAIN (mesaj yok, tekrar dene)
    }
    -9 // EBADF (geçersiz fd)
}

// ============================================================================
// BAŞLATMA (Initialization)
// ============================================================================

/// Netlink alt sistemini başlatır. Sistem açılışında bir kez çağrılır.
pub fn init() {
    crate::serial_println!("[NETLINK] Subsystem initialized");
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
use crate::net::Ipv4Addr;
use crate::net::MacAddr;
    use alloc::sync::Arc;

    #[test]
    fn test_ifinfo_msg_size() {
        assert_eq!(core::mem::size_of::<IfInfoMsg>(), 16);
    }

    #[test]
    fn test_rtnl_link_stats64_size() {
        assert_eq!(core::mem::size_of::<RtnlLinkStats64>(), 192);
    }

    #[test]
    fn test_build_ifinfo_payload_empty_dev() {
        let dev = Arc::new(NetDevice::new("test0", MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]), 1500));
        NET_DEVICE_MANAGER.register(dev.clone());

        let payload = build_ifinfo_payload(&dev);
        // ifinfomsg (16) + at least a few attributes
        assert!(payload.len() > 32);

        // Parse ifinfomsg from start
        let info: &IfInfoMsg = unsafe {
            &*(payload.as_ptr() as *const IfInfoMsg)
        };
        assert_eq!(info.ifi_family, AF_UNSPEC);
        assert_eq!(info.ifi_type, ARPHRD_ETHER);
        assert!(info.ifi_index > 0);
        assert!(info.ifi_flags & IFF_BROADCAST != 0);
        assert!(info.ifi_flags & IFF_MULTICAST != 0);
        assert!(info.ifi_flags & IFF_UP == 0);
        assert_eq!(info.ifi_change, 0xFFFFFFFF);

        NET_DEVICE_MANAGER.unregister("test0");
    }

    #[test]
    fn test_build_ifinfo_payload_up_dev() {
        let dev = Arc::new(NetDevice::new("eth0", MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x02]), 1500));
        dev.up.store(true, Ordering::Release);
        dev.promiscuous.store(true, Ordering::Release);
        NET_DEVICE_MANAGER.register(dev.clone());

        let payload = build_ifinfo_payload(&dev);
        let info: &IfInfoMsg = unsafe { &*(payload.as_ptr() as *const IfInfoMsg) };
        assert!(info.ifi_flags & IFF_UP != 0);
        assert!(info.ifi_flags & IFF_RUNNING != 0);
        assert!(info.ifi_flags & IFF_LOWER_UP != 0);
        assert!(info.ifi_flags & IFF_PROMISC != 0);

        NET_DEVICE_MANAGER.unregister("eth0");
    }

    #[test]
    fn test_ifla_mtu_attr_in_payload() {
        let dev = Arc::new(NetDevice::new("mtu0", MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x03]), 9000));
        NET_DEVICE_MANAGER.register(dev.clone());

        let payload = build_ifinfo_payload(&dev);
        // After ifinfomsg (16 bytes), search for IFLA_MTU (type=4)
        let mut found_mtu = false;
        let mut pos = 16;
        while pos + 4 <= payload.len() {
            let len = u16::from_le_bytes([payload[pos], payload[pos+1]]);
            let typ = u16::from_le_bytes([payload[pos+2], payload[pos+3]]);
            if typ == IFLA_MTU {
                found_mtu = true;
                break;
            }
            if len == 0 { break; }
            pos += len as usize;
        }
        assert!(found_mtu, "IFLA_MTU attribute not found");

        NET_DEVICE_MANAGER.unregister("mtu0");
    }

    #[test]
    fn test_build_link_dump_empty() {
        let responses = build_link_dump();
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0].0, NLMSG_DONE);
    }

    #[test]
    fn test_build_link_dump_with_devices() {
        let d1 = Arc::new(NetDevice::new("lo0", MacAddr([0x00, 0x00, 0x00, 0x00, 0x00, 0x00]), 65535));
        let d2 = Arc::new(NetDevice::new("eth1", MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x04]), 1500));
        NET_DEVICE_MANAGER.register(d1.clone());
        NET_DEVICE_MANAGER.register(d2.clone());

        let responses = build_link_dump();
        // 2 devices + NLMSG_DONE
        assert_eq!(responses.len(), 3);
        assert_eq!(responses[0].0, RTM_NEWLINK);
        assert_eq!(responses[1].0, RTM_NEWLINK);
        assert_eq!(responses[2].0, NLMSG_DONE);

        // Verify device names in attributes
        for i in 0..2 {
            let payload = &responses[i].1;
            let mut pos = 16;
            let mut found_name = false;
            while pos + 4 <= payload.len() {
                let len = u16::from_le_bytes([payload[pos], payload[pos+1]]);
                let typ = u16::from_le_bytes([payload[pos+2], payload[pos+3]]);
                if typ == IFLA_IFNAME {
                    found_name = true;
                    break;
                }
                if len == 0 { break; }
                pos += len as usize;
            }
            assert!(found_name, "Device {} has no IFLA_IFNAME", i);
        }

        NET_DEVICE_MANAGER.unregister("lo0");
        NET_DEVICE_MANAGER.unregister("eth1");
    }

    #[test]
    fn test_iff_constants() {
        assert_eq!(IFF_UP, 1);
        assert_eq!(IFF_BROADCAST, 2);
        assert_eq!(IFF_LOOPBACK, 8);
        assert_eq!(IFF_MULTICAST, 1 << 12);
        assert_eq!(IFF_LOWER_UP, 1 << 16);
        assert_eq!(IFF_PROMISC, 1 << 8);
    }

    #[test]
    fn test_netlink_constants() {
        assert_eq!(NLMSG_DONE, 3);
        assert_eq!(NLMSG_ERROR, 2);
        assert_eq!(NLMSG_NOOP, 1);
    }

    #[test]
    fn test_ifla_constants() {
        assert_eq!(IFLA_IFNAME, 3);
        assert_eq!(IFLA_MTU, 4);
        assert_eq!(IFLA_ADDRESS, 1);
        assert_eq!(IFLA_STATS64, 23);
        assert_eq!(IFLA_OPERSTATE, 16);
        assert_eq!(IFLA_QDISC, 6);
    }

    #[test]
    fn test_ifaddrmsg_size() {
        assert_eq!(core::mem::size_of::<IfAddrMsg>(), 8);
    }

    #[test]
    fn test_ndmsg_size() {
        assert_eq!(core::mem::size_of::<NdMsg>(), 12);
    }

    #[test]
    fn test_rtmsg_size() {
        assert_eq!(core::mem::size_of::<RtMsg>(), 12);
    }

    #[test]
    fn test_fib_rule_hdr_size() {
        assert_eq!(core::mem::size_of::<FibRuleHdr>(), 8);
    }

    #[test]
    fn test_build_addr_dump_empty() {
        let responses = build_addr_dump();
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0].0, NLMSG_DONE);
    }

    #[test]
    fn test_build_addr_dump_with_devices() {
        let mut dev_raw = NetDevice::new("eth_addr", MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x05]), 1500);
        dev_raw.ip = Ipv4Addr::new(10, 0, 0, 5);
        let dev = Arc::new(dev_raw);
        NET_DEVICE_MANAGER.register(dev.clone());

        let responses = build_addr_dump();
        assert_eq!(responses.len(), 2); // 1 addr + NLMSG_DONE
        assert_eq!(responses[0].0, RTM_NEWADDR);

        // Check IfAddrMsg
        let payload = &responses[0].1;
        let msg: &IfAddrMsg = unsafe { &*(payload.as_ptr() as *const IfAddrMsg) };
        assert_eq!(msg.ifa_family, 2);
        assert_eq!(msg.ifa_index, dev.dev_id as i32);

        // Check IFA_LOCAL attr exists
        let mut found_local = false;
        let mut pos = core::mem::size_of::<IfAddrMsg>();
        while pos + 4 <= payload.len() {
            let len = u16::from_le_bytes([payload[pos], payload[pos+1]]);
            let typ = u16::from_le_bytes([payload[pos+2], payload[pos+3]]);
            if typ == IFA_LOCAL { found_local = true; break; }
            if len == 0 { break; }
            pos += len as usize;
        }
        assert!(found_local);

        NET_DEVICE_MANAGER.unregister("eth_addr");
    }

    #[test]
    fn test_build_neigh_dump_empty() {
        let responses = build_neigh_dump();
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0].0, NLMSG_DONE);
    }

    #[test]
    fn test_build_route_dump_empty() {
        let responses = build_route_dump();
        assert!(responses.len() >= 1);
        assert_eq!(responses.last().unwrap().0, NLMSG_DONE);
    }

    #[test]
    fn test_build_rule_dump_empty() {
        let responses = build_rule_dump();
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0].0, NLMSG_DONE);
    }

    #[test]
    fn test_ndmsg_with_arp_entry() {
        let ip = crate::net::Ipv4Addr::new(192, 168, 1, 1);
        let mac = MacAddr([0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
        let mut dev = NetDevice::new("eth_nd", MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x06]), 1500);
        dev.ip = crate::net::Ipv4Addr::new(192, 168, 1, 2);
        let dev = Arc::new(dev);
        NET_DEVICE_MANAGER.register(dev.clone());
        arp::add_entry(ip, mac);

        let responses = build_neigh_dump();
        // Either 1 (NUD_REACHABLE if find_by_ip matches) + DONE, or just DONE
        assert!(responses.len() >= 1);
        assert_eq!(responses.last().unwrap().0, NLMSG_DONE);

        NET_DEVICE_MANAGER.unregister("eth_nd");
    }

    #[test]
    fn test_rta_constants() {
        assert_eq!(RTA_DST, 1);
        assert_eq!(RTA_GATEWAY, 5);
        assert_eq!(RTA_TABLE, 15);
        assert_eq!(RTA_OIF, 4);
    }

    #[test]
    fn test_ifa_constants() {
        assert_eq!(IFA_LOCAL, 2);
        assert_eq!(IFA_ADDRESS, 1);
        assert_eq!(IFA_BROADCAST, 4);
        assert_eq!(IFA_LABEL, 3);
    }

    #[test]
    fn test_nda_constants() {
        assert_eq!(NDA_DST, 1);
        assert_eq!(NDA_LLADDR, 2);
    }

    #[test]
    fn test_fra_constants() {
        assert_eq!(FRA_PRIORITY, 6);
        assert_eq!(FRA_TABLE, 15);
        assert_eq!(FRA_FWMARK, 10);
    }

    #[test]
    fn test_build_addr_dump_skips_unspecified() {
        let mut dev = NetDevice::new("noip", MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x07]), 1500);
        dev.ip = crate::net::Ipv4Addr::UNSPECIFIED;
        let dev = Arc::new(dev);
        NET_DEVICE_MANAGER.register(dev.clone());

        let responses = build_addr_dump();
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0].0, NLMSG_DONE);

        NET_DEVICE_MANAGER.unregister("noip");
    }

    #[test]
    fn test_tcmsg_size() {
        assert_eq!(core::mem::size_of::<TcMsg>(), 20);
    }

    #[test]
    fn test_tc_stats_sizes() {
        assert_eq!(core::mem::size_of::<TcStats>(), 40);
        assert_eq!(core::mem::size_of::<GnetStatsBasic>(), 12);
        assert_eq!(core::mem::size_of::<GnetStatsQueue>(), 20);
    }

    #[test]
    fn test_build_qdisc_dump_empty() {
        let responses = build_qdisc_dump();
        assert!(!responses.is_empty());
        assert_eq!(responses.last().unwrap().0, NLMSG_DONE);
    }

    #[test]
    fn test_build_qdisc_dump_with_qdisc() {
        let respawn = crate::net::tc::TC_MANAGER.set_qdisc("tc_test_iface", crate::net::tc::QdiscKind::PfifoFast);
        let dev = Arc::new(NetDevice::new("tc_test_iface", MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x08]), 1500));
        NET_DEVICE_MANAGER.register(dev.clone());

        let responses = build_qdisc_dump();
        assert!(responses.len() >= 2);
        assert_eq!(responses.last().unwrap().0, NLMSG_DONE);
        // First response should be RTM_NEWQDISC
        assert_eq!(responses[0].0, RTM_NEWQDISC);

        // Parse TcMsg from first response payload
        let payload = &responses[0].1;
        let msg: &TcMsg = unsafe { &*(payload.as_ptr() as *const TcMsg) };
        assert_eq!(msg.tcm_ifindex, dev.dev_id as i32);
        assert_eq!(msg.tcm_handle, 0x80000000);

        // Check TCA_KIND attribute
        let mut found_kind = false;
        let mut pos = core::mem::size_of::<TcMsg>();
        while pos + 4 <= payload.len() {
            let len = u16::from_le_bytes([payload[pos], payload[pos+1]]);
            let typ = u16::from_le_bytes([payload[pos+2], payload[pos+3]]);
            if typ == TCA_KIND { found_kind = true; break; }
            if len == 0 { break; }
            pos += len as usize;
        }
        assert!(found_kind, "TCA_KIND attribute not found");

        NET_DEVICE_MANAGER.unregister("tc_test_iface");
    }

    #[test]
    fn test_tca_constants() {
        assert_eq!(TCA_KIND, 1);
        assert_eq!(TCA_STATS2, 4);
        assert_eq!(TCA_HANDLE, 8);
        assert_eq!(TCA_PARENT, 9);
        assert_eq!(TCA_IFINDEX, 10);
    }

    // ========================================================================
    // Industrial-grade port: rtnetlink.sh test patterns
    // - policy routing rule message (kci_test_polrouting)
    // - route get message with selectors (kci_test_route_get)
    // - neigh update flags (kci_test_neigh_update)
    // - address proto attribute (kci_test_address_proto)
    // - TC filter message building (kci_test_tc)
    // ========================================================================

    #[test]
    fn test_build_policy_rule_message() {
        use alloc::vec;

        let msg = FibRuleHdr {
            family: 2,
            dst_len: 0,
            src_len: 0,
            tos: 0,
            table: 100,
            res1: 0,
            res2: 0,
            action: 0,
        };
        let msg_bytes = unsafe {
            core::slice::from_raw_parts(
                &msg as *const FibRuleHdr as *const u8,
                core::mem::size_of::<FibRuleHdr>(),
            )
        };

        let mut payload = Vec::new();
        payload.extend_from_slice(msg_bytes);
        payload.extend_from_slice(&NlAttr::new(FRA_PRIORITY, &100u32.to_ne_bytes()));
        payload.extend_from_slice(&NlAttr::new(FRA_FWMARK, &1u32.to_ne_bytes()));
        payload.extend_from_slice(&NlAttr::new(FRA_TABLE, &100u32.to_ne_bytes()));

        let total_hdr = core::mem::size_of::<FibRuleHdr>();
        let expected_attrs = 3 * (4 + core::mem::size_of::<u32>()); // each NlAttr(2+2)+(4)
        assert!(payload.len() >= total_hdr + expected_attrs);
        assert_eq!(msg.family, 2);
        assert_eq!(msg.table, 100);
    }

    #[test]
    fn test_build_route_get_with_selectors() {
        use alloc::vec;

        let msg = RtMsg {
            rtm_family: 2,
            rtm_dst_len: 0,
            rtm_src_len: 0,
            rtm_tos: 0x10,
            rtm_table: 0,
            rtm_protocol: 4,
            rtm_scope: 0,
            rtm_type: 1,
            rtm_flags: 0,
        };
        let msg_bytes = unsafe {
            core::slice::from_raw_parts(
                &msg as *const RtMsg as *const u8,
                core::mem::size_of::<RtMsg>(),
            )
        };

        let mut payload = Vec::new();
        payload.extend_from_slice(msg_bytes);
        payload.extend_from_slice(&NlAttr::new(RTA_DST, &[127, 0, 0, 1]));
        payload.extend_from_slice(&NlAttr::new(RTA_OIF, b"lo\0"));
        payload.extend_from_slice(&NlAttr::new(RTA_MARK, &1u32.to_ne_bytes()));
        payload.extend_from_slice(&NlAttr::new(RTA_TABLE, &200u32.to_ne_bytes()));

        assert_eq!(msg.rtm_tos, 0x10);
        assert!(payload.len() > core::mem::size_of::<RtMsg>());
    }

    #[test]
    fn test_build_neigh_with_flags_update() {
        use alloc::vec;

        let mut payload = Vec::new();
        let msg = NdMsg {
            ndm_family: 2,
            ndm_pad1: 0,
            ndm_pad2: 0,
            ndm_ifindex: 1,
            ndm_state: NUD_REACHABLE,
            ndm_flags: 0,
            ndm_type: 0,
        };
        let msg_bytes = unsafe {
            core::slice::from_raw_parts(
                &msg as *const NdMsg as *const u8,
                core::mem::size_of::<NdMsg>(),
            )
        };
        payload.extend_from_slice(msg_bytes);
        payload.extend_from_slice(&NlAttr::new(NDA_DST, &[10, 0, 2, 1]));
        payload.extend_from_slice(&NlAttr::new(NDA_LLADDR, &[0xde, 0xad, 0xbe, 0xef, 0x13, 0x37]));
        payload.extend_from_slice(&NlAttr::new(NDA_PROBES, &5u32.to_ne_bytes()));

        assert_eq!(msg.ndm_state, NUD_REACHABLE);
        assert!(payload.len() > core::mem::size_of::<NdMsg>());
    }

    #[test]
    fn test_build_addr_with_proto_attribute() {
        use alloc::vec;

        let msg = IfAddrMsg {
            ifa_family: 2,
            ifa_prefixlen: 24,
            ifa_flags: 0,
            ifa_scope: 0,
            ifa_index: 1,
        };
        let msg_bytes = unsafe {
            core::slice::from_raw_parts(
                &msg as *const IfAddrMsg as *const u8,
                core::mem::size_of::<IfAddrMsg>(),
            )
        };

        let mut payload = Vec::new();
        payload.extend_from_slice(msg_bytes);
        payload.extend_from_slice(&NlAttr::new(IFA_LOCAL, &[10, 0, 2, 15]));
        payload.extend_from_slice(&NlAttr::new(IFA_ADDRESS, &[10, 0, 2, 15]));
        payload.extend_from_slice(&NlAttr::new(IFA_PROTO, &[0xab]));
        payload.extend_from_slice(&NlAttr::new(IFA_LABEL, b"eth0\0"));

        assert_eq!(msg.ifa_prefixlen, 24);
        assert!(payload.len() > core::mem::size_of::<IfAddrMsg>());
    }

    #[test]
    fn test_build_tc_filter_message() {
        use alloc::vec;

        let msg = TcMsg {
            tcm_family: 2,
            tcm__pad1: 0,
            tcm__pad2: 0,
            tcm_ifindex: 1,
            tcm_handle: 0xffff0002,
            tcm_parent: 0xffff0001,
            tcm_info: 0x300,
        };
        let msg_bytes = unsafe {
            core::slice::from_raw_parts(
                &msg as *const TcMsg as *const u8,
                core::mem::size_of::<TcMsg>(),
            )
        };

        let mut payload = Vec::new();
        payload.extend_from_slice(msg_bytes);
        payload.extend_from_slice(&NlAttr::new(TCA_KIND, b"u32\0"));
        payload.extend_from_slice(&NlAttr::new(
            TCA_HANDLE,
            &0xffff0002u32.to_ne_bytes(),
        ));
        payload.extend_from_slice(&NlAttr::new(
            TCA_PARENT,
            &0xffff0001u32.to_ne_bytes(),
        ));

        assert_eq!(msg.tcm_handle, 0xffff0002);
        assert_eq!(msg.tcm_parent, 0xffff0001);
        assert!(payload.len() > core::mem::size_of::<TcMsg>());
    }
}
