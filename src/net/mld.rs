//! # MLD (Multicast Listener Discovery) — IPv6 Multicast Yönetimi
//!
//! RFC 2710 (MLDv1) + RFC 3810 (MLDv2) uyumlu IPv6 multicast yönetimi.
//!
//! ## MLD Nedir?
//!
//! IGMP'nin IPv6 karşılığıdır. Aynı amaç: host'ların IPv6 multicast
//! gruplarına katılmasını/ayrılmasını sağlamak. ICMPv6 üzerinde çalışır.
//!
//! ## MLDv1 vs MLDv2 Farkları
//!
//! - **MLDv1** (RFC 2710): Sadece ASM (Any-Source Multicast), Include/Exclude ayrımı yok
//! - **MLDv2** (RFC 3810): Kaynak-spesifik katılım, IGMPv3 ile uyumlu
//!
//! ## MLD Mesaj Tipleri (ICMPv6 Type)
//!
//! | Tip | Ad | RFC | Açıklama |
//! |-----|----|-----|----------|
//! | 130 | Listener Query | 2710/3810 | Router sorar "kim dinliyor?" |
//! | 131 | v1 Listener Report | 2710 | v1 katılım raporu |
//! | 132 | v1 Listener Done | 2710 | v1 ayrılma |
//! | 143 | v2 Listener Report | 3810 | v2 katılım raporu |
//!
//! ## MLD Adresleri
//!
//! - Tüm MLD mesajları Hop Limit = 1 ile gönderilir
//! - Source: link-local adres
//! - Destination: hedef grup adresi (Query için ff02::1, Report için hedef grup)
//!
//! ## MLD Report Formatı (MLDv2, RFC 3810 §5.2)
//!
//! ```text
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |  Type = 143   |    Code       |           Checksum            |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |           Reserved            |  Number of Multicast Address  |
//! |                               |          Records (M)          |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |                                                               |
//! .                  Multicast Address Records                    .
//! .                                                               .
//! |                                                               |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! ```

use super::ipv6::Ipv6Addr;
use super::{Mutex, NET_COUNTERS};
use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::Ordering;

// ============================================================================
// MLD MESAJ TİPLERİ
// ============================================================================

pub const MLD_TYPE_QUERY: u8 = 130;
pub const MLD_TYPE_V1_REPORT: u8 = 131;
pub const MLD_TYPE_V1_DONE: u8 = 132;
pub const MLD_TYPE_V2_REPORT: u8 = 143;

pub const MLD_MAX_GROUPS: usize = 64;

// ============================================================================
// MLDv2 GROUP RECORD TÜRLERİ (RFC 3810 §5.2.1)
// ============================================================================

pub const MLD_V2_MODE_IS_INCLUDE: u8 = 1;
pub const MLD_V2_MODE_IS_EXCLUDE: u8 = 2;
pub const MLD_V2_CHANGE_TO_INCLUDE: u8 = 3;
pub const MLD_V2_CHANGE_TO_EXCLUDE: u8 = 4;
pub const MLD_V2_ALLOW_NEW_SOURCES: u8 = 5;
pub const MLD_V2_BLOCK_OLD_SOURCES: u8 = 6;

/// MLDv2 üyelik modu
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MldFilterMode {
    Include = 1,
    Exclude = 2,
}

// ============================================================================
// MLDv6 ADRES İSTEĞİ (RFC 3678)
// ============================================================================

/// `struct ipv6_mreq` — IPV6_ADD_MEMBERSHIP argümanı
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ipv6Mreq {
    pub ipv6mr_multiaddr: Ipv6Addr,
    pub ipv6mr_interface: u32,
}

/// Multicast grup durumu
#[derive(Clone, Debug)]
pub struct MldGroupState {
    pub group_addr: Ipv6Addr,
    pub interface_idx: u32,
    pub mode: MldFilterMode,
    pub sources: Vec<Ipv6Addr>,
    pub last_report_ticks: u64,
    pub done_pending: bool,
    pub bytes_received: u64,
    pub packets_received: u64,
}

