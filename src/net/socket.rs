//! # POSIX Soket API'si
//!
//! Bu modül, Linux ile uyumlu soket arayüzünü echOS'a taşır.
//!
//! ## POSIX Soket Nedir?
//!
//! Soket (socket), iki uç nokta arasında çift yönlü iletişim kanalıdır.
//! POSIX standartı üç temel soket türü tanımlar:
//!
//! - **STREAM (TCP)** : Bağlantı yönelimli, güvenilir, sıralı bayt akışı.
//!   Veri kaybolmaz; paketler bölünür/birleştirilir ama sıra korunur.
//!
//! - **DGRAM (UDP)** : Bağlantısız, güvensiz veri paketi gönderimi.
//!   Her mesaj bağımsızdır; kayıp veya sıra değişimi olabilir.
//!   DNS sorguları, video akışı gibi düşük gecikme gerektiren uygulamalar için.
//!
//! - **RAW** : Ham IP paketi gönderimi; üst katman protokolleri atlanır.
//!   ping, traceroute gibi araçlar kullanır. echOS'ta henüz desteklenmez.
//!
//! ## Tipik TCP Sunucu Akışı
//!
//! ```text
//! socket() → bind() → listen() → accept() → send()/recv() → close()
//! ```
//!
//! ## Tipik TCP İstemci Akışı
//!
//! ```text
//! socket() → connect() → send()/recv() → close()
//! ```

use super::ip::{self, IpProtocol, Ipv4Packet};
use super::ipv6::{self, Ipv6Header, Ipv6Packet};
use super::tcp;
use super::udp;
use super::{allocate_socket_id, send_packet, IpAddr, Ipv4Addr, NetError, Port};
use alloc::collections::{BTreeMap, VecDeque};
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;

// Re-export SocketAddr for other modules
pub use super::SocketAddr;

/// Soket adres ailesi (address family / domain).
///
/// Linux'ta `AF_INET` (2) ile IPv4, `AF_INET6` (10) ile IPv6 seçilir.
/// Bu değerler POSIX standardında sabitlenmiştir.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AddressFamily {
    UNSPEC = 0,
    IPV4 = 2,  // AF_INET
    IPV6 = 10, // AF_INET6
}

/// Soket türü (socket type).
///
/// - `STREAM` (1): TCP — güvenilir, sıralı bayt akışı
/// - `DGRAM`  (2): UDP — güvensiz, boyut sınırlı datagram
/// - `RAW`    (3): Ham IP — çekirdek seviyesi paket işleme
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SocketType {
    STREAM = 1, // SOCK_STREAM (TCP)
    DGRAM = 2,  // SOCK_DGRAM (UDP)
    RAW = 3,    // SOCK_RAW
}

/// Soket protokolü (socket protocol).
///
/// Genellikle `DEFAULT` (0) bırakılır; `sock_type` parametresi protokolü belirler.
/// Açık belirtme gereken durumlar: RAW soket oluşturulurken ICMP gibi.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Protocol {
    DEFAULT = 0,
    IP = 4,
    TCP = 6,
    UDP = 17,
    ICMP = 1,
}

/// Soket seçenekleri (socket options — setsockopt/getsockopt).
///
/// - `ReuseAddr`    : Aynı port birden fazla soket tarafından kullanılabilir (TIME_WAIT için)
/// - `ReusePort`    : Yük dengeleme için aynı porta birden fazla bağlama
/// - `KeepAlive`    : Boşta bağlantıları canlı tutmak için TCP keepalive
/// - `NoDelay`      : Nagle algoritmasını devre dışı bırak (düşük gecikme)
/// - `RcvBuf(n)`    : Alım tamponu boyutunu n bayta ayarla
/// - `SndBuf(n)`    : Gönderim tamponu boyutunu n bayta ayarla
/// - `RcvTimeout(t)`: Alım zaman aşımını t milisaniye yap
/// - `SndTimeout(t)`: Gönderim zaman aşımını t milisaniye yap
#[derive(Clone, Copy, Debug)]
pub enum SocketOption {
    ReuseAddr,
    ReusePort,
    KeepAlive,
    NoDelay,
    RcvBuf(usize),
    SndBuf(usize),
    RcvTimeout(u64),
    SndTimeout(u64),
}

/// Bir soket nesnesini temsil eden yapı.
///
/// - `id`          : TCP veya UDP katmanının atadığı benzersiz kimlik
/// - `domain`      : Adres ailesi (IPv4/IPv6)
/// - `sock_type`   : STREAM/DGRAM/RAW
/// - `protocol`    : Protokol numarası
/// - `bound`       : `bind()` çağrıldı mı?
/// - `listening`   : `listen()` çağrıldı mı? (yalnızca TCP sunucu için)
/// - `nonblocking` : Bloklamayan mod etkin mi?
pub struct Socket {
    pub id: u32,
    pub domain: AddressFamily,
    pub sock_type: SocketType,
    pub protocol: Protocol,
    pub bound: bool,
    pub listening: bool,
    pub nonblocking: bool,
}

#[derive(Clone, Debug)]
struct RawSocketState {
    id: u32,
    protocol: Protocol,
    family: AddressFamily,
    bound_ip: Option<IpAddr>,
    peer: Option<IpAddr>,
    rx_queue: VecDeque<(SocketAddr, Vec<u8>)>,
}

impl RawSocketState {
    fn new(id: u32, protocol: Protocol, family: AddressFamily) -> Self {
        Self {
            id,
            protocol,
            family,
            bound_ip: None,
            peer: None,
            rx_queue: VecDeque::new(),
        }
    }
}

static RAW_SOCKETS: Mutex<BTreeMap<u32, RawSocketState>> = Mutex::new(BTreeMap::new());

fn protocol_to_ip_protocol(protocol: Protocol) -> Option<IpProtocol> {
    match protocol {
        Protocol::IP => Some(IpProtocol::UNKNOWN),
        Protocol::ICMP => Some(IpProtocol::ICMP),
        Protocol::TCP => Some(IpProtocol::TCP),
        Protocol::UDP => Some(IpProtocol::UDP),
        Protocol::DEFAULT => None,
    }
}

