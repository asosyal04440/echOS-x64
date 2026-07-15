//! # Checksum Offload Framework
//!
//! Internet checksum (RFC 1071) hesaplamalarını donanım veya optimize edilmiş
//! yazılım katmanına deleg eder. Her pakette CPU ile checksum hesaplamak yerine
//! NIC'e offload eder.
//!
//! ## Checksum Türleri
//!
//! | Tür              | Açıklama                                  |
//! |------------------|-------------------------------------------|
//! | `CHECKSUM_NONE`  | Donanım offload yok, SW hesaplamalı       |
//! | `CHECKSUM_PARTIAL`| HW/SW partial, tamamlayıcı gerekli       |
//! | `CHECKSUM_COMPLETE`| Donanım tam checksum hesapladı          |
//! | `CHECKSUM_UNNECESSARY`| Donanım doğruladı, kontrol gerekmez |
//!
//! ## IP/TCP/UDP Checksum Offset'leri
//!
//! ```text
//!  Ethernet(14) | IPv4(20) | TCP(20+) | Payload
//!       │           │          │
//!       │           ├── IP checksum: offset 24-25 (IPv4 header)
//!       │           └── (yok IPv6'da)
//!       └── TCP checksum: offset 50-51 (TCP header, IPv4)
//!       └── UDP checksum: offset 40-41 (UDP header, IPv4)
//! ```
//!
//! Kaynak: RFC 1071, Linux kernel sk_buff csum field'ları

use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use super::ipv6::Ipv6Addr;

// ============================================================================
// CHECKSUM TYPES
// ============================================================================

/// Checksum hesaplama durumu
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChecksumMode {
    /// Donanım offload yok — tam yazılım hesaplaması gerekli
    None,
    
    /// Kısmi checksum hesaplandı (başlangıç değeri + data),
    /// tamamlayıcı (pseudo-header) eklenmeli
    Partial {
        /// Checksum başlangıç offset'i (packet başından)
        csum_offset: u16,
        /// Checksum'un hesaplanacağı data başlangıç offset'i
        start_offset: u16,
    },
    
    /// Donanım tam checksum hesapladı — sadece doğrulama gerekli
    Complete {
        /// Hesaplanmış checksum değeri
        csum_value: u32,
    },
    
    /// Donanım doğruladı — kontrol gerekmez (RX path)
    Unnecessary,
}

// ============================================================================
// CHECKSUM STATISTICS
// ============================================================================

pub struct ChecksumStats {
    pub tx_computed: AtomicU64,
    pub tx_offloaded: AtomicU64,
    pub rx_verified: AtomicU64,
    pub rx_offloaded: AtomicU64,
    pub errors: AtomicU64,
}

impl ChecksumStats {
    pub const fn new() -> Self {
        ChecksumStats {
            tx_computed: AtomicU64::new(0),
            tx_offloaded: AtomicU64::new(0),
            rx_verified: AtomicU64::new(0),
            rx_offloaded: AtomicU64::new(0),
            errors: AtomicU64::new(0),
        }
    }
}

pub static CKSUM_STATS: ChecksumStats = ChecksumStats::new();

// ============================================================================
// INTERNET CHECKSUM (RFC 1071)
// ============================================================================

/// RFC 1071 Internet Checksum hesaplama
///
/// 16-bit one's complement sum'ın one's complement'i.
///
/// # Algoritma
///
/// 1. Data'yı 16-bit word'lere böl
/// 2. Tüm word'leri 32-bit accumulator'da topla
/// 3. Carry'leri ekle (fold)
/// 4. One's complement al (~sum)
pub fn internet_checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut i = 0;
    let len = data.len();
    
    // 16-bit word'leri topla
    while i + 1 < len {
        let word = u16::from_be_bytes([data[i], data[i + 1]]) as u32;
        sum = sum.wrapping_add(word);
        i += 2;
    }
    
    // Tek byte varsa ekle (padding 0)
    if i < len {
        sum = sum.wrapping_add((data[i] as u32) << 8);
    }
    
    // Carry fold
    while (sum >> 16) != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    
    !(sum as u16)
}

/// Incremental checksum update (RFC 1624)
///
/// Header alanı değiştiğinde full recalculate yerine incremental update:
/// `new_csum = ~(~old_csum + ~old_value + new_value)`
pub fn update_checksum(old_csum: u16, old_value: u16, new_value: u16) -> u16 {
    let mut sum: u32 = (!old_csum as u32)
        .wrapping_add(!old_value as u32)
        .wrapping_add(new_value as u32);
    
    while (sum >> 16) != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    
    !(sum as u16)
}

