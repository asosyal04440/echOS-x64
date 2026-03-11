//! # procfs — /proc Sanal Dosya Sistemi
//!
//! Linux tarzı `/proc` sanal dosya sisteminin echOS implementasyonu.
//! Donanım ve çalışma zamanı bilgilerini sanal dosyalar olarak sunar.
//!
//! ## Desteklenen Dosyalar
//!
//! | Yol                    | İçerik                                        |
//! |------------------------|-----------------------------------------------|
//! | /proc/cpuinfo          | İşlemci modeli, çekirdek sayısı, özellik bayrakları |
//! | /proc/meminfo          | Toplam/serbest RAM, swap, slab istatistikleri |
//! | /proc/uptime           | Sistem çalışma süresi (saniye)                |
//! | /proc/version          | Kernel sürüm bilgisi                          |
//! | /proc/cmdline          | Kernel komut satırı parametreleri             |
//! | /proc/filesystems      | Kayıtlı dosya sistemleri                      |
//! | /proc/mounts           | Aktif bağlama noktaları                       |
//! | /proc/interrupts       | IRQ istatistikleri                            |
//! | /proc/self/maps        | Mevcut sürecin VMA haritası                   |
//! | /proc/self/status      | Mevcut sürecin durumu                         |
//! | /proc/self/fd          | Mevcut sürecin açık dosya tanımlayıcıları     |

use alloc::format;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use rcore_fs::vfs::{FileType, FsError, FsInfo, INode, Metadata, PollStatus, Timespec};
use spin::Mutex;

use crate::drivers::ata::BLOCK_SIZE;

// ============================================================================
// KERNEL SÜRÜM BİLGİSİ
// ============================================================================

pub const ECHOS_VERSION: &str = "echOS 0.1.0-alpha";
pub const KERNEL_BUILD_DATE: &str = "2026-03-01";

// ============================================================================
// YARDIMCI: META VERİ OLUŞTURUCULAR
// ============================================================================

fn proc_file_meta(size: usize) -> Metadata {
    Metadata {
        dev: 1,
        inode: 0,
        size,
        blk_size: BLOCK_SIZE,
        blocks: 0,
        atime: Timespec { sec: 0, nsec: 0 },
        mtime: Timespec { sec: 0, nsec: 0 },
        ctime: Timespec { sec: 0, nsec: 0 },
        type_: FileType::File,
        mode: 0o100444, // r--r--r--
        nlinks: 1,
        uid: 0,
        gid: 0,
        rdev: 0,
    }
}

fn proc_dir_meta() -> Metadata {
    Metadata {
        dev: 1,
        inode: 0,
        size: 0,
        blk_size: BLOCK_SIZE,
        blocks: 0,
        atime: Timespec { sec: 0, nsec: 0 },
        mtime: Timespec { sec: 0, nsec: 0 },
        ctime: Timespec { sec: 0, nsec: 0 },
        type_: FileType::Dir,
        mode: 0o040555, // r-xr-xr-x
        nlinks: 2,
        uid: 0,
        gid: 0,
        rdev: 0,
    }
}

// ============================================================================
// SANAL İÇERİK ÜRETİCİLER
// ============================================================================

/// /proc/cpuinfo içeriğini üretir
pub fn gen_cpuinfo() -> String {
    let info = crate::cpu::get_cpu_info();
    let mut out = String::new();

    // Her mantıksal CPU için bir blok
    let cpus = crate::task::scheduler::get_cpu_count() as usize;
    for i in 0..cpus {
        out.push_str(&format!("processor\t: {}\n", i));
        out.push_str(&format!("vendor_id\t: {}\n", info.vendor_str()));
        out.push_str(&format!("model name\t: {}\n", info.brand_string()));
        out.push_str(&format!("cpu MHz\t\t: {}\n", info.freq_mhz()));
        out.push_str(&format!("cache size\t: {} KB\n", info.l2_cache_kb()));
        out.push_str("flags\t\t: fpu vme de pse tsc msr pae mce cx8 apic");
        if info.has_sse2() {
            out.push_str(" sse2");
        }
        if info.has_avx_feat() {
            out.push_str(" avx");
        }
        if info.has_aes() {
            out.push_str(" aes");
        }
        if info.has_rdrand() {
            out.push_str(" rdrand");
        }
        out.push('\n');
        out.push('\n');
    }
    out
}

