//! # IGMP / Multicast Grup Yönetimi (Internet Group Management Protocol)
//!
//! RFC 2236 (IGMPv2) + RFC 3376 (IGMPv3) uyumlu çoklu yayın yönetimi.
//!
//! ## IGMP Nedir?
//!
//! IGMP, bir IPv4 ağında host'ların router'a "şu multicast grubuna katılmak
//! istiyorum" demesini sağlayan protokoldür. Bu sayede:
//! - Router sadece üyelerin olduğu LAN segmentine multicast trafiği yollar
//! - Host ilgilenmediği grupların trafiğini almaz
//!
//! ## IGMP Mesaj Tipleri (Type)
//!
//! | Tip | Ad | RFC | Açıklama |
//! |-----|----|-----|----------|
//! | 0x11 | Membership Query | 2236/3376 | Router sorar "kim üye?" |
//! | 0x12 | v1 Membership Report | 1112 | Eski tip katılım raporu |
//! | 0x16 | v2 Membership Report | 2236 | v2 katılım raporu |
//! | 0x17 | Leave Group | 2236 | v2 ayrılma bildirimi |
//! | 0x22 | v3 Membership Report | 3376 | v3 kaynak-spesifik katılım |
//!
//! ## v2 Report vs v3 Report
//!
//! **v2 Report**: Tek grup, INCLUDE modu (o gruba katıl)
//! **v3 Report**: Çoklu grup + kaynak listesi (INCLUDE veya EXCLUDE modu)
//!
//! ## echOS IGMP Tasarımı
//!
//! echOS'ta multicast durumu şu şekilde tutulur:
//! - `MULTICAST_GROUPS`: (group_addr, interface_idx) -> `MulticastGroupState`
//! - Her grup: üyelik modu, kaynak listesi, son rapor zamanı, zamanlayıcılar
//!
//! **Zamanlayıcılar (RFC 3376 §8):**
//! - `group_membership_interval` = robustness_variable × query_interval + query_response_interval (varsayılan: 260s)
//! - `last_member_query_time` = last_member_query_count × last_member_query_interval (varsayılan: 3s)
//! - `unsolicited_report_interval` = 1s (ilk rapor)
//!
//! ## Kullanım
//!
//! ```text
//! // Gruba katıl
//! igmp::join_group(IpMreq { multiaddr: 224.0.0.1, interface: 0 });
//!
//! // Kaynak-spesifik katılım (SSM, RFC 4607)
//! igmp::join_source_specific(232.1.2.3, 0, 192.0.2.5);
//!
//! // Ayrıl
//! igmp::leave_group(IpMreq { multiaddr: 224.0.0.1, interface: 0 });
//!
//! // Yerel grup tablosu
//! let groups = igmp::list_groups();
//! ```

use super::{Ipv4Addr, Mutex};
use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

// ============================================================================
// IGMP MESAJ TİPLERİ (RFC 2236 §2, RFC 3376 §4.1)
// ============================================================================

/// IGMP Membership Query — router host'lara "kim üye?" diye sorar
pub const IGMP_TYPE_QUERY: u8 = 0x11;
/// IGMPv1 Membership Report (eski, RFC 1112)
pub const IGMP_TYPE_V1_REPORT: u8 = 0x12;
/// IGMPv2 Membership Report (RFC 2236)
pub const IGMP_TYPE_V2_REPORT: u8 = 0x16;
/// IGMPv2 Leave Group (RFC 2236)
pub const IGMP_TYPE_V2_LEAVE: u8 = 0x17;
/// IGMPv3 Membership Report (RFC 3376)
pub const IGMP_TYPE_V3_REPORT: u8 = 0x22;

/// Maksimum multicast grup sayısı (soket başına)
pub const MAX_MULTICAST_GROUPS: usize = 64;

// ============================================================================
// IP_ADD_MEMBERSHIP YAPISI (RFC 3678 / POSIX)
// ============================================================================

