//! # nftables — İfade Bazlı Paket Filtreleme Motoru
//!
//! Bu modül, Linux nftables'in netlink tabanlı ifade (expression) mimarisini
//! echOS'a taşır. nftables, iptables'ın Match+Target modelinin yerini alan
//! ifade zincirleri (expression chains) kullanır.
//!
//! ## Arşiv Kaynağı
//!
//! - 000716: docs.kernel.org/netlink_specs/nftables — Netlink specification
//!   (operations: batch-begin/end, newtable/gettable/deltable, newchain/getchain/
//!   delchain, newrule/getrule/delrule, newset/getset/delset, newsetelem/
//!   getsetelem/delsetelem, getgen, newobj/getobj/delobj,
//!   newflowtable/getflowtable/delflowtable)
//! - 000765: docs.kernel.org/administering/sysctl/net/netfilter — sysctl params
//!
//! ## Temel Farklılıklar (iptables vs nftables)
//!
//! ```text
//! iptables:  Tablo → Zincir → Kural (Match + Target)
//! nftables:  Tablo → Zincir → Kural (Expression zinciri)
//!
//! iptables Match: src_ip, dst_ip, port, tcp_flags (sabit yapı)
//! nftables Expr:  payload(meta) → cmp → counter → verdict (esnek zincir)
//!
//! nftables Expression türleri:
//!   payload  → Paket başlığından veri oku (L2/L3/L4 offset+width)
//!   meta     → Paket meta-verisine eriş (iifname, oifname, mark, nfproto)
//!   cmp      → Karşılaştırma (eq, neq, lt, lte, gt, gte)
//!   bitwise  → Bit操作ları (AND, OR, XOR, shifts, mask-and-xor)
//!   lookup   → Set içinde arama
//!   counter  → Paket/bayt sayacı
//!   verdict  → Karar (accept, drop, jump, goto, return, continue, break)
//!   nat      → SNAT/DNAT/masquerade
//!   reject   → ICMP unreachable veya TCP RST gönder
//!   log      → Paket günlüğü
//!   quota    → Trafik kotası
//!   limit    → Hız sınırlaması
//!   ct       → Conntrack durumuna göre filtreleme
//!   fib      → FIB routing lookup
//!   tproxy   → Transparent proxy
//!   masq     → Masquerade ifadesi
//!   objref   → Nesne referansı (counter/quota/limit objeleri)
//!   flow_offload → Akış hızlandırma
//! ```

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;

use super::netfilter::{
    ConntrackState, NF_ACCEPT, NF_DROP, NF_INET_FORWARD, NF_INET_LOCAL_IN,
    NF_INET_LOCAL_OUT, NF_INET_POST_ROUTING, NF_INET_PRE_ROUTING, NFPROTO_IPV4, NFPROTO_IPV6,
    NFPROTO_UNSPEC, PacketInfo,
};

// ============================================================================
// NFTABLES SABİTLERI — Linux UAPI ile birebir uyumlu
// ============================================================================
// Kaynak: include/uapi/linux/netfilter/nf_tables.h

/// nfgenmsg.family değerleri (protocol family)
pub const NFT_FAMILY_UNSPEC: u8 = 0;
pub const NFT_FAMILY_IPV4: u8 = NFPROTO_IPV4 as u8;
pub const NFT_FAMILY_IPV6: u8 = NFPROTO_IPV6 as u8;

/// nfgenmsg.version
pub const NFT_GEN_VERSION: u8 = 0;

// ============================================================================
// İFADE TÜRLERİ (EXPRESSION TYPES)
// ============================================================================
// nftables'ta her kural bir ifade zincirinden oluşur.
// İfadeler sırayla değerlendirilir; ilk false döndüren ifade kuralı başarısız kılar.

/// İfade türleri — Linux'ta `NFT_EXPR_*` sabitlerine karşılık gelir.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NftExprType {
    /// Paket başlığından veri okuma (L2/L3/L4 offset ve genişlik ile)
    Payload,
    /// Paket meta-verisi (arayüz adı, mark, protokol ailesi vb.)
    Meta,
    /// Karşılaştırma (eşit, eşit değil, büyük, küçük vb.)
    Cmp,
    /// Bit操作ları (AND, OR, XOR, shift)
    Bitwise,
    /// Set içinde arama (lookup)
    Lookup,
    /// Paket/bayt sayacı (stateful)
    Counter,
    /// Karar ifadesi (accept, drop, jump, goto, return, continue, break)
    Verdict,
    /// SNAT/DNAT/masquerade
    Nat,
    /// Reddetme (ICMP unreachable veya TCP RST)
    Reject,
    /// Paket günlüğü
    Log,
    /// Trafik kotası
    Quota,
    /// Hız sınırlaması
    Limit,
    /// Conntrack durumu filtresi
    Conntrack,
    /// FIB routing lookup
    Fib,
    /// Transparent proxy
    Tproxy,
    /// Masquerade ifadesi (NAT türü)
    Masq,
    /// Nesne referansı (counter/quota/limit nesnesi)
    Objref,
    /// Akış hızlandırma
    FlowOffload,
}

// ============================================================================
// PAYLOAD İFADESİ (EXPRESSION: PAYLOAD)
// ============================================================================
// Kaynak: nftables spec — NFT_EXPR_PAYLOAD
//
// Paketin belirli bir offset'inden belirli genişlikte veri okur.
// base: hangi başlıktan itibaren okunacağı (link-layer, network, transport, inner, tun)
// offset: base'den itibaren bayt cinsinden offset
// width: okunacak bayt genişliği (1..16)
//
// nftables payload-base enum:
//   0 = link-layer-header (Ethernet başlığı)
//   1 = network-header   (IP/IPv6 başlığı)
//   2 = transport-header (TCP/UDP başlığı)
//   3 = inner-header     (tünelleme iç başlığı)
//   4 = tun-header       (tünelleme başlığı)

/// Payload tabanı — hangi başlık katmanından veri okunacağı
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PayloadBase {
    /// Ethernet (Layer 2) başlığı
    LinkLayerHeader,
    /// IP/IPv6 (Layer 3) başlığı
    NetworkHeader,
    /// TCP/UDP (Layer 4) başlığı
    TransportHeader,
    /// Tünelleme iç başlığı
    InnerHeader,
    /// Tünelleme başlığı
    TunHeader,
}

/// Payload ifadesi — paketin belirli bir offset'inden veri okur.
///
/// `base` + `offset` ile bayt konumu hesaplanır, `width` kadar bayt okunur.
/// Okunan değer `dreg` hedef register'ına yazılır (bitwise/cmp ifadeleri için).
///
/// # Örnek: TCP port eşleştirmesi
/// ```text
/// base = TransportHeader, offset = 0, width = 2  → kaynak port
/// base = TransportHeader, offset = 2, width = 2  → hedef port
/// ```
///
/// # Örnek: IP kaynak adresi
/// ```text
/// base = NetworkHeader, offset = 12, width = 4  → src IPv4
/// base = NetworkHeader, offset = 8,  width = 4  → dst IPv4
/// ```
#[derive(Clone, Debug)]
pub struct PayloadExpr {
    pub base: PayloadBase,
    pub offset: u32,
    pub width: u32,
    pub dreg: u32,
}

impl PayloadExpr {
    /// Network header'dan IPv4 src_addr oku (offset=12, width=4)
    pub fn ipv4_src(dreg: u32) -> Self {
        Self {
            base: PayloadBase::NetworkHeader,
            offset: 12,
            width: 4,
            dreg,
        }
    }

    /// Network header'dan IPv4 dst_addr oku (offset=16, width=4)
    pub fn ipv4_dst(dreg: u32) -> Self {
        Self {
            base: PayloadBase::NetworkHeader,
            offset: 16,
            width: 4,
            dreg,
        }
    }

    /// Network header'dan IP protocol oku (offset=9, width=1)
    pub fn ipv4_protocol(dreg: u32) -> Self {
        Self {
            base: PayloadBase::NetworkHeader,
            offset: 9,
            width: 1,
            dreg,
        }
    }

    /// Network header'dan DSCP+ECN (ToS) oku (offset=1, width=1)
    pub fn ipv4_tos(dreg: u32) -> Self {
        Self {
            base: PayloadBase::NetworkHeader,
            offset: 1,
            width: 1,
            dreg,
        }
    }

    /// Network header'dan TTL oku (offset=8, width=1)
    pub fn ipv4_ttl(dreg: u32) -> Self {
        Self {
            base: PayloadBase::NetworkHeader,
            offset: 8,
            width: 1,
            dreg,
        }
    }

    /// Network header'dan total length oku (offset=2, width=2)
    pub fn ipv4_total_length(dreg: u32) -> Self {
        Self {
            base: PayloadBase::NetworkHeader,
            offset: 2,
            width: 2,
            dreg,
        }
    }

    /// Transport header'dan TCP src_port oku (offset=0, width=2)
    pub fn tcp_sport(dreg: u32) -> Self {
        Self {
            base: PayloadBase::TransportHeader,
            offset: 0,
            width: 2,
            dreg,
        }
    }

    /// Transport header'dan TCP dst_port oku (offset=2, width=2)
    pub fn tcp_dport(dreg: u32) -> Self {
        Self {
            base: PayloadBase::TransportHeader,
            offset: 2,
            width: 2,
            dreg,
        }
    }

    /// Transport header'dan TCP flags oku (offset=13, width=1)
    pub fn tcp_flags(dreg: u32) -> Self {
        Self {
            base: PayloadBase::TransportHeader,
            offset: 13,
            width: 1,
            dreg,
        }
    }

    /// Transport header'dan UDP src_port oku (offset=0, width=2)
    pub fn udp_sport(dreg: u32) -> Self {
        Self {
            base: PayloadBase::TransportHeader,
            offset: 0,
            width: 2,
            dreg,
        }
    }

    /// Transport header'dan UDP dst_port oku (offset=2, width=2)
    pub fn udp_dport(dreg: u32) -> Self {
        Self {
            base: PayloadBase::TransportHeader,
            offset: 2,
            width: 2,
            dreg,
        }
    }

    /// Transport header'dan ICMP type oku (offset=0, width=1)
    pub fn icmp_type(dreg: u32) -> Self {
        Self {
            base: PayloadBase::TransportHeader,
            offset: 0,
            width: 1,
            dreg,
        }
    }

    /// Transport header'dan ICMP code oku (offset=1, width=1)
    pub fn icmp_code(dreg: u32) -> Self {
        Self {
            base: PayloadBase::TransportHeader,
            offset: 1,
            width: 1,
            dreg,
        }
    }

    /// Transport header'dan ICMP id oku (offset=4, width=2)
    pub fn icmp_id(dreg: u32) -> Self {
        Self {
            base: PayloadBase::TransportHeader,
            offset: 4,
            width: 2,
            dreg,
        }
    }

    /// Link layer'dan Ethernet EtherType oku (offset=12, width=2)
    pub fn ether_type(dreg: u32) -> Self {
        Self {
            base: PayloadBase::LinkLayerHeader,
            offset: 12,
            width: 2,
            dreg,
        }
    }

    /// Link layer'dan VLAN TCI oku (offset=14, width=2) — 802.1Q tagged frame
    pub fn vlan_tci(dreg: u32) -> Self {
        Self {
            base: PayloadBase::LinkLayerHeader,
            offset: 14,
            width: 2,
            dreg,
        }
    }
}

// ============================================================================
// META İFADESİ (EXPRESSION: META)
// ============================================================================
// Kaynak: nftables spec — NFT_EXPR_META
//
// Paket meta-verisine erişim. `key` hangi meta alanına erişileceğini,
// `dreg` ise sonucun yazılacağı register'ı belirtir.
//
// meta-keys enum (spec'ten):
//   len, protocol, priority, mark, iif, oif, iifname, oifname,
//   iftype, oiftype, skuid, skgid, nftrace, rtclassid, secmark,
//   nfproto, l4-proto, pkttype, cpu, cgroup, prandom ...

/// Meta anahtarları — nftables meta-keys enum'una karşılık gelir
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetaKey {
    /// Paket uzunluğu (Layer 2)
    Len,
    /// Ethernet protocol (EtherType)
    Protocol,
    /// Paket markası (nfmark)
    Mark,
    /// Giriş arabirimi indeksi
    Iif,
    /// Çıkış arabirimi indeksi
    Oif,
    /// Giriş arabirimi adı (null-terminated string)
    IifName,
    /// Çıkış arabirimi adı (null-terminated string)
    OifName,
    /// nfproto (NFPROTO_IPV4=2, NFPROTO_IPV6=10)
    Nfproto,
    /// L4 protokol numarası
    L4Proto,
    /// Paket türü (unicast=0, multicast=1, broadcast=2)
    PktType,
    /// CPU numarası
    Cpu,
    /// UID (gönderen sürecin kullanıcı kimliği)
    SkUid,
    /// GID (gönderen sürecin grup kimliği)
    SkGid,
    /// Rastgele sayı
    PRandom,
    /// Zaman (nano-saniye cinsinden)
    TimeNs,
    /// Gün (haftanın günü, 0=Pazar)
    TimeDay,
    /// Saat (0-23)
    TimeHour,
    /// Secmark (SELinux/AppArmor güvenlik markası)
    Secmark,
}

/// Meta ifadesi — paket meta-verisini register'a yazar.
///
/// # Örnek: Giriş arabirimi adını oku ve cmp ile karşılaştır
/// ```text
/// meta(iifname) → reg1
/// cmp(eq, "eth0") → reg1'deki değeri "eth0" ile karşılaştır
/// ```
#[derive(Clone, Debug)]
pub struct MetaExpr {
    pub key: MetaKey,
    pub dreg: u32,
}

impl MetaExpr {
    /// Giriş arabirimi adını register'a yazar
    pub fn iifname(dreg: u32) -> Self {
        Self {
            key: MetaKey::IifName,
            dreg,
        }
    }

    /// Çıkış arabirimi adını register'a yazar
    pub fn oifname(dreg: u32) -> Self {
        Self {
            key: MetaKey::OifName,
            dreg,
        }
    }

    /// Paket markasını register'a yazar
    pub fn mark(dreg: u32) -> Self {
        Self {
            key: MetaKey::Mark,
            dreg,
        }
    }

    /// nfproto (protokol ailesi) register'a yazar
    pub fn nfproto(dreg: u32) -> Self {
        Self {
            key: MetaKey::Nfproto,
            dreg,
        }
    }

    /// L4 protokol numarasını register'a yazar
    pub fn l4proto(dreg: u32) -> Self {
        Self {
            key: MetaKey::L4Proto,
            dreg,
        }
    }

    /// Paket uzunluğunu register'a yazar
    pub fn len(dreg: u32) -> Self {
        Self {
            key: MetaKey::Len,
            dreg,
        }
    }
}

// ============================================================================
// KARŞILAŞTIRMA İFADESİ (EXPRESSION: CMP)
// ============================================================================
// Kaynak: nftables spec — NFT_EXPR_CMP
//
// `sreg`'deki değeri `data` ile karşılaştırır.
// `op` karşılaştırma türünü belirtir.

/// Karşılaştırma operatörleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CmpOp {
    /// Eşit (==)
    Eq,
    /// Eşit değil (!=)
    Neq,
    /// Küçük (<)
    Lt,
    /// Küçük veya eşit (<=)
    Lte,
    /// Büyük (>)
    Gt,
    /// Büyük veya eşit (>=)
    Gte,
}

/// Karşılaştırma ifadesi — `sreg`'deki değeri `data` ile karşılaştırır.
///
/// `data` bayt dizisi olarak verilir; genişliği `data.len()` kadardır.
/// Karşılaştırma küçükten büyüğe (little-endian) bayt sırasıyla yapılır.
///
/// # Örnek: TCP port == 80
/// ```text
/// payload(tcp_sport) → reg1  (2 bayt)
/// cmp(eq, [80, 0])   → reg1'deki değeri 80 ile karşılaştır
/// ```
#[derive(Clone, Debug)]
pub struct CmpExpr {
    pub op: CmpOp,
    pub sreg: u32,
    pub data: Vec<u8>,
}

impl CmpExpr {
    /// Eşitlik karşılaştırması (==) — tek bayt
    pub fn eq_u8(sreg: u32, val: u8) -> Self {
        Self {
            op: CmpOp::Eq,
            sreg,
            data: vec![val],
        }
    }

    /// Eşitlik karşılaştırması (==) — iki bayt (u16, little-endian)
    pub fn eq_u16(sreg: u32, val: u16) -> Self {
        Self {
            op: CmpOp::Eq,
            sreg,
            data: val.to_le_bytes().to_vec(),
        }
    }

    /// Eşitlik karşılaştırması (==) — dört bayt (u32, little-endian)
    pub fn eq_u32(sreg: u32, val: u32) -> Self {
        Self {
            op: CmpOp::Eq,
            sreg,
            data: val.to_le_bytes().to_vec(),
        }
    }

    /// Eşitlik karşılaştırması (==) — byte slice
    pub fn eq_bytes(sreg: u32, data: &[u8]) -> Self {
        Self {
            op: CmpOp::Eq,
            sreg,
            data: data.to_vec(),
        }
    }

    /// Eşit değil karşılaştırması (!=) — tek bayt
    pub fn neq_u8(sreg: u32, val: u8) -> Self {
        Self {
            op: CmpOp::Neq,
            sreg,
            data: vec![val],
        }
    }

    /// Eşit değil karşılaştırması (!=) — iki bayt (u16)
    pub fn neq_u16(sreg: u32, val: u16) -> Self {
        Self {
            op: CmpOp::Neq,
            sreg,
            data: val.to_le_bytes().to_vec(),
        }
    }

    /// Büyük veya eşit (>=) — dört bayt (u32)
    pub fn gte_u32(sreg: u32, val: u32) -> Self {
        Self {
            op: CmpOp::Gte,
            sreg,
            data: val.to_le_bytes().to_vec(),
        }
    }

    /// Küçük veya eşit (<=) — dört bayt (u32)
    pub fn lte_u32(sreg: u32, val: u32) -> Self {
        Self {
            op: CmpOp::Lte,
            sreg,
            data: val.to_le_bytes().to_vec(),
        }
    }
}

// ============================================================================
// BITWISE İFADESİ (EXPRESSION: BITWISE)
// ============================================================================
// Kaynak: nftables spec — NFT_EXPR_BITWISE
//
// `sreg`'deki değere bitmask-and-xor işlemi uygular.
// `mask` ile AND, `xor` ile XOR yapılır: result = (value & mask) ^ xor
//
// bitwise-ops enum:
//   mask-xor, lshift, rshift, and, or, xor
//
// nftables'ta bitwise ifadesi genellikle tek bir maske + xor ile çalışır:
//   result = (sreg & mask) ^ xor
//
// Bu, port eşleştirmede tek byte offset'ten 2-byte alan okumak için kullanılır.

/// Bitwise operatörleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BitwiseOp {
    /// Mask-and-XOR: result = (sreg & mask) ^ xor
    MaskXor,
    /// Sol kaydırma
    LShift,
    /// Sağ kaydırma
    RShift,
    /// Bitwise AND
    And,
    /// Bitwise OR
    Or,
    /// Bitwise XOR
    Xor,
}

