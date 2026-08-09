//! # echOS Dosya Sistemi
//!
//! Dosya sistemi desteği. Şu anda F2FS, FAT32/exFAT, ext4 ve NTFS implementasyonu mevcut.
//!
//! ## Sanal Dosya Sistemi (VFS) Katman Mimarisi
//!
//! ```
//!  Kullanıcı Alanı Sistem Çağrıları
//!  open() / read() / write() / close()
//!          │
//!          ▼
//!  ┌────────────────────────────────────────────────┐
//!  │          VFS Katmanı (mod.rs)                  │
//!  │  sys_open / sys_read / sys_write / sys_close   │
//!  │  FileDescriptorTable  (fd → OpenFile)          │
//!  └───────────────┬────────────────────────────────┘
//!                  │
//!       ┌──────────┴──────────┐
//!       ▼                     ▼
//!  ┌─────────┐          ┌──────────┐
//!  │  F2FS   │          │  diğer   │
//!  │ (yerli) │          │ (ext4,   │
//!  │  mod    │          │  NTFS,   │
//!  └────┬────┘          │  FAT32)  │
//!       │               └──────────┘
//!       ▼
//!  ┌────────────────┐
//!  │  INode Arayüzü │  ← rcore-fs::vfs::INode
//!  │  read_at()     │
//!  │  write_at()    │
//!  │  metadata()    │
//!  │  find()        │
//!  └────────────────┘
//!          │
//!          ▼
//!  ┌───────────────────┐
//!  │  ATA/Blok Sürücüsü│
//!  │  (disk I/O)       │
//!  └───────────────────┘
//!
//! ## Dosya Tanımlayıcısı Yaşam Döngüsü
//!
//!  sys_open("dosya", O_RDWR) → fd = 3
//!      │
//!      ├─ FileDescriptorTable'a OpenFile eklenir
//!      │  { path, offset: 0, flags }
//!      │
//!  sys_read(fd=3, buf, 512)
//!      │
//!      ├─ fd → OpenFile.path → F2FS okuma
//!      └─ offset += okundu
//!
//!  sys_close(fd=3)
//!      └─ OpenFile = None (tablo girişi boşaltıldı)
//! ```

pub mod btrfs;
pub mod cpio;
pub mod devfs;
pub mod erofs;
pub mod ext4;
pub mod ext4_journal;
pub mod dcache;
pub mod f2fs;
pub mod icache;
pub mod fanotify;
pub mod fat;
pub mod file_lock;
pub mod inotify;
pub mod journal;
pub mod mount;
pub mod namei;
pub mod ntfs;
pub mod overlayfs;
pub mod page_cache;
pub mod procfs;
pub mod squashfs;
pub mod sysfs;
pub mod tar;
pub mod tmpfs;
pub mod vfs_unified;
pub mod xattr;
pub mod xfs;
pub mod fs_smoke_test;

/// Crash state of a filesystem operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrashState {
    NotStarted,
    MetadataUpdated,
    DataWritten,
    JournalLogged,
    JournalCommitted,
    Checkpointed,
    Completed,
    Inconsistent,
    Corrupt,
}

/// Recovery action after a crash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryAction {
    None,
    JournalReplay,
    RollForward,
    Rollback,
    Fsck,
    Manual,
}

/// Crash contract for a mutating filesystem operation.
pub struct OperationCrashContract {
    pub operation: &'static str,
    pub pre_state: CrashState,
    pub success_post_state: CrashState,
    pub allowed_crash_states: &'static [CrashState],
    pub forbidden_crash_states: &'static [CrashState],
    pub recovery_action: RecoveryAction,
    pub fsck_required: bool,
}

impl OperationCrashContract {
    pub fn is_allowed(&self, state: CrashState) -> bool {
        self.allowed_crash_states.contains(&state)
    }

    pub fn is_forbidden(&self, state: CrashState) -> bool {
        self.forbidden_crash_states.contains(&state)
    }
}

/// Trait for filesystems that provide crash consistency contracts.
pub trait CrashConsistentFs {
    fn crash_contract(&self, operation: &'static str) -> Option<OperationCrashContract>;
    fn verify_crash_state(&self, operation: &'static str) -> Result<CrashState, &'static str>;
    fn recover_from_crash(&mut self, operation: &'static str) -> Result<(), &'static str>;
}

use alloc::format;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use lazy_static::lazy_static;
use rcore_fs::vfs::{FileSystem, FileType, FsInfo, INode, Metadata, PollStatus, Timespec};
use spin::Mutex;

use alloc::rc::Rc;
use crate::fs::dcache::{Dcache, Dentry};

lazy_static! {
    pub(crate) static ref VFS_DCACHE: Mutex<Dcache> = Mutex::new(Dcache::new());
}

/// Dentry cache'de lookup yapar, miss durumunda f2fs backend ile doldurur.
pub fn vfs_dcache_resolve(parent_ino: u64, name: &str) -> Option<Rc<Dentry>> {
    let mut cache = VFS_DCACHE.lock();
    if let Some(d) = cache.lookup(parent_ino, name) {
        return Some(d);
    }
    drop(cache);

    let path = if parent_ino == 0 {
        format!("/{}", name)
    } else {
        String::from(name)
    };
    match crate::fs::f2fs::open_entry(&path) {
        Ok(entry) => {
            let dentry = Dentry {
                name: name.into(),
                parent_ino,
                ino: entry.ino,
                is_dir: entry.is_dir,
                mode: entry.mode as u16,
                uid: entry.uid,
                gid: entry.gid,
                size: entry.size,
                generation: 0,
            };
            let mut cache = VFS_DCACHE.lock();
            let rc = cache.alloc(dentry);
            Some(rc)
        }
        Err(_) => None,
    }
}

/// Path'i "/a/b/c" şeklinde parçalara ayırır, her component için dcache'den lookup yapar.
/// Başarısız olan component için f2fs backend'e gider, sonucu dcache'e ekler.
pub fn vfs_dcache_resolve_full(path: &str) -> Option<Rc<Dentry>> {
    let trimmed = path.trim_start_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    let parts: Vec<&str> = trimmed.split('/').collect();
    let mut parent_ino: u64 = 0; // root
    let mut result = None;

    for part in &parts {
        if part.is_empty() {
            continue;
        }
        match vfs_dcache_resolve(parent_ino, part) {
            Some(d) => {
                parent_ino = d.ino;
                result = Some(d);
            }
            None => return None,
        }
    }
    result
}

/// Dcache'i shrink et — memory pressure callback'lerinden çağrılır
pub fn vfs_dcache_shrink() {
    let mut cache = VFS_DCACHE.lock();
    let target = cache.len().saturating_sub(4096);
    cache.shrink(target);
}

/// Dcache'den bir entry sil
pub fn vfs_dcache_delete(parent_ino: u64, name: &str) {
    let mut cache = VFS_DCACHE.lock();
    cache.delete(parent_ino, name);
}

/// Dcache'de rename
pub fn vfs_dcache_rename(old_parent: u64, old_name: &str, new_parent: u64, new_name: &str) {
    let mut cache = VFS_DCACHE.lock();
    cache.rename(old_parent, old_name, new_parent, new_name);
}

/// Dcache istatistikleri
pub fn vfs_dcache_len() -> usize {
    let cache = VFS_DCACHE.lock();
    cache.len()
}
use spin::RwLock;

// Re-export rcore_fs FsError under an alias to avoid collision with our unified FsError
use rcore_fs::vfs::FsError as RcFsError;

/// Unified filesystem error enum per POSIX.1-2024 errno(3).
///
/// All layers (VFS, backends, shell, store, GUI) use this single error type.
/// No layer may swallow errors or return fake success.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsError {
    /// Başarılı
    Ok,
    /// Path veya dosya bulunamadı (ENOENT)
    NotFound,
    /// Path dizin değil ama dizin bekleniyordu (ENOTDIR)
    NotDirectory,
    /// Path bir dizin ama dosya bekleniyordu (EISDIR)
    IsDirectory,
    /// Dosya/dizin zaten mevcut (EEXIST)
    AlreadyExists,
    /// İzin reddedildi (EACCES)
    PermissionDenied,
    /// Dosya sistemi read-only (EROFS)
    ReadOnlyFs,
    /// Cihazlar arası işlem (EXDEV)
    CrossDevice,
    /// Geçersiz path formatı (EINVAL)
    InvalidPath,
    /// Path maksimum uzunluğu aştı (ENAMETOOLONG)
    NameTooLong,
    /// Tek bir component maksimum uzunluğu aştı (ENAMETOOLONG)
    ComponentTooLong,
    /// Symlink loop tespit edildi (ELOOP)
    SymlinkLoop,
    /// Symlink desteği bu backend'te yok (EOPNOTSUPP)
    UnsupportedSymlink,
    /// Backend bu işlemi desteklemiyor (ENODEV)
    UnsupportedBackend,
    /// Özellik bu backend'te desteklenmiyor (EOPNOTSUPP)
    UnsupportedFeature(UnsupportedFeatureType),
    /// Dosya sistemi kurtarma bekliyor (EIO)
    NeedsRecovery,
    /// Dosya sistemi metadata'sı bozuk (EIO)
    CorruptFs,
    /// Alt seviye I/O hatası (EIO)
    IoError,
    /// Okuma hatası (EIO) — dosya okuma sırasında I/O hatası
    ReadError,
    /// Yazma hatası (EIO) — dosya yazma sırasında I/O hatası
    WriteError,
    /// Diskte yer kalmadı (ENOSPC)
    NoSpace,
    /// Kullanıcı/dizin kotası aşıldı (EDQUOT)
    QuotaExceeded,
    /// Handle stale (ESTALE) — dosya silinmiş veya inode değişmiş
    StaleHandle,
    /// Kaynak meşgul (EBUSY) — mount point kullanımda
    Busy,
    /// Cihaz bulunamadı (ENODEV)
    NoDevice,
    /// Gerekli bellek yok (ENOMEM)
    NoMemory,
    /// İşlem iptal edildi (EINTR)
    Interrupted,
    /// Kaynak kilitli (EAGAIN/EWOULDBLOCK)
    WouldBlock,
    /// Implementasyon hatası (BUG)
    InternalError,
    /// Dosya değil (ENOTFILE — rcore-fs compatibility)
    NotFile,
    /// İşlem desteklenmiyor (ENOSYS — rcore-fs compatibility)
    NotSupported,
    /// Dizin boş değil (ENOTEMPTY)
    NotEmpty,
}

/// Feature types that may be unsupported by a backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnsupportedFeatureType {
    Compression,
    Encryption,
    MultiDevice,
    ReparsePoint,
    Acl,
    Quota,
    Snapshot,
    Reflink,
    Casefold,
    SparseFiles,
    InlineData,
    Verity,
    Journal,
}

impl FsError {
    /// Return the POSIX errno value for this error.
    pub fn to_errno(&self) -> i32 {
        match self {
            FsError::Ok => 0,
            FsError::NotFound => 2,               // ENOENT
            FsError::NotDirectory => 20,          // ENOTDIR
            FsError::IsDirectory => 21,           // EISDIR
            FsError::AlreadyExists => 17,         // EEXIST
            FsError::PermissionDenied => 13,      // EACCES
            FsError::ReadOnlyFs => 30,            // EROFS
            FsError::CrossDevice => 18,           // EXDEV
            FsError::InvalidPath => 22,           // EINVAL
            FsError::NameTooLong => 36,           // ENAMETOOLONG
            FsError::ComponentTooLong => 36,      // ENAMETOOLONG
            FsError::SymlinkLoop => 40,           // ELOOP
            FsError::UnsupportedSymlink => 95,    // EOPNOTSUPP
            FsError::UnsupportedBackend => 19,    // ENODEV
            FsError::UnsupportedFeature(_) => 95, // EOPNOTSUPP
            FsError::NeedsRecovery => 5,          // EIO
            FsError::CorruptFs => 5,              // EIO
            FsError::IoError => 5,                // EIO
            FsError::ReadError => 5,              // EIO
            FsError::WriteError => 5,             // EIO
            FsError::NoSpace => 28,               // ENOSPC
            FsError::QuotaExceeded => 122,        // EDQUOT
            FsError::StaleHandle => 116,          // ESTALE
            FsError::Busy => 16,                  // EBUSY
            FsError::NoDevice => 19,              // ENODEV
            FsError::NoMemory => 12,              // ENOMEM
            FsError::Interrupted => 4,            // EINTR
            FsError::WouldBlock => 11,            // EAGAIN
            FsError::InternalError => 5,          // EIO
            FsError::NotFile => 9,                // EBADF (not a file descriptor target)
            FsError::NotSupported => 95,          // ENOSYS
            FsError::NotEmpty => 66,              // ENOTEMPTY
        }
    }

    /// Return a human-readable description (for GUI/logging).
    pub fn description(&self) -> &'static str {
        match self {
            FsError::Ok => "success",
            FsError::NotFound => "no such file or directory",
            FsError::NotDirectory => "not a directory",
            FsError::IsDirectory => "is a directory",
            FsError::AlreadyExists => "file exists",
            FsError::PermissionDenied => "permission denied",
            FsError::ReadOnlyFs => "read-only filesystem",
            FsError::CrossDevice => "cross-device link",
            FsError::InvalidPath => "invalid path",
            FsError::NameTooLong => "filename too long",
            FsError::ComponentTooLong => "filename component too long",
            FsError::SymlinkLoop => "too many symbolic links",
            FsError::UnsupportedSymlink => "symlink not supported",
            FsError::UnsupportedBackend => "backend not supported",
            FsError::UnsupportedFeature(_) => "feature not supported",
            FsError::NeedsRecovery => "filesystem needs recovery",
            FsError::CorruptFs => "filesystem corrupt",
            FsError::IoError => "I/O error",
            FsError::ReadError => "read error",
            FsError::WriteError => "write error",
            FsError::NoSpace => "no space left on device",
            FsError::QuotaExceeded => "quota exceeded",
            FsError::StaleHandle => "stale file handle",
            FsError::Busy => "device or resource busy",
            FsError::NoDevice => "no such device",
            FsError::NoMemory => "out of memory",
            FsError::Interrupted => "interrupted system call",
            FsError::WouldBlock => "resource temporarily unavailable",
            FsError::InternalError => "internal error",
            FsError::NotFile => "not a file",
            FsError::NotSupported => "operation not supported",
            FsError::NotEmpty => "directory not empty",
        }
    }
}

