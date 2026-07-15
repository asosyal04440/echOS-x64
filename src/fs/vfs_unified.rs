//! # VFS Birleşik Katman — Unified Virtual File System Layer
//!
//! ext4, XFS, Btrfs, FAT32, NTFS gibi farklı dosya sistemlerini tek bir
//! arayüz altında birleştiren katman. Mount tablosuna göre path routing yapar.
//!
//! ## Mimari
//!
//! ```text
//! ┌─────────────────────────────────────────────────┐
//! │                  VFS Unified                     │
//! │      vfs_unified_open() / vfs_unified_stat()    │
//! ├──────┬──────┬───────┬───────┬───────┬───────────┤
//! │ ext4 │ XFS  │ Btrfs │ FAT32 │ NTFS │ F2FS     │
//! │      │      │       │       │      │ (default) │
//! ├──────┴──────┴───────┴───────┴───────┴───────────┤
//! │              Block Device Layer                  │
//! └─────────────────────────────────────────────────┘
//! ```

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::hash::{Hash, Hasher};
use spin::Mutex;

use crate::fs::page_cache;

/// Simple path-to-u64 hash for page cache indexing.
fn hash_path(path: &str) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for b in path.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

// ============================================================================
// VFS Filesystem Type Registry
// ============================================================================

/// Desteklenen dosya sistemi türleri
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VfsFsType {
    F2fs,
    Ext4,
    Xfs,
    Btrfs,
    Fat32,
    ExFat,
    Ntfs,
    ProcFs,
    DevFs,
    SysFs,
    TmpFs,
    Erofs,
    Squashfs,
}

impl VfsFsType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::F2fs => "f2fs",
            Self::Ext4 => "ext4",
            Self::Xfs => "xfs",
            Self::Btrfs => "btrfs",
            Self::Fat32 => "vfat",
            Self::ExFat => "exfat",
            Self::Ntfs => "ntfs",
            Self::ProcFs => "proc",
            Self::DevFs => "devtmpfs",
            Self::SysFs => "sysfs",
            Self::TmpFs => "tmpfs",
            Self::Erofs => "erofs",
            Self::Squashfs => "squashfs",
        }
    }

    /// String'den VfsFsType parse eder
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "f2fs" => Some(Self::F2fs),
            "ext4" => Some(Self::Ext4),
            "xfs" => Some(Self::Xfs),
            "btrfs" => Some(Self::Btrfs),
            "vfat" | "fat32" | "fat" => Some(Self::Fat32),
            "exfat" => Some(Self::ExFat),
            "ntfs" => Some(Self::Ntfs),
            "proc" | "procfs" => Some(Self::ProcFs),
            "devtmpfs" | "devfs" => Some(Self::DevFs),
            "sysfs" => Some(Self::SysFs),
            "tmpfs" => Some(Self::TmpFs),
            "erofs" => Some(Self::Erofs),
            "squashfs" => Some(Self::Squashfs),
            _ => None,
        }
    }
}

// ============================================================================
// Mount Entry
// ============================================================================

/// VFS mount tablosu girdisi
#[derive(Clone, Debug)]
pub struct VfsMountEntry {
    /// Mount noktası (örn. "/", "/mnt/data", "/proc")
    pub mount_point: String,
    /// Dosya sistemi türü
    pub fs_type: VfsFsType,
    /// Kaynak cihaz (örn. "/dev/nvme0n1p1", "none")
    pub source: String,
    /// Mount bayrakları
    pub flags: VfsMountFlags,
    /// Read-only mu?
    pub readonly: bool,
    /// Backend feature matrix (Gate 4: mount feature gate enforcement)
    pub feature_matrix: Option<crate::fs::BackendFeatureMatrix>,
}

/// Mount bayrakları
#[derive(Clone, Copy, Debug, Default)]
pub struct VfsMountFlags {
    /// noexec — binary çalıştırma yasağı
    pub noexec: bool,
    /// nosuid — setuid biti yoksay
    pub nosuid: bool,
    /// nodev — cihaz dosyaları yoksay
    pub nodev: bool,
    /// noatime — erişim zamanını güncelleme
    pub noatime: bool,
    /// relatime — sadece mtime > atime ise güncelle
    pub relatime: bool,
}

// ============================================================================
// VFS Unified File Info
// ============================================================================

/// Birleşik dosya bilgisi (stat-benzeri)
#[derive(Clone, Debug)]
pub struct VfsFileInfo {
    /// İnode numarası
    pub inode: u64,
    /// Dosya boyutu (bayt)
    pub size: u64,
    /// Mod bitleri (permissions + type)
    pub mode: u32,
    /// Hard link sayısı
    pub nlink: u32,
    /// Kullanıcı ID
    pub uid: u32,
    /// Grup ID
    pub gid: u32,
    /// Dosya sistemi türü
    pub fs_type: VfsFsType,
    /// Blok boyutu
    pub block_size: u32,
    /// Blok sayısı
    pub blocks: u64,
}

#[derive(Clone, Debug)]
pub struct VfsDirEntry {
    pub name: String,
    pub size: u64,
    pub is_directory: bool,
    pub fs_type: VfsFsType,
}

/// Dosya türü
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VfsFileType {
    Regular,
    Directory,
    Symlink,
    CharDevice,
    BlockDevice,
    Fifo,
    Socket,
}

// ============================================================================
// super_operations vtable — unified backend init/sync/statfs
// ============================================================================

/// VFS superblock operations — per-mount filesystem lifecycle.
pub struct VfsSuperOps {
    /// Synchronize all dirty data to stable storage.
    pub sync_fs: Option<fn() -> Result<(), &'static str>>,
    /// Return filesystem statistics (total/used/free blocks).
    pub stat_fs: Option<fn() -> Result<(u64, u64, u64), &'static str>>,
    /// Mount-time initialization (e.g. replay journal).
    pub mount_root: Option<fn(source: &str, readonly: bool) -> Result<(), &'static str>>,
}

impl VfsSuperOps {
    pub const fn empty() -> Self {
        Self { sync_fs: None, stat_fs: None, mount_root: None }
    }
}

// ============================================================================
// inode_operations vtable — unified inode manipulation
// ============================================================================

/// VFS inode operations — directory and file manipulation.
pub struct VfsInodeOps {
    /// Lookup a child entry by name in a directory.
    pub lookup: Option<fn(parent_ino: u64, parent_path: &str, name: &str) -> Result<VfsFileInfo, &'static str>>,
    /// Create a regular file.
    pub create: Option<fn(parent_path: &str, name: &str) -> Result<(), &'static str>>,
    /// Create a directory.
    pub mkdir: Option<fn(parent_path: &str, name: &str) -> Result<(), &'static str>>,
    /// Remove a directory (must be empty).
    pub rmdir: Option<fn(parent_path: &str, name: &str) -> Result<(), &'static str>>,
    /// Unlink (delete) a file.
    pub unlink: Option<fn(parent_path: &str, name: &str) -> Result<(), &'static str>>,
    /// Rename a file or directory within same parent.
    pub rename: Option<fn(parent_path: &str, old_name: &str, new_name: &str) -> Result<(), &'static str>>,
    /// Create a symbolic link.
    pub symlink: Option<fn(parent_path: &str, name: &str, target: &str) -> Result<(), &'static str>>,
    /// Read a symbolic link target.
    pub readlink: Option<fn(path: &str) -> Result<String, &'static str>>,
    /// Create a hard link.
    pub link: Option<fn(parent_path: &str, name: &str, target_path: &str) -> Result<(), &'static str>>,
    /// Truncate a file to a given size.
    pub truncate: Option<fn(path: &str, new_size: u64) -> Result<(), &'static str>>,
    /// Change file mode (permissions).
    pub chmod: Option<fn(path: &str, mode: u32) -> Result<(), &'static str>>,
    /// Change file owner.
    pub chown: Option<fn(path: &str, uid: u32, gid: u32) -> Result<(), &'static str>>,
    /// Get file status (stat).
    pub stat: Option<fn(path: &str) -> Result<VfsFileInfo, &'static str>>,
}

impl VfsInodeOps {
    pub const fn empty() -> Self {
        Self {
            lookup: None, create: None, mkdir: None, rmdir: None,
            unlink: None, rename: None, symlink: None, readlink: None,
            link: None, truncate: None, chmod: None, chown: None, stat: None,
        }
    }
}

// ============================================================================
// file_operations vtable — unified file descriptor operations
// ============================================================================

/// VFS file operations — per-open-file operations extending VfsFileInfo.
pub struct VfsFileOps {
    /// Read from a specific file at offset.
    pub read: Option<fn(path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, &'static str>>,
    /// Write to a specific file at offset.
    pub write: Option<fn(path: &str, offset: u64, buf: &[u8]) -> Result<usize, &'static str>>,
    /// Copy a range of data between two files (in-kernel copy).
    pub copy_file_range: Option<fn(src_path: &str, src_off: u64, dst_path: &str, dst_off: u64, size: u64) -> Result<u64, &'static str>>,
    /// Advise access pattern for a file region.
    pub fadvise: Option<fn(path: &str, offset: u64, size: u64, advice: u32) -> Result<(), &'static str>>,
}

impl VfsFileOps {
    pub const fn empty() -> Self {
        Self { read: None, write: None, copy_file_range: None, fadvise: None }
    }
}

// ============================================================================
// VFS Unified Manager
// ============================================================================

/// Birleşik VFS yöneticisi
pub struct VfsUnified {
    /// Mount tablosu (mount_point → entry)
    pub mount_table: BTreeMap<String, VfsMountEntry>,
    /// Toplam dosya sistemi sayısı
    fs_count: usize,
}

impl VfsUnified {
    pub fn new() -> Self {
        Self {
            mount_table: BTreeMap::new(),
            fs_count: 0,
        }
    }

    /// Mount ekler
    pub fn mount(
        &mut self,
        mount_point: &str,
        fs_type: VfsFsType,
        source: &str,
        flags: VfsMountFlags,
    ) -> Result<(), &'static str> {
        let default_matrix = Self::build_feature_matrix(fs_type, false);
        let readonly = (flags.noexec && flags.nosuid) || !default_matrix.write;
        self.mount_with_readonly(mount_point, fs_type, source, flags, readonly)
    }

    /// Mount ekler (readonly açık kontrolü + feature gate enforcement ile)
    ///
    /// Gate 4: Mount sırasında feature matrix validate edilir.
    /// Unknown incompatible feature flag varsa mount reddedilir.
    pub fn mount_with_readonly(
        &mut self,
        mount_point: &str,
        fs_type: VfsFsType,
        source: &str,
        flags: VfsMountFlags,
        readonly: bool,
    ) -> Result<(), &'static str> {
        // POSIX.1-2024 path_resolution(7) contract for mount point
        if let Err(e) = validate_path(mount_point) {
            return Err(fs_error_to_str(&e));
        }

        // Gate 4: Feature matrix oluştur ve mount-time validation yap
        let feature_matrix = Self::build_feature_matrix(fs_type, readonly);
        if let Err(e) = Self::enforce_mount_gates(&feature_matrix, readonly) {
            return Err(fs_error_to_str(&e));
        }

