//! # VFS Mount Table
//!
//! Dosya sistemi baglama noktalarini yonetir.
//! Linux mount(2) syscall uyumlu flag'ler ve options parser icerir.
//!
//! ## Linux mount(2) Flag Sabitleri (include/uapi/linux/mount.h)
//! ```text
//! MS_RDONLY      = 1       Mount read-only
//! MS_NOSUID      = 2       Ignore suid and sgid bits
//! MS_NODEV       = 4       Disallow access to device special files
//! MS_NOEXEC      = 8       Disallow program execution
//! MS_SYNCHRONOUS = 16      Writes are synced at once
//! MS_REMOUNT     = 32      Alter flags of a mounted FS
//! MS_MANDLOCK    = 64      Mandatory locking
//! MS_NOATIME     = 1024    Do not update access times
//! MS_NODIRATIME  = 2048    Do not update directory access times
//! MS_BIND        = 4096    Bind mount
//! MS_REC         = 16384   Recursive bind mount
//! MS_RELATIME    = 2097152 Update atime relative to mtime/ctime
//! ```

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use lazy_static::lazy_static;
use spin::{Mutex, RwLock};

// ============================================================================
// Linux mount(2) Flag Sabitleri
// ============================================================================

/// MS_RDONLY — Mount read-only
pub const MS_RDONLY: u32 = 1;
/// MS_NOSUID — Ignore suid and sgid bits
pub const MS_NOSUID: u32 = 2;
/// MS_NODEV — Disallow access to device special files
pub const MS_NODEV: u32 = 4;
/// MS_NOEXEC — Disallow program execution
pub const MS_NOEXEC: u32 = 8;
/// MS_SYNCHRONOUS — Writes are synced at once
pub const MS_SYNCHRONOUS: u32 = 16;
/// MS_REMOUNT — Alter flags of a mounted FS
pub const MS_REMOUNT: u32 = 32;
/// MS_MANDLOCK — Mandatory locking (deprecated since Linux 5.15)
pub const MS_MANDLOCK: u32 = 64;
/// MS_NOATIME — Do not update access times
pub const MS_NOATIME: u32 = 1024;
/// MS_NODIRATIME — Do not update directory access times
pub const MS_NODIRATIME: u32 = 2048;
/// MS_BIND — Bind mount (Linux 2.4+)
pub const MS_BIND: u32 = 4096;
/// MS_REC — Recursive bind mount / propagation
pub const MS_REC: u32 = 16384;
/// MS_SILENT — Suppress printk warnings
pub const MS_SILENT: u32 = 32768;
/// MS_RELATIME — Update atime relative to mtime/ctime (Linux 2.6.20+)
pub const MS_RELATIME: u32 = 1 << 21;
/// MS_STRICTATIME — Always update atime (Linux 2.6.30+)
pub const MS_STRICTATIME: u32 = 1 << 24;
/// MS_LAZYTIME — Reduce on-disk timestamp updates (Linux 4.0+)
pub const MS_LAZYTIME: u32 = 1 << 25;

/// MS_DIRSYNC — Synchronous directory updates
pub const MS_DIRSYNC: u32 = 1 << 14;

// Ext4/data journaling mode flags (mount option ile set edilir)
// Linux'te bunlar mount option string olarak gelir, flag olarak saklanir

/// Data writeback modu — metadata only journaled, data ordering yok
pub const MS_DATA_WRITEBACK: u32 = 1 << 26;
/// Data ordered modu (default) — data metadata'dan once yazilir
pub const MS_DATA_ORDERED: u32 = 1 << 27;
/// Data journal modu — data ve metadata ikisi de journaled
pub const MS_DATA_JOURNAL: u32 = 1 << 28;
/// Barrier kapali — write barrier devre disi
pub const MS_NOBARRIER: u32 = 1 << 29;