/// /proc/meminfo içeriğini üretir
pub fn gen_meminfo() -> String {
    let stats = crate::memory::get_memory_stats();
    format!(
        "MemTotal:       {:>10} kB\n\
         MemFree:        {:>10} kB\n\
         MemAvailable:   {:>10} kB\n\
         Buffers:        {:>10} kB\n\
         Cached:         {:>10} kB\n\
         SwapCached:     {:>10} kB\n\
         Active:         {:>10} kB\n\
         Inactive:       {:>10} kB\n\
         SwapTotal:      {:>10} kB\n\
         SwapFree:       {:>10} kB\n\
         Slab:           {:>10} kB\n\
         PageTables:     {:>10} kB\n",
        stats.total_kb,
        stats.free_kb,
        stats.available_kb,
        stats.buffers_kb,
        stats.cached_kb,
        stats.swap_cached_kb,
        stats.active_kb,
        stats.inactive_kb,
        stats.swap_total_kb,
        stats.swap_free_kb,
        stats.slab_kb,
        stats.page_tables_kb,
    )
}

/// /proc/uptime içeriğini üretir
pub fn gen_uptime() -> String {
    let secs = crate::drivers::rtc::get_unix_time();
    format!("{}.00 {}.00\n", secs, secs)
}

/// /proc/version içeriğini üretir
pub fn gen_version() -> String {
    format!("{} (Rust nightly 2025) #1 SMP 2026-03-01\n", ECHOS_VERSION)
}

/// /proc/cmdline içeriğini üretir
pub fn gen_cmdline() -> String {
    String::from("console=ttyS0 loglevel=3 quiet\n")
}

/// /proc/filesystems içeriğini üretir
pub fn gen_filesystems() -> String {
    String::from(
        "nodev\tprocfs\n\
         nodev\tdevfs\n\
         nodev\tsysfs\n\
         \tf2fs\n\
         \text4\n\
         \tfat32\n\
         \tntfs\n",
    )
}

/// /proc/mounts içeriğini üretir
pub fn gen_mounts() -> String {
    String::from(
        "procfs /proc procfs rw,nosuid,nodev,noexec,relatime 0 0\n\
         devfs /dev devfs rw,nosuid,noexec,relatime 0 0\n\
         sysfs /sys sysfs rw,nosuid,nodev,noexec,relatime 0 0\n\
         /dev/nvme0n1p1 / f2fs rw,relatime 0 1\n",
    )
}

/// /proc/interrupts içeriğini üretir
pub fn gen_interrupts() -> String {
    let mut out = String::from(
        "           CPU0\n\
         0:          261   IO-APIC   2-edge     timer\n\
         1:            9   IO-APIC   1-edge     i8042\n\
         8:            0   IO-APIC   8-edge     rtc0\n\
         9:            0   IO-APIC   9-fasteoi  acpi\n\
         12:           4   IO-APIC  12-edge     i8042\n\
         16:           0   IO-APIC  16-fasteoi  ehci_hcd:usb1\n\
         17:           0   IO-APIC  17-fasteoi  amd7xxx, ehci_hcd:usb2\n\
         NMI:          0   Non-maskable interrupts\n\
         LOC:       1024   Local timer interrupts\n\
         IWI:          0   IRQ work interrupts\n\
         RES:          0   Rescheduling interrupts\n",
    );
    out
}

