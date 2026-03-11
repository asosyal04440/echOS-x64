//! # Cgroups v2 (Kontrol Grupları Sürüm 2)
//!
//! Konteyner ve süreç yalıtımı için kaynak kontrol alt sistemi.
//! Docker, Kubernetes gibi araçların altında yatan çekirdek mekanizması.
//!
//! ## Cgroup Hiyerarşisi
//!
//! ```text
//!  / (kök cgroup)
//!  ├── cpu controller: tüm CPU'ya erişim
//!  ├── pids controller: sınırsız süreç
//!  │
//!  ├── /system.slice
//!  │   ├── cpu.weight = 100  (varsayılan ağırlık)
//!  │   ├── pids.max = 500
//!  │   └── /nginx.service
//!  │       ├── cpu.max = "200000 1000000"  (%20 CPU sınırı)
//!  │       └── [PID 1234, PID 1235]
//!  │
//!  └── /user.slice
//!      ├── cpu.weight = 200  (daha yüksek öncelik)
//!      └── /user-1000.slice
//!          ├── pids.max = 100
//!          └── [PID 5678, PID 5679]
//! ```
//!
//! ## Temel Özellikler
//! - **cpu controller**: CPU bant genişliği ve ağırlık kontrolü
//! - **cpuset controller**: İşlemleri belirli CPU çekirdeklerine sabitler
//! - **pids controller**: Süreç sayısını sınırlar (fork bomb koruması)
//! - **memory controller**: Bellek kullanımını sınırlar
//!
//! ## Dosya Sistemi Arayüzü
//! Her cgroup `/sys/fs/cgroup/` altında bir dizin olarak görünür.
//! Kaynak limitleri bu dizindeki sanal dosyalara yazılarak ayarlanır.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// CGROUP V2 SABİTLERİ
// ============================================================================

/// Desteklenen denetleyiciler (controller'lar)
pub const CGROUP_CONTROLLER_CPU: &str = "cpu";
pub const CGROUP_CONTROLLER_CPUSET: &str = "cpuset";
pub const CGROUP_CONTROLLER_MEMORY: &str = "memory";
pub const CGROUP_CONTROLLER_IO: &str = "io";
pub const CGROUP_CONTROLLER_PIDS: &str = "pids";
pub const CGROUP_CONTROLLER_RDMA: &str = "rdma";
pub const CGROUP_CONTROLLER_HUGETLB: &str = "hugetlb";
pub const CGROUP_CONTROLLER_MISC: &str = "misc";

/// Her cgroup dizininde standart olarak bulunan sözde dosyalar
pub const CGROUP_FILE_CGROUP_TYPE: &str = "cgroup.type";
pub const CGROUP_FILE_CGROUP_PROCS: &str = "cgroup.procs";
pub const CGROUP_FILE_CGROUP_CONTROLLERS: &str = "cgroup.controllers";
pub const CGROUP_FILE_CGROUP_SUBTREE_CONTROL: &str = "cgroup.subtree_control";
pub const CGROUP_FILE_CGROUP_EVENTS: &str = "cgroup.events";
pub const CGROUP_FILE_CGROUP_MAX_DESCENDANTS: &str = "cgroup.max.descendants";
pub const CGROUP_FILE_CGROUP_MAX_DEPTH: &str = "cgroup.max.depth";
pub const CGROUP_FILE_CGROUP_STAT: &str = "cgroup.stat";

// ============================================================================
// CGROUP V2 YAPISI
// ============================================================================

