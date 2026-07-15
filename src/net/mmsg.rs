//! # sendmmsg / recvmmsg — Toplu Mesaj G/Ç (Batch Message I/O)
//!
//! Linux'ta `sendmmsg(2)` ve `recvmmsg(2)` ile tek syscall'da birden fazla
//! datagram göndermek/almak mümkündür. Özellikle yüksek paket/saniye
//! gerektiren uygulamalarda (DNS sunucuları, VoIP, oyun sunucuları) önemli
//! performans kazancı sağlar.
//!
//! ## sendmmsg
//!
//! ```c
//! struct mmsghdr {
//!     struct msghdr msg_hdr;  // standart mesaj başlığı
//!     unsigned int  msg_len;  // dönüş: gönderilen byte sayısı
//! };
//!
//! int sendmmsg(int sockfd, struct mmsghdr *msgvec, unsigned int vlen,
//!              int flags);
//! ```
//!
//! ## recvmmsg
//!
//! ```c
//! struct mmsghdr {
//!     struct msghdr msg_hdr;
//!     unsigned int  msg_len;
//! };
//!
//! int recvmmsg(int sockfd, struct mmsghdr *msgvec, unsigned int vlen,
//!              int flags, struct timespec *timeout);
//! ```
//!
//! ## Avantaj
//!
//! - **Syscall azaltma**: N ayrı sendto() yerine 1 sendmmsg() → %30-50 latency azalması
//! - **NIC batching**: Ağ kartı donanım seviyesinde paket gönderebilir
//! - **CPU cache locality**: Aynı veri yapıları art arda işlenir
//!
//! ## echOS Tasarımı
//!
//! Rust tarafında `MmsgHdr` ve `MmsgResult` yapıları ile toplu batch API.
//! UDP soketler için tam destek; TCP stream soketlerde de sıralı write
//! olarak uygulanır (Linux davranışı).

use super::{udp, NetError, SocketAddr};
use alloc::vec;
use alloc::vec::Vec;

/// `struct iovec` — Parçalı tampon (gönderilecek/alınacak veri)
#[derive(Clone, Debug)]
pub struct Iovec {
    /// Veri tamponu (gönderimde girdi, alımda çıktı)
    pub data: Vec<u8>,
}

impl Iovec {
    pub fn new(capacity: usize) -> Self {
        Iovec {
            data: Vec::with_capacity(capacity),
        }
    }

    pub fn from_slice(s: &[u8]) -> Self {
        Iovec {
            data: s.to_vec(),
        }
    }
}

/// `struct msghdr` — Tek mesaj tanımı
#[derive(Clone, Debug)]
pub struct MsgHdr {
    /// Hedef adres (send için) / kaynak adres (recv için doldurulur)
    pub addr: Option<SocketAddr>,
    /// Gönderilecek/alınacak veri (iovec dizisi)
    pub iovecs: Vec<Iovec>,
    /// Ek kontrol bayrakları (MSG_DONTWAIT, MSG_NOSIGNAL vs.)
    pub flags: u32,
}

impl MsgHdr {
    pub fn new() -> Self {
        MsgHdr {
            addr: None,
            iovecs: Vec::new(),
            flags: 0,
        }
    }

    pub fn with_data(data: &[u8]) -> Self {
        MsgHdr {
            addr: None,
            iovecs: vec![Iovec::from_slice(data)],
            flags: 0,
        }
    }

    pub fn total_len(&self) -> usize {
        self.iovecs.iter().map(|iv| iv.data.len()).sum()
    }
}

/// `struct mmsghdr` — sendmmsg/recvmmsg elemanı
#[derive(Clone, Debug)]
pub struct MmsgHdr {
    pub msg_hdr: MsgHdr,
    /// Dönüş değeri: gönderilen/alınan byte sayısı
    pub msg_len: u32,
}

impl MmsgHdr {
    pub fn new() -> Self {
        MmsgHdr {
            msg_hdr: MsgHdr::new(),
            msg_len: 0,
        }
    }
}

/// sendmmsg sonuç yapısı
#[derive(Clone, Debug)]
pub struct SendMmsgResult {
    /// Gönderilen mesaj sayısı (hata varsa daha az)
    pub sent: usize,
    /// Toplam gönderilen byte
    pub total_bytes: usize,
    /// Her mesaj için hata (None = başarılı)
    pub errors: Vec<Option<NetError>>,
}