        let mount_point = normalize_vfs_path(mount_point);
        let entry = VfsMountEntry {
            mount_point: mount_point.clone(),
            fs_type,
            source: String::from(source),
            flags,
            readonly,
            feature_matrix: Some(feature_matrix),
        };
        self.mount_table.insert(mount_point, entry);
        self.fs_count += 1;
        Ok(())
    }

    /// Build the BackendFeatureMatrix for a given fs_type.
    /// Deep web: Linux kernel fs/xfs/xfs_mount.h, fs/ext4/super.c
    fn build_feature_matrix(fs_type: VfsFsType, readonly: bool) -> crate::fs::BackendFeatureMatrix {
        let mut matrix = match fs_type {
            VfsFsType::F2fs => crate::fs::f2fs_feature_matrix(),
            VfsFsType::Ext4 => crate::fs::ext4_feature_matrix(),
            VfsFsType::Xfs => crate::fs::xfs_feature_matrix(), // XFS artık ayrı feature matrix
            VfsFsType::Btrfs => crate::fs::btrfs_feature_matrix(),
            VfsFsType::Fat32 => crate::fs::fat32_feature_matrix(),
            VfsFsType::ExFat => crate::fs::exfat_feature_matrix(),
            VfsFsType::Ntfs => crate::fs::ntfs_feature_matrix(),
            VfsFsType::ProcFs => crate::fs::tmpfs_feature_matrix(),
            VfsFsType::DevFs => crate::fs::tmpfs_feature_matrix(),
            VfsFsType::SysFs => crate::fs::tmpfs_feature_matrix(),
            VfsFsType::TmpFs => crate::fs::tmpfs_feature_matrix(),
            VfsFsType::Erofs => crate::fs::erofs_feature_matrix(),
            VfsFsType::Squashfs => crate::fs::squashfs_feature_matrix(),
        };
        // Override readonly if mount flag forces it
        if readonly {
            matrix.readonly = true;
            matrix.write = false;
        }
        matrix
    }

    /// Gate 4: Enforce mount-time feature gates.
    ///
    /// Mount policy decision tree (per phase6-backend-feature-gates.md):
    /// - Unknown incompatible feature → refuse mount
    /// - Unknown ro-compatible + no MS_RDONLY → refuse mount (ReadOnlyFs needed)
    /// - Journal replay needed but not supported → NeedsRecovery
    fn enforce_mount_gates(
        matrix: &crate::fs::BackendFeatureMatrix,
        readonly: bool,
    ) -> Result<(), crate::fs::FsError> {
        // Write mount requires write-capable backend
        if !readonly && !matrix.write {
            return Err(crate::fs::FsError::ReadOnlyFs);
        }

        // Multi-device not supported — fail closed
        if matrix.multi_device {
            return Err(crate::fs::FsError::UnsupportedFeature(
                crate::fs::UnsupportedFeatureType::MultiDevice,
            ));
        }

        // Encryption without decryption support — fail closed
        if matrix.encryption {
            return Err(crate::fs::FsError::UnsupportedFeature(
                crate::fs::UnsupportedFeatureType::Encryption,
            ));
        }

        Ok(())
    }

    /// Mount flag'lerini uygula: nosuid ise SUID/SGID bitlerini temizle
    fn apply_mount_flags(&self, path: &str, info: VfsFileInfo) -> VfsFileInfo {
        let normalized = normalize_vfs_path(path);
        if let Some(entry) = self.resolve_fs(&normalized) {
            let mut info = info;
            if entry.flags.nosuid {
                // SUID (0o4000) ve SGID (0o2000) bitlerini temizle
                info.mode &= !(0o4000 | 0o2000);
            }
            info
        } else {
            info
        }
    }

    /// noexec kontrolü: mount noexec ise ve dosya executable ise reddet
    fn check_noexec(&self, path: &str, mode: u32) -> Result<(), &'static str> {
        let normalized = normalize_vfs_path(path);
        if let Some(entry) = self.resolve_fs(&normalized) {
            if entry.flags.noexec {
                // Execute bitleri kontrol et (owner/group/other execute)
                if mode & 0o111 != 0 {
                    return Err("operation not permitted: mount has noexec flag");
                }
            }
        }
        Ok(())
    }

    /// Umount
    pub fn umount(&mut self, mount_point: &str) -> Result<(), &'static str> {
        let mount_point = normalize_vfs_path(mount_point);
        if self.mount_table.remove(mount_point.as_str()).is_some() {
            self.fs_count -= 1;
            crate::serial_println!("[VFS] umount: {}", mount_point);
            Ok(())
        } else {
            Err("Mount point not found")
        }
    }

    /// Path'e göre hangi dosya sisteminin sorumlu olduğunu bulur
    ///
    /// Mount boundary crossing (§1.3):
    /// - If the normalized path is a mount point root, resolve to the mount point's
    ///   own filesystem (not the parent). Actual boundary crossing only happens
    ///   when path traversal (e.g., `..`) explicitly leaves a mount.
    /// - `follow_up()` handles the case where a path component crosses from
    ///   a mount point up to its parent mount.
    pub fn resolve_fs(&self, path: &str) -> Option<&VfsMountEntry> {
        let path = normalize_vfs_path(path);
        // En uzun eşleşen mount point'i bul (longest prefix match)
        let mut best_match: Option<&VfsMountEntry> = None;
        let mut best_len = 0;

        for (mp, entry) in &self.mount_table {
            if mount_matches_path(mp.as_str(), path.as_str()) && mp.len() > best_len {
                best_match = Some(entry);
                best_len = mp.len();
            }
        }

        best_match
    }

    /// Follow up from a mount point to its parent mount.
    ///
    /// If `path` exactly matches a mount point's target (and is not root `/`),
    /// return the path resolved in the parent mount. This implements the
    /// mount boundary crossing contract (§1.3): when `..` traverses out of a
    /// mounted filesystem's root, we must land in the parent mount, not in the
    /// mounted filesystem's internal parent.
    ///
    /// Returns `Some(parent_mount_path)` if boundary crossing occurred,
    /// `None` if the path is not a mount point root.
    pub fn follow_up(&self, path: &str) -> Option<String> {
        let normalized = normalize_vfs_path(path);
        if normalized == "/" {
            return None; // root mount: cannot go up
        }

        // Check if the normalized path is a mount target
        if self.mount_table.contains_key(&normalized) {
            // The parent of a mount point is the directory containing it
            let parent_path = normalized
                .trim_end_matches('/')
                .rsplit_once('/')
                .map(|(parent, _)| {
                    if parent.is_empty() { "/" } else { parent }
                })
                .unwrap_or("/");

            // Resolve to the mount that covers the parent path
            let resolved = self.resolve_fs(parent_path)?;
            Some(resolved.mount_point.clone())
        } else {
            None
        }
    }

    /// Birleşik open — path'e göre doğru dosya sistemine yönlendirir
    /// Backend dispatch without VFS-level symlink following.
    ///
    /// Returns `VfsFileInfo` from the correct backend for the given path.
    /// Used internally and by `dispatch_open` from `lookup_component`.
    pub(crate) fn open_direct(&self, path: &str) -> Result<VfsFileInfo, &'static str> {
        let normalized_path = normalize_vfs_path(path);
        let entry = self
            .resolve_fs(normalized_path.as_str())
            .ok_or_else(no_filesystem_for_path)?;

        let relative_path =
            relative_mount_path(entry.mount_point.as_str(), normalized_path.as_str());

        self.dispatch_open(entry.fs_type, &entry.source, &relative_path)
    }

    /// Per-component directory lookup.
    ///
    /// Looks up a single path component in the directory identified by
    /// `parent_path` / `parent_ino` on the appropriate backend.  Returns
    /// the child's `VfsFileInfo`.
    ///
    /// For f2fs this uses the per-inode `lookup_child` to avoid O(n²)
    /// re-walking from the root.  For other backends it constructs the
    /// full child path and dispatches through `open_direct`.
    pub fn lookup_component(
        &self,
        parent_ino: u64,
        parent_path: &str,
        name: &str,
    ) -> Result<VfsFileInfo, &'static str> {
        let child_path = if parent_path == "/" || parent_path.is_empty() {
            alloc::format!("/{}", name)
        } else {
            alloc::format!("{}/{}", parent_path, name)
        };
        let entry = self
            .resolve_fs(&child_path)
            .ok_or_else(no_filesystem_for_path)?;

        match entry.fs_type {
            VfsFsType::F2fs => {
                // Per-inode lookup avoids full-path re-walk from root.
                let child = crate::fs::f2fs::lookup_child(parent_ino, name).or_else(|_| {
                    // Fallback: full-path when parent_ino is stale
                    // (e.g. after a mount-boundary cross).
                    let normalized = normalize_vfs_path(&child_path);
                    let rel = relative_mount_path(entry.mount_point.as_str(), &normalized);
                    crate::fs::f2fs::open_entry(&rel).map_err(|_| "f2fs: component not found")
                })?;
                Ok(vfs_info_from_f2fs_entry(&child))
            }
            _ => {
                let normalized = normalize_vfs_path(&child_path);
                let rel = relative_mount_path(entry.mount_point.as_str(), &normalized);
                self.dispatch_open(entry.fs_type, &entry.source, &rel)
            }
        }
    }

    /// Open a file on a specific backend (used by `lookup_component` fallback
    /// and by `open_direct`).
    fn dispatch_open(
        &self,
        fs_type: VfsFsType,
        _source: &str,
        relative_path: &str,
    ) -> Result<VfsFileInfo, &'static str> {
        match fs_type {
            VfsFsType::Ext4 => {
                let resolved = resolve_ext4_node(_source, relative_path)
                    .map_err(|_| "ext4: component not found")?;
                // Linux: inode_lock_shared(inode) — shared lock for stat/open reads
                crate::fs::ext4::ext4_inode_lock_shared(_source, resolved.inode_num);
                let info = vfs_info_from_ext4_inode(&resolved);
                crate::fs::ext4::ext4_inode_unlock_shared(_source, resolved.inode_num);
                Ok(info)
            }
            VfsFsType::Fat32 => {
                let resolved = resolve_fat32_node(_source, relative_path)
                    .map_err(|_| "fat32: component not found")?;
                Ok(vfs_info_from_fat32_file(&resolved))
            }
            VfsFsType::ExFat => {
                let resolved = resolve_exfat_node(_source, relative_path)
                    .map_err(|_| "exfat: component not found")?;
                Ok(vfs_info_from_exfat_file(&resolved))
            }
            VfsFsType::Ntfs => {
                let resolved = resolve_ntfs_node(_source, relative_path)
                    .map_err(|_| "ntfs: component not found")?;
                Ok(vfs_info_from_ntfs_entry(&resolved))
            }
            VfsFsType::Btrfs => {
                let resolved = resolve_btrfs_node(_source, relative_path)
                    .map_err(|_| "btrfs: component not found")?;
                Ok(vfs_info_from_btrfs_inode(&resolved))
            }
            VfsFsType::F2fs => {
                let f2fs_entry = crate::fs::f2fs::open_entry(relative_path)
                    .map_err(|_| "f2fs: component not found")?;
                Ok(vfs_info_from_f2fs_entry(&f2fs_entry))
            }
            VfsFsType::ProcFs => {
                if is_mount_root(relative_path) {
                    return Ok(directory_info(VfsFsType::ProcFs));
                }
                let content = generate_proc_content(relative_path);
                if content.is_empty() {
                    return Err("procfs: entry not found");
                }
                Ok(VfsFileInfo {
                    inode: 0,
                    size: content.len() as u64,
                    mode: 0o100444,
                    nlink: 1,
                    uid: 0,
                    gid: 0,
                    fs_type: VfsFsType::ProcFs,
                    block_size: 4096,
                    blocks: 0,
                })
            }
            VfsFsType::SysFs => {
                if is_mount_root(relative_path) {
                    return Ok(directory_info(VfsFsType::SysFs));
                }
                let content = read_sysfs_bytes(relative_path)?;
                Ok(VfsFileInfo {
                    inode: 0,
                    size: content.len() as u64,
                    mode: 0o100444,
                    nlink: 1,
                    uid: 0,
                    gid: 0,
                    fs_type: VfsFsType::SysFs,
                    block_size: 4096,
                    blocks: ((content.len() as u64) + 4095) / 4096,
                })
            }
            VfsFsType::DevFs | VfsFsType::TmpFs => {
                if is_mount_root(relative_path) {
                    Ok(directory_info(fs_type))
                } else {
                    Err(unsupported_vfs_capability(fs_type, VfsUnsupportedCapability::Open))
                }
            }
            VfsFsType::Xfs => Err(unsupported_vfs_capability(
                fs_type,
                VfsUnsupportedCapability::Open,
            )),
            VfsFsType::Erofs => {
                if is_mount_root(relative_path) {
                    return Ok(directory_info(VfsFsType::Erofs));
                }
                Err("erofs: VFS open requires full path resolution")
            }
            VfsFsType::Squashfs => {
                if is_mount_root(relative_path) {
                    return Ok(directory_info(VfsFsType::Squashfs));
                }
                Err("squashfs: VFS open requires full path resolution")
            }
        }
    }

    /// VFS open with symlink following, mount flag enforcement, and fanotify.
    ///
    /// Uses `namei::resolve` to follow symlinks at the VFS level (including
    /// cross-filesystem symlinks), then enforces mount flags and notifies
    /// fanotify watchers.
    pub fn open(&self, path: &str) -> Result<VfsFileInfo, &'static str> {
        // POSIX.1-2024 path_resolution(7) contract
        if let Err(e) = validate_path(path) {
            return Err(fs_error_to_str(&e));
        }
        let normalized_path = normalize_vfs_path(path);

        // Resolve path with symlink following + dcache caching
        let resolved = crate::fs::namei::resolve(
            |parent_ino, parent_path, name| {
                self.lookup_component(parent_ino, parent_path, name)
            },
            |p| self.read_bytes_direct(p),
            &normalized_path,
            true,
        )?;

        // Mount flag enforcement (resolve the mount for the final resolved path)
        let entry = self
            .resolve_fs(resolved.resolved_path.as_str())
            .ok_or_else(no_filesystem_for_path)?;
        if entry.flags.nodev && (resolved.info.mode & 0x6000) != 0 {
            return Err("operation not permitted: mount has nodev flag");
        }
        if entry.flags.noexec && (resolved.info.mode & 0o111) != 0 {
            return Err("operation not permitted: mount has noexec flag");
        }

        // fanotify: notify open event
        crate::fs::fanotify::notify_open(&resolved.resolved_path, 0);
        Ok(self.apply_mount_flags(path, resolved.info))
    }

    /// Mount tablosunu listeler (mount komutu çıktısı)
    pub fn list_mounts(&self) -> Vec<String> {
        self.mount_table
            .values()
            .map(|e| {
                let mut opts = Vec::new();
                if e.readonly {
                    opts.push("ro");
                } else {
                    opts.push("rw");
                }
                if e.flags.noexec {
                    opts.push("noexec");
                }
                if e.flags.nosuid {
                    opts.push("nosuid");
                }
                if e.flags.nodev {
                    opts.push("nodev");
                }
                if e.flags.noatime {
                    opts.push("noatime");
                }
                if e.flags.relatime {
                    opts.push("relatime");
                }
                format!(
                    "{} on {} type {} ({})",
                    e.source,
                    e.mount_point,
                    e.fs_type.as_str(),
                    opts.join(",")
                )
            })
            .collect()
    }

    /// Mount edilmiş dosya sistemi sayısı
    pub fn mount_count(&self) -> usize {
        self.fs_count
    }

    /// Toplam / kullanılan / boş alan (tüm mount noktaları)
    pub fn df_summary(&self) -> Vec<(String, VfsFsType, u64, u64, u64)> {
        // (mount_point, fs_type, total, used, free)
        let mut result = Vec::new();
        for (mp, entry) in &self.mount_table {
            match entry.fs_type {
                VfsFsType::ProcFs | VfsFsType::DevFs | VfsFsType::SysFs | VfsFsType::TmpFs => {
                    // Sanal dosya sistemlerinin boyutu yok
                    result.push((mp.clone(), entry.fs_type, 0, 0, 0));
                }
                VfsFsType::F2fs => {
                    if let Ok(stats) = crate::fs::f2fs::f2fs_stats() {
                        const F2FS_BLOCK_BYTES: u64 = 4096;
                        let total = stats.total_main_blocks.saturating_mul(F2FS_BLOCK_BYTES);
                        let used = stats.used_blocks.saturating_mul(F2FS_BLOCK_BYTES);
                        let free = stats.free_blocks.saturating_mul(F2FS_BLOCK_BYTES);
                        result.push((mp.clone(), entry.fs_type, total, used, free));
                    } else {
                        result.push((mp.clone(), entry.fs_type, 0, 0, 0));
                    }
                }
                VfsFsType::Ext4 => {
                    if let Some(mounted) = crate::fs::ext4::get_mounted_ext4(&entry.source) {
                        let total =
                            mounted.fs.superblock.total_blocks() * mounted.fs.block_size as u64;
                        let free =
                            mounted.fs.superblock.free_blocks() * mounted.fs.block_size as u64;
                        let used = total.saturating_sub(free);
                        result.push((mp.clone(), entry.fs_type, total, used, free));
                    } else {
                        result.push((mp.clone(), entry.fs_type, 0, 0, 0));
                    }
                }
                VfsFsType::Fat32 => {
                    if let Ok((total, used, free)) = fat32_capacity(&entry.source) {
                        result.push((mp.clone(), entry.fs_type, total, used, free));
                    } else {
                        result.push((mp.clone(), entry.fs_type, 0, 0, 0));
                    }
                }
                VfsFsType::ExFat => {
                    if let Ok((total, used, free)) = exfat_capacity(&entry.source) {
                        result.push((mp.clone(), entry.fs_type, total, used, free));
                    } else {
                        result.push((mp.clone(), entry.fs_type, 0, 0, 0));
                    }
                }
                VfsFsType::Ntfs => {
                    if let Ok((total, used, free)) = ntfs_capacity(&entry.source) {
                        result.push((mp.clone(), entry.fs_type, total, used, free));
                    } else {
                        result.push((mp.clone(), entry.fs_type, 0, 0, 0));
                    }
                }
                VfsFsType::Btrfs => {
                    if let Ok((total, used, free)) = btrfs_capacity(&entry.source) {
                        result.push((mp.clone(), entry.fs_type, total, used, free));
                    } else {
                        result.push((mp.clone(), entry.fs_type, 0, 0, 0));
                    }
                }
                _ => {
                    // Unsupported backends must not claim heuristic capacity.
                    result.push((mp.clone(), entry.fs_type, 0, 0, 0));
                }
            }
        }
        result
    }

    /// Backend read dispatch without page cache, without symlink following.
    ///
    /// Used by `namei::resolve` as the `readlink` callback, and by
    /// `read_bytes` as the final I/O call on the resolved path.
    pub(crate) fn read_bytes_direct(&self, path: &str) -> Result<Vec<u8>, &'static str> {
        let normalized_path = normalize_vfs_path(path);
        let entry = self
            .resolve_fs(normalized_path.as_str())
            .ok_or_else(no_filesystem_for_path)?;
        let relative_path =
            relative_mount_path(entry.mount_point.as_str(), normalized_path.as_str());

        let result = match entry.fs_type {
            VfsFsType::ProcFs => {
                if is_mount_root(relative_path) {
                    return Err("procfs: path is a directory");
                }
                let content = generate_proc_content(relative_path);
                if content.is_empty() {
                    Err("procfs: entry not found")
                } else {
                    Ok(content.into_bytes())
                }
            }
            VfsFsType::SysFs => {
                if is_mount_root(relative_path) {
                    return Err("sysfs: path is a directory");
                }
                read_sysfs_bytes(relative_path)
            }
            VfsFsType::DevFs => Err(unsupported_vfs_capability(
                VfsFsType::DevFs,
                VfsUnsupportedCapability::Read,
            )),
            VfsFsType::TmpFs => Err(unsupported_vfs_capability(
                VfsFsType::TmpFs,
                VfsUnsupportedCapability::Read,
            )),
            VfsFsType::F2fs => {
                let entry = crate::fs::f2fs::open_entry(relative_path)
                    .map_err(|_| "f2fs: file not found")?;
                if entry.is_dir {
                    return Err("f2fs: path is a directory");
                }
                read_f2fs_bytes_exact(relative_path, &entry)
            }
            VfsFsType::Ext4 => {
                let resolved = resolve_ext4_node(&entry.source, relative_path)?;
                if resolved.inode.is_directory() {
                    return Err("ext4: path is a directory");
                }
                // Linux: inode_lock_shared(file) — shared lock for reads
                crate::fs::ext4::ext4_inode_lock_shared(&entry.source, resolved.inode_num);
                let result = resolved
                    .mounted
                    .fs
                    .read_file_from_storage(&resolved.inode, &resolved.mounted.storage)
                    .map_err(|_| "ext4: failed to read file");
                crate::fs::ext4::ext4_inode_unlock_shared(&entry.source, resolved.inode_num);
                result
            }
            VfsFsType::Fat32 => {
                let resolved = resolve_fat32_node(&entry.source, relative_path)?;
                if resolved.file.is_dir {
                    return Err("fat32: path is a directory");
                }
                fat32_read_file(&resolved)
            }
            VfsFsType::ExFat => {
                let resolved = resolve_exfat_node(&entry.source, relative_path)?;
                if resolved.file.is_dir {
                    return Err("exfat: path is a directory");
                }
                exfat_read_file(&resolved)
            }
            VfsFsType::Ntfs => {
                let resolved = resolve_ntfs_node(&entry.source, relative_path)?;
                if matches!(
                    resolved.metadata.as_ref().map(|meta| meta.file_type),
                    Some(crate::fs::ntfs::NtfsFileType::Directory)
                ) {
                    return Err("ntfs: path is a directory");
                }
                resolved
                    .mounted
                    .fs
                    .read_file_from_storage(&resolved.entry, &resolved.mounted.storage)
                    .map_err(|_| "ntfs: failed to read file")
            }
            VfsFsType::Xfs => Err(unsupported_vfs_capability(
                VfsFsType::Xfs,
                VfsUnsupportedCapability::Read,
            )),
            VfsFsType::Btrfs => {
                let resolved = resolve_btrfs_node(&entry.source, relative_path)?;
                if resolved.inode.is_directory() {
                    return Err("btrfs: path is a directory");
                }
                resolved
                    .mounted
                    .fs
                    .read_file_from_storage(resolved.inode_num, &resolved.mounted.storage)
            }
            VfsFsType::Erofs => {
                if is_mount_root(relative_path) {
                    return Err("erofs: path is a directory");
                }
                Err("erofs: VFS read requires full path resolution")
            }
            VfsFsType::Squashfs => {
                if is_mount_root(relative_path) {
                    return Err("squashfs: path is a directory");
                }
                Err("squashfs: VFS read requires full path resolution")
            }
        };

        result
    }

    /// VFS read with symlink following, page cache, and fanotify.
    ///
    /// Uses `namei::resolve` to follow symlinks first, then reads
    /// from the resolved path.
    pub fn read_bytes(&self, path: &str) -> Result<Vec<u8>, &'static str> {
        // POSIX.1-2024 path_resolution(7) contract
        if let Err(e) = validate_path(path) {
            return Err(fs_error_to_str(&e));
        }
        let normalized_path = normalize_vfs_path(path);

        // Page cache lookup: check file-level cache before disk I/O
        let path_hash = hash_path(&normalized_path);
        if let Some(cached) = page_cache::find_page(path_hash, 0) {
            return Ok(cached.data);
        }

        if let Some(entry) = self.resolve_fs(&normalized_path) {
            if matches!(
                entry.fs_type,
                VfsFsType::DevFs | VfsFsType::TmpFs | VfsFsType::Xfs
            ) {
                return self.read_bytes_direct(&normalized_path);
            }
        }

        // Resolve path with symlink following + dcache caching
        let resolved = crate::fs::namei::resolve(
            |parent_ino, parent_path, name| {
                self.lookup_component(parent_ino, parent_path, name)
            },
            |p| self.read_bytes_direct(p),
            &normalized_path,
            true,
        )?;

        // Read the resolved path's content
        let result = self.read_bytes_direct(&resolved.resolved_path);

        // fanotify: notify access event
        if result.is_ok() {
            crate::fs::fanotify::notify_access(&resolved.resolved_path, 0);
        }
        result
    }

    pub fn list_dir(&self, path: &str) -> Result<Vec<VfsDirEntry>, &'static str> {
        // POSIX.1-2024 path_resolution(7) contract
        if let Err(e) = validate_path(path) {
            return Err(fs_error_to_str(&e));
        }
        let normalized_path = normalize_vfs_path(path);
        let entry = self
            .resolve_fs(normalized_path.as_str())
            .ok_or_else(no_filesystem_for_path)?;
        let relative_path =
            relative_mount_path(entry.mount_point.as_str(), normalized_path.as_str());

        match entry.fs_type {
            VfsFsType::ProcFs => list_procfs_dir(relative_path),
            VfsFsType::SysFs => list_sysfs_dir(relative_path),
            VfsFsType::DevFs => list_devfs_dir(relative_path),
            VfsFsType::TmpFs => {
                if is_mount_root(relative_path) {
                    Ok(Vec::new())
                } else {
                    Err(unsupported_vfs_capability(
                        VfsFsType::TmpFs,
                        VfsUnsupportedCapability::ListDirectory,
                    ))
                }
            }
            VfsFsType::F2fs => crate::fs::f2fs::list_dir(relative_path)
                .map(|entries| {
                    entries
                        .into_iter()
                        .map(|entry| VfsDirEntry {
                            name: entry
                                .name
                                .split('/')
                                .next_back()
                                .unwrap_or(entry.name.as_str())
                                .to_string(),
                            size: entry.size,
                            is_directory: entry.is_dir,
                            fs_type: VfsFsType::F2fs,
                        })
                        .collect()
                })
                .map_err(|_| "f2fs: failed to list directory"),
            VfsFsType::Ext4 => list_ext4_dir(&entry.source, relative_path),
            VfsFsType::Fat32 => list_fat32_dir(&entry.source, relative_path),
            VfsFsType::ExFat => list_exfat_dir(&entry.source, relative_path),
            VfsFsType::Ntfs => list_ntfs_dir(&entry.source, relative_path),
            VfsFsType::Xfs => Err(unsupported_vfs_capability(
                VfsFsType::Xfs,
                VfsUnsupportedCapability::ListDirectory,
            )),
            VfsFsType::Btrfs => list_btrfs_dir(&entry.source, relative_path),
            VfsFsType::Erofs => {
                if is_mount_root(relative_path) {
                    Ok(Vec::new())
                } else {
                    Err("erofs: VFS list_dir requires full path resolution")
                }
            }
            VfsFsType::Squashfs => {
                if is_mount_root(relative_path) {
                    Ok(Vec::new())
                } else {
                    Err("squashfs: VFS list_dir requires full path resolution")
                }
            }
        }
    }

    /// Write bytes to a file (truncating/replacing content).
    pub fn write_bytes(&self, path: &str, data: &[u8]) -> Result<(), &'static str> {
        if let Err(e) = validate_path(path) {
            return Err(fs_error_to_str(&e));
        }
        let normalized_path = normalize_vfs_path(path);
        let entry = self.resolve_fs(normalized_path.as_str()).ok_or_else(no_filesystem_for_path)?;
        if entry.readonly { return Err("read-only filesystem (EROFS)"); }
        let relative_path = relative_mount_path(entry.mount_point.as_str(), normalized_path.as_str());
        let result = match entry.fs_type {
            VfsFsType::F2fs => {
                let parent = crate::fs::namei::parent_path(relative_path);
                let name = normalized_path.rsplit_once('/').map(|(_, n)| n).unwrap_or("");
                if !name.is_empty() {
                    let _ = crate::fs::f2fs::create_f2fs_file_with_data(&parent, name, data)
                        .map_err(|_| "f2fs: write failed")?;
                }
                Ok(())
            }
            VfsFsType::Ext4 => {
                crate::fs::ext4::ext4_write_file(&entry.source, relative_path, data)
            }
            VfsFsType::Fat32 => {
                crate::fs::fat::create_fat32_file(&entry.source, relative_path, data)
            }
            VfsFsType::ExFat => {
                if relative_path.contains('/') {
                    return Err("exfat: nested create is not implemented; fail-closed");
                }
                let name = normalized_path.rsplit_once('/').map(|(_, n)| n).unwrap_or(relative_path);
                crate::fs::fat::create_exfat_file_vfs(&entry.source, name, data)
            }
            _ => Err(unsupported_vfs_capability(entry.fs_type, VfsUnsupportedCapability::Write)),
        };
        // fanotify: notify modify event
        if result.is_ok() {
            crate::fs::fanotify::notify_modify(&normalized_path, 0);
        }
        result
    }

    /// Read bytes from file at offset (sys_read için — POSIX offset tabanlı I/O).
    /// Tüm dosyayı okur, offset'ten itibaren buf.length kadar keser.
    pub fn read_bytes_at(&self, path: &str, offset: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
        if let Err(e) = validate_path(path) {
            return Err(fs_error_to_str(&e));
        }
        let normalized_path = normalize_vfs_path(path);
        let resolved = crate::fs::namei::resolve(
            |parent_ino, parent_path, name| {
                self.lookup_component(parent_ino, parent_path, name)
            },
            |p| self.read_bytes_direct(p),
            &normalized_path,
            true,
        )?;
        let all_data = self.read_bytes_direct(&resolved.resolved_path)?;
        if offset >= all_data.len() {
            return Ok(0); // EOF
        }
        let available = &all_data[offset..];
        let copy_len = buf.len().min(available.len());
        buf[..copy_len].copy_from_slice(&available[..copy_len]);
        Ok(copy_len)
    }

    /// Write bytes to file at offset (sys_write için — POSIX offset tabanlı I/O).
    pub fn write_bytes_at(&self, path: &str, offset: usize, data: &[u8]) -> Result<usize, &'static str> {
        if let Err(e) = validate_path(path) {
            return Err(fs_error_to_str(&e));
        }
        let normalized_path = normalize_vfs_path(path);
        let entry = self.resolve_fs(normalized_path.as_str()).ok_or_else(no_filesystem_for_path)?;
        if entry.readonly { return Err("read-only filesystem (EROFS)"); }
        let relative_path = relative_mount_path(entry.mount_point.as_str(), normalized_path.as_str());
        match entry.fs_type {
            VfsFsType::F2fs => {
                crate::fs::f2fs::write_f2fs_file_at(&relative_path, offset, data)
                    .map_err(|_| "f2fs: write failed")?;
            }
            VfsFsType::Ext4 => {
                let mut existing = {
                    let resolved = resolve_ext4_node(&entry.source, relative_path)?;
                    resolved.mounted.fs.read_file_from_storage(&resolved.inode, &resolved.mounted.storage)
                        .map_err(|_| "ext4: read failed")?
                };
                if offset > existing.len() {
                    existing.resize(offset, 0);
                }
                let end = offset + data.len();
                if end > existing.len() {
                    existing.resize(end, 0);
                }
                existing[offset..end].copy_from_slice(data);
                crate::fs::ext4::ext4_write_file(&entry.source, relative_path, &existing)?;
            }
            VfsFsType::Fat32 => {
                crate::fs::fat::write_fat32_file(&entry.source, relative_path, data, offset)?;
            }
            VfsFsType::ExFat => {
                crate::fs::fat::write_exfat_file_vfs(&entry.source, relative_path, data, offset)?;
            }
            _ => return Err(unsupported_vfs_capability(entry.fs_type, VfsUnsupportedCapability::Write)),
        }
        // Page cache invalidation: write sonrası cache'i temizle
        page_cache::invalidate_inode(hash_path(&normalized_path));
        Ok(data.len())
    }

    /// fsync: dosya ve metadata'yı diske yazdırır (tüm backend'ler için)
    /// fsync: dosya ve metadata'yı diske yazdırır
    /// Deep web: Linux kernel fs/sync.c sync_filesystem()
    ///
    /// # Destek Matrisi
    /// - F2fs: fsync_path() ile desteklenir
    /// - Ext4: ext4_fsync() ile desteklenir
    /// - Fat32/NTFS/Btrfs: fsync desteklenmez → `Err("unsupported")`
    pub fn fsync(&self, path: &str) -> Result<(), &'static str> {
        if let Err(e) = validate_path(path) {
            return Err(fs_error_to_str(&e));
        }
        let normalized_path = normalize_vfs_path(path);
        let entry = self.resolve_fs(normalized_path.as_str()).ok_or_else(no_filesystem_for_path)?;
        let relative_path = relative_mount_path(entry.mount_point.as_str(), normalized_path.as_str());

        // fsync destek matrisi — her backend için kontrol
        match entry.fs_type {
            VfsFsType::F2fs => crate::fs::f2fs::fsync_path(&relative_path).map_err(|_| "f2fs: fsync failed"),
            VfsFsType::Ext4 => crate::fs::ext4::ext4_fsync(&entry.source, &relative_path).map_err(|_| "ext4: fsync failed"),
            VfsFsType::Fat32 => Err("fat32: fsync desteklenmiyor (EOPNOTSUPP)"),
            VfsFsType::ExFat => Err("exfat: fsync desteklenmiyor (EOPNOTSUPP)"),
            VfsFsType::Ntfs => Err("ntfs: fsync desteklenmiyor (EOPNOTSUPP)"),
            VfsFsType::Btrfs => Err("btrfs: fsync desteklenmiyor (EOPNOTSUPP)"),
            VfsFsType::ProcFs | VfsFsType::SysFs | VfsFsType::DevFs | VfsFsType::TmpFs => {
                Err("sanal fs: fsync desteklenmiyor (EOPNOTSUPP)")
            }
            _ => Err("bilinmeyen fs: fsync desteklenmiyor (EOPNOTSUPP)"),
        }
    }

    /// fdatasync: sadece data'yı diske yazdırır (metadata değil)
    /// Deep web: Linux kernel fs/sync.c sync_filesystem()
    pub fn fdatasync(&self, path: &str) -> Result<(), &'static str> {
        if let Err(e) = validate_path(path) {
            return Err(fs_error_to_str(&e));
        }
        let normalized_path = normalize_vfs_path(path);
        let entry = self.resolve_fs(normalized_path.as_str()).ok_or_else(no_filesystem_for_path)?;
        let relative_path = relative_mount_path(entry.mount_point.as_str(), normalized_path.as_str());

        match entry.fs_type {
            VfsFsType::F2fs => crate::fs::f2fs::fdatasync_path(&relative_path).map_err(|_| "f2fs: fdatasync failed"),
            VfsFsType::Ext4 => crate::fs::ext4::ext4_fsync(&entry.source, &relative_path).map_err(|_| "ext4: fdatasync failed"),
            VfsFsType::Fat32 => Err("fat32: fdatasync desteklenmiyor (EOPNOTSUPP)"),
            VfsFsType::ExFat => Err("exfat: fdatasync desteklenmiyor (EOPNOTSUPP)"),
            VfsFsType::Ntfs => Err("ntfs: fdatasync desteklenmiyor (EOPNOTSUPP)"),
            VfsFsType::Btrfs => Err("btrfs: fdatasync desteklenmiyor (EOPNOTSUPP)"),
            VfsFsType::ProcFs | VfsFsType::SysFs | VfsFsType::DevFs | VfsFsType::TmpFs => {
                Err("sanal fs: fdatasync desteklenmiyor (EOPNOTSUPP)")
            }
            _ => Err("bilinmeyen fs: fdatasync desteklenmiyor (EOPNOTSUPP)"),
        }
    }

    /// chmod: dosya izinlerini değiştirir
    pub fn chmod(&self, path: &str, mode: u16) -> Result<(), &'static str> {
        if let Err(e) = validate_path(path) {
            return Err(fs_error_to_str(&e));
        }
        let normalized_path = normalize_vfs_path(path);
        let entry = self.resolve_fs(normalized_path.as_str()).ok_or_else(no_filesystem_for_path)?;
        let relative_path = relative_mount_path(entry.mount_point.as_str(), normalized_path.as_str());
        match entry.fs_type {
            VfsFsType::F2fs => crate::fs::f2fs::chmod_f2fs(&relative_path, mode).map_err(|_| "f2fs: chmod failed"),
            VfsFsType::Ext4 => crate::fs::ext4::ext4_chmod(&entry.source, &relative_path, mode).map_err(|_| "ext4: chmod failed"),
            _ => Err(unsupported_vfs_capability(entry.fs_type, VfsUnsupportedCapability::Chmod)),
        }
    }

    /// chown: dosya sahipliğini değiştirir
    pub fn chown(&self, path: &str, uid: u32, gid: u32) -> Result<(), &'static str> {
        if let Err(e) = validate_path(path) {
            return Err(fs_error_to_str(&e));
        }
        let normalized_path = normalize_vfs_path(path);
        let entry = self.resolve_fs(normalized_path.as_str()).ok_or_else(no_filesystem_for_path)?;
        let relative_path = relative_mount_path(entry.mount_point.as_str(), normalized_path.as_str());
        match entry.fs_type {
            VfsFsType::F2fs => crate::fs::f2fs::chown_f2fs(&relative_path, uid, gid).map_err(|_| "f2fs: chown failed"),
            VfsFsType::Ext4 => crate::fs::ext4::ext4_chown(&entry.source, &relative_path, uid, gid).map_err(|_| "ext4: chown failed"),
            _ => Err(unsupported_vfs_capability(entry.fs_type, VfsUnsupportedCapability::Chown)),
        }
    }

    /// fallocate: dosya için alan ayırır (deallocation da desteklenir)
    pub fn fallocate(&self, path: &str, offset: u64, len: u64) -> Result<(), &'static str> {
        if let Err(e) = validate_path(path) {
            return Err(fs_error_to_str(&e));
        }
        let normalized_path = normalize_vfs_path(path);
        let entry = self.resolve_fs(normalized_path.as_str()).ok_or_else(no_filesystem_for_path)?;
        if entry.readonly { return Err("read-only filesystem (EROFS)"); }
        let relative_path = relative_mount_path(entry.mount_point.as_str(), normalized_path.as_str());
        match entry.fs_type {
            VfsFsType::F2fs => {
                let new_size = (offset + len) as u64;
                crate::fs::f2fs::truncate_f2fs(&relative_path, new_size)
                    .map_err(|_| "f2fs: fallocate failed")?;
            }
            VfsFsType::Ext4 => {
                let new_size = (offset + len) as u64;
                crate::fs::ext4::ext4_truncate(&entry.source, &relative_path, new_size)
                    .map_err(|_| "ext4: fallocate failed")?;
            }
            _ => return Err(unsupported_vfs_capability(entry.fs_type, VfsUnsupportedCapability::Fallocate)),
        }
        Ok(())
    }

    pub fn create_file(&self, parent_path: &str, name: &str) -> Result<(), &'static str> {
        if let Err(e) = validate_path(parent_path) {
            return Err(fs_error_to_str(&e));
        }
        let normalized = normalize_vfs_path(parent_path);
        let entry = self.resolve_fs(&normalized).ok_or_else(no_filesystem_for_path)?;
        if entry.readonly { return Err("read-only filesystem (EROFS)"); }
        let relative = relative_mount_path(entry.mount_point.as_str(), &normalized);
        let full_path = if normalized == "/" {
            format!("/{}", name)
        } else {
            format!("{}/{}", normalized, name)
        };
        let result = match entry.fs_type {
            VfsFsType::F2fs => {
                crate::fs::f2fs::create_f2fs_file(relative, name)
                    .map_err(|_| "f2fs: create file failed")
            }
            VfsFsType::Ext4 => {
                crate::fs::ext4::ext4_create_file(&entry.source, relative, name)
            }
            VfsFsType::Fat32 => {
                let fat_path = if is_mount_root(relative) {
                    name.to_string()
                } else {
                    format!("{}/{}", relative, name)
                };
                crate::fs::fat::create_fat32_file(&entry.source, &fat_path, &[])
            }
            VfsFsType::ExFat => {
                if !is_mount_root(relative) {
                    return Err("exfat: nested create is not implemented; fail-closed");
                }
                crate::fs::fat::create_exfat_file_vfs(&entry.source, name, &[])
            }
            _ => Err(unsupported_vfs_capability(entry.fs_type, VfsUnsupportedCapability::Create)),
        };
        // fanotify: notify create event
        if result.is_ok() {
            crate::fs::fanotify::notify_create(&full_path, 0);
        }
        result
    }

    /// Create a directory (mkdir -p semantics: trailing components are created).
    pub fn create_dir(&self, parent_path: &str, name: &str) -> Result<(), &'static str> {
        if let Err(e) = validate_path(parent_path) {
            return Err(fs_error_to_str(&e));
        }
        let normalized = normalize_vfs_path(parent_path);
        let entry = self.resolve_fs(&normalized).ok_or_else(no_filesystem_for_path)?;
        if entry.readonly { return Err("read-only filesystem (EROFS)"); }
        let relative = relative_mount_path(entry.mount_point.as_str(), &normalized);
        let full_path = if normalized == "/" {
            format!("/{}", name)
        } else {
            format!("{}/{}", normalized, name)
        };
        let result = match entry.fs_type {
            VfsFsType::F2fs => {
                crate::fs::f2fs::create_f2fs_dir(relative, name)
                    .map_err(|_| "f2fs: mkdir failed")
            }
            VfsFsType::Ext4 => {
                crate::fs::ext4::ext4_create_dir(&entry.source, relative, name)
            }
            VfsFsType::Fat32 => {
                let fat_path = if is_mount_root(relative) {
                    name.to_string()
                } else {
                    format!("{}/{}", relative, name)
                };
                crate::fs::fat::mkdir_fat32(&entry.source, &fat_path)
            }
            VfsFsType::ExFat => {
                if !is_mount_root(relative) {
                    return Err("exfat: nested mkdir is not implemented; fail-closed");
                }
                crate::fs::fat::mkdir_exfat_vfs(&entry.source, name)
            }
            _ => Err(unsupported_vfs_capability(entry.fs_type, VfsUnsupportedCapability::Create)),
        };
        // fanotify: notify create event
        if result.is_ok() {
            crate::fs::fanotify::notify_create(&full_path, 0);
        }
        result
    }

    /// Remove an empty directory.
    pub fn remove_dir(&self, parent_path: &str, name: &str) -> Result<(), &'static str> {
        if let Err(e) = validate_path(parent_path) {
            return Err(fs_error_to_str(&e));
        }
        let normalized = normalize_vfs_path(parent_path);
        let entry = self.resolve_fs(&normalized).ok_or_else(no_filesystem_for_path)?;
        if entry.readonly { return Err("read-only filesystem (EROFS)"); }
        let relative = relative_mount_path(entry.mount_point.as_str(), &normalized);
        let full_path = if normalized == "/" {
            format!("/{}", name)
        } else {
            format!("{}/{}", normalized, name)
        };
        let result = match entry.fs_type {
            VfsFsType::F2fs => {
                let ops = self.inode_ops(entry.fs_type);
                match ops.rmdir {
                    Some(f) => f(relative, name),
                    None => Err(unsupported_vfs_capability(entry.fs_type, VfsUnsupportedCapability::Unlink)),
                }
            }
            VfsFsType::Ext4 => {
                crate::fs::ext4::ext4_unlink(&entry.source, relative, name)
            }
            VfsFsType::ExFat => {
                if !is_mount_root(relative) {
                    return Err("exfat: nested rmdir is not implemented; fail-closed");
                }
                crate::fs::fat::delete_exfat_file_vfs(&entry.source, name)
            }
            _ => Err(unsupported_vfs_capability(entry.fs_type, VfsUnsupportedCapability::Unlink)),
        };
        // fanotify: notify delete event
        if result.is_ok() {
            crate::fs::fanotify::notify_delete(&full_path, 0);
        }
        result
    }

    /// Unlink (delete) a file from a directory.
    pub fn unlink(&self, parent_path: &str, name: &str) -> Result<(), &'static str> {
        if let Err(e) = validate_path(parent_path) {
            return Err(fs_error_to_str(&e));
        }
        let normalized = normalize_vfs_path(parent_path);
        let entry = self.resolve_fs(&normalized).ok_or_else(no_filesystem_for_path)?;
        if entry.readonly { return Err("read-only filesystem (EROFS)"); }
        let relative = relative_mount_path(entry.mount_point.as_str(), &normalized);
        let full_path = if normalized == "/" {
            format!("/{}", name)
        } else {
            format!("{}/{}", normalized, name)
        };
        let result = match entry.fs_type {
            VfsFsType::F2fs => {
                crate::fs::f2fs::unlink_f2fs(relative, name)
                    .map_err(|_| "f2fs: unlink failed")
            }
            VfsFsType::Ext4 => {
                crate::fs::ext4::ext4_unlink(&entry.source, relative, name)
            }
            VfsFsType::ExFat => {
                if !is_mount_root(relative) {
                    return Err("exfat: nested unlink is not implemented; fail-closed");
                }
                crate::fs::fat::delete_exfat_file_vfs(&entry.source, name)
            }
            _ => Err(unsupported_vfs_capability(entry.fs_type, VfsUnsupportedCapability::Unlink)),
        };
        // fanotify: notify delete event
        if result.is_ok() {
            crate::fs::fanotify::notify_delete(&full_path, 0);
        }
        result
    }

    /// Rename a file or directory within the same parent.
    pub fn rename(&self, parent_path: &str, old_name: &str, new_name: &str) -> Result<(), &'static str> {
        if let Err(e) = validate_path(parent_path) {
            return Err(fs_error_to_str(&e));
        }
        let normalized = normalize_vfs_path(parent_path);
        let entry = self.resolve_fs(&normalized).ok_or_else(no_filesystem_for_path)?;
        if entry.readonly { return Err("read-only filesystem (EROFS)"); }
        let relative = relative_mount_path(entry.mount_point.as_str(), &normalized);
        let full_path = if normalized == "/" {
            format!("/{}", new_name)
        } else {
            format!("{}/{}", normalized, new_name)
        };
        let result = match entry.fs_type {
            VfsFsType::F2fs => {
                crate::fs::f2fs::rename_f2fs(relative, old_name, new_name)
                    .map_err(|_| "f2fs: rename failed")
            }
            VfsFsType::Ext4 => {
                crate::fs::ext4::ext4_rename(&entry.source, relative, old_name, relative, new_name)
            }
            VfsFsType::ExFat => {
                if !is_mount_root(relative) {
                    return Err("exfat: nested rename is not implemented; fail-closed");
                }
                crate::fs::fat::rename_exfat_vfs(&entry.source, old_name, new_name)
            }
            _ => Err(unsupported_vfs_capability(entry.fs_type, VfsUnsupportedCapability::Rename)),
        };
        // fanotify: notify move events
        if result.is_ok() {
            crate::fs::fanotify::notify_moved_from(&full_path, 0);
            crate::fs::fanotify::notify_moved_to(&full_path, 0);
        }
        result
    }

    /// Truncate a file to a given size.
    pub fn truncate(&self, path: &str, new_size: u64) -> Result<(), &'static str> {
        if let Err(e) = validate_path(path) {
            return Err(fs_error_to_str(&e));
        }
        let normalized = normalize_vfs_path(path);
        let entry = self.resolve_fs(&normalized).ok_or_else(no_filesystem_for_path)?;
        if entry.readonly { return Err("read-only filesystem (EROFS)"); }
        let relative = relative_mount_path(entry.mount_point.as_str(), &normalized);
        match entry.fs_type {
            VfsFsType::F2fs => {
                crate::fs::f2fs::truncate_f2fs(&normalized, new_size)
                    .map_err(|_| "f2fs: truncate failed")
            }
            VfsFsType::Ext4 => {
                crate::fs::ext4::ext4_truncate(&entry.source, relative, new_size)
            }
            VfsFsType::Fat32 => {
                if new_size > u32::MAX as u64 {
                    return Err("fat32: truncate size exceeds 4GiB maximum");
                }
                crate::fs::fat::truncate_fat32_file(&entry.source, relative, new_size as u32)
            }
            VfsFsType::ExFat => {
                if relative.contains('/') {
                    return Err("exfat: nested truncate is not implemented; fail-closed");
                }
                crate::fs::fat::truncate_exfat_file_vfs(&entry.source, relative, new_size)
            }
            _ => Err(unsupported_vfs_capability(entry.fs_type, VfsUnsupportedCapability::Truncate)),
        }
    }

    /// Create a symbolic link.
    pub fn symlink(&self, parent_path: &str, name: &str, target: &str) -> Result<(), &'static str> {
        if let Err(e) = validate_path(parent_path) {
            return Err(fs_error_to_str(&e));
        }
        let normalized = normalize_vfs_path(parent_path);
        let entry = self.resolve_fs(&normalized).ok_or_else(no_filesystem_for_path)?;
        if entry.readonly { return Err("read-only filesystem (EROFS)"); }
        let relative = relative_mount_path(entry.mount_point.as_str(), &normalized);
        match entry.fs_type {
            VfsFsType::F2fs => {
                crate::fs::f2fs::create_symlink(relative, name, target)
                    .map_err(|_| "f2fs: symlink failed")
            }
            VfsFsType::Ext4 => {
                crate::fs::ext4::ext4_create_symlink(&entry.source, relative, name, target)
            }
            _ => Err(unsupported_vfs_capability(entry.fs_type, VfsUnsupportedCapability::Symlink)),
        }
    }

    /// Stat a file: return VfsFileInfo.
    pub fn stat(&self, path: &str) -> Result<VfsFileInfo, &'static str> {
        self.open(path)
    }

    /// Get VfsSuperOps for a given backend type.
    pub fn super_ops(&self, fs_type: VfsFsType) -> VfsSuperOps {
        match fs_type {
            VfsFsType::F2fs => VfsSuperOps {
                sync_fs: Some(|| crate::fs::f2fs::sync_f2fs().map_err(|_| "f2fs sync failed")),
                stat_fs: Some(|| {
                    let stats = crate::fs::f2fs::f2fs_stats().map_err(|_| "f2fs stats failed")?;
                    Ok((stats.total_main_blocks * 4096, stats.used_blocks * 4096, stats.free_blocks * 4096))
                }),
                mount_root: None,
            },
            VfsFsType::Ext4 => VfsSuperOps {
                sync_fs: None,
                stat_fs: None,
                mount_root: None,
            },
            _ => VfsSuperOps::empty(),
        }
    }

    /// Get VfsInodeOps for a given backend type.
    pub fn inode_ops(&self, fs_type: VfsFsType) -> VfsInodeOps {
        match fs_type {
            VfsFsType::F2fs => {
                let ops: VfsInodeOps = VfsInodeOps {
                    create: Some(|parent, name| {
                        crate::fs::f2fs::create_f2fs_file(parent, name).map_err(|_| "f2fs: create failed")
                    }),
                    mkdir: Some(|parent, name| {
                        crate::fs::f2fs::create_f2fs_dir(parent, name).map_err(|_| "f2fs: mkdir failed")
                    }),
                    unlink: Some(|parent, name| {
                        crate::fs::f2fs::unlink_f2fs(parent, name).map_err(|_| "f2fs: unlink failed")
                    }),
                    rename: Some(|parent, old, new| {
                        crate::fs::f2fs::rename_f2fs(parent, old, new).map_err(|_| "f2fs: rename failed")
                    }),
                    symlink: Some(|parent, name, target| {
                        crate::fs::f2fs::create_symlink(parent, name, target).map_err(|_| "f2fs: symlink failed")
                    }),
                    truncate: Some(|path, size| {
                        crate::fs::f2fs::truncate_f2fs(path, size).map_err(|_| "f2fs: truncate failed")
                    }),
                    lookup: Some(|parent_ino, _parent_path, name| {
                        crate::fs::f2fs::lookup_child(parent_ino, name)
                            .map(|child| vfs_info_from_f2fs_entry(&child))
                            .map_err(|_| "f2fs: component not found")
                    }),
                    rmdir: Some(|parent, name| {
                        crate::fs::f2fs::unlink_f2fs(parent, name).map_err(|_| "f2fs: rmdir failed")
                    }),
                    readlink: Some(|path| {
                        crate::fs::f2fs::read_f2fs_symlink(path).map_err(|_| "f2fs: readlink failed")
                    }),
                    link: Some(|parent, name, target| {
                        crate::fs::f2fs::create_hardlink(parent, name, target).map_err(|_| "f2fs: link failed")
                    }),
                    chmod: Some(|path, mode| {
                        crate::fs::f2fs::set_file_metadata(path, Some(mode), None, None)
                            .map_err(|_| "f2fs: chmod failed")
                    }),
                    chown: Some(|path, uid, gid| {
                        crate::fs::f2fs::set_file_metadata(path, None, Some(uid), Some(gid))
                            .map_err(|_| "f2fs: chown failed")
                    }),
                    stat: Some(|path| {
                        crate::fs::f2fs::open_entry(path)
                            .map(|e| vfs_info_from_f2fs_entry(&e))
                            .map_err(|_| "f2fs: stat failed")
                    }),
                };
                ops
            }
            VfsFsType::Ext4 => {
                VfsInodeOps {
                    create: Some(|parent, name| {
                        // Need source from mount entry; use dispatch via open_direct pattern
                        Err("ext4: use VFS create_file dispatch instead")
                    }),
                    mkdir: Some(|parent, name| {
                        Err("ext4: use VFS create_dir dispatch instead")
                    }),
                    unlink: Some(|parent, name| {
                        Err("ext4: use VFS unlink dispatch instead")
                    }),
                    rename: Some(|parent, old, new| {
                        Err("ext4: use VFS rename dispatch instead")
                    }),
                    lookup: Some(|_parent_ino, parent_path, name| {
                        // Fallback to full open_direct
                        Err("ext4: use VFS lookup_component dispatch instead")
                    }),
                    stat: Some(|_path| {
                        Err("ext4: use VFS stat dispatch instead")
                    }),
                    ..VfsInodeOps::empty()
                }
            }
            _ => VfsInodeOps::empty(),
        }
    }

    /// Get VfsFileOps for a given backend type.
    pub fn file_ops(&self, fs_type: VfsFsType) -> VfsFileOps {
        match fs_type {
            VfsFsType::F2fs => VfsFileOps {
                read: Some(|path, offset, buf| {
                    crate::fs::f2fs::read_f2fs_file_at(path, offset as usize, buf)
                        .map_err(|_| "f2fs: read failed")
                }),
                write: Some(|path, offset, buf| {
                    crate::fs::f2fs::write_f2fs_file_at(path, offset as usize, buf)
                        .map_err(|_| "f2fs: write failed")
                }),
                copy_file_range: Some(|src_path, src_off, dst_path, dst_off, size| {
                    let mut buf = alloc::vec![0u8; size as usize];
                    let n = crate::fs::f2fs::read_f2fs_file_at(src_path, src_off as usize, &mut buf)
                        .map_err(|_| "copy_file_range read error")?;
                    crate::fs::f2fs::write_f2fs_file_at(dst_path, dst_off as usize, &buf)
                        .map_err(|_| "f2fs: copy dest write failed")?;
                    Ok(n as u64)
                }),
                fadvise: None,
            },
            VfsFsType::Ext4 => VfsFileOps {
                // Note: ext4 file_ops require source path from mount entry;
                // use read_bytes/write_bytes VFS methods which dispatch correctly.
                read: None,
                write: None,
                copy_file_range: None,
                fadvise: None,
            },
            _ => VfsFileOps::empty(),
        }
    }
}