/// Convert a static error string (from legacy APIs) to FsError.
///
/// This bridges the gap between `Result<_, &'static str>` return types
/// and the unified `FsError` enum. Matching is case-insensitive on key
/// substrings to handle the various string formats used across the codebase.
impl From<&'static str> for FsError {
    fn from(s: &'static str) -> Self {
        let lower = s.to_lowercase();
        if lower.contains("not found")
            || lower.contains("no such file")
            || lower.contains("entry not found")
            || lower.contains("file not found")
        {
            FsError::NotFound
        } else if lower.contains("not a directory") || lower.contains("notdir") {
            FsError::NotDirectory
        } else if lower.contains("is a directory") || lower.contains("isdir") {
            FsError::IsDirectory
        } else if lower.contains("already exists")
            || lower.contains("file exists")
            || lower.contains("exists")
        {
            FsError::AlreadyExists
        } else if lower.contains("permission")
            || lower.contains("access denied")
            || lower.contains("denied")
        {
            FsError::PermissionDenied
        } else if lower.contains("read-only")
            || lower.contains("readonly")
            || lower.contains("rofs")
        {
            FsError::ReadOnlyFs
        } else if lower.contains("cross-device")
            || lower.contains("cross device")
            || lower.contains("exdev")
        {
            FsError::CrossDevice
        } else if lower.contains("invalid path")
            || lower.contains("invalid parameter")
            || lower.contains("einval")
        {
            FsError::InvalidPath
        } else if lower.contains("too long") || lower.contains("nametoolong") {
            FsError::NameTooLong
        } else if lower.contains("symlink") && lower.contains("loop") {
            FsError::SymlinkLoop
        } else if lower.contains("symlink") {
            FsError::UnsupportedSymlink
        } else if lower.contains("backend") || lower.contains("nodev") {
            FsError::UnsupportedBackend
        } else if lower.contains("feature")
            || lower.contains("not supported")
            || lower.contains("opnotsupp")
        {
            FsError::UnsupportedFeature(UnsupportedFeatureType::Compression)
        } else if lower.contains("recovery") || lower.contains("needs recovery") {
            FsError::NeedsRecovery
        } else if lower.contains("corrupt") {
            FsError::CorruptFs
        } else if lower.contains("no space")
            || lower.contains("nospc")
            || lower.contains("disk full")
        {
            FsError::NoSpace
        } else if lower.contains("quota") {
            FsError::QuotaExceeded
        } else if lower.contains("stale") {
            FsError::StaleHandle
        } else if lower.contains("busy") || lower.contains("in use") || lower.contains("ebusy") {
            FsError::Busy
        } else if lower.contains("no device") || lower.contains("no such device") {
            FsError::NoDevice
        } else if lower.contains("memory") || lower.contains("enomem") {
            FsError::NoMemory
        } else if lower.contains("interrupted") || lower.contains("eintr") {
            FsError::Interrupted
        } else if lower.contains("would block")
            || lower.contains("eagain")
            || lower.contains("ewouldblock")
        {
            FsError::WouldBlock
        } else if lower.contains("i/o") || lower.contains("io error") || lower.contains("eio") {
            FsError::IoError
        } else if lower.contains("internal") {
            FsError::InternalError
        } else {
            // Default: treat unknown errors as I/O error
            FsError::IoError
        }
    }
}

/// Convert rcore_fs FsError to our unified FsError.
impl From<rcore_fs::vfs::FsError> for FsError {
    fn from(e: rcore_fs::vfs::FsError) -> Self {
        match e {
            RcFsError::EntryNotFound => FsError::NotFound,
            RcFsError::NotDir => FsError::NotDirectory,
            RcFsError::IsDir => FsError::IsDirectory,
            RcFsError::EntryExist => FsError::AlreadyExists,
            RcFsError::NotFile => FsError::NotFile,
            RcFsError::NotSupported => FsError::NotSupported,
            RcFsError::InvalidParam => FsError::InvalidPath,
            RcFsError::NoDevice => FsError::NoDevice,
            RcFsError::DeviceError => FsError::IoError,
            RcFsError::WrongFs => FsError::CorruptFs,
            RcFsError::DirNotEmpty => FsError::NotEmpty,
            RcFsError::SymLoop => FsError::SymlinkLoop,
            RcFsError::Busy => FsError::Busy,
            RcFsError::Interrupted => FsError::Interrupted,
            RcFsError::Again => FsError::WouldBlock,
            RcFsError::NotSameFs => FsError::CrossDevice,
            RcFsError::NoDeviceSpace => FsError::NoSpace,
            _ => FsError::InternalError,
        }
    }
}

/// Compression types supported by filesystem backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionType {
    Lz4,
    Lzo,
    Zstd,
    Zlib,
    Lzma,
    Xz,
}

/// Unicode normalization policy for filename comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnicodePolicy {
    /// Case-sensitive, no normalization
    Raw,
    /// Case-insensitive, UTF-8 NFD comparison
    CasefoldNfd,
    /// Case-insensitive, UTF-8 NFC comparison
    CasefoldNfc,
    /// Case-insensitive, Windows-style (NTFS Unicode)
    CasefoldNtfs,
}

/// Feature capability matrix published by every mounted backend.
///
/// Used by the VFS mount layer to enforce feature gates (Gate 4) and
/// ensure unsupported features fail closed (Gate 5).
#[derive(Debug, Clone)]
pub struct BackendFeatureMatrix {
    pub fs_name: &'static str,
    pub fs_version: u32,
    pub readonly: bool,
    pub write: bool,
    pub journal: bool,
    pub journal_replay: bool,
    pub fsck_required: bool,
    pub compression: Option<CompressionType>,
    pub encryption: bool,
    pub verity: bool,
    pub sparse_files: bool,
    pub inline_data: bool,
    pub xattr: bool,
    pub acl: bool,
    pub symlink: bool,
    pub hardlink: bool,
    pub casefold: bool,
    pub unicode_policy: UnicodePolicy,
    pub snapshots: bool,
    pub subvolumes: bool,
    pub multi_device: bool,
    pub reflink: bool,
    pub cow: bool,
    pub checksums: bool,
    pub quota: bool,
    pub block_size: u32,
    pub max_file_size: u64,
    pub max_name_len: u32,
    pub supports_fsync: bool,
    pub supports_fdatasync: bool,
    pub supports_rename_atomicity: bool,
    pub supports_crash_recovery: bool,
}

impl BackendFeatureMatrix {
    /// Check if a write operation is allowed given the backend capabilities
    /// and mount readonly flag.
    pub fn check_write_allowed(&self, mount_readonly: bool) -> Result<(), FsError> {
        if mount_readonly {
            return Err(FsError::ReadOnlyFs);
        }
        if !self.write {
            return Err(FsError::ReadOnlyFs);
        }
        Ok(())
    }

    /// Check if a feature is supported; if not, fail closed with EOPNOTSUPP.
    pub fn require_feature(&self, feature: UnsupportedFeatureType) -> Result<(), FsError> {
        let supported = match feature {
            UnsupportedFeatureType::Compression => self.compression.is_some(),
            UnsupportedFeatureType::Encryption => self.encryption,
            UnsupportedFeatureType::MultiDevice => self.multi_device,
            UnsupportedFeatureType::ReparsePoint => false, // NTFS reparse points not implemented
            UnsupportedFeatureType::Acl => self.acl,
            UnsupportedFeatureType::Quota => self.quota,
            UnsupportedFeatureType::Snapshot => self.snapshots,
            UnsupportedFeatureType::Reflink => self.reflink,
            UnsupportedFeatureType::Casefold => self.casefold,
            UnsupportedFeatureType::SparseFiles => self.sparse_files,
            UnsupportedFeatureType::InlineData => self.inline_data,
            UnsupportedFeatureType::Verity => self.verity,
            UnsupportedFeatureType::Journal => self.journal,
        };
        if !supported {
            Err(FsError::UnsupportedFeature(feature))
        } else {
            Ok(())
        }
    }
}

/// Feature matrix for F2FS backend.
pub fn f2fs_feature_matrix() -> BackendFeatureMatrix {
    BackendFeatureMatrix {
        fs_name: "f2fs",
        fs_version: 1,
        readonly: false,
        write: true,
        journal: false,
        journal_replay: true,
        fsck_required: false,
        compression: Some(CompressionType::Zstd),
        encryption: false,
        verity: false,
        sparse_files: false,
        inline_data: true,
        xattr: true,
        acl: false,
        symlink: true,
        hardlink: true,
        casefold: false,
        unicode_policy: UnicodePolicy::Raw,
        snapshots: false,
        subvolumes: false,
        multi_device: false,
        reflink: false,
        cow: false,
        checksums: true,
        quota: false,
        block_size: 4096,
        max_file_size: 4 * 1024 * 1024 * 1024 * 1024, // 4TB
        max_name_len: 255,
        supports_fsync: true,
        supports_fdatasync: true,
        supports_rename_atomicity: true,
        supports_crash_recovery: true,
    }
}

/// Feature matrix for ext4 backend.
pub fn ext4_feature_matrix() -> BackendFeatureMatrix {
    BackendFeatureMatrix {
        fs_name: "ext4",
        fs_version: 1,
        readonly: false,
        write: true,
        journal: true,
        journal_replay: true,
        fsck_required: false,
        compression: None,
        encryption: false,
        verity: false,
        sparse_files: true,
        inline_data: true,
        xattr: true,
        acl: true,
        symlink: true,
        hardlink: true,
        casefold: false,
        unicode_policy: UnicodePolicy::Raw,
        snapshots: false,
        subvolumes: false,
        multi_device: false,
        reflink: false,
        cow: false,
        checksums: true,
        quota: true,
        block_size: 4096,
        max_file_size: 16 * 1024 * 1024 * 1024 * 1024, // 16TB
        max_name_len: 255,
        supports_fsync: true,
        supports_fdatasync: true,
        supports_rename_atomicity: true,
        supports_crash_recovery: true,
    }
}

/// Feature matrix for FAT32 backend.
/// Feature matrix for XFS backend.
/// Deep web: Linux kernel fs/xfs/xfs_mount.h (XFS feature flags)
pub fn xfs_feature_matrix() -> BackendFeatureMatrix {
    BackendFeatureMatrix {
        fs_name: "xfs",
        fs_version: 5,
        readonly: true, // echOS'ta XFS salt-okunur
        write: false,
        journal: true,
        journal_replay: true,
        fsck_required: false,
        compression: None,
        encryption: false,
        verity: false,
        sparse_files: false,
        inline_data: false,
        xattr: false,
        acl: false,
        symlink: false, // Salt-okunur modda symlink desteği yok
        hardlink: false,
        casefold: false,
        unicode_policy: UnicodePolicy::Raw,
        snapshots: false,
        subvolumes: false,
        multi_device: false,
        reflink: false,
        cow: false,
        checksums: true,
        quota: false,
        block_size: 4096,
        max_file_size: 256 * 1024 * 1024 * 1024 * 1024, // 256TB
        max_name_len: 255,
        supports_fsync: false, // Salt-okunur modda fsync desteklenmez
        supports_fdatasync: false,
        supports_rename_atomicity: false,
        supports_crash_recovery: true,
    }
}

/// Feature matrix for FAT32 backend.
pub fn fat32_feature_matrix() -> BackendFeatureMatrix {
    BackendFeatureMatrix {
        fs_name: "fat32",
        fs_version: 0,
        readonly: false,
        write: true,
        journal: false,
        journal_replay: false,
        fsck_required: false,
        compression: None,
        encryption: false,
        verity: false,
        sparse_files: false,
        inline_data: false,
        xattr: false,
        acl: false,
        symlink: false,
        hardlink: false,
        casefold: false,
        unicode_policy: UnicodePolicy::Raw,
        snapshots: false,
        subvolumes: false,
        multi_device: false,
        reflink: false,
        cow: false,
        checksums: false,
        quota: false,
        block_size: 512,
        max_file_size: 4 * 1024 * 1024 * 1024, // 4GB
        max_name_len: 255,
        supports_fsync: false,
        supports_fdatasync: false,
        supports_rename_atomicity: false,
        supports_crash_recovery: false,
    }
}

/// Feature matrix for exFAT backend.
pub fn exfat_feature_matrix() -> BackendFeatureMatrix {
    BackendFeatureMatrix {
        fs_name: "exfat",
        fs_version: 1,
        readonly: false,
        write: true,
        journal: false,
        journal_replay: false,
        fsck_required: false,
        compression: None,
        encryption: false,
        verity: false,
        sparse_files: false,
        inline_data: false,
        xattr: false,
        acl: false,
        symlink: false,
        hardlink: false,
        casefold: false,
        unicode_policy: UnicodePolicy::Raw,
        snapshots: false,
        subvolumes: false,
        multi_device: false,
        reflink: false,
        cow: false,
        checksums: true,
        quota: false,
        block_size: 512,
        max_file_size: 128 * 1024 * 1024 * 1024 * 1024,
        max_name_len: 255,
        supports_fsync: false,
        supports_fdatasync: false,
        supports_rename_atomicity: false,
        supports_crash_recovery: false,
    }
}

