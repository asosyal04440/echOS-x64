//! # Unix Domain Socket (AF_UNIX)
//!
//! Yerel işlemler arası iletişim (IPC) için Unix domain socket implementasyonu.
//! `AF_UNIX` / `AF_LOCAL` adres ailesi, dosya sistemi yolu veya soyut isimle bağlanır.
//!
//! ## Desteklenen Tipler
//!
//! - `SOCK_STREAM`: Bağlantı tabanlı, güvenilir bayt akışı
//! - `SOCK_DGRAM`: Bağlantısız datagram
//! - `SOCK_SEQPACKET`: Bağlantı tabanlı, sıralı paketler
//!
//! ## SCM_RIGHTS — Dosya Tanımlayıcı Aktarımı
//!
//! `sendmsg` / `recvmsg` ile bir süreçten diğerine açık dosya tanımlayıcıları
//! (fd) göndermek mümkündür. Bu, konteyner izolasyonu ve sandboxing için
//! kritik bir mekanizmadır.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// SABITLER
// ============================================================================

/// Unix domain socket adres ailesi
pub const AF_UNIX: u16 = 1;
pub const AF_LOCAL: u16 = AF_UNIX;

/// Socket tipleri
pub const SOCK_STREAM: u32 = 1;
pub const SOCK_DGRAM: u32 = 2;
pub const SOCK_SEQPACKET: u32 = 5;

/// Yardımcı mesaj tipleri (cmsg)
pub const SOL_SOCKET: u32 = 1;
pub const SCM_RIGHTS: u32 = 1; // Dosya tanımlayıcı aktarımı
pub const SCM_CREDENTIALS: u32 = 2; // İşlem kimlik bilgisi

/// Maksimum yol uzunluğu
pub const UNIX_PATH_MAX: usize = 108;

/// Maksimum backlog (dinleme kuyruğu boyutu)
pub const SOMAXCONN: usize = 128;

/// Varsayılan buffer boyutu
pub const UNIX_SOCKET_BUF_SIZE: usize = 65536;

// ============================================================================
// Unix Socket Adresi
// ============================================================================

/// Unix domain socket adresi.
///
/// Üç adres türü desteklenir:
/// - Dosya yolu: `/var/run/daemon.sock`
/// - Soyut: `\0abstract-name` (dosya sistemi girişi yok)
/// - İsimsiz: `socketpair()` ile oluşturulan
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum UnixAddr {
    /// Dosya sistemi yolu ile bağlı
    Pathname(String),
    /// Soyut isimle bağlı (Linux'a özgü)
    Abstract(String),
    /// İsimsiz (socketpair)
    Unnamed,
}

impl UnixAddr {
    /// Bayt dizisinden adres oluşturur.
    pub fn from_bytes(data: &[u8]) -> Self {
        if data.is_empty() {
            return UnixAddr::Unnamed;
        }
        if data[0] == 0 && data.len() > 1 {
            // Soyut namespace
            let name = core::str::from_utf8(&data[1..])
                .unwrap_or("unknown")
                .trim_end_matches('\0');
            UnixAddr::Abstract(String::from(name))
        } else {
            let path = core::str::from_utf8(data)
                .unwrap_or("")
                .trim_end_matches('\0');
            UnixAddr::Pathname(String::from(path))
        }
    }

    /// Adres ismini döner.
    pub fn name(&self) -> &str {
        match self {
            UnixAddr::Pathname(p) => p.as_str(),
            UnixAddr::Abstract(a) => a.as_str(),
            UnixAddr::Unnamed => "(unnamed)",
        }
    }
}

// ============================================================================
// Unix Socket Durumu
// ============================================================================

/// Socket bağlantı durumu
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnixSocketState {
    /// Oluşturuldu ama bağlanmadı
    Created,
    /// Adrese bağlandı (bind)
    Bound,
    /// Bağlantı dinliyor (listen)
    Listening,
    /// Bağlantı kuruldu
    Connected,
    /// Bağlantı kapatılıyor
    Closing,
    /// Kapatıldı
    Closed,
}

// ============================================================================
// SCM_RIGHTS — Dosya Tanımlayıcı Aktarımı
// ============================================================================