// ============================================================================
// Global Instance
// ============================================================================

lazy_static::lazy_static! {
    pub static ref VFS_UNIFIED: Mutex<VfsUnified> = Mutex::new(VfsUnified::new());
}

/// VFS birleşik katmanını başlatır ve varsayılan mount noktalarını ekler
pub fn init() {
    let flags = VfsMountFlags::default();
    let mut vfs = VFS_UNIFIED.lock();

    // Root filesystem
    vfs.mount("/", VfsFsType::F2fs, "/dev/nvme0n1p1", flags);
    // Virtual filesystems
    vfs.mount("/proc", VfsFsType::ProcFs, "proc", flags);
    vfs.mount("/dev", VfsFsType::DevFs, "devtmpfs", flags);
    vfs.mount("/sys", VfsFsType::SysFs, "sysfs", flags);
    vfs.mount("/tmp", VfsFsType::TmpFs, "tmpfs", flags);

    crate::serial_println!(
        "[VFS] Unified layer initialized: {} filesystems mounted",
        vfs.mount_count()
    );
}

/// Path routing — dosya sistemi türünü döndürür
pub fn resolve_path(path: &str) -> Option<VfsFsType> {
    VFS_UNIFIED.lock().resolve_fs(path).map(|e| e.fs_type)
}