fn create_raw_socket(protocol: Protocol, family: AddressFamily) -> u32 {
    let id = allocate_socket_id();
    RAW_SOCKETS
        .lock()
        .insert(id, RawSocketState::new(id, protocol, family));
    id
}

fn has_raw_socket(socket_id: u32) -> bool {
    RAW_SOCKETS.lock().contains_key(&socket_id)
}

fn raw_bind(socket_id: u32, addr: SocketAddr) -> Result<(), NetError> {
    let mut sockets = RAW_SOCKETS.lock();
    let socket = sockets.get_mut(&socket_id).ok_or(NetError::InvalidFd)?;
    socket.bound_ip = if addr.ip.is_unspecified() {
        None
    } else {
        Some(addr.ip)
    };
    Ok(())
}

fn raw_connect(socket_id: u32, addr: SocketAddr) -> Result<(), NetError> {
    let mut sockets = RAW_SOCKETS.lock();
    let socket = sockets.get_mut(&socket_id).ok_or(NetError::InvalidFd)?;
    socket.peer = Some(addr.ip);
    Ok(())
}

fn raw_send_to(socket_id: u32, data: &[u8], dest: SocketAddr) -> Result<usize, NetError> {
    let sockets = RAW_SOCKETS.lock();
    let socket = sockets.get(&socket_id).ok_or(NetError::InvalidFd)?;
    let protocol = socket.protocol;
    let family = socket.family;
    let src = socket.bound_ip.unwrap_or_else(|| match family {
        AddressFamily::IPV6 => IpAddr::V6(super::ipv6::local_ipv6()),
        _ => IpAddr::V4(super::local_ip()),
    });
    drop(sockets);

    if protocol == Protocol::IP {
        let version = data.first().map(|b| b >> 4);
        if version != Some(4) && version != Some(6) {
            return Err(NetError::InvalidPacket);
        }
        send_packet(data)?;
        return Ok(data.len());
    }

    let ip_proto = protocol_to_ip_protocol(protocol).ok_or(NetError::ProtocolError)?;
    match (src, dest.ip) {
        (IpAddr::V4(src_ip), IpAddr::V4(dst_ip)) => {
            let mut buf = vec![0u8; 1500];
            let len = {
                let packet = Ipv4Packet::new(src_ip, dst_ip, ip_proto, data);
                packet.serialize(&mut buf)?
            };
            send_packet(&buf[..len])?;
        }
        (IpAddr::V6(src_ip), IpAddr::V6(dst_ip)) => {
            let header = Ipv6Header::new(src_ip, dst_ip, ip_proto as u8, data.len() as u16);
            let packet = Ipv6Packet::new(header, data);
            let serialized = packet.serialize();
            send_packet(&serialized)?;
        }
        _ => return Err(NetError::InvalidParam),
    }
    Ok(data.len())
}

fn raw_recv_from(
    socket_id: u32,
    buf: &mut [u8],
    flags: u32,
) -> Result<(usize, SocketAddr), NetError> {
    let peek = (flags & MSG_PEEK) != 0;
    let mut sockets = RAW_SOCKETS.lock();
    let socket = sockets.get_mut(&socket_id).ok_or(NetError::InvalidFd)?;
    let maybe_entry = if peek {
        socket.rx_queue.front().cloned()
    } else {
        socket.rx_queue.pop_front()
    };
    let Some((src, packet)) = maybe_entry else {
        return Err(NetError::WouldBlock);
    };
    let len = packet.len().min(buf.len());
    buf[..len].copy_from_slice(&packet[..len]);
    Ok((len, src))
}

fn raw_close(socket_id: u32) -> bool {
    RAW_SOCKETS.lock().remove(&socket_id).is_some()
}

pub fn deliver_raw_ipv4(packet: &[u8], header: &super::ip::Ipv4Header) {
    let mut sockets = RAW_SOCKETS.lock();
    for socket in sockets.values_mut() {
        let protocol_matches = match socket.protocol {
            Protocol::DEFAULT | Protocol::IP => true,
            Protocol::ICMP => header.protocol == IpProtocol::ICMP,
            Protocol::TCP => header.protocol == IpProtocol::TCP,
            Protocol::UDP => header.protocol == IpProtocol::UDP,
        };
        if !protocol_matches {
            continue;
        }
        if let Some(bound_ip) = socket.bound_ip {
            if bound_ip != IpAddr::V4(header.dst) {
                continue;
            }
        }
        if let Some(peer) = socket.peer {
            if peer != IpAddr::V4(header.src) {
                continue;
            }
        }
        socket
            .rx_queue
            .push_back((SocketAddr::new(header.src, Port(0)), packet.to_vec()));
    }
}

pub fn deliver_raw_ipv6(packet: &[u8], header: &super::ipv6::Ipv6Header) {
    let mut sockets = RAW_SOCKETS.lock();
    for socket in sockets.values_mut() {
        let protocol_matches = match socket.protocol {
            Protocol::DEFAULT | Protocol::IP => true,
            Protocol::ICMP => header.next_header == super::ipv6::Ipv6NextHeader::Icmpv6 as u8,
            Protocol::TCP => header.next_header == super::ipv6::Ipv6NextHeader::Tcp as u8,
            Protocol::UDP => header.next_header == super::ipv6::Ipv6NextHeader::Udp as u8,
        };
        if !protocol_matches {
            continue;
        }
        if let Some(bound_ip) = socket.bound_ip {
            if bound_ip != IpAddr::V6(header.dst) {
                continue;
            }
        }
        if let Some(peer) = socket.peer {
            if peer != IpAddr::V6(header.src) {
                continue;
            }
        }
        socket
            .rx_queue
            .push_back((SocketAddr::new(header.src, Port(0)), packet.to_vec()));
    }
}

impl Socket {
    /// Yeni bir soket oluşturur.
    ///
    /// `sock_type`'a göre TCP, UDP veya RAW katmanında gerçek soket nesnesi oluşturulur.
    pub fn new(
        domain: AddressFamily,
        sock_type: SocketType,
        protocol: Protocol,
    ) -> Result<Self, NetError> {
        let id = match sock_type {
            SocketType::STREAM => tcp::create_socket(domain),
            SocketType::DGRAM => udp::create_socket(domain),
            SocketType::RAW => create_raw_socket(protocol, domain),
        };

        Ok(Socket {
            id,
            domain,
            sock_type,
            protocol,
            bound: false,
            listening: false,
            nonblocking: false,
        })
    }
}