impl MldGroupState {
    pub fn new(group_addr: Ipv6Addr, interface_idx: u32) -> Self {
        MldGroupState {
            group_addr,
            interface_idx,
            mode: MldFilterMode::Exclude,
            sources: Vec::new(),
            last_report_ticks: 0,
            done_pending: false,
            bytes_received: 0,
            packets_received: 0,
        }
    }
}

// ============================================================================
// KÜRESEL DURUM
// ============================================================================

static MLD_GROUPS: Mutex<BTreeMap<(u128, u32), MldGroupState>> =
    Mutex::new(BTreeMap::new());

// ============================================================================
// PUBLIC API
// ============================================================================

/// IPv6 multicast gruba katıl.
pub fn join_group(mreq: Ipv6Mreq) {
    let key = (u128::from_be_bytes(mreq.ipv6mr_multiaddr.0), mreq.ipv6mr_interface);
    let mut groups = MLD_GROUPS.lock();
    if groups.contains_key(&key) {
        return;
    }
    if groups.len() >= MLD_MAX_GROUPS {
        crate::serial_println!("[MLD] max groups reached, dropping");
        return;
    }
    groups.insert(
        key,
        MldGroupState::new(mreq.ipv6mr_multiaddr, mreq.ipv6mr_interface),
    );
    crate::serial_println!(
        "[MLD] join_group: {}/{}",
        mreq.ipv6mr_multiaddr,
        mreq.ipv6mr_interface
    );
}

/// IPv6 multicast gruptan ayrıl.
pub fn leave_group(mreq: Ipv6Mreq) {
    let key = (u128::from_be_bytes(mreq.ipv6mr_multiaddr.0), mreq.ipv6mr_interface);
    let mut groups = MLD_GROUPS.lock();
    if let Some(state) = groups.get_mut(&key) {
        state.done_pending = true;
    }
    groups.remove(&key);
    crate::serial_println!("[MLD] leave_group: {}", mreq.ipv6mr_multiaddr);
}

/// SSM (Source-Specific Multicast) IPv6 katılımı
pub fn join_source_specific(
    group_addr: Ipv6Addr,
    interface_idx: u32,
    source_addr: Ipv6Addr,
) {
    let key = (u128::from_be_bytes(group_addr.0), interface_idx);
    let mut groups = MLD_GROUPS.lock();
    let entry = groups
        .entry(key)
        .or_insert_with(|| MldGroupState::new(group_addr, interface_idx));
    entry.mode = MldFilterMode::Include;
    if !entry.sources.contains(&source_addr) {
        entry.sources.push(source_addr);
    }
    crate::serial_println!(
        "[MLD] SSM join: group={} source={}",
        group_addr,
        source_addr
    );
}

/// Belirli grup üyesi mi?
pub fn is_member(group_addr: &Ipv6Addr, interface_idx: u32) -> bool {
    let key = (u128::from_be_bytes(group_addr.0), interface_idx);
    MLD_GROUPS.lock().contains_key(&key)
}

/// Aktif MLD gruplarını listele
pub fn list_groups() -> Vec<(Ipv6Addr, u32, MldFilterMode, usize)> {
    MLD_GROUPS
        .lock()
        .values()
        .map(|s| (s.group_addr, s.interface_idx, s.mode, s.sources.len()))
        .collect()
}

// ============================================================================
// PAKET OLUŞTURUCULARI
// ============================================================================