/// Kullanici alaninin set edebilecegi flag maskesi
pub const MS_USER_SETTABLE: u32 = MS_NOSUID
    | MS_NODEV
    | MS_NOEXEC
    | MS_NOATIME
    | MS_NODIRATIME
    | MS_RELATIME
    | MS_RDONLY
    | MS_SYNCHRONOUS
    | MS_LAZYTIME
    | MS_STRICTATIME;

// ============================================================================
// Dosya Sistemi Turu
// ============================================================================

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
    Bind,
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
            "devtmpfs" | "devfs" => FsType::DevFs,
            "sysfs" => FsType::SysFs,
            "tmpfs" => FsType::TmpFs,
            "none" | "" => FsType::Bind,
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
            FsType::Bind => "none",
            FsType::Unknown(s) => s.as_str(),
        }
    }
}

// ============================================================================
// Mount Options Parser (util-linux opt_map uyumlu)
// ============================================================================

/// Mount options string'ini (-o ile gecen) parse eder ve MS_* flag'lerine cevirir.
///
/// Linux util-linux mount(8) opt_map tablosuna uygun parsing yapar.
/// "ro,noexec,nosuid,nodev,noatime,relatime" gibi virgul ile ayrilmis
/// option listelerini destekler.
///
/// Donus: (ms_flags, fs_specific_options)
/// - ms_flags: VFS-level MS_* flag bit maskesi
/// - fs_specific_options: Filesystem'e ozel option string'i (MS flag'e eslesmeyenler)
pub fn parse_mount_options(options: &str) -> (u32, String) {
    let mut ms_flags: u32 = 0;
    let mut fs_opts: Vec<&str> = Vec::new();

    if options.is_empty() {
        return (0, String::new());
    }

    for raw_opt in options.split(',') {
        let opt = raw_opt.trim();
        if opt.is_empty() {
            continue;
        }

        // util-linux opt_map tablosuna uygun eslestirme
        // Her option ya bir MS_* flag'e map'lenir ya da fs-specific olarak kalir
        let matched = match opt {
            "ro" => {
                ms_flags |= MS_RDONLY;
                true
            }
            "rw" => {
                ms_flags &= !MS_RDONLY;
                true
            }
            "noexec" => {
                ms_flags |= MS_NOEXEC;
                true
            }
            "exec" => {
                ms_flags &= !MS_NOEXEC;
                true
            }
            "nosuid" => {
                ms_flags |= MS_NOSUID;
                true
            }
            "suid" => {
                ms_flags &= !MS_NOSUID;
                true
            }
            "nodev" => {
                ms_flags |= MS_NODEV;
                true
            }
            "dev" => {
                ms_flags &= !MS_NODEV;
                true
            }
            "noatime" => {
                ms_flags |= MS_NOATIME;
                true
            }
            "atime" => {
                ms_flags &= !MS_NOATIME;
                true
            }
            "nodiratime" => {
                ms_flags |= MS_NODIRATIME;
                true
            }
            "diratime" => {
                ms_flags &= !MS_NODIRATIME;
                true
            }
            "relatime" => {
                ms_flags |= MS_RELATIME;
                true
            }
            "norelatime" => {
                ms_flags &= !MS_RELATIME;
                true
            }
            "strictatime" => {
                ms_flags |= MS_STRICTATIME;
                true
            }
            "nostrictatime" => {
                ms_flags &= !MS_STRICTATIME;
                true
            }
            "sync" => {
                ms_flags |= MS_SYNCHRONOUS;
                true
            }
            "async" => {
                ms_flags &= !MS_SYNCHRONOUS;
                true
            }
            "lazytime" => {
                ms_flags |= MS_LAZYTIME;
                true
            }
            "nolazytime" => {
                ms_flags &= !MS_LAZYTIME;
                true
            }
            "remount" => {
                ms_flags |= MS_REMOUNT;
                true
            }
            "bind" => {
                ms_flags |= MS_BIND;
                true
            }
            "rbind" | "recursive" => {
                ms_flags |= MS_BIND | MS_REC;
                true
            }
            "silent" => {
                ms_flags |= MS_SILENT;
                true
            }
            "defaults" => true,
            "barrier" | "barrier=1" => {
                ms_flags &= !MS_NOBARRIER;
                true
            }
            "nobarrier" | "barrier=0" => {
                ms_flags |= MS_NOBARRIER;
                true
            }
            "data=journal" => {
                ms_flags |= MS_DATA_JOURNAL;
                ms_flags &= !MS_DATA_ORDERED;
                ms_flags &= !MS_DATA_WRITEBACK;
                true
            }
            "data=ordered" => {
                ms_flags |= MS_DATA_ORDERED;
                ms_flags &= !MS_DATA_JOURNAL;
                ms_flags &= !MS_DATA_WRITEBACK;
                true
            }
            "data=writeback" => {
                ms_flags |= MS_DATA_WRITEBACK;
                ms_flags &= !MS_DATA_ORDERED;
                ms_flags &= !MS_DATA_JOURNAL;
                true
            }
            "dirsync" => {
                ms_flags |= MS_DIRSYNC;
                true
            }
            _ => false,
        };

        if !matched {
            // VFS flag'e eslesmeyen option filesystem'e ozeldir
            fs_opts.push(opt);
        }
    }

    // MS_NOATIME implies MS_NODIRATIME (Linux kernel semantigi)
    if ms_flags & MS_NOATIME != 0 {
        ms_flags |= MS_NODIRATIME;
    }

    // MS_RELATIME default'tir (Linux 2.6.30+), hicbiri belirtilmediyse set et
    if ms_flags & (MS_NOATIME | MS_RELATIME | MS_STRICTATIME) == 0 {
        ms_flags |= MS_RELATIME;
    }

    let fs_specific = fs_opts.join(",");
    (ms_flags, fs_specific)
}

