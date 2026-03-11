//! # sysfs — /sys Sanal Çekirdek Nesnesi Dosya Sistemi
//!
//! Linux tarzı `/sys` sanal dosya sisteminin echOS implementasyonu.
//! Çekirdek nesnelerini (PCI cihazları, sürücüler, güç yönetimi) dosya
//! hiyerarşisi olarak dışa aktarır.
//!
//! ## Hiyerarşi
//!
//! ```text
//! /sys/
//! ├── bus/
//! │   └── pci/
//! │       └── devices/
//! │           ├── 0000:00:00.0/
//! │           │   ├── vendor      (ör. "0x8086\n")
//! │           │   ├── device      (ör. "0x1237\n")
//! │           │   └── class       (ör. "0x060000\n")
//! │           └── ...
//! ├── class/
//! │   ├── net/
//! │   └── block/
//! ├── devices/
//! │   ├── system/
//! │   │   └── cpu/
//! │   │       ├── cpu0/
//! │   │       │   ├── online      ("1\n")
//! │   │       │   └── topology/
//! │   │       └── ...
//! │   └── power/
//! ├── kernel/
//! │   └── mm/
//! │       └── transparent_hugepage/
//! │           └── enabled        ("always\n")
//! └── power/
//!     └── state                  ("mem\n")
//! ```

use alloc::format;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use rcore_fs::vfs::{FileType, FsError, INode, Metadata, PollStatus, Timespec};

use crate::drivers::ata::BLOCK_SIZE;

// ============================================================================
// YARDIMCI
// ============================================================================

fn sys_file_meta(content: &str) -> Metadata {
    Metadata {
        dev: 2,
        inode: 0,
        size: content.len(),
        blk_size: BLOCK_SIZE,
        blocks: 0,
        atime: Timespec { sec: 0, nsec: 0 },
        mtime: Timespec { sec: 0, nsec: 0 },
        ctime: Timespec { sec: 0, nsec: 0 },
        type_: FileType::File,
        mode: 0o100444,
        nlinks: 1,
        uid: 0,
        gid: 0,
        rdev: 0,
    }
}

fn sys_dir_meta() -> Metadata {
    Metadata {
        dev: 2,
        inode: 0,
        size: 0,
        blk_size: BLOCK_SIZE,
        blocks: 0,
        atime: Timespec { sec: 0, nsec: 0 },
        mtime: Timespec { sec: 0, nsec: 0 },
        ctime: Timespec { sec: 0, nsec: 0 },
        type_: FileType::Dir,
        mode: 0o040555,
        nlinks: 2,
        uid: 0,
        gid: 0,
        rdev: 0,
    }
}

// ============================================================================
// SABIT İÇERİKLİ DOSYA INODE
// ============================================================================

pub struct SysFileInode {
    content: alloc::string::String,
    writable: bool,
}

impl SysFileInode {
    pub fn new(content: &str) -> Self {
        SysFileInode {
            content: content.to_string(),
            writable: false,
        }
    }

    pub fn writable(content: &str) -> Self {
        SysFileInode {
            content: content.to_string(),
            writable: true,
        }
    }
}

impl INode for SysFileInode {
    fn read_at(&self, offset: usize, buf: &mut [u8]) -> Result<usize, FsError> {
        let bytes = self.content.as_bytes();
        if offset >= bytes.len() {
            return Ok(0);
        }
        let to_copy = (bytes.len() - offset).min(buf.len());
        buf[..to_copy].copy_from_slice(&bytes[offset..offset + to_copy]);
        Ok(to_copy)
    }

    fn write_at(&self, _offset: usize, buf: &[u8]) -> Result<usize, FsError> {
        if self.writable {
            Ok(buf.len())
        } else {
            Err(FsError::NotSupported)
        }
    }

    fn poll(&self) -> Result<PollStatus, FsError> {
        Ok(PollStatus {
            read: true,
            write: self.writable,
            error: false,
        })
    }

    fn metadata(&self) -> Result<Metadata, FsError> {
        Ok(sys_file_meta(&self.content))
    }

    fn as_any_ref(&self) -> &dyn core::any::Any {
        self
    }
}

// ============================================================================
// DİZİN INODE
// ============================================================================

pub struct SysDirInode {
    path: alloc::string::String,
}

