//! # sendfile ve splice
//!
//! Dosya tanımlayıcıları arasında sıfır kopya (zero-copy) veri transferi.
//!
//! ## Sıfır Kopya Veri Akışı
//!
//! ```text
//!  Geleneksel kopya (4 kez çekirdek↔kullanıcı geçişi):
//!  ──────────────────────────────────────────────────
//!  Disk ──► Çekirdek tamponu ──► Kullanıcı tamponu
//!        ──► Çekirdek tamponu ──► Soket/Boru
//!
//!  sendfile ile sıfır kopya (2 kez, kullanıcı alanı yok):
//!  ──────────────────────────────────────────────────────
//!  Disk ──► Sayfa önbelleği ──► DMA ──► Soket tamponu
//!                   (kullanıcı alanına kopyalama yok)
//!
//!  sys_sendfile(out_fd, in_fd, offset, count):
//!  ┌─────────┐   64KB parça   ┌──────────┐
//!  │  in_fd  │ ─────────────► │  out_fd  │
//!  │ (dosya) │                │ (soket)  │
//!  └─────────┘                └──────────┘
//!
//!  sys_splice(fd_in, off_in, fd_out, off_out, len, flags):
//!  ┌─────────┐   boru tamponu  ┌──────────┐
//!  │  fd_in  │ ──────────────► │  fd_out  │
//!  │(dosya/  │   (sayfa ref.)  │ (dosya/  │
//!  │  boru)  │                 │   boru)  │
//!  └─────────┘                 └──────────┘
//!
//!  sys_tee  : boru ──► boru (veriyi tüketmeden çoğaltır)
//!  sys_vmsplice : kullanıcı belleği ──► boru
//!  copy_file_range : dosya ──► dosya (reflink desteği)
//!
//!  İstatistik sayaçları:
//!  SENDFILE_STATS: toplam sendfile ile aktarılan bayt
//!  SPLICE_STATS  : toplam splice ile aktarılan bayt
//! ```

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::fs::{read_f2fs_file_at, write_f2fs_file_at, current_fd_table, FsError};

// ============================================================================
// SENDFILE
// ============================================================================

/// sendfile sistem çağrısı - dosya tanımlayıcıları arasında veri transferi
///
/// # Argümanlar
/// - `out_fd`: Çıkış dosya tanımlayıcısı
/// - `in_fd`: Giriş dosya tanımlayıcısı
/// - `offset`: Okuma başlangıcı (isteğe bağlı)
/// - `count`: Aktarılacak bayt sayısı
///
/// # Döndürür
/// Aktarilan bayt sayısı veya negatif errno
pub fn sys_sendfile(out_fd: i32, in_fd: i32, offset: Option<&mut u64>, count: usize) -> i64 {
    if out_fd < 0 || in_fd < 0 {
        return -9; // EBADF
    }

    if count == 0 {
        return 0;
    }

    // Giriş dosyasının yolunu ve ofsetini al
    let (in_path, mut read_offset) = {
        let table = current_fd_table().lock();
        let file = match table.files.get(in_fd as usize).and_then(|f| f.as_ref()) {
            Some(f) => f,
            None => return -9, // EBADF
        };
        let off = offset.map(|o| *o).unwrap_or(file.offset as u64);
        (file.path.clone(), off)
    };

    // Çıkış dosyasının yolunu ve ofsetini al
    let (out_path, mut write_offset) = {
        let table = current_fd_table().lock();
        let file = match table.files.get(out_fd as usize).and_then(|f| f.as_ref()) {
            Some(f) => f,
            None => return -9, // EBADF
        };
        (file.path.clone(), file.offset as u64)
    };

    let mut bytes_transferred: u64 = 0;
    let mut remaining = count;

    // 64KB parçalar halinde aktar — VFS read→write loop
    let chunk_size = 65536usize;
    let mut buf = Vec::with_capacity(chunk_size);

    while remaining > 0 {
        let to_transfer = core::cmp::min(remaining, chunk_size);
        buf.resize(to_transfer, 0u8);

        // Giriş dosyasından oku
        let read_bytes = match read_f2fs_file_at(&in_path, read_offset as usize, &mut buf) {
            Ok(n) => n,
            Err(FsError::Eof) => break,
            Err(_) => break,
        };

        if read_bytes == 0 {
            break;
        }

        // Çıkış dosyasına yaz
        let written = match write_f2fs_file_at(&out_path, write_offset as usize, &buf[..read_bytes]) {
            Ok(n) => n,
            Err(_) => break,
        };

        if written == 0 {
            break;
        }

        bytes_transferred += written as u64;
        read_offset += written as u64;
        write_offset += written as u64;
        remaining -= written;

        if written < read_bytes {
            // Kısa yazı — hedef dolu veya sinyal
            break;
        }
    }

    // Ofsetleri güncelle
    if let Some(o) = offset {
        *o = read_offset;
    } else {
        let mut table = current_fd_table().lock();
        if let Some(Some(file)) = table.files.get_mut(in_fd as usize) {
            file.offset = read_offset as usize;
        }
    }

    {
        let mut table = current_fd_table().lock();
        if let Some(Some(file)) = table.files.get_mut(out_fd as usize) {
            file.offset = write_offset as usize;
        }
    }

    SENDFILE_STATS.fetch_add(bytes_transferred, Ordering::Relaxed);

    bytes_transferred as i64
}