/// `struct ip_mreq` — IP_ADD_MEMBERSHIP / IP_DROP_MEMBERSHIP argümanı
///
/// Linux ABI:
/// ```c
/// struct ip_mreq {
///     struct in_addr imr_multiaddr;  // Multicast grup adresi
///     struct in_addr imr_interface;  // Yerel arayüz adresi
/// };
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IpMreq {
    pub imr_multiaddr: Ipv4Addr,
    pub imr_interface: Ipv4Addr,
}

impl IpMreq {
    pub const fn new(multi: Ipv4Addr, iface: Ipv4Addr) -> Self {
        IpMreq {
            imr_multiaddr: multi,
            imr_interface: iface,
        }
    }
}

// ============================================================================
// ÜYELİK MODLARI (RFC 3376 §3)
// ============================================================================

/// IGMPv3 üyelik modu
///
/// - `Include` : Sadece listedeki kaynaklardan gelen trafiği kabul et (SSM)
/// - `Exclude` : Listede OLMAYAN kaynaklardan gelen trafiği kabul et (ASM)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterMode {
    Include = 1,
    Exclude = 2,
}

// ============================================================================
// MULTICAST GRUP DURUMU
// ============================================================================

/// Bir multicast grup için yerel durum
#[derive(Clone, Debug)]
pub struct MulticastGroupState {
    pub group_addr: Ipv4Addr,
    pub interface_idx: u32,
    pub mode: FilterMode,
    /// Source listesi: Include modunda SADECE bu kaynaklardan kabul,
    /// Exclude modunda bu KAYNAKLAR HARİÇ hepsinden kabul
    pub sources: Vec<Ipv4Addr>,
    /// Son rapor gönderilme zamanı (ticks, ~1ms)
    pub last_report_ticks: u64,
    /// Pending v2 Leave gönderimi var mı?
    pub leave_pending: bool,
    /// Toplam alınan byte sayısı (istatistik)
    pub bytes_received: u64,
    /// Toplam alınan paket sayısı
    pub packets_received: u64,
}

impl MulticastGroupState {
    pub fn new(group_addr: Ipv4Addr, interface_idx: u32) -> Self {
        MulticastGroupState {
            group_addr,
            interface_idx,
            mode: FilterMode::Exclude,
            sources: Vec::new(),
            last_report_ticks: 0,
            leave_pending: false,
            bytes_received: 0,
            packets_received: 0,
        }
    }
}

// ============================================================================
// KÜRESEL DURUM
// ============================================================================

/// Multicast grup tablosu: (group_addr, interface_marker) -> durum
///
/// `interface_marker` yükü: `join_group` için `imr_interface` IPv4 adresinin
/// big-endian u32 karşılığı, `join_source_specific` için ham `interface_idx`.
/// POSIX `ip_mreq` ABI'si interface identifier olarak IPv4 adres kullanır.
static MULTICAST_GROUPS: Mutex<BTreeMap<(u32, u32), MulticastGroupState>> =
    Mutex::new(BTreeMap::new());

/// IGMP istatistikleri
static IGMP_STATS: IgmpStats = IgmpStats::new();

struct IgmpStats {
    queries_received: AtomicU32,
    reports_sent: AtomicU32,
    leaves_sent: AtomicU32,
    joins: AtomicU32,
    leaves: AtomicU32,
    drops: AtomicU32,
}

impl IgmpStats {
    const fn new() -> Self {
        IgmpStats {
            queries_received: AtomicU32::new(0),
            reports_sent: AtomicU32::new(0),
            leaves_sent: AtomicU32::new(0),
            joins: AtomicU32::new(0),
            leaves: AtomicU32::new(0),
            drops: AtomicU32::new(0),
        }
    }
}