pub struct CgroupV2 {
    /// Cgroup'un benzersiz kimlik numarası
    pub id: u64,
    /// Sanal dosya sistemi yolu (örn. /system.slice/nginx.service)
    pub path: String,
    /// Cgroup'un görünen adı
    pub name: String,
    /// Ebeveyn cgroup'un ID'si (kök için None)
    pub parent: Option<u64>,
    /// Alt cgroup'ların ID listesi
    pub children: Mutex<Vec<u64>>,
    /// Aktif denetleyiciler (cpu, pids, memory vb.)
    pub controllers: Mutex<BTreeMap<String, Box<dyn CgroupController>>>,
    /// Alt ağaçta etkinleştirilecek denetleyiciler (subtree_control)
    pub subtree_control: Mutex<Vec<String>>,
    /// Bu cgroup'a atanan süreç PID'leri
    pub processes: Mutex<Vec<u64>>,
    /// Bu cgroup'a atanan iş parçacığı TID'leri
    pub threads: Mutex<Vec<u64>>,
    /// Cgroup'da en az bir süreç var mı?
    pub populated: AtomicBool,
    /// Cgroup dondurulmuş mu? (tüm süreçler duraklatılır)
    pub frozen: AtomicBool,
    /// Olay bildirimleri (populate/freeze durumu)
    pub events: Mutex<CgroupEvents>,
    /// İstatistikler (torun sayısı vb.)
    pub stats: Mutex<CgroupStats>,
}

#[derive(Clone, Debug, Default)]
pub struct CgroupEvents {
    pub populated: bool,
    pub frozen: bool,
    pub memory_high: bool,
    pub memory_low: bool,
    pub memory_max: bool,
    pub memory_oom: bool,
    pub memory_oom_kill: bool,
}

#[derive(Clone, Debug, Default)]
pub struct CgroupStats {
    pub nr_descendants: u64,
    pub nr_dying_descendants: u64,
}

impl CgroupV2 {
    pub fn new(id: u64, path: &str, name: &str, parent: Option<u64>) -> Self {
        Self {
            id,
            path: String::from(path),
            name: String::from(name),
            parent,
            children: Mutex::new(Vec::new()),
            controllers: Mutex::new(BTreeMap::new()),
            subtree_control: Mutex::new(Vec::new()),
            processes: Mutex::new(Vec::new()),
            threads: Mutex::new(Vec::new()),
            populated: AtomicBool::new(false),
            frozen: AtomicBool::new(false),
            events: Mutex::new(CgroupEvents::default()),
            stats: Mutex::new(CgroupStats::default()),
        }
    }

    /// Cgroup'a süreç ekler. populated bayrağını günceller.
    pub fn add_process(&self, pid: u64) {
        self.processes.lock().push(pid);
        self.populated.store(true, Ordering::SeqCst);
        self.events.lock().populated = true;
    }

    /// Cgroup'dan süreç çıkarır. Boş kalırsa populated bayrağını temizler.
    pub fn remove_process(&self, pid: u64) {
        self.processes.lock().retain(|&p| p != pid);

        if self.processes.lock().is_empty() {
            self.populated.store(false, Ordering::SeqCst);
            self.events.lock().populated = false;
        }
    }

    /// Alt ağaç için denetleyici etkinleştirir (örn. "+cpu" komutu).
    pub fn enable_controller(&self, name: &str) {
        self.subtree_control.lock().push(String::from(name));
    }

    /// Alt ağaç için denetleyiciyi devre dışı bırakır (örn. "-cpu" komutu).
    pub fn disable_controller(&self, name: &str) {
        self.subtree_control.lock().retain(|c| c != name);
    }

    /// Cgroup sanal dosyasına değer yazar.
    /// Örnek: write("cpu.max", "500000 1000000") -> CPU'nun %50'sini sınırla
    pub fn write(&self, file: &str, value: &str) -> Result<(), CgroupError> {
        match file {
            "cgroup.procs" => {
                if let Ok(pid) = value.parse::<u64>() {
                    self.add_process(pid);
                }
            }
            "cgroup.subtree_control" => {
                // Denetleyici listesini ayrıştır ("+cpu -memory" gibi)
                for ctrl in value.split_whitespace() {
                    if ctrl.starts_with('+') {
                        self.enable_controller(&ctrl[1..]);
                    } else if ctrl.starts_with('-') {
                        self.disable_controller(&ctrl[1..]);
                    }
                }
            }
            "cgroup.freeze" => {
                self.frozen.store(value == "1", Ordering::SeqCst);
                self.events.lock().frozen = value == "1";
            }
            _ => {
                // İlgili denetleyiciye yönlendir
                for controller in self.controllers.lock().values_mut() {
                    if controller.handles_file(file) {
                        return controller.write(file, value);
                    }
                }
            }
        }
        Ok(())
    }

