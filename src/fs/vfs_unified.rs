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
use alloc::string::String;
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
                // procfs read dispatch
                let content = generate_proc_content(relative_path);
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
                // devfs dispatch
                Ok(VfsFileInfo {
                    inode: 0,
                    size: 0,
                    mode: 0o020666,
                    nlink: 1,
                    uid: 0,
                    gid: 0,
                    fs_type: VfsFsType::DevFs,
                    block_size: 4096,
                    blocks: 0,
                })
            }
            VfsFsType::SysFs => {
                // sysfs dispatch - use open_sys_inode for actual content
                Ok(VfsFileInfo {
                    inode: 0,
                    size: 0,
                    mode: 0o100444,
                    nlink: 1,
                    uid: 0,
                    gid: 0,
                    fs_type: VfsFsType::SysFs,
                    block_size: 4096,
                    blocks: 0,
                })
            }
            VfsFsType::TmpFs => {
                // tmpfs dispatch
                Ok(VfsFileInfo {
                    inode: 0,
                    size: 0,
                    mode: 0o100666,
                    nlink: 1,
                    uid: 0,
                    gid: 0,
                    fs_type: VfsFsType::TmpFs,
                    block_size: 4096,
                    blocks: 0,
                })
            }
            VfsFsType::Ext4 => {
                // ext4 dispatch - use ext4 filesystem functions
                // For now return basic info; full integration requires block device access
                Ok(VfsFileInfo {
                    inode: 1,
                    size: 0,
                    mode: 0o100644,
                    nlink: 1,
                    uid: 0,
                    gid: 0,
                    fs_type: VfsFsType::Ext4,
                    block_size: 4096,
                    blocks: 0,
                })
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
                // FAT32 dispatch via fat module
                Ok(VfsFileInfo {
                    inode: 1,
                    size: 0,
                    mode: 0o100644,
                    nlink: 1,
                    uid: 0,
                    gid: 0,
                    fs_type: VfsFsType::Fat32,
                    block_size: 4096,
                    blocks: 0,
                })
            }
            VfsFsType::Ntfs => {
                // NTFS dispatch
                Ok(VfsFileInfo {
                    inode: 1,
                    size: 0,
                    mode: 0o100644,
                    nlink: 1,
                    uid: 0,
                    gid: 0,
                    fs_type: VfsFsType::Ntfs,
                    block_size: 4096,
                    blocks: 0,
                })
            }
            VfsFsType::Xfs => {
                // XFS dispatch
                Ok(VfsFileInfo {
                    inode: 1,
                    size: 0,
                    mode: 0o100644,
                    nlink: 1,
                    uid: 0,
                    gid: 0,
                    fs_type: VfsFsType::Xfs,
                    block_size: 4096,
                    blocks: 0,
                })
            }
            VfsFsType::Btrfs => {
                // Btrfs dispatch
                Ok(VfsFileInfo {
                    inode: 1,
                    size: 0,
                    mode: 0o100644,
                    nlink: 1,
                    uid: 0,
                    gid: 0,
                    fs_type: VfsFsType::Btrfs,
                    block_size: 4096,
                    blocks: 0,
                })
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
                VfsFsType::ProcFs | VfsFsType::DevFs | VfsFsType::SysFs => {
                    // Sanal dosya sistemlerinin boyutu yok
                    result.push((mp.clone(), entry.fs_type, 0, 0, 0));
                }
                VfsFsType::Fat32 => {
                    // Try to use FAT module info if available
                    if let Some(fs) = crate::fs::fat::get_fat32(0) {
                        let total = (fs.cluster_size as u64) * (fs.total_clusters as u64);
                        // heuristics for used/free
                        let used = total / 4;
                        let free = total.saturating_sub(used);
                        result.push((mp.clone(), entry.fs_type, total, used, free));
                    } else {
                        // fallback stub
                        result.push((mp.clone(), entry.fs_type, 512 * 1024 * 1024, 128 * 1024 * 1024, 384 * 1024 * 1024));
                    }
                }
                _ => {
                    // Other real filesystems: keep conservative stub values
                    result.push((
                        mp.clone(),
                        entry.fs_type,
                        1024 * 1024 * 1024,
                        256 * 1024 * 1024,
                        768 * 1024 * 1024,
                    ));
                }
            }
        }
        result
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
    let vfs = VFS_UNIFIED.lock();
    let entry = vfs
        .resolve_fs(path)
        .ok_or("No filesystem mounted for path")?;
    let relative_path = path.strip_prefix(&entry.mount_point).unwrap_or(path);

    match entry.fs_type {
        VfsFsType::ProcFs => {
            let content = generate_proc_content(relative_path);
            Ok(content.into_bytes())
        }
        VfsFsType::SysFs => {
            // Use sysfs inode for reading
            if let Ok(inode) = crate::fs::sysfs::open_sys_inode(relative_path) {
                let mut buf = alloc::vec![0u8; 4096];
                if let Ok(n) = inode.read_at(0, &mut buf) {
                    buf.truncate(n);
                    return Ok(buf);
                }
            }
            Ok(Vec::new())
        }
        VfsFsType::DevFs => {
            // Device file reading handled by device drivers
            Ok(Vec::new())
        }
        VfsFsType::TmpFs => {
            // tmpfs reading
            Ok(Vec::new())
        }
        VfsFsType::F2fs => {
            // F2FS file reading
            let mut buf = alloc::vec![0u8; 4096];
            if let Ok(n) = crate::fs::f2fs::read_f2fs_file_at(relative_path, 0, &mut buf) {
                buf.truncate(n);
                return Ok(buf);
            }
            Err("Failed to read from F2FS")
        }
        VfsFsType::Ext4 => {
            // ext4 file reading - requires block device
            Ok(Vec::new())
        }
        _ => {
            // Other filesystems - stub
            Ok(Vec::new())
        }
    }
}
