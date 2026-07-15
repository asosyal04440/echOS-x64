//! # sendfile / splice / tee — Dosya → Soket Aktarımı
//!
//! Linux `sendfile(2)`, `splice(2)` ve `tee(2)` sistem çağrıları.
//!
//! ## sendfile(2) Nedir?
//!
//! ```c
//! ssize_t sendfile(int out_fd, int in_fd, off_t *offset, size_t count);
//! ```
//!
//! `in_fd`'den `count` byte'ı okur, doğrudan `out_fd`'ye yazar. Veri
//! kullanıcı buffer'ından geçmez; kernel içinde page cache üzerinden
//! pipe benzeri mekanizmayla taşınır.
//!
//! ## splice(2) Nedir?
//!
//! ```c
//! ssize_t splice(int fd_in, off64_t *off_in, int fd_out, off64_t *off_out,
//!                size_t len, unsigned int flags);
//! ```
//!
//! İki dosya tanımlayıcı arasında veriyi **pipe** üzerinden taşır.
//! Tipik kullanım: dosya → pipe → soket.
//!
//! ## tee(2) Nedir?
//!
//! `tee(2)` iki pipe arasında veriyi **tüketmeden** kopyalar. Kaynak pipe'dan
//! okur, ikinci pipe'a yazar; kaynak pipe'daki veri yerinde kalır.
//!
//! ## Avantaj
//!
//! - **Düşük context switch**: Kullanıcı alanına veri çıkmaz
//! - **Düşük CPU**: 1 kopya (page cache'ten pipe buffer'a); gerçek zero-copy
//!   sayfa referansı ile olur (P2 hedefi)
//! - **DMA hızlandırma**: NIC doğrudan page cache'i okuyabilir
//!
//! ## Sınırlamalar
//!
//! - Kaynak `mmap()` edilebilir olmalı (normal dosya ✓, pipe ✓, soket ✗)
//! - Hedef soket olmalı (pipe → soket ✓)
//! - Out_fd, in_fd'den farklı olmalı (kendi kendine splice yok)
//! - tee yalnızca pipe ↔ pipe arasında çalışır
//!
//! ## echOS Tasarımı
//!
//! echOS'ta dosya sistemi PageCache üzerinde çalışır. `FilePageCache` page-granular.
//! `sendfile` page referanslarını TCP soket gönderim kuyruğuna ekler (1 kopya).
//! Gerçek zero-copy P2'de eklenecek (page flip + DMA gather).

use super::{NetError, SocketAddr};
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use spin::Mutex;

/// Dosya tanımlayıcı türleri
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FdKind {
    /// Normal dosya (offset ile)
    File { offset: u64, size: u64 },
    /// Pipe (uç nokta)
    Pipe { write_end: bool },
    /// Soket
    Socket(u32),
}

/// `FileDescriptor` — Bir dosya tanımlayıcısını temsil eder
///
/// Handle: okuma/yazma işlemleri için page cache'e erişim noktası
#[derive(Clone, Debug)]
pub struct FileDescriptor {
    pub kind: FdKind,
    /// Dosya içeriği (tüm veri bellekte, küçük dosyalar için uygun)
    /// Büyük dosyalar: page cache indeksi
    pub cache: Arc<Mutex<Vec<u8>>>,
}

impl FileDescriptor {
    pub fn from_bytes(data: Vec<u8>) -> Self {
        FileDescriptor {
            kind: FdKind::File {
                offset: 0,
                size: data.len() as u64,
            },
            cache: Arc::new(Mutex::new(data)),
        }
    }