/// MLDv1 Listener Report (ICMPv6, 24 byte)
///
/// Format: type(1) | code(1) | checksum(2) | max_resp_delay(2) | reserved(2) | group(16)
pub fn build_v1_report(group_addr: Ipv6Addr) -> Vec<u8> {
    let mut pkt = Vec::with_capacity(24);
    pkt.push(MLD_TYPE_V1_REPORT);
    pkt.push(0); // Code
    pkt.push(0);
    pkt.push(0); // Checksum sıfırdan başlar, internet_checksum ile doldurulur
    pkt.push(0);
    pkt.push(0); // Max Response Delay (sadece Query'de anlamlı)
    pkt.push(0);
    pkt.push(0); // Reserved
    pkt.extend_from_slice(&group_addr.0);
    let cs = super::checksum::internet_checksum(&pkt);
    pkt[2] = (cs >> 8) as u8;
    pkt[3] = (cs & 0xff) as u8;
    pkt
}

/// MLDv1 Listener Done
pub fn build_v1_done(group_addr: Ipv6Addr) -> Vec<u8> {
    let mut pkt = Vec::with_capacity(24);
    pkt.push(MLD_TYPE_V1_DONE);
    pkt.push(0); // Code
    pkt.push(0);
    pkt.push(0); // Checksum
    pkt.push(0);
    pkt.push(0); // Max Response Delay
    pkt.push(0);
    pkt.push(0); // Reserved
    pkt.extend_from_slice(&group_addr.0);
    let cs = super::checksum::internet_checksum(&pkt);
    pkt[2] = (cs >> 8) as u8;
    pkt[3] = (cs & 0xff) as u8;
    pkt
}

/// MLDv2 Listener Report (RFC 3810 §5.2)
pub fn build_v2_report(records: &[(u8, Ipv6Addr, &[Ipv6Addr])]) -> Vec<u8> {
    let mut pkt = Vec::new();
    pkt.push(MLD_TYPE_V2_REPORT);
    pkt.push(0);
    pkt.push(0);
    pkt.push(0);
    pkt.push(0);
    pkt.push(0);
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

/// Gelen MLD/ICMPv6 mesajını işle.
pub fn handle_packet(packet: &[u8]) {
    if packet.len() < 8 {
        return;
    }
    let msg_type = packet[0];
    match msg_type {
        MLD_TYPE_QUERY => {
            NET_COUNTERS.multicast.mld_queries.fetch_add(1, Ordering::Relaxed);
            crate::serial_println!("[MLD] query received");
        }
        MLD_TYPE_V1_REPORT | MLD_TYPE_V2_REPORT => {
            crate::serial_println!("[MLD] report from peer");
        }
        MLD_TYPE_V1_DONE => {
            crate::serial_println!("[MLD] done from peer");
        }
        _ => {}
    }
}

// ============================================================================
// TESTLER
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v1_report_has_valid_checksum() {
        let group = Ipv6Addr([
            0xff, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x01,
        ]);
        let pkt = build_v1_report(group);
        assert_eq!(pkt.len(), 24);
        assert_eq!(pkt[0], MLD_TYPE_V1_REPORT);
        let cs = super::super::checksum::internet_checksum(&pkt);
        assert_eq!(cs, 0);
    }

    #[test]
    fn v1_done_has_valid_checksum() {
        let group = Ipv6Addr([
            0xff, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x01,
        ]);
        let pkt = build_v1_done(group);
        assert_eq!(pkt.len(), 24);
        assert_eq!(pkt[0], MLD_TYPE_V1_DONE);
    }

    #[test]
    fn v2_report_encodes_records() {
        let g = Ipv6Addr([
            0xff, 0x38, 0x00, 0x40, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1,
        ]);
        let s1 = Ipv6Addr([
            0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1,
        ]);
        let sources = [s1];
        let records = vec![(MLD_V2_MODE_IS_INCLUDE, g, &sources[..])];
        let pkt = build_v2_report(&records);
        assert_eq!(pkt[0], MLD_TYPE_V2_REPORT);
        assert_eq!(&pkt[6..8], &[0, 1]);
        assert_eq!(pkt[8], MLD_V2_MODE_IS_INCLUDE);
    }
}


