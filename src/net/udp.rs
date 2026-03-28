//! # UDP Protokolü (User Datagram Protocol)
//!
//! RFC 768 ile tanımlanan bağlantısız, güvenilmez ama hızlı taşıma katmanı protokolü.
//!
//! ## UDP ve TCP Karşılaştırması
//!
//! ```
//!  UDP:                        TCP:
//!  Gönder ve Unut              Bağlan -> Gönder -> Onayla -> Kapat
//!  Sıra garantisi yok          Sıralı teslimat
//!  Hata düzeltme yok           Yeniden iletim var
//!  Başlık: 8 byte              Başlık: 20+ byte
//!  Gecikme: düşük              Gecikme: yüksek
//!  Kullanım: DNS, VoIP, Oyun   Kullanım: HTTP, SSH, FTP
//! ```
//!
//! ## UDP Datagram Yapısı
//!
//! ```
//!  0      7 8     15 16    23 24    31
//!  +--------+--------+--------+--------+
//!  |  Kaynak |  Hedef  |                |
//!  |   Port  |   Port  |    Uzunluk     |
//!  +--------+--------+--------+--------+
//!  |         |                          |
//!  | Kontrol | ...Veri (Payload)...     |
//!  |  Toplamı|                          |
//!  +--------+--------------------------+
//! ```
//!
//! ## UDP Sahte Başlık (Pseudo-Header) - Checksum İçin
//!
//! ```
//!  UDP checksum'u hesaplamak için IP başlığından alanlar ödünç alınır:
//!  +---------------------------+
//!  | Kaynak IP (4 byte)        |
//!  +---------------------------+
//!  | Hedef IP (4 byte)         |
//!  +---------------------------+
//!  | 0x00 | Proto=17 | UDP Len |
//!  +---------------------------+
//!  | UDP başlığı + veri        |
//!  +---------------------------+
//! ```

use super::ip::{IpProtocol, Ipv4Packet};
use super::ipv6::{Ipv6Header, Ipv6NextHeader, Ipv6Packet};
use super::socket::SocketAddr;
use super::{allocate_socket_id, socket::AddressFamily, IpAddr, Ipv4Addr, NetError, Port};
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU16, Ordering};
use spin::Mutex;

/// Sonraki kısa ömürlü (ephemeral) port numarası.
/// IANA tanımlı dinamik port aralığı: 49152–65535
static NEXT_EPHEMERAL_PORT: AtomicU16 = AtomicU16::new(49152);

/// UDP başlık yapısı (8 byte sabit)
///
/// UDP'nin tüm başlığı yalnızca 8 byte'tır - TCP'nin 20+ byte başlığından çok daha küçük.
/// Bu yüzden UDP düşük başlık yüküne sahiptir.
#[derive(Clone, Copy, Debug)]
pub struct UdpHeader {
    /// Kaynak port numarası (0-65535)
    pub src_port: Port,
    /// Hedef port numarası (0-65535)
    pub dst_port: Port,
    /// Başlık + veri toplam uzunluğu (byte)
    pub length: u16,
    /// Hata tespiti için ones-complement checksum
    /// IPv4'te isteğe bağlıdır (0 = hesaplanmadı)
    pub checksum: u16,
}

impl UdpHeader {
    /// UDP başlık boyutu: 8 byte (sabit)
    pub const SIZE: usize = 8;

    /// Ham byte dizisinden UDP başlığını ayrıştır
    /// Tüm değerler ağ byte sırasıyla (big-endian) saklanır
    pub fn parse(data: &[u8]) -> Result<Self, NetError> {
        if data.len() < Self::SIZE {
            return Err(NetError::InvalidPacket);
        }

        let src_port = Port(u16::from_be_bytes([data[0], data[1]]));
        let dst_port = Port(u16::from_be_bytes([data[2], data[3]]));
        let length = u16::from_be_bytes([data[4], data[5]]);
        let checksum = u16::from_be_bytes([data[6], data[7]]);

        Ok(UdpHeader {
            src_port,
            dst_port,
            length,
            checksum,
        })
    }

    /// UDP başlığını byte dizisine serileştir (big-endian)
    pub fn serialize(&self, buf: &mut [u8]) -> Result<(), NetError> {
        if buf.len() < Self::SIZE {
            return Err(NetError::BufferFull);
        }

        buf[0..2].copy_from_slice(&self.src_port.0.to_be_bytes());
        buf[2..4].copy_from_slice(&self.dst_port.0.to_be_bytes());
        buf[4..6].copy_from_slice(&self.length.to_be_bytes());
        buf[6..8].copy_from_slice(&self.checksum.to_be_bytes());

        Ok(())
    }