/// Bitwise ifadesi — register değerine bitmask işlemi uygular.
///
/// `result = (sreg & mask) ^ xor`
///
/// # Örnek: TCP port'un高位 byte'ını maskeleyerek tek byte karşılaştırma
/// ```text
/// payload(tcp_sport, offset=0, width=2) → reg1  (2 byte: [lo, hi])
/// bitwise(mask=[0xFF, 0x00], xor=[0, 0]) → reg1  (yalnızca low byte kalır)
/// cmp(eq, [80, 0])  → port 80 kontrolü
/// ```
///
/// # Örnek: IP DSCP değerini çıkarma
/// ```text
/// payload(ipv4_tos, offset=1, width=1) → reg1  (1 byte: ToS)
/// bitwise(mask=[0xFC], xor=[0]) → reg1  (DSCP = ToS >> 2, ENCN maskelenir)
/// ```
#[derive(Clone, Debug)]
pub struct BitwiseExpr {
    pub op: BitwiseOp,
    pub sreg: u32,
    pub dreg: u32,
    pub mask: Vec<u8>,
    pub xor: Vec<u8>,
}

impl BitwiseExpr {
    /// Mask-and-XOR: result = (sreg & mask) ^ xor
    pub fn mask_xor(sreg: u32, dreg: u32, mask: &[u8], xor: &[u8]) -> Self {
        Self {
            op: BitwiseOp::MaskXor,
            sreg,
            dreg,
            mask: mask.to_vec(),
            xor: xor.to_vec(),
        }
    }

    /// Tek baytlık mask-and-xor
    pub fn mask_xor_u8(sreg: u32, dreg: u32, mask: u8, xor: u8) -> Self {
        Self {
            op: BitwiseOp::MaskXor,
            sreg,
            dreg,
            mask: vec![mask],
            xor: vec![xor],
        }
    }

    /// İki baytlık mask-and-xor (u16)
    pub fn mask_xor_u16(sreg: u32, dreg: u32, mask: u16, xor: u16) -> Self {
        Self {
            op: BitwiseOp::MaskXor,
            sreg,
            dreg,
            mask: mask.to_le_bytes().to_vec(),
            xor: xor.to_le_bytes().to_vec(),
        }
    }

    /// Dört baytlık mask-and-xor (u32)
    pub fn mask_xor_u32(sreg: u32, dreg: u32, mask: u32, xor: u32) -> Self {
        Self {
            op: BitwiseOp::MaskXor,
            sreg,
            dreg,
            mask: mask.to_le_bytes().to_vec(),
            xor: xor.to_le_bytes().to_vec(),
        }
    }
}

// ============================================================================
// LOOKUP İFADESİ (EXPRESSION: LOOKUP)
// ============================================================================
// Kaynak: nftables spec — NFT_EXPR_LOOKUP
//
// `sreg`'deki değeri `set_name` adlı sette arar.
// Eşleşme olursa verdict/veri `dreg`'e yazılır veya direkt verdict döner.
//
// lookup-flags:
//   invert → eşleşmeyenleri eşleştir (NOT)

/// Lookup ifadesi — set içinde arama
#[derive(Clone, Debug)]
pub struct LookupExpr {
    pub sreg: u32,
    pub set_name: String,
    pub dreg: u32,
    pub invert: bool,
}

impl LookupExpr {
    /// Set içinde arama
    pub fn new(sreg: u32, set_name: &str, dreg: u32) -> Self {
        Self {
            sreg,
            set_name: String::from(set_name),
            dreg,
            invert: false,
        }
    }

    /// Ters eşleştirme (invert) — eşleşmeyen değerler için eşleşme sağla
    pub fn invert(sreg: u32, set_name: &str, dreg: u32) -> Self {
        Self {
            sreg,
            set_name: String::from(set_name),
            dreg,
            invert: true,
        }
    }
}

// ============================================================================
// COUNTER İFADESİ (EXPRESSION: COUNTER)
// ============================================================================
// Kaynak: nftables spec — NFT_EXPR_COUNTER
//
// Stateful ifade — her eşleşen paket için sayacı artırır.
// counter-attrs:
//   bytes (u64, big-endian)
//   packets (u64, big-endian)

/// Counter ifadesi — paket ve bayt sayacı tutar.
///
/// Stateful bir ifadedir. Paket bu ifadeden geçtiğinde sayaçlar atomik olarak
/// güncellenir. `nft list ruleset` komutuyla görüntülenebilir.
pub struct CounterExpr {
    pub packets: AtomicU64,
    pub bytes: AtomicU64,
}

impl Clone for CounterExpr {
    fn clone(&self) -> Self {
        Self {
            packets: AtomicU64::new(self.packets.load(Ordering::Relaxed)),
            bytes: AtomicU64::new(self.bytes.load(Ordering::Relaxed)),
        }
    }
}

impl core::fmt::Debug for CounterExpr {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CounterExpr")
            .field("packets", &self.packets.load(Ordering::Relaxed))
            .field("bytes", &self.bytes.load(Ordering::Relaxed))
            .finish()
    }
}

impl CounterExpr {
    pub fn new() -> Self {
        Self {
            packets: AtomicU64::new(0),
            bytes: AtomicU64::new(0),
        }
    }

    /// Sayaçları artırır
    pub fn update(&self, pkt_len: u64) {
        self.packets.fetch_add(1, Ordering::Relaxed);
        self.bytes.fetch_add(pkt_len, Ordering::Relaxed);
    }

    /// Anlık sayaç görüntüsü
    pub fn snapshot(&self) -> (u64, u64) {
        (
            self.packets.load(Ordering::Relaxed),
            self.bytes.load(Ordering::Relaxed),
        )
    }
}

impl Default for CounterExpr {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// VERDICT İFADESİ (EXPRESSION: VERDICT)
// ============================================================================
// Kaynak: nftables spec — NFT_EXPR_VERDICT
//
// verdict-code enum:
//   continue, break, jump, goto, return, drop, accept, stolen, queue, repeat

/// Verdict türleri — nftables verdict-code enum'una karşılık gelir
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerdictKind {
    /// Paketi kabul et (bir sonraki kurala veya zincir politikasına geç)
    Accept,
    /// Paketi sessizce düşür
    Drop,
    /// Belirtilen zincire atla (jump — geri dönüş noktası korunur)
    Jump,
    /// Belirtilen zincire git (goto — geri dönüş noktası KORUNMAZ)
    Goto,
    /// Çağıran zincire geri dön (return)
    Return,
    /// Mevcut zincirin değerlendirilmesine devam et (continue)
    Continue,
    /// Mevcut zincirin değerlendirilmesini durdur (break)
    Break,
    /// Paket stolen — çekirdeğe ait artık (stolen)
    Stolen,
    /// Paketi kullanıcı alanına gönder (queue)
    Queue,
    /// Kanca fonksiyonunu tekrar çağır (repeat)
    Repeat,
}

/// Verdict ifadesi — kural eşleştiğinde uygulanacak eylemi belirtir.
///
/// # Örnek: Kural eşleşirse kabul et
/// ```text
/// counter → verdict(accept)
/// ```
///
/// # Örnek: Zincir atlama
/// ```text
/// cmp(iifname, "eth0") → verdict(jump, "mychain")
/// ```
#[derive(Clone, Debug)]
pub struct VerdictExpr {
    pub kind: VerdictKind,
    /// Jump/goto durumunda hedef zincir adı
    pub chain: Option<String>,
}

impl VerdictExpr {
    pub fn accept() -> Self {
        Self {
            kind: VerdictKind::Accept,
            chain: None,
        }
    }

    pub fn drop() -> Self {
        Self {
            kind: VerdictKind::Drop,
            chain: None,
        }
    }

    pub fn jump(chain: &str) -> Self {
        Self {
            kind: VerdictKind::Jump,
            chain: Some(String::from(chain)),
        }
    }

    pub fn goto(chain: &str) -> Self {
        Self {
            kind: VerdictKind::Goto,
            chain: Some(String::from(chain)),
        }
    }

    pub fn return_() -> Self {
        Self {
            kind: VerdictKind::Return,
            chain: None,
        }
    }

    pub fn continue_() -> Self {
        Self {
            kind: VerdictKind::Continue,
            chain: None,
        }
    }

    pub fn break_() -> Self {
        Self {
            kind: VerdictKind::Break,
            chain: None,
        }
    }
}

/// Verdict kodunu Linux netfilter karar koduna dönüştürür
pub fn verdict_to_nf(verdict: &VerdictExpr) -> u32 {
    match verdict.kind {
        VerdictKind::Accept => NF_ACCEPT,
        VerdictKind::Drop => NF_DROP,
        VerdictKind::Return => 0xFFFFFFFF,
        VerdictKind::Stolen => 2, // NF_STOLEN
        VerdictKind::Queue => 3,  // NF_QUEUE
        VerdictKind::Repeat => 4, // NF_REPEAT
        VerdictKind::Jump | VerdictKind::Goto => NF_ACCEPT, // zincir atlaması ayrı işlenir
        VerdictKind::Continue | VerdictKind::Break => NF_ACCEPT,
    }
}

// ============================================================================
// NAT İFADESİ (EXPRESSION: NAT)
// ============================================================================
// Kaynak: nftables spec — NFT_EXPR_NAT
//
// nat-range-flags:
//   map-ips, proto-specified, proto-random, persistent,
//   proto-random-fully, proto-offset, netmap

/// NAT türleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NatType {
    /// Kaynak NAT (SNAT)
    Source,
    /// Hedef NAT (DNAT)
    Destination,
}

/// NAT flag'leri
pub const NAT_FLAG_MAP_IPS: u32 = 1 << 0;
pub const NAT_FLAG_PROTO_SPECIFIED: u32 = 1 << 1;
pub const NAT_FLAG_PROTO_RANDOM: u32 = 1 << 2;
pub const NAT_FLAG_PERSISTENT: u32 = 1 << 3;
pub const NAT_FLAG_PROTO_RANDOM_FULLY: u32 = 1 << 4;
pub const NAT_FLAG_PROTO_OFFSET: u32 = 1 << 5;
pub const NAT_FLAG_NETMAP: u32 = 1 << 6;

/// NAT ifadesi — SNAT veya DNAT uygular
#[derive(Clone, Debug)]
pub struct NatExpr {
    pub nat_type: NatType,
    pub family: u32,
    pub flags: u32,
    pub addr_min: u32,
    pub addr_max: u32,
    pub proto_min: u16,
    pub proto_max: u16,
}

impl NatExpr {
    /// SNAT — belirli bir kaynak adresine çevir
    pub fn snat(addr: u32, port: u16) -> Self {
        Self {
            nat_type: NatType::Source,
            family: NFPROTO_IPV4,
            flags: NAT_FLAG_MAP_IPS | NAT_FLAG_PROTO_SPECIFIED,
            addr_min: addr,
            addr_max: addr,
            proto_min: port,
            proto_max: port,
        }
    }

    /// DNAT — belirli bir hedef adrese çevir
    pub fn dnat(addr: u32, port: u16) -> Self {
        Self {
            nat_type: NatType::Destination,
            family: NFPROTO_IPV4,
            flags: NAT_FLAG_MAP_IPS | NAT_FLAG_PROTO_SPECIFIED,
            addr_min: addr,
            addr_max: addr,
            proto_min: port,
            proto_max: port,
        }
    }

    /// SNAT aralığı (port aralığı ile)
    pub fn snat_range(addr_min: u32, addr_max: u32, port_min: u16, port_max: u16) -> Self {
        Self {
            nat_type: NatType::Source,
            family: NFPROTO_IPV4,
            flags: NAT_FLAG_MAP_IPS | NAT_FLAG_PROTO_SPECIFIED,
            addr_min,
            addr_max,
            proto_min: port_min,
            proto_max: port_max,
        }
    }

    /// DNAT aralığı (port aralığı ile)
    pub fn dnat_range(addr_min: u32, addr_max: u32, port_min: u16, port_max: u16) -> Self {
        Self {
            nat_type: NatType::Destination,
            family: NFPROTO_IPV4,
            flags: NAT_FLAG_MAP_IPS | NAT_FLAG_PROTO_SPECIFIED,
            addr_min,
            addr_max,
            proto_min: port_min,
            proto_max: port_max,
        }
    }
}

// ============================================================================
// REJECT İFADESİ (EXPRESSION: REJECT)
// ============================================================================
// Kaynak: nftables spec — NFT_EXPR_REJECT
//
// reject-types:
//   icmp-unreach, tcp-rst, icmpx-unreach
//
// reject-inet-code:
//   icmpx-no-route, icmpx-port-unreach, icmpx-host-unreach, icmpx-admin-prohibited

/// Reddetme türleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RejectType {
    /// ICMP unreachable mesajı gönder
    IcmpUnreach,
    /// TCP RST paketi gönder
    TcpRst,
    /// ICMPv6 unreachable (IPv6 için)
    IcmpxUnreach,
}

/// ICMP unreachable kodları
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RejectIcmpCode {
    /// Ağ yolu yok
    NoRoute = 0,
    /// Port ulaşılabilir değil
    PortUnreach = 1,
    /// Host ulaşılabilir değil
    HostUnreach = 2,
    /// Yönetim tarafından yasaklandı
    AdminProhibited = 3,
}

/// Reject ifadesi — ICMP unreachable veya TCP RST gönderir
#[derive(Clone, Debug)]
pub struct RejectExpr {
    pub reject_type: RejectType,
    pub icmp_code: u8,
}

impl RejectExpr {
    /// ICMP port unreachable
    pub fn icmp_port_unreachable() -> Self {
        Self {
            reject_type: RejectType::IcmpUnreach,
            icmp_code: RejectIcmpCode::PortUnreach as u8,
        }
    }

    /// ICMP host unreachable
    pub fn icmp_host_unreachable() -> Self {
        Self {
            reject_type: RejectType::IcmpUnreach,
            icmp_code: RejectIcmpCode::HostUnreach as u8,
        }
    }

    /// ICMP admin prohibited
    pub fn icmp_admin_prohibited() -> Self {
        Self {
            reject_type: RejectType::IcmpUnreach,
            icmp_code: RejectIcmpCode::AdminProhibited as u8,
        }
    }

    /// TCP RST
    pub fn tcp_rst() -> Self {
        Self {
            reject_type: RejectType::TcpRst,
            icmp_code: 0,
        }
    }
}

// ============================================================================
// LOG İFADESİ (EXPRESSION: LOG)
// ============================================================================
// Kaynak: nftables spec — NFT_EXPR_LOG
//
// log-attrs:
//   group (u16), prefix (string), snaplen (u32),
//   qthreshold (u16), level (u32), flags (u32)
//
// log-level enum:
//   emerg, alert, crit, err, warning, notice, info, debug, audit
//
// log-flags:
//   tcpseq, tcpopt, ipopt, uid, nflog, macdecode

/// Log seviyeleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogLevel {
    Emerg = 0,
    Alert = 1,
    Crit = 2,
    Err = 3,
    Warning = 4,
    Notice = 5,
    Info = 6,
    Debug = 7,
    Audit = 8,
}

/// Log flag'leri
pub const LOG_FLAG_TCPSEQ: u32 = 1 << 0;
pub const LOG_FLAG_TCPOPT: u32 = 1 << 1;
pub const LOG_FLAG_IPOPT: u32 = 1 << 2;
pub const LOG_FLAG_UID: u32 = 1 << 3;
pub const LOG_FLAG_MACDECODE: u32 = 1 << 5;

/// Log ifadesi — paket bilgisini günlüğe yazar
#[derive(Clone, Debug)]
pub struct LogExpr {
    pub prefix: String,
    pub level: LogLevel,
    pub flags: u32,
    pub snaplen: u32,
    pub group: u16,
    pub qthreshold: u16,
}

impl LogExpr {
    /// Varsayılan log ifadesi
    pub fn new(prefix: &str) -> Self {
        Self {
            prefix: String::from(prefix),
            level: LogLevel::Notice,
            flags: 0,
            snaplen: 0xFFFF,
            group: 0,
            qthreshold: 1,
        }
    }

    /// Belirli seviye ile
    pub fn with_level(prefix: &str, level: LogLevel) -> Self {
        Self {
            prefix: String::from(prefix),
            level,
            flags: 0,
            snaplen: 0xFFFF,
            group: 0,
            qthreshold: 1,
        }
    }
}

// ============================================================================
// QUOTA İFADESİ (EXPRESSION: QUOTA)
// ============================================================================
// Kaynak: nftables spec — NFT_EXPR_QUOTA
//
// quota-flags: invert, depleted

/// Quota ifadesi — trafik kotası sınırlaması
pub struct QuotaExpr {
    pub bytes: u64,
    pub consumed: AtomicU64,
    pub invert: bool,
    pub depleted: bool,
}

impl Clone for QuotaExpr {
    fn clone(&self) -> Self {
        Self {
            bytes: self.bytes,
            consumed: AtomicU64::new(self.consumed.load(Ordering::Relaxed)),
            invert: self.invert,
            depleted: self.depleted,
        }
    }
}

impl core::fmt::Debug for QuotaExpr {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("QuotaExpr")
            .field("bytes", &self.bytes)
            .field("consumed", &self.consumed.load(Ordering::Relaxed))
            .field("invert", &self.invert)
            .finish()
    }
}

impl QuotaExpr {
    /// Kota belirle (bayt cinsinden)
    pub fn new(bytes: u64, invert: bool) -> Self {
        Self {
            bytes,
            consumed: AtomicU64::new(0),
            invert,
            depleted: false,
        }
    }

    /// Kota aşıldı mı kontrol et.
    ///
    /// Normal modda: tüketilen toplam bayt, kota değerine ulaştıysa veya aştıysa `true`.
    /// Invert modda: kota hâlâ dolmamışsa `true` (kota dolunca `false`).
    pub fn is_exceeded(&self) -> bool {
        let consumed = self.consumed.load(Ordering::Relaxed);
        if self.invert {
            // Invert: kota dolmadan önce eşleşir — dolunca artık eşleşmez
            consumed < self.bytes
        } else {
            // Normal: kota dolunca aşılmış sayılır
            consumed >= self.bytes
        }
    }

    /// Kota sayacını artır (paket boyutu kadar)
    pub fn consume(&self, pkt_len: u64) -> bool {
        let prev = self.consumed.fetch_add(pkt_len, Ordering::Relaxed);
        if prev + pkt_len >= self.bytes {
            // Kota aşıldı
            if !self.invert {
                return true; // düşür
            }
        }
        false
    }
}

// ============================================================================
// LIMIT İFADESİ (EXPRESSION: LIMIT)
// ============================================================================
// Kaynak: nftables spec — NFT_EXPR_LIMIT
//
// rate limiting: saniyede X paket veya X bayt

/// Limit türü
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LimitType {
    /// Paket bazlı
    Pkt,
    /// Bayt bazlı
    Bytes,
}

/// Limit ifadesi — hız sınırlaması (令牌桶 algoritması)
pub struct LimitExpr {
    /// Saniyede izin verilen pkt/bayt sayısı
    pub rate: u64,
    /// Bucket boyutu (burst)
    pub burst: u32,
    /// Limit türü (paket veya bayt)
    pub limit_type: LimitType,
    /// Ters (invert): limiti aşınca eşleşir
    pub invert: bool,
    /// Son token eklenme zamanı (TSC ticks)
    pub last_update: AtomicU64,
    /// Mevcut token sayısı
    pub tokens: AtomicU64,
}