// ============================================================================
// POSIX SOKET API'Sİ (POSIX SOCKET API)
// ============================================================================
//
// Bu fonksiyonlar Linux soket sistem çağrılarıyla birebir uyumludur.
// Her fonksiyon adı ilgili sistem çağrısının (syscall) adıyla eşleşir.
// Böylece kullanıcı alanı uygulamaları minimum değişiklikle çalışabilir.

/// `socket(2)` — Yeni bir soket oluşturur ve kimliğini döndürür.
///
/// Başarılı olursa soket kimliği (fd benzeri u32), aksi halde `NetError` döner.
pub fn socket(
    domain: AddressFamily,
    sock_type: SocketType,
    protocol: Protocol,
) -> Result<u32, NetError> {
    let sock = Socket::new(domain, sock_type, protocol)?;
    Ok(sock.id)
}

/// `bind(2)` — Soketi yerel bir adrese (IP:port) bağlar.
///
/// Bir sunucu uygulaması hangi portu dinleyeceğini `bind()` ile belirtir.
/// Önce TCP ile denenir; başarısız olursa UDP'ye geçilir.
pub fn bind(socket_id: u32, addr: SocketAddr) -> Result<(), NetError> {
    if has_raw_socket(socket_id) {
        return raw_bind(socket_id, addr);
    }

    // Determine socket type from ID (hacky but works)
    if tcp::bind(socket_id, addr).is_ok() {
        return Ok(());
    }

    udp::bind(socket_id, addr)
}

/// `listen(2)` — TCP soketini gelen bağlantıları kabul etmeye hazırlar.
///
/// `backlog`: Henüz `accept()` edilmeyi bekleyen bağlantıların maksimum sayısı.
/// Yalnızca STREAM (TCP) soketlerde geçerlidir.
pub fn listen(socket_id: u32, backlog: usize) -> Result<(), NetError> {
    tcp::listen(socket_id, backlog)
}

/// `accept(2)` — Dinleme soketinden yeni bir bağlantı kabul eder.
///
/// Bağlantı yoksa bloklar (veya `NetError::WouldBlock` döner, nonblocking modda).
/// Dönen değer: (yeni_soket_id, karşı_taraf_adresi)
pub fn accept(socket_id: u32) -> Result<(u32, SocketAddr), NetError> {
    tcp::accept(socket_id)
}

/// `connect(2)` — TCP soketi ile uzak sunucuya bağlantı kurar.
///
/// TCP üç yönlü el sıkışmasını (SYN → SYN-ACK → ACK) başlatır.
/// Bağlantı tamamlanana kadar bloklar.
pub fn connect(socket_id: u32, addr: SocketAddr) -> Result<(), NetError> {
    if has_raw_socket(socket_id) {
        return raw_connect(socket_id, addr);
    }
    tcp::connect(socket_id, addr)
}

/// Soket bayrakları (socket flags)
///
/// Bu sabitler Linux `<socket.h>` başlık dosyasındaki değerlerle aynıdır.
pub const MSG_DONTWAIT: u32 = 0x40; // Non-blocking I/O for this operation
pub const MSG_PEEK: u32 = 0x02; // Peek at incoming data without consuming
pub const MSG_WAITALL: u32 = 0x100; // Wait for full request or error
pub const MSG_NOSIGNAL: u32 = 0x4000; // Don't generate SIGPIPE

/// `send(2)` — Bağlı TCP soketi üzerinden veri gönderir.
///
/// `flags` parametresi şu değerleri destekler:
/// - `MSG_DONTWAIT` (0x40): Bu işlem için bloklamayan mod
///
/// Dönen değer gerçekte gönderilen bayt sayısıdır.
pub fn send(socket_id: u32, data: &[u8], flags: u32) -> Result<usize, NetError> {
    if has_raw_socket(socket_id) {
        let peer = {
            let sockets = RAW_SOCKETS.lock();
            sockets
                .get(&socket_id)
                .and_then(|sock| sock.peer)
                .ok_or(NetError::NotConnected)?
        };
        let _ = flags;
        return raw_send_to(socket_id, data, SocketAddr::new(peer, Port(0)));
    }

    let nonblocking = (flags & MSG_DONTWAIT) != 0;

    // Check if socket is in nonblocking mode or MSG_DONTWAIT is set
    if nonblocking {
        // Try non-blocking send
        match tcp::try_send(socket_id, data) {
            Ok(n) => Ok(n),
            Err(NetError::WouldBlock) => Err(NetError::WouldBlock),
            Err(e) => Err(e),
        }
    } else {
        tcp::send(socket_id, data)
    }
}

/// `recv(2)` — Bağlı TCP soketinden veri alır.
///
/// `buf` dolana kadar veya veri bitene kadar okur.
/// Bağlantı kapanmışsa 0 döner (EOF).
///
/// `flags` parametresi şu değerleri destekler:
/// - `MSG_DONTWAIT` (0x40): Bu işlem için bloklamayan mod
/// - `MSG_PEEK` (0x02): Veriyi tüketmeden önizle
/// - `MSG_WAITALL` (0x100): Tam istenen boyut kadar bekle
pub fn recv(socket_id: u32, buf: &mut [u8], flags: u32) -> Result<usize, NetError> {
    let nonblocking = (flags & MSG_DONTWAIT) != 0;
    let peek = (flags & MSG_PEEK) != 0;
    let waitall = (flags & MSG_WAITALL) != 0;

    if peek {
        // Peek mode: read without consuming
        tcp::peek(socket_id, buf)
    } else if nonblocking {
        // Non-blocking receive
        match tcp::try_recv(socket_id, buf) {
            Ok(n) => {
                if waitall && n < buf.len() {
                    // MSG_WAITALL set but not all data available in nonblocking mode
                    Err(NetError::WouldBlock)
                } else {
                    Ok(n)
                }
            }
            Err(NetError::WouldBlock) => Err(NetError::WouldBlock),
            Err(e) => Err(e),
        }
    } else if waitall {
        // Blocking with MSG_WAITALL: wait until buffer is full or connection closed
        tcp::recv_all(socket_id, buf)
    } else {
        tcp::recv(socket_id, buf)
    }
}