    /// Yeni UDP başlığı oluştur (checksum = 0, sonradan hesaplanabilir)
    pub fn new(src_port: Port, dst_port: Port, length: u16) -> Self {
        UdpHeader {
            src_port,
            dst_port,
            length,
            checksum: 0,
        }
    }
}

/// UDP checksum hesapla
///
/// ## Ones-Complement Algoritması
///
/// ```
/// 1. Sahte başlık + UDP başlığı + veri'yi 16-bitlik kelimelere böl
/// 2. Tüm kelimelerin ones-complement toplamını al
/// 3. Elde taşımaları kat: while carry != 0 { sum = (sum & 0xFFFF) + carry }
/// 4. Sonucun ones-complement'ini (bitwise NOT) al
/// 5. Sonuç 0 ise 0xFFFF döndür (0 "checksum yok" anlamına gelir)
/// ```
///
/// Alıcı taraf: Başlık + veri üzerinden aynı hesabı yapar, sonuç 0 olmalı.
pub fn compute_checksum(src_ip: Ipv4Addr, dst_ip: Ipv4Addr, segment: &[u8]) -> u16 {
    let mut sum: u32 = 0;

    // Sahte başlık: Kaynak IP + Hedef IP + Protokol(17) + UDP Uzunluğu
    sum += u16::from_be_bytes([src_ip.0[0], src_ip.0[1]]) as u32;
    sum += u16::from_be_bytes([src_ip.0[2], src_ip.0[3]]) as u32;
    sum += u16::from_be_bytes([dst_ip.0[0], dst_ip.0[1]]) as u32;
    sum += u16::from_be_bytes([dst_ip.0[2], dst_ip.0[3]]) as u32;
    sum += 17u32; // UDP protokol numarası
    sum += segment.len() as u32;

    // UDP segmentini (başlık + veri) 16-bit kelimelere bölerek topla
    let mut i = 0;
    while i + 1 < segment.len() {
        sum += u16::from_be_bytes([segment[i], segment[i + 1]]) as u32;
        i += 2;
    }

    // Tek sayıda byte varsa son byte'ı yüksek bit olarak ekle
    if i < segment.len() {
        sum += (segment[i] as u32) << 8;
    }

    // Elde taşımalarını katla (fold carries)
    while (sum >> 16) != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }

    // Ones-complement (bitwise NOT)
    // UDP'de 0 = "checksum yok" - checksum 0 olursa 0xFFFF döndür
    let result = !(sum as u16);
    if result == 0 {
        0xFFFF
    } else {
        result
    }
}

/// UDP checksum doğrula
///
/// IPv4 UDP'de checksum isteğe bağlıdır:
/// - checksum == 0 ise doğrulama atlanır
/// - checksum != 0 ise tüm segment üzerinden yeniden hesaplanır, 0 çıkmalı
pub fn verify_checksum(src_ip: Ipv4Addr, dst_ip: Ipv4Addr, segment: &[u8]) -> bool {
    // 0 checksum means "not checked" for IPv4 UDP
    if segment.len() >= 8 {
        let checksum = u16::from_be_bytes([segment[6], segment[7]]);
        if checksum == 0 {
            return true;
        }
    }
    let verification = compute_checksum(src_ip, dst_ip, segment);
    verification == 0 || verification == 0xFFFF
}

/// UDP soketi
///
/// UDP soketleri TCP'den farklı olarak bağlantı durumu tutmaz.
/// Her `send_to` çağrısı bağımsız bir datagram gönderir.
/// Her `recv_from` çağrısı bir datagram ve kaynak adres döndürür.
#[derive(Clone, Debug)]
pub struct UdpSocket {
    /// Benzersiz soket kimliği
    pub id: u32,
    pub family: AddressFamily,
    /// Yerel adres (IP + Port)
    pub local: SocketAddr,
    /// Alınan datagramların tamponu: (kaynak_adres, veri) çiftleri
    pub rx_buffer: Vec<(SocketAddr, Vec<u8>)>,
}

impl UdpSocket {
    /// Yeni UDP soketi oluştur
    pub fn new(family: AddressFamily) -> Self {
        UdpSocket {
            id: allocate_socket_id(),
            family,
            local: SocketAddr::default(),
            rx_buffer: Vec::new(),
        }
    }

    /// Soketi belirtilen adrese bağla (yerel port tahsis et)
    pub fn bind(&mut self, addr: SocketAddr) -> Result<(), NetError> {
        self.local = addr;
        Ok(())
    }