/// Mount listesi (shell mount komutu için)
pub fn list_mounts() -> Vec<String> {
    VFS_UNIFIED.lock().list_mounts()
}

/// Convert FsError to a static string for VFS layer compatibility.
fn fs_error_to_str(err: &crate::fs::FsError) -> &'static str {
    match err {
        crate::fs::FsError::NotFound => "no such file or directory (ENOENT)",
        crate::fs::FsError::InvalidPath => "invalid path (EINVAL)",
        crate::fs::FsError::NameTooLong => "filename too long (ENAMETOOLONG)",
        crate::fs::FsError::ComponentTooLong => "filename component too long (ENAMETOOLONG)",
        crate::fs::FsError::NotDirectory => "not a directory (ENOTDIR)",
        crate::fs::FsError::IsDirectory => "is a directory (EISDIR)",
        crate::fs::FsError::AlreadyExists => "file exists (EEXIST)",
        crate::fs::FsError::PermissionDenied => "permission denied (EACCES)",
        crate::fs::FsError::ReadOnlyFs => "read-only filesystem (EROFS)",
        crate::fs::FsError::CrossDevice => "cross-device link (EXDEV)",
        crate::fs::FsError::SymlinkLoop => "too many symbolic links (ELOOP)",
        crate::fs::FsError::UnsupportedSymlink => "symlink not supported (EOPNOTSUPP)",
        crate::fs::FsError::UnsupportedBackend => "backend not supported (ENODEV)",
        crate::fs::FsError::UnsupportedFeature(_) => "feature not supported (EOPNOTSUPP)",
        crate::fs::FsError::NeedsRecovery => "filesystem needs recovery (EIO)",
        crate::fs::FsError::CorruptFs => "filesystem corrupt (EIO)",
        crate::fs::FsError::IoError => "I/O error (EIO)",
        crate::fs::FsError::ReadError => "read error (EIO)",
        crate::fs::FsError::WriteError => "write error (EIO)",
        crate::fs::FsError::NoSpace => "no space left on device (ENOSPC)",
        crate::fs::FsError::QuotaExceeded => "quota exceeded (EDQUOT)",
        crate::fs::FsError::StaleHandle => "stale file handle (ESTALE)",
        crate::fs::FsError::Busy => "device or resource busy (EBUSY)",
        crate::fs::FsError::NoDevice => "no such device (ENODEV)",
        crate::fs::FsError::NoMemory => "out of memory (ENOMEM)",
        crate::fs::FsError::Interrupted => "interrupted system call (EINTR)",
        crate::fs::FsError::WouldBlock => "resource temporarily unavailable (EAGAIN)",
        crate::fs::FsError::InternalError => "internal error (EIO)",
        crate::fs::FsError::Ok => "success",
        crate::fs::FsError::NotFile => "not a file (EBADF)",
        crate::fs::FsError::NotSupported => "operation not supported (ENOSYS)",
        crate::fs::FsError::NotEmpty => "directory not empty (ENOTEMPTY)",
    }
}

/// POSIX.1-2024 path resolution contract constants.
///
/// path_resolution(7) spec:
/// - Empty pathname → ENOENT
/// - NUL byte in path → EINVAL
/// - Pathname too long → ENAMETOOLONG (PATH_MAX = 4096)
/// - Component too long → ENAMETOOLONG (NAME_MAX = 255)
/// - Trailing slash on non-directory → ENOTDIR
/// - Symlink loop → ELOOP (MAXSYMLINKS = 40)
pub const PATH_MAX: usize = 4096;
pub const NAME_MAX: usize = 255;

/// Validate a pathname per POSIX.1-2024 path_resolution(7).
///
/// Returns Ok(()) if the path is valid, or Err(FsError) with the appropriate
/// error code. This is called before any VFS operation that takes a pathname.
pub fn validate_path(path: &str) -> Result<(), crate::fs::FsError> {
    // path_resolution(7): "POSIX decrees that an empty pathname must not be
    // resolved successfully. Linux returns ENOENT in this case."
    if path.is_empty() {
        return Err(crate::fs::FsError::NotFound);
    }

    // path_resolution(7): NUL byte terminates C strings; must not appear in path
    if path.contains('\0') {
        return Err(crate::fs::FsError::InvalidPath);
    }

    // path_resolution(7): "There is a maximum length for pathnames. If the
    // pathname is too long, an ENAMETOOLONG error is returned."
    if path.len() > PATH_MAX {
        return Err(crate::fs::FsError::NameTooLong);
    }

    // Check each component does not exceed NAME_MAX
    for component in path.split('/') {
        if component.is_empty() || component == "." || component == ".." {
            continue;
        }
        if component.len() > NAME_MAX {
            return Err(crate::fs::FsError::ComponentTooLong);
        }
    }

    Ok(())
}

/// path_resolution(7): Trailing slash on non-directory → ENOTDIR.
///
/// If `path` ends with '/' (after normalization stripping) and the resolved
/// entry `is_dir` is false, return ENOTDIR.
///
/// Call this AFTER resolving the entry, when you know whether it is a directory.
/// The caller must decide whether the trailing-slash constraint applies:
/// e.g. open(O_CREAT) skips this check because creating a file with trailing
/// slash is an error regardless.
pub fn check_trailing_slash_notdir(path: &str, is_dir: bool) -> Result<(), crate::fs::FsError> {
    if path.ends_with('/') && !is_dir {
        return Err(crate::fs::FsError::NotDirectory);
    }
    Ok(())
}

fn is_mount_root(relative_path: &str) -> bool {
    relative_path.is_empty() || relative_path == "/"
}

pub fn normalize_vfs_path(path: &str) -> String {
    let mut components: Vec<String> = Vec::new();
    for raw in path.split(['/', '\\']) {
        if raw.is_empty() || raw == "." {
            continue;
        }
        if raw == ".." {
            if !components.is_empty() {
                components.pop();
            }
            continue;
        }
        components.push(raw.to_string());
    }

    if components.is_empty() {
        return String::from("/");
    }

    let mut normalized = String::from("/");
    normalized.push_str(components[0].as_str());
    for component in components.iter().skip(1) {
        normalized.push('/');
        normalized.push_str(component.as_str());
    }
    normalized
}

fn mount_matches_path(mount_point: &str, path: &str) -> bool {
    if mount_point == "/" {
        return path.starts_with('/');
    }
    if path == mount_point {
        return true;
    }
    path.strip_prefix(mount_point)
        .is_some_and(|suffix| suffix.starts_with('/'))
}

pub(crate) fn relative_mount_path<'a>(mount_point: &str, path: &'a str) -> &'a str {
    if mount_point == "/" {
        return path;
    }
    if path == mount_point {
        return "/";
    }
    path.strip_prefix(mount_point).unwrap_or(path)
}

fn directory_info(fs_type: VfsFsType) -> VfsFileInfo {
    VfsFileInfo {
        inode: 0,
        size: 0,
        mode: 0o040755,
        nlink: 1,
        uid: 0,
        gid: 0,
        fs_type,
        block_size: 4096,
        blocks: 0,
    }
}

fn vfs_info_from_f2fs_entry(entry: &crate::fs::f2fs::F2fsEntry) -> VfsFileInfo {
    VfsFileInfo {
        inode: entry.ino,
        size: entry.size,
        mode: entry.mode,
        nlink: 1,
        uid: entry.uid,
        gid: entry.gid,
        fs_type: VfsFsType::F2fs,
        block_size: 4096,
        blocks: (entry.size + 4095) / 4096,
    }
}

fn read_f2fs_bytes_exact(
    relative_path: &str,
    entry: &crate::fs::f2fs::F2fsEntry,
) -> Result<Vec<u8>, &'static str> {
    let mut buf = vec![0u8; f2fs_exact_read_len(entry)?];
    let read_len = crate::fs::f2fs::read_f2fs_file_at(relative_path, 0, &mut buf)
        .map_err(|_| "f2fs: failed to read file")?;
    buf.truncate(read_len);
    Ok(buf)
}

fn f2fs_exact_read_len(entry: &crate::fs::f2fs::F2fsEntry) -> Result<usize, &'static str> {
    usize::try_from(entry.size).map_err(|_| "f2fs: file too large for host buffer")
}

#[derive(Clone, Copy)]
enum VfsUnsupportedCapability {
    Open,
    Read,
    Write,
    ListDirectory,
    Create,
    Unlink,
    Rename,
    Truncate,
    Symlink,
    Stat,
    CopyFileRange,
    Fadvise,
    Chmod,
    Chown,
    Fallocate,
}

fn no_filesystem_for_path() -> &'static str {
    "No filesystem mounted for path"
}

fn unsupported_vfs_capability(
    fs_type: VfsFsType,
    capability: VfsUnsupportedCapability,
) -> &'static str {
    match (fs_type, capability) {
        (VfsFsType::DevFs, VfsUnsupportedCapability::Open) => {
            "devfs: unified open requires a device-specific driver path"
        }
        (VfsFsType::DevFs, VfsUnsupportedCapability::Read) => {
            "devfs: unified reads require a device-specific driver path"
        }
        (VfsFsType::TmpFs, VfsUnsupportedCapability::Open) => {
            "tmpfs: unified tmpfs data path is not wired"
        }
        (VfsFsType::TmpFs, VfsUnsupportedCapability::Read) => {
            "tmpfs: unified tmpfs reads are not wired"
        }
        (VfsFsType::TmpFs, VfsUnsupportedCapability::ListDirectory) => {
            "tmpfs: unified tmpfs directory listing is not wired"
        }
        (VfsFsType::Xfs, VfsUnsupportedCapability::Open) => {
            "xfs: unified VFS open is not wired to a real backend"
        }
        (VfsFsType::Xfs, VfsUnsupportedCapability::Read) => {
            "xfs: unified reads are not wired to a real backend"
        }
        (VfsFsType::Xfs, VfsUnsupportedCapability::ListDirectory) => {
            "xfs: unified directory listing is not wired to a real backend"
        }
        (VfsFsType::DevFs, VfsUnsupportedCapability::Write) => {
            "devfs: unified writes require a device-specific driver path"
        }
        (VfsFsType::TmpFs, VfsUnsupportedCapability::Write) => {
            "tmpfs: unified writes are not wired"
        }
        (VfsFsType::Xfs, VfsUnsupportedCapability::Write) => {
            "xfs: unified writes are not wired to a real backend"
        }
        (VfsFsType::TmpFs, VfsUnsupportedCapability::Create) => {
            "tmpfs: unified file creation is not wired"
        }
        (fs, VfsUnsupportedCapability::Create) => {
            alloc::format!("{:?}: VFS file/dir creation is not wired", fs).leak()
        }
        (fs, VfsUnsupportedCapability::Unlink) => {
            alloc::format!("{:?}: VFS unlink is not wired", fs).leak()
        }
        (fs, VfsUnsupportedCapability::Rename) => {
            alloc::format!("{:?}: VFS rename is not wired", fs).leak()
        }
        (fs, VfsUnsupportedCapability::Truncate) => {
            alloc::format!("{:?}: VFS truncate is not wired", fs).leak()
        }
        (fs, VfsUnsupportedCapability::Symlink) => {
            alloc::format!("{:?}: VFS symlink is not wired", fs).leak()
        }
        (fs, VfsUnsupportedCapability::Stat) => {
            alloc::format!("{:?}: VFS stat is not wired", fs).leak()
        }
        (fs, VfsUnsupportedCapability::CopyFileRange) => {
            alloc::format!("{:?}: VFS copy_file_range is not wired", fs).leak()
        }
        (fs, VfsUnsupportedCapability::Fadvise) => {
            alloc::format!("{:?}: VFS fadvise is not wired", fs).leak()
        }
        (fs, VfsUnsupportedCapability::Chmod) => {
            alloc::format!("{:?}: VFS chmod is not wired", fs).leak()
        }
        (fs, VfsUnsupportedCapability::Chown) => {
            alloc::format!("{:?}: VFS chown is not wired", fs).leak()
        }
        (fs, VfsUnsupportedCapability::Fallocate) => {
            alloc::format!("{:?}: VFS fallocate is not wired", fs).leak()
        }
        _ => "vfs: unsupported capability",
    }
}