/// Partial checksum — sadece belirtilen aralık için hesapla
pub fn partial_checksum(data: &[u8], start: usize, end: usize) -> u32 {
    let mut sum: u32 = 0;
    let mut i = start;
    
    while i + 1 < end && i + 1 < data.len() {
        let word = u16::from_be_bytes([data[i], data[i + 1]]) as u32;
        sum = sum.wrapping_add(word);
        i += 2;
    }
    
    if i < end && i < data.len() {
        sum = sum.wrapping_add((data[i] as u32) << 8);
    }
    
    sum
}

// ============================================================================
// PSEUDO-HEADER CHECKSUM
// ============================================================================

/// IPv4 pseudo-header checksum (TCP/UDP için)
///
/// ```text
///  +--------+--------+--------+--------+
///  |           Source Address           |
///  +--------+--------+--------+--------+
///  |         Destination Address        |
///  +--------+--------+--------+--------+
///  |  Zero  |Protocol|   TCP Length     |
///  +--------+--------+--------+--------+
/// ```
pub fn ipv4_pseudo_header_checksum(
    src_ip: &[u8; 4],
    dst_ip: &[u8; 4],
    protocol: u8,
    transport_len: u16,
) -> u32 {
    let mut sum: u32 = 0;
    
    sum = sum.wrapping_add(u16::from_be_bytes([src_ip[0], src_ip[1]]) as u32);
    sum = sum.wrapping_add(u16::from_be_bytes([src_ip[2], src_ip[3]]) as u32);
    sum = sum.wrapping_add(u16::from_be_bytes([dst_ip[0], dst_ip[1]]) as u32);
    sum = sum.wrapping_add(u16::from_be_bytes([dst_ip[2], dst_ip[3]]) as u32);
    sum = sum.wrapping_add(protocol as u32);
    sum = sum.wrapping_add(transport_len as u32);
    
    sum
}

/// IPv6 pseudo-header checksum (TCP/UDP için)
pub fn ipv6_pseudo_header_checksum(
    src_ip: &[u8; 16],
    dst_ip: &[u8; 16],
    next_header: u8,
    transport_len: u32,
) -> u32 {
    let mut sum: u32 = 0;
    
    // Source address (8 x 16-bit)
    for i in (0..16).step_by(2) {
        sum = sum.wrapping_add(u16::from_be_bytes([src_ip[i], src_ip[i + 1]]) as u32);
    }
    
    // Destination address
    for i in (0..16).step_by(2) {
        sum = sum.wrapping_add(u16::from_be_bytes([dst_ip[i], dst_ip[i + 1]]) as u32);
    }
    
    // Upper-layer packet length (32-bit)
    sum = sum.wrapping_add((transport_len >> 16) as u32);
    sum = sum.wrapping_add((transport_len & 0xFFFF) as u32);
    
    // Next header
    sum = sum.wrapping_add(next_header as u32);
    
    sum
}

/// Fold 32-bit sum into 16-bit checksum (compute için — complement dahil)
pub fn fold_checksum(sum: u32) -> u16 {
    let mut s = sum;
    while (s >> 16) != 0 {
        s = (s & 0xFFFF) + (s >> 16);
    }
    !(s as u16)
}

/// Fold 32-bit sum into 16-bit without complement (verify için)
///
/// Doğrulama kuralı: geçerli paket için unfolded_sum == 0xFFFF
fn unfold_checksum(sum: u32) -> u16 {
    let mut s = sum;
    while (s >> 16) != 0 {
        s = (s & 0xFFFF) + (s >> 16);
    }
    s as u16
}

// ============================================================================
// LCO: LOCAL CHECKSUM OFFLOAD (Tunnel encapsulation)
// ============================================================================