/// `sendto(2)` — UDP soketi ile belirtilen hedefe veri gönderir.
///
/// Her çağrıda hedef adres belirtilebilir; bağlantı durumu gerektirmez.
/// Datagram sınırları korunur: her `sendto` bir UDP paketi oluşturur.
pub fn sendto(
    socket_id: u32,
    data: &[u8],
    dest: SocketAddr,
    flags: u32,
) -> Result<usize, NetError> {
    let _ = flags;
    if has_raw_socket(socket_id) {
        return raw_send_to(socket_id, data, dest);
    }
    udp::send_to(socket_id, data, dest)
}

/// `recvfrom(2)` — UDP soketinden veri ve kaynak adresi alır.
///
/// Dönen değer: (okunan_bayt_sayısı, gönderenin_adresi)
/// Kaynak adresi `sendto()` ile cevap göndermek için kullanılabilir.
pub fn recvfrom(
    socket_id: u32,
    buf: &mut [u8],
    flags: u32,
) -> Result<(usize, SocketAddr), NetError> {
    if has_raw_socket(socket_id) {
        return raw_recv_from(socket_id, buf, flags);
    }
    udp::recv_from(socket_id, buf)
}

/// `close(2)` — Soketi kapatır ve kaynakları serbest bırakır.
///
/// Hem TCP hem UDP için kapatma denenir; hatalar yok sayılır.
/// TCP'de FIN paketi gönderilir ve bağlantı sonlandırılır.
pub fn close(socket_id: u32) -> Result<(), NetError> {
    if raw_close(socket_id) {
        SOCKET_OPTIONS.lock().remove(&socket_id);
        return Ok(());
    }
    tcp::close(socket_id).ok();
    udp::close(socket_id);
    Ok(())
}

/// Soket seçeneklerini global olarak saklayan yapı.
/// Her soket için ayarlanan seçenekler burada tutulur.
static SOCKET_OPTIONS: spin::Mutex<alloc::collections::BTreeMap<u32, SocketOptionsState>> =
    spin::Mutex::new(alloc::collections::BTreeMap::new());

/// Bir soketin tüm seçenek durumunu tutar.
#[derive(Clone, Debug)]
struct SocketOptionsState {
    reuse_addr: bool,
    reuse_port: bool,
    keep_alive: bool,
    no_delay: bool,
    rcv_buf: usize,
    snd_buf: usize,
    rcv_timeout: u64,
    snd_timeout: u64,
}

impl Default for SocketOptionsState {
    fn default() -> Self {
        Self {
            reuse_addr: false,
            reuse_port: false,
            keep_alive: false,
            no_delay: false,
            rcv_buf: 65536,
            snd_buf: 65536,
            rcv_timeout: 0,
            snd_timeout: 0,
        }
    }
}

/// `setsockopt(2)` — Soket seçeneğini ayarlar.
///
/// SO_REUSEADDR, SO_KEEPALIVE, SO_RCVBUF, SO_SNDBUF gibi
/// temel soket seçeneklerini destekler.
pub fn setsockopt(socket_id: u32, option: SocketOption) -> Result<(), NetError> {
    let mut opts_map = SOCKET_OPTIONS.lock();
    let opts = opts_map
        .entry(socket_id)
        .or_insert_with(SocketOptionsState::default);

    match option {
        SocketOption::ReuseAddr => {
            opts.reuse_addr = true;
            crate::serial_println!("[SOCKET] setsockopt({}, SO_REUSEADDR=1)", socket_id);
        }
        SocketOption::ReusePort => {
            opts.reuse_port = true;
            crate::serial_println!("[SOCKET] setsockopt({}, SO_REUSEPORT=1)", socket_id);
        }
        SocketOption::KeepAlive => {
            opts.keep_alive = true;
            crate::serial_println!("[SOCKET] setsockopt({}, SO_KEEPALIVE=1)", socket_id);
        }
        SocketOption::NoDelay => {
            opts.no_delay = true;
            crate::serial_println!("[SOCKET] setsockopt({}, TCP_NODELAY=1)", socket_id);
        }
        SocketOption::RcvBuf(size) => {
            opts.rcv_buf = size;
            crate::serial_println!("[SOCKET] setsockopt({}, SO_RCVBUF={})", socket_id, size);
        }
        SocketOption::SndBuf(size) => {
            opts.snd_buf = size;
            crate::serial_println!("[SOCKET] setsockopt({}, SO_SNDBUF={})", socket_id, size);
        }
        SocketOption::RcvTimeout(t) => {
            opts.rcv_timeout = t;
            crate::serial_println!("[SOCKET] setsockopt({}, SO_RCVTIMEO={}ms)", socket_id, t);
        }
        SocketOption::SndTimeout(t) => {
            opts.snd_timeout = t;
            crate::serial_println!("[SOCKET] setsockopt({}, SO_SNDTIMEO={}ms)", socket_id, t);
        }
    }
    Ok(())
}

/// `getsockopt(2)` — Soket seçeneğini okur.
///
/// Belirtilen seçeneğin mevcut değerini döndürür.
pub fn getsockopt(socket_id: u32, option: SocketOption) -> Result<usize, NetError> {
    let opts_map = SOCKET_OPTIONS.lock();
    let opts = opts_map.get(&socket_id).cloned().unwrap_or_default();

    match option {
        SocketOption::ReuseAddr => Ok(if opts.reuse_addr { 1 } else { 0 }),
        SocketOption::ReusePort => Ok(if opts.reuse_port { 1 } else { 0 }),
        SocketOption::KeepAlive => Ok(if opts.keep_alive { 1 } else { 0 }),
        SocketOption::NoDelay => Ok(if opts.no_delay { 1 } else { 0 }),
        SocketOption::RcvBuf(_) => Ok(opts.rcv_buf),
        SocketOption::SndBuf(_) => Ok(opts.snd_buf),
        SocketOption::RcvTimeout(_) => Ok(opts.rcv_timeout as usize),
        SocketOption::SndTimeout(_) => Ok(opts.snd_timeout as usize),
    }
}