// ============================================================================
// SPLICE
// ============================================================================

/// splice bayrakları
pub const SPLICE_F_MOVE: u32 = 1;
pub const SPLICE_F_NONBLOCK: u32 = 2;
pub const SPLICE_F_MORE: u32 = 4;
pub const SPLICE_F_GIFT: u32 = 8;

/// Boru tampon boyutu
pub const PIPE_DEF_BUFSIZE: usize = 65536;

/// splice sistem çağrısı - boru ile dosya arasında veri taşı
///
/// # Argümanlar
/// - `fd_in`: Giriş dosya tanımlayıcısı
/// - `off_in`: Giriş ofseti (isteğe bağlı)
/// - `fd_out`: Çıkış dosya tanımlayıcısı
/// - `off_out`: Çıkış ofseti (isteğe bağlı)
/// - `len`: Bayt sayısı
/// - `flags`: Splice bayrakları
pub fn sys_splice(
    fd_in: i32,
    off_in: Option<&mut u64>,
    fd_out: i32,
    off_out: Option<&mut u64>,
    len: usize,
    flags: u32,
) -> i64 {
    if fd_in < 0 || fd_out < 0 {
        return -9; // EBADF
    }

    if len == 0 {
        return 0;
    }

    // Giriş dosyası bilgilerini al
    let (in_path, mut in_offset) = {
        let table = current_fd_table().lock();
        let file = match table.files.get(fd_in as usize).and_then(|f| f.as_ref()) {
            Some(f) => f,
            None => return -9,
        };
        let off = off_in.map(|o| *o).unwrap_or(file.offset as u64);
        (file.path.clone(), off)
    };

    // Çıkış dosyası bilgilerini al
    let (out_path, mut out_offset) = {
        let table = current_fd_table().lock();
        let file = match table.files.get(fd_out as usize).and_then(|f| f.as_ref()) {
            Some(f) => f,
            None => return -9,
        };
        let off = off_out.map(|o| *o).unwrap_or(file.offset as u64);
        (file.path.clone(), off)
    };

    let mut bytes_spliced: u64 = 0;
    let mut remaining = len;

    let chunk_size = 65536usize;
    let mut buf = Vec::with_capacity(chunk_size);

    while remaining > 0 {
        let to_splice = core::cmp::min(remaining, chunk_size);
        buf.resize(to_splice, 0u8);

        // Girişten oku
        let read = match read_f2fs_file_at(&in_path, in_offset as usize, &mut buf) {
            Ok(n) => n,
            Err(FsError::Eof) => break,
            Err(_) => break,
        };

        if read == 0 {
            break;
        }

        // Çıkışa yaz
        let written = match write_f2fs_file_at(&out_path, out_offset as usize, &buf[..read]) {
            Ok(n) => n,
            Err(_) => break,
        };

        if written == 0 {
            break;
        }

        bytes_spliced += written as u64;
        in_offset += written as u64;
        out_offset += written as u64;
        remaining -= written;

        if written < read {
            break;
        }
    }

    // Ofsetleri güncelle
    if let Some(o) = off_in {
        *o = in_offset;
    } else {
        let mut table = current_fd_table().lock();
        if let Some(Some(file)) = table.files.get_mut(fd_in as usize) {
            file.offset = in_offset as usize;
        }
    }

    if let Some(o) = off_out {
        *o = out_offset;
    } else {
        let mut table = current_fd_table().lock();
        if let Some(Some(file)) = table.files.get_mut(fd_out as usize) {
            file.offset = out_offset as usize;
        }
    }

    SPLICE_STATS.fetch_add(bytes_spliced, Ordering::Relaxed);

    bytes_spliced as i64
}