/// Local Checksum Offload (LCO) — tunnel encapsulation için
///
/// Arşiv (000679 checksum-offloads): LCO, inner checksum offload edildiğinde
/// outer checksum'u hesaplamanın verimli yoludur:
///
/// Linux `lco_csum()` ile aynı semantiği uygular:
/// 1. 0..csum_start aralığındaki tüm byte'ları topla (outer headers)
/// 2. csum_start + csum_offset'teki 16-bit word'ün complement'ini ekle
/// 3. Fold et → outer checksum
///
/// `csum_offset` negatif olabilir (outer UDP checksum field inner packet
/// start'ın gerisinde kaldığında, ör. VXLAN: csum_start=42, offset=-2).
///
/// Bu, inner payload'a bakmadan outer checksum hesaplamayı sağlar.
/// Kullanım: VXLAN, GENEVE, GRE tunnel header checksum.
pub fn lco_checksum(
    packet: &[u8],
    csum_start: usize,
    csum_offset: isize,
) -> u16 {
    let mut sum: u32 = 0;

    // 1. 0..csum_start aralığındaki header'ları topla (big-endian 16-bit words)
    let mut i = 0;
    while i + 1 < csum_start && i + 1 < packet.len() {
        let word = u16::from_be_bytes([packet[i], packet[i + 1]]) as u32;
        sum = sum.wrapping_add(word);
        i += 2;
    }

    // 2. csum_start + csum_offset'teki word'ün complement'ini ekle
    let field_offset = (csum_start as isize + csum_offset) as usize;
    if field_offset + 1 < packet.len() {
        let word = u16::from_be_bytes(
            [packet[field_offset], packet[field_offset + 1]]
        ) as u32;
        sum = sum.wrapping_add((!word) & 0xFFFF); // complement (16-bit)
    }

    fold_checksum(sum)
}

// ============================================================================
// TCP/UDP CHECKSUM COMPUTE
// ============================================================================

/// IPv4 TCP paketi için checksum hesapla ve yaz
///
/// `packet`: Ethernet + IPv4 + TCP + payload
/// Başarılı olursa true döner.
pub fn compute_ipv4_tcp_checksum(packet: &mut [u8]) -> bool {
    if packet.len() < 54 {
        return false;
    }
    
    let ihl = (packet[14] & 0x0F) as usize * 4;
    let tcp_offset = 14 + ihl;
    
    if packet.len() < tcp_offset + 20 {
        return false;
    }
    
    // IP checksum hesapla
    packet[24] = 0;
    packet[25] = 0;
    let ip_csum = internet_checksum(&packet[14..14 + ihl]);
    packet[24] = (ip_csum >> 8) as u8;
    packet[25] = (ip_csum & 0xFF) as u8;
    
    // TCP checksum hesapla (pseudo-header + TCP + payload)
    let src_ip = [packet[26], packet[27], packet[28], packet[29]];
    let dst_ip = [packet[30], packet[31], packet[32], packet[33]];
    let tcp_len = (packet.len() - tcp_offset) as u16;
    
    // TCP checksum field'ını sıfırla
    let csum_offset = tcp_offset + 16;
    packet[csum_offset] = 0;
    packet[csum_offset + 1] = 0;
    
    // Pseudo-header sum
    let pseudo_sum = ipv4_pseudo_header_checksum(&src_ip, &dst_ip, 6, tcp_len);
    
    // TCP + payload sum
    let tcp_sum = partial_checksum(packet, tcp_offset, packet.len());
    
    // Toplam
    let total = pseudo_sum.wrapping_add(tcp_sum);
    let csum = fold_checksum(total);
    
    packet[csum_offset] = (csum >> 8) as u8;
    packet[csum_offset + 1] = (csum & 0xFF) as u8;
    
    CKSUM_STATS.tx_computed.fetch_add(1, Ordering::Relaxed);
    true
}

/// IPv4 UDP paketi için checksum hesapla ve yaz
pub fn compute_ipv4_udp_checksum(packet: &mut [u8]) -> bool {
    if packet.len() < 42 {
        return false;
    }
    
    let ihl = (packet[14] & 0x0F) as usize * 4;
    let udp_offset = 14 + ihl;
    
    if packet.len() < udp_offset + 8 {
        return false;
    }
    
    // IP checksum
    packet[24] = 0;
    packet[25] = 0;
    let ip_csum = internet_checksum(&packet[14..14 + ihl]);
    packet[24] = (ip_csum >> 8) as u8;
    packet[25] = (ip_csum & 0xFF) as u8;
    
    // UDP checksum
    let src_ip = [packet[26], packet[27], packet[28], packet[29]];
    let dst_ip = [packet[30], packet[31], packet[32], packet[33]];
    let udp_len = (packet.len() - udp_offset) as u16;
    
    let csum_offset = udp_offset + 6;
    packet[csum_offset] = 0;
    packet[csum_offset + 1] = 0;
    
    let pseudo_sum = ipv4_pseudo_header_checksum(&src_ip, &dst_ip, 17, udp_len);
    let udp_sum = partial_checksum(packet, udp_offset, packet.len());
    let total = pseudo_sum.wrapping_add(udp_sum);
    let csum = fold_checksum(total);
    
    // UDP checksum 0 ise 0xFFFF yap (RFC 768: 0 = checksum yok)
    let final_csum = if csum == 0 { 0xFFFF } else { csum };
    
    packet[csum_offset] = (final_csum >> 8) as u8;
    packet[csum_offset + 1] = (final_csum & 0xFF) as u8;
    
    CKSUM_STATS.tx_computed.fetch_add(1, Ordering::Relaxed);
    true
}

