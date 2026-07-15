//! # GSO/TSO/UFO — Segmentation Offload Framework
//!
//! Büyük paketleri (64KB'a kadar) NIC veya yazılım katmanında MSS boyutunda
//! parçalara ayırır. Throughput'u 2-3x artırır.
//!
//! ## Segmentation Offload Türleri
//!
//! | Tür  | Açıklama                                    | Kaynak        |
//! |------|---------------------------------------------|---------------|
//! | TSO  | TCP Segmentation Offload (NIC hardware)     | Linux kernel  |
//! | GSO  | Generic Segmentation Offload (software)     | Linux kernel  |
//! | UFO  | UDP Fragmentation Offload                   | Linux kernel  |
//!
//! ## Çalışma Prensibi
//!
//! ```text
//!  Uygulama: 64 KB veri
//!       │
//!       ▼
//!  TCP send: Tek bir büyük segment (gso_size = MSS)
//!       │
//!       ├── HW TSO destekliyorsa → NIC segment eder
//!       │
//!       └── HW TSO yoksa → GSO software segment eder
//!             │
//!             ├── Segment 1: MSS bytes (TCP seq: 0)
//!             ├── Segment 2: MSS bytes (TCP seq: MSS)
//!             ├── Segment 3: MSS bytes (TCP seq: 2*MSS)
//!             └── ...
//! ```
//!
//! ## gso_size ve gso_type
//!
//! - `gso_size`: Her segment'in maksimum payload boyutu (MSS)
//! - `gso_type`: Segmentasyon türü (TCP4, TCP6, UDP, ...)
//!
//! Kaynak: Linux Kernel Documentation (networking/segmentation-offloads.html)

use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::net::checksum::{
    compute_ipv4_tcp_checksum, compute_ipv4_udp_checksum, fold_checksum,
    internet_checksum, ipv6_pseudo_header_checksum, partial_checksum,
};

// ============================================================================
// GSO TYPE FLAGS
// ============================================================================

/// GSO tür flag'leri (Linux sk_buff gso_type ile uyumlu)
pub const SKB_GSO_TCPV4: u32 = 1 << 0;
pub const SKB_GSO_TCPV6: u32 = 1 << 2;
pub const SKB_GSO_UDP: u32 = 1 << 4;
pub const SKB_GSO_DODGY: u32 = 1 << 6;
pub const SKB_GSO_TCP_ECN: u32 = 1 << 7;
pub const SKB_GSO_TCP_FIXEDID: u32 = 1 << 8;
pub const SKB_GSO_GRE: u32 = 1 << 9;
pub const SKB_GSO_PARTIAL: u32 = 1 << 13;

// ============================================================================
// SEGMENT DESCRIPTOR
// ============================================================================

/// Segmentasyon için meta-bilgi taşıyıcı
#[derive(Clone, Debug)]
pub struct GsoInfo {
    /// GSO türü (SKB_GSO_TCPV4, vb.)
    pub gso_type: u32,
    
    /// Her segment'in maksimum payload boyutu (MSS)
    pub gso_size: u16,
    
    /// Transport header offset (Ethernet başından)
    pub transport_header: u16,
    
    /// Network header offset (Ethernet başından)
    pub network_header: u16,
    
    /// MAC header uzunluğu
    pub mac_header: u16,
    
    /// Sequence number başlangıcı (TCP için)
    pub seq_start: u32,
    
    /// IP ID başlangıcı (IPv4 için)
    pub ip_id_start: u16,
    
    /// Fixed ID kullan (TSO_FIXEDID)
    pub fixed_ip_id: bool,
    
    /// Don't Fragment bit (IPv4)
    pub df_bit: bool,
}

impl GsoInfo {
    /// TCP/IPv4 için varsayılan GSO info
    pub fn tcp4(mss: u16, seq: u32) -> Self {
        GsoInfo {
            gso_type: SKB_GSO_TCPV4,
            gso_size: mss,
            transport_header: 34,  // 14 (Ethernet) + 20 (IPv4)
            network_header: 14,    // 14 (Ethernet)
            mac_header: 0,
            seq_start: seq,
            ip_id_start: 0,
            fixed_ip_id: false,
            df_bit: true,
        }
    }
    
    /// TCP/IPv6 için varsayılan GSO info
    pub fn tcp6(mss: u16, seq: u32) -> Self {
        GsoInfo {
            gso_type: SKB_GSO_TCPV6,
            gso_size: mss,
            transport_header: 54,  // 14 (Ethernet) + 40 (IPv6)
            network_header: 14,
            mac_header: 0,
            seq_start: seq,
            ip_id_start: 0,
            fixed_ip_id: false,
            df_bit: false,
        }
    }
    
