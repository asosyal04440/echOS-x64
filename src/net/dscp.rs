//! # DSCP / ToS Marking — Paket Sınıflandırma ve Yeniden İşaretleme
//!
//! ## ToS (Type of Service) Nedir?
//!
//! IPv4 başlığında 8-bit `Type of Service` alanı bulunur. Bu alan paketin
//! ağda nasıl muamele göreceğini belirler.
//!
//! ## ToS Bayt Formatı (RFC 1349)
//!
//! ```text
//!   0   1   2   3   4   5   6   7
//! +---+---+---+---+---+---+---+---+
//! |   Precedence  |  TOS  |   MBZ |
//! +---+---+---+---+---+---+---+---+
//! ```
//!
//! - **Precedence (3 bit)**: 0 (normal) - 7 (network control)
//! - **TOS (4 bit)**: minimize delay, maximize throughput, maximize reliability, minimize cost
//! - **MBZ (1 bit)**: Must Be Zero (eski), sonra ECN için kullanıldı
//!
//! ## DSCP (Differentiated Services Code Point, RFC 2474)
//!
//! ToS alanının ilk 6 bit'i DSCP olarak yorumlanır. Kalan 2 bit ECN.
//!
//! ```text
//!   0   1   2   3   4   5   6   7
//! +---+---+---+---+---+---+---+---+
//! |          DSCP         |  ECN  |
//! +---+---+---+---+---+---+---+---+
//! ```
//!
//! ## Yaygın DSCP Değerleri
//!
//! | Sınıf | DSCP | Decimal | Hex | Kullanım |
//! |-------|------|---------|-----|----------|
//! | BE (Best Effort) | 0 | 0x00 | Default |
//! | AF11 | 10 | 0x0A | Multimedia streaming |
//! | AF21 | 18 | 0x12 | Low-latency data |
//! | AF31 | 26 | 0x1A | Broadcast video |
//! | AF41 | 34 | 0x22 | Multimedia conferencing |
//! | EF (Expedited Forwarding) | 46 | 0x2E | VoIP, gerçek zamanlı |
//! | CS6 | 48 | 0x30 | Network control |
//! | CS7 | 56 | 0x38 | Network control (high) |
//!
//! ## echOS Tasarımı
//!
//! `DscpMark` enum'u ile sık kullanılan sınıflar.
//! `tos_to_dscp(tos)` ve `dscp_to_tos(dscp)` dönüşümleri.
//! `rewrite_ipv4_tos` paket üzerinde ToS byte'ını değiştirir.

use super::NetError;
#[cfg(test)]
use alloc::vec;

// ============================================================================
// DSCP SABİTLERİ
// ============================================================================

pub const DSCP_OFFSET_IN_TOS: u8 = 2; // DSCP TOS byte'ının üst 6 bit'i
pub const ECN_MASK: u8 = 0x03;

/// Yaygın DSCP değerleri (RFC 2474, RFC 2597, RFC 3246)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum DscpMark {
    BestEffort = 0,
    Cs1 = 8,    // Scavenger
    Af11 = 10,
    Af12 = 12,
    Af13 = 14,
    Cs2 = 16,
    Af21 = 18,
    Af22 = 20,
    Af23 = 22,
    Cs3 = 24,
    Af31 = 26,
    Af32 = 28,
    Af33 = 30,
    Cs4 = 32,
    Af41 = 34,
    Af42 = 36,
    Af43 = 38,
    Cs5 = 40,
    VoiceAdmit = 44,
    ExpeditedForwarding = 46, // EF — VoIP
    Cs6 = 48,
    Cs7 = 56,
}

impl DscpMark {
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => DscpMark::BestEffort,
            8 => DscpMark::Cs1,
            10 => DscpMark::Af11,
            12 => DscpMark::Af12,
            14 => DscpMark::Af13,
            16 => DscpMark::Cs2,
            18 => DscpMark::Af21,
            20 => DscpMark::Af22,
            22 => DscpMark::Af23,
            24 => DscpMark::Cs3,
            26 => DscpMark::Af31,
            28 => DscpMark::Af32,
            30 => DscpMark::Af33,
            32 => DscpMark::Cs4,
            34 => DscpMark::Af41,
            36 => DscpMark::Af42,
            38 => DscpMark::Af43,
            40 => DscpMark::Cs5,
            44 => DscpMark::VoiceAdmit,
            46 => DscpMark::ExpeditedForwarding,
            48 => DscpMark::Cs6,
            56 => DscpMark::Cs7,
            _ => DscpMark::BestEffort,
        }
    }
}