// ============================================================================
// Address Space Operations — page cache ↔ backing store bridge
// ============================================================================

/// Address space operations — bridge between VFS page cache and backing store.
///
/// Linux equivalent: `struct address_space_operations` in `<linux/fs.h>`.
/// Each filesystem backend provides an implementation that maps file pages
/// to physical block I/O.
///
/// ## echOS page cache granularity
///
/// Currently the VFS page cache (`page_cache.rs`) operates at file
/// granularity — one entry per file at `page_index = 0`. The address
/// space ops reflect this: `read_file`/`write_file` read or write the
/// entire file. Future work may introduce block-granular (4 KiB) caching;
/// the `page_index` parameter is reserved for that.
pub struct AddressSpaceOps {
    /// Read the entire file content from backing store (cache miss).
    pub read_file: fn(fs_type: VfsFsType, source: &str, relative_path: &str) -> Result<Vec<u8>, &'static str>,
    /// Write the entire file content to backing store (writeback).
    pub write_file: fn(fs_type: VfsFsType, source: &str, relative_path: &str, data: &[u8]) -> Result<(), &'static str>,
    /// Map logical page index to physical block number.
    /// page_index=0 for current file-granular cache.
    pub bmap: fn(fs_type: VfsFsType, source: &str, relative_path: &str, page_index: u64) -> Result<u64, &'static str>,
}

// ── Per-backend read_file implementations ──────────────────────────────────

fn asops_read_f2fs(_fs_type: VfsFsType, _source: &str, relative_path: &str) -> Result<Vec<u8>, &'static str> {
    let entry = crate::fs::f2fs::open_entry(relative_path).map_err(|_| "f2fs: file not found")?;
    if entry.is_dir {
        return Err("f2fs: path is a directory");
    }
    read_f2fs_bytes_exact(relative_path, &entry)
}

fn asops_read_ext4(_fs_type: VfsFsType, source: &str, relative_path: &str) -> Result<Vec<u8>, &'static str> {
    let resolved = resolve_ext4_node(source, relative_path)?;
    if resolved.inode.is_directory() {
        return Err("ext4: path is a directory");
    }
    resolved
        .mounted
        .fs
        .read_file_from_storage(&resolved.inode, &resolved.mounted.storage)
        .map_err(|_| "ext4: failed to read file")
}

fn asops_read_fat32(_fs_type: VfsFsType, source: &str, relative_path: &str) -> Result<Vec<u8>, &'static str> {
    let resolved = resolve_fat32_node(source, relative_path)?;
    if resolved.file.is_dir {
        return Err("fat32: path is a directory");
    }
    fat32_read_file(&resolved)
}

fn asops_read_exfat(_fs_type: VfsFsType, source: &str, relative_path: &str) -> Result<Vec<u8>, &'static str> {
    let resolved = resolve_exfat_node(source, relative_path)?;
    if resolved.file.is_dir {
        return Err("exfat: path is a directory");
    }
    exfat_read_file(&resolved)
}

fn asops_read_ntfs(_fs_type: VfsFsType, source: &str, relative_path: &str) -> Result<Vec<u8>, &'static str> {
    let resolved = resolve_ntfs_node(source, relative_path)?;
    if matches!(
        resolved.metadata.as_ref().map(|meta| meta.file_type),
        Some(crate::fs::ntfs::NtfsFileType::Directory)
    ) {
        return Err("ntfs: path is a directory");
    }
    resolved
        .mounted
        .fs
        .read_file_from_storage(&resolved.entry, &resolved.mounted.storage)
        .map_err(|_| "ntfs: failed to read file")
}

fn asops_read_btrfs(_fs_type: VfsFsType, source: &str, relative_path: &str) -> Result<Vec<u8>, &'static str> {
    let resolved = resolve_btrfs_node(source, relative_path)?;
    if resolved.inode.is_directory() {
        return Err("btrfs: path is a directory");
    }
    resolved
        .mounted
        .fs
        .read_file_from_storage(resolved.inode_num, &resolved.mounted.storage)
}

fn asops_read_procfs(_fs_type: VfsFsType, _source: &str, relative_path: &str) -> Result<Vec<u8>, &'static str> {
    let content = generate_proc_content(relative_path);
    if content.is_empty() {
        Err("procfs: entry not found")
    } else {
        Ok(content.into_bytes())
    }
}

fn asops_read_sysfs(_fs_type: VfsFsType, _source: &str, relative_path: &str) -> Result<Vec<u8>, &'static str> {
    read_sysfs_bytes(relative_path)
}

fn asops_read_unsupported(fs_type: VfsFsType, _source: &str, _relative_path: &str) -> Result<Vec<u8>, &'static str> {
    Err(unsupported_vfs_capability(fs_type, VfsUnsupportedCapability::Read))
}

// ── Per-backend write_file implementations ─────────────────────────────────

fn asops_write_f2fs(_fs_type: VfsFsType, _source: &str, relative_path: &str, data: &[u8]) -> Result<(), &'static str> {
    let parent = crate::fs::namei::parent_path(relative_path);
    let name = relative_path.rsplit_once('/').map(|(_, n)| n).unwrap_or("");
    if name.is_empty() {
        return Err("f2fs: empty file name");
    }
    crate::fs::f2fs::create_f2fs_file_with_data(&parent, name, data)
        .map_err(|_| "f2fs: write failed")
}

fn asops_write_ext4(_fs_type: VfsFsType, source: &str, relative_path: &str, data: &[u8]) -> Result<(), &'static str> {
    crate::fs::ext4::ext4_write_file(source, relative_path, data)
}

fn asops_write_unsupported(fs_type: VfsFsType, _source: &str, _relative_path: &str, _data: &[u8]) -> Result<(), &'static str> {
    Err(unsupported_vfs_capability(fs_type, VfsUnsupportedCapability::Write))
}

// ── Per-backend bmap implementations ───────────────────────────────────────

fn asops_bmap_f2fs(_fs_type: VfsFsType, _source: &str, _relative_path: &str, page_index: u64) -> Result<u64, &'static str> {
    if page_index == 0 {
        Err("f2fs: bmap not yet implemented")
    } else {
        Err("f2fs: block-level bmap not yet implemented")
    }
}

fn asops_bmap_unsupported(fs_type: VfsFsType, _source: &str, _relative_path: &str, _page_index: u64) -> Result<u64, &'static str> {
    Err(unsupported_vfs_capability(fs_type, VfsUnsupportedCapability::Read))
}

// ── Address space ops dispatch table ───────────────────────────────────────

fn get_address_space_ops(fs_type: VfsFsType) -> &'static AddressSpaceOps {
    match fs_type {
        VfsFsType::F2fs => &ASOPS_F2FS,
        VfsFsType::Ext4 => &ASOPS_EXT4,
        VfsFsType::Fat32 => &ASOPS_FAT32,
        VfsFsType::ExFat => &ASOPS_EXFAT,
        VfsFsType::Ntfs => &ASOPS_NTFS,
        VfsFsType::Btrfs => &ASOPS_BTRFS,
        VfsFsType::ProcFs => &ASOPS_PROCFS,
        VfsFsType::SysFs => &ASOPS_SYSFS,
        _ => &ASOPS_UNSUPPORTED,
    }
}

const ASOPS_F2FS: AddressSpaceOps = AddressSpaceOps {
    read_file: asops_read_f2fs,
    write_file: asops_write_f2fs,
    bmap: asops_bmap_f2fs,
};

const ASOPS_EXT4: AddressSpaceOps = AddressSpaceOps {
    read_file: asops_read_ext4,
    write_file: asops_write_ext4,
    bmap: asops_bmap_unsupported,
};

const ASOPS_FAT32: AddressSpaceOps = AddressSpaceOps {
    read_file: asops_read_fat32,
    write_file: asops_write_unsupported,
    bmap: asops_bmap_unsupported,
};

const ASOPS_EXFAT: AddressSpaceOps = AddressSpaceOps {
    read_file: asops_read_exfat,
    write_file: asops_write_unsupported,
    bmap: asops_bmap_unsupported,
};

const ASOPS_NTFS: AddressSpaceOps = AddressSpaceOps {
    read_file: asops_read_ntfs,
    write_file: asops_write_unsupported,
    bmap: asops_bmap_unsupported,
};

const ASOPS_BTRFS: AddressSpaceOps = AddressSpaceOps {
    read_file: asops_read_btrfs,
    write_file: asops_write_unsupported,
    bmap: asops_bmap_unsupported,
};

const ASOPS_PROCFS: AddressSpaceOps = AddressSpaceOps {
    read_file: asops_read_procfs,
    write_file: asops_write_unsupported,
    bmap: asops_bmap_unsupported,
};

const ASOPS_SYSFS: AddressSpaceOps = AddressSpaceOps {
    read_file: asops_read_sysfs,
    write_file: asops_write_unsupported,
    bmap: asops_bmap_unsupported,
};

const ASOPS_UNSUPPORTED: AddressSpaceOps = AddressSpaceOps {
    read_file: asops_read_unsupported,
    write_file: asops_write_unsupported,
    bmap: asops_bmap_unsupported,
};

fn read_sysfs_bytes(path: &str) -> Result<Vec<u8>, &'static str> {
    let inode = crate::fs::sysfs::open_sys_inode(path).map_err(|_| "sysfs: entry not found")?;
    let mut buf = alloc::vec![0u8; 4096];
    let n = inode
        .read_at(0, &mut buf)
        .map_err(|_| "sysfs: failed to read inode")?;
    buf.truncate(n);
    Ok(buf)
}

#[derive(Clone)]
struct ResolvedExt4Node {
    mounted: crate::fs::ext4::MountedExt4,
    inode_num: u32,
    inode: crate::fs::ext4::Ext4Inode,
}

/// Symlink hedefini çözer: absolute ise doğrudan kullanır,
/// relative ise parent directory'ye göre birleştirir.
/// Kalan path component'lerini ekler.
fn resolve_symlink_target(
    target: &str,
    original_path: &str,
    consumed_components: usize,
    source: &str,
    _fs_type: VfsFsType,
    depth: usize,
) -> Result<String, &'static str> {
    if depth >= crate::fs::namei::MAXSYMLINKS {
        return Err("too many symbolic links encountered (ELOOP)");
    }

    let remaining_count = path_components(original_path)
        .len()
        .saturating_sub(consumed_components);
    let remaining: Vec<String> = path_components(original_path)
        .into_iter()
        .skip(consumed_components)
        .take(remaining_count)
        .collect();

    let mut resolved = if target.starts_with('/') {
        // Absolute symlink: root'tan başla
        target.to_string()
    } else {
        // Relative symlink: parent directory'ye göre çöz
        let parent = crate::fs::namei::parent_path(original_path);
        if parent == "/" {
            format!("/{}", target)
        } else {
            format!("{}/{}", parent, target)
        }
    };

    // Kalan component'leri ekle
    for comp in remaining {
        resolved.push('/');
        resolved.push_str(&comp);
    }

    Ok(normalize_vfs_path(&resolved))
}

fn resolve_ext4_node(source: &str, relative_path: &str) -> Result<ResolvedExt4Node, &'static str> {
    resolve_ext4_node_with_depth(source, relative_path, 0)
}

fn resolve_ext4_node_with_depth(
    source: &str,
    relative_path: &str,
    depth: usize,
) -> Result<ResolvedExt4Node, &'static str> {
    if depth >= crate::fs::namei::MAXSYMLINKS {
        return Err("ext4: too many symbolic links encountered (ELOOP)");
    }

    let mounted =
        crate::fs::ext4::get_mounted_ext4(source).ok_or("ext4: backend not mounted for source")?;
    let mut inode_num = mounted.fs.root_inode;
    let mut inode = mounted
        .fs
        .root_inode_from_storage(&mounted.storage)
        .map_err(|_| "ext4: failed to load root inode")?;

    let components: Vec<String> = path_components(relative_path);
    let mut i = 0;

    while i < components.len() {
        let component = &components[i];

        if !inode.is_directory() {
            // Symlink ise takip et
            if inode.is_symlink() {
                let target = mounted
                    .fs
                    .read_symlink_from_storage(&inode, &mounted.storage)
                    .map_err(|_| "ext4: failed to read symlink target")?;
                // Symlink hedefini çöz
                let resolved = resolve_symlink_target(
                    &target,
                    relative_path,
                    i,
                    source,
                    VfsFsType::Ext4,
                    depth + 1,
                )?;
                return resolve_ext4_node_with_depth(source, &resolved, depth + 1);
            }
            return Err("ext4: parent path is not a directory");
        }

        let entries = mounted
            .fs
            .read_dir_from_storage(&inode, &mounted.storage)
            .map_err(|_| "ext4: failed to read directory")?;
        let child = entries
            .into_iter()
            .find(|entry| entry.name == *component)
            .ok_or("ext4: file not found")?;
        inode_num = child.inode;
        inode = mounted
            .fs
            .read_inode_from_storage(child.inode, &mounted.storage)
            .map_err(|_| "ext4: failed to read inode")?;
        i += 1;
    }

    // Son component de symlink ise takip et
    if inode.is_symlink() {
        let target = mounted
            .fs
            .read_symlink_from_storage(&inode, &mounted.storage)
            .map_err(|_| "ext4: failed to read symlink target")?;
        let resolved = resolve_symlink_target(
            &target,
            relative_path,
            components.len(),
            source,
            VfsFsType::Ext4,
            depth + 1,
        )?;
        return resolve_ext4_node_with_depth(source, &resolved, depth + 1);
    }

    Ok(ResolvedExt4Node {
        mounted,
        inode_num,
        inode,
    })
}

fn vfs_info_from_ext4_inode(resolved: &ResolvedExt4Node) -> VfsFileInfo {
    VfsFileInfo {
        inode: resolved.inode_num as u64,
        size: resolved.inode.size(),
        mode: resolved.inode.i_mode as u32,
        nlink: resolved.inode.i_links_count as u32,
        uid: resolved.inode.i_uid as u32,
        gid: resolved.inode.i_gid as u32,
        fs_type: VfsFsType::Ext4,
        block_size: resolved.mounted.fs.block_size,
        blocks: resolved.inode.i_blocks_lo as u64,
    }
}

fn list_ext4_dir(source: &str, relative_path: &str) -> Result<Vec<VfsDirEntry>, &'static str> {
    let resolved = resolve_ext4_node(source, relative_path)?;
    if !resolved.inode.is_directory() {
        return Err("ext4: path is not a directory");
    }
    // Linux: inode_lock_shared(dir) — shared lock for directory reads
    crate::fs::ext4::ext4_inode_lock_shared(source, resolved.inode_num);
    let entries = resolved
        .mounted
        .fs
        .read_dir_from_storage(&resolved.inode, &resolved.mounted.storage)
        .map_err(|e| {
            crate::fs::ext4::ext4_inode_unlock_shared(source, resolved.inode_num);
            e
        });
    let entries = entries.map_err(|_| "ext4: failed to read directory")?;
    let mut result = Vec::new();
    for entry in entries {
        let inode = resolved
            .mounted
            .fs
            .read_inode_from_storage(entry.inode, &resolved.mounted.storage)
            .map_err(|e| {
                crate::fs::ext4::ext4_inode_unlock_shared(source, resolved.inode_num);
                let _ = e;
                "ext4: failed to read inode"
            })?;
        result.push(VfsDirEntry {
            name: entry.name,
            size: inode.size(),
            is_directory: inode.is_directory(),
            fs_type: VfsFsType::Ext4,
        });
    }
    crate::fs::ext4::ext4_inode_unlock_shared(source, resolved.inode_num);
    Ok(result)
}

#[derive(Clone)]
struct ResolvedFat32Node {
    mounted: crate::fs::fat::MountedFat32,
    file: crate::fs::fat::Fat32File,
}

fn parse_fat32_source(source: &str) -> Result<usize, &'static str> {
    source
        .strip_prefix("fat32:")
        .unwrap_or(source)
        .parse::<usize>()
        .map_err(|_| "fat32: source must be a FAT32 mount index")
}

fn parse_exfat_source(source: &str) -> Result<usize, &'static str> {
    source
        .strip_prefix("exfat:")
        .unwrap_or(source)
        .parse::<usize>()
        .map_err(|_| "exfat: source must be an exFAT mount index")
}

fn read_fat32_table<'a>(
    fs: &crate::fs::fat::Fat32Fs,
    mounted: &crate::fs::fat::MountedFat32,
) -> Result<Vec<u8>, &'static str> {
    let offset = fs.fat_start as usize * fs.sector_size as usize;
    let len = fs.fat_size as usize * fs.sector_size as usize;
    if offset + len > mounted.storage.image_len()? {
        return Err("fat32: FAT table exceeds mounted image");
    }
    mounted.storage.read_exact(offset, len)
}

fn read_fat32_cluster(
    fs: &crate::fs::fat::Fat32Fs,
    mounted: &crate::fs::fat::MountedFat32,
    cluster: u32,
) -> Result<Vec<u8>, &'static str> {
    if cluster < 2 {
        return Err("fat32: invalid cluster number");
    }
    let offset = fs.cluster_to_sector(cluster) as usize * fs.sector_size as usize;
    let len = fs.cluster_size as usize;
    if offset + len > mounted.storage.image_len()? {
        return Err("fat32: cluster exceeds mounted image");
    }
    mounted.storage.read_exact(offset, len)
}

fn read_fat32_chain(
    mounted: &crate::fs::fat::MountedFat32,
    start_cluster: u32,
) -> Result<Vec<u8>, &'static str> {
    let fat = read_fat32_table(&mounted.fs, mounted)?;
    let mut data = Vec::new();
    let mut cluster = start_cluster;

    for _ in 0..mounted.fs.total_clusters.max(1) {
        let cluster_data = read_fat32_cluster(&mounted.fs, mounted, cluster)?;
        data.extend_from_slice(cluster_data.as_slice());
        let next = mounted.fs.read_fat_entry(fat.as_slice(), cluster);
        if mounted.fs.is_eof(next) {
            return Ok(data);
        }
        if next < 2 || next == cluster {
            return Err("fat32: corrupted cluster chain");
        }
        cluster = next;
    }

    Err("fat32: cluster chain exceeded filesystem bounds")
}

fn read_fat32_dir(
    mounted: &crate::fs::fat::MountedFat32,
    cluster: u32,
) -> Result<Vec<crate::fs::fat::Fat32File>, &'static str> {
    let dir_data = read_fat32_chain(mounted, cluster)?;
    let mut files = Vec::new();

    let entries = crate::fs::fat::parse_dir_entries_with_lfn(&dir_data);
    for (entry, long_name) in entries {
        if entry.is_empty() || entry.is_deleted() || entry.is_volume_label() {
            continue;
        }
        let mut file = crate::fs::fat::Fat32File::from_entry_with_lfn(&entry, long_name);
        // LFN varsa onu name olarak kullan
        if let Some(ref lfn) = file.long_name {
            file.name.clone_from(lfn);
        }
        files.push(file);
    }

    Ok(files)
}