    /// UDP için GSO info
    pub fn udp(mss: u16, ipv6: bool) -> Self {
        GsoInfo {
            gso_type: SKB_GSO_UDP,
            gso_size: mss,
            transport_header: if ipv6 { 54 } else { 34 },
            network_header: 14,
            mac_header: 0,
            seq_start: 0,
            ip_id_start: 0,
            fixed_ip_id: false,
            df_bit: false,
        }
    }
    
    /// TCP segmentasyonu mu?
    pub fn is_tcp(&self) -> bool {
        self.gso_type & (SKB_GSO_TCPV4 | SKB_GSO_TCPV6) != 0
    }
    
    /// IPv4 paketi mi?
    pub fn is_ipv4(&self) -> bool {
        self.gso_type & SKB_GSO_TCPV4 != 0
            || (self.gso_type & SKB_GSO_UDP != 0 && self.transport_header == 34)
    }
    
    /// IPv6 paketi mi?
    pub fn is_ipv6(&self) -> bool {
        self.gso_type & SKB_GSO_TCPV6 != 0
            || (self.gso_type & SKB_GSO_UDP != 0 && self.transport_header == 54)
    }
}

// ============================================================================
// GSO STATISTICS
// ============================================================================

/// GSO istatistikleri
pub struct GsoStats {
    /// Toplam segmentasyon sayısı
    pub total_segmented: AtomicU64,
    
    /// Toplam üretilen segment sayısı
    pub total_segments: AtomicU64,
    
    /// HW TSO ile işlenen paket sayısı
    pub hw_tso_count: AtomicU64,
    
    /// SW GSO ile işlenen paket sayısı
    pub sw_gso_count: AtomicU64,
    
    /// Segmentasyon hatası sayısı
    pub errors: AtomicU64,
}

impl GsoStats {
    pub const fn new() -> Self {
        GsoStats {
            total_segmented: AtomicU64::new(0),
            total_segments: AtomicU64::new(0),
            hw_tso_count: AtomicU64::new(0),
            sw_gso_count: AtomicU64::new(0),
            errors: AtomicU64::new(0),
        }
    }
    
    pub fn snapshot(&self) -> GsoStatsSnapshot {
        GsoStatsSnapshot {
            total_segmented: self.total_segmented.load(Ordering::Relaxed),
            total_segments: self.total_segments.load(Ordering::Relaxed),
            hw_tso_count: self.hw_tso_count.load(Ordering::Relaxed),
            sw_gso_count: self.sw_gso_count.load(Ordering::Relaxed),
            errors: self.errors.load(Ordering::Relaxed),
        }
    }
}

#[derive(Clone, Debug)]
pub struct GsoStatsSnapshot {
    pub total_segmented: u64,
    pub total_segments: u64,
    pub hw_tso_count: u64,
    pub sw_gso_count: u64,
    pub errors: u64,
}

/// Global GSO istatistikleri
pub static GSO_STATS: GsoStats = GsoStats::new();

// ============================================================================
// GSO SEGMENTER (Software Segmentation)
// ============================================================================