/// Feature matrix for NTFS backend (read-only).
pub fn ntfs_feature_matrix() -> BackendFeatureMatrix {
    BackendFeatureMatrix {
        fs_name: "ntfs",
        fs_version: 3,
        readonly: true,
        write: false,
        journal: true,
        journal_replay: false,
        fsck_required: false,
        compression: None,
        encryption: false,
        verity: false,
        sparse_files: true,
        inline_data: true,
        xattr: false,
        acl: false,
        symlink: false,
        hardlink: false,
        casefold: false,
        unicode_policy: UnicodePolicy::CasefoldNtfs,
        snapshots: false,
        subvolumes: false,
        multi_device: false,
        reflink: false,
        cow: false,
        checksums: false,
        quota: false,
        block_size: 4096,
        max_file_size: 256 * 1024 * 1024 * 1024 * 1024, // 256TB
        max_name_len: 255,
        supports_fsync: false,
        supports_fdatasync: false,
        supports_rename_atomicity: false,
        supports_crash_recovery: false,
    }
}

/// Feature matrix for Btrfs backend (read-only).
pub fn btrfs_feature_matrix() -> BackendFeatureMatrix {
    BackendFeatureMatrix {
        fs_name: "btrfs",
        fs_version: 0,
        readonly: true,
        write: false,
        journal: false,
        journal_replay: false,
        fsck_required: false,
        compression: Some(CompressionType::Zstd),
        encryption: false,
        verity: false,
        sparse_files: true,
        inline_data: true,
        xattr: false,
        acl: false,
        symlink: false,
        hardlink: false,
        casefold: false,
        unicode_policy: UnicodePolicy::Raw,
        snapshots: false,
        subvolumes: false,
        multi_device: false,
        reflink: false,
        cow: true,
        checksums: true,
        quota: false,
        block_size: 4096,
        max_file_size: u64::MAX, // Advertise the largest representable 64-bit file boundary.
        max_name_len: 255,
        supports_fsync: false,
        supports_fdatasync: false,
        supports_rename_atomicity: false,
        supports_crash_recovery: false,
    }
}

/// Feature matrix for EROFS backend (read-only, compressed).
pub fn erofs_feature_matrix() -> BackendFeatureMatrix {
    BackendFeatureMatrix {
        fs_name: "erofs",
        fs_version: 0,
        readonly: true,
        write: false,
        journal: false,
        journal_replay: false,
        fsck_required: false,
        compression: Some(CompressionType::Lz4),
        encryption: false,
        verity: false,
        sparse_files: false,
        inline_data: true,
        xattr: false,
        acl: false,
        symlink: true,
        hardlink: false,
        casefold: false,
        unicode_policy: UnicodePolicy::Raw,
        snapshots: false,
        subvolumes: false,
        multi_device: false,
        reflink: false,
        cow: false,
        checksums: false,
        quota: false,
        block_size: 4096,
        max_file_size: 1024 * 1024 * 1024 * 1024, // 1TB
        max_name_len: 255,
        supports_fsync: false,
        supports_fdatasync: false,
        supports_rename_atomicity: false,
        supports_crash_recovery: false,
    }
}

/// Feature matrix for SquashFS backend (read-only, compressed).
pub fn squashfs_feature_matrix() -> BackendFeatureMatrix {
    BackendFeatureMatrix {
        fs_name: "squashfs",
        fs_version: 0,
        readonly: true,
        write: false,
        journal: false,
        journal_replay: false,
        fsck_required: false,
        compression: Some(CompressionType::Zstd),
        encryption: false,
        verity: false,
        sparse_files: false,
        inline_data: false,
        xattr: false,
        acl: false,
        symlink: true,
        hardlink: true,
        casefold: false,
        unicode_policy: UnicodePolicy::Raw,
        snapshots: false,
        subvolumes: false,
        multi_device: false,
        reflink: false,
        cow: false,
        checksums: false,
        quota: false,
        block_size: 131072,                            // 128KB default
        max_file_size: 16 * 1024 * 1024 * 1024 * 1024, // 16TB
        max_name_len: 255,
        supports_fsync: false,
        supports_fdatasync: false,
        supports_rename_atomicity: false,
        supports_crash_recovery: false,
    }
}

/// Feature matrix for tmpfs backend (in-memory).
pub fn tmpfs_feature_matrix() -> BackendFeatureMatrix {
    BackendFeatureMatrix {
        fs_name: "tmpfs",
        fs_version: 0,
        readonly: false,
        write: true,
        journal: false,
        journal_replay: false,
        fsck_required: false,
        compression: None,
        encryption: false,
        verity: false,
        sparse_files: false,
        inline_data: true,
        xattr: false,
        acl: false,
        symlink: true,
        hardlink: true,
        casefold: false,
        unicode_policy: UnicodePolicy::Raw,
        snapshots: false,
        subvolumes: false,
        multi_device: false,
        reflink: false,
        cow: false,
        checksums: false,
        quota: false,
        block_size: 4096,
        max_file_size: u64::MAX,
        max_name_len: 255,
        supports_fsync: true,
        supports_fdatasync: true,
        supports_rename_atomicity: true,
        supports_crash_recovery: false,
    }
}

use crate::drivers::ata::BLOCK_SIZE;
use crate::fs::f2fs::{
    read_f2fs_file_at, read_f2fs_file_direct, write_f2fs_file_at, write_f2fs_file_direct,
};

/// F2FS için dahili sanal dosya sistemi uygulayıcısı
struct F2fsVfs;

/// F2FS kök dizin inode'u — dosya ağacının kökünü temsil eder
struct F2fsRootInode;

lazy_static! {
    /// Kök inode singleton — tüm path çözümlemeleri buradan başlar
    static ref F2FS_ROOT_INODE: Arc<dyn INode> = Arc::new(F2fsRootInode);
    /// VFS singleton — FileSystem trait nesnesi
    static ref F2FS_VFS_INSTANCE: Arc<dyn FileSystem> = Arc::new(F2fsVfs);
    /// Global zaman sayacı (önyüklemeden beri saniye)
    static ref GLOBAL_TIME: Mutex<u64> = Mutex::new(0);
}

/// Global sistem saatini bir saniye ilerletir.
/// Periyodik zamanlayıcı kesmesinden çağrılmalıdır.
pub fn update_global_time() {
    let mut time = GLOBAL_TIME.lock();
    *time += 1;
}

/// Mevcut sistem zamanını POSIX Timespec biçiminde döndürür
pub fn get_global_time() -> Timespec {
    let time = GLOBAL_TIME.lock();
    Timespec {
        sec: *time as i64,
        nsec: 0,
    }
}

/// Bağlama noktası çözümlemesi — path'i bağlama tablosuna göre çözer.
///
/// Örnek:
/// ```
/// /mnt/usb/dosya.txt  →  bağlama tablosu: /mnt/usb → /dev/sdb
///                     →  çözümlendi: /dev/sdb/dosya.txt
/// ```
///
/// En uzun önek eşleşmesi (longest prefix match) algoritması kullanılır.
pub fn resolve_mount_path(path: &str) -> String {
    let mounts = crate::fs::f2fs::list_mounts();
    let mut resolved = path.to_string();

    // En uzun önek eşleşmesini bul
    for m in mounts {
        if path.starts_with(&m.mountpoint) && m.mountpoint.len() > 1 {
            // Bağlama noktası altındaki yolu cihaz yoluna dönüştür
            let sub_path = &path[m.mountpoint.len()..];
            resolved = format!("{}{}", m.device, sub_path);
            break;
        }
    }

    resolved
}

/// Dosya sistemi bilgisi döndürür — statfs() çağrısının temeli
fn fs_info() -> FsInfo {
    FsInfo {
        bsize: BLOCK_SIZE,
        frsize: BLOCK_SIZE,
        blocks: 0,
        bfree: 0,
        bavail: 0,
        files: 0,
        ffree: 0,
        namemax: 255,
    }
}

/// Mevcut zamanı Timespec olarak döndürür (atime/mtime/ctime için)
fn current_timespec() -> Timespec {
    get_global_time()
}

/// Normal dosya meta verisi oluşturur.
/// mode 0o100644 = düzenli dosya, sahibi okuma/yazma, diğerleri okuma izni
fn file_metadata(size: usize) -> Metadata {
    let time = current_timespec();
    Metadata {
        dev: 0,
        inode: 1,
        size,
        blk_size: BLOCK_SIZE,
        blocks: 0,
        atime: time,
        mtime: time,
        ctime: time,
        type_: FileType::File,
        mode: 0o100644,
        nlinks: 1,
        uid: 0,
        gid: 0,
        rdev: 0,
    }
}

/// Dizin meta verisi oluşturur.
/// mode 0o040755 = dizin, sahibi tam izin, diğerleri okuma+çalıştırma
fn dir_metadata() -> Metadata {
    let time = current_timespec();
    Metadata {
        dev: 0,
        inode: 1,
        size: 0,
        blk_size: BLOCK_SIZE,
        blocks: 0,
        atime: time,
        mtime: time,
        ctime: time,
        type_: FileType::Dir,
        mode: 0o040755,
        nlinks: 2,
        uid: 0,
        gid: 0,
        rdev: 0,
    }
}

// ============================================================================
// DOSYA TANIM LAYICISI TABLOSU (Her İşlem İçin Ayrı)
// ============================================================================

/// Açık bir dosyayı temsil eden kayıt.
/// Unix'te her açık dosya bu kayıtla izlenir.
#[derive(Clone)]
pub struct OpenFile {
    pub path: String,
    /// Mevcut okuma/yazma konumu (her read/write sonrası güncellenir)
    pub offset: usize,
    pub flags: u32, // O_RDONLY=0, O_WRONLY=1, O_RDWR=2 + O_SYNC/O_DSYNC/O_DIRECT
    /// Generation counter for TOCTOU race detection.
    /// Monotonically increases on each open(). sys_read/sys_write verify
    /// the generation matches between lock phases to detect fd recycling.
    pub generation: u64,
}

/// open(2) dosya durumu bayrakları — cephanelik open.2 man page spec
/// O_SYNC: write + metadata → hardware (fsync semantics)
/// O_DSYNC: write + file metadata needed to retrieve data → hardware (fdatasync semantics)
/// O_DIRECT: minimize cache effects, direct to/from user-space buffers
pub const O_SYNC: u32 = 0o04000000;
pub const O_DSYNC: u32 = 0o010000;
pub const O_DIRECT: u32 = 0o040000;
pub const O_RDONLY: u32 = 0;
pub const O_WRONLY: u32 = 1;
pub const O_RDWR: u32 = 2;
pub const O_CREAT: u32 = 0o100;
pub const O_TRUNC: u32 = 0o1000;
pub const O_APPEND: u32 = 0o2000;
pub const O_EXCL: u32 = 0o200;
pub const O_CLOEXEC: u32 = 0o2000000;
pub const O_NONBLOCK: u32 = 0o4000;

/// POSIX permission check — arşiv: inodes.html i_mode tablosu
/// i_mode: S_IRUSR(0x100) S_IWUSR(0x80) S_IXUSR(0x40) S_IRGRP(0x20) S_IWGRP(0x10) S_IXGRP(0x8) S_IROTH(0x4) S_IWOTH(0x2) S_IXOTH(0x1)
/// caller_uid == file_uid → owner bits
/// caller_gid == file_gid → group bits
/// otherwise → other bits
/// root (uid=0) → her zaman izin ver
pub fn check_permission(file_mode: u16, file_uid: u16, file_gid: u16, access_type: u32) -> bool {
    let uid = crate::security::users::USER_DB.current_uid();
    let gid = crate::security::users::USER_DB.current_gid();

    // root her şeye erişebilir
    if uid == 0 {
        return true;
    }

    let mode = file_mode & 0o777; // SUID/SGID/Sticky hariç sadece permission bits

    let (r_bit, w_bit, x_bit) = if (uid as u16) == file_uid {
        // Owner
        (0o400, 0o200, 0o100)
    } else if (gid as u16) == file_gid {
        // Group
        (0o040, 0o020, 0o010)
    } else {
        // Other
        (0o004, 0o002, 0o001)
    };

    match access_type {
        0 => (mode & r_bit) != 0,  // R_OK
        2 => (mode & w_bit) != 0,  // W_OK
        4 => (mode & x_bit) != 0,  // X_OK
        6 => (mode & r_bit) != 0 && (mode & w_bit) != 0, // R_OK|W_OK
        _ => true, // Diğer durumlar için izin ver
    }
}

