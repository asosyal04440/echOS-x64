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
        // Uzunluk = 4-byte başlık + veri (padding sayılmaz)
        let len = (4 + data.len()) as u16;
        buf.extend_from_slice(&len.to_le_bytes());
        buf.extend_from_slice(&attr_type.to_le_bytes());
        buf.extend_from_slice(data);
        // 4-byte hizalama için sıfır doldur (Netlink zorunluluğu)
        while buf.len() % 4 != 0 {
            buf.push(0);
        }
        buf
    }
}

// ============================================================================
// NETLINK MESAJ TİPLERİ (RTM_* ve GENL_*)
// ============================================================================
//
// RTM (Route Message) tipleri NETLINK_ROUTE protokolünde kullanılır.
// Her nesne tipi için üçlü bir komut seti vardır: NEW, DEL, GET.
//
//  Nesne         NEW     DEL     GET     SET
//  ──────────────────────────────────────────
//  Arayüz (Link)  16      17      18      19
//  IP Adresi      20      21      22       -
//  Rota           24      25      26       -
//  Komşu (ARP)    28      29      30       -
//  Kural          32      33      34       -

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
            NETLINK_NETFILTER => self.handle_netfilter(msg),
            NETLINK_XFRM      => self.handle_xfrm(msg),
            NETLINK_AUDIT     => self.handle_audit(msg),
            NETLINK_KOBJECT_UEVENT => self.handle_uevent(msg),
            NETLINK_GENERIC   => self.handle_generic(msg),
            _ => Ok(()),
        }
    }

    /// NETLINK_ROUTE mesajlarını işler.
    /// GET istekleri -> yanıt oluştur ve rx_buf'a ekle (asenkron yanıt modeli).
    /// NEW/SET istekleri -> ağ yapılandırmasını güncelle.
    fn handle_route(&self, msg: &NetlinkMessage) -> Result<(), NetlinkError> {
        match msg.header.nlmsg_type {
            RTM_GETLINK | RTM_GETADDR | RTM_GETROUTE => {
                // Ağ yapılandırmasını döküm et (dump): NLM_F_MULTI serisi oluşturulur
                let reply = self.build_route_reply(msg);
                self.rx_buf.lock().push(reply);
            }
            RTM_NEWLINK | RTM_NEWADDR | RTM_NEWROUTE => {
                // Yeni arayüz/adres/rota yapılandırması uygula
            }
            _ => {}
        }
        Ok(())
    }

    /// RTM_GET* sorgusu için yanıt mesajı oluşturur.
    fn build_route_reply(&self, _msg: &NetlinkMessage) -> NetlinkMessage {
        NetlinkMessage {
            header: NlMsgHdr::new(0, RTM_NEWLINK, NLM_F_MULTI, 0, 0),
            payload: Vec::new(),
        }
    }

    /// NETLINK_NETFILTER: iptables/nftables kural işlemleri.
    fn handle_netfilter(&self, _msg: &NetlinkMessage) -> Result<(), NetlinkError> {
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
    fn handle_generic(&self, _msg: &NetlinkMessage) -> Result<(), NetlinkError> {
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
        *(buf.as_ptr() as *const NlMsgHdr)
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