/// Tek bir büyük paketi MSS boyutunda segmentlere ayırır
///
/// `packet`: Tam paket (Ethernet + IP + TCP + payload)
/// `info`: GSO meta-bilgisi
///
/// Döndürülen: Segment edilmiş paketler vektörü
pub fn gso_segment(packet: &[u8], info: &GsoInfo) -> Result<Vec<Vec<u8>>, GsoError> {
    if info.gso_size == 0 {
        return Err(GsoError::InvalidMss);
    }
    
    let transport_offset = info.transport_header as usize;
    let network_offset = info.network_header as usize;
    if packet.len() < transport_offset + 8 || packet.len() < network_offset + 20 {
        GSO_STATS.errors.fetch_add(1, Ordering::Relaxed);
        return Err(GsoError::PacketTooSmall);
    }

    let transport_header_len = if info.is_tcp() {
        if packet.len() < transport_offset + 20 {
            GSO_STATS.errors.fetch_add(1, Ordering::Relaxed);
            return Err(GsoError::InvalidHeader);
        }
        (((packet[transport_offset + 12] >> 4) as usize) * 4).max(20)
    } else if info.gso_type & SKB_GSO_UDP != 0 {
        8
    } else {
        GSO_STATS.errors.fetch_add(1, Ordering::Relaxed);
        return Err(GsoError::UnsupportedType);
    };

    let network_header_len = if info.is_ipv4() {
        ((packet[network_offset] & 0x0F) as usize * 4).max(20)
    } else if info.is_ipv6() {
        40
    } else {
        GSO_STATS.errors.fetch_add(1, Ordering::Relaxed);
        return Err(GsoError::UnsupportedType);
    };

    let header_len = transport_offset + transport_header_len;
    if packet.len() <= header_len {
        // Segment edilecek payload yok
        return Ok(vec![packet.to_vec()]);
    }
    
    let payload_offset = header_len;
    let payload_len = packet.len() - payload_offset;
    let mss = info.gso_size as usize;
    let segment_count = (payload_len + mss - 1) / mss;
    
    GSO_STATS.total_segmented.fetch_add(1, Ordering::Relaxed);
    GSO_STATS.total_segments.fetch_add(segment_count as u64, Ordering::Relaxed);
    GSO_STATS.sw_gso_count.fetch_add(1, Ordering::Relaxed);
    
    let mut segments = Vec::with_capacity(segment_count);
    let mut current_seq = info.seq_start;
    let mut current_ip_id = info.ip_id_start;
    
    for i in 0..segment_count {
        let start = payload_offset + i * mss;
        let end = core::cmp::min(start + mss, packet.len());
        let seg_payload = &packet[start..end];
        
        let mut segment = Vec::with_capacity(header_len + seg_payload.len());
        
        // Header'ları kopyala
        segment.extend_from_slice(&packet[..header_len]);
        segment.extend_from_slice(seg_payload);
        
        // TCP sequence number güncelle
        if info.is_tcp() && header_len >= 38 {
            let seq_offset = info.transport_header as usize + 4;  // TCP seq field
            let seq_bytes = current_seq.to_be_bytes();
            if segment.len() >= seq_offset + 4 {
                segment[seq_offset..seq_offset + 4].copy_from_slice(&seq_bytes);
            }
        }
        
        // IPv4 ID güncelle (fixed ID değilse)
        if info.is_ipv4() && !info.fixed_ip_id && header_len >= network_offset + 20 {
            let id_offset = network_offset + 4;  // IP ID field
            let id_bytes = current_ip_id.to_be_bytes();
            if segment.len() >= id_offset + 2 {
                segment[id_offset] = id_bytes[0];
                segment[id_offset + 1] = id_bytes[1];
            }
            current_ip_id = current_ip_id.wrapping_add(1);
        }
        
        // Son segment değilse PSH flag'ini temizle
        if i < segment_count - 1 && info.is_tcp() {
            let flags_offset = transport_offset + 13;
            if segment.len() > flags_offset {
                segment[flags_offset] &= !0x08;  // Clear PSH
            }
        }
        
        // FIN flag sadece son segment'te kalır
        if i < segment_count - 1 && info.is_tcp() {
            let flags_offset = transport_offset + 13;
            if segment.len() > flags_offset {
                segment[flags_offset] &= !0x01;  // Clear FIN
            }
        }

        if info.is_ipv4() {
            let len_offset = network_offset + 2;
            let ip_len = (segment.len() - network_offset) as u16;
            let len_bytes = ip_len.to_be_bytes();
            segment[len_offset] = len_bytes[0];
            segment[len_offset + 1] = len_bytes[1];

            let flags_fragment_offset = network_offset + 6;
            if info.df_bit {
                segment[flags_fragment_offset] |= 0x40;
            } else {
                segment[flags_fragment_offset] &= !0x40;
            }

            let checksum_offset = network_offset + 10;
            segment[checksum_offset] = 0;
            segment[checksum_offset + 1] = 0;
            let ip_checksum = internet_checksum(&segment[network_offset..network_offset + network_header_len]);
            segment[checksum_offset..checksum_offset + 2].copy_from_slice(&ip_checksum.to_be_bytes());
        } else if info.is_ipv6() {
            let payload_len_offset = network_offset + 4;
            let ipv6_payload_len = (segment.len() - network_offset - network_header_len) as u16;
            segment[payload_len_offset..payload_len_offset + 2]
                .copy_from_slice(&ipv6_payload_len.to_be_bytes());
        }

        if info.is_tcp() {
            if info.is_ipv4() {
                if !compute_ipv4_tcp_checksum(&mut segment) {
                    GSO_STATS.errors.fetch_add(1, Ordering::Relaxed);
                    return Err(GsoError::InvalidHeader);
                }
            } else if info.is_ipv6() {
                let src_ip: [u8; 16] = segment[network_offset + 8..network_offset + 24]
                    .try_into().unwrap();
                let dst_ip: [u8; 16] = segment[network_offset + 24..network_offset + 40]
                    .try_into().unwrap();
                let tcp_len = (segment.len() - transport_offset) as u32;
                let csum_offset = transport_offset + 16;
                segment[csum_offset] = 0;
                segment[csum_offset + 1] = 0;
                let pseudo_sum = ipv6_pseudo_header_checksum(&src_ip, &dst_ip, 6, tcp_len);
                let tcp_sum = partial_checksum(&segment, transport_offset, segment.len());
                let total = pseudo_sum.wrapping_add(tcp_sum);
                let csum = fold_checksum(total);
                segment[csum_offset] = (csum >> 8) as u8;
                segment[csum_offset + 1] = (csum & 0xFF) as u8;
            }
        } else if info.gso_type & SKB_GSO_UDP != 0 {
            if info.is_ipv4() {
                if !compute_ipv4_udp_checksum(&mut segment) {
                    GSO_STATS.errors.fetch_add(1, Ordering::Relaxed);
                    return Err(GsoError::InvalidHeader);
                }
            } else if info.is_ipv6() {
                let src_ip: [u8; 16] = segment[network_offset + 8..network_offset + 24]
                    .try_into().unwrap();
                let dst_ip: [u8; 16] = segment[network_offset + 24..network_offset + 40]
                    .try_into().unwrap();
                let udp_len = (segment.len() - transport_offset) as u32;
                let csum_offset = transport_offset + 6;
                segment[csum_offset] = 0;
                segment[csum_offset + 1] = 0;
                let pseudo_sum = ipv6_pseudo_header_checksum(&src_ip, &dst_ip, 17, udp_len);
                let udp_sum = partial_checksum(&segment, transport_offset, segment.len());
                let total = pseudo_sum.wrapping_add(udp_sum);
                let csum = fold_checksum(total);
                let final_csum = if csum == 0 { 0xFFFF } else { csum };
                segment[csum_offset] = (final_csum >> 8) as u8;
                segment[csum_offset + 1] = (final_csum & 0xFF) as u8;
            }
        }
        
        current_seq = current_seq.wrapping_add(seg_payload.len() as u32);
        segments.push(segment);
    }
    
    Ok(segments)
}