    /// Cgroup sanal dosyasından değer okur.
    pub fn read(&self, file: &str) -> Result<String, CgroupError> {
        match file {
            "cgroup.procs" => {
                let procs: Vec<String> = self
                    .processes
                    .lock()
                    .iter()
                    .map(|p| p.to_string())
                    .collect();
                Ok(procs.join("\n"))
            }
            "cgroup.controllers" => Ok(self
                .controllers
                .lock()
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .join(" ")),
            "cgroup.subtree_control" => Ok(self.subtree_control.lock().join(" ")),
            "cgroup.events" => {
                let events = self.events.lock();
                Ok(alloc::format!(
                    "populated {}\nfrozen {}",
                    if events.populated { 1 } else { 0 },
                    if events.frozen { 1 } else { 0 }
                ))
            }
            "cgroup.stat" => {
                let stats = self.stats.lock();
                Ok(alloc::format!(
                    "nr_descendants {}\nnr_dying_descendants {}",
                    stats.nr_descendants,
                    stats.nr_dying_descendants
                ))
            }
            _ => {
                for controller in self.controllers.lock().values() {
                    if controller.handles_file(file) {
                        return controller.read(file);
                    }
                }
                Err(CgroupError::FileNotFound)
            }
        }
    }
}

// ============================================================================
// CGROUP DENETLEYİCİ TRAIT'İ
// ============================================================================
//
// Her kaynak türü (cpu, memory, pids...) bu trait'i uygular.
// Controller'lar cgroup ağacının düğümlerine dinamik olarak bağlanır.

pub trait CgroupController: Send + Sync {
    fn name(&self) -> &str;
    fn handles_file(&self, file: &str) -> bool;
    fn write(&mut self, file: &str, value: &str) -> Result<(), CgroupError>;
    fn read(&self, file: &str) -> Result<String, CgroupError>;
    fn charge(&self, amount: u64) -> Result<(), CgroupError>;
    fn uncharge(&self, amount: u64);
}

// ============================================================================
// CPU DENETLEYİCİSİ
// ============================================================================
//
// cpu.weight  : CFS ağırlığı (1-10000, varsayılan 100)
// cpu.max     : Bant genişliği kısıtlaması "quota period" (örn. "500000 1000000" = %50)
// cpu.max.burst: Kısa süreli limit aşımı izni

pub struct CpuController {
    pub weight: AtomicU64,
    pub max: AtomicU64,
    pub max_burst: AtomicU64,
    pub stat: Mutex<CpuStat>,
}

#[derive(Clone, Debug, Default)]
pub struct CpuStat {
    pub usage_usec: u64,
    pub user_usec: u64,
    pub system_usec: u64,
    pub nr_periods: u64,
    pub nr_throttled: u64,
    pub throttled_usec: u64,
}

impl CpuController {
    pub fn new() -> Self {
        Self {
            weight: AtomicU64::new(100),
            max: AtomicU64::new(u64::MAX),
            max_burst: AtomicU64::new(0),
            stat: Mutex::new(CpuStat::default()),
        }
    }
}

impl CgroupController for CpuController {
    fn name(&self) -> &str {
        "cpu"
    }

    fn handles_file(&self, file: &str) -> bool {
        matches!(
            file,
            "cpu.weight" | "cpu.max" | "cpu.max.burst" | "cpu.stat"
        )
    }