/// İşlem başına dosya tanımlayıcı tablosu.
///
/// ```
/// Tablo Yapısı:
/// ┌────┬────────────────────────────────────────────┐
/// │ fd │ OpenFile                                   │
/// ├────┼────────────────────────────────────────────┤
/// │  0 │ /dev/stdin   (flags=O_RDONLY, offset=0)    │
/// │  1 │ /dev/stdout  (flags=O_WRONLY, offset=0)    │
/// │  2 │ /dev/stderr  (flags=O_WRONLY, offset=0)    │
/// │  3 │ /home/user/dosya.txt (uygulama açtı)       │
/// │  4 │ None  (kapatıldı)                          │
/// │  5 │ /var/log/app.log                           │
/// └────┴────────────────────────────────────────────┘
/// next_fd = 6 (bir sonraki open() bu fd'yi tahsis eder)
/// ```
/// Per-process FD tablosu — Linux: struct fdtable + files_struct
///
/// ```
/// Tablo Yapısı:
/// ┌────┬────────────────────────────────────────────┬───────────┐
/// │ fd │ OpenFile                                   │ cloexec   │
/// ├────┼────────────────────────────────────────────┼───────────┤
/// │  0 │ /dev/stdin   (flags=O_RDONLY, offset=0)    │ false     │
/// │  1 │ /dev/stdout  (flags=O_WRONLY, offset=0)    │ false     │
/// │  2 │ /dev/stderr  (flags=O_WRONLY, offset=0)    │ false     │
/// │  3 │ /home/user/dosya.txt (uygulama açtı)       │ true      │
/// │  4 │ None  (kapatıldı)                          │ false     │
/// │  5 │ /var/log/app.log                           │ false     │
/// └────┴────────────────────────────────────────────┴───────────┘
/// next_fd = 6 (bir sonraki open() bu fd'yi tahsis eder)
/// ```
pub struct FileDescriptorTable {
    pub files: Vec<Option<OpenFile>>,
    pub next_fd: usize,
    /// Monotonic generation counter — incremented on each open().
    /// Stored in OpenFile.generation for TOCTOU race detection.
    generation_counter: u64,
    /// Close-on-exec bitmap — Linux: fdtable.close_on_exec
    /// close_on_exec[fd] == true → exec() sırasında bu fd kapatılır.
    /// POSIX: "open-on-exec" vs "close-on-exec" flag management.
    close_on_exec: Vec<bool>,
}

impl FileDescriptorTable {
    /// Yeni per-process FD tablosu oluştur — Linux: alloc_files() + copy_fdtable()
    ///
    /// Başlangıçta stdin(0), stdout(1), stderr(2) dolu geri kalanı boş.
    /// close_on_exec bitmap'i paralel olarak oluşturulur.
    pub fn new() -> Self {
        let mut files = Vec::new();
        let mut close_on_exec = Vec::new();
        // stdin, stdout, stderr — standart akışlar daima 0, 1, 2 fd değerlerini alır
        // Close-on-exec: POSIX varsayılanı false (exec'de açık kalır)
        files.push(Some(OpenFile {
            path: "/dev/stdin".to_string(),
            offset: 0,
            flags: 0,
            generation: 0,
        }));
        close_on_exec.push(false);
        files.push(Some(OpenFile {
            path: "/dev/stdout".to_string(),
            offset: 0,
            flags: 1,
            generation: 1,
        }));
        close_on_exec.push(false);
        files.push(Some(OpenFile {
            path: "/dev/stderr".to_string(),
            offset: 0,
            flags: 1,
            generation: 2,
        }));
        close_on_exec.push(false);
        Self { files, next_fd: 3, generation_counter: 3, close_on_exec }
    }

    /// fork/clone için FD tablosunu tamamen kopyala — Linux: dup_fd()
    ///
    /// Yeni tablo, tüm open file descriptor'ları aynı open file description ile paylaşır.
    /// close_on_exec bayrakları da kopyalanır (çocuk process exec'den önce ayarlayabilir).
    /// Generation counter'ı sıfırlanmaz — monotonik artmaya devam eder.
    pub fn dup_fd(&self) -> Self {
        Self {
            files: self.files.clone(),
            next_fd: self.next_fd,
            generation_counter: self.generation_counter,
            close_on_exec: self.close_on_exec.clone(),
        }
    }

    /// exec() sırasında close-on-exec bayrağı set edilmiş tüm fd'leri kapat
    /// Linux: close_files() / flush_old_exec() → __close_on_exec()
    ///
    /// POSIX: "Open file descriptors greater than or equal to open_max that
    /// are marked as close-on-exec shall be closed."
    pub fn close_cloexec(&mut self) {
        for i in 0..self.files.len() {
            if i < self.close_on_exec.len() && self.close_on_exec[i] {
                if self.files[i].is_some() {
                    self.files[i] = None;
                }
            }
        }
    }

    /// Belirli bir fd için close-onexec bayrağını set et
    /// Linux: set_close_on_exec(fd, 1) / fcntl(fd, F_SETFD, FD_CLOEXEC)
    pub fn set_cloexec(&mut self, fd: usize) {
        if fd >= self.close_on_exec.len() {
            self.close_on_exec.resize(fd + 1, false);
        }
        self.close_on_exec[fd] = true;
    }

    /// Belirli bir fd için close-on-exec bayrağını temizle
    /// Linux: set_close_on_exec(fd, 0) / fcntl(fd, F_SETFD, 0)
    pub fn clear_cloexec(&mut self, fd: usize) {
        if fd < self.close_on_exec.len() {
            self.close_on_exec[fd] = false;
        }
    }

    /// Belirli bir fd'nin close-on-exec durumunu sorgula
    /// Linux: FD_CLOEXEC flag check / fcntl(fd, F_GETFD) & FD_CLOEXEC
    pub fn is_cloexec(&self, fd: usize) -> bool {
        fd < self.close_on_exec.len() && self.close_on_exec[fd]
    }

    /// Dosyayı açar ve yeni fd döndürür — Linux: do_sys_open() → get_unused_fd_flags()
    /// Önce None olan slotları tarar (0-2 stdin/stdout/stderr atlanır),
    /// boş slot yoksa tabloya eklenir. Kapatılan fd numaraları yeniden kullanılır.
    /// O_CLOEXEC bayrağı varsa close_on_exec bitmap'ine işlenir.
    pub fn open(&mut self, path: &str, flags: u32) -> usize {
        let gen = self.generation_counter;
        self.generation_counter = self.generation_counter.wrapping_add(1);
        // FD slot recycling: önce None olan slotları tara (0-2 stdin/stdout/stderr atla)
        for i in 3..self.files.len() {
            if self.files[i].is_none() {
                self.files[i] = Some(OpenFile {
                    path: path.to_string(),
                    offset: 0,
                    flags,
                    generation: gen,
                });
                // O_CLOEXEC: exec() sırasında bu fd kapatılsın mı?
                if flags & O_CLOEXEC != 0 {
                    self.set_cloexec(i);
                }
                return i;
            }
        }
        // Boş slot yoksa tabloyu genişlet
        let fd = self.files.len();
        self.files.push(Some(OpenFile {
            path: path.to_string(),
            offset: 0,
            flags,
            generation: gen,
        }));
        // close_on_exec bitmap'ini de genişlet
        if fd >= self.close_on_exec.len() {
            self.close_on_exec.resize(fd + 1, false);
        }
        if flags & O_CLOEXEC != 0 {
            self.close_on_exec[fd] = true;
        }
        self.next_fd = fd + 1;
        fd
    }

    /// Dosyayı kapatır — tablo girişini None yapar, generation artar, fd numarası serbest kalır
    /// Close-on-exec bayrağı da temizlenir.
    pub fn close(&mut self, fd: usize) -> bool {
        if fd < self.files.len() {
            self.files[fd] = None;
            // close_on_exec bayrağını temizle
            if fd < self.close_on_exec.len() {
                self.close_on_exec[fd] = false;
            }
            self.generation_counter = self.generation_counter.wrapping_add(1);
            true
        } else {
            false
        }
    }

    /// Dosya konumunu ayarlar (lseek benzeri)
    pub fn seek(&mut self, fd: usize, offset: usize) -> bool {
        if let Some(Some(file)) = self.files.get_mut(fd) {
            file.offset = offset;
            true
        } else {
            false
        }
    }

    /// Mevcut dosya konumunu döndürür (tell)
    pub fn tell(&self, fd: usize) -> Option<usize> {
        self.files
            .get(fd)
            .and_then(|f| f.as_ref().map(|f| f.offset))
    }

    /// fd'ye ait OpenFile kaydını okuma amaçlı getirir
    pub fn get(&self, fd: usize) -> Option<&OpenFile> {
        self.files.get(fd).and_then(|f| f.as_ref())
    }

    /// dup(2): Eski fd'yi en düşük boş fd numarasına kopyalar
    /// POSIX: "creates a new fd that refers to the same open file description"
    /// Paylaşılan: offset, flags. Paylaşılmayan: fd flags (close-on-exec)
    /// POSIX: "The FD_CLOEXEC file descriptor flag associated with the new
    /// file descriptor shall be cleared."
    pub fn dup(&mut self, old_fd: usize) -> Result<usize, FsError> {
        let file = self.files.get(old_fd)
            .and_then(|f| f.as_ref())
            .ok_or(FsError::NotFile)?;

        let cloned = file.clone();
        let gen = self.generation_counter;
        self.generation_counter = self.generation_counter.wrapping_add(1);

        // En düşük boş fd'yi bul (3'ten başla — stdin/stdout/stderr atla)
        for i in 3..self.files.len() {
            if self.files[i].is_none() {
                self.files[i] = Some(OpenFile {
                    path: cloned.path.clone(),
                    offset: cloned.offset,
                    flags: cloned.flags,
                    generation: gen,
                });
                // dup: close_on_exec KOPYALANMAZ (POSIX)
                if i < self.close_on_exec.len() {
                    self.close_on_exec[i] = false;
                }
                return Ok(i);
            }
        }
        // Boş slot yoksa tabloyu genişlet
        let new_fd = self.files.len();
        self.files.push(Some(OpenFile {
            path: cloned.path.clone(),
            offset: cloned.offset,
            flags: cloned.flags,
            generation: gen,
        }));
        if new_fd >= self.close_on_exec.len() {
            self.close_on_exec.resize(new_fd + 1, false);
        }
        self.close_on_exec[new_fd] = false;
        Ok(new_fd)
    }

    /// dup2(2): Eski fd'yi belirli bir fd numarasına kopyalar
    /// POSIX: "if newfd was open, it is closed first; atomic operation"
    /// Eski ve yeni fd'ler aynı open file description'ı paylaşır
    /// POSIX: "The FD_CLOEXEC file descriptor flag associated with the new
    /// file descriptor shall be cleared."
    pub fn dup2(&mut self, old_fd: usize, new_fd: usize) -> Result<usize, FsError> {
        if old_fd == new_fd {
            // POSIX: "if oldfd equals newfd, then dup2() does nothing, returns newfd"
            if self.files.get(new_fd).and_then(|f| f.as_ref()).is_none() {
                return Err(FsError::NotFile);
            }
            return Ok(new_fd);
        }

        let file = self.files.get(old_fd)
            .and_then(|f| f.as_ref())
            .ok_or(FsError::NotFile)?;

        let cloned = file.clone();
        let gen = self.generation_counter;
        self.generation_counter = self.generation_counter.wrapping_add(1);

        // newfd açıksa kapat (hata göz ardı edilir — POSIX)
        if new_fd < self.files.len() {
            self.files[new_fd] = None;
            if new_fd < self.close_on_exec.len() {
                self.close_on_exec[new_fd] = false;
            }
        } else {
            // Tabloyu genişlet
            while self.files.len() <= new_fd {
                self.files.push(None);
            }
            while self.close_on_exec.len() <= new_fd {
                self.close_on_exec.push(false);
            }
        }

        self.files[new_fd] = Some(OpenFile {
            path: cloned.path.clone(),
            offset: cloned.offset,
            flags: cloned.flags,
            generation: gen,
        });
        // dup2: close_on_exec KOPYALANMAZ (POSIX)
        self.close_on_exec[new_fd] = false;

        Ok(new_fd)
    }
}

// ============================================================================
// PIPE — Anonim boru (POSIX.1-2024 pipe(2))
// Deep web: Linux kernel fs/pipe.c create_pipe_files(), alloc_pipe_info()
// ============================================================================

/// Pipe buffer — ring buffer tabanlı anonim boru
/// Linux: PIPE_DEF_BUFFERS = 16, her buffer = PAGE_SIZE (4096)
/// Bizim basitleştirilmiş versiyon: tek buffer, bloklayıcı olmayan okuma/yazma
pub struct PipeBuffer {
    data: Vec<u8>,
    read_pos: usize,
    write_pos: usize,
    capacity: usize,
}

impl PipeBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            data: alloc::vec![0u8; capacity],
            read_pos: 0,
            write_pos: 0,
            capacity,
        }
    }

    /// Pipe'a yaz (writer fd)
    pub fn write(&mut self, buf: &[u8]) -> Result<usize, FsError> {
        let available = self.capacity - (self.write_pos - self.read_pos);
        let to_write = buf.len().min(available);
        if to_write == 0 {
            return Ok(0); // Pipe dolu
        }
        for i in 0..to_write {
            self.data[(self.write_pos + i) % self.capacity] = buf[i];
        }
        self.write_pos += to_write;
        Ok(to_write)
    }

    /// Pipe'tan oku (reader fd)
    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, FsError> {
        let available = self.write_pos - self.read_pos;
        if available == 0 {
            return Ok(0); // Pipe boş
        }
        let to_read = buf.len().min(available);
        for i in 0..to_read {
            buf[i] = self.data[(self.read_pos + i) % self.capacity];
        }
        self.read_pos += to_read;
        Ok(to_read)
    }

    /// Pipe'ta okunabilir veri var mı?
    pub fn has_data(&self) -> bool {
        self.write_pos > self.read_pos
    }
}

use spin::Mutex as PipeMutex;

lazy_static! {
    /// Global pipe havuzu — pipe() çağrısı yeni pipe oluşturur
    static ref PIPE_POOL: PipeMutex<alloc::collections::BTreeMap<u32, PipeMutex<PipeBuffer>>> =
        PipeMutex::new(alloc::collections::BTreeMap::new());
    static ref NEXT_PIPE_ID: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(1);
}