fn resolve_fat32_node(
    source: &str,
    relative_path: &str,
) -> Result<ResolvedFat32Node, &'static str> {
    let index = parse_fat32_source(source)?;
    let mounted = crate::fs::fat::get_mounted_fat32(index)
        .ok_or("fat32: backend not mounted for source index")?;

    if is_mount_root(relative_path) {
        return Ok(ResolvedFat32Node {
            mounted,
            file: crate::fs::fat::Fat32File {
                name: String::from("/"),
                long_name: None,
                cluster: 0,
                size: 0,
                is_dir: true,
                attributes: 0x10,
            },
        });
    }

    let mut current_cluster = mounted.fs.root_cluster;
    let mut current_file: Option<crate::fs::fat::Fat32File> = None;

    for component in path_components(relative_path) {
        let entries = read_fat32_dir(&mounted, current_cluster)?;
        let entry = entries
            .into_iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(&component))
            .ok_or("fat32: file not found")?;
        current_cluster = entry.cluster;
        current_file = Some(entry);
    }

    current_file
        .map(|file| ResolvedFat32Node { mounted, file })
        .ok_or("fat32: file not found")
}

fn fat32_read_file(resolved: &ResolvedFat32Node) -> Result<Vec<u8>, &'static str> {
    if resolved.file.size == 0 {
        return Ok(Vec::new());
    }
    let mut data = read_fat32_chain(&resolved.mounted, resolved.file.cluster)?;
    data.truncate(resolved.file.size as usize);
    Ok(data)
}

fn list_fat32_dir(source: &str, relative_path: &str) -> Result<Vec<VfsDirEntry>, &'static str> {
    let resolved = resolve_fat32_node(source, relative_path)?;
    if !resolved.file.is_dir {
        return Err("fat32: path is not a directory");
    }
    let cluster = if resolved.file.cluster == 0 {
        resolved.mounted.fs.root_cluster
    } else {
        resolved.file.cluster
    };
    let entries = read_fat32_dir(&resolved.mounted, cluster)?;
    Ok(entries
        .into_iter()
        .map(|entry| VfsDirEntry {
            name: entry.name,
            size: entry.size as u64,
            is_directory: entry.is_dir,
            fs_type: VfsFsType::Fat32,
        })
        .collect())
}

fn vfs_info_from_fat32_file(resolved: &ResolvedFat32Node) -> VfsFileInfo {
    VfsFileInfo {
        inode: resolved.file.cluster as u64,
        size: resolved.file.size as u64,
        mode: if resolved.file.is_dir {
            0o040755
        } else {
            0o100644
        },
        nlink: 1,
        uid: 0,
        gid: 0,
        fs_type: VfsFsType::Fat32,
        block_size: resolved.mounted.fs.cluster_size,
        blocks: ((resolved.file.size as u64) + resolved.mounted.fs.cluster_size as u64 - 1)
            / resolved.mounted.fs.cluster_size as u64,
    }
}

fn fat32_capacity(source: &str) -> Result<(u64, u64, u64), &'static str> {
    let index = parse_fat32_source(source)?;
    let mounted = crate::fs::fat::get_mounted_fat32(index)
        .ok_or("fat32: backend not mounted for source index")?;
    let fat = read_fat32_table(&mounted.fs, &mounted)?;
    let mut free_clusters = 0u64;
    for cluster in 2..mounted.fs.total_clusters {
        if mounted.fs.read_fat_entry(fat.as_slice(), cluster) == 0 {
            free_clusters += 1;
        }
    }
    let total_clusters = mounted.fs.total_clusters.saturating_sub(2) as u64;
    let total = total_clusters.saturating_mul(mounted.fs.cluster_size as u64);
    let free = free_clusters.saturating_mul(mounted.fs.cluster_size as u64);
    let used = total.saturating_sub(free);
    Ok((total, used, free))
}

#[derive(Clone)]
struct ResolvedExFatNode {
    mounted: crate::fs::fat::MountedExFat,
    file: ExFatFileRecord,
}

#[derive(Clone, Debug)]
struct ExFatFileRecord {
    name: String,
    cluster: u32,
    size: u64,
    is_dir: bool,
    attributes: u16,
    no_fat_chain: bool,
}

const EXFAT_ENTRY_FILE: u8 = 0x85;
const EXFAT_ENTRY_STREAM: u8 = 0xC0;
const EXFAT_ENTRY_FILENAME: u8 = 0xC1;
const EXFAT_ENTRY_END: u8 = 0x00;
const EXFAT_ATTR_DIRECTORY: u16 = 0x0010;
const EXFAT_FLAG_NO_FAT_CHAIN: u8 = 0x02;

fn read_exfat_fat(
    fs: &crate::fs::fat::ExFatFs,
    mounted: &crate::fs::fat::MountedExFat,
) -> Result<Vec<u8>, &'static str> {
    let offset = fs.fat_offset as usize * fs.sector_size as usize;
    let len = fs.fat_length as usize * fs.sector_size as usize;
    if offset + len > mounted.storage.image_len()? {
        return Err("exfat: FAT exceeds mounted image");
    }
    mounted.storage.read_exact(offset, len)
}

fn read_exfat_cluster(
    fs: &crate::fs::fat::ExFatFs,
    mounted: &crate::fs::fat::MountedExFat,
    cluster: u32,
) -> Result<Vec<u8>, &'static str> {
    if cluster < 2 {
        return Err("exfat: invalid cluster number");
    }
    let offset = fs.cluster_to_sector(cluster) as usize * fs.sector_size as usize;
    let len = fs.cluster_size as usize;
    if offset + len > mounted.storage.image_len()? {
        return Err("exfat: cluster exceeds mounted image");
    }
    mounted.storage.read_exact(offset, len)
}

fn read_exfat_chain(
    mounted: &crate::fs::fat::MountedExFat,
    start_cluster: u32,
    size_hint: u64,
    no_fat_chain: bool,
) -> Result<Vec<u8>, &'static str> {
    if start_cluster < 2 {
        return Ok(Vec::new());
    }
    let mut data = Vec::new();
    let cluster_size = mounted.fs.cluster_size as usize;
    if no_fat_chain {
        let clusters = if size_hint == 0 {
            1
        } else {
            (size_hint as usize).div_ceil(cluster_size)
        };
        for cluster in start_cluster..start_cluster.saturating_add(clusters as u32) {
            let cluster_data = read_exfat_cluster(&mounted.fs, mounted, cluster)?;
            data.extend_from_slice(cluster_data.as_slice());
        }
        if size_hint > 0 {
            data.truncate(size_hint as usize);
        }
        return Ok(data);
    }

    let fat = read_exfat_fat(&mounted.fs, mounted)?;
    let mut cluster = start_cluster;
    for _ in 0..mounted.fs.cluster_count.max(1) {
        let cluster_data = read_exfat_cluster(&mounted.fs, mounted, cluster)?;
        data.extend_from_slice(cluster_data.as_slice());
        let next = mounted.fs.read_fat_entry(fat.as_slice(), cluster);
        if mounted.fs.is_eof(next) {
            break;
        }
        if next < 2 || next == cluster {
            return Err("exfat: corrupted cluster chain");
        }
        cluster = next;
        if size_hint > 0 && data.len() >= size_hint as usize {
            break;
        }
    }
    if size_hint > 0 {
        data.truncate(size_hint as usize);
    }
    Ok(data)
}

fn read_exfat_dir(
    mounted: &crate::fs::fat::MountedExFat,
    cluster: u32,
    size_hint: u64,
    no_fat_chain: bool,
) -> Result<Vec<ExFatFileRecord>, &'static str> {
    let dir_data = read_exfat_chain(mounted, cluster, size_hint, no_fat_chain)?;
    let mut files = Vec::new();
    let mut index = 0usize;

    while index + 32 <= dir_data.len() {
        let entry_type = dir_data[index];
        if entry_type == EXFAT_ENTRY_END {
            break;
        }
        if entry_type != EXFAT_ENTRY_FILE {
            index += 32;
            continue;
        }
        let primary: crate::fs::fat::ExFatFileAttribute =
            unsafe { core::ptr::read_unaligned(dir_data[index..].as_ptr() as *const _) };
        let secondary_count = primary.entry_count as usize;
        if secondary_count == 0 || index + 32 * (secondary_count + 1) > dir_data.len() {
            break;
        }
        let stream_offset = index + 32;
        if dir_data[stream_offset] != EXFAT_ENTRY_STREAM {
            index += 32 * (secondary_count + 1);
            continue;
        }
        let stream: crate::fs::fat::ExFatStreamExtension =
            unsafe { core::ptr::read_unaligned(dir_data[stream_offset..].as_ptr() as *const _) };
        let mut name_utf16 = Vec::new();
        for name_index in 0..secondary_count.saturating_sub(1) {
            let offset = stream_offset + 32 + name_index * 32;
            if dir_data[offset] != EXFAT_ENTRY_FILENAME {
                continue;
            }
            let name_entry: crate::fs::fat::ExFatFileName =
                unsafe { core::ptr::read_unaligned(dir_data[offset..].as_ptr() as *const _) };
            for code_unit in name_entry.name {
                if code_unit == 0 {
                    break;
                }
                name_utf16.push(code_unit);
            }
        }
        let name = String::from_utf16_lossy(&name_utf16);
        files.push(ExFatFileRecord {
            name,
            cluster: stream.first_cluster,
            size: stream.data_length,
            is_dir: (primary.attributes & EXFAT_ATTR_DIRECTORY) != 0,
            attributes: primary.attributes,
            no_fat_chain: (stream.general_secondary_flags & EXFAT_FLAG_NO_FAT_CHAIN) != 0,
        });
        index += 32 * (secondary_count + 1);
    }

    Ok(files)
}

fn resolve_exfat_node(
    source: &str,
    relative_path: &str,
) -> Result<ResolvedExFatNode, &'static str> {
    let index = parse_exfat_source(source)?;
    let mounted = crate::fs::fat::get_mounted_exfat(index)
        .ok_or("exfat: backend not mounted for source index")?;

    if is_mount_root(relative_path) {
        return Ok(ResolvedExFatNode {
            mounted,
            file: ExFatFileRecord {
                name: String::from("/"),
                cluster: 0,
                size: 0,
                is_dir: true,
                attributes: EXFAT_ATTR_DIRECTORY,
                no_fat_chain: false,
            },
        });
    }

    let mut current = ExFatFileRecord {
        name: String::from("/"),
        cluster: mounted.fs.root_cluster,
        size: 0,
        is_dir: true,
        attributes: EXFAT_ATTR_DIRECTORY,
        no_fat_chain: false,
    };
    for component in path_components(relative_path) {
        if !current.is_dir {
            return Err("exfat: parent path is not a directory");
        }
        let entries = read_exfat_dir(
            &mounted,
            current.cluster,
            current.size,
            current.no_fat_chain,
        )?;
        current = entries
            .into_iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(&component))
            .ok_or("exfat: file not found")?;
    }

    Ok(ResolvedExFatNode {
        mounted,
        file: current,
    })
}

fn exfat_read_file(resolved: &ResolvedExFatNode) -> Result<Vec<u8>, &'static str> {
    read_exfat_chain(
        &resolved.mounted,
        resolved.file.cluster,
        resolved.file.size,
        resolved.file.no_fat_chain,
    )
}

fn list_exfat_dir(source: &str, relative_path: &str) -> Result<Vec<VfsDirEntry>, &'static str> {
    let resolved = resolve_exfat_node(source, relative_path)?;
    if !resolved.file.is_dir {
        return Err("exfat: path is not a directory");
    }
    let cluster = if resolved.file.cluster == 0 {
        resolved.mounted.fs.root_cluster
    } else {
        resolved.file.cluster
    };
    let entries = read_exfat_dir(
        &resolved.mounted,
        cluster,
        resolved.file.size,
        resolved.file.no_fat_chain,
    )?;
    Ok(entries
        .into_iter()
        .map(|entry| VfsDirEntry {
            name: entry.name,
            size: entry.size,
            is_directory: entry.is_dir,
            fs_type: VfsFsType::ExFat,
        })
        .collect())
}

fn vfs_info_from_exfat_file(resolved: &ResolvedExFatNode) -> VfsFileInfo {
    VfsFileInfo {
        inode: resolved.file.cluster as u64,
        size: resolved.file.size,
        mode: if resolved.file.is_dir {
            0o040755
        } else {
            0o100644
        },
        nlink: 1,
        uid: 0,
        gid: 0,
        fs_type: VfsFsType::ExFat,
        block_size: resolved.mounted.fs.cluster_size,
        blocks: resolved
            .file
            .size
            .div_ceil(resolved.mounted.fs.cluster_size as u64),
    }
}

fn exfat_capacity(source: &str) -> Result<(u64, u64, u64), &'static str> {
    let index = parse_exfat_source(source)?;
    let mounted = crate::fs::fat::get_mounted_exfat(index)
        .ok_or("exfat: backend not mounted for source index")?;
    let total = mounted
        .fs
        .cluster_count
        .saturating_sub(2)
        .saturating_mul(mounted.fs.cluster_size) as u64;
    let percent = mounted.fs.boot_sector.percent_in_use.min(100) as u64;
    let used = total.saturating_mul(percent) / 100;
    let free = total.saturating_sub(used);
    Ok((total, used, free))
}

#[derive(Clone)]
struct ResolvedNtfsNode {
    mounted: crate::fs::ntfs::MountedNtfs,
    entry: crate::fs::ntfs::MftEntry,
    metadata: Option<crate::fs::ntfs::NtfsMetadata>,
}

fn resolve_ntfs_node(source: &str, relative_path: &str) -> Result<ResolvedNtfsNode, &'static str> {
    let mounted =
        crate::fs::ntfs::get_mounted_ntfs(source).ok_or("ntfs: backend not mounted for source")?;
    let entry = mounted
        .fs
        .resolve_path_from_storage(relative_path, &mounted.storage)
        .map_err(|_| "ntfs: file not found")?;
    let metadata = mounted.fs.get_metadata(&entry);
    Ok(ResolvedNtfsNode {
        mounted,
        entry,
        metadata,
    })
}

fn vfs_info_from_ntfs_entry(resolved: &ResolvedNtfsNode) -> VfsFileInfo {
    let metadata = resolved.metadata.clone();
    let file_type = metadata
        .as_ref()
        .map(|meta| meta.file_type)
        .unwrap_or(crate::fs::ntfs::NtfsFileType::Unknown);
    let size = metadata.as_ref().map(|meta| meta.size).unwrap_or(0);
    let mode = match file_type {
        crate::fs::ntfs::NtfsFileType::Directory => 0o040755,
        _ => 0o100644,
    };
    let block_size = resolved.mounted.fs.cluster_size as u32;
    VfsFileInfo {
        inode: resolved.entry.entry_number,
        size,
        mode,
        nlink: resolved.entry.link_count as u32,
        uid: 0,
        gid: 0,
        fs_type: VfsFsType::Ntfs,
        block_size,
        blocks: if block_size == 0 {
            0
        } else {
            size.div_ceil(block_size as u64)
        },
    }
}

fn ntfs_capacity(source: &str) -> Result<(u64, u64, u64), &'static str> {
    let mounted =
        crate::fs::ntfs::get_mounted_ntfs(source).ok_or("ntfs: backend not mounted for source")?;
    mounted
        .fs
        .bitmap_usage_from_storage(&mounted.storage)
        .map_err(|_| "ntfs: failed to read allocation bitmap")
}

fn list_ntfs_dir(source: &str, relative_path: &str) -> Result<Vec<VfsDirEntry>, &'static str> {
    let resolved = resolve_ntfs_node(source, relative_path)?;
    if !matches!(
        resolved.metadata.as_ref().map(|meta| meta.file_type),
        Some(crate::fs::ntfs::NtfsFileType::Directory)
    ) {
        return Err("ntfs: path is not a directory");
    }
    let entries = resolved
        .mounted
        .fs
        .list_directory_from_storage(resolved.entry.entry_number, &resolved.mounted.storage)
        .map_err(|_| "ntfs: failed to list directory")?;
    Ok(entries
        .into_iter()
        .map(|entry| VfsDirEntry {
            name: entry.name,
            size: entry.size,
            is_directory: matches!(entry.file_type, crate::fs::ntfs::NtfsFileType::Directory),
            fs_type: VfsFsType::Ntfs,
        })
        .collect())
}

#[derive(Clone)]
struct ResolvedBtrfsNode {
    mounted: crate::fs::btrfs::MountedBtrfs,
    inode_num: u64,
    inode: crate::fs::btrfs::BtrfsInodeItem,
}

fn resolve_btrfs_node(
    source: &str,
    relative_path: &str,
) -> Result<ResolvedBtrfsNode, &'static str> {
    let mounted = crate::fs::btrfs::get_mounted_btrfs(source)
        .ok_or("btrfs: backend not mounted for source")?;
    let inode_num = mounted
        .fs
        .resolve_path(relative_path)
        .map_err(|_| "btrfs: file not found")?;
    let inode = mounted.fs.get_inode(inode_num)?;
    Ok(ResolvedBtrfsNode {
        mounted,
        inode_num,
        inode,
    })
}

fn vfs_info_from_btrfs_inode(resolved: &ResolvedBtrfsNode) -> VfsFileInfo {
    let inode = &resolved.inode;
    let block_size = resolved.mounted.fs.superblock.sector_size;
    VfsFileInfo {
        inode: resolved.inode_num,
        size: inode.size,
        mode: inode.mode,
        nlink: inode.nlink,
        uid: inode.uid,
        gid: inode.gid,
        fs_type: VfsFsType::Btrfs,
        block_size,
        blocks: if block_size == 0 {
            0
        } else {
            inode.size.div_ceil(block_size as u64)
        },
    }
}

fn btrfs_capacity(source: &str) -> Result<(u64, u64, u64), &'static str> {
    let mounted = crate::fs::btrfs::get_mounted_btrfs(source)
        .ok_or("btrfs: backend not mounted for source")?;
    let total = mounted.fs.superblock.total_size();
    let used = mounted.fs.superblock.used_size();
    let free = mounted.fs.superblock.free_size();
    Ok((total, used, free))
}

fn list_btrfs_dir(source: &str, relative_path: &str) -> Result<Vec<VfsDirEntry>, &'static str> {
    let resolved = resolve_btrfs_node(source, relative_path)?;
    let entries = resolved.mounted.fs.list_directory(resolved.inode_num)?;
    Ok(entries
        .into_iter()
        .map(|entry| {
            let (size, is_directory) = resolved
                .mounted
                .fs
                .get_inode(entry.inode)
                .map(|inode| (inode.size, inode.is_directory()))
                .unwrap_or((0, entry.file_type == crate::fs::btrfs::BTRFS_FT_DIR));
            VfsDirEntry {
                name: entry.name,
                size,
                is_directory,
                fs_type: VfsFsType::Btrfs,
            }
        })
        .collect())
}

fn list_procfs_dir(relative_path: &str) -> Result<Vec<VfsDirEntry>, &'static str> {
    let path = relative_path.trim_matches('/');
    let names: &[(&str, bool)] = match path {
        "" => &[
            ("cpuinfo", false),
            ("meminfo", false),
            ("uptime", false),
            ("version", false),
            ("cmdline", false),
            ("filesystems", false),
            ("mounts", false),
            ("interrupts", false),
            ("stat", false),
            ("loadavg", false),
            ("driver", true),
            ("self", true),
        ],
        "driver" => &[("tier", false), ("nvme", false)],
        "self" => &[("status", false), ("maps", false), ("fd", false)],
        _ => return Err("procfs: path is not a directory"),
    };
    Ok(names
        .iter()
        .map(|(name, is_directory)| VfsDirEntry {
            name: String::from(*name),
            size: 0,
            is_directory: *is_directory,
            fs_type: VfsFsType::ProcFs,
        })
        .collect())
}

fn list_sysfs_dir(relative_path: &str) -> Result<Vec<VfsDirEntry>, &'static str> {
    let path = relative_path.trim_matches('/');
    if !path.is_empty() {
        return Err("sysfs: path is not a directory");
    }
    Ok(["version", "devices", "fs", "kernel"]
        .iter()
        .map(|name| VfsDirEntry {
            name: String::from(*name),
            size: 0,
            is_directory: false,
            fs_type: VfsFsType::SysFs,
        })
        .collect())
}