    /// Belirtilen hedefe datagram gönder
    ///
    /// Her çağrı ayrı bir UDP datagramı oluşturur ve IP katmanına iletir.
    /// Bağlantısız olduğu için her çağrıda hedef adres belirtilir.
    pub fn send_to(&mut self, data: &[u8], dst: SocketAddr) -> Result<usize, NetError> {
        // UDP başlığını oluştur: uzunluk = başlık(8) + veri
        let header = UdpHeader::new(
            self.local.port,
            dst.port,
            (UdpHeader::SIZE + data.len()) as u16,
        );

        // Başlığı ve veriyi birleştir
        let mut segment = vec![0u8; UdpHeader::SIZE + data.len()];
        header.serialize(&mut segment)?;
        segment[UdpHeader::SIZE..].copy_from_slice(data);

        match (self.local.ip, dst.ip) {
            (IpAddr::V4(mut src_ip), IpAddr::V4(dst_ip)) => {
                if src_ip.is_unspecified() {
                    src_ip = super::local_ip();
                }
                let checksum = compute_checksum(src_ip, dst_ip, &segment);
                segment[6..8].copy_from_slice(&checksum.to_be_bytes());
                let mut ip_buf = vec![0u8; 1500];
                let len = super::ip::build_packet(dst_ip, IpProtocol::UDP, &segment, &mut ip_buf)?;
                super::send_packet(&ip_buf[..len])?;
            }
            (IpAddr::V6(mut src_ip), IpAddr::V6(dst_ip)) => {
                if src_ip.is_unspecified() {
                    src_ip = super::ipv6::local_ipv6();
                }
                let checksum = compute_checksum_v6(src_ip, dst_ip, &segment);
                segment[6..8].copy_from_slice(&checksum.to_be_bytes());
                let packet = Ipv6Packet::new(
                    Ipv6Header::new(
                        src_ip,
                        dst_ip,
                        Ipv6NextHeader::Udp as u8,
                        segment.len() as u16,
                    ),
                    &segment,
                );
                let serialized = packet.serialize();
                super::send_packet(&serialized)?;
            }
            _ => return Err(NetError::InvalidParam),
        }

        Ok(data.len())
    }

    /// Gelen datagramı al
    ///
    /// Tampon boşsa WouldBlock hatası döner (senkron I/O).
    /// Döndürülen ikinci değer, gönderenin adresidir.
    pub fn recv_from(&mut self, buf: &mut [u8]) -> Result<(usize, SocketAddr), NetError> {
        if self.rx_buffer.is_empty() {
            return Err(NetError::WouldBlock);
        }

        let (src, data) = self.rx_buffer.remove(0);
        let len = buf.len().min(data.len());
        buf[..len].copy_from_slice(&data[..len]);

        Ok((len, src))
    }
}

// ============================================================================
// UDP YÖNETİCİSİ (UDP MANAGER)
// ============================================================================
//
// UDP soketlerini ve port bağlamalarını yöneten global durum.
//
// Mimari:
//   UDP_SOCKETS   : socket_id -> UdpSocket  (tüm aktif soketler)
//   UDP_BINDINGS  : port -> socket_id       (port->soket haritası)
//
// Paket geldiğinde:
//   1. Hedef port -> socket_id (UDP_BINDINGS)
//   2. socket_id -> UdpSocket (UDP_SOCKETS)
//   3. rx_buffer'a ekle

/// Tüm aktif UDP soketleri: soket_id -> UdpSocket
static UDP_SOCKETS: Mutex<BTreeMap<u32, Box<UdpSocket>>> = Mutex::new(BTreeMap::new());
/// Port -> soket kimliği eşleşmesi (gelen paket yönlendirme için)
static UDP_BINDINGS: Mutex<BTreeMap<(AddressFamily, Port), u32>> = Mutex::new(BTreeMap::new());

/// UDP alt sistemini başlat
pub fn init() {
    crate::serial_println!("[UDP] Initialized");
}

/// Yeni UDP soketi oluştur ve ID döndür
pub fn create_socket(family: AddressFamily) -> u32 {
    let sock = UdpSocket::new(family);
    let id = sock.id;
    UDP_SOCKETS.lock().insert(id, Box::new(sock));
    id
}

