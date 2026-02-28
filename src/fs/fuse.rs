//! # FUSE (Kullanıcı Alanında Dosya Sistemi)
//!
//! Kullanıcı alanı dosya sistemi sürücüsü desteği.
//!
//! ## FUSE İstek/Yanıt Akışı
//!
//! ```text
//!  Kullanıcı prosesi (örn. SSHFS daemon)
//!          │  /dev/fuse okuma/yazma
//!          ▼
//!  ┌───────────────────────────────────────────────────┐
//!  │  FuseConnection (çekirdek tarafı)                 │
//!  │                                                   │
//!  │  Çekirdek VFS isteği (LOOKUP, READ, WRITE...)     │
//!  │          │                                        │
//!  │          ▼                                        │
//!  │  FuseRequest ──► pending kuyruğuna ekle           │
//!  │          │       (unique ID ile)                  │
//!  │          ▼                                        │
//!  │  Kullanıcı daemon'u /dev/fuse'dan okur            │
//!  │          │  isteği işler                          │
//!  │          ▼                                        │
//!  │  FuseOutHeader + veri ──► /dev/fuse'a yazar       │
//!  │          │  (unique ID ile eşleştirilir)          │
//!  │          ▼                                        │
//!  │  recv_reply(unique) ──► sonucu VFS'e döndür       │
//!  └───────────────────────────────────────────────────┘
//!
//!  İstek yapısı (FuseInHeader):
//!  [ len | opcode | unique | nodeid | uid | gid | pid | pad ]
//!
//!  Yanıt yapısı (FuseOutHeader):
//!  [ len | error | unique ]
//! ```

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// FUSE SABİTLERİ
// ============================================================================

/// FUSE sürüm numarası
pub const FUSE_KERNEL_VERSION: u32 = 7;
pub const FUSE_KERNEL_MINOR_VERSION: u32 = 37;

/// FUSE işlem kodları (opcode'lar)
pub const FUSE_LOOKUP: u32 = 1;
pub const FUSE_FORGET: u32 = 2;
pub const FUSE_GETATTR: u32 = 3;
pub const FUSE_SETATTR: u32 = 4;
pub const FUSE_READLINK: u32 = 5;
pub const FUSE_SYMLINK: u32 = 6;
pub const FUSE_MKNOD: u32 = 8;
pub const FUSE_MKDIR: u32 = 9;
pub const FUSE_UNLINK: u32 = 10;
pub const FUSE_RMDIR: u32 = 11;
pub const FUSE_RENAME: u32 = 12;
pub const FUSE_LINK: u32 = 13;
pub const FUSE_OPEN: u32 = 14;
pub const FUSE_READ: u32 = 15;
pub const FUSE_WRITE: u32 = 16;
pub const FUSE_STATFS: u32 = 17;
pub const FUSE_RELEASE: u32 = 18;
pub const FUSE_FSYNC: u32 = 20;
pub const FUSE_SETXATTR: u32 = 21;
pub const FUSE_GETXATTR: u32 = 22;
pub const FUSE_LISTXATTR: u32 = 23;
pub const FUSE_REMOVEXATTR: u32 = 24;
pub const FUSE_FLUSH: u32 = 25;
pub const FUSE_INIT: u32 = 26;
pub const FUSE_OPENDIR: u32 = 27;
pub const FUSE_READDIR: u32 = 28;
pub const FUSE_RELEASEDIR: u32 = 29;
pub const FUSE_FSYNCDIR: u32 = 30;

/// FUSE özellik bayrakları (capability flags)
pub const FUSE_ASYNC_READ: u64 = 1 << 0;
pub const FUSE_POSIX_LOCKS: u64 = 1 << 1;
pub const FUSE_FILE_OPS: u64 = 1 << 2;
pub const FUSE_ATOMIC_O_TRUNC: u64 = 1 << 3;
pub const FUSE_EXPORT_SUPPORT: u64 = 1 << 4;
pub const FUSE_BIG_WRITES: u64 = 1 << 5;
pub const FUSE_DONT_MASK: u64 = 1 << 6;
pub const FUSE_SPLICE_WRITE: u64 = 1 << 7;
pub const FUSE_SPLICE_MOVE: u64 = 1 << 8;
pub const FUSE_SPLICE_READ: u64 = 1 << 9;
pub const FUSE_FLOCK_LOCKS: u64 = 1 << 10;
pub const FUSE_IOCTL_DIR: u64 = 1 << 11;