/// Paket GSO gerektiriyor mu kontrol et
pub fn needs_segmentation(packet_len: usize, mtu: u16) -> bool {
    packet_len > mtu as usize
}

/// HW TSO destekleniyorsa, segment etmeden gönder
pub fn try_hw_tso(packet: &[u8], info: &GsoInfo) -> Option<Vec<u8>> {
    // HW TSO capability check (VirtIO-Net feature negotiation)
    // Eğer NIC TSO destekliyorsa, paketi olduğu gibi gönder
    // Bu fonksiyon driver seviyesinde override edilir
    
    // Şimdilik: HW TSO yok, None döner (SW GSO gerekli)
    None
}

// ============================================================================
// NIC CAPABILITY FLAGS
// ============================================================================

/// NIC segmentation offload capability'leri
#[derive(Clone, Copy, Debug, Default)]
pub struct NicOffloadCaps {
    /// TCP Segmentation Offload (IPv4)
    pub tso_ipv4: bool,
    
    /// TCP Segmentation Offload (IPv6)
    pub tso_ipv6: bool,
    
    /// UDP Fragmentation Offload
    pub ufo: bool,
    
    /// Generic Segmentation Offload (software fallback)
    pub gso: bool,
    
    /// Maksimum segment boyutu (bytes)
    pub max_segment_size: u32,
    
    /// Maksimum segment sayısı (tek paket için)
    pub max_segments: u16,
}

impl NicOffloadCaps {
    /// Tüm offload'lar kapalı (minimum capability)
    pub fn none() -> Self {
        NicOffloadCaps::default()
    }
    
    /// VirtIO-Net VIRTIO_NET_F_GSO feature'ları
    pub fn virtio_net(features: u64) -> Self {
        NicOffloadCaps {
            tso_ipv4: features & (1 << 7) != 0,  // VIRTIO_NET_F_GUEST_TSO4
            tso_ipv6: features & (1 << 8) != 0,  // VIRTIO_NET_F_GUEST_TSO6
            ufo: features & (1 << 6) != 0,        // VIRTIO_NET_F_GSO
            gso: true,
            max_segment_size: 65535,
            max_segments: 64,
        }
    }
    
    /// TSO destekleniyor mu?
    pub fn supports_tso(&self, ipv6: bool) -> bool {
        if ipv6 { self.tso_ipv6 } else { self.tso_ipv4 }
    }
}

// ============================================================================
// GSO ERROR
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GsoError {
    InvalidMss,
    PacketTooSmall,
    InvalidHeader,
    UnsupportedType,
}