/// IGMP istatistiklerini oku
pub fn stats() -> (u32, u32, u32, u32, u32, u32) {
    (
        IGMP_STATS.queries_received.load(Ordering::Relaxed),
        IGMP_STATS.reports_sent.load(Ordering::Relaxed),
        IGMP_STATS.leaves_sent.load(Ordering::Relaxed),
        IGMP_STATS.joins.load(Ordering::Relaxed),
        IGMP_STATS.leaves.load(Ordering::Relaxed),
        IGMP_STATS.drops.load(Ordering::Relaxed),
    )
}

// ============================================================================
// PUBLIC API
// ============================================================================

/// Belirtilen multicast gruba katıl (v2 uyumlu, Exclude modu, kaynak listesi boş).
///
/// Bu çağrı gruba ilk kez katılıyorsa IGMPv2/v3 Membership Report üretmek
/// için `mark_unsolicited_report` çağrılmalıdır.
pub fn join_group(mreq: IpMreq) {
    let key = (
        u32::from_be_bytes(mreq.imr_multiaddr.0),
        u32::from_be_bytes(mreq.imr_interface.0),
    );
    let mut groups = MULTICAST_GROUPS.lock();
    if groups.contains_key(&key) {
        return; // Zaten üye
    }
    if groups.len() >= MAX_MULTICAST_GROUPS {
        IGMP_STATS.drops.fetch_add(1, Ordering::Relaxed);
        crate::serial_println!(
            "[IGMP] join_group: max groups ({}) reached, dropping",
            MAX_MULTICAST_GROUPS
        );
        return;
    }
    groups.insert(key, MulticastGroupState::new(mreq.imr_multiaddr, 0));
    IGMP_STATS.joins.fetch_add(1, Ordering::Relaxed);
    crate::serial_println!(
        "[IGMP] join_group: {}/{}",
        mreq.imr_multiaddr,
        mreq.imr_interface
    );
}

/// SSM (Source-Specific Multicast, RFC 4607) için kaynak-spesifik katılım.
///
/// `group_addr` 232.0.0.0/8 aralığında olmalı (SSM adres alanı).
/// `source_addr` kabul edilecek tek kaynak.
pub fn join_source_specific(
    group_addr: Ipv4Addr,
    interface_idx: u32,
    source_addr: Ipv4Addr,
) {
    let key = (u32::from_be_bytes(group_addr.0), interface_idx);
    let mut groups = MULTICAST_GROUPS.lock();
    let entry = groups
        .entry(key)
        .or_insert_with(|| MulticastGroupState::new(group_addr, interface_idx));
    entry.mode = FilterMode::Include;
    if !entry.sources.contains(&source_addr) {
        entry.sources.push(source_addr);
    }
    IGMP_STATS.joins.fetch_add(1, Ordering::Relaxed);
    crate::serial_println!(
        "[IGMP] SSM join: group={} source={}",
        group_addr,
        source_addr
    );
}

/// Multicast gruptan ayrıl.
///
/// Entry'i hemen silmez; `leave_pending = true` işaretler ve
/// `mark_leave_sent` çağrılana kadar tabloda kalır. Bu sayede `pending_leaves`
/// Leave paketini üretebilir, gönderici ağ katmanı iletir, sonra
/// `mark_leave_sent` çağrılarak entry temizlenir.
pub fn leave_group(mreq: IpMreq) {
    let key = (
        u32::from_be_bytes(mreq.imr_multiaddr.0),
        u32::from_be_bytes(mreq.imr_interface.0),
    );
    let mut groups = MULTICAST_GROUPS.lock();
    if let Some(state) = groups.get_mut(&key) {
        // v2 davranışı: v2 router varsa Leave gönder
        state.leave_pending = true;
        IGMP_STATS.leaves.fetch_add(1, Ordering::Relaxed);
        crate::serial_println!(
            "[IGMP] leave_group: {}/{} (pending)",
            mreq.imr_multiaddr,
            mreq.imr_interface
        );
    }
}