/// Yardımcı mesaj (ancillary data) — cmsg.
///
/// `sendmsg()` / `recvmsg()` ile kontrol verisi aktarımı.
#[derive(Debug, Clone)]
pub struct CmsgHdr {
    /// Mesaj seviyesi (SOL_SOCKET vb.)
    pub cmsg_level: u32,
    /// Mesaj tipi (SCM_RIGHTS, SCM_CREDENTIALS)
    pub cmsg_type: u32,
    /// Veri
    pub data: Vec<u8>,
}

/// SCM_CREDENTIALS yapısı (ucred)
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Ucred {
    pub pid: u32,
    pub uid: u32,
    pub gid: u32,
}

/// Aktarılacak dosya tanımlayıcı listesi
#[derive(Debug, Clone)]
pub struct ScmRights {
    /// Aktarılan fd'ler
    pub fds: Vec<i32>,
}

impl ScmRights {
    /// Yeni SCM_RIGHTS mesajı oluşturur.
    pub fn new(fds: Vec<i32>) -> Self {
        Self { fds }
    }

    /// cmsg formatına serileştirir.
    pub fn to_cmsg(&self) -> CmsgHdr {
        let mut data = Vec::new();
        for &fd in &self.fds {
            data.extend_from_slice(&fd.to_le_bytes());
        }
        CmsgHdr {
            cmsg_level: SOL_SOCKET,
            cmsg_type: SCM_RIGHTS,
            data,
        }
    }

    /// cmsg'den deserileştirir.
    pub fn from_cmsg(cmsg: &CmsgHdr) -> Option<Self> {
        if cmsg.cmsg_type != SCM_RIGHTS {
            return None;
        }
        let mut fds = Vec::new();
        for chunk in cmsg.data.chunks(4) {
            if chunk.len() == 4 {
                fds.push(i32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
            }
        }
        Some(Self { fds })
    }
}

// ============================================================================
// Unix Socket
// ============================================================================

/// Unix domain socket.
///
/// Her socket, bir iletişim uç noktasıdır. `SOCK_STREAM` için bağlantı
/// kurulması, `SOCK_DGRAM` için doğrudan datagram gönderi mümkündür.
pub struct UnixSocket {
    /// Socket kimliği (global benzersiz)
    pub id: u64,
    /// Socket tipi
    pub sock_type: u32,
    /// Mevcut durum
    pub state: UnixSocketState,
    /// Bağlı adres (bind sonrası)
    pub local_addr: UnixAddr,
    /// Karşı taraf adresi (connect sonrası)
    pub remote_addr: UnixAddr,
    /// Alım tamponu
    pub recv_buffer: Vec<u8>,
    /// Gönderim tamponu
    pub send_buffer: Vec<u8>,
    /// Alım tamponu sınırı
    pub recv_buf_size: usize,
    /// Gönderim tamponu sınırı
    pub send_buf_size: usize,
    /// Yardımcı mesaj kuyruğu (SCM_RIGHTS vb.)
    pub pending_cmsg: Vec<CmsgHdr>,
    /// Bağlantı bekleme kuyruğu (listen için)
    pub backlog: Vec<u64>, // pending socket IDs
    /// Maksimum backlog
    pub max_backlog: usize,
    /// Bağlı çift (socketpair veya accept sonucu)
    pub peer_id: Option<u64>,
    /// Non-blocking mod
    pub nonblocking: bool,
    /// Kapatıldı mı
    pub closed: bool,
    /// Kimlik bilgileri
    pub credentials: Ucred,
    /// İstatistikler
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub msgs_sent: u64,
    pub msgs_received: u64,
}

impl UnixSocket {
    /// Yeni Unix domain socket oluşturur.
    pub fn new(id: u64, sock_type: u32) -> Self {
        Self {
            id,
            sock_type,
            state: UnixSocketState::Created,
            local_addr: UnixAddr::Unnamed,
            remote_addr: UnixAddr::Unnamed,
            recv_buffer: Vec::new(),
            send_buffer: Vec::new(),
            recv_buf_size: UNIX_SOCKET_BUF_SIZE,
            send_buf_size: UNIX_SOCKET_BUF_SIZE,
            pending_cmsg: Vec::new(),
            backlog: Vec::new(),
            max_backlog: SOMAXCONN,
            peer_id: None,
            nonblocking: false,
            closed: false,
            credentials: Ucred {
                pid: 0,
                uid: 0,
                gid: 0,
            },
            bytes_sent: 0,
            bytes_received: 0,
            msgs_sent: 0,
            msgs_received: 0,
        }
    }