/// Soketi belirtilen adrese bağla ve port tablosunu güncelle
/// Kısa ömürlü port tahsis et (49152–65535 aralığından döngüsel)
fn allocate_ephemeral_port() -> Port {
    loop {
        let port = NEXT_EPHEMERAL_PORT.fetch_add(1, Ordering::Relaxed);
        // 65535'i aştıysa başa sar
        if port < 49152 {
            NEXT_EPHEMERAL_PORT.store(49152, Ordering::Relaxed);
            continue;
        }
        // Port zaten kullanılıyor mu kontrol et
        let bindings = UDP_BINDINGS.lock();
        let candidate = Port(port);
        if !bindings.contains_key(&(AddressFamily::IPV4, candidate))
            && !bindings.contains_key(&(AddressFamily::IPV6, candidate))
        {
            return candidate;
        }
        // Kullanımdaysa bir sonrakini dene (wrap-around)
    }
}

pub fn bind(socket_id: u32, addr: SocketAddr) -> Result<(), NetError> {
    let mut socks = UDP_SOCKETS.lock();
    let sock = socks.get_mut(&socket_id).ok_or(NetError::ProtocolError)?;

    // Port 0 ise otomatik olarak kısa ömürlü port tahsis et
    let bind_addr = if addr.port.0 == 0 {
        let eport = allocate_ephemeral_port();
        SocketAddr::new(addr.ip, eport)
    } else {
        addr
    };

    sock.bind(bind_addr)?;

    // Port -> socket_id eşleşmesini kaydet
    UDP_BINDINGS
        .lock()
        .insert((sock.family, bind_addr.port), socket_id);

    Ok(())
}

/// Belirtilen soket üzerinden datagram gönder
pub fn send_to(socket_id: u32, data: &[u8], dst: SocketAddr) -> Result<usize, NetError> {
    let mut socks = UDP_SOCKETS.lock();
    let sock = socks.get_mut(&socket_id).ok_or(NetError::ProtocolError)?;
    sock.send_to(data, dst)
}

/// Belirtilen soketten datagram al (bağlayan + veri)
pub fn recv_from(socket_id: u32, buf: &mut [u8]) -> Result<(usize, SocketAddr), NetError> {
    let mut socks = UDP_SOCKETS.lock();
    let sock = socks.get_mut(&socket_id).ok_or(NetError::ProtocolError)?;
    sock.recv_from(buf)
}

/// UDP soketini kapat ve port bağlamasını temizle
pub fn close(socket_id: u32) {
    if let Some(sock) = UDP_SOCKETS.lock().remove(&socket_id) {
        UDP_BINDINGS.lock().remove(&(sock.family, sock.local.port));
    }
}

/// Get socket by ID (for event checking)
pub fn get_socket(socket_id: u32) -> Option<UdpSocket> {
    let socks = UDP_SOCKETS.lock();
    socks.get(&socket_id).map(|s| (**s).clone())
}

/// Get all sockets (for ss utility)
pub fn get_all_sockets() -> Vec<UdpSocket> {
    let socks = UDP_SOCKETS.lock();
    socks.values().map(|s| (**s).clone()).collect()
}

/// Gelen IP paketinden UDP datagramını işle
///
/// ## İşlem Akışı
///
/// ```
/// IP paketi gelir
///     -> UDP başlığını ayrıştır
///     -> Hedef port'u bul
///     -> UDP_BINDINGS'te port -> socket_id bak
///     -> Soketi bul ve rx_buffer'a ekle
/// ```
pub fn process_packet(ip_packet: &Ipv4Packet) -> Result<(), NetError> {
    // ── Checksum doğrulaması ──
    // Checksum 0 değilse (yani hesaplanmışsa) doğrula; geçersizse paketi düşür.
    if !verify_checksum(
        ip_packet.header.src,
        ip_packet.header.dst,
        ip_packet.payload,
    ) {
        crate::serial_println!("[UDP] Checksum verification failed, dropping packet");
        return Err(NetError::ChecksumError);
    }

    let udp_header = UdpHeader::parse(ip_packet.payload)?;
    // Veri: UDP başlığından sonra, UDP uzunluk alanı kadar
    let data = &ip_packet.payload[UdpHeader::SIZE..udp_header.length as usize];

    // Kaynak adres: IP + Port
    let src = SocketAddr::new(ip_packet.header.src, udp_header.src_port);

    // Hedef porta kayıtlı soketi bul
    let bindings = UDP_BINDINGS.lock();
    if let Some(&socket_id) = bindings.get(&(AddressFamily::IPV4, udp_header.dst_port)) {
        drop(bindings); // Kilidi serbest bırak (deadlock önlemi)

        let mut socks = UDP_SOCKETS.lock();
        if let Some(sock) = socks.get_mut(&socket_id) {
            // Veriyi soketin alıcı tamponuna ekle
            sock.rx_buffer.push((src, data.to_vec()));
        }
    }

    Ok(())
}