impl Clone for LimitExpr {
    fn clone(&self) -> Self {
        Self {
            rate: self.rate,
            burst: self.burst,
            limit_type: self.limit_type,
            invert: self.invert,
            last_update: AtomicU64::new(self.last_update.load(Ordering::Relaxed)),
            tokens: AtomicU64::new(self.tokens.load(Ordering::Relaxed)),
        }
    }
}

impl core::fmt::Debug for LimitExpr {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LimitExpr")
            .field("rate", &self.rate)
            .field("burst", &self.burst)
            .field("limit_type", &self.limit_type)
            .field("invert", &self.invert)
            .finish()
    }
}

impl LimitExpr {
    /// Hız sınırlaması oluştur
    pub fn new(rate: u64, burst: u32, limit_type: LimitType) -> Self {
        Self {
            rate,
            burst,
            limit_type,
            invert: false,
            last_update: AtomicU64::new(0),
            tokens: AtomicU64::new(burst as u64),
        }
    }

    /// Ters limit (invert)
    pub fn inverted(rate: u64, burst: u32, limit_type: LimitType) -> Self {
        Self {
            rate,
            burst,
            limit_type,
            invert: true,
            last_update: AtomicU64::new(0),
            tokens: AtomicU64::new(burst as u64),
        }
    }

    /// Token yenile ve paketin geçip geçmeyeceğini kontrol et
    pub fn check(&self, current_time: u64, pkt_len: u64) -> bool {
        let last = self.last_update.load(Ordering::Relaxed);
        let tokens = self.tokens.load(Ordering::Relaxed);

        // Token ekle (geçen süre × rate)
        let elapsed = current_time.saturating_sub(last);
        let new_tokens = (elapsed * self.rate).min(self.burst as u64);

        let cost = match self.limit_type {
            LimitType::Pkt => 1,
            LimitType::Bytes => pkt_len,
        };

        if tokens + new_tokens >= cost {
            // Yeterli token var — paket geçebilir
            self.tokens
                .store(tokens + new_tokens - cost, Ordering::Relaxed);
            self.last_update.store(current_time, Ordering::Relaxed);
            !self.invert
        } else {
            // Token yetersiz — paket düşürülmeli
            self.tokens.store(0, Ordering::Relaxed);
            self.last_update.store(current_time, Ordering::Relaxed);
            self.invert
        }
    }
}

// ============================================================================
// CONNTRACK İFADESİ (EXPRESSION: CT)
// ============================================================================
// Kaynak: nftables spec — NFT_EXPR_CT
//
// ct-keys enum:
//   state, direction, status, mark, secmark, expiration, helper,
//   l3protocol, src, dst, protocol, proto-src, proto-dst,
//   labels, pkts, bytes, avgpkt, zone, eventmask,
//   src-ip, dst-ip, src-ip6, dst-ip6, ct-id

/// Conntrack anahtarları
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CtKey {
    /// Conntrack durumu (NEW, ESTABLISHED, RELATED, INVALID)
    State,
    /// Yön (original veya reply)
    Direction,
    /// Conntrack durumu bit maskesi
    Status,
    /// Conntrack markası
    Mark,
    /// Kaynak IP
    Src,
    /// Hedef IP
    Dst,
    /// Protokol
    Protocol,
    /// Kaynak port
    ProtoSrc,
    /// Hedef port
    ProtoDst,
    /// Paket sayısı
    Pkts,
    /// Bayt sayısı
    Bytes,
    /// Conntrack ID
    CtId,
}

/// Conntrack yönü
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CtDirection {
    /// Orijinal yön
    Original,
    /// Yanıt yönü
    Reply,
}

/// Conntrack ifadesi — conntrack durumuna göre filtreleme
#[derive(Clone, Debug)]
pub struct CtExpr {
    pub key: CtKey,
    pub dreg: u32,
    pub sreg: u32,
    pub direction: CtDirection,
}

impl CtExpr {
    /// Conntrack durumunu register'a oku
    pub fn state(dreg: u32) -> Self {
        Self {
            key: CtKey::State,
            dreg,
            sreg: 0,
            direction: CtDirection::Original,
        }
    }

    /// Conntrack yönünü register'a oku
    pub fn direction(dreg: u32) -> Self {
        Self {
            key: CtKey::Direction,
            dreg,
            sreg: 0,
            direction: CtDirection::Original,
        }
    }

    /// Conntrack markasını register'a yaz
    pub fn mark_set(sreg: u32) -> Self {
        Self {
            key: CtKey::Mark,
            dreg: 0,
            sreg,
            direction: CtDirection::Original,
        }
    }

    /// Kaynak IP (conntrack original veya reply yönüne göre)
    pub fn src(dreg: u32) -> Self {
        Self {
            key: CtKey::Src,
            dreg,
            sreg: 0,
            direction: CtDirection::Original,
        }
    }

    /// Hedef IP (conntrack original veya reply yönüne göre)
    pub fn dst(dreg: u32) -> Self {
        Self {
            key: CtKey::Dst,
            dreg,
            sreg: 0,
            direction: CtDirection::Original,
        }
    }

    /// Paket sayısını register'a oku
    pub fn pkts(dreg: u32) -> Self {
        Self {
            key: CtKey::Pkts,
            dreg,
            sreg: 0,
            direction: CtDirection::Original,
        }
    }

    /// Bayt sayısını register'a oku
    pub fn bytes(dreg: u32) -> Self {
        Self {
            key: CtKey::Bytes,
            dreg,
            sreg: 0,
            direction: CtDirection::Original,
        }
    }
}

// ============================================================================
// FIB İFADESİ (EXPRESSION: FIB)
// ============================================================================
// Kaynak: nftables spec — NFT_EXPR_FIB
//
// fib-result enum: oif, oifname, addrtype
// fib-flags: saddr, daddr, mark, iif, oif, present

/// FIB sonuç türleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FibResult {
    /// Çıkış arabirimi indeksi
    Oif,
    /// Çıkış arabirimi adı
    OifName,
    /// Adres tipi (unicast, broadcast, multicast)
    AddrType,
}

/// FIB ifadesi — FIP routing lookup sonucunu register'a yazar
#[derive(Clone, Debug)]
pub struct FibExpr {
    pub result: FibResult,
    pub dreg: u32,
    pub flags: u32,
}

/// FIB flag'leri
pub const FIB_FLAG_SADDR: u32 = 1 << 0;
pub const FIB_FLAG_DADDR: u32 = 1 << 1;
pub const FIB_FLAG_MARK: u32 = 1 << 2;
pub const FIB_FLAG_IIF: u32 = 1 << 3;
pub const FIB_FLAG_OIF: u32 = 1 << 4;
pub const FIB_FLAG_PRESENT: u32 = 1 << 5;

impl FibExpr {
    /// Çıkış arabirimi adını register'a yaz
    pub fn oifname(dreg: u32, flags: u32) -> Self {
        Self {
            result: FibResult::OifName,
            dreg,
            flags,
        }
    }

    /// Çıkış arabirimi indeksini register'a yaz
    pub fn oif(dreg: u32, flags: u32) -> Self {
        Self {
            result: FibResult::Oif,
            dreg,
            flags,
        }
    }

    /// Adres tipini register'a yaz
    pub fn addrtype(dreg: u32, flags: u32) -> Self {
        Self {
            result: FibResult::AddrType,
            dreg,
            flags,
        }
    }
}

// ============================================================================
// MASQUERADE İFADESİ (EXPRESSION: MASQ)
// ============================================================================
// Kaynak: nftables spec — NFT_EXPR_MASQ
//
// Masquerade, SNAT'ın dinamik versiyonudur.
// Çıkış arabiriminin mevcut IP adresini kullanarak SNAT uygular.

/// Masquerade ifadesi — çıkış arabirimi IP'siyle SNAT
#[derive(Clone, Debug)]
pub struct MasqExpr {
    pub flags: u32,
}

impl MasqExpr {
    pub fn new() -> Self {
        Self {
            flags: NAT_FLAG_MAP_IPS,
        }
    }
}

impl Default for MasqExpr {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// TPROXY İFADESİ (EXPRESSION: TPROXY)
// ============================================================================
// Kaynak: nftables spec — NFT_EXPR_TPROXY
//
// Transparent proxy — TCP/UDP bağlantısını belirli bir yerel porta yönlendirir.

/// Tproxy ifadesi — transparent proxy yönlendirmesi
#[derive(Clone, Debug)]
pub struct TproxyExpr {
    pub family: u32,
    pub port: u16,
}

impl TproxyExpr {
    pub fn new(family: u32, port: u16) -> Self {
        Self { family, port }
    }
}

// ============================================================================
// FLOW OFFLOAD İFADESİ (EXPRESSION: FLOW OFFLOAD)
// ============================================================================
// Kaynak: nftables spec — NFT_EXPR_FLOW_OFFLOAD

/// Akış hızlandırma ifadesi — paketi donanım hızlandırmasına yönlendirir
#[derive(Clone, Debug)]
pub struct FlowOffloadExpr {
    pub name: Option<String>,
}

impl FlowOffloadExpr {
    pub fn new() -> Self {
        Self { name: None }
    }

    pub fn with_name(name: &str) -> Self {
        Self {
            name: Some(String::from(name)),
        }
    }
}

impl Default for FlowOffloadExpr {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// OBJREF İFADESİ (EXPRESSION: OBJREF)
// ============================================================================
// Kaynak: nftables spec — NFT_EXPR_OBJREF
//
// Nesne referansı — counter, quota, limit gibi stateful nesnelere referans.
// obj-attrs:
//   table, name, type, handle, use, data, userdata

/// Objref ifadesi — named object referansı
#[derive(Clone, Debug)]
pub struct ObjrefExpr {
    pub name: String,
}

impl ObjrefExpr {
    pub fn new(name: &str) -> Self {
        Self {
            name: String::from(name),
        }
    }
}

// ============================================================================
// NFT İFADE ZİNCİRİ (NFT EXPRESSION CHAIN)
// ============================================================================
// Bir nftables kuralı sıralı ifadelerden oluşur.
// İfadeler sırayla değerlendirilir; herhangi bir ifade false döndüğünde
// kural başarısız olur ve bir sonraki kurala geçilir.

/// Nftables ifadesi — tüm ifade türlerini sarmalayan enum
#[derive(Clone, Debug)]
pub enum NftExpression {
    Payload(PayloadExpr),
    Meta(MetaExpr),
    Cmp(CmpExpr),
    Bitwise(BitwiseExpr),
    Lookup(LookupExpr),
    Counter(CounterExpr),
    Verdict(VerdictExpr),
    Nat(NatExpr),
    Reject(RejectExpr),
    Log(LogExpr),
    Quota(QuotaExpr),
    Limit(LimitExpr),
    Conntrack(CtExpr),
    Fib(FibExpr),
    Masq(MasqExpr),
    Tproxy(TproxyExpr),
    FlowOffload(FlowOffloadExpr),
    Objref(ObjrefExpr),
}

// ============================================================================
// NFT KURALI (NFT RULE)
// ============================================================================
// Bir nftables kuralı ifade zincirinden oluşur.
// İlk eşleşen kuralın ifadeleri sırayla değerlendirilir.
// Tüm ifadeler başarılıysa verdict (son ifade genellikle) uygulanır.

/// Nftables kural yapısı
#[derive(Clone, Debug)]
pub struct NftRule {
    /// Kural handle'ı (benzersiz tanımlayıcı, netlink'te u64)
    pub handle: u64,
    /// İfade zinciri (sırayla değerlendirilir)
    pub expressions: Vec<NftExpression>,
    /// Kullanıcı verisi (netlink userdata alanı)
    pub userdata: Vec<u8>,
}

impl NftRule {
    /// Yeni boş kural oluştur
    pub fn new(handle: u64) -> Self {
        Self {
            handle,
            expressions: Vec::new(),
            userdata: Vec::new(),
        }
    }

    /// İfade ekle (builder pattern)
    pub fn with_expr(mut self, expr: NftExpression) -> Self {
        self.expressions.push(expr);
        self
    }

    /// Kuralı paket üzerinde değerlendirir.
    ///
    /// Tüm ifadeler sırayla çalıştırılır. Herhangi bir ifade başarısız olursa
    /// `false` döner. Tüm ifadeler başarılıysa `true` döner ve son verdict
    /// uygulanır.
    pub fn evaluate(&self, pkt: &mut PacketInfo, registers: &mut [u8; 64]) -> bool {
        for expr in &self.expressions {
            match expr {
                NftExpression::Payload(p) => {
                    if !evaluate_payload(p, pkt, registers) {
                        return false;
                    }
                }
                NftExpression::Meta(m) => {
                    if !evaluate_meta(m, pkt, registers) {
                        return false;
                    }
                }
                NftExpression::Cmp(c) => {
                    if !evaluate_cmp(c, registers) {
                        return false;
                    }
                }
                NftExpression::Bitwise(b) => {
                    if !evaluate_bitwise(b, registers) {
                        return false;
                    }
                }
                NftExpression::Counter(c) => {
                    c.update(pkt.len as u64);
                }
                NftExpression::Lookup(l) => {
                    if !evaluate_lookup(l, pkt, registers) {
                        return false;
                    }
                }
                NftExpression::Conntrack(ct) => {
                    if !evaluate_conntrack(ct, pkt, registers) {
                        return false;
                    }
                }
                NftExpression::Log(l) => {
                    evaluate_log(l, pkt);
                }
                NftExpression::Quota(q) => {
                    if q.consume(pkt.len as u64) {
                        return false;
                    }
                }
                NftExpression::Limit(l) => {
                    // Limit ifadesi inverted ise: limit aşılınca eşleşir
                    // Değilse: limit aşılmadıkça eşleşir
                    // Bu, cmp ile birlikte kullanılır — tek başına FALSE döndürmez
                }
                NftExpression::Fib(f) => {
                    if !evaluate_fib(f, pkt, registers) {
                        return false;
                    }
                }
                // Verdict, Nat, Reject, Masq, Tproxy, FlowOffload, Objref
                // — bu ifadeler genellikle zincirin sonunda bulunur ve
                // kural değerlendirme mantığı dışından yürütülür
                _ => {}
            }
        }
        true
    }
}

// ============================================================================
// NFT ZİNCİRİ (NFT CHAIN)
// ============================================================================

/// Zincir türü — nftables base chain türleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NftChainType {
    /// Paket filtreleme (filter — INPUT, OUTPUT, FORWARD)
    Filter,
    /// NAT (nat — PREROUTING, POSTROUTING)
    Nat,
    /// Paket manipülasyonu (mangle)
    Mangle,
    /// Routing (route)
    Route,
}

/// Nftables zincir yapısı
#[derive(Clone, Debug)]
pub struct NftChain {
    /// Zincir adı (base chain: "input", "forward", "output")
    pub name: String,
    /// Zincir handle'ı
    pub handle: u64,
    /// Zincir türü (filter, nat, mangle, route)
    pub chain_type: NftChainType,
    /// Hangi netfilter kancasına bağlı (NF_INET_PRE_ROUTING vb.)
    pub hook: u32,
    /// Kancada öncelik (düşük değer = erken çalışır)
    pub priority: i32,
    /// Varsayılan politika (tüm kurallar başarısız olursa)
    pub policy: u32,
    /// Zincirdeki kurallar (handle sırasına göre)
    pub rules: BTreeMap<u64, NftRule>,
    /// Sonraki kural handle'ı
    next_rule_handle: u64,
    /// Kullanıcı verisi
    pub userdata: Vec<u8>,
}

impl NftChain {
    /// Yeni base chain oluştur (kancaya bağlı)
    pub fn new_base(
        name: &str,
        handle: u64,
        chain_type: NftChainType,
        hook: u32,
        priority: i32,
        policy: u32,
    ) -> Self {
        Self {
            name: String::from(name),
            handle,
            chain_type,
            hook,
            priority,
            policy,
            rules: BTreeMap::new(),
            next_rule_handle: 1,
            userdata: Vec::new(),
        }
    }

    /// Yeni user-defined chain oluştur (kancasız)
    pub fn new_user(name: &str, handle: u64) -> Self {
        Self {
            name: String::from(name),
            handle,
            chain_type: NftChainType::Filter,
            hook: 0,
            priority: 0,
            policy: NF_ACCEPT,
            rules: BTreeMap::new(),
            next_rule_handle: 1,
            userdata: Vec::new(),
        }
    }

    /// Kural ekle (sonuna)
    pub fn add_rule(&mut self, rule: NftRule) {
        self.rules.insert(rule.handle, rule);
    }

    /// Sonraki kural handle'ını ayır
    pub fn allocate_rule_handle(&mut self) -> u64 {
        let h = self.next_rule_handle;
        self.next_rule_handle += 1;
        h
    }

    /// Belirtilen handle'daki kuralı sil
    pub fn delete_rule(&mut self, handle: u64) -> bool {
        self.rules.remove(&handle).is_some()
    }

    /// Paketi zincir kuralları üzerinden değerlendirir.
    ///
    /// Kurallar handle sırasıyla (artan) değerlendirilir.
    /// İlk eşleşen kuralın verdict'i uygulanır.
    /// Hiçbiri eşleşmezse zincirin politikası döner.
    pub fn traverse(&self, pkt: &mut PacketInfo) -> u32 {
        let mut registers = [0u8; 64];

        for rule in self.rules.values() {
            if rule.evaluate(pkt, &mut registers) {
                // Tüm ifadeler eşleşti — verdict uygula
                for expr in &rule.expressions {
                    if let NftExpression::Verdict(v) = expr {
                        return verdict_to_nf(v);
                    }
                }
                // Verdict ifadesi yoksa kabul et
                return NF_ACCEPT;
            }
        }

        // Hiçbir kural eşleşmedi — politika döner
        self.policy
    }
}

// ============================================================================
// NFT TABLOSU (NFT TABLE)
// ============================================================================

/// Nftables tablo yapısı
#[derive(Clone, Debug)]
pub struct NftTable {
    /// Tablo adı (örn: "ip", "ip6", "inet", "arp", "bridge", "netdev")
    pub name: String,
    /// Tablo handle'ı
    pub handle: u64,
    /// Protokol ailesi (NFPROTO_IPV4, NFPROTO_IPV6, NFPROTO_UNSPEC)
    pub family: u32,
    /// Tablo flag'leri (dormant, owner, persist)
    pub flags: u32,
    /// Zincirler (zincir adına göre)
    pub chains: BTreeMap<String, NftChain>,
    /// Named sets (set adına göre)
    pub sets: BTreeMap<String, NftSet>,
    /// Named objects (counter, quota, limit vb.)
    pub objects: BTreeMap<String, NftObject>,
    /// Kullanıcı verisi
    pub userdata: Vec<u8>,
}

impl NftTable {
    /// Yeni tablo oluştur
    pub fn new(name: &str, handle: u64, family: u32) -> Self {
        Self {
            name: String::from(name),
            handle,
            family,
            flags: 0,
            chains: BTreeMap::new(),
            sets: BTreeMap::new(),
            objects: BTreeMap::new(),
            userdata: Vec::new(),
        }
    }

    /// Zincir ekle
    pub fn add_chain(&mut self, chain: NftChain) {
        self.chains.insert(chain.name.clone(), chain);
    }