fn list_devfs_dir(relative_path: &str) -> Result<Vec<VfsDirEntry>, &'static str> {
    let path = relative_path.trim_matches('/');
    if !path.is_empty() {
        return Err("devfs: path is not a directory");
    }
    Ok(["tty", "null", "zero", "random"]
        .iter()
        .map(|name| VfsDirEntry {
            name: String::from(*name),
            size: 0,
            is_directory: false,
            fs_type: VfsFsType::DevFs,
        })
        .collect())
}

pub(crate) fn path_components(path: &str) -> Vec<String> {
    normalize_vfs_path(path)
        .trim_matches('/')
        .split('/')
        .filter(|component| !component.is_empty() && *component != ".")
        .map(|component| component.to_string())
        .collect()
}

// ============================================================================
// Procfs Content Generator Helper
// ============================================================================

/// Generate procfs content based on path
fn generate_proc_content(path: &str) -> String {
    let path = path.trim_start_matches('/');
    match path {
        "cpuinfo" => crate::fs::procfs::gen_cpuinfo(),
        "meminfo" => crate::fs::procfs::gen_meminfo(),
        "uptime" => crate::fs::procfs::gen_uptime(),
        "version" => crate::fs::procfs::gen_version(),
        "cmdline" => crate::fs::procfs::gen_cmdline(),
        "filesystems" => crate::fs::procfs::gen_filesystems(),
        "mounts" => crate::fs::procfs::gen_mounts(),
        "interrupts" => crate::fs::procfs::gen_interrupts(),
        "stat" => crate::fs::procfs::gen_stat(),
        "loadavg" => crate::fs::procfs::gen_loadavg(),
        "driver/tier" => crate::fs::procfs::gen_driver_tier(),
        "driver/nvme" => crate::fs::procfs::gen_driver_nvme(),
        "self/status" | "self\\status" => crate::fs::procfs::gen_self_status(),
        "self/maps" | "self\\maps" => crate::fs::procfs::gen_self_maps(),
        "self/fd" | "self\\fd" => crate::fs::procfs::gen_self_fd(),
        _ => String::new(),
    }
}

/// Read file content from unified VFS - dispatches to correct filesystem
///
/// Uses VFS page cache to avoid redundant disk I/O on repeated reads.
pub fn read_file(path: &str) -> Result<Vec<u8>, &'static str> {
    let normalized = normalize_vfs_path(path);
    let path_hash = hash_path(&normalized);

    // Page cache lookup
    if let Some(cached) = page_cache::find_page(path_hash, 0) {
        return Ok(cached.data);
    }

    let result = VFS_UNIFIED.lock().read_bytes(path);

    // Populate cache on success
    if let Ok(ref data) = result {
        // disk_lba=0: VFS-level cache doesn't know the physical LBA.
        // Backend-specific caches handle block-level writeback.
        page_cache::add_page(path_hash, 0, data.clone(), 0);
    }

    result
}

pub fn list_dir(path: &str) -> Result<Vec<VfsDirEntry>, &'static str> {
    VFS_UNIFIED.lock().list_dir(path)
}

/// Write bytes to a file via the unified VFS.
///
/// On success, the page cache entry for the file is invalidated so that
/// subsequent reads fetch fresh data (write-through semantics).
pub fn write_file(path: &str, data: &[u8]) -> Result<(), &'static str> {
    let normalized = normalize_vfs_path(path);
    let path_hash = hash_path(&normalized);
    let result = VFS_UNIFIED.lock().write_bytes(path, data);
    if result.is_ok() {
        page_cache::invalidate_inode(path_hash);
    }
    result
}

/// Create a regular file in a directory.
pub fn create_file(parent_path: &str, name: &str) -> Result<(), &'static str> {
    VFS_UNIFIED.lock().create_file(parent_path, name)
}

pub fn create_dir(parent_path: &str, name: &str) -> Result<(), &'static str> {
    VFS_UNIFIED.lock().create_dir(parent_path, name)
}

pub fn remove_dir(parent_path: &str, name: &str) -> Result<(), &'static str> {
    VFS_UNIFIED.lock().remove_dir(parent_path, name)
}

pub fn unlink_file(parent_path: &str, name: &str) -> Result<(), &'static str> {
    VFS_UNIFIED.lock().unlink(parent_path, name)
}

/// Rename a file or directory.
pub fn rename_file(parent_path: &str, old_name: &str, new_name: &str) -> Result<(), &'static str> {
    VFS_UNIFIED.lock().rename(parent_path, old_name, new_name)
}

/// Truncate a file to a given size.
pub fn truncate_file(path: &str, new_size: u64) -> Result<(), &'static str> {
    VFS_UNIFIED.lock().truncate(path, new_size)
}

/// Create a symbolic link.
pub fn symlink_file(parent_path: &str, name: &str, target: &str) -> Result<(), &'static str> {
    VFS_UNIFIED.lock().symlink(parent_path, name, target)
}

/// Stat a file — return VfsFileInfo.
pub fn stat_file(path: &str) -> Result<VfsFileInfo, &'static str> {
    VFS_UNIFIED.lock().stat(path)
}

#[cfg(test)]
mod tests {
    use super::{
        f2fs_exact_read_len, no_filesystem_for_path, unsupported_vfs_capability,
        vfs_info_from_f2fs_entry, VfsFsType, VfsMountFlags, VfsUnified, VfsUnsupportedCapability,
    };
    use alloc::format;
    use alloc::string::String;
    use alloc::vec;
    use alloc::vec::Vec;

    #[test]
    fn open_rejects_unwired_ext4_backend() {
        let mut vfs = VfsUnified::new();
        vfs.mount(
            "/mnt",
            VfsFsType::Ext4,
            "/dev/vda1",
            VfsMountFlags::default(),
        );

        assert_eq!(
            vfs.open("/mnt/hello.txt").unwrap_err(),
            "ext4: component not found"
        );
    }

    #[test]
    fn read_rejects_devfs_empty_success() {
        let mut vfs = VfsUnified::new();
        vfs.mount(
            "/dev",
            VfsFsType::DevFs,
            "devtmpfs",
            VfsMountFlags::default(),
        );

        assert_eq!(
            vfs.read_bytes("/dev/null"),
            Err("devfs: unified reads require a device-specific driver path")
        );
    }

    #[test]
    fn procfs_unknown_entry_is_not_reported_as_empty_file() {
        let mut vfs = VfsUnified::new();
        vfs.mount("/proc", VfsFsType::ProcFs, "proc", VfsMountFlags::default());

        assert_eq!(
            vfs.read_bytes("/proc/definitely-not-real"),
            Err("procfs: entry not found")
        );
    }

    #[test]
    fn mount_resolution_normalizes_separators_and_respects_boundaries() {
        let mut vfs = VfsUnified::new();
        vfs.mount("/", VfsFsType::TmpFs, "tmpfs", VfsMountFlags::default());
        vfs.mount(
            "/mnt",
            VfsFsType::Ext4,
            "ext4:test",
            VfsMountFlags::default(),
        );
        vfs.mount(
            "/mnt-data",
            VfsFsType::Ntfs,
            "ntfs:test",
            VfsMountFlags::default(),
        );

        let ext4 = vfs
            .resolve_fs("\\mnt\\folder\\hello.txt")
            .expect("normalized ext4 route");
        assert_eq!(ext4.mount_point, "/mnt");
        assert_eq!(ext4.fs_type, VfsFsType::Ext4);

        let ntfs = vfs.resolve_fs("/mnt-data/report.txt").expect("ntfs route");
        assert_eq!(ntfs.mount_point, "/mnt-data");
        assert_eq!(ntfs.fs_type, VfsFsType::Ntfs);

        let root = vfs.resolve_fs("/mntpoint.txt").expect("root fallback");
        assert_eq!(root.mount_point, "/");
        assert_eq!(root.fs_type, VfsFsType::TmpFs);
    }

    #[test]
    fn normalize_path_collapses_dotdot_without_escaping_root() {
        assert_eq!(
            super::normalize_vfs_path("/proc/../etc/shadow"),
            "/etc/shadow"
        );
        assert_eq!(
            super::normalize_vfs_path("/../../etc/passwd"),
            "/etc/passwd"
        );
        assert_eq!(super::normalize_vfs_path("\\mnt\\..\\tmp\\./a"), "/tmp/a");
    }

    #[test]
    fn resolve_path_rejects_mount_boundary_bypass_via_dotdot() {
        let mut vfs = VfsUnified::new();
        vfs.mount("/", VfsFsType::TmpFs, "tmpfs", VfsMountFlags::default());
        vfs.mount("/proc", VfsFsType::ProcFs, "proc", VfsMountFlags::default());

        let resolved = vfs
            .resolve_fs("/proc/../etc/passwd")
            .expect("normalized path should resolve");
        assert_eq!(resolved.mount_point, "/");
        assert_eq!(resolved.fs_type, VfsFsType::TmpFs);
    }

    #[test]
    fn follow_up_returns_parent_mount_for_mount_point() {
        let mut vfs = VfsUnified::new();
        vfs.mount("/", VfsFsType::TmpFs, "tmpfs", VfsMountFlags::default());
        vfs.mount("/proc", VfsFsType::ProcFs, "proc", VfsMountFlags::default());
        vfs.mount("/dev", VfsFsType::DevFs, "devtmpfs", VfsMountFlags::default());

        // /proc mount point → follow_up goes to root mount
        let parent = vfs.follow_up("/proc").expect("follow_up from /proc");
        assert_eq!(parent, "/");

        // /dev mount point → follow_up goes to root mount
        let parent = vfs.follow_up("/dev").expect("follow_up from /dev");
        assert_eq!(parent, "/");

        // Root mount → follow_up returns None (cannot go up from root)
        assert!(vfs.follow_up("/").is_none());

        // Non-mount path → follow_up returns None
        assert!(vfs.follow_up("/etc/passwd").is_none());
    }