/// MS_* flag'lerinden MountFlags yapisini olusturur
pub fn mount_flags_from_ms(ms_flags: u32) -> MountFlags {
    MountFlags {
        read_only: ms_flags & MS_RDONLY != 0,
        no_exec: ms_flags & MS_NOEXEC != 0,
        no_suid: ms_flags & MS_NOSUID != 0,
        no_dev: ms_flags & MS_NODEV != 0,
        no_atime: ms_flags & MS_NOATIME != 0,
        no_dir_atime: ms_flags & MS_NODIRATIME != 0,
        relatime: ms_flags & MS_RELATIME != 0,
        strict_atime: ms_flags & MS_STRICTATIME != 0,
        lazy_time: ms_flags & MS_LAZYTIME != 0,
        synchronous: ms_flags & MS_SYNCHRONOUS != 0,
    }
}

// ============================================================================
// Mount Bayraklari (Genisletilmis)
// ============================================================================

#[derive(Debug, Clone, Copy, Default)]
pub struct MountFlags {
    pub read_only: bool,
    pub no_exec: bool,
    pub no_suid: bool,
    pub no_dev: bool,
    pub no_atime: bool,
    pub no_dir_atime: bool,
    pub relatime: bool,
    pub strict_atime: bool,
    pub lazy_time: bool,
    pub synchronous: bool,
}

impl MountFlags {
    pub const fn default_rw() -> Self {
        Self {
            read_only: false,
            no_exec: false,
            no_suid: false,
            no_dev: false,
            no_atime: false,
            no_dir_atime: false,
            relatime: true,
            strict_atime: false,
            lazy_time: false,
            synchronous: false,
        }
    }

    pub const fn read_only() -> Self {
        Self {
            read_only: true,
            no_exec: false,
            no_suid: false,
            no_dev: false,
            no_atime: false,
            no_dir_atime: false,
            relatime: true,
            strict_atime: false,
            lazy_time: false,
            synchronous: false,
        }
    }

    pub const fn virtual_fs() -> Self {
        Self {
            read_only: false,
            no_exec: true,
            no_suid: true,
            no_dev: false,
            no_atime: true,
            no_dir_atime: true,
            relatime: false,
            strict_atime: false,
            lazy_time: false,
            synchronous: false,
        }
    }