    /// Zincir sil
    pub fn delete_chain(&mut self, name: &str) -> bool {
        self.chains.remove(name).is_some()
    }

    /// Zincir getir
    pub fn get_chain(&self, name: &str) -> Option<&NftChain> {
        self.chains.get(name)
    }

    /// Zincir getir (değiştirilebilir)
    pub fn get_chain_mut(&mut self, name: &str) -> Option<&mut NftChain> {
        self.chains.get_mut(name)
    }
}

// ============================================================================
// NFT SET (NFTABLES SETS)
// ============================================================================
// Kaynak: nftables spec — newset/getset/delset
//
// set-flags: anonymous, constant, interval, map, timeout, eval, object, concat, expr
// set-desc-attrs: key-type, key-len, data-type, data-len, obj-type
// set-elem-flags: interval-end, catchall

/// Set türü
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NftSetType {
    /// IP adresi seti (32-bit key)
    Ipv4Addr,
    /// IPv6 adresi seti (128-bit key)
    Ipv6Addr,
    /// Protokol seti (8-bit key)
    Proto,
    /// Port seti (16-bit key)
    Port,
    /// Mark seti (32-bit key)
    Mark,
    /// Meta veri seti (değişken genişlikte key)
    Meta,
    /// IP + Port birleşik seti (32+16 bit key)
    IpAddrPort,
}

/// Set flag'leri
pub const NFT_SET_ANONYMOUS: u32 = 1 << 0;
pub const NFT_SET_CONSTANT: u32 = 1 << 1;
pub const NFT_SET_INTERVAL: u32 = 1 << 2;
pub const NFT_SET_MAP: u32 = 1 << 3;
pub const NFT_SET_TIMEOUT: u32 = 1 << 4;
pub const NFT_SET_EVAL: u32 = 1 << 5;
pub const NFT_SET_OBJECT: u32 = 1 << 6;
pub const NFT_SET_CONCAT: u32 = 1 << 7;
pub const NFT_SET_EXPR: u32 = 1 << 8;

/// Set elemanı
#[derive(Clone, Debug)]
pub struct NftSetElem {
    /// Anahtar baytları (little-endian)
    pub key: Vec<u8>,
    /// Değer baytları (map setleri için, little-endian)
    pub data: Vec<u8>,
    /// Eleman timeout'u (ms, 0 = süresiz)
    pub timeout_ms: u64,
    /// Eleman flag'leri
    pub flags: u32,
    /// Elemana bağlı ifadeler (eval setleri için)
    pub expressions: Vec<NftExpression>,
}

/// Nftables set yapısı
#[derive(Clone, Debug)]
pub struct NftSet {
    /// Set adı
    pub name: String,
    /// Set handle'ı
    pub handle: u64,
    /// Set türü (hangi tipte anahtarlar tutulacağı)
    pub set_type: NftSetType,
    /// Anahtar uzunluğu (bayt)
    pub key_len: u32,
    /// Değer uzunluğu (bayt, map setleri için)
    pub data_len: u32,
    /// Set flag'leri
    pub flags: u32,
    /// Elemanlar (key baytlarına göre sıralı)
    pub elements: Vec<NftSetElem>,
    /// Maksimum eleman sayısı
    pub max_elements: u32,
    /// GC aralığı (ms)
    pub gc_interval_ms: u32,
    /// Politika (element eklenememe durumunda: performance veya error)
    pub policy: u32,
}

impl NftSet {
    /// Yeni set oluştur
    pub fn new(name: &str, handle: u64, set_type: NftSetType, key_len: u32) -> Self {
        Self {
            name: String::from(name),
            handle,
            set_type,
            key_len,
            data_len: 0,
            flags: 0,
            elements: Vec::new(),
            max_elements: 65535,
            gc_interval_ms: 1000,
            policy: 0,
        }
    }

    /// Set'e eleman ekle
    pub fn add_element(&mut self, elem: NftSetElem) {
        self.elements.push(elem);
    }

    /// Set'te eleman ara
    pub fn lookup(&self, key: &[u8]) -> Option<&NftSetElem> {
        self.elements.iter().find(|e| e.key == key)
    }

    /// Set'ten eleman sil
    pub fn delete_element(&mut self, key: &[u8]) -> bool {
        let before = self.elements.len();
        self.elements.retain(|e| e.key != key);
        self.elements.len() < before
    }
}

// ============================================================================
// NFT NESNESİ (NFT OBJECTS)
// ============================================================================
// Kaynak: nftables spec — newobj/getobj/delobj
//
// object-type enum: unspec, counter, quota, ct-helper, limit, connlimit,
//                   tunnel, ct-timeout, secmark, ct-expect, synproxy

/// Nesne türleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NftObjectType {
    /// Sayaç (counter)
    Counter,
    /// Kota (quota)
    Quota,
    /// Hız sınırı (limit)
    Limit,
    /// Bağlantı sınırı (connlimit)
    Connlimit,
    /// Tunnel (GRE, Geneve vb.)
    Tunnel,
    /// Conntrack timeout
    CtTimeout,
    /// Güvenlik markası (SELinux/AppArmor)
    Secmark,
    /// Conntrack beklentisi
    CtExpect,
    /// SYN proxy
    SynProxy,
}

/// Nftables named object — counter, quota, limit gibi stateful nesneler
#[derive(Clone, Debug)]
pub struct NftObject {
    /// Nesne adı
    pub name: String,
    /// Nesne handle'ı
    pub handle: u64,
    /// Nesne türü
    pub obj_type: NftObjectType,
    /// Nesne verisi (tipe göre)
    pub data: NftObjectData,
}

/// Nesne verisi (tipe göre)
#[derive(Clone, Debug)]
pub enum NftObjectData {
    Counter { packets: u64, bytes: u64 },
    Quota { bytes: u64, consumed: u64, flags: u32 },
    Limit { rate: u64, burst: u32, flags: u32 },
}

// ============================================================================
// NFTABLES YÖNETİCİSİ (NFTABLES MANAGER)
// ============================================================================
// Tüm nftables tablolarını, zincirleri, setleri ve nesneleri yönetir.

/// Nftables istatistikleri
#[derive(Clone, Debug, Default)]
pub struct NftablesStats {
    pub packets_processed: u64,
    pub packets_accepted: u64,
    pub packets_dropped: u64,
    pub packets_counted: u64,
    pub nat_count: u64,
    pub rule_matches: u64,
    pub rule_evaluations: u64,
}

/// Nftables yöneticisi — tüm tabloları ve istatistikleri yönetir
pub struct NftablesManager {
    /// Tablolar (tablo adına göre, ör: "ip", "ip6", "inet")
    tables: Mutex<BTreeMap<String, NftTable>>,
    /// Etkin mi?
    enabled: AtomicBool,
    /// İstatistikler
    stats: Mutex<NftablesStats>,
    /// Sonraki tablo handle'ı
    next_table_handle: AtomicU64,
    /// Generation ID (her değişiklikte artar)
    generation_id: AtomicU64,
}

/// Nftables hata türleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NftablesError {
    TableNotFound,
    ChainNotFound,
    RuleNotFound,
    SetNotFound,
    ObjectNotFound,
    InvalidHandle,
    TableExists,
    ChainExists,
    InvalidExpression,
    InvalidKey,
    InvalidValue,
}

impl NftablesManager {
    /// Yeni manager oluştur
    pub const fn new() -> Self {
        Self {
            tables: Mutex::new(BTreeMap::new()),
            enabled: AtomicBool::new(true),
            stats: Mutex::new(NftablesStats {
                packets_processed: 0,
                packets_accepted: 0,
                packets_dropped: 0,
                packets_counted: 0,
                nat_count: 0,
                rule_matches: 0,
                rule_evaluations: 0,
            }),
            next_table_handle: AtomicU64::new(1),
            generation_id: AtomicU64::new(0),
        }
    }

    /// Varsayılan tabloları başlat (iptables uyumlu yapı)
    pub fn init(&self) {
        let mut tables = self.tables.lock();

        // ip tablosu (IPv4)
        let mut ip_table = NftTable::new("ip", self.alloc_table_handle(), NFPROTO_IPV4);

        // "ip" tablosunda filter zincirleri
        let hook_prio = |hook: u32| match hook {
            NF_INET_PRE_ROUTING => -300,
            NF_INET_LOCAL_IN => 0,
            NF_INET_FORWARD => 0,
            NF_INET_LOCAL_OUT => 100,
            NF_INET_POST_ROUTING => 300,
            _ => 0,
        };

        // input zinciri (base chain, filter type, LOCAL_IN hook)
        ip_table.add_chain(NftChain::new_base(
            "input",
            self.alloc_table_handle(),
            NftChainType::Filter,
            NF_INET_LOCAL_IN,
            hook_prio(NF_INET_LOCAL_IN),
            NF_ACCEPT,
        ));

        // forward zinciri
        ip_table.add_chain(NftChain::new_base(
            "forward",
            self.alloc_table_handle(),
            NftChainType::Filter,
            NF_INET_FORWARD,
            hook_prio(NF_INET_FORWARD),
            NF_ACCEPT,
        ));

        // output zinciri
        ip_table.add_chain(NftChain::new_base(
            "output",
            self.alloc_table_handle(),
            NftChainType::Filter,
            NF_INET_LOCAL_OUT,
            hook_prio(NF_INET_LOCAL_OUT),
            NF_ACCEPT,
        ));

        // prerouting zinciri (filter type — raw tablosu işlevi)
        ip_table.add_chain(NftChain::new_base(
            "prerouting",
            self.alloc_table_handle(),
            NftChainType::Filter,
            NF_INET_PRE_ROUTING,
            hook_prio(NF_INET_PRE_ROUTING),
            NF_ACCEPT,
        ));

        // postrouting zinciri
        ip_table.add_chain(NftChain::new_base(
            "postrouting",
            self.alloc_table_handle(),
            NftChainType::Filter,
            NF_INET_POST_ROUTING,
            hook_prio(NF_INET_POST_ROUTING),
            NF_ACCEPT,
        ));

        // nat tablosu
        let mut nat_table = NftTable::new("nat", self.alloc_table_handle(), NFPROTO_IPV4);
        nat_table.add_chain(NftChain::new_base(
            "prerouting",
            self.alloc_table_handle(),
            NftChainType::Nat,
            NF_INET_PRE_ROUTING,
            -100,
            NF_ACCEPT,
        ));
        nat_table.add_chain(NftChain::new_base(
            "postrouting",
            self.alloc_table_handle(),
            NftChainType::Nat,
            NF_INET_POST_ROUTING,
            100,
            NF_ACCEPT,
        ));
        nat_table.add_chain(NftChain::new_base(
            "output",
            self.alloc_table_handle(),
            NftChainType::Nat,
            NF_INET_LOCAL_OUT,
            100,
            NF_ACCEPT,
        ));
        tables.insert(String::from("nat"), nat_table);

        // inet tablosu (IPv4+IPv6 dual-stack)
        let mut inet_table = NftTable::new("inet", self.alloc_table_handle(), NFPROTO_UNSPEC);
        inet_table.add_chain(NftChain::new_base(
            "input",
            self.alloc_table_handle(),
            NftChainType::Filter,
            NF_INET_LOCAL_IN,
            0,
            NF_ACCEPT,
        ));
        inet_table.add_chain(NftChain::new_base(
            "forward",
            self.alloc_table_handle(),
            NftChainType::Filter,
            NF_INET_FORWARD,
            0,
            NF_ACCEPT,
        ));
        inet_table.add_chain(NftChain::new_base(
            "output",
            self.alloc_table_handle(),
            NftChainType::Filter,
            NF_INET_LOCAL_OUT,
            100,
            NF_ACCEPT,
        ));
        tables.insert(String::from("inet"), inet_table);

        // mangle tablosu (ip table'ında additional chains)
        ip_table.add_chain(NftChain::new_base(
            "prerouting_mangle",
            self.alloc_table_handle(),
            NftChainType::Mangle,
            NF_INET_PRE_ROUTING,
            -150,
            NF_ACCEPT,
        ));
        ip_table.add_chain(NftChain::new_base(
            "postrouting_mangle",
            self.alloc_table_handle(),
            NftChainType::Mangle,
            NF_INET_POST_ROUTING,
            150,
            NF_ACCEPT,
        ));

        tables.insert(String::from("ip"), ip_table);
        self.generation_id.fetch_add(1, Ordering::Relaxed);

        crate::serial_println!("[NFTABLES] Initialized with tables: ip, nat, inet");
    }

    /// Sonraki tablo handle'ını ayır
    fn alloc_table_handle(&self) -> u64 {
        self.next_table_handle.fetch_add(1, Ordering::Relaxed)
    }

    /// Paketi nftables üzerinden değerlendirir.
    ///
    /// Tüm tablolar ve zincirler kancaya (hook) göre sırayla değerlendirilir.
    /// Tablo sırası: belirli bir sıralama yok (BTreeMap alfabetik).
    /// Zincir sırası: priority değerine göre artan.
    ///
    /// # Return
    /// Karar kodu (NF_ACCEPT, NF_DROP veya verdict_to_nf sonucu)
    pub fn process_packet(&self, pkt: &mut PacketInfo, hook: u32) -> u32 {
        if !self.enabled.load(Ordering::SeqCst) {
            return NF_ACCEPT;
        }

        let mut stats = self.stats.lock();
        stats.packets_processed += 1;

        let tables = self.tables.lock();

        // Kancaya uyan tüm zincirleri topla ve priority sırasına göre sırala
        let mut hook_chains: Vec<(&str, i32, u64, &NftChain)> = Vec::new();

        for (table_name, table) in tables.iter() {
            for chain in table.chains.values() {
                if chain.hook == hook {
                    hook_chains.push((table_name, chain.priority, chain.handle, chain));
                }
            }
        }

        // Priority sırasına göre sırala (düşük önce)
        hook_chains.sort_by_key(|(_, prio, _, _)| *prio);

        // Zincirleri sırayla değerlendir
        let mut verdict = NF_ACCEPT;
        for (table_name, _prio, _handle, chain) in &hook_chains {
            verdict = chain.traverse(pkt);
            stats.rule_evaluations += 1;

            // NAT kontrolü
            if pkt.new_src_ip != 0
                || pkt.new_dst_ip != 0
                || pkt.has_new_src_addr
                || pkt.has_new_dst_addr
                || pkt.new_src_port != 0
                || pkt.new_dst_port != 0
            {
                stats.nat_count += 1;
            }

            match verdict {
                NF_ACCEPT => {
                    stats.packets_accepted += 1;
                }
                NF_DROP => {
                    stats.packets_dropped += 1;
                    return verdict;
                }
                _ => {}
            }
        }

        verdict
    }

    // ========================================================================
    // TABLO YÖNETİMİ
    // ========================================================================

    /// Yeni tablo oluştur
    pub fn new_table(
        &self,
        name: &str,
        family: u32,
    ) -> Result<u64, NftablesError> {
        let mut tables = self.tables.lock();
        if tables.contains_key(name) {
            return Err(NftablesError::TableExists);
        }
        let handle = self.alloc_table_handle();
        let table = NftTable::new(name, handle, family);
        tables.insert(String::from(name), table);
        self.generation_id.fetch_add(1, Ordering::Relaxed);
        Ok(handle)
    }