    /// Adrese bağlar.
    pub fn bind(&mut self, addr: UnixAddr) -> Result<(), i32> {
        if self.state != UnixSocketState::Created {
            return Err(-22); // EINVAL
        }

        // Aynı adres zaten kullanılıyor mu?
        let sockets = UNIX_SOCKETS.lock();
        for (_id, sock) in sockets.iter() {
            if sock.local_addr == addr && !sock.closed {
                return Err(-98); // EADDRINUSE
            }
        }
        drop(sockets);

        self.local_addr = addr;
        self.state = UnixSocketState::Bound;
        Ok(())
    }

    /// Dinlemeye başlar (SOCK_STREAM / SOCK_SEQPACKET).
    pub fn listen(&mut self, backlog: usize) -> Result<(), i32> {
        if self.sock_type == SOCK_DGRAM {
            return Err(-95); // EOPNOTSUPP
        }
        if self.state != UnixSocketState::Bound {
            return Err(-22); // EINVAL
        }
        self.max_backlog = backlog.min(SOMAXCONN);
        self.state = UnixSocketState::Listening;
        Ok(())
    }

    /// Bağlantı kabul eder.
    pub fn accept(&mut self) -> Option<u64> {
        if self.state != UnixSocketState::Listening {
            return None;
        }
        self.backlog.pop()
    }

    /// Karşı tarafa bağlanır.
    pub fn connect(&mut self, remote_addr: &UnixAddr) -> Result<(), i32> {
        if self.state == UnixSocketState::Connected {
            return Err(-106); // EISCONN
        }

        // Dinleyen socket bul
        let mut sockets = UNIX_SOCKETS.lock();
        let listener_id = sockets
            .iter()
            .find(|(_, s)| s.local_addr == *remote_addr && s.state == UnixSocketState::Listening)
            .map(|(id, _)| *id);

        if let Some(lid) = listener_id {
            // Backlog'a ekle
            if let Some(listener) = sockets.get_mut(&lid) {
                if listener.backlog.len() >= listener.max_backlog {
                    return Err(-111); // ECONNREFUSED (backlog full)
                }
                listener.backlog.push(self.id);
            }

            self.remote_addr = remote_addr.clone();
            self.state = UnixSocketState::Connected;
            self.peer_id = Some(lid);
            Ok(())
        } else {
            Err(-111) // ECONNREFUSED
        }
    }

    /// Veri gönderir.
    pub fn send(&mut self, data: &[u8]) -> Result<usize, i32> {
        if self.sock_type == SOCK_STREAM && self.state != UnixSocketState::Connected {
            return Err(-107); // ENOTCONN
        }
        if self.closed {
            return Err(-32); // EPIPE
        }

        let len = data.len().min(self.send_buf_size);
        self.send_buffer.extend_from_slice(&data[..len]);
        self.bytes_sent += len as u64;
        self.msgs_sent += 1;

        // Karşı tarafa aktar
        if let Some(peer_id) = self.peer_id {
            let mut sockets = UNIX_SOCKETS.lock();
            if let Some(peer) = sockets.get_mut(&peer_id) {
                let avail = peer.recv_buf_size.saturating_sub(peer.recv_buffer.len());
                let transfer = len.min(avail);
                peer.recv_buffer.extend_from_slice(&data[..transfer]);
                peer.bytes_received += transfer as u64;
                peer.msgs_received += 1;
            }
        }

        Ok(len)
    }

    /// Veri alır.
    pub fn recv(&mut self, buf: &mut [u8]) -> Result<usize, i32> {
        if self.sock_type == SOCK_STREAM && self.state != UnixSocketState::Connected {
            return Err(-107); // ENOTCONN
        }

        if self.recv_buffer.is_empty() {
            if self.nonblocking {
                return Err(-11); // EAGAIN
            }
            return Ok(0); // EOF veya bekle
        }

        let len = buf.len().min(self.recv_buffer.len());
        buf[..len].copy_from_slice(&self.recv_buffer[..len]);
        self.recv_buffer.drain(..len);
        Ok(len)
    }

