//! # cgroups v2 — Kontrol Grupları
//!
//! Linux cgroups v2 uyumlu kaynak yönetim sistemi.
//! Süreç gruplarını CPU, bellek ve I/O bant genişliği ile sınırlar.
//!
//! ## Hiyerarşi
//!
//! ```text
//! /sys/fs/cgroup/
//! ├── cgroup.controllers    (cpu memory io)
//! ├── cgroup.subtree_control
//! ├── system.slice/
//! │   ├── cpu.max           (100000 100000) → %100 CPU
//! │   ├── memory.max        (268435456)     → 256 MB
//! │   ├── memory.current    (okunur)
//! │   ├── io.max            (8:0 rbps=1048576)
//! │   └── cgroup.procs      (PID listesi)
//! └── user.slice/
//!     ├── cpu.max
//!     └── memory.max
//! ```
//!
//! ## Desteklenen Kontrolörler
//!
//! - **cpu**: CPU bant genişliği sınırlama (cpu.max, cpu.weight)
//! - **memory**: Bellek kullanım sınırlama (memory.max, memory.current, memory.high)
//! - **io**: Blok I/O bant genişliği sınırlama (io.max, io.stat)
//! - **pids**: Süreç sayısı sınırlama (pids.max, pids.current)

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

// ============================================================================
// Cgroup Kontrolörleri
// ============================================================================

/// CPU kontrolör limitleri
#[derive(Clone, Debug)]
pub struct CpuController {
    /// cpu.max: quota ve period (mikrosaniye)
    /// quota/period = izin verilen CPU oranı
    /// Örn: 50000/100000 = %50 CPU
    pub quota_us: u64,
    pub period_us: u64,
    /// cpu.weight: göreceli ağırlık (1-10000, varsayılan 100)
    pub weight: u32,
    /// İstatistik: toplam kullanılan CPU zamanı (ns)
    pub usage_ns: u64,
    /// Throttle sayısı (bütçe aşıldığında)
    pub throttled_count: u64,
}

impl CpuController {
    fn new() -> Self {
        Self {
            quota_us: 100_000,  // Varsayılan: sınırsız (max)
            period_us: 100_000, // 100ms periyot
            weight: 100,
            usage_ns: 0,
            throttled_count: 0,
        }
    }

    /// CPU kullanım yüzdesini hesaplar
    pub fn usage_percent(&self) -> f64 {
        if self.period_us == 0 {
            return 0.0;
        }
        (self.quota_us as f64 / self.period_us as f64) * 100.0
    }
}

/// Memory kontrolör limitleri
#[derive(Clone, Debug)]
pub struct MemoryController {
    /// memory.max: maksimum bellek kullanımı (byte; 0 = sınırsız)
    pub max_bytes: u64,
    /// memory.high: yüksek bellek eşiği (throttle başlar)
    pub high_bytes: u64,
    /// memory.current: mevcut kullanım (byte)
    pub current_bytes: u64,
    /// OOM kill sayısı
    pub oom_kills: u64,
}

impl MemoryController {
    fn new() -> Self {
        Self {
            max_bytes: u64::MAX, // Varsayılan: sınırsız
            high_bytes: u64::MAX,
            current_bytes: 0,
            oom_kills: 0,
        }
    }
}

/// I/O kontrolör limitleri
#[derive(Clone, Debug)]
pub struct IoController {
    /// io.max: maksimum okuma hızı (byte/s; 0 = sınırsız)
    pub read_bps_max: u64,
    /// io.max: maksimum yazma hızı (byte/s)
    pub write_bps_max: u64,
    /// io.max: maksimum okuma IOPS
    pub read_iops_max: u64,
    /// io.max: maksimum yazma IOPS
    pub write_iops_max: u64,
    /// İstatistik: toplam okunan byte
    pub read_bytes: u64,
    /// İstatistik: toplam yazılan byte
    pub write_bytes: u64,
}

impl IoController {
    fn new() -> Self {
        Self {
            read_bps_max: u64::MAX,
            write_bps_max: u64::MAX,
            read_iops_max: u64::MAX,
            write_iops_max: u64::MAX,
            read_bytes: 0,
            write_bytes: 0,
        }
    }
}

/// PID kontrolör limitleri
#[derive(Clone, Debug)]
pub struct PidsController {
    /// pids.max: maksimum süreç sayısı
    pub max_pids: u32,
    /// pids.current: mevcut süreç sayısı
    pub current_pids: u64,
}

impl PidsController {
    fn new() -> Self {
        Self {
            max_pids: 4096,
            current_pids: 0,
        }
    }
}

// ============================================================================
// Cgroup Yapısı
// ============================================================================