    /// MS_* flag maskesine cevir
    pub fn to_ms_flags(&self) -> u32 {
        let mut flags: u32 = 0;
        if self.read_only {
            flags |= MS_RDONLY;
        }
        if self.no_exec {
            flags |= MS_NOEXEC;
        }
        if self.no_suid {
            flags |= MS_NOSUID;
        }
        if self.no_dev {
            flags |= MS_NODEV;
        }
        if self.no_atime {
            flags |= MS_NOATIME;
        }
        if self.no_dir_atime {
            flags |= MS_NODIRATIME;
        }
        if self.relatime {
            flags |= MS_RELATIME;
        }
        if self.strict_atime {
            flags |= MS_STRICTATIME;
        }
        if self.lazy_time {
            flags |= MS_LAZYTIME;
        }
        if self.synchronous {
            flags |= MS_SYNCHRONOUS;
        }
        flags
    }
}

// ============================================================================
// Mount Noktasi
// ============================================================================

#[derive(Debug, Clone)]
pub struct MountPoint {
    pub source: String,
    pub target: String,
    pub fs_type: FsType,
    pub flags: MountFlags,
    /// MS_* raw flag maskesi
    pub ms_flags: u32,
    /// Filesystem-specific option string (MS flag'e eslesmeyen -o option'lari)
    pub fs_options: String,
    /// Bind mount ise kaynak yol (bind source path)
    pub bind_source: Option<String>,
    /// Recursive bind mount mu (MS_REC)
    pub bind_recursive: bool,
}

impl MountPoint {
    /// Bu mount bir bind mount mu?
    pub fn is_bind(&self) -> bool {
        self.ms_flags & MS_BIND != 0 || self.fs_type == FsType::Bind
    }

    /// Data journaling modunu dondurur (ext4 icin)
    /// 0 = default (ordered), 1 = journal, 2 = writeback
    pub fn data_journal_mode(&self) -> u8 {
        if self.ms_flags & MS_DATA_JOURNAL != 0 {
            1
        } else if self.ms_flags & MS_DATA_WRITEBACK != 0 {
            2
        } else {
            0 // ordered (default)
        }
    }

    /// Write barrier aktif mi?
    pub fn barrier_enabled(&self) -> bool {
        self.ms_flags & MS_NOBARRIER == 0
    }

    /// Commit interval (saniye) — fs_options icinden parse eder
    /// Default: 5 saniye (Linux ext4 default'u)
    pub fn commit_interval_secs(&self) -> u32 {
        for opt in self.fs_options.split(',') {
            if let Some(val) = opt.strip_prefix("commit=") {
                if let Ok(secs) = val.parse::<u32>() {
                    return secs.max(1).min(600); // 1-600 saniye arasi
                }
            }
        }
        5 // Linux ext4 default
    }
}

// ============================================================================
// Mount Tablosu
// ============================================================================

pub struct MountTable {
    mounts: RwLock<Vec<MountPoint>>,
}

impl MountTable {
    pub const fn new() -> Self {
        Self {
            mounts: RwLock::new(Vec::new()),
        }
    }