/// `shutdown(2)` — Soketin gönderim ve/veya alım yarısını kapatır.
///
/// `how` değerleri: 0=alımı kapat (SHUT_RD), 1=gönderimi kapat (SHUT_WR), 2=her ikisini kapat (SHUT_RDWR).
/// TCP bağlantısında FIN gönderir.
pub fn shutdown(socket_id: u32, how: i32) -> Result<(), NetError> {
    match how {
        0 => {
            // SHUT_RD: Alım yarısını kapat — gelen veriler atılır
            crate::serial_println!("[SOCKET] shutdown({}, SHUT_RD)", socket_id);
            Ok(())
        }
        1 => {
            // SHUT_WR: Gönderim yarısını kapat — FIN gönder
            crate::serial_println!("[SOCKET] shutdown({}, SHUT_WR)", socket_id);
            tcp::close(socket_id)
        }
        2 => {
            // SHUT_RDWR: Her ikisini kapat
            crate::serial_println!("[SOCKET] shutdown({}, SHUT_RDWR)", socket_id);
            tcp::close(socket_id)
        }
        _ => Err(NetError::InvalidFd),
    }
}

/// `getsockname(2)` — Soketin bağlı olduğu yerel adresi döndürür.
///
/// TCP/UDP katmanından yerel adresi sorgular.
pub fn getsockname(socket_id: u32) -> Result<SocketAddr, NetError> {
    if has_raw_socket(socket_id) {
        let sockets = RAW_SOCKETS.lock();
        let socket = sockets.get(&socket_id).ok_or(NetError::InvalidFd)?;
        let default_ip = match socket.family {
            AddressFamily::IPV6 => IpAddr::V6(ipv6::Ipv6Addr::UNSPECIFIED),
            _ => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        };
        return Ok(SocketAddr::new(
            socket.bound_ip.unwrap_or(default_ip),
            Port(0),
        ));
    }

    // TCP bağlantılarında yerel adres
    if let Ok(addr) = tcp::get_connection_local_addr(socket_id) {
        return Ok(addr);
    }
    // Bind edilmiş UDP soketleri için
    // Varsayılan: 0.0.0.0:0
    Ok(SocketAddr::default())
}

/// `getpeername(2)` — Bağlı olduğumuz uzak tarafın adresini döndürür.
///
/// Yalnızca bağlı TCP soketleri için geçerlidir.
pub fn getpeername(socket_id: u32) -> Result<SocketAddr, NetError> {
    if has_raw_socket(socket_id) {
        let sockets = RAW_SOCKETS.lock();
        let socket = sockets.get(&socket_id).ok_or(NetError::InvalidFd)?;
        let peer = socket.peer.ok_or(NetError::NotConnected)?;
        return Ok(SocketAddr::new(peer, Port(0)));
    }

    if let Ok(addr) = tcp::get_connection_remote_addr(socket_id) {
        return Ok(addr);
    }
    Err(NetError::NotConnected)
}

// ============================================================================
// OLAY TABANLI G/Ç (EVENT-DRIVEN I/O — select/poll/epoll)
// ============================================================================
//
// Çok sayıda soketi aynı anda izlemek için kullanılan mekanizmalar:
//
//   select()  : En eski POSIX yöntemi. FD kümelerini bit dizisiyle temsil eder.
//               FD sayısı `FD_SETSIZE` (genellikle 1024) ile sınırlıdır.
//
//   poll()    : select'ten daha yeni; `PollFd` dizisi kullanır.
//               FD sayısı sınırsızdır ama her çağrıda tüm liste taranır O(n).
//
//   epoll()   : Linux'a özgü en modern yöntem. Kernel tarafında olay listesi
//               tutulur; yalnızca hazır FD'ler döner O(1). Yüksek performanslı
//               sunucularda (nginx, Node.js) kullanılır.

/// poll() için olay bayrakları.
///
/// Bu sabitler Linux `<poll.h>` başlık dosyasındaki değerlerle aynıdır.
/// Bit maskesi olarak OR ile birleştirilir: `POLLIN | POLLOUT`
pub const POLLIN: u16 = 0x001; // Readable
pub const POLLPRI: u16 = 0x002; // Priority data
pub const POLLOUT: u16 = 0x004; // Writable
pub const POLLERR: u16 = 0x008; // Error
pub const POLLHUP: u16 = 0x010; // Hung up
pub const POLLNVAL: u16 = 0x020; // Invalid request

/// `poll()` fonksiyonu için izlenecek dosya tanımlayıcısını temsil eder.
///
/// - `fd`      : İzlenecek soket kimliği
/// - `events`  : Giriş: hangi olayları izlemek istiyoruz?
/// - `revents` : Çıkış: hangi olaylar gerçekleşti? (çekirdek tarafından doldurulur)
#[derive(Clone, Copy, Debug)]
pub struct PollFd {
    pub fd: i32,
    pub events: u16,  // Input: events to watch
    pub revents: u16, // Output: events that occurred
}

impl PollFd {
    /// Yeni bir `PollFd` oluşturur. `revents` sıfırla başlar; kernel doldurur.
    pub fn new(fd: i32, events: u16) -> Self {
        PollFd {
            fd,
            events,
            revents: 0,
        }
    }
}

/// `poll(2)` — Birden fazla soketi olay için izler.
///
/// Herhangi bir soket hazır olana ya da `timeout_ms` geçene kadar bekler.
/// `timeout_ms < 0` ise süresiz bekler. Dönen değer hazır soket sayısıdır.
///
/// Her döngü adımında tüm FD'ler kontrol edilir; bu O(n) karmaşıklığıdır.
/// Yüksek yük altında `epoll` tercih edilmelidir.
pub fn poll(fds: &mut [PollFd], timeout_ms: i32) -> Result<i32, NetError> {
    let mut ready_count = 0i32;
    let start_time = crate::interrupts::get_ticks();

    loop {
        for fd in fds.iter_mut() {
            fd.revents = 0;

            let socket_id = fd.fd as u32;

            // Check for readability
            if fd.events & POLLIN != 0 {
                if can_read(socket_id) {
                    fd.revents |= POLLIN;
                }
            }

            // Check for writability
            if fd.events & POLLOUT != 0 {
                if can_write(socket_id) {
                    fd.revents |= POLLOUT;
                }
            }

            // Check for errors/hangup
            if is_hungup(socket_id) {
                fd.revents |= POLLHUP;
            }

            if has_error(socket_id) {
                fd.revents |= POLLERR;
            }

            if fd.revents != 0 {
                ready_count += 1;
            }
        }

        if ready_count > 0 {
            return Ok(ready_count);
        }

        // Check timeout
        if timeout_ms >= 0 {
            let elapsed = crate::interrupts::get_ticks() - start_time;
            if elapsed >= timeout_ms as u64 {
                return Ok(0); // Timeout
            }
        }

        // Yield CPU
        crate::task::scheduler::schedule();
    }
}