/// /proc/self/status içeriğini üretir
/// Gerçek görev verilerini kullanır
pub fn gen_self_status() -> String {
    // Try to get real task data from scheduler
    let tasks = crate::task::scheduler::list_tasks();
    let current_task = tasks.first();

    let (name, pid, state, priority) = if let Some(task) = current_task {
        (
            task.name,
            task.pid,
            match task.state {
                crate::task::TaskState::Ready => "R (running)",
                crate::task::TaskState::Running => "R (running)",
                crate::task::TaskState::Blocked => "S (sleeping)",
                crate::task::TaskState::Sleeping { .. } => "S (sleeping)",
                crate::task::TaskState::Stopped => "T (stopped)",
                crate::task::TaskState::Zombie => "Z (zombie)",
                crate::task::TaskState::Terminated => "X (dead)",
            },
            task.priority,
        )
    } else {
        // Fallback to defaults
        (
            "echOS-shell",
            1,
            "R (running)",
            crate::task::Priority::Normal,
        )
    };

    let (vm_peak, vm_size, vm_rss) = {
        // Get memory stats
        let mem_stats = crate::memory::get_memory_stats();
        let heap_kb = (mem_stats.total_kb.saturating_sub(mem_stats.free_kb)) as u32;
        (heap_kb + 4096, heap_kb + 4096, heap_kb / 4)
    };

    format!(
        "Name:\t{}\n\
         State:\t{}\n\
         Tgid:\t{}\n\
         Pid:\t{}\n\
         PPid:\t0\n\
         TracerPid:\t0\n\
         Uid:\t0\t0\t0\t0\n\
         Gid:\t0\t0\t0\t0\n\
         VmPeak:\t{} kB\n\
         VmSize:\t{} kB\n\
         VmRSS:\t{} kB\n\
         Threads:\t1\n\
         SigBlk:\t0000000000000000\n\
         SigIgn:\t0000000000000000\n\
         CapInh:\t0000000000000000\n\
         CapPrm:\t0000003fffffffff\n\
         CapEff:\t0000003fffffffff\n",
        name, state, pid, pid, vm_peak, vm_size, vm_rss,
    )
}

/// /proc/self/maps içeriğini üretir
pub fn gen_self_maps() -> String {
    use crate::memory::{KERNEL_HEAP_BASE, KERNEL_HEAP_SIZE};
    format!(
        "{:016x}-{:016x} rw-p 00000000 00:00 0  [heap]\n\
         ffff800000000000-ffff800100000000 r--p 00000000 00:00 0  [hhdm]\n\
         ffffffff80000000-ffffffff81000000 r-xp 00000000 00:00 0  [kernel]\n",
        KERNEL_HEAP_BASE,
        KERNEL_HEAP_BASE + KERNEL_HEAP_SIZE as u64,
    )
}

/// /proc/self/fd içeriğini üretir (açık dosya tanımlayıcı listesi)
pub fn gen_self_fd() -> String {
    String::from(
        "0 -> /dev/stdin\n\
         1 -> /dev/stdout\n\
         2 -> /dev/stderr\n",
    )
}

/// /proc/stat içeriğini üretir (CPU istatistikleri)
pub fn gen_stat() -> String {
    format!(
        "cpu  {} {} {} {} 0 0 0 0 0 0\n\
         cpu0 {} {} {} {} 0 0 0 0 0 0\n\
         intr 0\n\
         ctxt 0\n\
         btime 0\n\
         processes 1\n\
         procs_running 1\n\
         procs_blocked 0\n",
        100, 0, 50, 1000, 100, 0, 50, 1000,
    )
}

/// /proc/loadavg içeriğini üretir
pub fn gen_loadavg() -> String {
    String::from("0.00 0.00 0.00 1/1 1\n")
}

