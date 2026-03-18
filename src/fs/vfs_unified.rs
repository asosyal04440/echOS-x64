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
        let entry = VfsMountEntry {
            mount_point: String::from(mount_point),
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

        self.mount_table.insert(String::from(mount_point), entry);
        self.fs_count += 1;
    }

    /// Umount
    pub fn umount(&mut self, mount_point: &str) -> Result<(), &'static str> {
        if self.mount_table.remove(mount_point).is_some() {
            self.fs_count -= 1;
            crate::serial_println!("[VFS] umount: {}", mount_point);
            Ok(())
        } else {
            Err("Mount point not found")
        }
    }

    /// Path'e göre hangi dosya sisteminin sorumlu olduğunu bulur
    pub fn resolve_fs(&self, path: &str) -> Option<&VfsMountEntry> {
        // En uzun eşleşen mount point'i bul (longest prefix match)
        let mut best_match: Option<&VfsMountEntry> = None;
        let mut best_len = 0;

        for (mp, entry) in &self.mount_table {
            if path.starts_with(mp.as_str()) && mp.len() > best_len {
                best_match = Some(entry);
                best_len = mp.len();
            }
        }

        best_match
    }

    /// Birleşik open — path'e göre doğru dosya sistemine yönlendirir
    pub fn open(&self, path: &str) -> Result<VfsFileInfo, &'static str> {
        let entry = self
            .resolve_fs(path)
            .ok_or("No filesystem mounted for path")?;
        let relative_path = path.strip_prefix(&entry.mount_point).unwrap_or(path);

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
                    Err("devfs: unified open requires a device-specific driver path")
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
                    Err("tmpfs: unified tmpfs data path is not wired")
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
                Ok(VfsFileInfo {
                    inode: 1, // F2fsEntry doesn't expose inode number
                    size: f2fs_entry.size as u64,
                    mode: if f2fs_entry.is_dir {
                        0o040755
                    } else {
                        0o100644
                    },
                    nlink: 1,
                    uid: 0,
                    gid: 0,
                    fs_type: VfsFsType::F2fs,
                    block_size: 4096,
                    blocks: (f2fs_entry.size + 4095) / 4096,
                })
            }
            VfsFsType::Fat32 => {
                let resolved = resolve_fat32_node(&entry.source, relative_path)?;
                Ok(vfs_info_from_fat32_file(&resolved))
            }
            VfsFsType::Ntfs => {
                let resolved = resolve_ntfs_node(&entry.source, relative_path)?;
                Ok(vfs_info_from_ntfs_entry(&resolved))
            }
            VfsFsType::Xfs => Err("xfs: unified VFS open is not wired to a real backend"),
            VfsFsType::Btrfs => Err("btrfs: unified VFS open is not wired to a real backend"),
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
                VfsFsType::Ntfs => {
                    if let Ok((total, used, free)) = ntfs_capacity(&entry.source) {
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
        let entry = self
            .resolve_fs(path)
            .ok_or("No filesystem mounted for path")?;
        let relative_path = path.strip_prefix(&entry.mount_point).unwrap_or(path);

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
            VfsFsType::DevFs => Err("devfs: unified reads require a device-specific driver path"),
            VfsFsType::TmpFs => Err("tmpfs: unified tmpfs reads are not wired"),
            VfsFsType::F2fs => {
                let mut buf = alloc::vec![0u8; 4096];
                if let Ok(n) = crate::fs::f2fs::read_f2fs_file_at(relative_path, 0, &mut buf) {
                    buf.truncate(n);
                    return Ok(buf);
                }
                Err("f2fs: failed to read file")
            }
            VfsFsType::Ext4 => {
                let resolved = resolve_ext4_node(&entry.source, relative_path)?;
                if resolved.inode.is_directory() {
                    return Err("ext4: path is a directory");
                }
                resolved
                    .mounted
                    .fs
                    .read_file(&resolved.inode, &resolved.mounted.device_data)
                    .map_err(|_| "ext4: failed to read file")
            }
            VfsFsType::Fat32 => {
                let resolved = resolve_fat32_node(&entry.source, relative_path)?;
                if resolved.file.is_dir {
                    return Err("fat32: path is a directory");
                }
                fat32_read_file(&resolved)
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
                    .read_file(&resolved.entry, &resolved.mounted.device_data)
                    .map_err(|_| "ntfs: failed to read file")
            }
            VfsFsType::Xfs => Err("xfs: unified reads are not wired to a real backend"),
            VfsFsType::Btrfs => Err("btrfs: unified reads are not wired to a real backend"),
        }
    }

    pub fn list_dir(&self, path: &str) -> Result<Vec<VfsDirEntry>, &'static str> {
        let entry = self
            .resolve_fs(path)
            .ok_or("No filesystem mounted for path")?;
        let relative_path = path.strip_prefix(&entry.mount_point).unwrap_or(path);

        match entry.fs_type {
            VfsFsType::ProcFs => list_procfs_dir(relative_path),
            VfsFsType::SysFs => list_sysfs_dir(relative_path),
            VfsFsType::DevFs => list_devfs_dir(relative_path),
            VfsFsType::TmpFs => {
                if is_mount_root(relative_path) {
                    Ok(Vec::new())
                } else {
                    Err("tmpfs: unified tmpfs directory listing is not wired")
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
            VfsFsType::Ntfs => list_ntfs_dir(&entry.source, relative_path),
            VfsFsType::Xfs => Err("xfs: unified directory listing is not wired to a real backend"),
            VfsFsType::Btrfs => {
                Err("btrfs: unified directory listing is not wired to a real backend")
            }
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
        .root_inode_data(&mounted.device_data)
        .map_err(|_| "ext4: failed to load root inode")?;

    for component in path_components(relative_path) {
        if !inode.is_directory() {
            return Err("ext4: parent path is not a directory");
        }
        let entries = mounted
            .fs
            .read_dir(&inode, &mounted.device_data)
            .map_err(|_| "ext4: failed to read directory")?;
        let child = entries
            .into_iter()
            .find(|entry| entry.name == component)
            .ok_or("ext4: file not found")?;
        inode_num = child.inode;
        inode = mounted
            .fs
            .read_inode(child.inode, &mounted.device_data)
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
        .read_dir(&resolved.inode, &resolved.mounted.device_data)
        .map_err(|_| "ext4: failed to read directory")?;
    let mut result = Vec::new();
    for entry in entries {
        let inode = resolved
            .mounted
            .fs
            .read_inode(entry.inode, &resolved.mounted.device_data)
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

fn read_fat32_table<'a>(
    fs: &crate::fs::fat::Fat32Fs,
    image: &'a [u8],
) -> Result<&'a [u8], &'static str> {
    let offset = fs.fat_start as usize * fs.sector_size as usize;
    let len = fs.fat_size as usize * fs.sector_size as usize;
    if offset + len > image.len() {
        return Err("fat32: FAT table exceeds mounted image");
    }
    Ok(&image[offset..offset + len])
}

fn read_fat32_cluster<'a>(
    fs: &crate::fs::fat::Fat32Fs,
    image: &'a [u8],
    cluster: u32,
) -> Result<&'a [u8], &'static str> {
    if cluster < 2 {
        return Err("fat32: invalid cluster number");
    }
    let offset = fs.cluster_to_sector(cluster) as usize * fs.sector_size as usize;
    let len = fs.cluster_size as usize;
    if offset + len > image.len() {
        return Err("fat32: cluster exceeds mounted image");
    }
    Ok(&image[offset..offset + len])
}

fn read_fat32_chain(
    fs: &crate::fs::fat::Fat32Fs,
    image: &[u8],
    start_cluster: u32,
) -> Result<Vec<u8>, &'static str> {
    let fat = read_fat32_table(fs, image)?;
    let mut data = Vec::new();
    let mut cluster = start_cluster;

    for _ in 0..fs.total_clusters.max(1) {
        data.extend_from_slice(read_fat32_cluster(fs, image, cluster)?);
        let next = fs.read_fat_entry(fat, cluster);
        if fs.is_eof(next) {
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
    fs: &crate::fs::fat::Fat32Fs,
    image: &[u8],
    cluster: u32,
) -> Result<Vec<crate::fs::fat::Fat32File>, &'static str> {
    let dir_data = read_fat32_chain(fs, image, cluster)?;
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
        let entries = read_fat32_dir(&mounted.fs, &mounted.image, current_cluster)?;
        let entry = entries
            .into_iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(component))
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
    let mut data = read_fat32_chain(
        &resolved.mounted.fs,
        &resolved.mounted.image,
        resolved.file.cluster,
    )?;
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
    let entries = read_fat32_dir(&resolved.mounted.fs, &resolved.mounted.image, cluster)?;
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
    let fat = read_fat32_table(&mounted.fs, &mounted.image)?;
    let mut free_clusters = 0u64;
    for cluster in 2..mounted.fs.total_clusters {
        if mounted.fs.read_fat_entry(fat, cluster) == 0 {
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
        .resolve_path(relative_path, &mounted.device_data)
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
        .bitmap_usage(&mounted.device_data)
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
        .list_directory(resolved.entry.entry_number, &resolved.mounted.device_data)
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

fn path_components(path: &str) -> impl Iterator<Item = &str> {
    path.trim_matches('/')
        .split('/')
        .filter(|component| !component.is_empty() && *component != ".")
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
    use super::{VfsFsType, VfsMountFlags, VfsUnified};
    use alloc::format;
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