impl SysDirInode {
    pub fn new(path: &str) -> Self {
        SysDirInode {
            path: path.to_string(),
        }
    }
}

impl INode for SysDirInode {
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
        Ok(sys_dir_meta())
    }

    fn find(&self, name: &str) -> Result<Arc<dyn INode>, FsError> {
        lookup_sys_path(&format!("{}/{}", self.path, name))
    }

    fn as_any_ref(&self) -> &dyn core::any::Any {
        self
    }
}

// ============================================================================
// PATh ÇÖZÜMLEYICI
// ============================================================================

/// /sys path'ini çözümler ve uygun inode döndürür
fn lookup_sys_path(path: &str) -> Result<Arc<dyn INode>, FsError> {
    // Normalleştir
    let p = path
        .trim_start_matches('/')
        .trim_start_matches("sys")
        .trim_start_matches('/');

    match p {
        // Kök dizinleri
        "" | "sys" => return Ok(Arc::new(SysDirInode::new("/sys"))),
        "bus" => return Ok(Arc::new(SysDirInode::new("/sys/bus"))),
        "class" => return Ok(Arc::new(SysDirInode::new("/sys/class"))),
        "devices" => return Ok(Arc::new(SysDirInode::new("/sys/devices"))),
        "kernel" => return Ok(Arc::new(SysDirInode::new("/sys/kernel"))),
        "power" => return Ok(Arc::new(SysDirInode::new("/sys/power"))),
        _ => {}
    }

    // /sys/power/state
    if p == "power/state" {
        return Ok(Arc::new(SysFileInode::writable("mem\n")));
    }

    // /sys/kernel/mm/transparent_hugepage/enabled
    if p == "kernel/mm/transparent_hugepage/enabled" {
        return Ok(Arc::new(SysFileInode::new("[always] madvise never\n")));
    }
    if p == "kernel/mm/transparent_hugepage" {
        return Ok(Arc::new(SysDirInode::new(
            "/sys/kernel/mm/transparent_hugepage",
        )));
    }
    if p == "kernel/mm" {
        return Ok(Arc::new(SysDirInode::new("/sys/kernel/mm")));
    }

    // /sys/bus/pci/
    if p == "bus/pci" || p == "bus/pci/" {
        return Ok(Arc::new(SysDirInode::new("/sys/bus/pci")));
    }
    if p == "bus/pci/devices" {
        return pci_devices_dir();
    }

    // /sys/bus/pci/devices/XXXX:XX:XX.X
    if let Some(rest) = p.strip_prefix("bus/pci/devices/") {
        return pci_device_node(rest);
    }

    // /sys/devices/system/cpu
    if p.starts_with("devices/system/cpu") {
        return cpu_sys_node(&p["devices/system/cpu".len()..]);
    }

    // /sys/class/net, /sys/class/block
    if p == "class/net" {
        return Ok(Arc::new(SysDirInode::new("/sys/class/net")));
    }
    if p == "class/block" {
        return Ok(Arc::new(SysDirInode::new("/sys/class/block")));
    }

    Err(FsError::EntryNotFound)
}

/// /sys/bus/pci/devices dizini
fn pci_devices_dir() -> Result<Arc<dyn INode>, FsError> {
    Ok(Arc::new(SysDirInode::new("/sys/bus/pci/devices")))
}