    fn write(&mut self, file: &str, value: &str) -> Result<(), CgroupError> {
        match file {
            "cpu.weight" => {
                if let Ok(w) = value.parse::<u64>() {
                    self.weight.store(w.clamp(1, 10000), Ordering::SeqCst);
                }
            }
            "cpu.max" => {
                // Biçim: "kota period" veya "max" (sınırsız)
                if value == "max" {
                    self.max.store(u64::MAX, Ordering::SeqCst);
                } else {
                    let parts: Vec<&str> = value.split_whitespace().collect();
                    if parts.len() == 2 {
                        if let Ok(quota) = parts[0].parse::<u64>() {
                            self.max.store(quota, Ordering::SeqCst);
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn read(&self, file: &str) -> Result<String, CgroupError> {
        match file {
            "cpu.weight" => Ok(self.weight.load(Ordering::Relaxed).to_string()),
            "cpu.max" => {
                let max = self.max.load(Ordering::Relaxed);
                if max == u64::MAX {
                    Ok(String::from("max 100000"))
                } else {
                    Ok(alloc::format!("{} 100000", max))
                }
            }
            "cpu.stat" => {
                let stat = self.stat.lock();
                Ok(alloc::format!(
                    "usage_usec {}\nuser_usec {}\nsystem_usec {}\nnr_periods {}\nnr_throttled {}\nthrottled_usec {}",
                    stat.usage_usec, stat.user_usec, stat.system_usec,
                    stat.nr_periods, stat.nr_throttled, stat.throttled_usec
                ))
            }
            _ => Err(CgroupError::FileNotFound),
        }
    }

    fn charge(&self, amount: u64) -> Result<(), CgroupError> {
        let mut stat = self.stat.lock();
        stat.usage_usec += amount;
        Ok(())
    }

    fn uncharge(&self, _amount: u64) {}
}

// ============================================================================
// CPUSET DENETLEYİCİSİ
// ============================================================================
//
// cpuset.cpus : İzin verilen CPU çekirdekleri (örn. "0-3,5,7")
// cpuset.mems : İzin verilen NUMA bellek düğümleri (örn. "0,1")
// cpuset.cpus.effective: Gerçekte kullanılan CPU'lar (NUMA toplolojisinden miras alınır)

pub struct CpusetController {
    pub cpus: Mutex<Vec<u32>>,
    pub mems: Mutex<Vec<u32>>,
    pub cpus_effective: Mutex<Vec<u32>>,
    pub mems_effective: Mutex<Vec<u32>>,
    pub partition: AtomicBool,
}

impl CpusetController {
    pub fn new() -> Self {
        Self {
            cpus: Mutex::new(Vec::new()),
            mems: Mutex::new(Vec::new()),
            cpus_effective: Mutex::new(Vec::new()),
            mems_effective: Mutex::new(Vec::new()),
            partition: AtomicBool::new(false),
        }
    }
}

impl CgroupController for CpusetController {
    fn name(&self) -> &str {
        "cpuset"
    }

    fn handles_file(&self, file: &str) -> bool {
        matches!(
            file,
            "cpuset.cpus" | "cpuset.mems" | "cpuset.cpus.effective" | "cpuset.mems.effective"
        )
    }

    fn write(&mut self, file: &str, value: &str) -> Result<(), CgroupError> {
        match file {
            "cpuset.cpus" => {
                // CPU listesini ayrıştır (örn. "0-3,5,7")
                let mut cpus = Vec::new();
                for part in value.split(',') {
                    if part.contains('-') {
                        let range: Vec<&str> = part.split('-').collect();
                        if let (Ok(start), Ok(end)) =
                            (range[0].parse::<u32>(), range[1].parse::<u32>())
                        {
                            for cpu in start..=end {
                                cpus.push(cpu);
                            }
                        }
                    } else if let Ok(cpu) = part.parse::<u32>() {
                        cpus.push(cpu);
                    }
                }
                *self.cpus.lock() = cpus;
            }
            "cpuset.mems" => {
                // NUMA bellek düğümü listesini ayrıştır
                let mut mems = Vec::new();
                for part in value.split(',') {
                    if let Ok(mem) = part.trim().parse::<u32>() {
                        mems.push(mem);
                    }
                }
                *self.mems.lock() = mems;
            }
            _ => {}
        }
        Ok(())
    }

    fn read(&self, file: &str) -> Result<String, CgroupError> {
        match file {
            "cpuset.cpus" => {
                let cpus = self.cpus.lock();
                Ok(cpus
                    .iter()
                    .map(|c| c.to_string())
                    .collect::<Vec<_>>()
                    .join(","))
            }
            "cpuset.mems" => {
                let mems = self.mems.lock();
                Ok(mems
                    .iter()
                    .map(|m| m.to_string())
                    .collect::<Vec<_>>()
                    .join(","))
            }
            _ => Err(CgroupError::FileNotFound),
        }
    }

    fn charge(&self, _amount: u64) -> Result<(), CgroupError> {
        Ok(())
    }
    fn uncharge(&self, _amount: u64) {}
}

// ============================================================================
// PIDs DENETLEYİCİSİ
// ============================================================================
//
// Fork bomb saldırılarına karşı koruma sağlar.
// pids.max    : Cgroup içindeki maksimum süreç+iş parçacığı sayısı
// pids.current: Şu anki süreç+iş parçacığı sayısı
// pids.events : Sınır aşım olayları sayacı

pub struct PidsController {
    pub max: AtomicI64,
    pub current: AtomicU64,
    pub events: Mutex<PidsEvents>,
}

#[derive(Clone, Debug, Default)]
pub struct PidsEvents {
    pub max: u64,
}

impl PidsController {
    pub fn new() -> Self {
        Self {
            max: AtomicI64::new(-1), // -1 = sınırsız
            current: AtomicU64::new(0),
            events: Mutex::new(PidsEvents::default()),
        }
    }
}

impl CgroupController for PidsController {
    fn name(&self) -> &str {
        "pids"
    }

    fn handles_file(&self, file: &str) -> bool {
        matches!(file, "pids.max" | "pids.current" | "pids.events")
    }

    fn write(&mut self, file: &str, value: &str) -> Result<(), CgroupError> {
        match file {
            "pids.max" => {
                if value == "max" {
                    self.max.store(-1, Ordering::SeqCst);
                } else if let Ok(limit) = value.parse::<i64>() {
                    self.max.store(limit, Ordering::SeqCst);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn read(&self, file: &str) -> Result<String, CgroupError> {
        match file {
            "pids.max" => {
                let max = self.max.load(Ordering::Relaxed);
                if max < 0 {
                    Ok(String::from("max"))
                } else {
                    Ok(max.to_string())
                }
            }
            "pids.current" => Ok(self.current.load(Ordering::Relaxed).to_string()),
            "pids.events" => Ok(alloc::format!("max {}", self.events.lock().max)),
            _ => Err(CgroupError::FileNotFound),
        }
    }

    fn charge(&self, _amount: u64) -> Result<(), CgroupError> {
        let current = self.current.fetch_add(1, Ordering::SeqCst) + 1;
        let max = self.max.load(Ordering::Relaxed);

        if max >= 0 && current > max as u64 {
            self.current.fetch_sub(1, Ordering::SeqCst);
            self.events.lock().max += 1;
            return Err(CgroupError::LimitExceeded);
        }
        Ok(())
    }

    fn uncharge(&self, _amount: u64) {
        self.current.fetch_sub(1, Ordering::SeqCst);
    }
}

// ============================================================================
// CGROUP V2 YÖNETİCİSİ
// ============================================================================
//
// Tüm cgroup hiyerarşisini yönetir: oluşturma, silme, yol eşleme.
// `path_map` ile O(1) yol araması sağlanır.

pub struct CgroupV2Manager {
    cgroups: Mutex<BTreeMap<u64, Arc<CgroupV2>>>,
    path_map: Mutex<BTreeMap<String, u64>>,
    next_id: AtomicU64,
    root: AtomicU64,
}

impl CgroupV2Manager {
    pub const fn new() -> Self {
        Self {
            cgroups: Mutex::new(BTreeMap::new()),
            path_map: Mutex::new(BTreeMap::new()),
            next_id: AtomicU64::new(1),
            root: AtomicU64::new(0),
        }
    }

    pub fn init(&self) {
        let root = Arc::new(CgroupV2::new(0, "/", "root", None));

        // Kök cgroup'a temel denetleyicileri ekle
        root.controllers
            .lock()
            .insert(String::from("cpu"), Box::new(CpuController::new()));
        root.controllers
            .lock()
            .insert(String::from("cpuset"), Box::new(CpusetController::new()));
        root.controllers
            .lock()
            .insert(String::from("pids"), Box::new(PidsController::new()));

        self.cgroups.lock().insert(0, root);
        self.path_map.lock().insert(String::from("/"), 0);

        crate::serial_println!("[CGROUPV2] cgroup v2 başlatıldı");
    }

    pub fn create(&self, parent_path: &str, name: &str) -> Result<Arc<CgroupV2>, CgroupError> {
        let parent_id = self
            .path_map
            .lock()
            .get(parent_path)
            .copied()
            .ok_or(CgroupError::NotFound)?;

        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let path = alloc::format!("{}/{}", parent_path.trim_end_matches('/'), name);

        let cgroup = Arc::new(CgroupV2::new(id, &path, name, Some(parent_id)));

        // Ebeveyn cgroup'a alt olarak ekle
        if let Some(parent) = self.cgroups.lock().get(&parent_id) {
            parent.children.lock().push(id);
        }

        self.cgroups.lock().insert(id, cgroup.clone());
        self.path_map.lock().insert(path, id);

        Ok(cgroup)
    }

    pub fn get(&self, path: &str) -> Option<Arc<CgroupV2>> {
        let id = self.path_map.lock().get(path).copied()?;
        self.cgroups.lock().get(&id).cloned()
    }
}

lazy_static::lazy_static! {
    pub static ref CGROUP_V2: CgroupV2Manager = CgroupV2Manager::new();
}

// ============================================================================
// HATA TÜRÜ
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CgroupError {
    NotFound,
    FileNotFound,
    LimitExceeded,
    PermissionDenied,
    InvalidValue,
}

// ============================================================================
// BAŞLATMA
// ============================================================================

pub fn init() {
    CGROUP_V2.init();
}

// ============================================================================
// FREEZER CONTROLLER — İşlem Dondurma
// ============================================================================

/// Cgroup freezer controller — işlem grubunu dondur/çöz.
///
/// Container duraklatma ve SIGSTOP benzeri davranış için kullanılır.
#[derive(Debug, Clone)]
pub struct FreezerController {
    /// Dondurulmuş mu
    pub frozen: bool,
    /// Kendi kendine dondurulmuş (kullanıcı isteğiyle)
    pub self_freezing: bool,
    /// Ebeveyn dondurduğu için donmuş
    pub parent_freezing: bool,
}

impl FreezerController {
    pub fn new() -> Self {
        Self {
            frozen: false,
            self_freezing: false,
            parent_freezing: false,
        }
    }

    /// İşlem grubunu dondurur.
    pub fn freeze(&mut self) {
        self.self_freezing = true;
        self.frozen = true;
        crate::serial_println!("[cgroup-freezer] Grup donduruldu");
    }

    /// İşlem grubunun dondurmasını çözer.
    pub fn thaw(&mut self) {
        self.self_freezing = false;
        if !self.parent_freezing {
            self.frozen = false;
        }
        crate::serial_println!("[cgroup-freezer] Grup çözüldü");
    }

    /// Ebeveyn freeze propagasyonu.
    pub fn parent_freeze(&mut self, freeze: bool) {
        self.parent_freezing = freeze;
        self.frozen = freeze || self.self_freezing;
    }

    /// Durum string'i.
    pub fn state_str(&self) -> &str {
        if self.frozen {
            "FROZEN"
        } else {
            "THAWED"
        }
    }
}