pub fn process_ipv6_packet(ip_packet: &Ipv6Packet) -> Result<(), NetError> {
    if !verify_checksum_v6(
        ip_packet.header.src,
        ip_packet.header.dst,
        &ip_packet.payload,
    ) {
        crate::serial_println!("[UDPv6] Checksum verification failed, dropping packet");
        return Err(NetError::ChecksumError);
    }

    let udp_header = UdpHeader::parse(&ip_packet.payload)?;
    let data = &ip_packet.payload[UdpHeader::SIZE..udp_header.length as usize];
    let src = SocketAddr::new(ip_packet.header.src, udp_header.src_port);

    let bindings = UDP_BINDINGS.lock();
    if let Some(&socket_id) = bindings.get(&(AddressFamily::IPV6, udp_header.dst_port)) {
        drop(bindings);

        let mut socks = UDP_SOCKETS.lock();
        if let Some(sock) = socks.get_mut(&socket_id) {
            sock.rx_buffer.push((src, data.to_vec()));
        }
    }

    Ok(())
}

pub fn compute_checksum_v6(
    src_ip: super::ipv6::Ipv6Addr,
    dst_ip: super::ipv6::Ipv6Addr,
    segment: &[u8],
) -> u16 {
    let mut sum: u32 = 0;
    for chunk in src_ip.0.chunks(2) {
        sum += u16::from_be_bytes([chunk[0], chunk[1]]) as u32;
    }
    for chunk in dst_ip.0.chunks(2) {
        sum += u16::from_be_bytes([chunk[0], chunk[1]]) as u32;
    }
    let len = segment.len() as u32;
    sum += (len >> 16) as u32;
    sum += len & 0xFFFF;
    sum += Ipv6NextHeader::Udp as u32;
    let mut i = 0;
    while i + 1 < segment.len() {
        sum += u16::from_be_bytes([segment[i], segment[i + 1]]) as u32;
        i += 2;
    }
    if i < segment.len() {
        sum += (segment[i] as u32) << 8;
    }
    while (sum >> 16) != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    let result = !(sum as u16);
    if result == 0 {
        0xFFFF
    } else {
        result
    }
}

pub fn verify_checksum_v6(
    src_ip: super::ipv6::Ipv6Addr,
    dst_ip: super::ipv6::Ipv6Addr,
    segment: &[u8],
) -> bool {
    let verification = compute_checksum_v6(src_ip, dst_ip, segment);
    verification == 0 || verification == 0xFFFF
}

// ============================================================================
// netstat desteği
// ============================================================================

/// netstat komutu için UDP soket özeti
#[derive(Clone, Debug)]
pub struct UdpSocketInfo {
    pub port: u16,
}

/// Tüm UDP soketlerini listele (netstat için)
pub fn list_sockets() -> Vec<UdpSocketInfo> {
    let bindings = UDP_BINDINGS.lock();
    bindings
        .keys()
        .map(|(_, p)| UdpSocketInfo { port: p.0 })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn udp_ipv6_process_packet_routes_bound_socket() {
        let socket_id = create_socket(AddressFamily::IPV6);
        bind(socket_id, SocketAddr::unspecified_v6(Port(53530))).unwrap();

        let payload = [1u8, 2, 3, 4, 5];
        let header = UdpHeader::new(
            Port(40000),
            Port(53530),
            (UdpHeader::SIZE + payload.len()) as u16,
        );
        let mut segment = vec![0u8; UdpHeader::SIZE + payload.len()];
        header.serialize(&mut segment).unwrap();
        segment[UdpHeader::SIZE..].copy_from_slice(&payload);

        let src = super::super::ipv6::Ipv6Addr::from_segments([0x2001, 0xdb8, 0, 0, 0, 0, 0, 1]);
        let dst = super::super::ipv6::Ipv6Addr::from_segments([0x2001, 0xdb8, 0, 0, 0, 0, 0, 2]);
        let checksum = compute_checksum_v6(src, dst, &segment);
        segment[6..8].copy_from_slice(&checksum.to_be_bytes());

        let packet = Ipv6Packet::new(
            Ipv6Header::new(src, dst, Ipv6NextHeader::Udp as u8, segment.len() as u16),
            &segment,
        );
        process_ipv6_packet(&packet).unwrap();

        let mut buf = [0u8; 16];
        let (len, addr) = recv_from(socket_id, &mut buf).unwrap();
        assert_eq!(len, payload.len());
        assert_eq!(&buf[..len], &payload);
        assert_eq!(addr.ip, IpAddr::V6(src));
        assert_eq!(addr.port, Port(40000));

        close(socket_id);
    }
}