/// Belirli bir (group, interface) çifti için üyelik var mı?
///
/// `interface_addr` IPv4 arayüz adresi (`IpMreq::imr_interface` ile aynı ABI).
pub fn is_member(group_addr: Ipv4Addr, interface_addr: Ipv4Addr) -> bool {
    let key = (
        u32::from_be_bytes(group_addr.0),
        u32::from_be_bytes(interface_addr.0),
    );
    MULTICAST_GROUPS.lock().contains_key(&key)
}

/// Tüm aktif multicast gruplarını listele
pub fn list_groups() -> Vec<(Ipv4Addr, u32, FilterMode, usize)> {
    MULTICAST_GROUPS
        .lock()
        .values()
        .map(|s| (s.group_addr, s.interface_idx, s.mode, s.sources.len()))
        .collect()
}

/// Gelen bir IGMP Query paketini işle (RFC 3376 §5).
///
/// Çağıran (genellikle IPv4 alıcı katmanı) gelen IGMP mesajını bu fonksiyona
/// iletir. Fonksiyon mesaj tipine göre istatistik günceller ve gerekirse
/// pending report/leave gönderimini tetikler.
pub fn handle_packet(packet: &[u8]) {
    if packet.len() < 8 {
        return;
    }
    let msg_type = packet[0];
    let _max_resp = packet[1]; // v3 Query: max response code (100ms units)
    let _checksum = u16::from_be_bytes([packet[2], packet[3]]);
    let group_addr = Ipv4Addr([
        packet[4], packet[5], packet[6], packet[7],
    ]);

    match msg_type {
        IGMP_TYPE_QUERY => {
            IGMP_STATS.queries_received.fetch_add(1, Ordering::Relaxed);
            // Gerçek router'dan query aldık; pending report'larımızı gönderme
            // zamanlaması güncellenebilir. Basitleştirilmiş: sadece log.
            crate::serial_println!("[IGMP] query for group {}", group_addr);
        }
        IGMP_TYPE_V1_REPORT | IGMP_TYPE_V2_REPORT | IGMP_TYPE_V3_REPORT => {
            // Başka bir host'un report'u — bizim göndermemize gerek yok
            crate::serial_println!("[IGMP] report from another host for {}", group_addr);
        }
        IGMP_TYPE_V2_LEAVE => {
            crate::serial_println!("[IGMP] v2 leave for {}", group_addr);
        }
        _ => {
            crate::serial_println!("[IGMP] unknown msg type 0x{:02x}", msg_type);
        }
    }
}

/// Pending IGMP raporlarını üret (tick handler'dan çağrılır).
///
/// Her rapor:
/// - `IGMP_TYPE_V2_REPORT` (8 byte: type, max_resp=0, checksum, group_addr)
///   veya `IGMP_TYPE_V3_REPORT` (RFC 3376 §4.4 formatı) olabilir
///
/// Dönüş: (group_addr, raw_packet) listesi — gönderici ağ katmanına teslim edilir
pub fn pending_reports() -> Vec<(Ipv4Addr, Vec<u8>)> {
    let now = crate::interrupts::get_ticks();
    let groups = MULTICAST_GROUPS.lock();
    let mut out = Vec::new();
    for state in groups.values() {
        // İlk rapor 1 saniye içinde, sonraki raporlar 1 saniye aralıkla
        if state.last_report_ticks == 0
            || now.saturating_sub(state.last_report_ticks) >= 1000
        {
            let pkt = build_v2_report(state.group_addr);
            out.push((state.group_addr, pkt));
        }
    }
    out
}

/// Pending IGMP v2 Leave mesajlarını üret.
pub fn pending_leaves() -> Vec<(Ipv4Addr, Vec<u8>)> {
    let groups = MULTICAST_GROUPS.lock();
    let mut out = Vec::new();
    for state in groups.values() {
        if state.leave_pending {
            let pkt = build_v2_leave(state.group_addr);
            out.push((state.group_addr, pkt));
        }
    }
    out
}