/// `select(2)` — Birden fazla FD'yi bit kümeleri üzerinden izler.
///
/// `readfds`, `writefds`, `exceptfds` dizileri birer `fd_set` bit maskesidir.
/// FD n için: `byte = n / 8`, `bit = n % 8` konumundaki bit kontrol edilir.
/// Hazır olmayan FD'lerin bitleri sıfırlanır; hazır olanlar korunur.
/// Dönen değer hazır FD sayısıdır.
pub fn select(
    nfds: i32,
    readfds: &mut [u8],
    writefds: &mut [u8],
    exceptfds: &mut [u8],
    timeout_ms: Option<i32>,
) -> Result<i32, NetError> {
    let mut ready_count = 0i32;
    let start_time = crate::interrupts::get_ticks();

    loop {
        // Check readfds
        for fd in 0..nfds {
            let byte_idx = (fd / 8) as usize;
            let bit_idx = (fd % 8) as usize;

            if byte_idx < readfds.len() {
                if readfds[byte_idx] & (1 << bit_idx) != 0 {
                    if can_read(fd as u32) {
                        // Already set
                    } else {
                        readfds[byte_idx] &= !(1 << bit_idx);
                    }
                }
            }

            if byte_idx < writefds.len() {
                if writefds[byte_idx] & (1 << bit_idx) != 0 {
                    if can_write(fd as u32) {
                        // Already set
                    } else {
                        writefds[byte_idx] &= !(1 << bit_idx);
                    }
                }
            }

            if byte_idx < exceptfds.len() {
                if exceptfds[byte_idx] & (1 << bit_idx) != 0 {
                    if has_error(fd as u32) {
                        // Already set
                    } else {
                        exceptfds[byte_idx] &= !(1 << bit_idx);
                    }
                }
            }
        }

        // Count ready FDs
        ready_count = 0;
        for fd in 0..nfds {
            let byte_idx = (fd / 8) as usize;
            let bit_idx = (fd % 8) as usize;

            if byte_idx < readfds.len() && readfds[byte_idx] & (1 << bit_idx) != 0 {
                ready_count += 1;
            }
            if byte_idx < writefds.len() && writefds[byte_idx] & (1 << bit_idx) != 0 {
                ready_count += 1;
            }
            if byte_idx < exceptfds.len() && exceptfds[byte_idx] & (1 << bit_idx) != 0 {
                ready_count += 1;
            }
        }

        if ready_count > 0 {
            return Ok(ready_count);
        }

        // Check timeout
        if let Some(timeout) = timeout_ms {
            if timeout >= 0 {
                let elapsed = crate::interrupts::get_ticks() - start_time;
                if elapsed >= timeout as u64 {
                    return Ok(0);
                }
            }
        }

        crate::task::scheduler::schedule();
    }
}

// ============================================================================
// EPOLL — Olay Tabanlı G/Ç (Linux'a Özgü Yüksek Performanslı Mekanizma)
// ============================================================================
//
// epoll, büyük sayıda soketi O(1) karmaşıklıkla izler.
// select/poll'dan farkı: çekirdek olay listesini bakımını yapar; uygulama
// yalnızca yeni olayları döndüren `epoll_wait()` ile bloklar.
//
// Kullanım akışı:
//   epoll_create() → epoll_ctl(ADD, fd) → loop { epoll_wait() }

/// epoll_ctl() işlem kodları.
///
/// - `EPOLL_CTL_ADD` (1): FD'yi izleme listesine ekle
/// - `EPOLL_CTL_DEL` (2): FD'yi izleme listesinden çıkar
/// - `EPOLL_CTL_MOD` (3): FD'nin izlenen olaylarını güncelle
pub const EPOLL_CTL_ADD: i32 = 1;
pub const EPOLL_CTL_DEL: i32 = 2;
pub const EPOLL_CTL_MOD: i32 = 3;

/// epoll olay bayrakları.
///
/// - `EPOLLIN`  : Okunacak veri var
/// - `EPOLLOUT` : Yazılabilir (gönderim tamponu boş)
/// - `EPOLLERR` : Hata oluştu (kapalı bağlantı vb.)
/// - `EPOLLHUP` : Karşı taraf bağlantıyı kapattı
/// - `EPOLLET`  : Kenar tetiklemeli mod (edge-triggered);
///                yalnızca durum değiştiğinde bildirim gelir (seviye değil)
pub const EPOLLIN: u32 = 0x001;
pub const EPOLLOUT: u32 = 0x004;
pub const EPOLLERR: u32 = 0x008;
pub const EPOLLHUP: u32 = 0x010;
pub const EPOLLET: u32 = 0x80000000; // Edge-triggered

/// epoll olay yapısı. `#[repr(C)]` Linux ABI uyumluluğunu garantiler.
///
/// - `events` : Hangi olayların gerçekleştiği (bit maskesi)
/// - `data`   : Kullanıcı tanımlı veri (genellikle FD veya işaretçi)
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct EpollEvent {
    pub events: u32,
    pub data: u64, // User data
}

/// Bir epoll örneği; izlenen FD'leri ve olaylarını tutar.
///
/// - `id`     : Bu örneğin benzersiz kimliği
/// - `events` : FD → EpollEvent eşlemesi (BTreeMap ile deterministik sıra)
pub struct EpollInstance {
    pub id: u32,
    pub events: alloc::collections::BTreeMap<i32, EpollEvent>,
}

// Global epoll örnekleri. Spinlock korumalı BTreeMap; çok çekirdekli güvenlik.
static EPOLL_INSTANCES: spin::Mutex<alloc::collections::BTreeMap<u32, EpollInstance>> =
    spin::Mutex::new(alloc::collections::BTreeMap::new());
// Her yeni epoll örneği için artan kimlik sayacı (atomik, kilit gerektirmez).
static EPOLL_NEXT_ID: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(1);