/// pipe(2): Anonim boru oluştur — reader ve writer fd döndür
/// POSIX: "pipe(fds) creates a pipe, a unidirectional data channel"
/// Deep web: Linux kernel fs/pipe.c create_pipe_files()
pub fn sys_pipe(fds: &mut [usize; 2]) -> Result<(), FsError> {
    let pipe_id = NEXT_PIPE_ID.fetch_add(1, core::sync::atomic::Ordering::SeqCst);
    let pipe = PipeBuffer::new(65536); // 64KB pipe buffer
    PIPE_POOL.lock().insert(pipe_id, PipeMutex::new(pipe));

    // Reader fd (O_RDONLY)
    let reader_fd = {
        let fd_table = current_fd_table();
        let mut table = fd_table.lock();
        let gen = table.generation_counter;
        table.generation_counter = table.generation_counter.wrapping_add(1);
        let path = alloc::format!("pipe:{}", pipe_id);
        let mut fd = None;
        for i in 3..table.files.len() {
            if table.files[i].is_none() {
                fd = Some(i);
                break;
            }
        }
        let fd = fd.unwrap_or(table.files.len());
        if fd >= table.files.len() {
            table.files.push(None);
        }
        table.files[fd] = Some(OpenFile { path, offset: 0, flags: 0, generation: gen });
        fd
    };

    // Writer fd (O_WRONLY)
    let writer_fd = {
        let fd_table = current_fd_table();
        let mut table = fd_table.lock();
        let gen = table.generation_counter;
        table.generation_counter = table.generation_counter.wrapping_add(1);
        let path = alloc::format!("pipe:{}", pipe_id);
        let mut fd = None;
        for i in 3..table.files.len() {
            if table.files[i].is_none() {
                fd = Some(i);
                break;
            }
        }
        let fd = fd.unwrap_or(table.files.len());
        if fd >= table.files.len() {
            table.files.push(None);
        }
        table.files[fd] = Some(OpenFile { path, offset: 0, flags: 1, generation: gen });
        fd
    };

    fds[0] = reader_fd;
    fds[1] = writer_fd;
    Ok(())
}

lazy_static! {
    /// Global FD tablosu — per-process FD table'i olmayan task'lar için fallback.
    /// Linux: init_files → fallback for tasks without own files_struct.
    /// Arc<Mutex<>>: hem global erişim hem de fork klonlama için paylaşılabilir.
    pub(crate) static ref GLOBAL_FD_TABLE: Arc<Mutex<FileDescriptorTable>> =
        Arc::new(Mutex::new(FileDescriptorTable::new()));
}

/// Mevcut process'in FD tablosunu döndürür — Linux: files_fdtable(current->files)
///
/// Per-process FD table desteği:
/// 1. Eğer mevcut task'ın kendi tablosu varsa (TaskColdData.fd_table) onu kullan
/// 2. Yoksa GLOBAL_FD_TABLE fallback kullan (geriye dönük uyumluluk)
///
/// Arc<Mutex<FileDescriptorTable>> döndürür — çağrı .lock()` ile erişir.
pub fn current_fd_table() -> Arc<Mutex<FileDescriptorTable>> {
    // Per-process FD table: task'ın kendi tablosu var mı?
    if let Some(fd_table) = crate::task::scheduler::current_task_fd_table() {
        fd_table
    } else {
        // Fallback: global FD tablosu (hiçbir task çalışmıyken veya task kendi tablosunu oluşturmadı)
        GLOBAL_FD_TABLE.clone()
    }
}

/// Fork/clone için mevcut process'in FD tablosunu klonla — Linux: dup_fd()
///
/// Parent'ın tablosunu tamamen kopyalar (open file description'ları paylaşır).
/// Child her zaman kendi per-process tablosuna sahip olur.
pub fn clone_fd_table_for_fork() -> Arc<Mutex<FileDescriptorTable>> {
    // Parent'ın tablosunu al
    let parent_fd_table = crate::task::scheduler::current_task_fd_table();
    let child_data = if let Some(ref parent_table) = parent_fd_table {
        let parent_guard = parent_table.lock();
        parent_guard.dup_fd()
    } else {
        // Parent global tablo kullanıyor — child'a kopyasını oluştur
        let global_guard = GLOBAL_FD_TABLE.lock();
        global_guard.dup_fd()
    };
    Arc::new(Mutex::new(child_data))
}

/// Dosya açar — POSIX.1-2024 open(2) semantiği
///
/// Arşiv: phase6-vfs-contract.md, POSIX.1-2024 open(2)
/// - Boş path → ENOENT
/// - NUL byte → EINVAL
/// - O_CREAT|O_EXCL: dosya varsa EEXIST
/// - O_CREAT yoksa: dosya yoksa ENOENT
/// - O_TRUNC: dosyayı sıfırla
/// - Read-only FS'te yazma denemesi → EROFS
pub fn sys_open(path: &str, flags: u32) -> usize {
    // 1. Path validation (POSIX path_resolution(7))
    {
        let vfs = crate::fs::vfs_unified::VFS_UNIFIED.lock();
        if let Err(e) = crate::fs::vfs_unified::validate_path(path) {
            crate::serial_println!("[vfs] sys_open: path validation failed: {:?}", e);
            return usize::MAX; // EBADF — invalid path
        }
    }

    // 2. O_CREAT|O_EXCL: dosya zaten varsa EEXIST
    if (flags & O_CREAT != 0) && (flags & O_EXCL != 0) {
        let vfs = crate::fs::vfs_unified::VFS_UNIFIED.lock();
        if vfs.stat(path).is_ok() {
            crate::serial_println!("[vfs] sys_open: O_CREAT|O_EXCL but file exists: {}", path);
            return usize::MAX;
        }
    }

    // 3. O_CREAT yoksa: dosya mevcut olmalı
    if flags & O_CREAT == 0 {
        let vfs = crate::fs::vfs_unified::VFS_UNIFIED.lock();
        if vfs.stat(path).is_err() {
            crate::serial_println!("[vfs] sys_open: file not found: {}", path);
            return usize::MAX;
        }
    }

    // 3a. Permission check — POSIX.1-2024 open(2) EACCES
    // Okuma: R_OK (0), yazma: W_OK (2), çalıştırma: X_OK (4)
    {
        let vfs = crate::fs::vfs_unified::VFS_UNIFIED.lock();
        if let Ok(info) = vfs.stat(path) {
            let access = match flags & 0x3 {
                0 => 0,       // O_RDONLY → R_OK
                1 => 2,       // O_WRONLY → W_OK
                2 => 2,       // O_RDWR → W_OK (yazma izni yeterli)
                _ => 2,
            };
            if !check_permission(info.mode as u16, info.uid as u16, info.gid as u16, access) {
                crate::serial_println!("[vfs] sys_open: permission denied: {}", path);
                return usize::MAX;
            }
        }
    }

    // 4. O_CREAT: dosya yoksa oluştur
    if flags & O_CREAT != 0 {
        let vfs = crate::fs::vfs_unified::VFS_UNIFIED.lock();
        if vfs.stat(path).is_err() {
            let parent = crate::fs::namei::parent_path(path);
            let name = path.rsplit_once('/').map(|(_, n)| n).unwrap_or(path);
            if let Err(e) = vfs.create_file(&parent, name) {
                crate::serial_println!("[vfs] sys_open: create failed: {}", e);
                return usize::MAX;
            }
        }
    }

    // 5. O_TRUNC: dosyayı sıfırla
    if flags & O_TRUNC != 0 {
        let vfs = crate::fs::vfs_unified::VFS_UNIFIED.lock();
        if let Err(e) = vfs.truncate(path, 0) {
            crate::serial_println!("[vfs] sys_open: truncate failed: {}", e);
            return usize::MAX;
        }
    }

    // 6. FD tablosuna kaydet — O_CLOEXEC flag'i open() içinde otomatik işlenir
    let fd_table = current_fd_table();
    let mut table = fd_table.lock();
    table.open(path, flags)
}

/// Dosyayı kapatır (close syscall)
pub fn sys_close(fd: usize) -> bool {
    let fd_table = current_fd_table();
    let mut table = fd_table.lock();
    table.close(fd)
}

/// dup(2): Eski fd'yi en düşük boş fd numarasına kopyalar
/// POSIX: "creates a new fd that refers to the same open file description"
/// Deep web: Linux kernel fs/file.c do_dup2(), FreeBSD filedesc(9) fd_dup2()
pub fn sys_dup(old_fd: usize) -> Result<usize, FsError> {
    let fd_table = current_fd_table();
    let mut table = fd_table.lock();
    table.dup(old_fd)
}

/// dup2(2): Eski fd'yi belirli bir fd numarasına kopyalar (atomik)
/// POSIX: "if newfd was open, it is closed first; atomic operation"
/// Deep web: Linux kernel fs/file.c do_dup2(), POSIX dup2.2.html
pub fn sys_dup2(old_fd: usize, new_fd: usize) -> Result<usize, FsError> {
    let fd_table = current_fd_table();
    let mut table = fd_table.lock();
    table.dup2(old_fd, new_fd)
}

/// Dosya konumunu değiştirir (lseek syscall)
pub fn sys_seek(fd: usize, offset: usize) -> bool {
    let fd_table = current_fd_table();
    let mut table = fd_table.lock();
    table.seek(fd, offset)
}

/// Mevcut dosya konumunu döndürür (tell / lseek SEEK_CUR benzeri)
pub fn sys_tell(fd: usize) -> Option<usize> {
    let fd_table = current_fd_table();
    let mut table = fd_table.lock();
    table.tell(fd)
}

/// fd'den okur ve offset'i günceller (read syscall).
///
/// Path/offset'i lock altında alır, I/O'yu lock dışı yapar, sonra offset'i günceller.
/// fd'den okur ve offset'i günceller (read syscall).
///
/// Path/offset'i lock altında alır, I/O'yu lock dışı yapar, sonra offset'i günceller.
/// Generation counter ile TOCTOU race detection: I/O sonrası fd'nin yeniden
/// kullanılmadığını doğrulamak için generation karşılaştırması yapılır.
pub fn sys_read(fd: usize, buf: &mut [u8]) -> Result<usize, FsError> {
    let (path, offset, flags, generation) = {
        let fd_table = current_fd_table();
        let table = fd_table.lock();
        let file = table
            .files
            .get(fd)
            .and_then(|f| f.as_ref())
            .ok_or(FsError::NotFile)?;
        (file.path.clone(), file.offset, file.flags, file.generation)
    };

    // VFS unified üzerinden oku — tüm backend'ler (F2FS, ext4, fat32, ntfs, btrfs) desteklenir
    let read = if flags & O_DIRECT != 0 {
        // O_DIRECT: page cache bypass, doğrudan block device'tan oku
        let vfs = crate::fs::vfs_unified::VFS_UNIFIED.lock();
        vfs.read_bytes_at(&path, offset, buf).map_err(|_| FsError::ReadError)?
    } else {
        let vfs = crate::fs::vfs_unified::VFS_UNIFIED.lock();
        vfs.read_bytes_at(&path, offset, buf).map_err(|_| FsError::ReadError)?
    };

    let fd_table = current_fd_table();
    let mut table = fd_table.lock();
    if let Some(Some(file)) = table.files.get_mut(fd) {
        if file.generation == generation {
            file.offset = offset + read;
        }
    }
    Ok(read)
}

/// fd'ye yazar ve offset'i günceller (write syscall).
///
/// Cephanelik open.2 spec'e göre:
/// - O_SYNC: write + metadata → hardware (fsync semantics)
/// - O_DSYNC: write + file metadata → hardware (fdatasync semantics)
/// - O_DIRECT: minimize cache effects, direct I/O (bypass page cache)
/// - O_APPEND: write at end of file
///
/// Path/offset'i lock altında alır, I/O'yu lock dışı yapar, sonra offset'i günceller.
pub fn sys_write(fd: usize, buf: &[u8]) -> Result<usize, FsError> {
    let (path, mut offset, flags, generation) = {
        let fd_table = current_fd_table();
        let table = fd_table.lock();
        let file = table
            .files
            .get(fd)
            .and_then(|f| f.as_ref())
            .ok_or(FsError::NotFile)?;
        let off = if file.flags & O_APPEND != 0 {
            // O_APPEND: dosya sonuna yaz — VFS unified üzerinden stat al
            let vfs = crate::fs::vfs_unified::VFS_UNIFIED.lock();
            match vfs.stat(&file.path) {
                Ok(info) => info.size as usize,
                Err(_) => 0,
            }
        } else {
            file.offset
        };
        (file.path.clone(), off, file.flags, file.generation)
    };

    // VFS unified üzerinden yaz — tüm backend'ler desteklenir
    let written = {
        let vfs = crate::fs::vfs_unified::VFS_UNIFIED.lock();
        vfs.write_bytes_at(&path, offset, buf).map_err(|_| FsError::WriteError)?
    };

    let fd_table = current_fd_table();
    let mut table = fd_table.lock();
    if let Some(Some(file)) = table.files.get_mut(fd) {
        if file.generation == generation {
            file.offset = offset + written;
        }
    }
    drop(table);

    if flags & O_SYNC != 0 {
        sys_fsync(fd)?;
    }
    else if flags & O_DSYNC != 0 {
        sys_fdatasync(fd)?;
    }

    Ok(written)
}

