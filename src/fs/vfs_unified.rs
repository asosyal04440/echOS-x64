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
use alloc::vec;
use alloc::vec::Vec;
use spin::Mutex;

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
// VFS Unified Manager
// ============================================================================

/// Birleşik VFS yöneticisi
pub struct VfsUnified {
    /// Mount tablosu (mount_point → entry)
    mount_table: BTreeMap<String, VfsMountEntry>,
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
    ) {
        let mount_point = normalize_vfs_path(mount_point);
        let entry = VfsMountEntry {
            mount_point: mount_point.clone(),
            fs_type,
            source: String::from(source),
            flags,
            readonly: false,
        };

        crate::serial_println!(
            "[VFS] mount: {} -> {} (type={})",
            source,
            mount_point,
            fs_type.as_str()
        );

        self.mount_table.insert(mount_point, entry);
        self.fs_count += 1;
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

    /// Birleşik open — path'e göre doğru dosya sistemine yönlendirir
    pub fn open(&self, path: &str) -> Result<VfsFileInfo, &'static str> {
        let normalized_path = normalize_vfs_path(path);
        let entry = self
            .resolve_fs(normalized_path.as_str())
            .ok_or_else(no_filesystem_for_path)?;
        let relative_path =
            relative_mount_path(entry.mount_point.as_str(), normalized_path.as_str());

        match entry.fs_type {
            VfsFsType::ProcFs => {
                if is_mount_root(relative_path) {
                    return Ok(directory_info(VfsFsType::ProcFs));
                }
                // procfs read dispatch
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
            VfsFsType::DevFs => {
                if is_mount_root(relative_path) {
                    Ok(directory_info(VfsFsType::DevFs))
                } else {
                    Err(unsupported_vfs_capability(
                        VfsFsType::DevFs,
                        VfsUnsupportedCapability::Open,
                    ))
                }
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
            VfsFsType::TmpFs => {
                if is_mount_root(relative_path) {
                    Ok(directory_info(VfsFsType::TmpFs))
                } else {
                    Err(unsupported_vfs_capability(
                        VfsFsType::TmpFs,
                        VfsUnsupportedCapability::Open,
                    ))
                }
            }
            VfsFsType::Ext4 => {
                let resolved = resolve_ext4_node(&entry.source, relative_path)?;
                Ok(vfs_info_from_ext4_inode(&resolved))
            }
            VfsFsType::F2fs => {
                // F2FS dispatch - primary filesystem
                let f2fs_entry = crate::fs::f2fs::open_entry(relative_path)
                    .map_err(|_| "f2fs: file not found")?;
                Ok(vfs_info_from_f2fs_entry(&f2fs_entry))
            }
            VfsFsType::Fat32 => {
                let resolved = resolve_fat32_node(&entry.source, relative_path)?;
                Ok(vfs_info_from_fat32_file(&resolved))
            }
            VfsFsType::ExFat => {
                let resolved = resolve_exfat_node(&entry.source, relative_path)?;
                Ok(vfs_info_from_exfat_file(&resolved))
            }
            VfsFsType::Ntfs => {
                let resolved = resolve_ntfs_node(&entry.source, relative_path)?;
                Ok(vfs_info_from_ntfs_entry(&resolved))
            }
            VfsFsType::Xfs => Err(unsupported_vfs_capability(
                VfsFsType::Xfs,
                VfsUnsupportedCapability::Open,
            )),
            VfsFsType::Btrfs => {
                let resolved = resolve_btrfs_node(&entry.source, relative_path)?;
                Ok(vfs_info_from_btrfs_inode(&resolved))
            }
        }
    }

    /// Mount tablosunu listeler (mount komutu çıktısı)
    pub fn list_mounts(&self) -> Vec<String> {
        self.mount_table
            .values()
            .map(|e| {
                format!(
                    "{} on {} type {} ({})",
                    e.source,
                    e.mount_point,
                    e.fs_type.as_str(),
                    if e.readonly { "ro" } else { "rw" }
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

    pub fn read_bytes(&self, path: &str) -> Result<Vec<u8>, &'static str> {
        let normalized_path = normalize_vfs_path(path);
        let entry = self
            .resolve_fs(normalized_path.as_str())
            .ok_or_else(no_filesystem_for_path)?;
        let relative_path =
            relative_mount_path(entry.mount_point.as_str(), normalized_path.as_str());

        match entry.fs_type {
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
                resolved
                    .mounted
                    .fs
                    .read_file_from_storage(&resolved.inode, &resolved.mounted.storage)
                    .map_err(|_| "ext4: failed to read file")
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
        }
    }

    pub fn list_dir(&self, path: &str) -> Result<Vec<VfsDirEntry>, &'static str> {
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

fn is_mount_root(relative_path: &str) -> bool {
    relative_path.is_empty() || relative_path == "/"
}

fn normalize_vfs_path(path: &str) -> String {
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

fn relative_mount_path<'a>(mount_point: &str, path: &'a str) -> &'a str {
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
        mode: if entry.is_dir { 0o040755 } else { 0o100644 },
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
    ListDirectory,
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
        _ => "vfs: unsupported capability",
    }
}

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

fn resolve_ext4_node(source: &str, relative_path: &str) -> Result<ResolvedExt4Node, &'static str> {
    let mounted =
        crate::fs::ext4::get_mounted_ext4(source).ok_or("ext4: backend not mounted for source")?;
    let mut inode_num = mounted.fs.root_inode;
    let mut inode = mounted
        .fs
        .root_inode_from_storage(&mounted.storage)
        .map_err(|_| "ext4: failed to load root inode")?;

    for component in path_components(relative_path) {
        if !inode.is_directory() {
            return Err("ext4: parent path is not a directory");
        }
        let entries = mounted
            .fs
            .read_dir_from_storage(&inode, &mounted.storage)
            .map_err(|_| "ext4: failed to read directory")?;
        let child = entries
            .into_iter()
            .find(|entry| entry.name == component)
            .ok_or("ext4: file not found")?;
        inode_num = child.inode;
        inode = mounted
            .fs
            .read_inode_from_storage(child.inode, &mounted.storage)
            .map_err(|_| "ext4: failed to read inode")?;
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
    let entries = resolved
        .mounted
        .fs
        .read_dir_from_storage(&resolved.inode, &resolved.mounted.storage)
        .map_err(|_| "ext4: failed to read directory")?;
    let mut result = Vec::new();
    for entry in entries {
        let inode = resolved
            .mounted
            .fs
            .read_inode_from_storage(entry.inode, &resolved.mounted.storage)
            .map_err(|_| "ext4: failed to read inode")?;
        result.push(VfsDirEntry {
            name: entry.name,
            size: inode.size(),
            is_directory: inode.is_directory(),
            fs_type: VfsFsType::Ext4,
        });
    }
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

    for chunk in dir_data.chunks_exact(32) {
        let entry: crate::fs::fat::Fat32DirEntry =
            unsafe { core::ptr::read_unaligned(chunk.as_ptr() as *const _) };
        if entry.is_empty() {
            break;
        }
        if entry.is_deleted() || entry.is_long_name() || entry.is_volume_label() {
            continue;
        }
        files.push(crate::fs::fat::Fat32File::from_entry(&entry));
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

fn path_components(path: &str) -> Vec<String> {
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
pub fn read_file(path: &str) -> Result<Vec<u8>, &'static str> {
    VFS_UNIFIED.lock().read_bytes(path)
}

pub fn list_dir(path: &str) -> Result<Vec<VfsDirEntry>, &'static str> {
    VFS_UNIFIED.lock().list_dir(path)
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
            "ext4: backend not mounted for source"
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