    /// Standart mount — device + fs_type + options
    pub fn mount(
        &self,
        source: &str,
        target: &str,
        fs_type: &str,
        flags: MountFlags,
    ) -> Result<(), &'static str> {
        let ms_flags = flags.to_ms_flags();
        self.mount_with_ms(source, target, fs_type, ms_flags, "")
    }

    /// MS_* flag maskesi ve options string'i ile mount
    pub fn mount_with_ms(
        &self,
        source: &str,
        target: &str,
        fs_type: &str,
        ms_flags: u32,
        options: &str,
    ) -> Result<(), &'static str> {
        let mut mounts = self.mounts.write();

        // Ayni hedefe zaten baglanmis mi
        if mounts.iter().any(|m| m.target == target) {
            return Err("Mount point already in use");
        }

        let (parsed_ms, fs_opts) = if options.is_empty() {
            (0, String::new())
        } else {
            parse_mount_options(options)
        };

        // Options'tan gelen flag'leri birlestir
        let combined_ms = ms_flags | parsed_ms;
        let combined_opts = if fs_opts.is_empty() {
            fs_opts
        } else if options.is_empty() {
            options.to_string()
        } else {
            // fs_opts zaten options'tan filtrelenmis hali
            fs_opts
        };

        let ft = FsType::from_str(fs_type);
        let bind_source = if ft == FsType::Bind || combined_ms & MS_BIND != 0 {
            Some(source.to_string())
        } else {
            None
        };

        let mp = MountPoint {
            source: source.to_string(),
            target: target.to_string(),
            fs_type: ft,
            flags: mount_flags_from_ms(combined_ms),
            ms_flags: combined_ms,
            fs_options: combined_opts,
            bind_source,
            bind_recursive: combined_ms & MS_REC != 0,
        };

        crate::serial_println!(
            "[VFS] Mounting {} on {} (type: {}, flags: 0x{:x})",
            source,
            target,
            fs_type,
            combined_ms
        );

        mounts.push(mp);
        Ok(())
    }

    /// Bind mount — bir path'i baska bir path'te gorunur yap
    ///
    /// Linux semantigi:
    /// - source bir dosya veya dizin olabilir
    /// - target parent dizini var olmali
    /// - source file ise target da file olmali (ustune yazma)
    /// - source dir ise target da dir olmali
    /// - MS_REC ile recursive (alt mount'lari da tasir)
    pub fn bind_mount(
        &self,
        source: &str,
        target: &str,
        recursive: bool,
    ) -> Result<(), &'static str> {
        let mut mounts = self.mounts.write();

        if mounts.iter().any(|m| m.target == target) {
            return Err("Mount point already in use");
        }

        // Source mount'unu bul (source baska bir mount altinda olabilir)
        let source_mount = self.find_mount_internal(&mounts, source);

        let mut ms_flags = MS_BIND;
        if recursive {
            ms_flags |= MS_REC;
        }

        // Source'dan flag'leri devral (bind mount kaynak flag'leri korur)
        if let Some(ref src_mp) = source_mount {
            // Kaydettigimiz mount'tan flag'leri al
            ms_flags |= src_mp.ms_flags & MS_USER_SETTABLE;
        }

        let mp = MountPoint {
            source: source.to_string(),
            target: target.to_string(),
            fs_type: FsType::Bind,
            flags: mount_flags_from_ms(ms_flags),
            ms_flags,
            fs_options: String::new(),
            bind_source: Some(source.to_string()),
            bind_recursive: recursive,
        };

        crate::serial_println!(
            "[VFS] Bind mount: {} -> {} (recursive: {})",
            source,
            target,
            recursive
        );

        mounts.push(mp);

        // If recursive, propagate to all submounts under source
        if recursive {
            let source_prefix = if source.ends_with('/') {
                source.to_string()
            } else {
                let mut s = source.to_string();
                s.push('/');
                s
            };

            // Collect submount entries to clone (avoid borrow issues)
            let submounts_to_clone: Vec<MountPoint> = mounts
                .iter()
                .filter(|m| {
                    // Only clone submounts that are NOT the one we just added
                    m.target != target
                        && (m.target.starts_with(&source_prefix)
                            || (source == "/" && !m.target.is_empty()))
                })
                .cloned()
                .collect();

            for submount in submounts_to_clone {
                let relative_path = if source == "/" {
                    submount.target.clone()
                } else {
                    submount.target.strip_prefix(&source_prefix).unwrap_or("").to_string()
                };

                let sub_target = if target.ends_with('/') {
                    format!("{}{}", target, relative_path)
                } else if relative_path.is_empty() {
                    target.to_string()
                } else {
                    format!("{}/{}", target, relative_path)
                };

                // Skip if target already taken
                if mounts.iter().any(|m| m.target == sub_target) {
                    continue;
                }

                let sub_mp = MountPoint {
                    source: submount.source.clone(),
                    target: sub_target,
                    fs_type: submount.fs_type,
                    flags: mount_flags_from_ms(submount.ms_flags),
                    ms_flags: submount.ms_flags,
                    fs_options: submount.fs_options.clone(),
                    bind_source: submount.bind_source.clone(),
                    bind_recursive: false,
                };

                mounts.push(sub_mp);
            }
        }

        Ok(())
    }

    /// Remount — mevcut mount'un flag'lerini degistir
    pub fn remount(&self, target: &str, new_ms_flags: u32) -> Result<(), &'static str> {
        let mut mounts = self.mounts.write();
        let mp = mounts
            .iter_mut()
            .find(|m| m.target == target)
            .ok_or("Mount point not found")?;

        // MS_REMOUNT sadece flag degisikligine izin verir
        // read_only, noexec, nosuid, nodev, noatime, relatime degistirilebilir
        let allowed = MS_RDONLY
            | MS_NOEXEC
            | MS_NOSUID
            | MS_NODEV
            | MS_NOATIME
            | MS_NODIRATIME
            | MS_RELATIME
            | MS_STRICTATIME
            | MS_LAZYTIME
            | MS_SYNCHRONOUS;

        // Mevcut flag'lerden allowed maskesini temizle, yenilerini ekle
        mp.ms_flags = (mp.ms_flags & !allowed) | (new_ms_flags & allowed);
        mp.flags = mount_flags_from_ms(mp.ms_flags);

        crate::serial_println!("[VFS] Remounted {} with flags 0x{:x}", target, mp.ms_flags);

        Ok(())
    }

    /// Unmount
    pub fn umount(&self, target: &str) -> Result<(), &'static str> {
        let mut mounts = self.mounts.write();
        let pos = mounts
            .iter()
            .position(|m| m.target == target)
            .ok_or("Mount point not found")?;

        // Root filesystem unmount edilemez
        if mounts[pos].target == "/" {
            return Err("Cannot unmount root filesystem");
        }

        let mp = mounts.remove(pos);
        crate::serial_println!("[VFS] Unmounted {} from {}", mp.source, mp.target);
        Ok(())
    }

    /// Mount listesi
    pub fn list(&self) -> Vec<MountPoint> {
        self.mounts.read().clone()
    }

    /// Path icin mount noktasi bul (longest prefix match)
    pub fn find_mount(&self, path: &str) -> Option<MountPoint> {
        let mounts = self.mounts.read();
        self.find_mount_internal(&mounts, path)
    }

    fn find_mount_internal(&self, mounts: &[MountPoint], path: &str) -> Option<MountPoint> {
        mounts
            .iter()
            .filter(|m| path.starts_with(&m.target) || m.target == path)
            .max_by_key(|m| m.target.len())
            .cloned()
    }

    /// Path read-only mount'ta mi?
    pub fn is_read_only(&self, path: &str) -> bool {
        self.find_mount(path)
            .map(|m| m.flags.read_only)
            .unwrap_or(false)
    }

    /// Path noexec mount'ta mi?
    pub fn is_noexec(&self, path: &str) -> bool {
        self.find_mount(path)
            .map(|m| m.flags.no_exec)
            .unwrap_or(false)
    }

    /// Path nosuid mount'ta mi?
    pub fn is_nosuid(&self, path: &str) -> bool {
        self.find_mount(path)
            .map(|m| m.flags.no_suid)
            .unwrap_or(false)
    }

    /// Path nodev mount'ta mi?
    pub fn is_nodev(&self, path: &str) -> bool {
        self.find_mount(path)
            .map(|m| m.flags.no_dev)
            .unwrap_or(false)
    }

    /// Path noatime mount'ta mi?
    pub fn is_noatime(&self, path: &str) -> bool {
        self.find_mount(path)
            .map(|m| m.flags.no_atime)
            .unwrap_or(false)
    }
}

lazy_static! {
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