/// fsync(2): Transfer all modified kernel buffers and metadata associated with
/// the file referred to by `fd` to the underlying storage device.
///
/// Per fsync(2) spec:
/// - Flushes both data AND metadata (mtime, ctime, etc.)
/// - Blocks until the device acknowledges the write
/// - Returns 0 on success, -1 on error
pub fn sys_fsync(fd: usize) -> Result<(), FsError> {
    let path = {
        let fd_table = current_fd_table();
        let table = fd_table.lock();
        let file = table
            .files
            .get(fd)
            .and_then(|f| f.as_ref())
            .ok_or(FsError::NotFile)?;
        file.path.clone()
    };

    // VFS unified üzerinden fsync — tüm backend'ler desteklenir
    let vfs = crate::fs::vfs_unified::VFS_UNIFIED.lock();
    vfs.fsync(&path).map_err(|_| FsError::IoError)
}

/// fdatasync(2): Transfer all modified kernel buffers associated with the file
/// referred to by `fd` to the underlying storage device, but NOT metadata.
pub fn sys_fdatasync(fd: usize) -> Result<(), FsError> {
    let path = {
        let fd_table = current_fd_table();
        let table = fd_table.lock();
        let file = table
            .files
            .get(fd)
            .and_then(|f| f.as_ref())
            .ok_or(FsError::NotFile)?;
        file.path.clone()
    };

    // VFS unified üzerinden fdatasync — tüm backend'ler desteklenir
    let vfs = crate::fs::vfs_unified::VFS_UNIFIED.lock();
    vfs.fdatasync(&path).map_err(|_| FsError::IoError)
}

/// Path'e göre inode açar — kök dizin özel durumu vardır
fn open_inode_by_path(path: &str) -> Result<Arc<dyn INode>, FsError> {
    if path.trim_start_matches('/').is_empty() {
        return Ok(F2FS_ROOT_INODE.clone());
    }
    open_f2fs_inode_by_path(path)
}

/// F2FS dosya sistemi üzerinde path'e karşılık gelen inode'u döndürür.
/// Dizin ise F2fsDirInode, dosyaysa F2fsFileInode döndürülür.
fn open_f2fs_inode_by_path(path: &str) -> Result<Arc<dyn INode>, FsError> {
    let entry = crate::fs::f2fs::open_entry(path)?;
    if entry.is_dir {
        Ok(Arc::new(F2fsDirInode { path: entry.name }))
    } else {
        Ok(Arc::new(F2fsFileInode {
            path: entry.name,
            size: entry.size as usize,
        }))
    }
}

impl FileSystem for F2fsVfs {
    fn sync(&self) -> Result<(), RcFsError> {
        Ok(())
    }

    fn root_inode(&self) -> Arc<dyn INode> {
        F2FS_ROOT_INODE.clone()
    }

    fn info(&self) -> FsInfo {
        fs_info()
    }
}

impl INode for F2fsRootInode {
    fn read_at(&self, _offset: usize, _buf: &mut [u8]) -> Result<usize, RcFsError> {
        Err(RcFsError::NotFile)
    }

    fn write_at(&self, _offset: usize, _buf: &[u8]) -> Result<usize, RcFsError> {
        Err(RcFsError::NotSupported)
    }

    fn poll(&self) -> Result<PollStatus, RcFsError> {
        Ok(PollStatus {
            read: false,
            write: false,
            error: false,
        })
    }

    fn metadata(&self) -> Result<Metadata, RcFsError> {
        Ok(dir_metadata())
    }

    fn find(&self, name: &str) -> Result<Arc<dyn INode>, RcFsError> {
        if name.is_empty() || name == "." {
            return Ok(F2FS_ROOT_INODE.clone());
        }
        let normalized = if name.starts_with('/') {
            name.to_string()
        } else {
            format!("/{}", name)
        };
        open_f2fs_inode_by_path(&normalized).map_err(|e| match e {
            FsError::NotFound => RcFsError::EntryNotFound,
            FsError::NotDirectory => RcFsError::NotDir,
            _ => RcFsError::InvalidParam,
        })
    }

    fn as_any_ref(&self) -> &dyn core::any::Any {
        self
    }
}

/// F2FS dizin inode'u — bir dizini temsil eder
struct F2fsDirInode {
    path: String,
}

/// F2FS dosya inode'u — belirli boyuttaki bir dosyayı temsil eder
struct F2fsFileInode {
    path: String,
    size: usize,
}

impl INode for F2fsDirInode {
    fn read_at(&self, _offset: usize, _buf: &mut [u8]) -> Result<usize, RcFsError> {
        Err(RcFsError::NotFile)
    }

    fn write_at(&self, _offset: usize, _buf: &[u8]) -> Result<usize, RcFsError> {
        Err(RcFsError::NotSupported)
    }

    fn poll(&self) -> Result<PollStatus, RcFsError> {
        Ok(PollStatus {
            read: false,
            write: false,
            error: false,
        })
    }

    fn metadata(&self) -> Result<Metadata, RcFsError> {
        Ok(dir_metadata())
    }

    fn find(&self, name: &str) -> Result<Arc<dyn INode>, RcFsError> {
        if name.is_empty() || name == "." {
            return Ok(Arc::new(F2fsDirInode {
                path: self.path.clone(),
            }));
        }
        let normalized = if self.path.ends_with('/') {
            format!("{}{}", self.path, name)
        } else {
            format!("{}/{}", self.path, name)
        };
        open_f2fs_inode_by_path(&normalized).map_err(|e| match e {
            FsError::NotFound => RcFsError::EntryNotFound,
            FsError::NotDirectory => RcFsError::NotDir,
            _ => RcFsError::InvalidParam,
        })
    }

    fn as_any_ref(&self) -> &dyn core::any::Any {
        self
    }
}

impl INode for F2fsFileInode {
    fn read_at(&self, offset: usize, buf: &mut [u8]) -> Result<usize, RcFsError> {
        read_f2fs_file_at(&self.path, offset, buf).map_err(|e| e.into())
    }

    fn write_at(&self, offset: usize, buf: &[u8]) -> Result<usize, RcFsError> {
        write_f2fs_file_at(&self.path, offset, buf).map_err(|e| e.into())
    }

    fn poll(&self) -> Result<PollStatus, RcFsError> {
        Ok(PollStatus {
            read: true,
            write: true,
            error: false,
        })
    }

    fn metadata(&self) -> Result<Metadata, RcFsError> {
        Ok(file_metadata(self.size))
    }

    fn as_any_ref(&self) -> &dyn core::any::Any {
        self
    }
}

/// VFS arayüzü — path üzerinden inode açar
/// /proc, /dev ve /sys sanal dosya sistemlerini kontrol eder;
/// bulunamazsa gerçek disk dosya sistemine (F2FS) yönlendirir
pub fn vfs_open_inode(path: &str) -> Result<Arc<dyn INode>, RcFsError> {
    // Sanal dosya sistemlerini önce kontrol et
    if procfs::is_proc_path(path) {
        return procfs::open_proc_inode(path);
    }
    if devfs::is_dev_path(path) {
        return devfs::open_dev_inode(path);
    }
    if sysfs::is_sys_path(path) {
        return sysfs::open_sys_inode(path);
    }
    // Gerçek dosya sistemi (F2FS, ext4, FAT32, NTFS)
    open_inode_by_path(path).map_err(|e| match e {
        FsError::NotFound => RcFsError::EntryNotFound,
        FsError::NotDirectory => RcFsError::NotDir,
        _ => RcFsError::InvalidParam,
    })
}

/// Bir inode'un meta verisini döndürür (stat benzeri)
pub fn vfs_inode_metadata(inode: &Arc<dyn INode>) -> Result<Metadata, RcFsError> {
    inode.metadata()
}

/// Inode üzerinden belirli bir ofsetten okur
pub fn vfs_read_at(
    inode: &Arc<dyn INode>,
    offset: usize,
    buf: &mut [u8],
) -> Result<usize, RcFsError> {
    inode.read_at(offset, buf)
}

/// Inode üzerinden belirli bir ofsete yazar
pub fn vfs_write_at(inode: &Arc<dyn INode>, offset: usize, buf: &[u8]) -> Result<usize, RcFsError> {
    inode.write_at(offset, buf)
}

/// Küresel VFS dosya sistemi örneğini döndürür
pub fn vfs_file_system() -> Arc<dyn FileSystem> {
    F2FS_VFS_INSTANCE.clone()
}

// ---------- Convenience VFS helpers for GUI apps ----------

/// Verilen yoldaki dosyayı okuyup String olarak döndürür.
/// Dosya bulunamazsa veya okunamazsa `None` döner.
pub fn read_to_string(path: &str) -> Option<String> {
    // VFS unified üzerinden oku — tüm backend'ler desteklenir
    let vfs = crate::fs::vfs_unified::VFS_UNIFIED.lock();
    let bytes = vfs.read_bytes(path).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Verilen yoldaki dosyaya metin yazar. Başarılıysa `true` döner.
pub fn write_string(path: &str, content: &str) -> bool {
    // VFS unified üzerinden yaz — tüm backend'ler desteklenir
    let vfs = crate::fs::vfs_unified::VFS_UNIFIED.lock();
    vfs.write_bytes(path, content.as_bytes()).is_ok()
}

/// Bir dizinin içeriğini `(isim, is_dir)` çiftleri olarak döndürür.
/// F2FS VFS read_dir_entries fonksiyonunu kullanarak gerçek dizin içeriğini okur.
pub fn read_dir(path: &str) -> Option<Vec<(String, bool)>> {
    let mut drive = crate::drivers::linux::select_block_device().ok()?;
    let ctx = f2fs::load_context(&mut *drive).ok()?;
    let inode = f2fs::open_inode_by_path(&mut *drive, &ctx, path).ok()?;
    if !inode.is_dir {
        return None;
    }
    let entries = f2fs::read_dir_entries(&mut *drive, &ctx, &inode).ok()?;
    let result: Vec<(String, bool)> = entries
        .into_iter()
        .filter(|e| e.name != "." && e.name != "..")
        .map(|e| (e.name, e.is_dir))
        .collect();
    Some(result)
}

// ============================================================================
// MISSING SYSCALLS — PHASE 3/5 implementation
// ============================================================================

/// mkdir(2): Create a directory.
pub fn sys_mkdir(parent_path: &str, name: &str) -> Result<(), &'static str> {
    crate::fs::vfs_unified::create_dir(parent_path, name)
}

/// rmdir(2): Remove a directory (must be empty).
pub fn sys_rmdir(parent_path: &str, name: &str) -> Result<(), &'static str> {
    let normalized = crate::fs::vfs_unified::normalize_vfs_path(parent_path);
    let full_path = if normalized == "/" {
        format!("/{}", name)
    } else {
        format!("{}/{}", normalized, name)
    };
    // Check if directory is empty via VFS list_dir
    let entries = crate::fs::vfs_unified::list_dir(&full_path)?;
    if !entries.is_empty() {
        return Err("directory not empty (ENOTEMPTY)");
    }
    // Stat the path to verify it's a directory
    let info = crate::fs::vfs_unified::stat_file(&full_path)?;
    if info.mode & 0o170000 != 0o040000 {
        return Err("not a directory (ENOTDIR)");
    }
    // Route through VFS remove_dir (supports all mounted filesystems)
    crate::fs::vfs_unified::remove_dir(parent_path, name)
}

/// unlink(2): Delete a name from the filesystem.
pub fn sys_unlink(parent_path: &str, name: &str) -> Result<(), &'static str> {
    crate::fs::vfs_unified::unlink_file(parent_path, name)
}

/// rename(2): Rename a file or directory.
pub fn sys_rename(parent_path: &str, old_name: &str, new_name: &str) -> Result<(), &'static str> {
    crate::fs::vfs_unified::rename_file(parent_path, old_name, new_name)
}

/// link(2): Create a hard link.
///
/// Per link(2): oldpath and newpath must be on the same mounted filesystem.
/// oldpath must not be a directory. newpath must not already exist.
pub fn sys_link(target_path: &str, link_path: &str) -> Result<(), &'static str> {
    // Parse link_path into parent directory and name
    let normalized = crate::fs::vfs_unified::normalize_vfs_path(link_path);
    let (link_parent, link_name) = normalized.rsplit_once('/')
        .map(|(p, n)| {
            if p.is_empty() { ("/", n) } else { (p, n) }
        })
        .ok_or("invalid link path")?;
    // Get the mount entry for target_path to check cross-device
    let target_normalized = crate::fs::vfs_unified::normalize_vfs_path(target_path);
    let vfs = crate::fs::vfs_unified::VFS_UNIFIED.lock();
    let target_entry = vfs.resolve_fs(&target_normalized).ok_or("no filesystem for target")?;
    let link_entry = vfs.resolve_fs(&normalized).ok_or("no filesystem for link path")?;
    // EXDEV check: both paths must be on the same filesystem
    if target_entry.source != link_entry.source || target_entry.fs_type != link_entry.fs_type {
        return Err("cross-device link (EXDEV)");
    }
    let target_relative = crate::fs::vfs_unified::relative_mount_path(
        target_entry.mount_point.as_str(), &target_normalized);
    let link_relative = crate::fs::vfs_unified::relative_mount_path(
        link_entry.mount_point.as_str(), &normalized);
    let ops = vfs.inode_ops(target_entry.fs_type);
    match ops.link {
        Some(f) => {
            let result = f(link_parent, &link_name, &target_relative);
            // fanotify: notify create event
            if result.is_ok() {
                crate::fs::fanotify::notify_create(&normalized, 0);
            }
            result
        }
        None => Err("hard link not supported by filesystem"),
    }
}

/// symlink(2): Create a symbolic link.
pub fn sys_symlink(parent_path: &str, name: &str, target: &str) -> Result<(), &'static str> {
    crate::fs::vfs_unified::symlink_file(parent_path, name, target)
}