    #[test]
    fn fat32_mounted_backend_supports_open_and_read() {
        let image = fat32_test_image();
        let index = crate::fs::fat::mount_fat32(&image).expect("fat32 mount");
        let mut vfs = VfsUnified::new();
        vfs.mount(
            "/fat",
            VfsFsType::Fat32,
            &format!("fat32:{}", index),
            VfsMountFlags::default(),
        );

        let info = vfs.open("/fat/hello.txt").expect("fat32 open");
        assert_eq!(info.size, 5);
        assert_eq!(
            vfs.read_bytes("/fat/hello.txt").expect("fat32 read"),
            b"hello"
        );
        let entries = vfs.list_dir("/fat").expect("fat32 list");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "HELLO.TXT");
    }

    #[test]
    fn ntfs_mounted_backend_supports_open_and_read() {
        let image = ntfs_test_image();
        crate::fs::ntfs::mount_ntfs("ntfs:test", &image).expect("ntfs mount");
        let mut vfs = VfsUnified::new();
        vfs.mount(
            "/ntfs",
            VfsFsType::Ntfs,
            "ntfs:test",
            VfsMountFlags::default(),
        );

        let info = vfs.open("/ntfs/hello.txt").expect("ntfs open");
        assert_eq!(info.inode, 8);
        assert_eq!(info.size, 5);
        assert_eq!(
            vfs.read_bytes("/ntfs/hello.txt").expect("ntfs read"),
            b"hello"
        );
        let entries = vfs.list_dir("/ntfs").expect("ntfs list");
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().any(|entry| entry.name == "$Bitmap"));
        assert!(entries.iter().any(|entry| entry.name == "hello.txt"));
    }

    #[test]
    fn btrfs_mounted_backend_supports_open_read_list_and_df() {
        let image = btrfs_test_image();
        crate::fs::btrfs::mount_named_from_data("btrfs:test", &image, "/btrfs")
            .expect("btrfs mount");
        let mut vfs = VfsUnified::new();
        vfs.mount(
            "/btrfs",
            VfsFsType::Btrfs,
            "btrfs:test",
            VfsMountFlags::default(),
        );

        let info = vfs.open("/btrfs/hello.txt").expect("btrfs open");
        assert_eq!(info.inode, 257);
        assert_eq!(info.size, 5);
        assert_eq!(
            vfs.read_bytes("/btrfs/hello.txt").expect("btrfs read"),
            b"hello"
        );
        assert_eq!(
            vfs.read_bytes("/btrfs/inline.txt")
                .expect("btrfs inline read"),
            b"inline"
        );

        let entries = vfs.list_dir("/btrfs").expect("btrfs list");
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().any(|entry| entry.name == "hello.txt"));
        assert!(entries.iter().any(|entry| entry.name == "inline.txt"));

        let summary = vfs.df_summary();
        let (_, fs_type, total, used, free) = summary
            .into_iter()
            .find(|(mount_point, _, _, _, _)| mount_point == "/btrfs")
            .expect("btrfs df row");
        assert_eq!(fs_type, VfsFsType::Btrfs);
        assert_eq!(total, 0x40000);
        assert_eq!(used + free, total);
    }

    #[test]
    fn btrfs_mount_rejects_multi_device_image() {
        let image = btrfs_test_image_with_params(2, 0);
        let err = crate::fs::btrfs::mount_named_from_data("btrfs:multi", &image, "/btrfs")
            .expect_err("multi-device mount must fail closed");
        assert_eq!(err, "btrfs: multi-device volumes are not supported");
    }

    #[test]
    fn btrfs_mount_rejects_compressed_extent_image() {
        let image = btrfs_test_image_with_params(1, 1);
        let err = crate::fs::btrfs::mount_named_from_data("btrfs:compressed", &image, "/btrfs")
            .expect_err("compressed extent mount must fail closed");
        assert_eq!(err, "btrfs: compressed extents are not supported");
    }

    #[test]
    fn xfs_unwired_capabilities_share_one_contract_surface() {
        let mut vfs = VfsUnified::new();
        vfs.mount("/xfs", VfsFsType::Xfs, "xfs:test", VfsMountFlags::default());

        assert_eq!(
            vfs.open("/xfs/file.txt").unwrap_err(),
            unsupported_vfs_capability(VfsFsType::Xfs, VfsUnsupportedCapability::Open)
        );
        assert_eq!(
            vfs.read_bytes("/xfs/file.txt").unwrap_err(),
            unsupported_vfs_capability(VfsFsType::Xfs, VfsUnsupportedCapability::Read)
        );
        assert_eq!(
            vfs.list_dir("/xfs").unwrap_err(),
            unsupported_vfs_capability(VfsFsType::Xfs, VfsUnsupportedCapability::ListDirectory)
        );

        let (_, _, total, used, free) = vfs
            .df_summary()
            .into_iter()
            .find(|(mount_point, _, _, _, _)| mount_point == "/xfs")
            .expect("xfs df row");
        assert_eq!((total, used, free), (0, 0, 0));
    }

    #[test]
    fn missing_mount_contract_is_shared_across_open_read_and_list() {
        let vfs = VfsUnified::new();
        assert_eq!(
            vfs.open("/missing/file.txt").unwrap_err(),
            no_filesystem_for_path()
        );
        assert_eq!(
            vfs.read_bytes("/missing/file.txt"),
            Err(no_filesystem_for_path())
        );
        assert_eq!(
            vfs.list_dir("/missing").unwrap_err(),
            no_filesystem_for_path()
        );
    }

    #[test]
    fn f2fs_vfs_info_preserves_real_inode_identity() {
        let info = vfs_info_from_f2fs_entry(&crate::fs::f2fs::F2fsEntry {
            ino: 77,
            name: String::from("/demo.txt"),
            size: 8193,
            is_dir: false,
            is_symlink: false,
            mode: 0o600,
            uid: 1000,
            gid: 1001,
        });
        assert_eq!(info.inode, 77);
        assert_eq!(info.size, 8193);
        assert_eq!(info.blocks, 3);
        assert_eq!(info.uid, 1000);
        assert_eq!(info.gid, 1001);
    }

    #[test]
    fn f2fs_exact_read_len_tracks_full_file_size() {
        let len = f2fs_exact_read_len(&crate::fs::f2fs::F2fsEntry {
            ino: 88,
            name: String::from("/large.bin"),
            size: 8193,
            is_dir: false,
            is_symlink: false,
            mode: 0o644,
            uid: 0,
            gid: 0,
        })
        .expect("f2fs read len");
        assert_eq!(len, 8193);
    }

    #[test]
    fn exfat_mounted_backend_supports_open_and_read() {
        let image = exfat_test_image();
        let index = crate::fs::fat::mount_exfat(&image).expect("exfat mount");
        let mut vfs = VfsUnified::new();
        vfs.mount(
            "/exfat",
            VfsFsType::ExFat,
            &format!("exfat:{}", index),
            VfsMountFlags::default(),
        );

        let info = vfs.open("/exfat/HELLO.TXT").expect("exfat open");
        assert_eq!(info.size, 5);
        assert_eq!(
            vfs.read_bytes("/exfat/HELLO.TXT").expect("exfat read"),
            b"hello"
        );
        let entries = vfs.list_dir("/exfat").expect("exfat list");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "HELLO.TXT");
    }

    fn fat32_test_image() -> Vec<u8> {
        let mut image = vec![0u8; 4 * 512];

        image[11..13].copy_from_slice(&512u16.to_le_bytes());
        image[13] = 1;
        image[14..16].copy_from_slice(&1u16.to_le_bytes());
        image[16] = 1;
        image[32..36].copy_from_slice(&4u32.to_le_bytes());
        image[36..40].copy_from_slice(&1u32.to_le_bytes());
        image[44..48].copy_from_slice(&2u32.to_le_bytes());
        image[510..512].copy_from_slice(&0xAA55u16.to_le_bytes());

        let fat = 512;
        image[fat + 0..fat + 4].copy_from_slice(&0x0FFFFFF8u32.to_le_bytes());
        image[fat + 4..fat + 8].copy_from_slice(&0x0FFFFFFFu32.to_le_bytes());
        image[fat + 8..fat + 12].copy_from_slice(&0x0FFFFFFFu32.to_le_bytes());
        image[fat + 12..fat + 16].copy_from_slice(&0x0FFFFFFFu32.to_le_bytes());

        let root = 2 * 512;
        image[root..root + 11].copy_from_slice(b"HELLO   TXT");
        image[root + 11] = 0x20;
        image[root + 26..root + 28].copy_from_slice(&3u16.to_le_bytes());
        image[root + 28..root + 32].copy_from_slice(&5u32.to_le_bytes());

        let file = 3 * 512;
        image[file..file + 5].copy_from_slice(b"hello");

        image
    }

    fn ntfs_test_image() -> Vec<u8> {
        let mut image = vec![0u8; 12 * 1024];

        image[3..11].copy_from_slice(b"NTFS    ");
        image[11..13].copy_from_slice(&512u16.to_le_bytes());
        image[13] = 1;
        image[40..48].copy_from_slice(&24u64.to_le_bytes());
        image[48..56].copy_from_slice(&1u64.to_le_bytes());
        image[56..64].copy_from_slice(&2u64.to_le_bytes());
        image[64] = (-10i8) as u8;
        image[68] = 1;
        image[72..80].copy_from_slice(&0x1122334455667788u64.to_le_bytes());

        let mft_base = 512usize;
        write_test_mft_entry(
            &mut image[mft_base + 5 * 1024..mft_base + 6 * 1024],
            5,
            5,
            "",
            None,
            true,
        );
        write_bitmap_entry(&mut image[mft_base + 6 * 1024..mft_base + 7 * 1024], 6);
        write_test_mft_entry(
            &mut image[mft_base + 8 * 1024..mft_base + 9 * 1024],
            8,
            5,
            "hello.txt",
            Some(b"hello"),
            false,
        );

        image
    }

    fn exfat_test_image() -> Vec<u8> {
        let mut image = vec![0u8; 4 * 512];
        image[3..11].copy_from_slice(b"EXFAT   ");
        image[80..84].copy_from_slice(&1u32.to_le_bytes()); // fat offset
        image[84..88].copy_from_slice(&1u32.to_le_bytes()); // fat length
        image[88..92].copy_from_slice(&2u32.to_le_bytes()); // cluster heap offset
        image[92..96].copy_from_slice(&4u32.to_le_bytes()); // cluster count
        image[96..100].copy_from_slice(&2u32.to_le_bytes()); // root cluster
        image[104..106].copy_from_slice(&0x0100u16.to_le_bytes()); // revision 1.0
        image[108] = 9; // 512-byte sectors
        image[109] = 0; // 1 sector per cluster
        image[110] = 1; // one FAT
        image[510..512].copy_from_slice(&0xAA55u16.to_le_bytes());

        let fat = 512;
        image[fat + 8..fat + 12].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes()); // cluster 2 eof
        image[fat + 12..fat + 16].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes()); // cluster 3 eof

        let root = 2 * 512;
        image[root] = 0x85;
        image[root + 1] = 2;
        image[root + 4..root + 6].copy_from_slice(&0x20u16.to_le_bytes());
        image[root + 32] = 0xC0;
        image[root + 33] = 0;
        image[root + 35] = 9;
        image[root + 52..root + 56].copy_from_slice(&3u32.to_le_bytes());
        image[root + 56..root + 64].copy_from_slice(&5u64.to_le_bytes());
        image[root + 64] = 0xC1;
        let name = "HELLO.TXT".encode_utf16().collect::<Vec<_>>();
        for (index, code_unit) in name.iter().enumerate() {
            let offset = root + 66 + index * 2;
            image[offset..offset + 2].copy_from_slice(&code_unit.to_le_bytes());
        }

        let file = 3 * 512;
        image[file..file + 5].copy_from_slice(b"hello");
        image
    }

    fn btrfs_test_image() -> Vec<u8> {
        btrfs_test_image_with_params(1, 0)
    }

    fn btrfs_test_image_with_params(num_devices: u64, regular_extent_compression: u8) -> Vec<u8> {
        const IMAGE_LEN: usize = 0x40000;
        const ROOT_TREE_LOGICAL: u64 = 0x20000;
        const CHUNK_TREE_LOGICAL: u64 = 0x21000;
        const FS_TREE_LOGICAL: u64 = 0x22000;
        const DATA_LOGICAL: u64 = 0x23000;
        const NODE_SIZE: usize = 4096;

        let fsid = *b"btrfs-test-fsid!";
        let mut image = vec![0u8; IMAGE_LEN];

        let chunk_key = crate::fs::btrfs::BtrfsKey {
            objectid: crate::fs::btrfs::BTRFS_FIRST_CHUNK_TREE_OBJECTID,
            item_type: crate::fs::btrfs::BTRFS_CHUNK_ITEM_KEY,
            offset: 0,
        };
        let chunk_item = chunk_item_bytes(IMAGE_LEN as u64, fsid);
        let root_item = root_item_bytes(FS_TREE_LOGICAL, 256);

        let root_tree = build_btrfs_leaf(
            crate::fs::btrfs::BTRFS_ROOT_TREE_OBJECTID,
            ROOT_TREE_LOGICAL,
            fsid,
            vec![(
                crate::fs::btrfs::BtrfsKey {
                    objectid: crate::fs::btrfs::BTRFS_FS_TREE_OBJECTID,
                    item_type: crate::fs::btrfs::BTRFS_ROOT_ITEM_KEY,
                    offset: 1,
                },
                root_item,
            )],
        );
        let chunk_tree = build_btrfs_leaf(
            crate::fs::btrfs::BTRFS_CHUNK_TREE_OBJECTID,
            CHUNK_TREE_LOGICAL,
            fsid,
            vec![(chunk_key, chunk_item.clone())],
        );
        let fs_tree = build_btrfs_leaf(
            crate::fs::btrfs::BTRFS_FS_TREE_OBJECTID,
            FS_TREE_LOGICAL,
            fsid,
            vec![
                (
                    crate::fs::btrfs::BtrfsKey {
                        objectid: 256,
                        item_type: crate::fs::btrfs::BTRFS_INODE_ITEM_KEY,
                        offset: 0,
                    },
                    inode_item_bytes(0o040755, 0, 1),
                ),
                (
                    crate::fs::btrfs::BtrfsKey {
                        objectid: 256,
                        item_type: crate::fs::btrfs::BTRFS_DIR_INDEX_KEY,
                        offset: 2,
                    },
                    dir_item_bytes(257, "hello.txt", crate::fs::btrfs::BTRFS_FT_REG_FILE),
                ),
                (
                    crate::fs::btrfs::BtrfsKey {
                        objectid: 256,
                        item_type: crate::fs::btrfs::BTRFS_DIR_INDEX_KEY,
                        offset: 3,
                    },
                    dir_item_bytes(258, "inline.txt", crate::fs::btrfs::BTRFS_FT_REG_FILE),
                ),
                (
                    crate::fs::btrfs::BtrfsKey {
                        objectid: 257,
                        item_type: crate::fs::btrfs::BTRFS_INODE_ITEM_KEY,
                        offset: 0,
                    },
                    inode_item_bytes(0o100644, 5, 1),
                ),
                (
                    crate::fs::btrfs::BtrfsKey {
                        objectid: 257,
                        item_type: crate::fs::btrfs::BTRFS_EXTENT_DATA_KEY,
                        offset: 0,
                    },
                    regular_extent_bytes(
                        DATA_LOGICAL,
                        NODE_SIZE as u64,
                        5,
                        regular_extent_compression,
                    ),
                ),
                (
                    crate::fs::btrfs::BtrfsKey {
                        objectid: 258,
                        item_type: crate::fs::btrfs::BTRFS_INODE_ITEM_KEY,
                        offset: 0,
                    },
                    inode_item_bytes(0o100644, 6, 1),
                ),
                (
                    crate::fs::btrfs::BtrfsKey {
                        objectid: 258,
                        item_type: crate::fs::btrfs::BTRFS_EXTENT_DATA_KEY,
                        offset: 0,
                    },
                    inline_extent_bytes(b"inline"),
                ),
            ],
        );

        image[ROOT_TREE_LOGICAL as usize..ROOT_TREE_LOGICAL as usize + NODE_SIZE]
            .copy_from_slice(&root_tree);
        image[CHUNK_TREE_LOGICAL as usize..CHUNK_TREE_LOGICAL as usize + NODE_SIZE]
            .copy_from_slice(&chunk_tree);
        image[FS_TREE_LOGICAL as usize..FS_TREE_LOGICAL as usize + NODE_SIZE]
            .copy_from_slice(&fs_tree);
        image[DATA_LOGICAL as usize..DATA_LOGICAL as usize + 5].copy_from_slice(b"hello");

        let superblock = &mut image[crate::fs::btrfs::BTRFS_SUPER_OFFSET
            ..crate::fs::btrfs::BTRFS_SUPER_OFFSET + NODE_SIZE];
        superblock[32..48].copy_from_slice(&fsid);
        superblock[48..56]
            .copy_from_slice(&(crate::fs::btrfs::BTRFS_SUPER_OFFSET as u64).to_le_bytes());
        superblock[64..72].copy_from_slice(&crate::fs::btrfs::BTRFS_MAGIC.to_le_bytes());
        superblock[72..80].copy_from_slice(&1u64.to_le_bytes());
        superblock[80..88].copy_from_slice(&ROOT_TREE_LOGICAL.to_le_bytes());
        superblock[88..96].copy_from_slice(&CHUNK_TREE_LOGICAL.to_le_bytes());
        superblock[112..120].copy_from_slice(&(IMAGE_LEN as u64).to_le_bytes());
        superblock[120..128].copy_from_slice(&(5 * NODE_SIZE as u64).to_le_bytes());
        superblock[128..136].copy_from_slice(&6u64.to_le_bytes());
        superblock[136..144].copy_from_slice(&num_devices.to_le_bytes());
        superblock[144..148].copy_from_slice(&(NODE_SIZE as u32).to_le_bytes());
        superblock[148..152].copy_from_slice(&(NODE_SIZE as u32).to_le_bytes());
        superblock[152..156].copy_from_slice(&(NODE_SIZE as u32).to_le_bytes());
        superblock[156..160].copy_from_slice(&(NODE_SIZE as u32).to_le_bytes());
        superblock[160..164].copy_from_slice(&(17u32 + chunk_item.len() as u32).to_le_bytes());
        superblock[164..172].copy_from_slice(&1u64.to_le_bytes());
        superblock[196..198]
            .copy_from_slice(&crate::fs::btrfs::BTRFS_CSUM_TYPE_SHA256.to_le_bytes());
        superblock[198] = 0;
        superblock[199] = 0;
        superblock[200] = 0;
        superblock[299..309].copy_from_slice(b"btrfs-test");
        let sys_chunk = build_sys_chunk_array(IMAGE_LEN as u64, fsid);
        let sys_chunk_start = 811usize;
        superblock[sys_chunk_start..sys_chunk_start + sys_chunk.len()].copy_from_slice(&sys_chunk);
        crate::fs::btrfs::stamp_superblock_checksum(superblock).expect("superblock checksum");

        image
    }

    fn build_sys_chunk_array(image_len: u64, fsid: [u8; 16]) -> Vec<u8> {
        let chunk_item = chunk_item_bytes(image_len, fsid);
        let mut data = Vec::new();
        write_btrfs_key(
            &mut data,
            crate::fs::btrfs::BtrfsKey {
                objectid: crate::fs::btrfs::BTRFS_FIRST_CHUNK_TREE_OBJECTID,
                item_type: crate::fs::btrfs::BTRFS_CHUNK_ITEM_KEY,
                offset: 0,
            },
        );
        data.extend_from_slice(&chunk_item);
        data
    }

    fn build_btrfs_leaf(
        owner: u64,
        bytenr: u64,
        fsid: [u8; 16],
        items: Vec<(crate::fs::btrfs::BtrfsKey, Vec<u8>)>,
    ) -> Vec<u8> {
        let mut block = vec![0u8; 4096];
        block[32..48].copy_from_slice(&fsid);
        block[48..56].copy_from_slice(&bytenr.to_le_bytes());
        block[80..88].copy_from_slice(&1u64.to_le_bytes());
        block[88..96].copy_from_slice(&owner.to_le_bytes());
        block[96..100].copy_from_slice(&(items.len() as u32).to_le_bytes());
        block[100] = 0;

        let mut data_cursor = block.len();
        for (index, (key, payload)) in items.into_iter().enumerate() {
            data_cursor -= payload.len();
            block[data_cursor..data_cursor + payload.len()].copy_from_slice(&payload);
            let slot = 101 + index * 25;
            write_btrfs_key_into(&mut block[slot..slot + 17], key);
            block[slot + 17..slot + 21].copy_from_slice(&(data_cursor as u32).to_le_bytes());
            block[slot + 21..slot + 25].copy_from_slice(&(payload.len() as u32).to_le_bytes());
        }

        block
    }

    fn chunk_item_bytes(length: u64, fsid: [u8; 16]) -> Vec<u8> {
        let mut data = vec![0u8; 80];
        data[0..8].copy_from_slice(&length.to_le_bytes());
        data[8..16].copy_from_slice(&crate::fs::btrfs::BTRFS_CHUNK_TREE_OBJECTID.to_le_bytes());
        data[16..24].copy_from_slice(&4096u64.to_le_bytes());
        data[24..32].copy_from_slice(&1u64.to_le_bytes());
        data[32..36].copy_from_slice(&4096u32.to_le_bytes());
        data[36..40].copy_from_slice(&4096u32.to_le_bytes());
        data[40..44].copy_from_slice(&4096u32.to_le_bytes());
        data[44..46].copy_from_slice(&1u16.to_le_bytes());
        data[46..48].copy_from_slice(&0u16.to_le_bytes());
        data[48..56].copy_from_slice(&1u64.to_le_bytes());
        data[56..64].copy_from_slice(&0u64.to_le_bytes());
        data[64..80].copy_from_slice(&fsid);
        data
    }

    fn inode_item_bytes(mode: u32, size: u64, nlink: u32) -> Vec<u8> {
        let mut data = vec![0u8; 160];
        data[0..8].copy_from_slice(&1u64.to_le_bytes());
        data[8..16].copy_from_slice(&1u64.to_le_bytes());
        data[16..24].copy_from_slice(&size.to_le_bytes());
        data[24..32].copy_from_slice(&size.to_le_bytes());
        data[40..44].copy_from_slice(&nlink.to_le_bytes());
        data[52..56].copy_from_slice(&mode.to_le_bytes());
        data[72..80].copy_from_slice(&1u64.to_le_bytes());
        data
    }

    fn root_item_bytes(fs_tree_bytenr: u64, root_dirid: u64) -> Vec<u8> {
        let mut data = vec![0u8; 239];
        let inode = inode_item_bytes(0o040755, 0, 1);
        data[..inode.len()].copy_from_slice(&inode);
        data[160..168].copy_from_slice(&1u64.to_le_bytes());
        data[168..176].copy_from_slice(&root_dirid.to_le_bytes());
        data[176..184].copy_from_slice(&fs_tree_bytenr.to_le_bytes());
        data[184..192].copy_from_slice(&0u64.to_le_bytes());
        data[192..200].copy_from_slice(&(4096u64).to_le_bytes());
        data[200..208].copy_from_slice(&0u64.to_le_bytes());
        data[208..216].copy_from_slice(&0u64.to_le_bytes());
        data[216..220].copy_from_slice(&1u32.to_le_bytes());
        data[238] = 0;
        data
    }

    fn dir_item_bytes(inode: u64, name: &str, file_type: u8) -> Vec<u8> {
        let name_bytes = name.as_bytes();
        let mut data = vec![0u8; 30 + name_bytes.len()];
        write_btrfs_key_into(
            &mut data[..17],
            crate::fs::btrfs::BtrfsKey {
                objectid: inode,
                item_type: crate::fs::btrfs::BTRFS_INODE_ITEM_KEY,
                offset: 0,
            },
        );
        data[17..25].copy_from_slice(&1u64.to_le_bytes());
        data[25..27].copy_from_slice(&0u16.to_le_bytes());
        data[27..29].copy_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        data[29] = file_type;
        data[30..30 + name_bytes.len()].copy_from_slice(name_bytes);
        data
    }

    fn regular_extent_bytes(
        logical: u64,
        disk_num_bytes: u64,
        num_bytes: u64,
        compression: u8,
    ) -> Vec<u8> {
        let mut data = vec![0u8; 53];
        data[0..8].copy_from_slice(&1u64.to_le_bytes());
        data[8..16].copy_from_slice(&num_bytes.to_le_bytes());
        data[16] = compression;
        data[17] = 0;
        data[18] = 0;
        data[20] = 1;
        data[21..29].copy_from_slice(&logical.to_le_bytes());
        data[29..37].copy_from_slice(&disk_num_bytes.to_le_bytes());
        data[37..45].copy_from_slice(&0u64.to_le_bytes());
        data[45..53].copy_from_slice(&num_bytes.to_le_bytes());
        data
    }

    fn inline_extent_bytes(payload: &[u8]) -> Vec<u8> {
        let mut data = vec![0u8; 21 + payload.len()];
        data[0..8].copy_from_slice(&1u64.to_le_bytes());
        data[8..16].copy_from_slice(&(payload.len() as u64).to_le_bytes());
        data[16] = 0;
        data[17] = 0;
        data[18] = 0;
        data[20] = 0;
        data[21..21 + payload.len()].copy_from_slice(payload);
        data
    }

    fn write_btrfs_key(buffer: &mut Vec<u8>, key: crate::fs::btrfs::BtrfsKey) {
        let mut encoded = [0u8; 17];
        write_btrfs_key_into(&mut encoded, key);
        buffer.extend_from_slice(&encoded);
    }

    fn write_btrfs_key_into(buffer: &mut [u8], key: crate::fs::btrfs::BtrfsKey) {
        buffer[0..8].copy_from_slice(&key.objectid.to_le_bytes());
        buffer[8] = key.item_type;
        buffer[9..17].copy_from_slice(&key.offset.to_le_bytes());
    }

    fn write_test_mft_entry(
        entry: &mut [u8],
        entry_number: u64,
        parent: u64,
        name: &str,
        data: Option<&[u8]>,
        is_dir: bool,
    ) {
        entry[..4].copy_from_slice(b"FILE");
        entry[16..18].copy_from_slice(&1u16.to_le_bytes());
        entry[18..20].copy_from_slice(&1u16.to_le_bytes());
        entry[20..22].copy_from_slice(&56u16.to_le_bytes());

        let name_utf16: Vec<u16> = name.encode_utf16().collect();
        let mut filename_payload = vec![0u8; 66 + name_utf16.len() * 2];
        filename_payload[0..8].copy_from_slice(&parent.to_le_bytes());
        filename_payload[56..64]
            .copy_from_slice(&(data.map(|bytes| bytes.len()).unwrap_or(0) as u64).to_le_bytes());
        let flags = if is_dir { 0x10000000u32 } else { 0x20u32 };
        filename_payload[52..56].copy_from_slice(&flags.to_le_bytes());
        filename_payload[64] = name_utf16.len() as u8;
        filename_payload[65] = 1;
        for (index, code_unit) in name_utf16.iter().enumerate() {
            let offset = 66 + index * 2;
            filename_payload[offset..offset + 2].copy_from_slice(&code_unit.to_le_bytes());
        }

        let mut offset = 56usize;
        offset += write_resident_attr(entry, offset, 0x30, &filename_payload);
        if let Some(bytes) = data {
            offset += write_resident_attr(entry, offset, 0x80, bytes);
        }
        entry[offset..offset + 4].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        entry[24..28].copy_from_slice(&((offset + 8) as u32).to_le_bytes());
        entry[44..48].copy_from_slice(&(entry_number as u32).to_le_bytes());
    }

    fn write_bitmap_entry(entry: &mut [u8], entry_number: u64) {
        let bitmap = [0b0111_1111u8];
        write_test_mft_entry(entry, entry_number, 5, "$Bitmap", Some(&bitmap), false);
    }

    fn write_resident_attr(
        entry: &mut [u8],
        offset: usize,
        attr_type: u32,
        payload: &[u8],
    ) -> usize {
        let total_length = 24 + payload.len();
        entry[offset..offset + 4].copy_from_slice(&attr_type.to_le_bytes());
        entry[offset + 4..offset + 8].copy_from_slice(&(total_length as u32).to_le_bytes());
        entry[offset + 8] = 0;
        entry[offset + 9] = 0;
        entry[offset + 10..offset + 12].copy_from_slice(&0u16.to_le_bytes());
        entry[offset + 12..offset + 14].copy_from_slice(&0u16.to_le_bytes());
        entry[offset + 14..offset + 16].copy_from_slice(&0u16.to_le_bytes());
        entry[offset + 16..offset + 20].copy_from_slice(&(payload.len() as u32).to_le_bytes());
        entry[offset + 20..offset + 22].copy_from_slice(&24u16.to_le_bytes());
        entry[offset + 22..offset + 24].copy_from_slice(&0u16.to_le_bytes());
        entry[offset + 24..offset + 24 + payload.len()].copy_from_slice(payload);
        total_length
    }
}

/// Sync all mounted writable filesystems.
///
/// VFS-level sync: flushes all cached data to stable storage.
///
/// Per §5.3 contract:
/// - VFS page cache dirty flags are cleared (sync_cache)
/// - F2FS internal buffers are flushed (sync_f2fs)
/// - Other backends (ext4, btrfs, fat32, ntfs) manage their own writeback
///   internally; no additional VFS-level flush is needed for those.
pub fn vfs_sync_all() -> Result<(), crate::fs::FsError> {
    // 0. VFS page cache — clear dirty flags
    page_cache::sync_cache();

    // 1. F2FS — primary backend (checkpoint + flush)
    let _ = crate::fs::f2fs::sync_f2fs();

    Ok(())
}