/// `sendmmsg(2)` — Tek syscall'da N datagram gönder
///
/// `headers[i].msg_hdr.addr` zorunlu (UDP için).
/// `headers[i].msg_hdr.iovecs` tüm iovec'ler peş peşe gönderilir.
pub fn sendmmsg(socket_id: u32, headers: &mut [MmsgHdr]) -> Result<SendMmsgResult, NetError> {
    if headers.is_empty() {
        return Ok(SendMmsgResult {
            sent: 0,
            total_bytes: 0,
            errors: Vec::new(),
        });
    }

    let mut sent = 0usize;
    let mut total_bytes = 0usize;
    let mut errors = Vec::with_capacity(headers.len());

    for hdr in headers.iter_mut() {
        let dest = hdr.msg_hdr.addr.ok_or(NetError::InvalidArg)?;
        // Iovec'leri ardışık buffer olarak birleştir
        let total_len: usize = hdr.msg_hdr.iovecs.iter().map(|iv| iv.data.len()).sum();
        let mut combined = Vec::with_capacity(total_len);
        for iv in &hdr.msg_hdr.iovecs {
            combined.extend_from_slice(&iv.data);
        }

        match udp::send_to(socket_id, &combined, dest) {
            Ok(n) => {
                hdr.msg_len = n as u32;
                sent += 1;
                total_bytes += n;
                errors.push(None);
            }
            Err(e) => {
                hdr.msg_len = 0;
                errors.push(Some(e));
                // Linux davranışı: ilk hatada dur ve kısmi sonuç döndür
                break;
            }
        }
    }

    Ok(SendMmsgResult {
        sent,
        total_bytes,
        errors,
    })
}

/// recvmmsg sonuç yapısı
#[derive(Clone, Debug)]
pub struct RecvMmsgResult {
    /// Alınan mesaj sayısı
    pub received: usize,
    /// Her mesajın kaynak adresi
    pub addrs: Vec<Option<SocketAddr>>,
    /// Her mesajın uzunluğu
    pub lens: Vec<u32>,
}

/// `recvmmsg(2)` — Tek syscall'da N datagram al
///
/// `headers[i].msg_hdr.iovecs[i].data` alım tamponudur, dönüşte doldurulur.
/// `headers[i].msg_hdr.addr` dönüşte kaynak adresi olarak doldurulur.
pub fn recvmmsg(
    socket_id: u32,
    headers: &mut [MmsgHdr],
    max_per_call: usize,
) -> Result<RecvMmsgResult, NetError> {
    if headers.is_empty() || max_per_call == 0 {
        return Ok(RecvMmsgResult {
            received: 0,
            addrs: Vec::new(),
            lens: Vec::new(),
        });
    }

    let to_recv = headers.len().min(max_per_call);
    let mut received = 0usize;
    let mut addrs = Vec::with_capacity(to_recv);
    let mut lens = Vec::with_capacity(to_recv);

    for hdr in headers.iter_mut().take(to_recv) {
        // İlk iovec'i alım tamponu olarak kullan
        if hdr.msg_hdr.iovecs.is_empty() {
            hdr.msg_hdr.iovecs.push(Iovec::new(2048));
        }
        let buf = &mut hdr.msg_hdr.iovecs[0].data;
        buf.clear();
        // Ensure capacity
        if buf.capacity() < 2048 {
            buf.reserve(2048 - buf.capacity());
        }
        // unsafe gerekmiyor; buf'ı sabit uzunlukta olarak geçici bir &mut [u8] olarak
        // hazırlayıp udp::recv_from'a ver. Pratik olarak buf'ı sıfırla ve güvenli bir
        // &mut [u8] elde etmek için unsafe kullanmadan core::mem::take yapalım:
        let mut tmp = core::mem::take(buf);
        tmp.resize(2048, 0);
        match udp::recv_from_into(socket_id, &mut tmp) {
            Ok((n, addr)) => {
                tmp.truncate(n);
                hdr.msg_hdr.iovecs[0].data = tmp;
                hdr.msg_hdr.addr = Some(addr);
                hdr.msg_len = n as u32;
                received += 1;
                addrs.push(Some(addr));
                lens.push(n as u32);
            }
            Err(e) => {
                hdr.msg_hdr.iovecs[0].data = tmp;
                if received == 0 {
                    return Err(e);
                }
                break;
            }
        }
    }

    Ok(RecvMmsgResult {
        received,
        addrs,
        lens,
    })
}

// ============================================================================
// TESTLER
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn msg_hdr_total_len_sums_iovecs() {
        let mut h = MsgHdr::new();
        h.iovecs.push(Iovec::from_slice(b"hello"));
        h.iovecs.push(Iovec::from_slice(b" world"));
        assert_eq!(h.total_len(), 11);
    }

    #[test]
    fn mmsg_hdr_defaults_zero_len() {
        let m = MmsgHdr::new();
        assert_eq!(m.msg_len, 0);
        assert_eq!(m.msg_hdr.iovecs.len(), 0);
    }

    #[test]
    fn sendmmsg_empty_input_returns_zero() {
        // Boş girdi: erken dönüş yolu, udp::send_to çağrılmaz
        let mut hdrs: Vec<MmsgHdr> = Vec::new();
        let r = sendmmsg(0, &mut hdrs).unwrap();
        assert_eq!(r.sent, 0);
        assert_eq!(r.total_bytes, 0);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn iovec_from_slice_copies_data() {
        let iv = Iovec::from_slice(&[1, 2, 3, 4]);
        assert_eq!(iv.data, vec![1, 2, 3, 4]);
    }
}