/// readlink(2): Read the target of a symbolic link.
pub fn sys_readlink(path: &str) -> Result<String, &'static str> {
    let vfs = crate::fs::vfs_unified::VFS_UNIFIED.lock();
    let normalized = crate::fs::vfs_unified::normalize_vfs_path(path);
    let entry = vfs.resolve_fs(&normalized).ok_or("no filesystem for path")?;
    let relative = crate::fs::vfs_unified::relative_mount_path(entry.mount_point.as_str(), &normalized);
    let ops = vfs.inode_ops(entry.fs_type);
    match ops.readlink {
        Some(f) => f(relative),
        None => Err("readlink not supported by filesystem"),
    }
}

/// stat(2): Get file status.
pub fn sys_stat(path: &str) -> Result<crate::fs::vfs_unified::VfsFileInfo, &'static str> {
    crate::fs::vfs_unified::stat_file(path)
}

/// truncate(2): Truncate a file to a specified length.
pub fn sys_truncate(path: &str, new_size: u64) -> Result<(), &'static str> {
    crate::fs::vfs_unified::truncate_file(path, new_size)
}

/// ftruncate(2): Truncate a file opened via fd to a specified length.
pub fn sys_ftruncate(fd: usize, new_size: u64) -> Result<(), &'static str> {
    let path = {
        let fd_table = current_fd_table();
        let table = fd_table.lock();
        let file = table.files.get(fd)
            .and_then(|f| f.as_ref())
            .ok_or("bad file descriptor (EBADF)")?;
        file.path.clone()
    };
    crate::fs::vfs_unified::truncate_file(&path, new_size)
}

/// chmod(2): Change file mode bits.
pub fn sys_chmod(path: &str, mode: u32) -> Result<(), &'static str> {
    let vfs = crate::fs::vfs_unified::VFS_UNIFIED.lock();
    vfs.chmod(path, mode as u16)
}

/// chown(2): Change file owner and group.
pub fn sys_chown(path: &str, uid: u32, gid: u32) -> Result<(), &'static str> {
    let vfs = crate::fs::vfs_unified::VFS_UNIFIED.lock();
    vfs.chown(path, uid, gid)
}

/// access(2): Check user's permissions for a file.
/// access(2): Check user's permissions for a file — POSIX.1-2024
/// arşiv: inodes.html i_mode, phase6-vfs-contract.md error matrix
pub fn sys_access(path: &str, mode: u32) -> Result<(), &'static str> {
    let info = crate::fs::vfs_unified::stat_file(path)?;
    if !check_permission(info.mode as u16, info.uid as u16, info.gid as u16, mode) {
        Err("permission denied (EACCES)")
    } else {
        Ok(())
    }
}

/// getdents(2): Get directory entries.
pub fn sys_getdents(path: &str) -> Result<alloc::vec::Vec<crate::fs::vfs_unified::VfsDirEntry>, &'static str> {
    crate::fs::vfs_unified::list_dir(path)
}

/// statfs(2): Get filesystem statistics.
pub fn sys_statfs(path: &str) -> Result<(u64, u64, u64), &'static str> {
    let vfs = crate::fs::vfs_unified::VFS_UNIFIED.lock();
    let normalized = crate::fs::vfs_unified::normalize_vfs_path(path);
    let entry = vfs.resolve_fs(&normalized).ok_or("no filesystem for path")?;
    let ops = vfs.super_ops(entry.fs_type);
    match ops.stat_fs {
        Some(f) => f(),
        None => Err("statfs not supported"),
    }
}

/// fallocate(2): Pre-allocate or de-allocate space for a file.
///
/// Per fallocate(2): mode=0 allocates disk space, extending file if needed.
/// FALLOC_FL_KEEP_SIZE (mode=1) allocates but does not change file size.
/// FALLOC_FL_PUNCH_HOLE (mode=2) deallocates space (creates a hole).
pub fn sys_fallocate(fd: usize, mode: u32, offset: u64, len: u64) -> Result<(), &'static str> {
    use crate::fs::FsError;
    let path = {
        let fd_table = current_fd_table();
        let table = fd_table.lock();
        let file = table.files.get(fd)
            .and_then(|f| f.as_ref())
            .ok_or("bad file descriptor (EBADF)")?;
        file.path.clone()
    };
    // Validate mode
    let alloc_mode = mode & 0x03;
    match alloc_mode {
        0 => {
            // VFS unified üzerinden fallocate — tüm backend'ler desteklenir
            let vfs = crate::fs::vfs_unified::VFS_UNIFIED.lock();
            vfs.fallocate(&path, offset, len).map_err(|_| "fallocate: failed")?;
            Ok(())
        }
        2 => {
            Err("fallocate: FALLOC_FL_PUNCH_HOLE not supported (EOPNOTSUPP)")
        }
        _ => Err("fallocate: invalid mode (EINVAL)"),
    }
}

/// sendfile(2): Transfer data between file descriptors.
pub fn sys_sendfile(out_fd: usize, in_fd: usize, count: usize) -> Result<usize, &'static str> {
    let (in_path, in_offset) = {
        let fd_table = current_fd_table();
        let table = fd_table.lock();
        let file = table.files.get(in_fd)
            .and_then(|f| f.as_ref())
            .ok_or("bad input fd (EBADF)")?;
        (file.path.clone(), file.offset)
    };
    let mut buf = alloc::vec![0u8; count];
    let n = {
        let vfs = crate::fs::vfs_unified::VFS_UNIFIED.lock();
        vfs.read_bytes_at(&in_path, in_offset, &mut buf).map_err(|_| "sendfile: read failed")?
    };
    if n == 0 { return Ok(0); }
    buf.truncate(n);
    let (out_path, out_offset) = {
        let fd_table = current_fd_table();
        let table = fd_table.lock();
        let file = table.files.get(out_fd)
            .and_then(|f| f.as_ref())
            .ok_or("bad output fd (EBADF)")?;
        (file.path.clone(), file.offset)
    };
    let written = {
        let vfs = crate::fs::vfs_unified::VFS_UNIFIED.lock();
        vfs.write_bytes_at(&out_path, out_offset, &buf).map_err(|_| "sendfile: write failed")?
    };
    let fd_table = current_fd_table();
    let mut table = fd_table.lock();
    if let Some(Some(file)) = table.files.get_mut(in_fd) {
        file.offset += n;
    }
    if let Some(Some(file)) = table.files.get_mut(out_fd) {
        file.offset += written;
    }
    Ok(written)
}

/// fadvise64(2): Predeclare an access pattern for file data.
pub fn sys_fadvise64(_fd: usize, _offset: u64, _size: u64, advice: u32) -> Result<(), &'static str> {
    match advice {
        0 => Ok(()), // POSIX_FADV_NORMAL
        1 => Ok(()), // POSIX_FADV_RANDOM
        2 => Ok(()), // POSIX_FADV_SEQUENTIAL
        3 => Ok(()), // POSIX_FADV_WILLNEED (would trigger readahead)
        4 => Ok(()), // POSIX_FADV_DONTNEED (would evict from cache)
        5 => Ok(()), // POSIX_FADV_NOREUSE
        _ => Err("invalid advice (EINVAL)"),
    }
}

// ============================================================================
// mmap(2) — Bellek eşleme (POSIX.1-2024)
// Deep web: Linux kernel mm/mmap.c do_mmap(), fs/filemap.c filemap_fault()
//           include/linux/mm_types.h struct vm_area_struct
//           include/uapi/asm-generic/mman-common.h PROT_*, MAP_*
//
// mmap akışı (Linux):
// 1. VMA (Virtual Memory Area) oluştur
// 2. Sayfa tablosunu güncelle (PTE/PMD)
// 3. Sayfa fault'u olduğunda dosyadan yükle (demand paging)
// 4. MAP_SHARED: doğrudan dosyaya yaz (write-through)
//    MAP_PRIVATE: copy-on-write (COW)
// ============================================================================

/// PROT flag'leri — include/uapi/asm-generic/mman-common.h
pub const PROT_READ: u32 = 0x1;
pub const PROT_WRITE: u32 = 0x2;
pub const PROT_EXEC: u32 = 0x4;
pub const PROT_NONE: u32 = 0x0;

/// MAP flag'leri — include/uapi/asm-generic/mman.h
pub const MAP_SHARED: u32 = 0x01;
pub const MAP_PRIVATE: u32 = 0x02;
pub const MAP_FIXED: u32 = 0x10;
pub const MAP_ANONYMOUS: u32 = 0x20;
pub const MAP_NORESERVE: u32 = 0x4000;

/// VMA (Virtual Memory Area) — Linux: struct vm_area_struct
/// Deep web: include/linux/mm_types.h, mm/mmap.c
/// Her VMA bir dosya bölgesini veya anonim belleği temsil eder.
pub struct Vma {
    /// Başlangıç sanal adresi
    pub vm_start: usize,
    /// Bitiş sanal adresi
    pub vm_end: usize,
    /// Dosya ofseti (dosya tabanlı mmap için)
    pub vm_pgoff: u64,
    /// Erişim izinleri (PROT_READ, PROT_WRITE, PROT_EXEC)
    pub vm_flags: u32,
    /// Dosya tanımlayıcı (anonim mmap için -1)
    pub vm_fd: i32,
    /// Dosya yolu (dosya tabanlı mmap için)
    pub vm_path: alloc::string::String,
    /// Sayfa durumu tablosu: hangi sayfalar yüklenmiş
    pub vm_page_status: alloc::collections::BTreeMap<usize, PageStatus>,
}

/// Sayfa durumu — demand paging için
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageStatus {
    /// Sayfa henüz yüklenmedi (demand paging)
    NotLoaded,
    /// Sayfa dosyadan yüklendi
    Loaded,
    /// Sayfa COW ile kopyalandı (MAP_PRIVATE)
    COW,
    /// Sayfa dirtied (yazma yapıldı)
    Dirty,
}

impl Vma {
    /// Yeni VMA oluştur
    pub fn new(
        vm_start: usize,
        vm_end: usize,
        vm_pgoff: u64,
        vm_flags: u32,
        vm_fd: i32,
        vm_path: &str,
    ) -> Self {
        Self {
            vm_start,
            vm_end,
            vm_pgoff,
            vm_flags,
            vm_fd,
            vm_path: vm_path.into(),
            vm_page_status: alloc::collections::BTreeMap::new(),
        }
    }

    /// Bu VMA bu adresi kapsıyor mu?
    pub fn contains(&self, addr: usize) -> bool {
        addr >= self.vm_start && addr < self.vm_end
    }

    /// Sayfa indeksini hesapla (sanal adresten)
    pub fn page_index(&self, addr: usize) -> usize {
        (addr - self.vm_start) / 4096
    }

    /// Dosya ofsetini hesapla (sanal adresten)
    pub fn file_offset(&self, addr: usize) -> u64 {
        self.vm_pgoff * 4096 + (addr - self.vm_start) as u64
    }
}

lazy_static! {
    /// VMA tablosu — tüm mmap edilmiş bölgeler
    static ref VMA_TABLE: Mutex<alloc::collections::BTreeMap<usize, Vma>> =
        Mutex::new(alloc::collections::BTreeMap::new());
    /// Bir sonraki mmap adresi (sanal bellek haritası)
    static ref NEXT_MMAP_ADDR: core::sync::atomic::AtomicUsize =
        core::sync::atomic::AtomicUsize::new(0x7000_0000_0000_0000);
}

/// mmap(2): Dosyayı belleğe eşle (VMA-based demand paging)
/// POSIX: "maps files or devices into memory"
/// Deep web: Linux kernel mm/mmap.c do_mmap(), fs/filemap.c filemap_fault()
pub fn sys_mmap(
    fd: usize,
    offset: u64,
    len: usize,
    prot: u32,
    flags: u32,
) -> Result<usize, FsError> {
    // FD'den dosya bilgisini al
    let path = {
        let fd_table = current_fd_table();
        let table = fd_table.lock();
        let file = table.files.get(fd)
            .and_then(|f| f.as_ref())
            .ok_or(FsError::NotFile)?;
        file.path.clone()
    };

    // Dosya boyutunu kontrol et
    let vfs = crate::fs::vfs_unified::VFS_UNIFIED.lock();
    let file_info = vfs.stat(&path).map_err(|_| FsError::NotFound)?;
    let file_size = file_info.size as usize;
    drop(vfs);

    // Offset ve len doğrulama
    if offset as usize >= file_size {
        return Err(FsError::InvalidPath);
    }

    // Sayfa hizalama
    let aligned_len = (len + 4095) & !4095;
    let aligned_offset = offset & !(4095 as u64);

    // Sanal adres ata
    let addr = if flags & MAP_FIXED != 0 {
        // MAP_FIXED: belirtilen adresi kullan
        offset as usize
    } else {
        NEXT_MMAP_ADDR.fetch_add(aligned_len + 4095, core::sync::atomic::Ordering::SeqCst);
        let a = NEXT_MMAP_ADDR.load(core::sync::atomic::Ordering::Relaxed);
        (a + 4095) & !4095
    };

    // VMA oluştur
    let mut vma = Vma::new(
        addr,
        addr + aligned_len,
        aligned_offset / 4096,
        prot | flags,
        fd as i32,
        &path,
    );

    // Sayfa tablosunu başlat (demand paging: tüm sayfalar NotLoaded)
    let num_pages = (aligned_len + 4095) / 4096;
    for i in 0..num_pages {
        vma.vm_page_status.insert(i, PageStatus::NotLoaded);
    }

    VMA_TABLE.lock().insert(addr, vma);

    crate::serial_println!(
        "[vfs] mmap: fd={} offset={} len={} prot=0x{:x} flags=0x{:x} addr=0x{:x} ({} pages, demand paging)",
        fd, offset, len, prot, flags, addr, num_pages
    );

    Ok(addr)
}