/// Gelen paketin checksum'ını doğrula
pub fn verify_ipv4_tcp_checksum(packet: &[u8]) -> bool {
    if packet.len() < 54 {
        return false;
    }
    
    let ihl = (packet[14] & 0x0F) as usize * 4;
    let tcp_offset = 14 + ihl;
    
    if packet.len() < tcp_offset + 20 {
        return false;
    }
    
    // IP checksum doğrula
    let ip_csum = internet_checksum(&packet[14..14 + ihl]);
    if ip_csum != 0 {
        CKSUM_STATS.errors.fetch_add(1, Ordering::Relaxed);
        return false;
    }
    
    // TCP checksum doğrula
    let src_ip = [packet[26], packet[27], packet[28], packet[29]];
    let dst_ip = [packet[30], packet[31], packet[32], packet[33]];
    let tcp_len = (packet.len() - tcp_offset) as u16;
    
    let pseudo_sum = ipv4_pseudo_header_checksum(&src_ip, &dst_ip, 6, tcp_len);
    let tcp_sum = partial_checksum(packet, tcp_offset, packet.len());
    let total = pseudo_sum.wrapping_add(tcp_sum);
    // RFC 1071: verify — complement yok, folded sum 0xFFFF olmalı
    let result = unfold_checksum(total);

    CKSUM_STATS.rx_verified.fetch_add(1, Ordering::Relaxed);

    result == 0xFFFF
}

// ============================================================================
// IPv6 TCP/UDP CHECKSUM COMPUTE + VERIFY
// ============================================================================

pub fn compute_ipv6_tcp_checksum(
    src: Ipv6Addr,
    dst: Ipv6Addr,
    tcp_header: &[u8],
    payload: &[u8],
) -> u16 {
    let tcp_len = (tcp_header.len() + payload.len()) as u32;
    let pseudo_sum = ipv6_pseudo_header_checksum(&src.0, &dst.0, 6, tcp_len);

    let mut sum = pseudo_sum;
    let mut i = 0;
    while i + 1 < tcp_header.len() {
        let word = u16::from_be_bytes([tcp_header[i], tcp_header[i + 1]]) as u32;
        sum = sum.wrapping_add(word);
        i += 2;
    }
    if i < tcp_header.len() {
        sum = sum.wrapping_add((tcp_header[i] as u32) << 8);
    }

    i = 0;
    while i + 1 < payload.len() {
        let word = u16::from_be_bytes([payload[i], payload[i + 1]]) as u32;
        sum = sum.wrapping_add(word);
        i += 2;
    }
    if i < payload.len() {
        sum = sum.wrapping_add((payload[i] as u32) << 8);
    }

    fold_checksum(sum)
}

pub fn compute_ipv6_udp_checksum(
    src: Ipv6Addr,
    dst: Ipv6Addr,
    udp_header: &[u8],
    payload: &[u8],
) -> u16 {
    let udp_len = (udp_header.len() + payload.len()) as u32;
    let pseudo_sum = ipv6_pseudo_header_checksum(&src.0, &dst.0, 17, udp_len);

    let mut sum = pseudo_sum;
    let mut i = 0;
    while i + 1 < udp_header.len() {
        let word = u16::from_be_bytes([udp_header[i], udp_header[i + 1]]) as u32;
        sum = sum.wrapping_add(word);
        i += 2;
    }
    if i < udp_header.len() {
        sum = sum.wrapping_add((udp_header[i] as u32) << 8);
    }

    i = 0;
    while i + 1 < payload.len() {
        let word = u16::from_be_bytes([payload[i], payload[i + 1]]) as u32;
        sum = sum.wrapping_add(word);
        i += 2;
    }
    if i < payload.len() {
        sum = sum.wrapping_add((payload[i] as u32) << 8);
    }

    let csum = fold_checksum(sum);
    if csum == 0 { 0xFFFF } else { csum }
}