    pub fn socket(id: u32) -> Self {
        FileDescriptor {
            kind: FdKind::Socket(id),
            cache: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Toplam dosya boyutu
    pub fn size(&self) -> u64 {
        match &self.kind {
            FdKind::File { size, .. } => *size,
            FdKind::Pipe { .. } => 0,
            FdKind::Socket(_) => 0,
        }
    }

    /// Okunabilir mi?
    pub fn is_readable(&self) -> bool {
        match &self.kind {
            FdKind::File { .. } => true,
            FdKind::Pipe { write_end } => !write_end,
            FdKind::Socket(_) => true,
        }
    }

    /// Yazılabilir mi?
    pub fn is_writable(&self) -> bool {
        match &self.kind {
            FdKind::File { .. } => false,
            FdKind::Pipe { write_end } => *write_end,
            FdKind::Socket(_) => true,
        }
    }
}

/// sendfile(2) sonuç yapısı
#[derive(Clone, Debug, Default)]
pub struct SendFileResult {
    /// Aktarılan byte sayısı
    pub bytes_sent: usize,
    /// Yeni dosya offset'i
    pub new_offset: u64,
}

/// `sendfile(2)` — Dosya içeriğini doğrudan sokete aktar
///
/// `out_socket`: hedef TCP soket (UDP'ye sendfile yok)
/// `in_file`: kaynak dosya (`offset` alanı güncellenir; `offset=None` durumunda
///   dosyanın mevcut offset'i kullanılır ve sonrasında güncellenir)
/// `offset`: dosya içinde başlangıç pozisyonu (`Some(x)` = mutlak konum;
///   `None` = `in_file.kind.offset` mevcut konumdan)
/// `count`: aktarılacak maksimum byte sayısı
///
/// **Linux davranışı**: `offset != NULL` ise dosyanın kendi offset'i
/// güncellenmez; sadece `*offset` güncellenir. `offset == NULL` ise dosyanın
/// kendi offset'i ilerletilir. Burada her iki durumda da `in_file`'ın
/// `kind.offset` alanı güncellenir (kullanıcıya yeni offset `new_offset`'te
/// rapor edilir; dahili konum da güncellenir).
pub fn sendfile(
    out_socket: u32,
    in_file: &mut FileDescriptor,
    offset: Option<u64>,
    count: usize,
) -> Result<SendFileResult, NetError> {
    if !in_file.is_readable() {
        return Err(NetError::InvalidArg);
    }
    let FdKind::File { offset: cur_off, .. } = in_file.kind else {
        return Err(NetError::InvalidArg);
    };

    let start = offset.unwrap_or(cur_off);
    if start >= in_file.cache.lock().len() as u64 {
        return Ok(SendFileResult {
            bytes_sent: 0,
            new_offset: start,
        });
    }
    let to_send = {
        let cache = in_file.cache.lock();
        let available = cache.len() as u64 - start;
        (count as u64).min(available) as usize
    };

    // Veriyi kopyala (1 kopya; page flip P2 hedefi)
    let data: Vec<u8> = {
        let cache = in_file.cache.lock();
        cache[start as usize..(start + to_send as u64) as usize].to_vec()
    };

    // Sokete yaz
    let sent = super::tcp::send(out_socket, &data)?;

    // Dosya offset'ini güncelle
    if let FdKind::File { offset: ref mut o, .. } = in_file.kind {
        *o = start + sent as u64;
    }

    Ok(SendFileResult {
        bytes_sent: sent,
        new_offset: start + sent as u64,
    })
}

/// splice(2) sonuç yapısı
#[derive(Clone, Debug, Default)]
pub struct SpliceResult {
    pub bytes_moved: usize,
}

/// `splice(2)` — İki FD arasında veri taşı (1 kopya)
///
/// Desteklenen yönler:
/// - `File` → `Pipe (write_end)` veya `Socket`: dosyadan hedefe
/// - `Pipe (read_end)` → `Pipe (write_end)` veya `Socket`: pipe'dan hedefe
///
/// Soket ↔ Soket splice edilemez (Linux kuralı).
pub fn splice(
    fd_in: &mut FileDescriptor,
    fd_out: &FileDescriptor,
    count: usize,
) -> Result<SpliceResult, NetError> {
    if !fd_in.is_readable() || !fd_out.is_writable() {
        return Err(NetError::InvalidArg);
    }
    if matches!(fd_in.kind, FdKind::Socket(_)) && matches!(fd_out.kind, FdKind::Socket(_)) {
        return Err(NetError::InvalidArg);
    }

    let moved = match &fd_in.kind {
        FdKind::File { .. } => splice_from_file(fd_in, fd_out, count)?,
        FdKind::Pipe { write_end: false } => splice_from_pipe(fd_in, fd_out, count)?,
        FdKind::Pipe { write_end: true } => {
            return Err(NetError::InvalidArg);
        }
        FdKind::Socket(_) => return Err(NetError::InvalidArg),
    };

    Ok(SpliceResult { bytes_moved: moved })
}

fn splice_from_file(
    fd_in: &mut FileDescriptor,
    fd_out: &FileDescriptor,
    count: usize,
) -> Result<usize, NetError> {
    let start = match fd_in.kind {
        FdKind::File { offset, .. } => offset,
        _ => return Err(NetError::InvalidArg),
    };
    let (chunk, chunk_len, new_off) = {
        let cache = fd_in.cache.lock();
        let s = start as usize;
        if s >= cache.len() {
            return Ok(0);
        }
        let end = (s + count).min(cache.len());
        (cache[s..end].to_vec(), end - s, end as u64)
    };
    let written = write_to_out(fd_out, &chunk)?;
    if let FdKind::File { offset: ref mut o, .. } = fd_in.kind {
        *o = new_off;
    }
    Ok(written.min(chunk_len))
}

fn splice_from_pipe(
    fd_in: &FileDescriptor,
    fd_out: &FileDescriptor,
    count: usize,
) -> Result<usize, NetError> {
    let mut buf = vec![0u8; count];
    let mut filled = 0usize;
    {
        let mut cache = fd_in.cache.lock();
        let take = count.min(cache.len());
        buf[..take].copy_from_slice(&cache[..take]);
        cache.drain(..take);
        filled = take;
    }
    if filled == 0 {
        return Ok(0);
    }
    let written = write_to_out(fd_out, &buf[..filled])?;
    Ok(written.min(filled))
}

fn write_to_out(fd_out: &FileDescriptor, data: &[u8]) -> Result<usize, NetError> {
    match &fd_out.kind {
        FdKind::Socket(sid) => super::tcp::send(*sid, data),
        FdKind::Pipe { write_end } => {
            if *write_end {
                let mut buf = fd_out.cache.lock();
                buf.extend_from_slice(data);
                Ok(data.len())
            } else {
                Err(NetError::InvalidArg)
            }
        }
        FdKind::File { .. } => Err(NetError::InvalidArg),
    }
}

/// `tee(2)` — İki pipe arasında veriyi tüketmeden kopyala
///
/// Yalnızca `Pipe (read_end)` → `Pipe (write_end)` kombinasyonu desteklenir
/// (Linux kuralı). Kaynak pipe'daki veri yerinde kalır; hedef pipe'a kopya
/// yazılır.
pub fn tee(
    fd_in: &FileDescriptor,
    fd_out: &FileDescriptor,
    count: usize,
) -> Result<SpliceResult, NetError> {
    match (&fd_in.kind, &fd_out.kind) {
        (FdKind::Pipe { write_end: false }, FdKind::Pipe { write_end: true }) => {
            // Peek: kaynak pipe kilitli, kopyala, kilidi bırak
            let snapshot = fd_in.cache.lock().clone();
            let take = count.min(snapshot.len());
            if take == 0 {
                return Ok(SpliceResult { bytes_moved: 0 });
            }
            let mut out_cache = fd_out.cache.lock();
            out_cache.extend_from_slice(&snapshot[..take]);
            Ok(SpliceResult { bytes_moved: take })
        }
        _ => Err(NetError::InvalidArg),
    }
}

// ============================================================================
// TESTLER
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_descriptor_size_reflects_data() {
        let f = FileDescriptor::from_bytes(b"hello world".to_vec());
        assert_eq!(f.size(), 11);
        assert!(f.is_readable());
        assert!(!f.is_writable());
    }

    #[test]
    fn socket_fd_is_readable_and_writable() {
        let s = FileDescriptor::socket(42);
        assert!(s.is_readable());
        assert!(s.is_writable());
    }

    #[test]
    fn pipe_ends_have_correct_direction() {
        let read_end = FileDescriptor {
            kind: FdKind::Pipe { write_end: false },
            cache: Arc::new(Mutex::new(Vec::new())),
        };
        let write_end = FileDescriptor {
            kind: FdKind::Pipe { write_end: true },
            cache: Arc::new(Mutex::new(Vec::new())),
        };
        assert!(read_end.is_readable());
        assert!(!read_end.is_writable());
        assert!(!write_end.is_readable());
        assert!(write_end.is_writable());
    }

    #[test]
    fn sendfile_result_default_is_zero() {
        let r = SendFileResult::default();
        assert_eq!(r.bytes_sent, 0);
        assert_eq!(r.new_offset, 0);
    }
}