// ============================================================================
// INIT
// ============================================================================

pub fn init() {
    crate::serial_println!("[GSO] Segmentation offload framework initialized");
}

// ============================================================================
// TESTLER
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn gso_info_tcp4() {
        let info = GsoInfo::tcp4(1460, 1000);
        assert!(info.is_tcp());
        assert!(info.is_ipv4());
        assert!(!info.is_ipv6());
        assert_eq!(info.gso_size, 1460);
        assert_eq!(info.seq_start, 1000);
    }
    
    #[test]
    fn gso_segment_basic() {
        // Ethernet(14) + IPv4(20) + TCP(20) + payload(4380) = 4434 bytes
        // MSS=1460, segment_count = ceil(4380/1460) = 3
        let mut packet = vec![0u8; 14 + 20 + 20 + 4380];
        
        // Ethernet type = IPv4 (0x0800)
        packet[12] = 0x08;
        packet[13] = 0x00;
        
        let info = GsoInfo::tcp4(1460, 0);
        let segments = gso_segment(&packet, &info).unwrap();
        
        assert_eq!(segments.len(), 3);
        
        // İlk 2 segment tam MSS
        assert_eq!(segments[0].len(), 14 + 20 + 20 + 1460);
        assert_eq!(segments[1].len(), 14 + 20 + 20 + 1460);
        
        // Son segment kalan payload
        assert_eq!(segments[2].len(), 14 + 20 + 20 + (4380 - 2*1460));
    }

    #[test]
    fn gso_segment_updates_ipv4_length_and_checksum() {
        let mut packet = vec![0u8; 14 + 20 + 20 + 3000];
        packet[12] = 0x08;
        packet[13] = 0x00;
        packet[14] = 0x45;
        packet[16..18].copy_from_slice(&(20u16 + 20u16 + 3000u16).to_be_bytes());
        packet[23] = 6;
        packet[26..30].copy_from_slice(&[10, 0, 0, 1]);
        packet[30..34].copy_from_slice(&[10, 0, 0, 2]);
        packet[34 + 12] = 0x50;
        packet[34 + 13] = 0x19; // FIN + PSH + ACK

        let info = GsoInfo::tcp4(1460, 1000);
        let segments = gso_segment(&packet, &info).unwrap();
        assert_eq!(segments.len(), 3);

        let ip_total_len_0 = u16::from_be_bytes([segments[0][16], segments[0][17]]) as usize;
        let ip_total_len_2 = u16::from_be_bytes([segments[2][16], segments[2][17]]) as usize;
        assert_eq!(ip_total_len_0, segments[0].len() - 14);
        assert_eq!(ip_total_len_2, segments[2].len() - 14);

        assert_eq!(internet_checksum(&segments[0][14..34]), 0);
        assert_eq!(internet_checksum(&segments[2][14..34]), 0);
        assert_eq!(segments[0][34 + 13] & 0x09, 0); // non-last: PSH/FIN cleared
        assert_eq!(segments[2][34 + 13] & 0x09, 0x09); // last: PSH/FIN preserved
    }
    
    #[test]
    fn gso_segment_small_packet() {
        // MSS'den küçük paket segment edilmez
        let packet = vec![0u8; 100];
        let info = GsoInfo::tcp4(1460, 0);
        let segments = gso_segment(&packet, &info).unwrap();
        
        assert_eq!(segments.len(), 1);
    }
    
    #[test]
    fn gso_zero_mss_error() {
        let packet = vec![0u8; 200];
        let info = GsoInfo {
            gso_type: SKB_GSO_TCPV4,
            gso_size: 0,  // Geçersiz!
            transport_header: 34,
            network_header: 14,
            mac_header: 0,
            seq_start: 0,
            ip_id_start: 0,
            fixed_ip_id: false,
            df_bit: true,
        };
        
        let result = gso_segment(&packet, &info);
        assert_eq!(result.unwrap_err(), GsoError::InvalidMss);
    }
    
    #[test]
    fn nic_offload_caps_virtio() {
        let features = (1 << 7) | (1 << 8);  // TSO4 + TSO6
        let caps = NicOffloadCaps::virtio_net(features);
        
        assert!(caps.supports_tso(false));  // IPv4 TSO
        assert!(caps.supports_tso(true));   // IPv6 TSO
    }
    
    #[test]
    fn needs_segmentation_check() {
        assert!(needs_segmentation(9000, 1500));    // Jumbo frame
        assert!(!needs_segmentation(1400, 1500));   // Normal packet
        assert!(!needs_segmentation(1500, 1500));   // Exactly MTU
    }
}