pub fn verify_ipv6_tcp_checksum(
    src: Ipv6Addr,
    dst: Ipv6Addr,
    tcp_segment: &[u8],
) -> bool {
    if tcp_segment.len() < 20 {
        return false;
    }
    let data_offset = ((tcp_segment[12] >> 4) as usize) * 4;
    if tcp_segment.len() < data_offset {
        return false;
    }
    let tcp_header = &tcp_segment[..data_offset];
    let payload = &tcp_segment[data_offset..];
    let computed = compute_ipv6_tcp_checksum(src, dst, tcp_header, payload);
    let mut sum = computed as u32;
    let mut i = 0;
    while i + 1 < tcp_segment.len() {
        let word = u16::from_be_bytes([tcp_segment[i], tcp_segment[i + 1]]) as u32;
        sum = sum.wrapping_add(word);
        i += 2;
    }
    if i < tcp_segment.len() {
        sum = sum.wrapping_add((tcp_segment[i] as u32) << 8);
    }
    CKSUM_STATS.rx_verified.fetch_add(1, Ordering::Relaxed);
    unfold_checksum(sum) == 0xFFFF
}

pub fn verify_ipv6_udp_checksum(
    src: Ipv6Addr,
    dst: Ipv6Addr,
    udp_segment: &[u8],
) -> bool {
    if udp_segment.len() < 8 {
        return false;
    }
    let payload = &udp_segment[8..];
    let computed = compute_ipv6_udp_checksum(src, dst, &udp_segment[..8], payload);
    let mut sum = computed as u32;
    let mut i = 0;
    while i + 1 < udp_segment.len() {
        let word = u16::from_be_bytes([udp_segment[i], udp_segment[i + 1]]) as u32;
        sum = sum.wrapping_add(word);
        i += 2;
    }
    if i < udp_segment.len() {
        sum = sum.wrapping_add((udp_segment[i] as u32) << 8);
    }
    CKSUM_STATS.rx_verified.fetch_add(1, Ordering::Relaxed);
    unfold_checksum(sum) == 0xFFFF
}

// ============================================================================
// INIT
// ============================================================================

pub fn init() {
    crate::serial_println!("[CHECKSUM] Checksum offload framework initialized");
}