/// /proc/driver/tier içeriğini üretir — TIER 1/TIER 2 sürücü durumları
pub fn gen_driver_tier() -> String {
    let mut s = String::from("=== echOS Two-Tier Driver Caste System ===\n\n");
    s.push_str("TIER 1 (Lock-Free Native):\n");
    s.push_str("  NVMe     : Active  (lock-free, DMA-based)\n");
    s.push_str("  NIC      : Active  (lock-free, zero-copy)\n");
    s.push_str("  GPU      : Active  (lock-free, MMIO)\n");
    s.push_str("\nTIER 2 (Jail Sandbox):\n");
    s.push_str("  USB-XHCI : Jailed  (SPSC ring, crash-isolated)\n");
    s.push_str("  USB-MSC  : Jailed  (BBB/SCSI, crash-isolated)\n");
    s.push_str("  Audio    : Jailed  (HDA codec, crash-isolated)\n");
    s.push_str("  WiFi     : Jailed  (VirtIO-WiFi, crash-isolated)\n");
    s.push_str("  Bluetooth: Jailed  (HCI, crash-isolated)\n");
    s
}

/// /proc/driver/nvme içeriğini üretir — NVMe sürücü bilgileri
pub fn gen_driver_nvme() -> String {
    let mut s = String::from("=== NVMe Controller Info ===\n");
    let info = crate::drivers::nvme::get_controller_info();
    if info.is_empty() {
        s.push_str("No NVMe controllers found\n");
    } else {
        for (idx, io_queues, namespaces) in &info {
            s.push_str(&format!("\nController {}:\n", idx));
            s.push_str(&format!("  I/O Queues: {}\n", io_queues));
            for ns in namespaces {
                s.push_str(&format!(
                    "  NS {}: {} blocks x {} bytes = {} MB\n",
                    ns.nsid,
                    ns.block_count,
                    ns.block_size,
                    ns.capacity_bytes / (1024 * 1024)
                ));
            }
        }
    }
    s
}

/// /proc/driver/gpu içeriğini üretir — GPU sürücü bilgileri
pub fn gen_driver_gpu() -> String {
    let count = crate::drivers::gpu_native::device_count();
    let mut s = String::from("=== GPU Native Driver Info ===\n");
    s.push_str(&format!("Devices: {}\n", count));
    s.push_str("Tier: 1 (Lock-Free Native)\n");
    s
}

// ============================================================================
// INODE UYGULAMASI
// ============================================================================

/// Sabit içerikli bir procfs dosya inode'u
pub struct ProcFileInode {
    pub name: &'static str,
    pub generator: fn() -> String,
}

impl INode for ProcFileInode {
    fn read_at(&self, offset: usize, buf: &mut [u8]) -> Result<usize, FsError> {
        let content = (self.generator)();
        let bytes = content.as_bytes();
        if offset >= bytes.len() {
            return Ok(0);
        }
        let available = &bytes[offset..];
        let to_copy = available.len().min(buf.len());
        buf[..to_copy].copy_from_slice(&available[..to_copy]);
        Ok(to_copy)
    }

    fn write_at(&self, _offset: usize, _buf: &[u8]) -> Result<usize, FsError> {
        Err(FsError::NotSupported) // /proc files are read-only
    }

    fn poll(&self) -> Result<PollStatus, FsError> {
        Ok(PollStatus {
            read: true,
            write: false,
            error: false,
        })
    }

    fn metadata(&self) -> Result<Metadata, FsError> {
        let content = (self.generator)();
        Ok(proc_file_meta(content.len()))
    }

    fn as_any_ref(&self) -> &dyn core::any::Any {
        self
    }
}

/// /proc ve /proc/self dizin inode'u
pub struct ProcDirInode {
    pub path: &'static str,
}

impl INode for ProcDirInode {
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
        Ok(proc_dir_meta())
    }

    fn find(&self, name: &str) -> Result<Arc<dyn INode>, FsError> {
        self.lookup(name)
    }

    fn as_any_ref(&self) -> &dyn core::any::Any {
        self
    }
}