/// Bir control group — süreç grubunun kaynak limitleri
#[derive(Clone, Debug)]
pub struct Cgroup {
    /// Cgroup ID (benzersiz)
    pub id: u32,
    /// Cgroup adı (ör: "system.slice")
    pub name: String,
    /// Üst cgroup ID (root=0 için None)
    pub parent_id: Option<u32>,
    /// CPU kontrolörü (aktifse)
    pub cpu: Option<CpuController>,
    /// Memory kontrolörü (aktifse)
    pub memory: Option<MemoryController>,
    /// I/O kontrolörü (aktifse)
    pub io: Option<IoController>,
    /// PID kontrolörü (aktifse)
    pub pids: Option<PidsController>,
    /// Bu gruba ait süreç PID'leri
    pub procs: Vec<u32>,
    /// Aktif kontrolörler
    pub controllers: Vec<String>,
}

impl Cgroup {
    pub fn new(id: u32, name: &str, parent_id: Option<u32>) -> Self {
        Self {
            id,
            name: String::from(name),
            parent_id,
            cpu: Some(CpuController::new()),
            memory: Some(MemoryController::new()),
            io: Some(IoController::new()),
            pids: Some(PidsController::new()),
            procs: Vec::new(),
            controllers: alloc::vec![
                String::from("cpu"),
                String::from("memory"),
                String::from("io"),
                String::from("pids"),
            ],
        }
    }
}

// ============================================================================
// Global Cgroup Registry
// ============================================================================

lazy_static::lazy_static! {
    /// Tüm cgroup'ların merkezi kaydı: id → Cgroup
    static ref CGROUP_TREE: Mutex<BTreeMap<u32, Cgroup>> = {
        let mut tree = BTreeMap::new();
        // Root cgroup (id=0)
        tree.insert(0, Cgroup::new(0, "/", None));
        Mutex::new(tree)
    };

    /// PID → cgroup_id eşlemesi
    static ref PID_CGROUP_MAP: Mutex<BTreeMap<u32, u32>> = Mutex::new(BTreeMap::new());

    /// Sonraki cgroup ID
    static ref NEXT_CGROUP_ID: Mutex<u32> = Mutex::new(1);
}

/// Yeni bir cgroup oluşturur.
pub fn create_cgroup(name: &str, parent_id: u32) -> Result<u32, &'static str> {
    let mut tree = CGROUP_TREE.lock();

    // Üst cgroup var mı?
    if !tree.contains_key(&parent_id) {
        return Err("Parent cgroup not found");
    }

    let id = {
        let mut next = NEXT_CGROUP_ID.lock();
        let id = *next;
        *next += 1;
        id
    };

    let cg = Cgroup::new(id, name, Some(parent_id));
    tree.insert(id, cg);

    crate::serial_println!(
        "[cgroup] Created: '{}' (id={}, parent={})",
        name,
        id,
        parent_id
    );

    Ok(id)
}

/// Süreci bir cgroup'a ekler.
pub fn attach_pid(cgroup_id: u32, pid: u32) -> Result<(), &'static str> {
    let mut tree = CGROUP_TREE.lock();
    let cg = tree.get_mut(&cgroup_id).ok_or("Cgroup not found")?;

    // PID limit kontrolü
    if let Some(ref mut pids) = cg.pids {
        let current = pids.current_pids as u32;
        if current >= pids.max_pids {
            return Err("PID limit exceeded");
        }
        pids.current_pids += 1;
    }

    if !cg.procs.contains(&pid) {
        cg.procs.push(pid);
    }

    drop(tree);
    PID_CGROUP_MAP.lock().insert(pid, cgroup_id);

    crate::serial_println!("[cgroup] PID {} attached to cgroup {}", pid, cgroup_id);

    Ok(())
}

/// Sürecin cgroup'unu döner.
pub fn get_pid_cgroup(pid: u32) -> Option<u32> {
    PID_CGROUP_MAP.lock().get(&pid).copied()
}

/// CPU limiti ayarlar: quota_us / period_us
pub fn set_cpu_max(cgroup_id: u32, quota_us: u64, period_us: u64) -> Result<(), &'static str> {
    let mut tree = CGROUP_TREE.lock();
    let cg = tree.get_mut(&cgroup_id).ok_or("Cgroup not found")?;

    if let Some(ref mut cpu) = cg.cpu {
        cpu.quota_us = quota_us;
        cpu.period_us = period_us;
        crate::serial_println!("[cgroup:{}] cpu.max = {}/{}", cg.name, quota_us, period_us);
        Ok(())
    } else {
        Err("CPU controller not enabled")
    }
}

/// Memory limiti ayarlar
pub fn set_memory_max(cgroup_id: u32, max_bytes: u64) -> Result<(), &'static str> {
    let mut tree = CGROUP_TREE.lock();
    let cg = tree.get_mut(&cgroup_id).ok_or("Cgroup not found")?;

    if let Some(ref mut mem) = cg.memory {
        mem.max_bytes = max_bytes;
        crate::serial_println!(
            "[cgroup:{}] memory.max = {} bytes ({} MB)",
            cg.name,
            max_bytes,
            max_bytes / (1024 * 1024)
        );
        Ok(())
    } else {
        Err("Memory controller not enabled")
    }
}