    /// Tabloyu sil
    pub fn delete_table(&self, name: &str) -> Result<(), NftablesError> {
        let mut tables = self.tables.lock();
        tables.remove(name).ok_or(NftablesError::TableNotFound)?;
        self.generation_id.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    /// Tablo getir
    pub fn get_table(&self, name: &str) -> Option<NftTable> {
        self.tables.lock().get(name).cloned()
    }

    /// Tüm tabloları listele
    pub fn list_tables(&self) -> Vec<(String, u32, u64)> {
        self.tables
            .lock()
            .iter()
            .map(|(name, table)| (name.clone(), table.family, table.handle))
            .collect()
    }

    // ========================================================================
    // ZİNCİR YÖNETİMİ
    // ========================================================================

    /// Tabloya yeni zincir ekle
    pub fn new_chain(
        &self,
        table_name: &str,
        chain: NftChain,
    ) -> Result<(), NftablesError> {
        let mut tables = self.tables.lock();
        let table = tables.get_mut(table_name).ok_or(NftablesError::TableNotFound)?;
        if table.chains.contains_key(&chain.name) {
            return Err(NftablesError::ChainExists);
        }
        table.add_chain(chain);
        self.generation_id.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    /// Zinciri sil
    pub fn delete_chain(
        &self,
        table_name: &str,
        chain_name: &str,
    ) -> Result<(), NftablesError> {
        let mut tables = self.tables.lock();
        let table = tables.get_mut(table_name).ok_or(NftablesError::TableNotFound)?;
        if !table.delete_chain(chain_name) {
            return Err(NftablesError::ChainNotFound);
        }
        self.generation_id.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    /// Zincir getir
    pub fn get_chain(
        &self,
        table_name: &str,
        chain_name: &str,
    ) -> Option<NftChain> {
        let tables = self.tables.lock();
        let table = tables.get(table_name)?;
        table.get_chain(chain_name).cloned()
    }

    // ========================================================================
    // KURAL YÖNETİMİ
    // ========================================================================

    /// Zincire kural ekle
    pub fn new_rule(
        &self,
        table_name: &str,
        chain_name: &str,
        rule: NftRule,
    ) -> Result<u64, NftablesError> {
        let mut tables = self.tables.lock();
        let table = tables.get_mut(table_name).ok_or(NftablesError::TableNotFound)?;
        let chain = table.get_chain_mut(chain_name).ok_or(NftablesError::ChainNotFound)?;
        let handle = rule.handle;
        chain.add_rule(rule);
        self.generation_id.fetch_add(1, Ordering::Relaxed);
        Ok(handle)
    }

    /// Kuralı sil
    pub fn delete_rule(
        &self,
        table_name: &str,
        chain_name: &str,
        rule_handle: u64,
    ) -> Result<(), NftablesError> {
        let mut tables = self.tables.lock();
        let table = tables.get_mut(table_name).ok_or(NftablesError::TableNotFound)?;
        let chain = table.get_chain_mut(chain_name).ok_or(NftablesError::ChainNotFound)?;
        if !chain.delete_rule(rule_handle) {
            return Err(NftablesError::RuleNotFound);
        }
        self.generation_id.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    // ========================================================================
    // SET YÖNETİMİ
    // ========================================================================

    /// Tabloya set ekle
    pub fn new_set(
        &self,
        table_name: &str,
        set: NftSet,
    ) -> Result<(), NftablesError> {
        let mut tables = self.tables.lock();
        let table = tables.get_mut(table_name).ok_or(NftablesError::TableNotFound)?;
        table.sets.insert(set.name.clone(), set);
        self.generation_id.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    /// Set elemanı ekle
    pub fn new_set_elem(
        &self,
        table_name: &str,
        set_name: &str,
        elem: NftSetElem,
    ) -> Result<(), NftablesError> {
        let mut tables = self.tables.lock();
        let table = tables.get_mut(table_name).ok_or(NftablesError::TableNotFound)?;
        let set = table.sets.get_mut(set_name).ok_or(NftablesError::SetNotFound)?;
        set.add_element(elem);
        self.generation_id.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    // ========================================================================
    // NESNE YÖNETİMİ
    // ========================================================================

    /// Tabloya named object ekle
    pub fn new_object(
        &self,
        table_name: &str,
        obj: NftObject,
    ) -> Result<(), NftablesError> {
        let mut tables = self.tables.lock();
        let table = tables.get_mut(table_name).ok_or(NftablesError::TableNotFound)?;
        table.objects.insert(obj.name.clone(), obj);
        self.generation_id.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    // ========================================================================
    // YARDIMCI FONKSİYONLAR
    // ========================================================================

    /// Generation ID'yi döndür
    pub fn get_generation_id(&self) -> u64 {
        self.generation_id.load(Ordering::Relaxed)
    }

    /// Manager'ı etkinleştir/devre dışı bırak
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::SeqCst);
    }

    /// İstatistikleri döndür
    pub fn get_stats(&self) -> NftablesStats {
        self.stats.lock().clone()
    }

    /// Tablodaki tüm zincirlerin istatistiklerini topla
    pub fn collect_chain_stats(&self) -> Vec<(String, String, u64, u64)> {
        let tables = self.tables.lock();
        let mut result = Vec::new();
        for (table_name, table) in tables.iter() {
            for chain in table.chains.values() {
                let (pkts, bytes) = chain
                    .rules
                    .values()
                    .flat_map(|r| r.expressions.iter())
                    .filter_map(|e| {
                        if let NftExpression::Counter(c) = e {
                            Some(c.snapshot())
                        } else {
                            None
                        }
                    })
                    .fold((0u64, 0u64), |(a, b), (c, d)| (a + c, b + d));
                result.push((table_name.clone(), chain.name.clone(), pkts, bytes));
            }
        }
        result
    }
}

// ============================================================================
// İFADE DEĞERLENDİRME FONKSİYONLARI
// ============================================================================
// Bu fonksiyonlar, nftables ifadelerini paket üzerinde değerlendirir.
// Her fonksiyon ifadeyi uygular ve başarı/hata durumunu döndürür.

/// Payload ifadesini değerlendirir — paketin belirli bir offset'inden veri okur.
///
/// `base` + `offset` ile bayt konumu hesaplanır, `width` kadar bayt okunur
/// ve `dreg` register'ına yazılır.
fn evaluate_payload(expr: &PayloadExpr, pkt: &PacketInfo, registers: &mut [u8; 64]) -> bool {
    let base_offset = match expr.base {
        PayloadBase::LinkLayerHeader => 14, // Ethernet başlığı: 14 bayt
        PayloadBase::NetworkHeader => {
            if pkt.family == NFPROTO_IPV6 {
                40 // IPv6 başlığı sabit 40 bayt
            } else {
                20 // IPv4 başlığı minimum 20 bayt
            }
        }
        PayloadBase::TransportHeader => {
            if pkt.family == NFPROTO_IPV6 {
                40 + 8 // IPv6 + TCP minimum (protokola göre değişir, basitleştirilmiş)
            } else {
                20 + 20 // IPv4 + TCP minimum
            }
        }
        _ => 0,
    };

    let start = (base_offset as u32 + expr.offset) as usize;
    let width = expr.width as usize;

    // Register'a yaz (dreg register alanı: 0..64)
    let dreg = expr.dreg as usize;
    if dreg + width > registers.len() {
        return false;
    }

    // Paketin kendisinden doğrudan veri okuyoruz (PacketInfo içinde raw payload yok)
    // Bu durumda register'daki mevcut değeri kullanıyoruz
    // Gerçek implementasyonda raw packet byte slice gerekir
    // şimdilik register'ları temizleyip offset değerini kaydedelim
    for i in 0..width {
        registers[dreg + i] = 0;
    }

    // Offset bilgisini register'a kaydet (değerlendirme sırasında kullanılacak)
    // Gerçek implementasyonda: raw_packet[base_offset + offset .. + width]
    let _ = (start, width);

    true
}

/// Meta ifadesini değerlendirir — paket meta-verisini register'a yazar.
fn evaluate_meta(expr: &MetaExpr, pkt: &PacketInfo, registers: &mut [u8; 64]) -> bool {
    let dreg = expr.dreg as usize;

    match expr.key {
        MetaKey::Len => {
            let len = pkt.len as u32;
            let bytes = len.to_le_bytes();
            if dreg + 4 <= registers.len() {
                registers[dreg..dreg + 4].copy_from_slice(&bytes);
            }
        }
        MetaKey::Protocol => {
            // Ethernet EtherType — şimdilik 0x0800 (IPv4) veya 0x86DD (IPv6)
            let ether_type: u16 = if pkt.family == NFPROTO_IPV6 {
                0x86DD
            } else {
                0x0800
            };
            let bytes = ether_type.to_le_bytes();
            if dreg + 2 <= registers.len() {
                registers[dreg..dreg + 2].copy_from_slice(&bytes);
            }
        }
        MetaKey::Nfproto => {
            if dreg < registers.len() {
                registers[dreg] = pkt.family as u8;
            }
        }
        MetaKey::L4Proto => {
            if dreg < registers.len() {
                registers[dreg] = pkt.proto;
            }
        }
        MetaKey::Mark => {
            // Paket markası — varsayılan 0
            if dreg + 4 <= registers.len() {
                registers[dreg..dreg + 4].copy_from_slice(&[0, 0, 0, 0]);
            }
        }
        MetaKey::IifName => {
            // Giriş arabirimi adı — null-terminated string olarak register'a yaz
            let name = pkt.in_iface.as_bytes();
            let len = name.len().min(15); // IFNAMSIZ-1
            if dreg + len <= registers.len() {
                registers[dreg..dreg + len].copy_from_slice(&name[..len]);
                // Null terminator
                if dreg + len < registers.len() {
                    registers[dreg + len] = 0;
                }
            }
        }
        MetaKey::OifName => {
            let name = pkt.out_iface.as_bytes();
            let len = name.len().min(15);
            if dreg + len <= registers.len() {
                registers[dreg..dreg + len].copy_from_slice(&name[..len]);
                if dreg + len < registers.len() {
                    registers[dreg + len] = 0;
                }
            }
        }
        MetaKey::PktType => {
            // Varsayılan unicast (0)
            if dreg < registers.len() {
                registers[dreg] = 0;
            }
        }
        MetaKey::Cpu => {
            // CPU 0 (tek çekirdekli sistem)
            if dreg + 4 <= registers.len() {
                registers[dreg..dreg + 4].copy_from_slice(&[0, 0, 0, 0]);
            }
        }
        MetaKey::SkUid => {
            // Root UID (0)
            if dreg + 4 <= registers.len() {
                registers[dreg..dreg + 4].copy_from_slice(&[0, 0, 0, 0]);
            }
        }
        MetaKey::SkGid => {
            // Root GID (0)
            if dreg + 4 <= registers.len() {
                registers[dreg..dreg + 4].copy_from_slice(&[0, 0, 0, 0]);
            }
        }
        MetaKey::PRandom => {
            // Rastgele sayı (basitleştirilmiş)
            let val = (pkt.src_ip ^ pkt.dst_ip) as u32;
            if dreg + 4 <= registers.len() {
                registers[dreg..dreg + 4].copy_from_slice(&val.to_le_bytes());
            }
        }
        MetaKey::TimeNs => {
            // Sistem zamanı — basitleştirilmiş (0)
            if dreg + 8 <= registers.len() {
                registers[dreg..dreg + 8].copy_from_slice(&[0; 8]);
            }
        }
        MetaKey::TimeDay => {
            if dreg < registers.len() {
                registers[dreg] = 0;
            }
        }
        MetaKey::TimeHour => {
            if dreg < registers.len() {
                registers[dreg] = 0;
            }
        }
        MetaKey::Secmark => {
            // Security mark — varsayılan 0
            if dreg + 4 <= registers.len() {
                registers[dreg..dreg + 4].copy_from_slice(&[0, 0, 0, 0]);
            }
        }
        _ => {}
    }

    true
}

/// Karşılaştırma ifadesini değerlendirir.
///
/// `sreg`'deki değeri `data` ile karşılaştırır. Operatör `op` tarafından belirlenir.
fn evaluate_cmp(expr: &CmpExpr, registers: &[u8; 64]) -> bool {
    let sreg = expr.sreg as usize;
    let data_len = expr.data.len();

    if sreg + data_len > registers.len() {
        return false;
    }

    let reg_slice = &registers[sreg..sreg + data_len];

    match expr.op {
        CmpOp::Eq => reg_slice == expr.data.as_slice(),
        CmpOp::Neq => reg_slice != expr.data.as_slice(),
        CmpOp::Lt => reg_slice < expr.data.as_slice(),
        CmpOp::Lte => reg_slice <= expr.data.as_slice(),
        CmpOp::Gt => reg_slice > expr.data.as_slice(),
        CmpOp::Gte => reg_slice >= expr.data.as_slice(),
    }
}

/// Bitwise ifadesini değerlendirir.
///
/// `result = (sreg & mask) ^ xor` işlemini uygular ve sonucu `dreg`'e yazar.
fn evaluate_bitwise(expr: &BitwiseExpr, registers: &mut [u8; 64]) -> bool {
    let sreg = expr.sreg as usize;
    let dreg = expr.dreg as usize;
    let len = expr.mask.len();

    if sreg + len > registers.len() || dreg + len > registers.len() {
        return false;
    }

    match expr.op {
        BitwiseOp::MaskXor => {
            for i in 0..len {
                let masked = registers[sreg + i] & expr.mask[i];
                registers[dreg + i] = masked ^ expr.xor[i];
            }
        }
        BitwiseOp::LShift => {
            // Basitleştirilmiş: sadece tek baytlık kaydırma
            if len == 1 && sreg < registers.len() && dreg < registers.len() {
                registers[dreg] = registers[sreg] << expr.mask[0];
            }
        }
        BitwiseOp::RShift => {
            if len == 1 && sreg < registers.len() && dreg < registers.len() {
                registers[dreg] = registers[sreg] >> expr.mask[0];
            }
        }
        BitwiseOp::And => {
            for i in 0..len {
                if sreg + i < registers.len() && dreg + i < registers.len() {
                    registers[dreg + i] = registers[sreg + i] & expr.mask[i];
                }
            }
        }
        BitwiseOp::Or => {
            for i in 0..len {
                if sreg + i < registers.len() && dreg + i < registers.len() {
                    registers[dreg + i] = registers[sreg + i] | expr.mask[i];
                }
            }
        }
        BitwiseOp::Xor => {
            for i in 0..len {
                if sreg + i < registers.len() && dreg + i < registers.len() {
                    registers[dreg + i] = registers[sreg + i] ^ expr.mask[i];
                }
            }
        }
    }

    true
}

/// Lookup ifadesini değerlendirir — set içinde arama.
///
/// `sreg`'deki değeri set_name adlı sette arar.
/// Eşleşme olursa `dreg`'e yazılır veya doğrudan true döner.
fn evaluate_lookup(
    expr: &LookupExpr,
    _pkt: &PacketInfo,
    _registers: &mut [u8; 64],
) -> bool {
    // Gerçek implementasyonda set tablosunda arama yapılır
    // şimdilik invert durumuna göre davran
    if expr.invert {
        true // invert: eşleşmeyenler eşleşir
    } else {
        true // varsayılan: eşleşme başarılı
    }
}

/// Conntrack ifadesini değerlendirir.
fn evaluate_conntrack(
    _expr: &CtExpr,
    _pkt: &PacketInfo,
    _registers: &mut [u8; 64],
) -> bool {
    // Gerçek implementasyonda conntrack tablosundan durum okunur
    // şimdilik başarılı varsayalım
    true
}

/// Log ifadesini değerlendirir — paket günlüğe yazılır.
fn evaluate_log(expr: &LogExpr, pkt: &PacketInfo) {
    crate::serial_println!(
        "[NFT_LOG] {} {}:{} -> {}:{} proto={} len={} {}",
        expr.prefix,
        pkt.src_ip,
        pkt.src_port,
        pkt.dst_ip,
        pkt.dst_port,
        pkt.proto,
        pkt.len,
        match expr.level {
            LogLevel::Emerg => "EMERG",
            LogLevel::Alert => "ALERT",
            LogLevel::Crit => "CRIT",
            LogLevel::Err => "ERR",
            LogLevel::Warning => "WARNING",
            LogLevel::Notice => "NOTICE",
            LogLevel::Info => "INFO",
            LogLevel::Debug => "DEBUG",
            LogLevel::Audit => "AUDIT",
        }
    );
}

/// FIB ifadesini değerlendirir — FIP routing lookup.
fn evaluate_fib(
    _expr: &FibExpr,
    _pkt: &PacketInfo,
    _registers: &mut [u8; 64],
) -> bool {
    // Gerçek implementasyonda FIP lookup yapılır
    // şimdilik başarılı varsayalım
    true
}

// ============================================================================
// İNİTEGRASYON: NFTABLES ↔ NETFILTER
// ============================================================================
// nftables ifade motorunu mevcut netfilter altyapısına entegre eder.
// `process_ipv4_packet` ve `process_ipv6_packet` çağrılarından sonra
// nftables motoru da değerlendirilir.

/// nftables ifadesi ile iptables kuralı arasında dönüştürme yardımcıları.
///
/// Bu fonksiyonlar, mevcut iptables kurallarını nftables formatına
/// veya tersine dönüştürmek için kullanılabilir.

/// IptEntry'yi NftRule'a dönüştür (geçiş晚期 desteği)
pub fn ipt_entry_to_nft_rule(entry: &super::netfilter::IptEntry) -> NftRule {
    let mut rule = NftRule::new(1);
    let mut dreg = 1;

    // Kaynak IP filtresi
    if entry.src_ip != 0 || entry.src_mask != 0xFFFFFFFF {
        rule = rule.with_expr(NftExpression::Payload(PayloadExpr::ipv4_src(dreg)));
        rule = rule.with_expr(NftExpression::Cmp(CmpExpr::eq_u32(
            dreg,
            entry.src_ip & entry.src_mask,
        )));
        dreg += 4;
    }

    // Hedef IP filtresi
    if entry.dst_ip != 0 || entry.dst_mask != 0xFFFFFFFF {
        rule = rule.with_expr(NftExpression::Payload(PayloadExpr::ipv4_dst(dreg)));
        rule = rule.with_expr(NftExpression::Cmp(CmpExpr::eq_u32(
            dreg,
            entry.dst_ip & entry.dst_mask,
        )));
        dreg += 4;
    }

    // Protokol filtresi
    if entry.proto != 0 {
        rule = rule.with_expr(NftExpression::Payload(PayloadExpr::ipv4_protocol(dreg)));
        rule = rule.with_expr(NftExpression::Cmp(CmpExpr::eq_u8(dreg, entry.proto)));
        dreg += 1;
    }

    // Kaynak port filtresi
    if entry.src_ports != (0, 65535) {
        rule = rule.with_expr(NftExpression::Payload(PayloadExpr::tcp_sport(dreg)));
        rule = rule.with_expr(NftExpression::Cmp(CmpExpr::eq_u16(
            dreg,
            entry.src_ports.0,
        )));
        dreg += 2;
    }

    // Hedef port filtresi
    if entry.dst_ports != (0, 65535) {
        rule = rule.with_expr(NftExpression::Payload(PayloadExpr::tcp_dport(dreg)));
        rule = rule.with_expr(NftExpression::Cmp(CmpExpr::eq_u16(
            dreg,
            entry.dst_ports.0,
        )));
        dreg += 2;
    }

    // Counter
    rule = rule.with_expr(NftExpression::Counter(CounterExpr::new()));

    // Target → Verdict
    let verdict = match entry.target.name.as_str() {
        "ACCEPT" => NftExpression::Verdict(VerdictExpr::accept()),
        "DROP" => NftExpression::Verdict(VerdictExpr::drop()),
        "RETURN" => NftExpression::Verdict(VerdictExpr::return_()),
        "LOG" => {
            rule = rule.with_expr(NftExpression::Log(LogExpr::new("[IPT]")));
            NftExpression::Verdict(VerdictExpr::accept())
        }
        _ => NftExpression::Verdict(VerdictExpr::accept()),
    };
    rule = rule.with_expr(verdict);

    rule
}

// ============================================================================
// PUBLIC API — mod.rs'den erişilebilir
// ============================================================================

/// Küresel nftables yöneticisi
pub static NFTABLES: NftablesManager = NftablesManager::new();

/// nftables alt sistemini başlatır
pub fn init() {
    NFTABLES.init();
}

// ============================================================================
// NFTABLES NETLINK ARABİRİMİ (NETLINK BINDING)
// ============================================================================
// Bu bölüm, NETLINK_NETFILTER üzerinden gelen nftables mesajlarını işler.
// nft userspace aracı, NFNL_SUBSYS_NFTABLES (10) altında NFT_MSG_* komutları
// gönderir. Her mesaj NlMsgHdr → nfgenmsg → NlAttr* zincirinden oluşur.
//
// Kaynak: include/uapi/linux/netfilter/nfnetlink.h
//         include/uapi/linux/netfilter/nf_tables.h

// ---------------------------------------------------------------------------
// NFNETLINK SABİTLERİ
// ---------------------------------------------------------------------------

/// nfnetlink subsystem IDs — NFNL_SUBSYS_*
pub const NFNL_SUBSYS_NFTABLES: u16 = 10;

// ---------------------------------------------------------------------------
// NFT_MSG_* KOMUT SABİTLERİ
// ---------------------------------------------------------------------------
// nlmsg_type = (NFNL_SUBSYS_NFTABLES << 8) | NFT_MSG_*
// Kaynak: include/uapi/linux/netfilter/nf_tables.h

pub const NFT_MSG_NEWTABLE: u8 = 0;
pub const NFT_MSG_GETTABLE: u8 = 1;
pub const NFT_MSG_DELTABLE: u8 = 2;
pub const NFT_MSG_NEWCHAIN: u8 = 3;
pub const NFT_MSG_GETCHAIN: u8 = 4;
pub const NFT_MSG_DELCHAIN: u8 = 5;
pub const NFT_MSG_NEWRULE: u8 = 6;
pub const NFT_MSG_GETRULE: u8 = 7;
pub const NFT_MSG_DELRULE: u8 = 8;
pub const NFT_MSG_NEWSET: u8 = 9;
pub const NFT_MSG_GETSET: u8 = 10;
pub const NFT_MSG_DELSET: u8 = 11;
pub const NFT_MSG_NEWSETELEM: u8 = 12;
pub const NFT_MSG_GETSETELEM: u8 = 13;
pub const NFT_MSG_DELSETELEM: u8 = 14;
pub const NFT_MSG_NEWGEN: u8 = 15;
pub const NFT_MSG_GETGEN: u8 = 16;
pub const NFT_MSG_TRACE: u8 = 17;
pub const NFT_MSG_NEWOBJ: u8 = 18;
pub const NFT_MSG_GETOBJ: u8 = 19;
pub const NFT_MSG_DELOBJ: u8 = 20;
pub const NFT_MSG_GETOBJ_RESET: u8 = 21;
pub const NFT_MSG_NEWFLOWTABLE: u8 = 22;
pub const NFT_MSG_GETFLOWTABLE: u8 = 23;
pub const NFT_MSG_DELFLOWTABLE: u8 = 24;
pub const NFT_MSG_GETRULE_RESET: u8 = 25;
pub const NFT_MSG_DESTROYTABLE: u8 = 26;
pub const NFT_MSG_DESTROYCHAIN: u8 = 27;
pub const NFT_MSG_DESTROYRULE: u8 = 28;
pub const NFT_MSG_DESTROYSET: u8 = 29;
pub const NFT_MSG_DESTROYSETELEM: u8 = 30;
pub const NFT_MSG_DESTROYOBJ: u8 = 31;
pub const NFT_MSG_DESTROYFLOWTABLE: u8 = 32;

// ---------------------------------------------------------------------------
// NFTA_* NİTELİK SABİTLERİ — Tablo (Table attributes)
// ---------------------------------------------------------------------------
// Kaynak: include/uapi/linux/netfilter/nf_tables.h

pub const NFTA_TABLE_UNSPEC: u16 = 0;
pub const NFTA_TABLE_NAME: u16 = 1;
pub const NFTA_TABLE_FLAGS: u16 = 2;
pub const NFTA_TABLE_USE: u16 = 3;
pub const NFTA_TABLE_HANDLE: u16 = 4;
pub const NFTA_TABLE_MAX: u16 = 5;

/// NFTA_CHAIN_* — Zincir nitelikleri
pub const NFTA_CHAIN_UNSPEC: u16 = 0;
pub const NFTA_CHAIN_TABLE: u16 = 1;
pub const NFTA_CHAIN_HANDLE: u16 = 2;
pub const NFTA_CHAIN_NAME: u16 = 3;
pub const NFTA_CHAIN_HOOK: u16 = 4;
pub const NFTA_CHAIN_PRIO: u16 = 5;
pub const NFTA_CHAIN_POLICY: u16 = 6;
pub const NFTA_CHAIN_USE: u16 = 7;
pub const NFTA_CHAIN_TYPE: u16 = 8;
pub const NFTA_CHAIN_COUNTERS: u16 = 9;
pub const NFTA_CHAIN_PAD: u16 = 10;
pub const NFTA_CHAIN_MAX: u16 = 11;

/// NFTA_RULE_* — Kural nitelikleri
pub const NFTA_RULE_UNSPEC: u16 = 0;
pub const NFTA_RULE_TABLE: u16 = 1;
pub const NFTA_RULE_CHAIN: u16 = 2;
pub const NFTA_RULE_HANDLE: u16 = 3;
pub const NFTA_RULE_EXPRESSIONS: u16 = 4;
pub const NFTA_RULE_COMPAT: u16 = 5;
pub const NFTA_RULE_POSITION: u16 = 6;
pub const NFTA_RULE_USERDATA: u16 = 7;
pub const NFTA_RULE_ID: u16 = 8;
pub const NFTA_RULE_POSITION_ID: u16 = 9;
pub const NFTA_RULE_MAX: u16 = 10;

/// NFTA_SET_* — Set nitelikleri
pub const NFTA_SET_UNSPEC: u16 = 0;
pub const NFTA_SET_TABLE: u16 = 1;
pub const NFTA_SET_NAME: u16 = 2;
pub const NFTA_SET_FLAGS: u16 = 3;
pub const NFTA_SET_KEY_TYPE: u16 = 4;
pub const NFTA_SET_KEY_LEN: u16 = 5;
pub const NFTA_SET_DATA_TYPE: u16 = 6;
pub const NFTA_SET_DATA_LEN: u16 = 7;
pub const NFTA_SET_TIMEOUT: u16 = 8;
pub const NFTA_SET_GC_INTERVAL: u16 = 9;
pub const NFTA_SET_POLICY: u16 = 10;
pub const NFTA_SET_DESC: u16 = 11;
pub const NFTA_SET_ID: u16 = 12;
pub const NFTA_SET_USERDATA: u16 = 13;
pub const NFTA_SET_EXPR: u16 = 14;
pub const NFTA_SET_OBJ_TYPE: u16 = 15;
pub const NFTA_SET_HANDLE: u16 = 16;
pub const NFTA_SET_MAX: u16 = 17;

/// NFTA_SET_ELEM_* — Set elemanı nitelikleri
pub const NFTA_SET_ELEM_UNSPEC: u16 = 0;
pub const NFTA_SET_ELEM_KEY: u16 = 1;
pub const NFTA_SET_ELEM_DATA: u16 = 2;
pub const NFTA_SET_ELEM_FLAGS: u16 = 3;
pub const NFTA_SET_ELEM_TIMEOUT: u16 = 4;
pub const NFTA_SET_ELEM_EXPIRATION: u16 = 5;
pub const NFTA_SET_ELEM_USERDATA: u16 = 6;
pub const NFTA_SET_ELEM_EXPR: u16 = 7;
pub const NFTA_SET_ELEM_OBJREF: u16 = 8;
pub const NFTA_SET_ELEM_MAX: u16 = 9;

/// NFTA_SET_ELEM_LIST_* — Set elemanı listesi (batch ekleme/silme)
pub const NFTA_SET_ELEM_LIST_UNSPEC: u16 = 0;
pub const NFTA_SET_ELEM_LIST_TABLE: u16 = 1;
pub const NFTA_SET_ELEM_LIST_SET: u16 = 2;
pub const NFTA_SET_ELEM_LIST_ELEMENTS: u16 = 3;
pub const NFTA_SET_ELEM_LIST_SET_ID: u16 = 4;
pub const NFTA_SET_ELEM_LIST_MAX: u16 = 5;

/// NFTA_LIST_* — Generic list wrapper
pub const NFTA_LIST_UNSPEC: u16 = 0;
pub const NFTA_LIST_ELEM: u16 = 1;
pub const NFTA_LIST_MAX: u16 = 2;

/// NFTA_OBJ_* — Named object nitelikleri
pub const NFTA_OBJ_UNSPEC: u16 = 0;
pub const NFTA_OBJ_TABLE: u16 = 1;
pub const NFTA_OBJ_NAME: u16 = 2;
pub const NFTA_OBJ_TYPE: u16 = 3;
pub const NFTA_OBJ_DATA: u16 = 4;
pub const NFTA_OBJ_HANDLE: u16 = 5;
pub const NFTA_OBJ_MAX: u16 = 6;

/// NFTA_FLOWTABLE_* — Flowtable nitelikleri
pub const NFTA_FLOWTABLE_UNSPEC: u16 = 0;
pub const NFTA_FLOWTABLE_TABLE: u16 = 1;
pub const NFTA_FLOWTABLE_NAME: u16 = 2;
pub const NFTA_FLOWTABLE_HOOK: u16 = 3;
pub const NFTA_FLOWTABLE_PRIO: u16 = 4;
pub const NFTA_FLOWTABLE_USE: u16 = 5;
pub const NFTA_FLOWTABLE_HANDLE: u16 = 6;
pub const NFTA_FLOWTABLE_FLAGS: u16 = 7;
pub const NFTA_FLOWTABLE_MAX: u16 = 8;

/// NFTA_GEN_* — Generation ID nitelikleri
pub const NFTA_GEN_UNSPEC: u16 = 0;
pub const NFTA_GEN_ID: u16 = 1;
pub const NFTA_GEN_PROC_PID: u16 = 2;
pub const NFTA_GEN_MAX: u16 = 3;

// ---------------------------------------------------------------------------
// NFNL MSG FLAGS (netlink header flags for nfnetlink)
// ---------------------------------------------------------------------------
pub const NLM_F_DUMP: u16 = 0x300; // NLM_F_DUMP = NLM_F_ROOT | NLM_F_MATCH

// ---------------------------------------------------------------------------
// NfGenMsg — nfnetlink sabit başlığı (nfgenmsg)
// ---------------------------------------------------------------------------
// Linux struct nfgenmsg:
//   u8  nfgen_family   → AF_INET/AF_INET6/AF_UNSPEC
//   u8  version        → NFNETLINK_V0 (0)
//   u16 res_id          → resource ID (0 for nftables)
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct NfGenMsg {
    pub nfgen_family: u8,
    pub version: u8,
    pub res_id: u16,
}

impl NfGenMsg {
    pub const SIZE: usize = 4;

    pub fn new(family: u8) -> Self {
        Self {
            nfgen_family: family,
            version: 0,
            res_id: 0,
        }
    }

    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < Self::SIZE {
            return None;
        }
        Some(Self {
            nfgen_family: data[0],
            version: data[1],
            res_id: u16::from_be_bytes([data[2], data[3]]),
        })
    }

    pub fn to_bytes(&self) -> [u8; 4] {
        [self.nfgen_family, self.version, self.res_id.to_be_bytes()[0], self.res_id.to_be_bytes()[1]]
    }
}

// ---------------------------------------------------------------------------
// Netlink Attribute Okuma Yardımcıları
// ---------------------------------------------------------------------------

/// Netlink niteliğini byte diliminden okur.
/// Format: [len(2B LE) | type(2B LE) | data(padded to 4B)]
struct NlAttrReader<'a> {
    data: &'a [u8],
    offset: usize,
}

impl<'a> NlAttrReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, offset: 0 }
    }

    fn next_attr(&mut self) -> Option<(u16, &'a [u8])> {
        if self.offset + 4 > self.data.len() {
            return None;
        }
        let len = u16::from_le_bytes([self.data[self.offset], self.data[self.offset + 1]]) as usize;
        let attr_type = u16::from_le_bytes([self.data[self.offset + 2], self.data[self.offset + 3]]);
        if len < 4 || self.offset + len > self.data.len() {
            return None;
        }
        let value = &self.data[self.offset + 4..self.offset + len];
        // 4-byte aligned ilerle
        let padded = ((len + 3) / 4) * 4;
        self.offset += padded;
        Some((attr_type & 0x3FFF, value)) // mask out NLA_F_NESTED/NLA_F_NET_BYTEORDER
    }

    fn read_string(attr_type: u16, value: &[u8]) -> Option<String> {
        // String'ler null-terminated
        let end = value.iter().position(|&b| b == 0).unwrap_or(value.len());
        core::str::from_utf8(&value[..end]).ok().map(String::from)
    }

    fn read_u32(value: &[u8]) -> Option<u32> {
        if value.len() >= 4 {
            Some(u32::from_ne_bytes([value[0], value[1], value[2], value[3]]))
        } else {
            None
        }
    }

    fn read_u64(value: &[u8]) -> Option<u64> {
        if value.len() >= 8 {
            Some(u64::from_ne_bytes([value[0], value[1], value[2], value[3], value[4], value[5], value[6], value[7]]))
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Netlink Attribute Yazma Yardımcıları
// ---------------------------------------------------------------------------

/// TLV formatında netlink niteliği oluştur (NlAttr::new ile aynı mantık)
fn build_attr(attr_type: u16, data: &[u8]) -> Vec<u8> {
    let mut buf = Vec::new();
    let len = (4 + data.len()) as u16;
    buf.extend_from_slice(&len.to_le_bytes());
    buf.extend_from_slice(&attr_type.to_le_bytes());
    buf.extend_from_slice(data);
    // 4-byte alignment padding
    while buf.len() % 4 != 0 {
        buf.push(0);
    }
    buf
}

fn build_attr_string(attr_type: u16, s: &str) -> Vec<u8> {
    let mut data = s.as_bytes().to_vec();
    data.push(0); // null terminator
    build_attr(attr_type, &data)
}

fn build_attr_u32(attr_type: u16, val: u32) -> Vec<u8> {
    build_attr(attr_type, &val.to_ne_bytes())
}

fn build_attr_u64(attr_type: u16, val: u64) -> Vec<u8> {
    build_attr(attr_type, &val.to_ne_bytes())
}

fn build_attr_u8(attr_type: u16, val: u8) -> Vec<u8> {
    build_attr(attr_type, &[val])
}

// ---------------------------------------------------------------------------
// Komut İşleyiciler — Her NFT_MSG_* için ayrı
// ---------------------------------------------------------------------------

fn handle_nft_newtable(
    genmsg: &NfGenMsg,
    payload: &[u8],
) -> Result<Vec<(u16, Vec<u8>)>, ()> {
    let mut reader = NlAttrReader::new(payload);
    let mut name = None;
    let mut flags = None;

    while let Some((attr_type, value)) = reader.next_attr() {
        match attr_type {
            NFTA_TABLE_NAME => name = NlAttrReader::read_string(attr_type, value),
            NFTA_TABLE_FLAGS => flags = NlAttrReader::read_u32(value),
            _ => {}
        }
    }

    let table_name = name.ok_or(())?;
    let family = genmsg.nfgen_family as u32;
    let _ = NFTABLES.new_table(&table_name, family);

    Ok(Vec::new())
}

fn handle_nft_gettable(
    genmsg: &NfGenMsg,
    payload: &[u8],
) -> Result<Vec<(u16, Vec<u8>)>, ()> {
    let family = genmsg.nfgen_family as u32;

    // Belirli bir tablo sorgusu
    let mut reader = NlAttrReader::new(payload);
    let mut name_filter = None;
    while let Some((attr_type, value)) = reader.next_attr() {
        if attr_type == NFTA_TABLE_NAME {
            name_filter = NlAttrReader::read_string(attr_type, value);
        }
    }

    let mut responses = Vec::new();
    let tables = if let Some(ref name) = name_filter {
        NFTABLES.get_table(name).map(|t| vec![(t.name.clone(), t.family, t.handle, t.flags)])
            .unwrap_or_default()
    } else {
        // Dump all tables
        NFTABLES.list_tables().iter().map(|(n, f, h)| {
            (n.clone(), *f, *h, NFTABLES.get_table(n).map(|t| t.flags).unwrap_or(0))
        }).collect()
    };

    for (tname, tfam, thandle, tflags) in &tables {
        if genmsg.nfgen_family != 0 && family != *tfam {
            continue;
        }
        let mut attrs = Vec::new();
        attrs.extend_from_slice(&build_attr_string(NFTA_TABLE_NAME, tname));
        attrs.extend_from_slice(&build_attr_u32(NFTA_TABLE_FLAGS, *tflags));
        attrs.extend_from_slice(&build_attr_u64(NFTA_TABLE_HANDLE, *thandle));

        let mut msg = Vec::new();
        msg.extend_from_slice(&NfGenMsg::new(*tfam as u8).to_bytes());
        msg.extend_from_slice(&attrs);
        responses.push(((NFNL_SUBSYS_NFTABLES << 8) | NFT_MSG_NEWTABLE as u16, msg));
    }

    Ok(responses)
}

fn handle_nft_deltable(
    _genmsg: &NfGenMsg,
    payload: &[u8],
) -> Result<Vec<(u16, Vec<u8>)>, ()> {
    let mut reader = NlAttrReader::new(payload);
    let mut name = None;
    while let Some((attr_type, value)) = reader.next_attr() {
        if attr_type == NFTA_TABLE_NAME {
            name = NlAttrReader::read_string(attr_type, value);
        }
    }
    let _ = NFTABLES.delete_table(&name.ok_or(())?);
    Ok(Vec::new())
}

fn handle_nft_newchain(
    genmsg: &NfGenMsg,
    payload: &[u8],
) -> Result<Vec<(u16, Vec<u8>)>, ()> {
    let mut reader = NlAttrReader::new(payload);
    let mut table = None;
    let mut name = None;
    let mut chain_type_s = None;
    let mut hook = None;
    let mut prio = None;
    let mut policy = None;

    while let Some((attr_type, value)) = reader.next_attr() {
        match attr_type {
            NFTA_CHAIN_TABLE => table = NlAttrReader::read_string(attr_type, value),
            NFTA_CHAIN_NAME => name = NlAttrReader::read_string(attr_type, value),
            NFTA_CHAIN_TYPE => chain_type_s = NlAttrReader::read_string(attr_type, value),
            NFTA_CHAIN_HOOK => hook = NlAttrReader::read_u32(value),
            NFTA_CHAIN_PRIO => prio = NlAttrReader::read_u32(value),
            NFTA_CHAIN_POLICY => policy = NlAttrReader::read_u32(value),
            _ => {}
        }
    }

    let tname = table.ok_or(())?;
    let cname = name.ok_or(())?;

    if let (Some(hk), Some(pr), Some(pl)) = (hook, prio, policy) {
        let ctype = match chain_type_s.as_deref() {
            Some("nat") => NftChainType::Nat,
            Some("mangle") => NftChainType::Mangle,
            Some("route") => NftChainType::Route,
            _ => NftChainType::Filter,
        };
        let chain = NftChain::new_base(&cname, 0, ctype, hk, pr as i32, pl);
        let _ = NFTABLES.new_chain(&tname, chain);
    } else {
        let chain = NftChain::new_user(&cname, 0);
        let _ = NFTABLES.new_chain(&tname, chain);
    }

    Ok(Vec::new())
}

fn handle_nft_getchain(
    genmsg: &NfGenMsg,
    payload: &[u8],
) -> Result<Vec<(u16, Vec<u8>)>, ()> {
    let mut reader = NlAttrReader::new(payload);
    let mut table_filter = None;
    let mut chain_filter = None;

    while let Some((attr_type, value)) = reader.next_attr() {
        match attr_type {
            NFTA_CHAIN_TABLE => table_filter = NlAttrReader::read_string(attr_type, value),
            NFTA_CHAIN_NAME => chain_filter = NlAttrReader::read_string(attr_type, value),
            _ => {}
        }
    }

    let mut responses = Vec::new();
    let tables = NFTABLES.list_tables();

    for (tname, tfam, _thandle) in &tables {
        if let Some(ref tf) = table_filter {
            if tname != tf {
                continue;
            }
        }
        if let Some(table) = NFTABLES.get_table(tname) {
            for chain in table.chains.values() {
                if let Some(ref cf) = chain_filter {
                    if chain.name != *cf {
                        continue;
                    }
                }
                let mut attrs = Vec::new();
                attrs.extend_from_slice(&build_attr_string(NFTA_CHAIN_TABLE, tname));
                attrs.extend_from_slice(&build_attr_string(NFTA_CHAIN_NAME, &chain.name));
                attrs.extend_from_slice(&build_attr_u64(NFTA_CHAIN_HANDLE, chain.handle));
                if chain.hook != 0 {
                    attrs.extend_from_slice(&build_attr_u32(NFTA_CHAIN_HOOK, chain.hook));
                    attrs.extend_from_slice(&build_attr_u32(NFTA_CHAIN_PRIO, chain.priority as u32));
                    attrs.extend_from_slice(&build_attr_u32(NFTA_CHAIN_POLICY, chain.policy));
                    let ctype_str = match chain.chain_type {
                        NftChainType::Filter => "filter",
                        NftChainType::Nat => "nat",
                        NftChainType::Mangle => "mangle",
                        NftChainType::Route => "route",
                    };
                    attrs.extend_from_slice(&build_attr_string(NFTA_CHAIN_TYPE, ctype_str));
                }

                let mut msg = Vec::new();
                msg.extend_from_slice(&NfGenMsg::new(*tfam as u8).to_bytes());
                msg.extend_from_slice(&attrs);
                responses.push(((NFNL_SUBSYS_NFTABLES << 8) | NFT_MSG_NEWCHAIN as u16, msg));
            }
        }
    }

    Ok(responses)
}

fn handle_nft_delchain(
    _genmsg: &NfGenMsg,
    payload: &[u8],
) -> Result<Vec<(u16, Vec<u8>)>, ()> {
    let mut reader = NlAttrReader::new(payload);
    let mut table = None;
    let mut name = None;
    while let Some((attr_type, value)) = reader.next_attr() {
        match attr_type {
            NFTA_CHAIN_TABLE => table = NlAttrReader::read_string(attr_type, value),
            NFTA_CHAIN_NAME => name = NlAttrReader::read_string(attr_type, value),
            _ => {}
        }
    }
    let _ = NFTABLES.delete_chain(&table.ok_or(())?, &name.ok_or(())?);
    Ok(Vec::new())
}

fn handle_nft_newrule(
    genmsg: &NfGenMsg,
    payload: &[u8],
) -> Result<Vec<(u16, Vec<u8>)>, ()> {
    let mut reader = NlAttrReader::new(payload);
    let mut table = None;
    let mut chain = None;
    let mut handle = 0u64;
    let mut userdata = Vec::new();
    let mut expressions_data: Option<Vec<u8>> = None;

    while let Some((attr_type, value)) = reader.next_attr() {
        match attr_type {
            NFTA_RULE_TABLE => table = NlAttrReader::read_string(attr_type, value),
            NFTA_RULE_CHAIN => chain = NlAttrReader::read_string(attr_type, value),
            NFTA_RULE_HANDLE => {
                if let Some(h) = NlAttrReader::read_u64(value) {
                    handle = h;
                }
            }
            NFTA_RULE_EXPRESSIONS => expressions_data = Some(value.to_vec()),
            NFTA_RULE_USERDATA => userdata = value.to_vec(),
            _ => {}
        }
    }

    let tname = table.ok_or(())?;
    let cname = chain.ok_or(())?;

    let mut rule = NftRule::new(handle);
    rule.userdata = userdata;

    // İfadeleri parse et (NFTA_RULE_EXPRESSIONS nested attr)
    if let Some(expr_data) = expressions_data {
        parse_expressions_nested(&mut rule, &expr_data);
    }

    let _ = NFTABLES.new_rule(&tname, &cname, rule);
    Ok(Vec::new())
}

/// Nested expression listesini parse eder (basitleştirilmiş)
fn parse_expressions_nested(rule: &mut NftRule, data: &[u8]) {
    let mut reader = NlAttrReader::new(data);
    // Her nested attr bir ifade
    while let Some((_expr_index, expr_value)) = reader.next_attr() {
        // İfade içindeki NFTA_EXPR_* attr'ları
        let mut expr_reader = NlAttrReader::new(expr_value);
        let mut expr_name = None;
        let mut expr_data: Option<Vec<u8>> = None;
        while let Some((eat, ev)) = expr_reader.next_attr() {
            if eat == 1 {
                // NFTA_EXPR_NAME
                expr_name = NlAttrReader::read_string(eat, ev);
            } else if eat == 2 {
                // NFTA_EXPR_DATA (nested, opaq)
                expr_data = Some(ev.to_vec());
            }
        }
        if let Some(ref name) = expr_name {
            match name.as_str() {
                "counter" => rule.expressions.push(NftExpression::Counter(CounterExpr::new())),
                "verdict" => {
                    if let Some(ref d) = expr_data {
                        let mut vr = NlAttrReader::new(d);
                        while let Some((vat, vv)) = vr.next_attr() {
                            if vat == 1 {
                                // NFTA_VERDICT_CODE
                                if let Some(code) = NlAttrReader::read_u32(vv) {
                                    let v = match code {
                                        0 => VerdictExpr::accept(),
                                        1 => VerdictExpr::drop(),
                                        _ => VerdictExpr::accept(),
                                    };
                                    rule.expressions.push(NftExpression::Verdict(v));
                                }
                            }
                        }
                    }
                }
                "payload" => {
                    rule.expressions.push(NftExpression::Payload(PayloadExpr::ipv4_src(1)));
                }
                "cmp" => {
                    if let Some(ref d) = expr_data {
                        let mut cr = NlAttrReader::new(d);
                        let mut sreg = 0u32;
                        let mut op = 0u32;
                        let mut data_bytes = Vec::new();
                        while let Some((cat, cv)) = cr.next_attr() {
                            if cat == 1 {
                                // NFTA_CMP_SREG
                                if let Some(v) = NlAttrReader::read_u32(cv) { sreg = v; }
                            } else if cat == 2 {
                                // NFTA_CMP_OP
                                if let Some(v) = NlAttrReader::read_u32(cv) { op = v; }
                            } else if cat == 3 {
                                // NFTA_CMP_DATA
                                data_bytes = cv.to_vec();
                            }
                        }
                        match op {
                            0 => rule.expressions.push(NftExpression::Cmp(CmpExpr::eq_bytes(sreg, &data_bytes))),
                            1 => rule.expressions.push(NftExpression::Cmp(CmpExpr::neq_u8(sreg, 0))),
                            _ => {}
                        }
                    }
                }
                "log" => {
                    let prefix = if let Some(ref d) = expr_data {
                        let mut lr = NlAttrReader::new(d);
                        let mut p = String::from("nft");
                        while let Some((lat, lv)) = lr.next_attr() {
                            if lat == 1 {
                                // NFTA_LOG_PREFIX
                                if let Some(s) = NlAttrReader::read_string(lat, lv) { p = s; }
                            }
                        }
                        p
                    } else {
                        String::from("nft")
                    };
                    rule.expressions.push(NftExpression::Log(LogExpr::new(&prefix)));
                }
                "reject" => {
                    rule.expressions.push(NftExpression::Reject(RejectExpr::icmp_port_unreachable()));
                }
                _ => {
                    // Diğer ifade türleri için varsayılan payload atla
                }
            }
        }
    }
}

fn handle_nft_getrule(
    genmsg: &NfGenMsg,
    payload: &[u8],
) -> Result<Vec<(u16, Vec<u8>)>, ()> {
    let mut reader = NlAttrReader::new(payload);
    let mut table_filter = None;
    let mut chain_filter = None;

    while let Some((attr_type, value)) = reader.next_attr() {
        match attr_type {
            NFTA_RULE_TABLE => table_filter = NlAttrReader::read_string(attr_type, value),
            NFTA_RULE_CHAIN => chain_filter = NlAttrReader::read_string(attr_type, value),
            _ => {}
        }
    }

    let mut responses = Vec::new();

    let tables = NFTABLES.list_tables();
    for (tname, tfam, _th) in &tables {
        if let Some(ref tf) = table_filter {
            if tname != tf { continue; }
        }
        if let Some(table) = NFTABLES.get_table(tname) {
            for chain in table.chains.values() {
                if let Some(ref cf) = chain_filter {
                    if chain.name != *cf { continue; }
                }
                for rule in chain.rules.values() {
                    let mut attrs = Vec::new();
                    attrs.extend_from_slice(&build_attr_string(NFTA_RULE_TABLE, tname));
                    attrs.extend_from_slice(&build_attr_string(NFTA_RULE_CHAIN, &chain.name));
                    attrs.extend_from_slice(&build_attr_u64(NFTA_RULE_HANDLE, rule.handle));
                    if !rule.userdata.is_empty() {
                        attrs.extend_from_slice(&build_attr(NFTA_RULE_USERDATA, &rule.userdata));
                    }

                    let mut msg = Vec::new();
                    msg.extend_from_slice(&NfGenMsg::new(*tfam as u8).to_bytes());
                    msg.extend_from_slice(&attrs);
                    responses.push(((NFNL_SUBSYS_NFTABLES << 8) | NFT_MSG_NEWRULE as u16, msg));
                }
            }
        }
    }

    Ok(responses)
}

fn handle_nft_delrule(
    _genmsg: &NfGenMsg,
    payload: &[u8],
) -> Result<Vec<(u16, Vec<u8>)>, ()> {
    let mut reader = NlAttrReader::new(payload);
    let mut table = None;
    let mut chain = None;
    let mut handle = None;
    while let Some((attr_type, value)) = reader.next_attr() {
        match attr_type {
            NFTA_RULE_TABLE => table = NlAttrReader::read_string(attr_type, value),
            NFTA_RULE_CHAIN => chain = NlAttrReader::read_string(attr_type, value),
            NFTA_RULE_HANDLE => handle = NlAttrReader::read_u64(value),
            _ => {}
        }
    }
    if let (Some(t), Some(c), Some(h)) = (&table, &chain, handle) {
        let _ = NFTABLES.delete_rule(t, c, h);
    }
    Ok(Vec::new())
}

fn handle_nft_newset(
    genmsg: &NfGenMsg,
    payload: &[u8],
) -> Result<Vec<(u16, Vec<u8>)>, ()> {
    let mut reader = NlAttrReader::new(payload);
    let mut table = None;
    let mut name = None;
    let mut flags = 0u32;
    let mut key_len = 0u32;
    let mut set_type_val = NftSetType::Ipv4Addr;

    while let Some((attr_type, value)) = reader.next_attr() {
        match attr_type {
            NFTA_SET_TABLE => table = NlAttrReader::read_string(attr_type, value),
            NFTA_SET_NAME => name = NlAttrReader::read_string(attr_type, value),
            NFTA_SET_FLAGS => { flags = NlAttrReader::read_u32(value).unwrap_or(0); }
            NFTA_SET_KEY_LEN => { key_len = NlAttrReader::read_u32(value).unwrap_or(4); }
            NFTA_SET_KEY_TYPE => {
                if key_len == 4 { set_type_val = NftSetType::Ipv4Addr; }
                else if key_len == 16 { set_type_val = NftSetType::Ipv6Addr; }
                else { set_type_val = NftSetType::IpAddrPort; }
            }
            _ => {}
        }
    }

    let tname = table.ok_or(())?;
    let sname = name.ok_or(())?;
    let set = NftSet::new(&sname, 0, set_type_val, key_len);
    let _ = NFTABLES.new_set(&tname, set);
    Ok(Vec::new())
}

fn handle_nft_getset(
    genmsg: &NfGenMsg,
    payload: &[u8],
) -> Result<Vec<(u16, Vec<u8>)>, ()> {
    let mut reader = NlAttrReader::new(payload);
    let mut table_filter = None;

    while let Some((attr_type, value)) = reader.next_attr() {
        if attr_type == NFTA_SET_TABLE {
            table_filter = NlAttrReader::read_string(attr_type, value);
        }
    }

    let mut responses = Vec::new();
    let tables = NFTABLES.list_tables();
    for (tname, tfam, _th) in &tables {
        if let Some(ref tf) = table_filter {
            if tname != tf { continue; }
        }
        if let Some(table) = NFTABLES.get_table(tname) {
            for set in table.sets.values() {
                let mut attrs = Vec::new();
                attrs.extend_from_slice(&build_attr_string(NFTA_SET_TABLE, tname));
                attrs.extend_from_slice(&build_attr_string(NFTA_SET_NAME, &set.name));
                attrs.extend_from_slice(&build_attr_u32(NFTA_SET_FLAGS, set.flags));
                attrs.extend_from_slice(&build_attr_u32(NFTA_SET_KEY_LEN, set.key_len));
                attrs.extend_from_slice(&build_attr_u32(NFTA_SET_ID, set.handle as u32));
                attrs.extend_from_slice(&build_attr_u64(NFTA_SET_HANDLE, set.handle));

                let mut msg = Vec::new();
                msg.extend_from_slice(&NfGenMsg::new(*tfam as u8).to_bytes());
                msg.extend_from_slice(&attrs);
                responses.push(((NFNL_SUBSYS_NFTABLES << 8) | NFT_MSG_NEWSET as u16, msg));
            }
        }
    }

    Ok(responses)
}

fn handle_nft_newsetelem(
    _genmsg: &NfGenMsg,
    payload: &[u8],
) -> Result<Vec<(u16, Vec<u8>)>, ()> {
    let mut reader = NlAttrReader::new(payload);
    let mut table: Option<String> = None;
    let mut set_name: Option<String> = None;
    let mut elements: Vec<NftSetElem> = Vec::new();

    while let Some((attr_type, value)) = reader.next_attr() {
        match attr_type {
            NFTA_SET_ELEM_LIST_TABLE => {
                table = NlAttrReader::read_string(attr_type, value);
            }
            NFTA_SET_ELEM_LIST_SET => {
                set_name = NlAttrReader::read_string(attr_type, value);
            }
            NFTA_SET_ELEM_LIST_ELEMENTS => {
                // NFTA_LIST_ELEM ile sarılmış elemanları parse et
                let mut list_reader = NlAttrReader::new(value);
                while let Some((_list_type, elem_nested)) = list_reader.next_attr() {
                    let mut er = NlAttrReader::new(elem_nested);
                    let mut key = Vec::new();
                    let mut data = Vec::new();
                    let mut flags = 0u32;
                    let mut timeout = 0u64;
                    while let Some((eat, ev)) = er.next_attr() {
                        match eat {
                            NFTA_SET_ELEM_KEY => key = ev.to_vec(),
                            NFTA_SET_ELEM_DATA => data = ev.to_vec(),
                            NFTA_SET_ELEM_FLAGS => flags = NlAttrReader::read_u32(ev).unwrap_or(0),
                            NFTA_SET_ELEM_TIMEOUT => timeout = NlAttrReader::read_u64(ev).unwrap_or(0),
                            _ => {}
                        }
                    }
                    elements.push(NftSetElem {
                        key, data, timeout_ms: timeout, flags, expressions: Vec::new(),
                    });
                }
            }
            _ => {}
        }
    }

    if let (Some(tname), Some(sname)) = (&table, &set_name) {
        for elem in elements {
            let _ = NFTABLES.new_set_elem(tname, sname, elem);
        }
    }
    Ok(Vec::new())
}

fn handle_nft_getgen(
    genmsg: &NfGenMsg,
    _payload: &[u8],
) -> Result<Vec<(u16, Vec<u8>)>, ()> {
    let gen_id = NFTABLES.get_generation_id();
    let mut attrs = Vec::new();
    attrs.extend_from_slice(&build_attr_u32(NFTA_GEN_ID, gen_id as u32));

    let mut msg = Vec::new();
    msg.extend_from_slice(&NfGenMsg::new(genmsg.nfgen_family).to_bytes());
    msg.extend_from_slice(&attrs);
    Ok(vec![((NFNL_SUBSYS_NFTABLES << 8) | NFT_MSG_NEWGEN as u16, msg)])
}

// ---------------------------------------------------------------------------
// Ana Dispatch — nftables netlink mesajlarını yönlendirir
// ---------------------------------------------------------------------------

/// NETLINK_NETFILTER üzerinden gelen nftables mesajını işler.
///
/// # Arguments
/// * `nlmsg_type` — Netlink başlığındaki nlmsg_type alanı
/// * `payload` — Netlink payload (NfGenMsg + NFTA_* attributes)
///
/// # Returns
/// * `Vec<(u16, Vec<u8>)>` — (yanıt nlmsg_type, yanıt payload) çiftleri
pub fn handle_nftables_netlink(nlmsg_type: u16, payload: &[u8]) -> Vec<(u16, Vec<u8>)> {
    // nlmsg_type'ın üst 8 biti subsystem, alt 8 biti komut
    let subsys = nlmsg_type >> 8;
    let msg_type = (nlmsg_type & 0xFF) as u8;

    if subsys as u16 != NFNL_SUBSYS_NFTABLES {
        return Vec::new();
    }

    // NfGenMsg'i payload'un başından oku
    let genmsg = match NfGenMsg::from_bytes(payload) {
        Some(g) => g,
        None => return Vec::new(),
    };
    let attr_payload = &payload[NfGenMsg::SIZE..];

    let result = match msg_type {
        NFT_MSG_NEWTABLE => handle_nft_newtable(&genmsg, attr_payload),
        NFT_MSG_GETTABLE => handle_nft_gettable(&genmsg, attr_payload),
        NFT_MSG_DELTABLE => handle_nft_deltable(&genmsg, attr_payload),
        NFT_MSG_DESTROYTABLE => handle_nft_deltable(&genmsg, attr_payload),

        NFT_MSG_NEWCHAIN => handle_nft_newchain(&genmsg, attr_payload),
        NFT_MSG_GETCHAIN => handle_nft_getchain(&genmsg, attr_payload),
        NFT_MSG_DELCHAIN => handle_nft_delchain(&genmsg, attr_payload),
        NFT_MSG_DESTROYCHAIN => handle_nft_delchain(&genmsg, attr_payload),

        NFT_MSG_NEWRULE => handle_nft_newrule(&genmsg, attr_payload),
        NFT_MSG_GETRULE => handle_nft_getrule(&genmsg, attr_payload),
        NFT_MSG_DELRULE => handle_nft_delrule(&genmsg, attr_payload),
        NFT_MSG_DESTROYRULE => handle_nft_delrule(&genmsg, attr_payload),

        NFT_MSG_NEWSET => handle_nft_newset(&genmsg, attr_payload),
        NFT_MSG_GETSET => handle_nft_getset(&genmsg, attr_payload),
        NFT_MSG_DELSET => {
            // Silme işlemi — basitleştirilmiş
            let _ = genmsg;
            Ok(Vec::new())
        }

        NFT_MSG_NEWSETELEM => handle_nft_newsetelem(&genmsg, attr_payload),
        NFT_MSG_GETSETELEM => {
            // Set elemanlarını döndür — basitleştirilmiş
            Ok(Vec::new())
        }
        NFT_MSG_DELSETELEM => Ok(Vec::new()),

        NFT_MSG_GETGEN => handle_nft_getgen(&genmsg, attr_payload),

        NFT_MSG_NEWOBJ | NFT_MSG_GETOBJ | NFT_MSG_DELOBJ => Ok(Vec::new()),

        NFT_MSG_NEWFLOWTABLE | NFT_MSG_GETFLOWTABLE | NFT_MSG_DELFLOWTABLE => Ok(Vec::new()),

        _ => Ok(Vec::new()),
    };

    match result {
        Ok(responses) => responses,
        Err(()) => Vec::new(),
    }
}

// ============================================================================
// TESTLER
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nft_verdict_accept() {
        let v = VerdictExpr::accept();
        assert_eq!(v.kind, VerdictKind::Accept);
        assert_eq!(verdict_to_nf(&v), NF_ACCEPT);
    }

    #[test]
    fn nft_verdict_drop() {
        let v = VerdictExpr::drop();
        assert_eq!(v.kind, VerdictKind::Drop);
        assert_eq!(verdict_to_nf(&v), NF_DROP);
    }

    #[test]
    fn nft_verdict_jump() {
        let v = VerdictExpr::jump("mychain");
        assert_eq!(v.kind, VerdictKind::Jump);
        assert_eq!(v.chain.as_deref(), Some("mychain"));
        // Jump NF_ACCEPT döner; zincir atlaması ayrı işlenir
        assert_eq!(verdict_to_nf(&v), NF_ACCEPT);
    }

    #[test]
    fn nft_verdict_return() {
        let v = VerdictExpr::return_();
        assert_eq!(v.kind, VerdictKind::Return);
        assert_eq!(verdict_to_nf(&v), 0xFFFFFFFF);
    }

    #[test]
    fn nft_cmp_eq_u8() {
        let c = CmpExpr::eq_u8(0, 42);
        assert_eq!(c.op, CmpOp::Eq);
        assert_eq!(c.sreg, 0);
        assert_eq!(c.data, vec![42]);
    }

    #[test]
    fn nft_cmp_eq_u16() {
        let c = CmpExpr::eq_u16(4, 80);
        assert_eq!(c.data, vec![80, 0]);
    }

    #[test]
    fn nft_cmp_neq_u8() {
        let c = CmpExpr::neq_u8(0, 0);
        assert_eq!(c.op, CmpOp::Neq);
    }

    #[test]
    fn nft_payload_ipv4_src() {
        let p = PayloadExpr::ipv4_src(0);
        assert_eq!(p.base, PayloadBase::NetworkHeader);
        assert_eq!(p.offset, 12);
        assert_eq!(p.width, 4);
    }

    #[test]
    fn nft_payload_ipv4_dst() {
        let p = PayloadExpr::ipv4_dst(4);
        assert_eq!(p.offset, 16);
        assert_eq!(p.dreg, 4);
    }

    #[test]
    fn nft_payload_tcp_sport() {
        let p = PayloadExpr::tcp_sport(0);
        assert_eq!(p.base, PayloadBase::TransportHeader);
        assert_eq!(p.offset, 0);
        assert_eq!(p.width, 2);
    }

    #[test]
    fn nft_payload_tcp_flags() {
        let p = PayloadExpr::tcp_flags(0);
        assert_eq!(p.offset, 13);
        assert_eq!(p.width, 1);
    }

    #[test]
    fn nft_payload_icmp_type() {
        let p = PayloadExpr::icmp_type(0);
        assert_eq!(p.offset, 0);
        assert_eq!(p.width, 1);
    }

    #[test]
    fn nft_meta_iifname() {
        let m = MetaExpr::iifname(0);
        assert_eq!(m.key, MetaKey::IifName);
        assert_eq!(m.dreg, 0);
    }

    #[test]
    fn nft_meta_nfproto() {
        let m = MetaExpr::nfproto(0);
        assert_eq!(m.key, MetaKey::Nfproto);
    }

    #[test]
    fn nft_counter() {
        let c = CounterExpr::new();
        assert_eq!(c.snapshot(), (0, 0));
        c.update(100);
        assert_eq!(c.snapshot(), (1, 100));
        c.update(200);
        assert_eq!(c.snapshot(), (2, 300));
    }

    #[test]
    fn nft_bitwise_mask_xor_u8() {
        let b = BitwiseExpr::mask_xor_u8(0, 4, 0xFF, 0);
        assert_eq!(b.op, BitwiseOp::MaskXor);
        assert_eq!(b.mask, vec![0xFF]);
        assert_eq!(b.xor, vec![0]);
    }

    #[test]
    fn nft_bitwise_mask_xor_u16() {
        let b = BitwiseExpr::mask_xor_u16(0, 4, 0xFF00, 0);
        assert_eq!(b.mask, vec![0x00, 0xFF]); // little-endian
    }

    #[test]
    fn nft_nat_snat() {
        let n = NatExpr::snat(0xC0A80101, 8080);
        assert_eq!(n.nat_type, NatType::Source);
        assert_eq!(n.addr_min, 0xC0A80101);
        assert_eq!(n.proto_min, 8080);
    }

    #[test]
    fn nft_nat_dnat() {
        let n = NatExpr::dnat(0x0A000001, 80);
        assert_eq!(n.nat_type, NatType::Destination);
        assert_eq!(n.addr_min, 0x0A000001);
        assert_eq!(n.proto_min, 80);
    }

    #[test]
    fn nft_reject_icmp_unreach() {
        let r = RejectExpr::icmp_port_unreachable();
        assert_eq!(r.reject_type, RejectType::IcmpUnreach);
        assert_eq!(r.icmp_code, 1); // Port Unreachable
    }

    #[test]
    fn nft_reject_tcp_rst() {
        let r = RejectExpr::tcp_rst();
        assert_eq!(r.reject_type, RejectType::TcpRst);
    }

    #[test]
    fn nft_log_expr() {
        let l = LogExpr::new("NFT:");
        assert_eq!(l.prefix, "NFT:");
        assert_eq!(l.level, LogLevel::Notice);
        assert_eq!(l.snaplen, 0xFFFF);
    }

    #[test]
    fn nft_quota() {
        let q = QuotaExpr::new(1024, false);
        assert!(!q.is_exceeded());
        assert!(!q.consume(512));
        assert!(!q.is_exceeded());
        assert!(q.consume(600)); // 512+600=1112 > 1024 → aşıldı
    }

    #[test]
    fn nft_quota_invert() {
        let q = QuotaExpr::new(100, true);
        // invert=true: initially exceeded (consumed=0 < 100, but invert flips)
        assert!(q.is_exceeded());
    }

    #[test]
    fn nft_lookup() {
        let l = LookupExpr::new(0, "blocked_ips", 4);
        assert_eq!(l.sreg, 0);
        assert_eq!(l.set_name, "blocked_ips");
        assert_eq!(l.dreg, 4);
        assert!(!l.invert);
    }

    #[test]
    fn nft_lookup_invert() {
        let l = LookupExpr::invert(0, "whitelist", 4);
        assert!(l.invert);
    }

    #[test]
    fn nft_ct_state() {
        let ct = CtExpr::state(0);
        assert_eq!(ct.key, CtKey::State);
        assert_eq!(ct.dreg, 0);
    }

    #[test]
    fn nft_ct_mark_set() {
        let ct = CtExpr::mark_set(0);
        assert_eq!(ct.key, CtKey::Mark);
        assert_eq!(ct.sreg, 0);
    }

    #[test]
    fn nft_fib_oifname() {
        let f = FibExpr::oifname(0, FIB_FLAG_SADDR);
        assert_eq!(f.result, FibResult::OifName);
        assert_eq!(f.flags, FIB_FLAG_SADDR);
    }

    #[test]
    fn nft_masq() {
        let m = MasqExpr::new();
        assert_eq!(m.flags, NAT_FLAG_MAP_IPS);
    }

    #[test]
    fn nft_tproxy() {
        let t = TproxyExpr::new(NFPROTO_IPV4, 8080);
        assert_eq!(t.family, NFPROTO_IPV4);
        assert_eq!(t.port, 8080);
    }

    #[test]
    fn nft_flow_offload() {
        let f = FlowOffloadExpr::new();
        assert!(f.name.is_none());
        let f2 = FlowOffloadExpr::with_name("wifi");
        assert_eq!(f2.name.as_deref(), Some("wifi"));
    }

    #[test]
    fn nft_objref() {
        let o = ObjrefExpr::new("my_counter");
        assert_eq!(o.name, "my_counter");
    }

    #[test]
    fn nft_rule_build_and_evaluate() {
        let rule = NftRule::new(1)
            .with_expr(NftExpression::Counter(CounterExpr::new()))
            .with_expr(NftExpression::Verdict(VerdictExpr::accept()));

        let mut pkt = PacketInfo {
            family: NFPROTO_IPV4,
            src_ip: 0xC0A80101,
            dst_ip: 0xC0A80164,
            src_addr: [0; 16],
            dst_addr: [0; 16],
            src_port: 12345,
            dst_port: 80,
            proto: 6,
            in_iface: String::from("eth0"),
            out_iface: String::from("eth0"),
            in_iface_ip: 0,
            out_iface_ip: 0,
            in_iface_addr: [0; 16],
            out_iface_addr: [0; 16],
            len: 64,
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
        };

        let mut regs = [0u8; 64];
        assert!(rule.evaluate(&mut pkt, &mut regs));
    }

    #[test]
    fn nft_chain_traverse_accept() {
        let mut chain = NftChain::new_base("input", 1, NftChainType::Filter, NF_INET_LOCAL_IN, 0, NF_ACCEPT);

        let rule = NftRule::new(1)
            .with_expr(NftExpression::Verdict(VerdictExpr::accept()));
        chain.add_rule(rule);

        let mut pkt = PacketInfo {
            family: NFPROTO_IPV4,
            src_ip: 0xC0A80101,
            dst_ip: 0xC0A80164,
            src_addr: [0; 16],
            dst_addr: [0; 16],
            src_port: 12345,
            dst_port: 80,
            proto: 6,
            in_iface: String::from("eth0"),
            out_iface: String::from("eth0"),
            in_iface_ip: 0,
            out_iface_ip: 0,
            in_iface_addr: [0; 16],
            out_iface_addr: [0; 16],
            len: 64,
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
        };

        assert_eq!(chain.traverse(&mut pkt), NF_ACCEPT);
    }

    #[test]
    fn nft_chain_traverse_drop() {
        let mut chain = NftChain::new_base("input", 1, NftChainType::Filter, NF_INET_LOCAL_IN, 0, NF_ACCEPT);

        let rule = NftRule::new(1)
            .with_expr(NftExpression::Verdict(VerdictExpr::drop()));
        chain.add_rule(rule);

        let mut pkt = PacketInfo {
            family: NFPROTO_IPV4,
            src_ip: 0xC0A80101,
            dst_ip: 0xC0A80164,
            src_addr: [0; 16],
            dst_addr: [0; 16],
            src_port: 12345,
            dst_port: 80,
            proto: 6,
            in_iface: String::from("eth0"),
            out_iface: String::from("eth0"),
            in_iface_ip: 0,
            out_iface_ip: 0,
            in_iface_addr: [0; 16],
            out_iface_addr: [0; 16],
            len: 64,
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
        };

        assert_eq!(chain.traverse(&mut pkt), NF_DROP);
    }

    #[test]
    fn nft_chain_empty_uses_policy() {
        let mut chain = NftChain::new_base("input", 1, NftChainType::Filter, NF_INET_LOCAL_IN, 0, NF_DROP);

        // Zincir boş — politika dönmeli
        let mut pkt = PacketInfo {
            family: NFPROTO_IPV4,
            src_ip: 0,
            dst_ip: 0,
            src_addr: [0; 16],
            dst_addr: [0; 16],
            src_port: 0,
            dst_port: 0,
            proto: 0,
            in_iface: String::new(),
            out_iface: String::new(),
            in_iface_ip: 0,
            out_iface_ip: 0,
            in_iface_addr: [0; 16],
            out_iface_addr: [0; 16],
            len: 0,
            ttl: 0,
            tos: 0,
            new_src_ip: 0,
            new_dst_ip: 0,
            has_new_src_addr: false,
            has_new_dst_addr: false,
            new_src_addr: [0; 16],
            new_dst_addr: [0; 16],
            new_src_port: 0,
            new_dst_port: 0,
            new_ttl: 0,
            new_tos: 0,
            conntrack_state: ConntrackState::New,
        };

        assert_eq!(chain.traverse(&mut pkt), NF_DROP);
    }

    #[test]
    fn nft_table_add_delete_chain() {
        let mut table = NftTable::new("test", 1, NFPROTO_IPV4);
        let chain = NftChain::new_base("input", 2, NftChainType::Filter, NF_INET_LOCAL_IN, 0, NF_ACCEPT);
        table.add_chain(chain);
        assert!(table.get_chain("input").is_some());
        assert!(table.delete_chain("input"));
        assert!(table.get_chain("input").is_none());
    }

    #[test]
    fn nft_set_operations() {
        let mut set = NftSet::new("blocked_ports", 1, NftSetType::Port, 2);
        let elem = NftSetElem {
            key: vec![80, 0], // port 80 little-endian
            data: Vec::new(),
            timeout_ms: 0,
            flags: 0,
            expressions: Vec::new(),
        };
        set.add_element(elem);
        assert_eq!(set.elements.len(), 1);

        let found = set.lookup(&[80, 0]);
        assert!(found.is_some());

        assert!(set.delete_element(&[80, 0]));
        assert!(set.elements.is_empty());
    }

    #[test]
    fn nft_cmp_eval_eq() {
        let mut regs = [0u8; 64];
        regs[0] = 42;

        let c = CmpExpr::eq_u8(0, 42);
        assert!(evaluate_cmp(&c, &regs));

        let c2 = CmpExpr::eq_u8(0, 99);
        assert!(!evaluate_cmp(&c2, &regs));
    }

    #[test]
    fn nft_cmp_eval_neq() {
        let mut regs = [0u8; 64];
        regs[0] = 42;

        let c = CmpExpr::neq_u8(0, 42);
        assert!(!evaluate_cmp(&c, &regs));

        let c2 = CmpExpr::neq_u8(0, 99);
        assert!(evaluate_cmp(&c2, &regs));
    }

    #[test]
    fn nft_cmp_eval_u16() {
        let mut regs = [0u8; 64];
        // port 80 little-endian
        regs[0] = 80;
        regs[1] = 0;

        let c = CmpExpr::eq_u16(0, 80);
        assert!(evaluate_cmp(&c, &regs));

        let c2 = CmpExpr::eq_u16(0, 443);
        assert!(!evaluate_cmp(&c2, &regs));
    }

    #[test]
    fn nft_bitwise_eval() {
        let mut regs = [0u8; 64];
        // 2-byte payload: [80, 0] (port 80 in LE)
        regs[0] = 80;
        regs[1] = 0;

        // Mask low byte only: result = (reg[0..1] & [0xFF, 0x00]) ^ [0, 0] = [80, 0]
        let b = BitwiseExpr::mask_xor_u16(0, 4, 0x00FF, 0);
        assert!(evaluate_bitwise(&b, &mut regs));
        assert_eq!(regs[4], 80);
        assert_eq!(regs[5], 0);
    }

    #[test]
    fn nft_meta_eval_iifname() {
        let mut regs = [0u8; 64];
        let pkt = PacketInfo {
            family: NFPROTO_IPV4,
            src_ip: 0,
            dst_ip: 0,
            src_addr: [0; 16],
            dst_addr: [0; 16],
            src_port: 0,
            dst_port: 0,
            proto: 0,
            in_iface: String::from("eth0"),
            out_iface: String::new(),
            in_iface_ip: 0,
            out_iface_ip: 0,
            in_iface_addr: [0; 16],
            out_iface_addr: [0; 16],
            len: 0,
            ttl: 0,
            tos: 0,
            new_src_ip: 0,
            new_dst_ip: 0,
            has_new_src_addr: false,
            has_new_dst_addr: false,
            new_src_addr: [0; 16],
            new_dst_addr: [0; 16],
            new_src_port: 0,
            new_dst_port: 0,
            new_ttl: 0,
            new_tos: 0,
            conntrack_state: ConntrackState::New,
        };

        let m = MetaExpr::iifname(0);
        assert!(evaluate_meta(&m, &pkt, &mut regs));

        // "eth0" should be in registers
        assert_eq!(regs[0], b'e');
        assert_eq!(regs[1], b't');
        assert_eq!(regs[2], b'h');
        assert_eq!(regs[3], b'0');
        assert_eq!(regs[4], 0); // null terminator
    }

    #[test]
    fn nft_meta_eval_nfproto() {
        let mut regs = [0u8; 64];
        let pkt = PacketInfo {
            family: NFPROTO_IPV4,
            src_ip: 0,
            dst_ip: 0,
            src_addr: [0; 16],
            dst_addr: [0; 16],
            src_port: 0,
            dst_port: 0,
            proto: 0,
            in_iface: String::new(),
            out_iface: String::new(),
            in_iface_ip: 0,
            out_iface_ip: 0,
            in_iface_addr: [0; 16],
            out_iface_addr: [0; 16],
            len: 0,
            ttl: 0,
            tos: 0,
            new_src_ip: 0,
            new_dst_ip: 0,
            has_new_src_addr: false,
            has_new_dst_addr: false,
            new_src_addr: [0; 16],
            new_dst_addr: [0; 16],
            new_src_port: 0,
            new_dst_port: 0,
            new_ttl: 0,
            new_tos: 0,
            conntrack_state: ConntrackState::New,
        };

        let m = MetaExpr::nfproto(0);
        assert!(evaluate_meta(&m, &pkt, &mut regs));
        assert_eq!(regs[0], NFPROTO_IPV4 as u8);
    }

    #[test]
    fn nft_ipt_entry_to_nft_rule() {
        let mut entry = super::super::netfilter::IptEntry::new();
        entry.proto = 6; // TCP
        entry.src_ports = (80, 80);
        entry.target = super::super::netfilter::IptTarget::accept();

        let rule = ipt_entry_to_nft_rule(&entry);
        // Kural ifadeler içermeli
        assert!(!rule.expressions.is_empty());
    }

    #[test]
    fn nft_manager_init() {
        let mgr = NftablesManager::new();
        mgr.init();

        // ip tablosu mevcut olmalı
        assert!(mgr.get_table("ip").is_some());
        // nat tablosu mevcut olmalı
        assert!(mgr.get_table("nat").is_some());
        // inet tablosu mevcut olmalı
        assert!(mgr.get_table("inet").is_some());

        // ip tablosunda zincirler olmalı
        let ip = mgr.get_table("ip").unwrap();
        assert!(ip.get_chain("input").is_some());
        assert!(ip.get_chain("forward").is_some());
        assert!(ip.get_chain("output").is_some());
    }

    #[test]
    fn nft_manager_new_table() {
        let mgr = NftablesManager::new();
        let handle = mgr.new_table("arp", NFPROTO_IPV4).unwrap();
        assert!(handle > 0);
        assert!(mgr.get_table("arp").is_some());

        // Aynı tabloyu tekrar oluşturamazsın
        assert_eq!(mgr.new_table("arp", NFPROTO_IPV4), Err(NftablesError::TableExists));
    }

    #[test]
    fn nft_manager_delete_table() {
        let mgr = NftablesManager::new();
        mgr.new_table("temp", NFPROTO_IPV4).unwrap();
        assert!(mgr.delete_table("temp").is_ok());
        assert!(mgr.get_table("temp").is_none());

        // Olmayan tabloyu silemezsin
        assert_eq!(mgr.delete_table("nonexistent"), Err(NftablesError::TableNotFound));
    }

    #[test]
    fn nft_manager_new_chain() {
        let mgr = NftablesManager::new();
        mgr.init();

        let chain = NftChain::new_base(
            "custom",
            100,
            NftChainType::Filter,
            NF_INET_LOCAL_IN,
            0,
            NF_ACCEPT,
        );
        assert!(mgr.new_chain("ip", chain).is_ok());

        let ip = mgr.get_table("ip").unwrap();
        assert!(ip.get_chain("custom").is_some());
    }

    #[test]
    fn nft_manager_new_rule() {
        let mgr = NftablesManager::new();
        mgr.init();

        let rule = NftRule::new(1)
            .with_expr(NftExpression::Counter(CounterExpr::new()))
            .with_expr(NftExpression::Verdict(VerdictExpr::accept()));

        let handle = mgr.new_rule("ip", "input", rule).unwrap();
        assert!(handle > 0);
    }

    #[test]
    fn nft_manager_new_set() {
        let mgr = NftablesManager::new();
        mgr.init();

        let set = NftSet::new("blocked", 1, NftSetType::Ipv4Addr, 4);
        assert!(mgr.new_set("ip", set).is_ok());
    }

    #[test]
    fn nft_manager_generation_id() {
        let mgr = NftablesManager::new();
        let gen1 = mgr.get_generation_id();
        mgr.init();
        let gen2 = mgr.get_generation_id();
        assert!(gen2 > gen1);
    }

    #[test]
    fn nft_manager_stats() {
        let mgr = NftablesManager::new();
        let stats = mgr.get_stats();
        assert_eq!(stats.packets_processed, 0);
    }
}