// ============================================================================
// ToS ESKİ ALANLARI
// ============================================================================

/// ToS "Precedence" değerleri (RFC 791, eski)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Precedence {
    Routine = 0,
    Priority = 1,
    Immediate = 2,
    Flash = 3,
    FlashOverride = 4,
    CriticEcp = 5,
    InternetControl = 6,
    NetworkControl = 7,
}

/// ToS "Type" bit alanları (RFC 1349)
#[derive(Clone, Copy, Debug)]
pub struct TosFlags {
    pub minimize_delay: bool,
    pub maximize_throughput: bool,
    pub maximize_reliability: bool,
    pub minimize_cost: bool,
}

impl TosFlags {
    pub fn to_u8(&self) -> u8 {
        let mut v = 0u8;
        if self.minimize_delay {
            v |= 0x10;
        }
        if self.maximize_throughput {
            v |= 0x08;
        }
        if self.maximize_reliability {
            v |= 0x04;
        }
        if self.minimize_cost {
            v |= 0x02;
        }
        v
    }
}

// ============================================================================
// DÖNÜŞÜM FONKSİYONLARI
// ============================================================================

/// IPv4 ToS byte'ından DSCP değerini çıkar (üst 6 bit)
pub fn tos_to_dscp(tos: u8) -> u8 {
    tos >> DSCP_OFFSET_IN_TOS
}

/// DSCP değerini IPv4 ToS byte'ına yerleştir (ECN korunur)
pub fn dscp_to_tos(dscp: u8, ecn: u8) -> u8 {
    (dscp << DSCP_OFFSET_IN_TOS) | (ecn & ECN_MASK)
}

/// Paketin IPv4 ToS alanını değiştir (RFC 791 byte 1, "Type of Service")
///
/// `packet` IPv4 paket içerir (en az 20 byte header)
/// `dscp` yeni DSCP değeri
pub fn rewrite_ipv4_tos(packet: &mut [u8], dscp: DscpMark) -> Result<u8, NetError> {
    if packet.len() < 2 {
        return Err(NetError::InvalidPacket);
    }
    // IPv4: version/ihl (0), ToS (1), total length high (2)
    let old_tos = packet[1];
    let ecn = old_tos & ECN_MASK;
    let new_tos = dscp_to_tos(dscp.as_u8(), ecn);
    packet[1] = new_tos;
    // IP header checksum'ı RFC 1624 incremental formülle güncelle
    // (bytes 10-11). HC' = ~(~HC + ~m + m') ones-complement aritmetiğinde.
    // IP word containing ToS: offset 0 = bytes [version+IHL, ToS].
    // ToS, ilk 16-bit word'ün alt byte'ı; üst byte version+IHL (değişmez).
    if packet.len() >= 20 {
        let old_csum = u16::from_be_bytes([packet[10], packet[11]]);
        let old_word = u16::from_be_bytes([packet[0], old_tos]);
        let new_word = u16::from_be_bytes([packet[0], new_tos]);
        let mut sum = (!old_csum as u32)
            .wrapping_add((!old_word) as u32)
            .wrapping_add(new_word as u32);
        // Carry fold (ones-complement 16-bit wrap)
        while sum > 0xFFFF {
            sum = (sum & 0xFFFF) + (sum >> 16);
        }
        let csum = (!sum) as u16;
        packet[10] = (csum >> 8) as u8;
        packet[11] = (csum & 0xFF) as u8;
    }
    Ok(old_tos)
}

/// Sınıflandırma: Paket için DSCP öner
///
/// Kurallar:
/// - Dst port 5060 (SIP), 5061 (SIPS) → EF (VoIP)
/// - Dst port 443 (HTTPS) → AF21 (interactive)
/// - Dst port 80, 8080 → AF11 (web)
/// - Dst port 53 (DNS) → CS5 (önemli)
/// - Diğer → BestEffort
pub fn classify_tcp_port(dst_port: u16) -> DscpMark {
    match dst_port {
        5060 | 5061 => DscpMark::ExpeditedForwarding, // VoIP
        53 => DscpMark::Cs5,                          // DNS
        443 => DscpMark::Af21,                        // HTTPS
        80 | 8080 => DscpMark::Af11,                  // HTTP
        22 => DscpMark::Cs6,                          // SSH
        25 | 465 | 587 => DscpMark::Af31,             // SMTP
        _ => DscpMark::BestEffort,
    }
}