impl ProcDirInode {
    fn lookup(&self, name: &str) -> Result<Arc<dyn INode>, FsError> {
        match self.path {
            "/proc" | "/" => match name {
                "cpuinfo" => Ok(Arc::new(ProcFileInode {
                    name: "cpuinfo",
                    generator: gen_cpuinfo,
                })),
                "meminfo" => Ok(Arc::new(ProcFileInode {
                    name: "meminfo",
                    generator: gen_meminfo,
                })),
                "uptime" => Ok(Arc::new(ProcFileInode {
                    name: "uptime",
                    generator: gen_uptime,
                })),
                "version" => Ok(Arc::new(ProcFileInode {
                    name: "version",
                    generator: gen_version,
                })),
                "cmdline" => Ok(Arc::new(ProcFileInode {
                    name: "cmdline",
                    generator: gen_cmdline,
                })),
                "filesystems" => Ok(Arc::new(ProcFileInode {
                    name: "filesystems",
                    generator: gen_filesystems,
                })),
                "mounts" => Ok(Arc::new(ProcFileInode {
                    name: "mounts",
                    generator: gen_mounts,
                })),
                "interrupts" => Ok(Arc::new(ProcFileInode {
                    name: "interrupts",
                    generator: gen_interrupts,
                })),
                "stat" => Ok(Arc::new(ProcFileInode {
                    name: "stat",
                    generator: gen_stat,
                })),
                "loadavg" => Ok(Arc::new(ProcFileInode {
                    name: "loadavg",
                    generator: gen_loadavg,
                })),
                "self" => Ok(Arc::new(ProcDirInode { path: "/proc/self" })),
                "driver" => Ok(Arc::new(ProcDirInode {
                    path: "/proc/driver",
                })),
                _ => Err(FsError::EntryNotFound),
            },
            "/proc/self" => match name {
                "status" => Ok(Arc::new(ProcFileInode {
                    name: "status",
                    generator: gen_self_status,
                })),
                "maps" => Ok(Arc::new(ProcFileInode {
                    name: "maps",
                    generator: gen_self_maps,
                })),
                "fd" => Ok(Arc::new(ProcFileInode {
                    name: "fd",
                    generator: gen_self_fd,
                })),
                _ => Err(FsError::EntryNotFound),
            },
            "/proc/driver" => match name {
                "tier" => Ok(Arc::new(ProcFileInode {
                    name: "tier",
                    generator: gen_driver_tier,
                })),
                "nvme" => Ok(Arc::new(ProcFileInode {
                    name: "nvme",
                    generator: gen_driver_nvme,
                })),
                "gpu" => Ok(Arc::new(ProcFileInode {
                    name: "gpu",
                    generator: gen_driver_gpu,
                })),
                _ => Err(FsError::EntryNotFound),
            },
            _ => Err(FsError::EntryNotFound),
        }
    }
}

// ============================================================================
// PROCFS GİRİŞ NOKTASI
// ============================================================================

/// Path'e göre /proc inode'u döndürür.
/// Örnek: `open_proc_inode("/proc/meminfo")` → ProcFileInode { generator: gen_meminfo }
pub fn open_proc_inode(path: &str) -> Result<Arc<dyn INode>, FsError> {
    // Normalize
    let path = path.trim_start_matches('/');
    let parts: Vec<&str> = path.splitn(3, '/').collect();

    // /proc
    if parts.is_empty() || parts[0] != "proc" {
        return Err(FsError::EntryNotFound);
    }

    if parts.len() == 1 {
        // /proc itself
        return Ok(Arc::new(ProcDirInode { path: "/proc" }));
    }

    let entry = parts[1];

    // /proc/self
    if entry == "self" {
        if parts.len() == 2 {
            return Ok(Arc::new(ProcDirInode { path: "/proc/self" }));
        }
        let sub = parts[2];
        return ProcDirInode { path: "/proc/self" }.lookup(sub);
    }

    // /proc/driver
    if entry == "driver" {
        if parts.len() == 2 {
            return Ok(Arc::new(ProcDirInode {
                path: "/proc/driver",
            }));
        }
        let sub = parts[2];
        return ProcDirInode {
            path: "/proc/driver",
        }
        .lookup(sub);
    }

    // /proc/<file>
    ProcDirInode { path: "/proc" }.lookup(entry)
}

/// Bu path'in /proc kapsamına girip girmediğini kontrol eder
pub fn is_proc_path(path: &str) -> bool {
    path == "/proc" || path.starts_with("/proc/")
}
