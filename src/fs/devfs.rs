//! # devfs — /dev Sanal Cihaz Dosya Sistemi
//!
//! Linux tarzı `/dev` karakter/blok cihaz düğümlerinin echOS implementasyonu.
//! Donanım ve soyut cihazlara dosya arayüzü üzerinden erişim sağlar.
//!
//! ## Desteklenen Cihazlar
//!
//! | Cihaz         | Tür     | Açıklama                                       |
//! |---------------|---------|------------------------------------------------|
//! | /dev/null     | karakter| Her şeyi yutan, okuyunca 0 byte döndüren cihaz |
//! | /dev/zero     | karakter| Sonsuz sıfır bayt üretir                       |
//! | /dev/random   | karakter| Çekirdek PRNG (RDRAND/RDSEED)                  |
//! | /dev/urandom  | karakter| Bloke olmayan PRNG (random ile aynı)           |
//! | /dev/full     | karakter| Her yazmaya ENOSPC, okumada sıfır              |
//! | /dev/tty      | karakter| Kontrol terminali                              |
//! | /dev/console  | karakter| Çekirdek konsolu                               |
//! | /dev/kmsg     | karakter| Çekirdek mesaj kuyruğu (dmesg)                 |
//! | /dev/sda      | blok    | İlk disk (ATA/NVMe soyutlaması)                |
//! | /dev/nvme0n1  | blok    | İlk NVMe disk                                  |

use alloc::format;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use rcore_fs::vfs::{FileType, FsError, FsInfo, INode, Metadata, PollStatus, Timespec};
use spin::Mutex;

use crate::drivers::ata::BLOCK_SIZE;

// ============================================================================
// YARDIMCI METAVERİ
// ============================================================================

fn char_dev_meta(size: usize) -> Metadata {
    Metadata {
        dev: 5, // major 5 = /dev/tty etc. (Linux convention)
        inode: 0,
        size,
        blk_size: 1,
        blocks: 0,
        atime: Timespec { sec: 0, nsec: 0 },
        mtime: Timespec { sec: 0, nsec: 0 },
        ctime: Timespec { sec: 0, nsec: 0 },
        type_: FileType::CharDevice,
        mode: 0o020666, // crw-rw-rw-
        nlinks: 1,
        uid: 0,
        gid: 5, // gid 5 = tty group
        rdev: 0,
    }
}

fn block_dev_meta() -> Metadata {
    Metadata {
        dev: 8, // major 8 = sda
        inode: 0,
        size: 0,
        blk_size: BLOCK_SIZE,
        blocks: 0,
        atime: Timespec { sec: 0, nsec: 0 },
        mtime: Timespec { sec: 0, nsec: 0 },
        ctime: Timespec { sec: 0, nsec: 0 },
        type_: FileType::BlockDevice,
        mode: 0o060640, // brw-r-----
        nlinks: 1,
        uid: 0,
        gid: 6, // gid 6 = disk group
        rdev: 0,
    }
}

fn dev_dir_meta() -> Metadata {
    Metadata {
        dev: 5,
        inode: 0,
        size: 0,
        blk_size: BLOCK_SIZE,
        blocks: 0,
        atime: Timespec { sec: 0, nsec: 0 },
        mtime: Timespec { sec: 0, nsec: 0 },
        ctime: Timespec { sec: 0, nsec: 0 },
        type_: FileType::Dir,
        mode: 0o040755,
        nlinks: 2,
        uid: 0,
        gid: 0,
        rdev: 0,
    }
}

// ============================================================================
// NULL CİHAZI — /dev/null
// ============================================================================

/// /dev/null: Okuma → 0 byte. Yazma → başarılı (yutulur).
pub struct DevNull;

impl INode for DevNull {
    fn read_at(&self, _offset: usize, _buf: &mut [u8]) -> Result<usize, FsError> {
        Ok(0) // EOF
    }

    fn write_at(&self, _offset: usize, buf: &[u8]) -> Result<usize, FsError> {
        Ok(buf.len()) // Hepsini yut
    }

    fn poll(&self) -> Result<PollStatus, FsError> {
        Ok(PollStatus {
            read: true,
            write: true,
            error: false,
        })
    }

    fn metadata(&self) -> Result<Metadata, FsError> {
        Ok(char_dev_meta(0))
    }

    fn as_any_ref(&self) -> &dyn core::any::Any {
        self
    }
}

// ============================================================================
// ZERO CİHAZI — /dev/zero
// ============================================================================

/// /dev/zero: Okuma → istenilen kadar 0 bayt. Yazma → başarılı.
pub struct DevZero;

impl INode for DevZero {
    fn read_at(&self, _offset: usize, buf: &mut [u8]) -> Result<usize, FsError> {
        for b in buf.iter_mut() {
            *b = 0;
        }
        Ok(buf.len())
    }

    fn write_at(&self, _offset: usize, buf: &[u8]) -> Result<usize, FsError> {
        Ok(buf.len())
    }

    fn poll(&self) -> Result<PollStatus, FsError> {
        Ok(PollStatus {
            read: true,
            write: true,
            error: false,
        })
    }