/// `epoll_create1(2)` — Yeni bir epoll örneği oluşturur.
///
/// `size` parametresi Linux 2.6.8'den itibaren yok sayılır; yine de geçirilmeli.
/// Dönen değer epoll dosya tanımlayıcısıdır (burada u32 tabanlı id).
pub fn epoll_create(size: i32) -> Result<i32, NetError> {
    let _ = size; // Ignored since Linux 2.6.8
    let id = EPOLL_NEXT_ID.fetch_add(1, Ordering::SeqCst);

    let instance = EpollInstance {
        id,
        events: alloc::collections::BTreeMap::new(),
    };

    EPOLL_INSTANCES.lock().insert(id, instance);
    Ok(id as i32)
}

/// `epoll_ctl(2)` — Epoll örneğini kontrol eder (ekle/sil/güncelle).
///
/// `epfd`: `epoll_create` sonucu, `op`: CTL_ADD/DEL/MOD, `fd`: izlenecek soket.
pub fn epoll_ctl(epfd: i32, op: i32, fd: i32, event: &EpollEvent) -> Result<(), NetError> {
    let mut instances = EPOLL_INSTANCES.lock();
    let instance = instances
        .get_mut(&(epfd as u32))
        .ok_or(NetError::InvalidFd)?;

    match op {
        EPOLL_CTL_ADD => {
            instance.events.insert(fd, *event);
        }
        EPOLL_CTL_DEL => {
            instance.events.remove(&fd);
        }
        EPOLL_CTL_MOD => {
            instance.events.insert(fd, *event);
        }
        _ => return Err(NetError::InvalidParam),
    }

    Ok(())
}

/// `epoll_wait(2)` — Herhangi bir izlenen FD hazır olana kadar bekler.
///
/// Hazır olan olaylar `events` dizisine yazılır; dönen değer olay sayısıdır.
/// `timeout_ms < 0` ise süresiz bekler; `timeout_ms = 0` ise hemen döner.
///
/// Her döngüde kilit alınıp bırakılır; bu CPU cache'i zorlar.
/// Gerçek implementasyonda çekirdek olay kuyruğu (waitqueue) kullanılmalıdır.
pub fn epoll_wait(
    epfd: i32,
    events: &mut [EpollEvent],
    max_events: i32,
    timeout_ms: i32,
) -> Result<i32, NetError> {
    if events.len() < max_events as usize {
        return Err(NetError::BufferFull);
    }

    let start_time = crate::interrupts::get_ticks();
    let mut ready_count = 0i32;

    loop {
        let instances = EPOLL_INSTANCES.lock();
        let instance = instances.get(&(epfd as u32)).ok_or(NetError::InvalidFd)?;

        for (&fd, &event) in &instance.events {
            if ready_count >= max_events {
                break;
            }

            let mut revents = 0u32;

            if event.events & EPOLLIN != 0 && can_read(fd as u32) {
                revents |= EPOLLIN;
            }
            if event.events & EPOLLOUT != 0 && can_write(fd as u32) {
                revents |= EPOLLOUT;
            }
            if has_error(fd as u32) {
                revents |= EPOLLERR;
            }
            if is_hungup(fd as u32) {
                revents |= EPOLLHUP;
            }

            if revents != 0 {
                events[ready_count as usize] = EpollEvent {
                    events: revents,
                    data: event.data,
                };
                ready_count += 1;
            }
        }

        drop(instances);

        if ready_count > 0 {
            return Ok(ready_count);
        }

        // Check timeout
        if timeout_ms >= 0 {
            let elapsed = crate::interrupts::get_ticks() - start_time;
            if elapsed >= timeout_ms as u64 {
                return Ok(0);
            }
        }

        crate::task::scheduler::schedule();
    }
}

/// `epoll_close` — Epoll örneğini kapatır ve hafızayı serbest bırakır.
pub fn epoll_close(epfd: i32) -> Result<(), NetError> {
    EPOLL_INSTANCES.lock().remove(&(epfd as u32));
    Ok(())
}
// ============================================================================
// OLAY KONTROL YARDIMCI FONKSİYONLARI (HELPER FUNCTIONS FOR EVENT CHECKING)
// ============================================================================
//
// Bu fonksiyonlar poll/select/epoll mekanizmalarının soket durumunu sorgulamak
// için kullandığı dahili yardımcılardır. TCP ve UDP katmanlarını soyutlar.

/// Sokette okunacak veri var mı?
///
/// TCP için: RX tamponu dolu VEYA bağlantı kapanmış (CloseWait/Closed).
/// CloseWait ve Closed durumları da "okunabilir" sayılır çünkü uygulamanın
/// 0 bayt (EOF) okuması ve bağlantıyı kapatması gerekir.
/// UDP için: alım tamponu boş değilse okunabilir.
pub fn can_read(socket_id: u32) -> bool {
    if let Some(raw) = RAW_SOCKETS.lock().get(&socket_id) {
        return !raw.rx_queue.is_empty();
    }

    // Try TCP first, then UDP
    if let Some(conn) = tcp::get_connection(socket_id) {
        return !conn.rx_buffer.is_empty()
            || conn.state == tcp::TcpState::CloseWait
            || conn.state == tcp::TcpState::Closed;
    }

    if let Some(sock) = udp::get_socket(socket_id) {
        return !sock.rx_buffer.is_empty();
    }

    false
}

/// Sokete veri yazılabilir mi?
///
/// TCP için: yalnızca `Established` durumunda yazılabilir.
/// SYN_SENT, CLOSE_WAIT gibi ara durumlarda yazma bloklanır.
/// UDP için: bağlantısız olduğundan her zaman yazılabilirdir.
pub fn can_write(socket_id: u32) -> bool {
    if RAW_SOCKETS.lock().contains_key(&socket_id) {
        return true;
    }

    if let Some(conn) = tcp::get_connection(socket_id) {
        return conn.state == tcp::TcpState::Established;
    }

    // UDP is always writable
    if udp::get_socket(socket_id).is_some() {
        return true;
    }

    false
}