/// Rapor gönderildi işaretle (zamanlayıcı reset).
///
/// `interface_addr` ile tam eşleşen entry'yi günceller; bulunamazsa hiçbir
/// şey yapmaz (entry zaten silinmiş olabilir).
pub fn mark_report_sent(group_addr: Ipv4Addr, interface_addr: Ipv4Addr) {
    let key = (
        u32::from_be_bytes(group_addr.0),
        u32::from_be_bytes(interface_addr.0),
    );
    let now = crate::interrupts::get_ticks();
    if let Some(s) = MULTICAST_GROUPS.lock().get_mut(&key) {
        s.last_report_ticks = now;
        IGMP_STATS.reports_sent.fetch_add(1, Ordering::Relaxed);
    }
}

/// Leave gönderildi işaretle. Entry'yi tablodan kaldırır (gönderildikten
/// sonra grup artık üye değil).
pub fn mark_leave_sent(group_addr: Ipv4Addr, interface_addr: Ipv4Addr) {
    let key = (
        u32::from_be_bytes(group_addr.0),
        u32::from_be_bytes(interface_addr.0),
    );
    let mut groups = MULTICAST_GROUPS.lock();
    if let Some(s) = groups.get_mut(&key) {
        s.leave_pending = false;
        IGMP_STATS.leaves_sent.fetch_add(1, Ordering::Relaxed);
    }
    groups.remove(&key);
}

// ============================================================================
// IGMPv2 PAKET OLUŞTURUCULARI
// ============================================================================

/// IGMPv2 Membership Report paketi (8 byte)
pub fn build_v2_report(group_addr: Ipv4Addr) -> Vec<u8> {
    let mut pkt = Vec::with_capacity(8);
    pkt.push(IGMP_TYPE_V2_REPORT);
    pkt.push(0); // Max Response Time (report'larda 0)
    pkt.push(0);
    pkt.push(0); // Checksum sıfırdan başlar, internet_checksum ile doldurulur
    pkt.extend_from_slice(&group_addr.0);
    let cs = super::checksum::internet_checksum(&pkt);
    pkt[2] = (cs >> 8) as u8;
    pkt[3] = (cs & 0xff) as u8;
    pkt
}

/// IGMPv2 Leave Group paketi (8 byte)
pub fn build_v2_leave(group_addr: Ipv4Addr) -> Vec<u8> {
    let mut pkt = Vec::with_capacity(8);
    pkt.push(IGMP_TYPE_V2_LEAVE);
    pkt.push(0);
    pkt.push(0);
    pkt.push(0);
    pkt.extend_from_slice(&group_addr.0);
    let cs = super::checksum::internet_checksum(&pkt);
    pkt[2] = (cs >> 8) as u8;
    pkt[3] = (cs & 0xff) as u8;
    pkt
}

/// IGMPv3 Membership Report paketi (RFC 3376 §4.4)
///
/// Format:
/// - type(1) | reserved(1) | checksum(2) | reserved(2) | number_of_group_records(2)
/// - Sonra her group record: record_type(1) | aux_data_len(1) | number_of_sources(2)
///   | multicast_address(4) | source_address × N | aux_data × M
pub fn build_v3_report(records: &[(u8, Ipv4Addr, &[Ipv4Addr])]) -> Vec<u8> {
    let mut pkt = Vec::new();
    pkt.push(IGMP_TYPE_V3_REPORT);
    pkt.push(0); // Reserved
    pkt.push(0);
    pkt.push(0); // Checksum sıfırdan başlar, internet_checksum ile doldurulur
    pkt.push(0);
    pkt.push(0); // Reserved
    pkt.push((records.len() as u16 >> 8) as u8);
    pkt.push((records.len() as u16 & 0xff) as u8);

    for (rec_type, group, sources) in records {
        pkt.push(*rec_type);
        pkt.push(0); // Aux data length
        pkt.push(((sources.len() as u16) >> 8) as u8);
        pkt.push((sources.len() as u16 & 0xff) as u8);
        pkt.extend_from_slice(&group.0);
        for s in *sources {
            pkt.extend_from_slice(&s.0);
        }
    }
    let cs = super::checksum::internet_checksum(&pkt);
    pkt[2] = (cs >> 8) as u8;
    pkt[3] = (cs & 0xff) as u8;
    pkt
}