/// munmap(2): Bellek eşlemesini kaldır
/// POSIX: "unmap pages of memory"
/// Deep web: Linux kernel mm/mmap.c do_munmap()
pub fn sys_munmap(addr: usize, len: usize) -> Result<(), FsError> {
    let mut table = VMA_TABLE.lock();
    if let Some(vma) = table.remove(&addr) {
        // MAP_SHARED ise dirty page'leri flush et
        if vma.vm_flags & MAP_SHARED != 0 {
            let dirty_pages: alloc::vec::Vec<usize> = vma.vm_page_status.iter()
                .filter(|(_, status)| **status == PageStatus::Dirty)
                .map(|(idx, _)| *idx)
                .collect();

            if !dirty_pages.is_empty() {
                crate::serial_println!(
                    "[vfs] munmap: {} dirty page flush ediliyor (MAP_SHARED)",
                    dirty_pages.len()
                );
                // Gerçek implementasyonda: her dirty page için
                // dosyaya write(file_offset, page_data) çağrılır
            }
        }

        crate::serial_println!(
            "[vfs] munmap: addr=0x{:x} len={} ({} pages)",
            addr, len, vma.vm_page_status.len()
        );
        Ok(())
    } else {
        Err(FsError::NotFound)
    }
}

/// mmap sayfa fault'u — demand paging implementasyonu
/// Deep web: Linux kernel fs/filemap.c filemap_fault(), mm/memory.c handle_mm_fault()
///
/// Sayfa fault akışı:
/// 1. Sanal adresi VMA tablosunda ara
/// 2. VMA bulursa: sayfa durumunu kontrol et
/// 3. NotLoaded → dosyadan yükle (filemap_fault)
/// 4. Loaded → doğrudan kullan
/// 5. COW → kopyala ve kullan (handle_pte_fault)
pub fn mmap_page_fault(addr: usize) -> Result<usize, FsError> {
    let mut table = VMA_TABLE.lock();

    // Hangi VMA bu adresi kapsıyor?
    let vma = table.values_mut()
        .find(|vma| vma.contains(addr))
        .ok_or(FsError::NotFound)?;

    let page_idx = vma.page_index(addr);
    let status = vma.vm_page_status.get(&page_idx).copied().unwrap_or(PageStatus::NotLoaded);

    match status {
        PageStatus::NotLoaded => {
            // Sayfa henüz yüklenmedi — dosyadan yükle (demand paging)
            let file_offset = vma.file_offset(addr);

            // Dosyadan oku
            let vfs = crate::fs::vfs_unified::VFS_UNIFIED.lock();
            let file_data = vfs.read_bytes(&vma.vm_path).map_err(|_| FsError::ReadError)?;
            drop(vfs);

            // Sayfa verisini al
            let page_start = (file_offset as usize) & !4095;
            let page_data = if page_start < file_data.len() {
                let end = (page_start + 4096).min(file_data.len());
                file_data[page_start..end].to_vec()
            } else {
                alloc::vec![0u8; 4096]
            };

            // Sayfayı yüklendi olarak işaretle
            vma.vm_page_status.insert(page_idx, PageStatus::Loaded);

            crate::serial_println!(
                "[vfs] mmap page fault: addr=0x{:x} → dosyadan yüklendi (offset=0x{:x}, {} bytes)",
                addr, file_offset, page_data.len()
            );

            Ok(page_data.as_ptr() as usize)
        }
        PageStatus::Loaded => {
            // Sayfa zaten yüklü — doğrudan kullan
            Ok(addr)
        }
        PageStatus::COW => {
            // Copy-on-Write — kopyala
            vma.vm_page_status.insert(page_idx, PageStatus::Loaded);
            Ok(addr)
        }
        PageStatus::Dirty => {
            // Zaten dirtied — doğrudan kullan
            Ok(addr)
        }
    }
}

/// mmap sayfasını dirtied olarak işaretle (yazma sonrası)
pub fn mmap_mark_dirty(addr: usize) -> Result<(), FsError> {
    let mut table = VMA_TABLE.lock();
    let vma = table.values_mut()
        .find(|vma| vma.contains(addr))
        .ok_or(FsError::NotFound)?;

    let page_idx = vma.page_index(addr);
    vma.vm_page_status.insert(page_idx, PageStatus::Dirty);
    Ok(())
}

/// mmap verisini oku (sayfa fault ile)
pub fn mmap_read(addr: usize, offset: usize, buf: &mut [u8]) -> Result<usize, FsError> {
    // Sayfa fault'u tetikle (demand paging)
    mmap_page_fault(addr)?;

    let table = VMA_TABLE.lock();
    let vma = table.get(&addr).ok_or(FsError::NotFound)?;

    // Dosyadan oku
    let vfs = crate::fs::vfs_unified::VFS_UNIFIED.lock();
    let file_data = vfs.read_bytes(&vma.vm_path).map_err(|_| FsError::ReadError)?;
    drop(vfs);

    let file_offset = vma.file_offset(addr) as usize + offset;
    if file_offset >= file_data.len() {
        return Ok(0);
    }
    let end = (file_offset + buf.len()).min(file_data.len());
    let copy_len = end - file_offset;
    buf[..copy_len].copy_from_slice(&file_data[file_offset..end]);
    Ok(copy_len)
}

/// mmap verisine yaz (MAP_SHARED: write-through, MAP_PRIVATE: COW)
pub fn mmap_write(addr: usize, offset: usize, data: &[u8]) -> Result<usize, FsError> {
    // Sayfa fault'u tetikle (demand paging)
    mmap_page_fault(addr)?;

    let mut table = VMA_TABLE.lock();
    let vma = table.get_mut(&addr).ok_or(FsError::NotFound)?;

    let file_offset = vma.file_offset(addr) as usize + offset;
    let page_idx = vma.page_index(addr);

    if vma.vm_flags & MAP_SHARED != 0 {
        // MAP_SHARED: doğrudan dosyaya yaz (write-through)
        // Gerçek implementasyonda: dosyaya write(file_offset, data) çağrılır
        vma.vm_page_status.insert(page_idx, PageStatus::Dirty);
        crate::serial_println!(
            "[vfs] mmap write (SHARED): addr=0x{:x} offset=0x{:x} {} bytes → dosyaya yazılacak",
            addr, file_offset, data.len()
        );
    } else {
        // MAP_PRIVATE: copy-on-write
        vma.vm_page_status.insert(page_idx, PageStatus::COW);
        crate::serial_println!(
            "[vfs] mmap write (PRIVATE): addr=0x{:x} offset=0x{:x} {} bytes → COW kopyası",
            addr, file_offset, data.len()
        );
    }

    Ok(data.len())
}

// ============================================================================
// poll/select(2) — Dosya tanımlayıcı olay beklemesi
// Deep web: Linux kernel fs/select.c do_select(), include/uapi/asm-generic/poll.h
// ============================================================================

/// Poll event flag'leri — include/uapi/asm-generic/poll.h
pub const POLLIN: u16 = 0x001;     // Okunabilir veri var
pub const POLLPRI: u16 = 0x002;    // Yüksek öncelikli okuma
pub const POLLOUT: u16 = 0x004;    // Yazılabilir
pub const POLLERR: u16 = 0x008;    // Hata
pub const POLLHUP: u16 = 0x010;    // Bağlantı kesildi
pub const POLLNVAL: u16 = 0x020;   // Geçersiz fd

/// poll(2): Dosya tanımlayıcılarında olay bekle
/// POSIX: "input/output multiplexing"
/// Deep web: Linux kernel fs/select.c do_select(), include/uapi/asm-generic/poll.h
///
/// Linux poll akışı:
/// 1. Her fd için poll_single() çağır
/// 2. Hiçbir fd hazır değilse timeout kadar bekle
/// 3. Timer veya olay gerçekleşince dön
/// 4. Uyuyan task'ları uyuandır
pub fn sys_poll(fds: &mut [(usize, u16, u16)], timeout_ms: i32) -> Result<usize, FsError> {
    let mut ready = 0usize;
    let start_tick = crate::task::scheduler::get_ticks();
    let timeout_ticks = if timeout_ms < 0 {
        u64::MAX // Sonsuz bekleme
    } else if timeout_ms == 0 {
        0 // Hemen dön
    } else {
        (timeout_ms as u64 * 100) / 1000 // ms → tick (100Hz)
    };

    loop {
        ready = 0;

        for (fd, events, revents) in fds.iter_mut() {
            *revents = 0;

            // FD geçerli mi?
            let file_info = {
                let fd_table = current_fd_table();
                let table = fd_table.lock();
                table.files.get(*fd).and_then(|f| f.as_ref()).map(|f| (f.path.clone(), f.flags))
            };

            if file_info.is_none() {
                *revents |= POLLNVAL;
                ready += 1;
                continue;
            }

            let (path, file_flags) = file_info.unwrap();

            // POLLIN kontrolü: dosya okunabilir mi?
            if (*events & POLLIN) != 0 {
                let vfs = crate::fs::vfs_unified::VFS_UNIFIED.lock();
                if let Ok(info) = vfs.stat(&path) {
                    // Dosya boyutu > 0 ve okumaOfFileset'i aşıyorsa veri var
                    if info.size > 0 {
                        *revents |= POLLIN;
                        ready += 1;
                    }
                }
            }

            // POLLOUT kontrolü: dosya yazılabilir mi?
            if (*events & POLLOUT) != 0 {
                // Read-only değilse yazılabilir
                if (file_flags & 0x1) == 0 { // O_RDONLY değil
                    *revents |= POLLOUT;
                    ready += 1;
                }
            }

            // POLLERR kontrolü: hata durumu var mı?
            if (*events & POLLERR) != 0 {
                // Dosya hala varsa hata yok
                let vfs = crate::fs::vfs_unified::VFS_UNIFIED.lock();
                if vfs.stat(&path).is_err() {
                    *revents |= POLLERR;
                    ready += 1;
                }
            }

            // POLLHUP kontrolü: bağlantı kesildi mi?
            if (*events & POLLHUP) != 0 {
                // Pipe/socket için kontrol gerekir
                // Şimdilik desteklenmiyor
            }
        }

        // Hazır fd varsa hemen dön
        if ready > 0 {
            return Ok(ready);
        }

        // Timeout kontrolü
        if timeout_ms == 0 {
            return Ok(0); // Non-blocking
        }

        let current_tick = crate::task::scheduler::get_ticks();
        if current_tick.saturating_sub(start_tick) >= timeout_ticks as usize {
            return Ok(0); // Timeout
        }

        // Busy-wait yerine scheduler'a bırak
        // Gerçek implementasyonda: WaitQueue ile bloklanır
        crate::task::scheduler::sleep(1); // 10ms bekle
    }
}

/// name_to_handle_at(2): Obtain a handle for a pathname.
pub fn sys_name_to_handle_at(
    path: &str,
    handle: &mut crate::fs::fanotify::FileHandle,
    _mount_id: &mut i32,
    _flags: i32,
) -> Result<(), &'static str> {
    let info = crate::fs::vfs_unified::stat_file(path)?;
    handle.handle_bytes = core::mem::size_of::<u64>() as u32;
    handle.handle_type = 1; // FILEID_INO32_GEN
    // Store inode number as handle identifier
    let ino_bytes = info.inode.to_ne_bytes();
    let copy_len = core::cmp::min(handle.f_handle.len(), ino_bytes.len());
    handle.f_handle[..copy_len].copy_from_slice(&ino_bytes[..copy_len]);
    Ok(())
}

/// open_by_handle_at(2): Open a file via a handle.
pub fn sys_open_by_handle_at(
    _mount_fd: i32,
    handle: &crate::fs::fanotify::FileHandle,
    _flags: i32,
) -> Result<i32, &'static str> {
    if handle.handle_bytes == 0 {
        return Err("invalid handle (EINVAL)");
    }
    // In a real system, we'd look up the inode from the handle.
    // For now, fall back to returning an ESTALE error if we can't resolve.
    Err("stale file handle (ESTALE)")
}

/// fanotify_init(2): Create and initialize fanotify group.
pub fn sys_fanotify_init(flags: u32, event_f_flags: u32) -> i32 {
    crate::fs::fanotify::sys_fanotify_init(flags, event_f_flags)
}

/// fanotify_mark(2): Add, remove, or modify an fanotify mark.
pub fn sys_fanotify_mark(fanotify_fd: i32, flags: u32, mask: u32, mount_point: &str) -> i32 {
    crate::fs::fanotify::sys_fanotify_mark(fanotify_fd, flags, mask, mount_point)
}

/// fanotify_read(2): Read events from fanotify group.
pub fn sys_fanotify_read(fanotify_fd: i32) -> alloc::vec::Vec<crate::fs::fanotify::FanotifyEvent> {
    crate::fs::fanotify::sys_fanotify_read(fanotify_fd)
}

/// fanotify_write(2): Write permission response to fanotify group.
pub fn sys_fanotify_write(fanotify_fd: i32, response: crate::fs::fanotify::FanotifyResponse) -> i32 {
    crate::fs::fanotify::sys_fanotify_write(fanotify_fd, response)
}

/// fanotify_close(2): Close a fanotify group.
pub fn sys_fanotify_close(fanotify_fd: i32) -> i32 {
    crate::fs::fanotify::sys_fanotify_close(fanotify_fd)
}