/// I/O limiti ayarlar
pub fn set_io_max(cgroup_id: u32, rbps: u64, wbps: u64) -> Result<(), &'static str> {
    let mut tree = CGROUP_TREE.lock();
    let cg = tree.get_mut(&cgroup_id).ok_or("Cgroup not found")?;

    if let Some(ref mut io) = cg.io {
        io.read_bps_max = rbps;
        io.write_bps_max = wbps;
        Ok(())
    } else {
        Err("IO controller not enabled")
    }
}

/// PID limiti ayarlar
pub fn set_pids_max(cgroup_id: u32, max_pids: u32) -> Result<(), &'static str> {
    let mut tree = CGROUP_TREE.lock();
    let cg = tree.get_mut(&cgroup_id).ok_or("Cgroup not found")?;

    if let Some(ref mut pids) = cg.pids {
        pids.max_pids = max_pids;
        Ok(())
    } else {
        Err("PIDs controller not enabled")
    }
}

/// Bellek ayırma kontrolü: cgroup bellek limitini aşıyor mu?
///
/// Bellek ayırıcı bu fonksiyonu çağırarak cgroup limitlerini denetler.
pub fn check_memory_limit(pid: u32, alloc_bytes: u64) -> bool {
    let cgroup_id = match PID_CGROUP_MAP.lock().get(&pid) {
        Some(&id) => id,
        None => return true, // Cgroup yoksa izin ver
    };

    let mut tree = CGROUP_TREE.lock();
    let cg = match tree.get_mut(&cgroup_id) {
        Some(cg) => cg,
        None => return true,
    };

    if let Some(ref mut mem) = cg.memory {
        let current = mem.current_bytes;
        if current + alloc_bytes > mem.max_bytes {
            mem.oom_kills += 1;
            crate::serial_println!(
                "[cgroup:{}] OOM: pid={} tried to alloc {} bytes (current={}, max={})",
                cg.name,
                pid,
                alloc_bytes,
                current,
                mem.max_bytes
            );
            return false;
        }
    }

    true
}

/// Bellek kullanımını günceller
pub fn account_memory(pid: u32, bytes: i64) {
    let cgroup_id = match PID_CGROUP_MAP.lock().get(&pid) {
        Some(&id) => id,
        None => return,
    };

    let mut tree = CGROUP_TREE.lock();
    if let Some(cg) = tree.get_mut(&cgroup_id) {
        if let Some(ref mut mem) = cg.memory {
            if bytes > 0 {
                mem.current_bytes += bytes as u64;
            } else {
                let abs = (-bytes) as u64;
                mem.current_bytes = mem.current_bytes.saturating_sub(abs);
            }
        }
    }
}

/// Tüm cgroup'ları listeler
pub fn list_cgroups() -> Vec<(u32, String)> {
    CGROUP_TREE
        .lock()
        .iter()
        .map(|(&id, cg)| (id, cg.name.clone()))
        .collect()
}

/// Cgroup istatistiklerini yazdırır
pub fn print_cgroup_stats(cgroup_id: u32) {
    let tree = CGROUP_TREE.lock();
    if let Some(cg) = tree.get(&cgroup_id) {
        crate::serial_println!("=== Cgroup '{}' (id={}) ===", cg.name, cg.id);
        crate::serial_println!("  procs: {:?}", cg.procs);
        if let Some(ref cpu) = cg.cpu {
            crate::serial_println!(
                "  cpu.max: {}/{} weight={}",
                cpu.quota_us,
                cpu.period_us,
                cpu.weight
            );
            crate::serial_println!("  cpu.usage: {} ns", cpu.usage_ns);
        }
        if let Some(ref mem) = cg.memory {
            crate::serial_println!("  memory.max: {} bytes", mem.max_bytes);
            crate::serial_println!("  memory.current: {} bytes", mem.current_bytes);
            crate::serial_println!("  memory.oom_kills: {}", mem.oom_kills);
        }
        if let Some(ref io) = cg.io {
            crate::serial_println!("  io.read: {} bytes", io.read_bytes);
            crate::serial_println!("  io.write: {} bytes", io.write_bytes);
        }
        if let Some(ref pids) = cg.pids {
            crate::serial_println!("  pids.max: {}", pids.max_pids);
            crate::serial_println!("  pids.current: {}", pids.current_pids);
        }
    }
}

/// cgroups v2 alt sistemini başlatır.
pub fn init() {
    crate::serial_println!("[cgroups] v2 control group subsystem initialized");
    crate::serial_println!("[cgroups]   Controllers: cpu, memory, io, pids");
    crate::serial_println!("[cgroups]   Root cgroup: / (id=0)");
}