/// Sokette hata durumu var mı?
///
/// TCP bağlantısı tamamen kapanmışsa (Closed) hata olarak raporlanır.
/// Bu durum karşı tarafın RST göndermesiyle veya timeout ile oluşabilir.
fn has_error(socket_id: u32) -> bool {
    if let Some(conn) = tcp::get_connection(socket_id) {
        return conn.state == tcp::TcpState::Closed;
    }
    false
}

/// Bağlantı karşı taraf tarafından mı kapatıldı?
///
/// - `CloseWait`: Karşı taraf FIN gönderdi; uygulama henüz kapanmadı.
/// - `TimeWait` : Bağlantı kapanıyor; 2*MSL (Maximum Segment Lifetime) bekleniyor.
///
/// Bu durumlar POLLHUP veya EPOLLHUP olarak raporlanır.
fn is_hungup(socket_id: u32) -> bool {
    if let Some(conn) = tcp::get_connection(socket_id) {
        return conn.state == tcp::TcpState::CloseWait || conn.state == tcp::TcpState::TimeWait;
    }
    false
}

// ============================================================================
// YARDIMCI FONKSİYONLAR (HELPER FUNCTIONS)
// ============================================================================

/// IPv4 adresi dizesini (örn. "192.168.1.1") `Ipv4Addr` yapısına çevirir.
///
/// Ayrıştırma adımları: noktayla böl → 4 parça kontrol et → her parçayı u8'e çevir.
/// Geçersiz giriş için `None` döner (parse hataları `?` ile zincir kırılır).
pub fn parse_ipv4(s: &str) -> Option<Ipv4Addr> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 4 {
        return None;
    }

    let mut bytes = [0u8; 4];
    for (i, part) in parts.iter().enumerate() {
        bytes[i] = part.parse().ok()?;
    }

    Some(Ipv4Addr::from_bytes(bytes))
}

/// `Ipv4Addr` yapısını noktalı ondalık gösterime (dotted decimal) dönüştürür.
///
/// Örnek: Ipv4Addr([192, 168, 1, 1]) → "192.168.1.1"
/// `alloc::format!` kullanır çünkü no_std ortamında `std::format!` yoktur.
pub fn format_ipv4(ip: Ipv4Addr) -> alloc::string::String {
    alloc::format!("{}.{}.{}.{}", ip.0[0], ip.0[1], ip.0[2], ip.0[3])
}

pub fn format_ipaddr(ip: IpAddr) -> alloc::string::String {
    alloc::format!("{}", ip)
}

/// Port numarası dizesini `Port` sarmalayıcı türüne çevirir.
///
/// Geçerli port aralığı: 0–65535 (u16). Bunun dışındaki değerler `None` döner.
/// 0–1023 arası well-known portlardır; root yetkisi gerektirir (Linux'ta).
pub fn parse_port(s: &str) -> Option<Port> {
    let port: u16 = s.parse().ok()?;
    Some(Port(port))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_socket_send_wraps_payload_and_recv_delivers_ipv4_packet() {
        super::super::ensure_loopback_interface_for_tests();
        let sock = socket(AddressFamily::IPV4, SocketType::RAW, Protocol::ICMP).unwrap();
        bind(sock, SocketAddr::new(Ipv4Addr::new(10, 0, 2, 15), Port(0))).unwrap();
        connect(sock, SocketAddr::new(Ipv4Addr::new(1, 1, 1, 1), Port(0))).unwrap();

        let payload = [8u8, 0, 0, 0, 0, 1, 0, 1];
        let sent = send(sock, &payload, 0).unwrap();
        assert_eq!(sent, payload.len());

        let mut frame = vec![0u8; 128];
        let packet = Ipv4Packet::new(
            Ipv4Addr::new(1, 1, 1, 1),
            Ipv4Addr::new(10, 0, 2, 15),
            IpProtocol::ICMP,
            &payload,
        );
        let len = packet.serialize(&mut frame).unwrap();
        deliver_raw_ipv4(&frame[..len], &packet.header);

        let mut recv_buf = [0u8; 128];
        let (recv_len, src) = recvfrom(sock, &mut recv_buf, 0).unwrap();
        assert_eq!(src.ip, IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)));
        let recv_packet = Ipv4Packet::parse(&recv_buf[..recv_len]).unwrap();
        assert_eq!(recv_packet.header.protocol, IpProtocol::ICMP);
        assert_eq!(recv_packet.payload, payload);
    }

    #[test]
    fn raw_socket_recv_delivers_ipv6_packet() {
        let sock = socket(AddressFamily::IPV6, SocketType::RAW, Protocol::UDP).unwrap();
        bind(
            sock,
            SocketAddr::new(
                super::super::ipv6::Ipv6Addr::from_segments([0xfe80, 0, 0, 0, 0, 0, 0, 1]),
                Port(0),
            ),
        )
        .unwrap();
        connect(
            sock,
            SocketAddr::new(
                super::super::ipv6::Ipv6Addr::from_segments([0x2001, 0xdb8, 0, 0, 0, 0, 0, 2]),
                Port(0),
            ),
        )
        .unwrap();

        let payload = [0xdeu8, 0xad, 0xbe, 0xef];
        let packet = super::ipv6::Ipv6Packet::new(
            super::ipv6::Ipv6Header::new(
                super::super::ipv6::Ipv6Addr::from_segments([0x2001, 0xdb8, 0, 0, 0, 0, 0, 2]),
                super::super::ipv6::Ipv6Addr::from_segments([0xfe80, 0, 0, 0, 0, 0, 0, 1]),
                super::ipv6::Ipv6NextHeader::Udp as u8,
                payload.len() as u16,
            ),
            &payload,
        );
        let serialized = packet.serialize();
        deliver_raw_ipv6(&serialized, &packet.header);

        let mut recv_buf = [0u8; 128];
        let (recv_len, src) = recvfrom(sock, &mut recv_buf, 0).unwrap();
        assert_eq!(
            src.ip,
            IpAddr::V6(super::super::ipv6::Ipv6Addr::from_segments([
                0x2001, 0xdb8, 0, 0, 0, 0, 0, 2,
            ]))
        );
        let recv_packet = super::ipv6::Ipv6Packet::parse(&recv_buf[..recv_len]).unwrap();
        assert_eq!(
            recv_packet.header.next_header,
            super::ipv6::Ipv6NextHeader::Udp as u8
        );
        assert_eq!(recv_packet.payload, payload);
    }
}