    /// Yardımcı mesajla veri gönderir (SCM_RIGHTS).
    pub fn sendmsg(&mut self, data: &[u8], cmsg: Option<CmsgHdr>) -> Result<usize, i32> {
        if let Some(peer_id) = self.peer_id {
            let mut sockets = UNIX_SOCKETS.lock();
            if let Some(peer) = sockets.get_mut(&peer_id) {
                peer.recv_buffer.extend_from_slice(data);
                if let Some(cm) = cmsg {
                    peer.pending_cmsg.push(cm);
                }
                peer.bytes_received += data.len() as u64;
                Ok(data.len())
            } else {
                Err(-32) // EPIPE
            }
        } else {
            Err(-107) // ENOTCONN
        }
    }

    /// Yardımcı mesajla veri alır.
    pub fn recvmsg(&mut self, buf: &mut [u8]) -> Result<(usize, Option<CmsgHdr>), i32> {
        let len = self.recv(buf)?;
        let cmsg = if !self.pending_cmsg.is_empty() {
            Some(self.pending_cmsg.remove(0))
        } else {
            None
        };
        Ok((len, cmsg))
    }

    /// Socket'i kapatır.
    pub fn close(&mut self) {
        self.state = UnixSocketState::Closed;
        self.closed = true;
        self.send_buffer.clear();
        self.recv_buffer.clear();
    }

    /// Okunabilecek bayt sayısı.
    pub fn bytes_available(&self) -> usize {
        self.recv_buffer.len()
    }
}

// ============================================================================
// Global State
// ============================================================================

lazy_static::lazy_static! {
    /// Global Unix domain socket tablosu
    static ref UNIX_SOCKETS: Mutex<BTreeMap<u64, UnixSocket>> = Mutex::new(BTreeMap::new());
    /// Sonraki socket ID
    static ref NEXT_UNIX_SOCK_ID: AtomicU64 = AtomicU64::new(1);
}

/// Yeni Unix domain socket oluşturur.
pub fn create_socket(sock_type: u32) -> u64 {
    let id = NEXT_UNIX_SOCK_ID.fetch_add(1, Ordering::Relaxed);
    let socket = UnixSocket::new(id, sock_type);
    UNIX_SOCKETS.lock().insert(id, socket);
    id
}

/// Socket çifti oluşturur (socketpair).
///
/// İki bağlı socket döner — IPC'de yaygın kullanılır.
pub fn socketpair(sock_type: u32) -> (u64, u64) {
    let id1 = NEXT_UNIX_SOCK_ID.fetch_add(1, Ordering::Relaxed);
    let id2 = NEXT_UNIX_SOCK_ID.fetch_add(1, Ordering::Relaxed);

    let mut s1 = UnixSocket::new(id1, sock_type);
    let mut s2 = UnixSocket::new(id2, sock_type);

    s1.state = UnixSocketState::Connected;
    s2.state = UnixSocketState::Connected;
    s1.peer_id = Some(id2);
    s2.peer_id = Some(id1);

    let mut sockets = UNIX_SOCKETS.lock();
    sockets.insert(id1, s1);
    sockets.insert(id2, s2);

    (id1, id2)
}

/// Socket'e bağlar.
pub fn bind(sock_id: u64, addr: UnixAddr) -> Result<(), i32> {
    let mut sockets = UNIX_SOCKETS.lock();
    // Check address conflict first, before taking mutable borrow
    let is_in_use = sockets
        .iter()
        .any(|(id, s)| *id != sock_id && s.local_addr == addr && !s.closed);
    if is_in_use {
        return Err(-98);
    }
    if let Some(sock) = sockets.get_mut(&sock_id) {
        sock.local_addr = addr;
        sock.state = UnixSocketState::Bound;
        Ok(())
    } else {
        Err(-9) // EBADF
    }
}

/// Kayıtlı Unix socket sayısı.
pub fn socket_count() -> usize {
    UNIX_SOCKETS.lock().len()
}

/// Modülü başlatır.
pub fn init() {
    crate::serial_println!("[unix-socket] AF_UNIX domain socket alt sistemi hazır");
    crate::serial_println!("[unix-socket] SCM_RIGHTS dosya tanımlayıcı aktarımı destekleniyor");
}