/// /sys/bus/pci/devices/<bdf>/<attr> inode'u
fn pci_device_node(rest: &str) -> Result<Arc<dyn INode>, FsError> {
    // rest = "0000:00:01.0" veya "0000:00:01.0/vendor" gibi
    let mut parts = rest.splitn(2, '/');
    let bdf = parts.next().unwrap_or("");
    let attr = parts.next().unwrap_or("");

    if attr.is_empty() {
        // BDF dizin inode'u
        return Ok(Arc::new(SysDirInode::new(&format!(
            "/sys/bus/pci/devices/{}",
            bdf
        ))));
    }

    // BDF parse: "SSSS:BB:DD.F"
    // bdf örn: "0000:00:01.0"
    let bus = u8::from_str_radix(bdf.get(5..7).unwrap_or("00"), 16).unwrap_or(0);
    let dev = u8::from_str_radix(bdf.get(8..10).unwrap_or("00"), 16).unwrap_or(0);
    let func = u8::from_str_radix(bdf.get(11..12).unwrap_or("0"), 16).unwrap_or(0);

    // PCI config space'den oku (dword okuyup word kısmını maskele)
    let id_reg = crate::drivers::pci::read_config_dword(bus, dev, func, 0x00);
    let vendor = id_reg & 0xFFFF;
    let device = (id_reg >> 16) & 0xFFFF;
    let class = crate::drivers::pci::read_config_dword(bus, dev, func, 0x08) >> 8;

    match attr {
        "vendor" => Ok(Arc::new(SysFileInode::new(&format!("0x{:04x}\n", vendor)))),
        "device" => Ok(Arc::new(SysFileInode::new(&format!("0x{:04x}\n", device)))),
        "class" => Ok(Arc::new(SysFileInode::new(&format!("0x{:06x}\n", class)))),
        "subsystem_vendor" => Ok(Arc::new(SysFileInode::new("0x0000\n"))),
        "subsystem_device" => Ok(Arc::new(SysFileInode::new("0x0000\n"))),
        "irq" => Ok(Arc::new(SysFileInode::new("0\n"))),
        "enable" => Ok(Arc::new(SysFileInode::writable("1\n"))),
        _ => Err(FsError::EntryNotFound),
    }
}

/// /sys/devices/system/cpu/... inode'u
fn cpu_sys_node(rest: &str) -> Result<Arc<dyn INode>, FsError> {
    let rest = rest.trim_start_matches('/');

    if rest.is_empty() {
        return Ok(Arc::new(SysDirInode::new("/sys/devices/system/cpu")));
    }

    // /sys/devices/system/cpu/cpuN/...
    if let Some(cpu_rest) = rest.strip_prefix("cpu") {
        let mut parts = cpu_rest.splitn(2, '/');
        let cpu_idx_str = parts.next().unwrap_or("0");
        let attr = parts.next().unwrap_or("");

        let cpu_idx: usize = cpu_idx_str.parse().unwrap_or(0);
        let total_cpus = crate::task::scheduler::get_cpu_count() as usize;

        if cpu_idx >= total_cpus {
            return Err(FsError::EntryNotFound);
        }

        if attr.is_empty() {
            return Ok(Arc::new(SysDirInode::new(&format!(
                "/sys/devices/system/cpu/cpu{}",
                cpu_idx
            ))));
        }

        return match attr {
            "online" => Ok(Arc::new(SysFileInode::new("1\n"))),
            "topology" => Ok(Arc::new(SysDirInode::new(&format!(
                "/sys/devices/system/cpu/cpu{}/topology",
                cpu_idx
            )))),
            "topology/core_id" => Ok(Arc::new(SysFileInode::new(&format!("{}\n", cpu_idx)))),
            "topology/physical_package_id" => Ok(Arc::new(SysFileInode::new("0\n"))),
            "topology/core_siblings" => Ok(Arc::new(SysFileInode::new("0000000000000001\n"))),
            _ => Err(FsError::EntryNotFound),
        };
    }

    // /sys/devices/system/cpu/present, possible, online
    match rest {
        "present" => {
            let n = crate::task::scheduler::get_cpu_count();
            Ok(Arc::new(SysFileInode::new(&format!(
                "0-{}\n",
                n.saturating_sub(1)
            ))))
        }
        "possible" => Ok(Arc::new(SysFileInode::new("0-8191\n"))),
        "online" => {
            let n = crate::task::scheduler::get_cpu_count();
            Ok(Arc::new(SysFileInode::new(&format!(
                "0-{}\n",
                n.saturating_sub(1)
            ))))
        }
        _ => Err(FsError::EntryNotFound),
    }
}

// ============================================================================
// GİRİŞ NOKTASI
// ============================================================================

/// Path'e göre /sys inode'u döndürür
pub fn open_sys_inode(path: &str) -> Result<Arc<dyn INode>, FsError> {
    if !is_sys_path(path) {
        return Err(FsError::EntryNotFound);
    }
    lookup_sys_path(path.trim_start_matches('/'))
}

/// Bu path'in /sys kapsamına girip girmediğini kontrol eder
pub fn is_sys_path(path: &str) -> bool {
    path == "/sys" || path.starts_with("/sys/")
}