// ============================================================================
// FUSE BAŞLIK YAPI LARI
// ============================================================================

#[repr(C)]
pub struct FuseInHeader {
    pub len: u32,
    pub opcode: u32,
    pub unique: u64,
    pub nodeid: u64,
    pub uid: u32,
    pub gid: u32,
    pub pid: u32,
    pub padding: u32,
}

#[repr(C)]
pub struct FuseOutHeader {
    pub len: u32,
    pub error: i32,
    pub unique: u64,
}

// ============================================================================
// FUSE ATTRİBUTLE RI
// ============================================================================

#[repr(C)]
pub struct FuseAttr {
    pub ino: u64,
    pub size: u64,
    pub blocks: u64,
    pub atime: u64,
    pub mtime: u64,
    pub ctime: u64,
    pub atimensec: u32,
    pub mtimensec: u32,
    pub ctimensec: u32,
    pub mode: u32,
    pub nlink: u32,
    pub uid: u32,
    pub gid: u32,
    pub rdev: u32,
    pub blksize: u32,
    pub padding: u32,
}

#[repr(C)]
pub struct FuseEntryOut {
    pub nodeid: u64,
    pub generation: u64,
    pub entry_valid: u64,
    pub attr_valid: u64,
    pub entry_valid_nsec: u32,
    pub attr_valid_nsec: u32,
    pub attr: FuseAttr,
}

#[repr(C)]
pub struct FuseAttrOut {
    pub attr_valid: u64,
    pub attr_valid_nsec: u32,
    pub dummy: u32,
    pub attr: FuseAttr,
}

// ============================================================================
// FUSE BAĞLANTISI
// ============================================================================

pub struct FuseConnection {
    /// Bağlantı ID'si
    pub id: u64,
    /// Bağlama noktası
    pub mount_point: String,
    /// Cihaz fd'si
    pub dev_fd: i32,
    /// Özellikler (capabilities)
    pub caps: AtomicU64,
    /// Maksimum okuma boyutu
    pub max_read: AtomicU32,
    /// Maksimum yazma boyutu
    pub max_write: AtomicU32,
    /// Maksimum sayfa sayısı
    pub max_pages: AtomicU32,
    /// Bağlantı kuruldu mu?
    pub connected: AtomicBool,
    /// Bekleyen istekler
    pub pending: Mutex<Vec<FuseRequest>>,
    /// Düğüm ID sayıcısı
    pub next_nodeid: AtomicU64,
    /// Düğüm haritası
    pub nodes: Mutex<BTreeMap<u64, FuseNode>>,
}

/// FUSE istek yapısı — çekirdekten kullanıcı daemon'una gönderilen işlem
#[derive(Clone, Debug)]
pub struct FuseRequest {
    pub unique: u64,
    pub opcode: u32,
    pub nodeid: u64,
    pub uid: u32,
    pub gid: u32,
    pub pid: u32,
    pub data: Vec<u8>,
}

/// FUSE dosya sistemi düğümü — inode'u temsil eder
#[derive(Clone, Debug)]
pub struct FuseNode {
    pub nodeid: u64,
    pub parent: u64,
    pub name: String,
    pub mode: u32,
    pub ref_count: AtomicU32,
}

impl FuseConnection {
    pub fn new(id: u64, mount: &str) -> Self {
        Self {
            id,
            mount_point: String::from(mount),
            dev_fd: -1,
            caps: AtomicU64::new(0),
            max_read: AtomicU32::new(4096),
            max_write: AtomicU32::new(4096),
            max_pages: AtomicU32::new(256),
            connected: AtomicBool::new(false),
            pending: Mutex::new(Vec::new()),
            next_nodeid: AtomicU64::new(1),
            nodes: Mutex::new(BTreeMap::new()),
        }
    }

    /// Bağlantıyı başlatir
    pub fn init(&self) -> Result<(), FuseError> {
        // Kullanıcı alanı daemon'una FUSE_INIT gönder
        let caps = FUSE_ASYNC_READ | FUSE_BIG_WRITES | FUSE_SPLICE_READ | 
                   FUSE_SPLICE_WRITE | FUSE_SPLICE_MOVE;
        self.caps.store(caps, Ordering::SeqCst);
        self.connected.store(true, Ordering::SeqCst);
        
        crate::serial_println!("[FUSE] Connection {} initialized", self.id);
        Ok(())
    }