    fn metadata(&self) -> Result<Metadata, FsError> {
        Ok(char_dev_meta(0))
    }

    fn as_any_ref(&self) -> &dyn core::any::Any {
        self
    }
}

// ============================================================================
// RASGELE CİHAZI — /dev/random, /dev/urandom
// ============================================================================

/// /dev/random ve /dev/urandom: RDRAND/RDSEED tabanlı CSPRNG
pub struct DevRandom {
    /// true = /dev/random (Linux'ta bloke ederdi; biz engellemeyiz), false = /dev/urandom
    pub is_random: bool,
}

impl INode for DevRandom {
    fn read_at(&self, _offset: usize, buf: &mut [u8]) -> Result<usize, FsError> {
        // RDRAND veya Xorshift PRNG'den doldur
        let mut i = 0;
        while i + 4 <= buf.len() {
            let rand = crate::random::next_u32();
            buf[i..i + 4].copy_from_slice(&rand.to_le_bytes());
            i += 4;
        }
        // Kalan baytlar
        if i < buf.len() {
            let rand = crate::random::next_u32();
            let rem = buf.len() - i;
            buf[i..].copy_from_slice(&rand.to_le_bytes()[..rem]);
        }
        Ok(buf.len())
    }

    fn write_at(&self, _offset: usize, buf: &[u8]) -> Result<usize, FsError> {
        // Entropi ekleme (göz ardı edilir)
        Ok(buf.len())
    }

    fn poll(&self) -> Result<PollStatus, FsError> {
        Ok(PollStatus {
            read: true,
            write: true,
            error: false,
        })
    }

    fn metadata(&self) -> Result<Metadata, FsError> {
        Ok(char_dev_meta(0))
    }

    fn as_any_ref(&self) -> &dyn core::any::Any {
        self
    }
}

// ============================================================================
// FULL CİHAZI — /dev/full
// ============================================================================

/// /dev/full: Okuma → sıfır bayt, Yazma → ENOSPC hatası
pub struct DevFull;

impl INode for DevFull {
    fn read_at(&self, _offset: usize, buf: &mut [u8]) -> Result<usize, FsError> {
        for b in buf.iter_mut() {
            *b = 0;
        }
        Ok(buf.len())
    }

    fn write_at(&self, _offset: usize, _buf: &[u8]) -> Result<usize, FsError> {
        Err(FsError::NoDeviceSpace)
    }

    fn poll(&self) -> Result<PollStatus, FsError> {
        Ok(PollStatus {
            read: true,
            write: false,
            error: false,
        })
    }

    fn metadata(&self) -> Result<Metadata, FsError> {
        Ok(char_dev_meta(0))
    }

    fn as_any_ref(&self) -> &dyn core::any::Any {
        self
    }
}

// ============================================================================
// TTY/CONSOLE — /dev/tty, /dev/console
// ============================================================================

/// /dev/tty ve /dev/console: terminal I/O köprüsü
pub struct DevTty {
    pub name: &'static str,
}

impl INode for DevTty {
    fn read_at(&self, _offset: usize, buf: &mut [u8]) -> Result<usize, FsError> {
        // TTY katmanından oku (keyboard input)
        let n = crate::tty::DEFAULT_TTY.sys_read(buf);
        Ok(n)
    }

    fn write_at(&self, _offset: usize, buf: &[u8]) -> Result<usize, FsError> {
        // Serial port üzerinden yazı (basıt TTY output)
        if let Ok(s) = core::str::from_utf8(buf) {
            for c in s.chars() {
                crate::serial_print!("{}", c);
            }
        }
        Ok(buf.len())
    }

    fn poll(&self) -> Result<PollStatus, FsError> {
        Ok(PollStatus {
            read: true,
            write: true,
            error: false,
        })
    }

    fn metadata(&self) -> Result<Metadata, FsError> {
        Ok(char_dev_meta(0))
    }

    fn as_any_ref(&self) -> &dyn core::any::Any {
        self
    }
}

// ============================================================================
// KMSG — /dev/kmsg (çekirdek mesaj kuyruğu)
// ============================================================================

/// Çekirdek log tamponunu tutar
static KMSG_BUF: Mutex<alloc::collections::VecDeque<u8>> =
    Mutex::new(alloc::collections::VecDeque::new());
static KMSG_SEQ: AtomicU64 = AtomicU64::new(0);

/// Çekirdek log kuyruğuna mesaj ekler (serial_println! yerine kullanılabilir)
pub fn kmsg_push(msg: &str) {
    let seq = KMSG_SEQ.fetch_add(1, Ordering::Relaxed);
    let line = format!("{},{},{},-;{}\n", 6, seq, 0, msg);
    let mut buf = KMSG_BUF.lock();
    for b in line.bytes() {
        if buf.len() >= 262144 {
            buf.pop_front();
        } // 256 KB ring
        buf.push_back(b);
    }
}

pub struct DevKmsg;