// ============================================================================
// TESTLER
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tos_to_dscp_extracts_upper_six_bits() {
        // 0xB8 = 1011_1000 → DSCP 0x2E (46) = 101110
        assert_eq!(tos_to_dscp(0xB8), 0x2E);
    }

    #[test]
    fn dscp_to_tos_preserves_ecn() {
        // DSCP 46 + ECN 3 → 0xBB
        assert_eq!(dscp_to_tos(46, 3), 0xBB);
    }

    #[test]
    fn ef_dscp_value_is_46() {
        assert_eq!(DscpMark::ExpeditedForwarding.as_u8(), 46);
    }

    #[test]
    fn dscp_mark_from_u8_round_trip() {
        for v in [0u8, 8, 10, 18, 26, 34, 46, 48, 56] {
            assert_eq!(DscpMark::from_u8(v).as_u8(), v);
        }
    }

    #[test]
    fn classify_tcp_port_voip() {
        assert_eq!(classify_tcp_port(5060), DscpMark::ExpeditedForwarding);
    }

    #[test]
    fn rewrite_tos_updates_checksum() {
        // IPv4 header (no options) with ToS 0, csum 0x1234 (artificially)
        let mut pkt = vec![
            0x45, // version + ihl
            0x00, // ToS = 0
            0x00, 0x3C, // total length
            0x00, 0x01, // identification
            0x40, 0x00, // flags + frag
            0x40, // TTL
            0x06, // protocol (TCP)
            0x12, 0x34, // checksum
            // src IP, dst IP...
            0x0A, 0x00, 0x00, 0x01,
            0x0A, 0x00, 0x00, 0x02,
        ];
        let old_tos = rewrite_ipv4_tos(&mut pkt, DscpMark::ExpeditedForwarding).unwrap();
        assert_eq!(old_tos, 0);
        // Yeni ToS: DSCP 46 (0x2E) shifted = 0xB8
        assert_eq!(pkt[1], 0xB8);
        // Checksum güncellenmiş olmalı (yeni değer 0x1234 değil)
        let new_csum = u16::from_be_bytes([pkt[10], pkt[11]]);
        assert_ne!(new_csum, 0x1234);
    }

    #[test]
    fn tos_flags_compose_correctly() {
        let f = TosFlags {
            minimize_delay: true,
            maximize_throughput: false,
            maximize_reliability: true,
            minimize_cost: false,
        };
        // 0x10 (min-delay) | 0x04 (max-reliability) = 0x14
        assert_eq!(f.to_u8(), 0x14);
    }

    #[test]
    fn rewrite_tos_yields_valid_header_checksum() {
        // Geçerli bir IPv4 header kur: ToS=0, checksum=doğru hesaplanmış
        let mut pkt = vec![
            0x45,       // version + ihl
            0x00,       // ToS = 0
            0x00, 0x3C, // total length
            0x00, 0x01, // identification
            0x40, 0x00, // flags + frag
            0x40,       // TTL
            0x06,       // protocol (TCP)
            0x00, 0x00, // checksum (henüz hesaplanmamış)
            0x0A, 0x00, 0x00, 0x01,
            0x0A, 0x00, 0x00, 0x02,
        ];
        // Doğru checksum'ı hesapla (16-bit one'ın complement sum of all words)
        let mut sum: u32 = 0;
        for w in pkt.chunks(2) {
            sum += u16::from_be_bytes([w[0], w[1]]) as u32;
        }
        while sum > 0xFFFF {
            sum = (sum & 0xFFFF) + (sum >> 16);
        }
        let csum = (!sum) as u16;
        pkt[10] = (csum >> 8) as u8;
        pkt[11] = (csum & 0xFF) as u8;

        // ToS'u değiştir: EF (DSCP 46)
        rewrite_ipv4_tos(&mut pkt, DscpMark::ExpeditedForwarding).unwrap();
        assert_eq!(pkt[1], 0xB8); // DSCP 46 << 2 = 0xB8

        // Yeni checksum doğru olmalı: tüm header'ın ones-complement toplamı 0
        let mut verify: u32 = 0;
        for w in pkt[..20].chunks(2) {
            verify += u16::from_be_bytes([w[0], w[1]]) as u32;
        }
        while verify > 0xFFFF {
            verify = (verify & 0xFFFF) + (verify >> 16);
        }
        // verify == 0xFFFF (ones-complement sum of valid checksummed packet = -0 = 0xFFFF)
        assert_eq!(verify, 0xFFFF, "checksum doğru hesaplanmamış");
    }
}

