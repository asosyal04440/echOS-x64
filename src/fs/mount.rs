//! # VFS Mount Table
//!
//! Dosya sistemi baglama noktalarini yonetir.
//! Her baglama noktasi bir kaynak, hedef dizin ve dosya sistemi turunu icerir.
//!
//! ```text
//!  /            ─── F2FS (root filesystem)
//!  /proc        ─── procfs (virtual)
//!  /dev         ─── devfs (virtual)
//!  /sys         ─── sysfs (virtual)
//!  /tmp         ─── tmpfs (RAM-backed)
//!  /mnt/disk1   ─── ext4 / FAT32 (secondary disk)
//! ```

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use lazy_static::lazy_static;
use spin::Mutex;

/// Dosya sistemi turu
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsType {
    F2fs,
    Ext4,
    Fat32,
    ExFat,
    Ntfs,
    Xfs,
    Btrfs,
    ProcFs,
    DevFs,
    SysFs,
    TmpFs,
    Unknown(String),
}

impl FsType {
    pub fn from_str(s: &str) -> Self {
        match s {
            "f2fs" => FsType::F2fs,
            "ext4" => FsType::Ext4,
            "fat32" | "vfat" => FsType::Fat32,
            "exfat" => FsType::ExFat,
            "ntfs" => FsType::Ntfs,
            "xfs" => FsType::Xfs,
            "btrfs" => FsType::Btrfs,
            "proc" | "procfs" => FsType::ProcFs,
            "devfs" | "devtmpfs" => FsType::DevFs,
            "sysfs" => FsType::SysFs,
            "tmpfs" => FsType::TmpFs,
            other => FsType::Unknown(other.to_string()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            FsType::F2fs => "f2fs",
            FsType::Ext4 => "ext4",
            FsType::Fat32 => "vfat",
            FsType::ExFat => "exfat",
            FsType::Ntfs => "ntfs",
            FsType::Xfs => "xfs",
            FsType::Btrfs => "btrfs",
            FsType::ProcFs => "proc",
            FsType::DevFs => "devtmpfs",
            FsType::SysFs => "sysfs",
            FsType::TmpFs => "tmpfs",
            FsType::Unknown(s) => s.as_str(),
        }
    }
}

/// Mount bayraklari
#[derive(Debug, Clone, Copy)]
pub struct MountFlags {
    /// Salt okunur mu?
    pub read_only: bool,
    /// noexec — calistirma izni yok
    pub no_exec: bool,
    /// nosuid — setuid/setgid yok
    pub no_suid: bool,
    /// nodev — device dosyalari yok
    pub no_dev: bool,
}

impl MountFlags {
    pub const fn default_rw() -> Self {
        Self {
            read_only: false,
            no_exec: false,
            no_suid: false,
            no_dev: false,
        }
    }

    pub const fn read_only() -> Self {
        Self {
            read_only: true,
            no_exec: false,
            no_suid: false,
            no_dev: false,
        }
    }

    pub const fn virtual_fs() -> Self {
        Self {
            read_only: false,
            no_exec: true,
            no_suid: true,
            no_dev: false,
        }
    }
}

/// Tek bir baglama noktasi
#[derive(Debug, Clone)]
pub struct MountPoint {
    /// Kaynak aygit veya "none" (sanal FS icin)
    pub source: String,
    /// Hedef dizin (ornegin "/proc")
    pub target: String,
    /// Dosya sistemi turu
    pub fs_type: FsType,
    /// Mount bayraklari
    pub flags: MountFlags,
}

/// Mount tablosu yoneticisi
pub struct MountTable {
    mounts: Mutex<Vec<MountPoint>>,
}

impl MountTable {
    pub const fn new() -> Self {
        Self {
            mounts: Mutex::new(Vec::new()),
        }
    }

    /// Yeni bir dosya sistemi bagla
    pub fn mount(
        &self,
        source: &str,
        target: &str,
        fs_type: &str,
        flags: MountFlags,
    ) -> Result<(), &'static str> {
        let mut mounts = self.mounts.lock();

        // Ayni hedefe zaten baglanmis mi kontrol et
        if mounts.iter().any(|m| m.target == target) {
            return Err("Mount point already in use");
        }

        let mp = MountPoint {
            source: source.to_string(),
            target: target.to_string(),
            fs_type: FsType::from_str(fs_type),
            flags,
        };

        crate::serial_println!(
            "[VFS] Mounting {} on {} (type: {})",
            source,
            target,
            fs_type
        );

        mounts.push(mp);
        Ok(())
    }

    /// Dosya sistemini cikar (unmount)
    pub fn umount(&self, target: &str) -> Result<(), &'static str> {
        let mut mounts = self.mounts.lock();
        let pos = mounts
            .iter()
            .position(|m| m.target == target)
            .ok_or("Mount point not found")?;

        let mp = mounts.remove(pos);
        crate::serial_println!("[VFS] Unmounted {} from {}", mp.source, mp.target);
        Ok(())
    }

    /// Tum mount noktalarini listele (mount komutu ciktisi gibi)
    pub fn list(&self) -> Vec<MountPoint> {
        self.mounts.lock().clone()
    }

    /// Verilen yol icin uygun mount noktasini bul
    /// En uzun prefix eslemesi yapilir (longest prefix match)
    pub fn find_mount(&self, path: &str) -> Option<MountPoint> {
        let mounts = self.mounts.lock();
        mounts
            .iter()
            .filter(|m| path.starts_with(&m.target))
            .max_by_key(|m| m.target.len())
            .cloned()
    }

    /// Verilen yolun salt okunur bir mount uzerinde olup olmadigini kontrol et
    pub fn is_read_only(&self, path: &str) -> bool {
        self.find_mount(path)
            .map(|m| m.flags.read_only)
            .unwrap_or(false)
    }
}

lazy_static! {
    /// Global mount tablosu
    pub static ref MOUNT_TABLE: MountTable = MountTable::new();
}

/// Sanal dosya sistemleri icin varsayilan mount'lari kaydet
pub fn mount_virtual_filesystems() {
    let _ = MOUNT_TABLE.mount("/dev/sda1", "/", "f2fs", MountFlags::default_rw());
    let _ = MOUNT_TABLE.mount("proc", "/proc", "proc", MountFlags::virtual_fs());
    let _ = MOUNT_TABLE.mount("devtmpfs", "/dev", "devtmpfs", MountFlags::virtual_fs());
    let _ = MOUNT_TABLE.mount("sysfs", "/sys", "sysfs", MountFlags::virtual_fs());
    let _ = MOUNT_TABLE.mount("tmpfs", "/tmp", "tmpfs", MountFlags::default_rw());

    crate::serial_println!("[VFS] {} mount points registered", MOUNT_TABLE.list().len());
}