impl INode for DevKmsg {
    fn read_at(&self, offset: usize, buf: &mut [u8]) -> Result<usize, FsError> {
        let lock = KMSG_BUF.lock();
        let data: Vec<u8> = lock.iter().cloned().collect();
        if offset >= data.len() {
            return Ok(0);
        }
        let to_copy = (data.len() - offset).min(buf.len());
        buf[..to_copy].copy_from_slice(&data[offset..offset + to_copy]);
        Ok(to_copy)
    }

    fn write_at(&self, _offset: usize, buf: &[u8]) -> Result<usize, FsError> {
        if let Ok(s) = core::str::from_utf8(buf) {
            kmsg_push(s.trim_end_matches('\n'));
        }
        Ok(buf.len())
    }

    fn poll(&self) -> Result<PollStatus, FsError> {
        Ok(PollStatus {
            read: true,
            write: true,
            error: false,
        })
    }

    fn metadata(&self) -> Result<Metadata, FsError> {
        Ok(char_dev_meta(KMSG_BUF.lock().len()))
    }

    fn as_any_ref(&self) -> &dyn core::any::Any {
        self
    }
}

// ============================================================================
// BLOK CİHAZLARI — /dev/sda, /dev/nvme0n1
// ============================================================================

/// Blok cihazı inode'u — ATA veya NVMe sürücüsüne köprü
pub struct DevBlock {
    pub name: &'static str,
    pub is_nvme: bool,
}

impl INode for DevBlock {
    fn read_at(&self, _offset: usize, _buf: &mut [u8]) -> Result<usize, FsError> {
        // Blok cihazı doğrudan okunamaz — dosya sistemi katmanı üzerinden erişilmelidir
        // Gelecekte: sektör bazlı raw okuma desteği eklenecek
        Err(FsError::NotSupported)
    }

    fn write_at(&self, _offset: usize, _buf: &[u8]) -> Result<usize, FsError> {
        Err(FsError::NotSupported)
    }

    fn poll(&self) -> Result<PollStatus, FsError> {
        Ok(PollStatus {
            read: true,
            write: true,
            error: false,
        })
    }

    fn metadata(&self) -> Result<Metadata, FsError> {
        Ok(block_dev_meta())
    }

    fn as_any_ref(&self) -> &dyn core::any::Any {
        self
    }
}

// ============================================================================
// /dev DİZİN INODE
// ============================================================================

pub struct DevDirInode;

impl INode for DevDirInode {
    fn read_at(&self, _offset: usize, _buf: &mut [u8]) -> Result<usize, FsError> {
        Err(FsError::NotFile)
    }

    fn write_at(&self, _offset: usize, _buf: &[u8]) -> Result<usize, FsError> {
        Err(FsError::NotSupported)
    }

    fn poll(&self) -> Result<PollStatus, FsError> {
        Ok(PollStatus {
            read: false,
            write: false,
            error: false,
        })
    }

    fn metadata(&self) -> Result<Metadata, FsError> {
        Ok(dev_dir_meta())
    }

    fn find(&self, name: &str) -> Result<Arc<dyn INode>, FsError> {
        lookup_dev(name)
    }

    fn as_any_ref(&self) -> &dyn core::any::Any {
        self
    }
}

/// İsme göre /dev inode'u döndürür
fn lookup_dev(name: &str) -> Result<Arc<dyn INode>, FsError> {
    match name {
        "null" => Ok(Arc::new(DevNull)),
        "zero" => Ok(Arc::new(DevZero)),
        "full" => Ok(Arc::new(DevFull)),
        "random" => Ok(Arc::new(DevRandom { is_random: true })),
        "urandom" => Ok(Arc::new(DevRandom { is_random: false })),
        "tty" => Ok(Arc::new(DevTty { name: "tty" })),
        "console" => Ok(Arc::new(DevTty { name: "console" })),
        "stdin" => Ok(Arc::new(DevTty { name: "stdin" })),
        "stdout" => Ok(Arc::new(DevTty { name: "stdout" })),
        "stderr" => Ok(Arc::new(DevTty { name: "stderr" })),
        "kmsg" => Ok(Arc::new(DevKmsg)),
        "sda" => Ok(Arc::new(DevBlock {
            name: "sda",
            is_nvme: false,
        })),
        "nvme0n1" => Ok(Arc::new(DevBlock {
            name: "nvme0n1",
            is_nvme: true,
        })),
        _ => Err(FsError::EntryNotFound),
    }
}

// ============================================================================
// GİRİŞ NOKTASI
// ============================================================================

/// Path'e göre /dev inode'u döndürür
pub fn open_dev_inode(path: &str) -> Result<Arc<dyn INode>, FsError> {
    let path = path.trim_start_matches('/');
    let parts: Vec<&str> = path.splitn(2, '/').collect();

    if parts.is_empty() || parts[0] != "dev" {
        return Err(FsError::EntryNotFound);
    }

    if parts.len() == 1 {
        return Ok(Arc::new(DevDirInode));
    }

    lookup_dev(parts[1])
}

/// Bu path'in /dev kapsamına girip girmediğini kontrol eder
pub fn is_dev_path(path: &str) -> bool {
    path == "/dev" || path.starts_with("/dev/")
}