// ============================================================================
// TESTLER
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn internet_checksum_rfc1071() {
        // Basit test verisi
        let data = [0x00, 0x01, 0x00, 0x02];
        let csum = internet_checksum(&data);
        // 0x0001 + 0x0002 = 0x0003, ~0x0003 = 0xFFFC
        assert_eq!(csum, 0xFFFC);
    }
    
    #[test]
    fn internet_checksum_with_carry() {
        let data = [0xFF, 0xFF, 0x00, 0x01];
        let csum = internet_checksum(&data);
        // 0xFFFF + 0x0001 = 0x10000, fold = 0x0001, ~0x0001 = 0xFFFE
        assert_eq!(csum, 0xFFFE);
    }
    
    #[test]
    fn incremental_checksum_update() {
        let old_csum: u16 = 0x1234;
        let old_value: u16 = 0x0001;
        let new_value: u16 = 0x0002;
        
        let new_csum = update_checksum(old_csum, old_value, new_value);
        
        // new = ~(~old_csum + ~old + new)
        // ~0x1234 = 0xEDCB
        // ~0x0001 = 0xFFFE
        // 0xEDCB + 0xFFFE + 0x0002 = 0x1EDCB, fold = 0xEDCC
        // ~0xEDCC = 0x1233
        assert!(new_csum != old_csum);
    }
    
    #[test]
    fn fold_large_sum() {
        assert_eq!(fold_checksum(0x10000), 0xFFFE);
        assert_eq!(fold_checksum(0x00000), 0xFFFF);
    }

    #[test]
    fn unfold_checksum_returns_folded_without_complement() {
        // unfold(0x10000) = fold without ~ = 0x0001 (carry: 0+1)
        assert_eq!(unfold_checksum(0x10000), 0x0001);
        // unfold(0x00000) = 0x0000
        assert_eq!(unfold_checksum(0x00000), 0x0000);
        // unfold(0xFFFF) = 0xFFFF (no carry, no complement)
        assert_eq!(unfold_checksum(0xFFFF), 0xFFFF);
        // For valid packet: unfold(total_sum) == 0xFFFF
        // Example: data=0x0003, checksum=~0x0003=0xFFFC
        // total = 0x0003 + 0xFFFC = 0xFFFF → unfold = 0xFFFF ✓
        assert_eq!(unfold_checksum(0x0003u32.wrapping_add(0xFFFC)), 0xFFFF);
    }
    
    #[test]
    fn tcp_checksum_compute() {
        // Minimal TCP packet: Ethernet(14) + IPv4(20) + TCP(20) + payload(4)
        let mut packet = vec![0u8; 58];
        
        // Ethernet type = IPv4
        packet[12] = 0x08;
        packet[13] = 0x00;
        
        // IPv4 header
        packet[14] = 0x45;  // V4, IHL=5
        packet[16] = 0x00;  // Total length = 44
        packet[17] = 0x2C;
        packet[23] = 6;     // Protocol = TCP
        packet[26] = 192;   // Src IP
        packet[27] = 168;
        packet[28] = 1;
        packet[29] = 1;
        packet[30] = 192;   // Dst IP
        packet[31] = 168;
        packet[32] = 1;
        packet[33] = 2;
        
        // TCP header
        packet[34 + 12] = 0x50;  // Data offset = 5 (20 bytes)
        
        let result = compute_ipv4_tcp_checksum(&mut packet);
        assert!(result);

        // IP checksum doğrula: unfold = 0xFFFF
        let ihl = (packet[14] & 0x0F) as usize * 4;
        let ip_sum: u32 = (0..ihl).step_by(2)
            .map(|i| u16::from_be_bytes([packet[14 + i], packet[15 + i]]) as u32)
            .sum();
        assert_eq!(unfold_checksum(ip_sum), 0xFFFF);
    }
    
    #[test]
    fn verify_checksum_roundtrip() {
        let mut packet = vec![0u8; 58];
        packet[12] = 0x08;
        packet[13] = 0x00;
        packet[14] = 0x45;
        packet[16] = 0x00;
        packet[17] = 0x2C;
        packet[23] = 6;
        packet[26] = 10; packet[27] = 0; packet[28] = 0; packet[29] = 1;
        packet[30] = 10; packet[31] = 0; packet[32] = 0; packet[33] = 2;
        packet[46] = 0x50;
        
        compute_ipv4_tcp_checksum(&mut packet);
        let valid = verify_ipv4_tcp_checksum(&packet);
        assert!(valid);
    }
    
    #[test]
    fn checksum_mode_variants() {
        let none = ChecksumMode::None;
        let partial = ChecksumMode::Partial { csum_offset: 50, start_offset: 14 };
        let complete = ChecksumMode::Complete { csum_value: 0x1234 };
        let unnecessary = ChecksumMode::Unnecessary;

        assert_eq!(none, ChecksumMode::None);
        assert_ne!(partial, none);
        assert_ne!(complete, partial);
        assert_ne!(unnecessary, complete);
    }

    #[test]
    fn lco_checksum_basic() {
        // VXLAN-like scenario:
        // Bytes 0..14: Ethernet, 14..34: IPv4, 34..42: Outer UDP
        // Bytes 42..: Inner packet (csum_start = 42)
        // Outer UDP checksum at bytes 40..41 (csum_offset = -2 from csum_start)
        let mut packet = vec![0u8; 50];

        // Outer UDP header (bytes 34..41)
        packet[34] = 0x12; packet[35] = 0x34; // src port
        packet[36] = 0x56; packet[37] = 0x78; // dst port
        packet[38] = 0x00; packet[39] = 0x08; // length = 8
        // checksum at 40..41 = 0 (not yet filled by NIC)

        let csum = lco_checksum(&packet, 42, -2);

        // Manual:
        // Loop sums bytes 0..42 = UDP words: 0x1234 + 0x5678 + 0x0008 + 0x0000 = 0x68B4
        // complement(0x0000) = 0xFFFF
        // total = 0x68B4 + 0xFFFF = 0x168B3
        // fold: 0x68B3 + 0x0001 = 0x68B4
        // fold_checksum(0x68B4) = !(0x68B4 as u16) = 0x974B
        assert_eq!(csum, 0x974B, "LCO checksum mismatch");

        // Now with checksum field = 0x1234 (pre-filled scenario)
        // LCO key property: word + !word = 0xFFFF, so the checksum field
        // value doesn't affect the result. Both cases yield the same checksum.
        packet[40] = 0x12; packet[41] = 0x34;
        let csum2 = lco_checksum(&packet, 42, -2);
        assert_eq!(csum2, csum, "LCO result independent of checksum field value");
    }
}