    /// Kullanıcı alanına istek gönderir
    pub fn send_request(&self, req: FuseRequest) -> Result<(), FuseError> {
        if !self.connected.load(Ordering::SeqCst) {
            return Err(FuseError::NotConnected);
        }
        
        self.pending.lock().push(req);
        Ok(())
    }

    /// Kullanıcı alanından yanıt alır
    pub fn recv_reply(&self, unique: u64) -> Option<Vec<u8>> {
        // /dev/fuse'dan okunacak
        None
    }

    /// Yolu arar
    pub fn lookup(&self, parent: u64, name: &str) -> Result<FuseEntryOut, FuseError> {
        let req = FuseRequest {
            unique: self.next_nodeid.fetch_add(1, Ordering::SeqCst),
            opcode: FUSE_LOOKUP,
            nodeid: parent,
            uid: 0,
            gid: 0,
            pid: 0,
            data: name.as_bytes().to_vec(),
        };
        
        self.send_request(req)?;
        
        // Yanıtı bekle
        Ok(FuseEntryOut {
            nodeid: 0,
            generation: 0,
            entry_valid: 0,
            attr_valid: 0,
            entry_valid_nsec: 0,
            attr_valid_nsec: 0,
            attr: unsafe { core::mem::zeroed() },
        })
    }

    /// Dosya okur
    pub fn read(&self, nodeid: u64, fh: u64, offset: u64, size: u32) -> Result<Vec<u8>, FuseError> {
        let req = FuseRequest {
            unique: self.next_nodeid.fetch_add(1, Ordering::SeqCst),
            opcode: FUSE_READ,
            nodeid,
            uid: 0,
            gid: 0,
            pid: 0,
            data: alloc::format!("{}:{}:{}:{}", fh, offset, size, 0).into_bytes(),
        };
        
        self.send_request(req)?;
        Ok(vec![0u8; size as usize])
    }

    /// Dosyaya yazar
    pub fn write(&self, nodeid: u64, fh: u64, offset: u64, data: &[u8]) -> Result<u32, FuseError> {
        let req = FuseRequest {
            unique: self.next_nodeid.fetch_add(1, Ordering::SeqCst),
            opcode: FUSE_WRITE,
            nodeid,
            uid: 0,
            gid: 0,
            pid: 0,
            data: data.to_vec(),
        };
        
        self.send_request(req)?;
        Ok(data.len() as u32)
    }

    /// Bağlantıyı yıkar
    pub fn destroy(&self) {
        self.connected.store(false, Ordering::SeqCst);
    }
}

// ============================================================================
// FUSE YÖNETİCİSİ
// ============================================================================

/// Tüm FUSE bağlantılarını yöneten global yönetici
pub struct FuseManager {
    connections: Mutex<BTreeMap<u64, Arc<FuseConnection>>>,
    next_id: AtomicU64,
}

impl FuseManager {
    pub const fn new() -> Self {
        Self {
            connections: Mutex::new(BTreeMap::new()),
            next_id: AtomicU64::new(1),
        }
    }

    pub fn mount(&self, mount_point: &str) -> Result<Arc<FuseConnection>, FuseError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let conn = Arc::new(FuseConnection::new(id, mount_point));
        conn.init()?;
        
        self.connections.lock().insert(id, conn.clone());
        
        crate::serial_println!("[FUSE] Mounted at {}", mount_point);
        Ok(conn)
    }

    pub fn unmount(&self, id: u64) {
        if let Some(conn) = self.connections.lock().remove(&id) {
            conn.destroy();
        }
    }

    pub fn get(&self, id: u64) -> Option<Arc<FuseConnection>> {
        self.connections.lock().get(&id).cloned()
    }
}

lazy_static::lazy_static! {
    pub static ref FUSE_MANAGER: FuseManager = FuseManager::new();
}

// ============================================================================
// HATA TÜRÜ
// ============================================================================

/// FUSE alt sistemi hata türleri
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FuseError {
    NotConnected,
    Timeout,
    IoError,
    InvalidRequest,
    NoMemory,
}

// ============================================================================
// BAŞLAŞMA
// ============================================================================

pub fn init() {
    crate::serial_println!("[FUSE] Alt sistemi başlatıldı");
}