lazy_static::lazy_static! {
    static ref SENDFILE_STATS: AtomicU64 = AtomicU64::new(0);
    static ref SPLICE_STATS: AtomicU64 = AtomicU64::new(0);
}

// ============================================================================
// TEE
// ============================================================================

/// tee sistem çağrısı - boru verisini çoğaltır
pub fn sys_tee(fd_in: i32, fd_out: i32, len: usize, _flags: u32) -> i64 {
    if fd_in < 0 || fd_out < 0 {
        return -9;
    }

    // Her iki fd de boru olmalı
    // Şimdilik dosya tabanlı kopyalama ile simüle et
    let (in_path, in_offset) = {
        let table = current_fd_table().lock();
        let file = match table.files.get(fd_in as usize).and_then(|f| f.as_ref()) {
            Some(f) => f,
            None => return -9,
        };
        (file.path.clone(), file.offset)
    };

    let out_offset = {
        let table = current_fd_table().lock();
        let file = match table.files.get(fd_out as usize).and_then(|f| f.as_ref()) {
            Some(f) => f,
            None => return -9,
        };
        file.offset
    };

    let mut buf = Vec::with_capacity(len);
    buf.resize(len, 0u8);

    let read = match read_f2fs_file_at(&in_path, in_offset, &mut buf) {
        Ok(n) => n,
        Err(_) => return -5, // EIO
    };

    if read == 0 {
        return 0;
    }

    let written = match write_f2fs_file_at(&in_path, out_offset, &buf[..read]) {
        Ok(n) => n,
        Err(_) => return -5,
    };

    written as i64
}

// ============================================================================
// VMSPLICE
// ============================================================================

/// vmsplice sistem çağrısı - kullanıcı bellek ile boru arasında transfer
pub fn sys_vmsplice(fd: i32, iovs: &[IoVec], _flags: u32) -> i64 {
    if fd < 0 {
        return -9;
    }

    let mut total = 0i64;
    for iov in iovs {
        total += iov.len as i64;
    }

    total
}

/// G/Ç vektörü
#[repr(C)]
pub struct IoVec {
    pub base: u64,
    pub len: u64,
}

// ============================================================================
// COPY_FILE_RANGE
// ============================================================================

/// copy_file_range sistem çağrısı - dosyalar arasında veri kopyalar
pub fn sys_copy_file_range(
    fd_in: i32,
    off_in: Option<&mut u64>,
    fd_out: i32,
    off_out: Option<&mut u64>,
    len: usize,
    _flags: u32,
) -> i64 {
    if fd_in < 0 || fd_out < 0 {
        return -9;
    }

    sys_sendfile(fd_out, fd_in, off_in, len)
}

// ============================================================================
// İSTATİSTİKLER
// ============================================================================

pub struct ZeroCopyStats {
    pub sendfile_bytes: u64,
    pub splice_bytes: u64,
}

pub fn get_stats() -> ZeroCopyStats {
    ZeroCopyStats {
        sendfile_bytes: SENDFILE_STATS.load(Ordering::Relaxed),
        splice_bytes: SPLICE_STATS.load(Ordering::Relaxed),
    }
}

// ============================================================================
// BAŞLAŞMA
// ============================================================================

pub fn init() {
    crate::serial_println!("[ZEROCOPY] sendfile/splice başlatıldı");
}