/// IGMPv3 Group Record türleri (RFC 3376 §4.4.1)
pub const IGMP_V3_MODE_IS_INCLUDE: u8 = 1;
pub const IGMP_V3_MODE_IS_EXCLUDE: u8 = 2;
pub const IGMP_V3_CHANGE_TO_INCLUDE: u8 = 3;
pub const IGMP_V3_CHANGE_TO_EXCLUDE: u8 = 4;
pub const IGMP_V3_ALLOW_NEW_SOURCES: u8 = 5;
pub const IGMP_V3_BLOCK_OLD_SOURCES: u8 = 6;

// ============================================================================
// TESTLER
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v2_report_has_valid_checksum() {
        let group = Ipv4Addr::new(224, 0, 0, 1);
        let pkt = build_v2_report(group);
        assert_eq!(pkt.len(), 8);
        assert_eq!(pkt[0], IGMP_TYPE_V2_REPORT);
        // 224.0.0.1 little-endian big-endian
        assert_eq!(&pkt[4..8], &[224, 0, 0, 1]);
        // Checksum doğru olmalı
        let cs = super::super::checksum::internet_checksum(&pkt);
        assert_eq!(cs, 0, "checksum must sum to zero");
    }

    #[test]
    fn v2_leave_has_valid_checksum() {
        let group = Ipv4Addr::new(239, 1, 2, 3);
        let pkt = build_v2_leave(group);
        assert_eq!(pkt.len(), 8);
        assert_eq!(pkt[0], IGMP_TYPE_V2_LEAVE);
        let cs = super::super::checksum::internet_checksum(&pkt);
        assert_eq!(cs, 0);
    }

    #[test]
    fn v3_report_encodes_records() {
        let g = Ipv4Addr::new(232, 1, 2, 3);
        let s1 = Ipv4Addr::new(192, 0, 2, 5);
        let s2 = Ipv4Addr::new(198, 51, 100, 1);
        let sources = [s1, s2];
        let records = vec![(IGMP_V3_MODE_IS_INCLUDE, g, &sources[..])];
        let pkt = build_v3_report(&records);
        assert_eq!(pkt[0], IGMP_TYPE_V3_REPORT);
        // Number of records = 1 at offset 6..8
        assert_eq!(&pkt[6..8], &[0, 1]);
        // record_type at offset 8
        assert_eq!(pkt[8], IGMP_V3_MODE_IS_INCLUDE);
        // number_of_sources = 2 at offset 10..12
        assert_eq!(&pkt[10..12], &[0, 2]);
        // group address at 12..16
        assert_eq!(&pkt[12..16], &[232, 1, 2, 3]);
        // sources 16..20 and 20..24
        assert_eq!(&pkt[16..20], &[192, 0, 2, 5]);
        assert_eq!(&pkt[20..24], &[198, 51, 100, 1]);
    }

    #[test]
    fn join_and_leave_lifecycle() {
        // Benzersiz grup adresiyle çakışma olmasın
        let g = Ipv4Addr::new(239, 99, 99, 1);
        let iface = Ipv4Addr::new(192, 0, 2, 1);
        let mreq = IpMreq::new(g, iface);
        join_group(mreq);
        assert!(is_member(g, iface));
        leave_group(mreq);
        // leave_pending true, entry hâlâ tabloda
        let pending = pending_leaves();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].0, g);
        // Leave gönderildi → entry silinir
        mark_leave_sent(g, iface);
        assert!(!is_member(g, iface));
    }
}

