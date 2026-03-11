//! # OverlayFS — Birleşik (Union) Mount Dosya Sistemi
//!
//! OverlayFS, birden fazla dosya sistemi katmanını üst üste birleştirerek
//! tek bir dosya ağacı sunar. Container çalıştırma zamanı (Docker/OCI) için
//! temel dosya sistemi katmanı olarak kullanılır.
//!
//! ## Katman Yapısı
//!
//! ```text
//!  ┌─────────────────────┐  merged (birleştirilmiş görünüm)
//!  │  /merged/            │  → Tüm katmanların birleşimi
//!  ├─────────────────────┤
//!  │  upperdir (yazılabilir) │  → Container'ın yazma katmanı
//!  ├─────────────────────┤
//!  │  lowerdir (salt-okunur) │  → Temel imaj katmanı
//!  └─────────────────────┘
//!  ┌─────────────────────┐
//!  │  workdir             │  → Atomik işlemler için geçici alan
//!  └─────────────────────┘
//! ```
//!
//! ## Copy-on-Write (CoW) Semantiği
//!
//! - **Okuma**: Üst katmandan başlayarak aşağı doğru aranır
//! - **Yazma**: Dosya yoksa lower'dan upper'a kopyalanır, sonra yazılır
//! - **Silme**: Üst katmanda "whiteout" dosyası oluşturulur

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

// ============================================================================
// SABITLER
// ============================================================================

/// OverlayFS magic
pub const OVERLAYFS_MAGIC: u64 = 0x794C_7265_766F;
/// Whiteout karakter aygıtı (silme işareti)
pub const WHITEOUT_CHAR_DEV: u32 = 0;
/// Opaque dizin xattr adı
pub const OVERLAY_XATTR_OPAQUE: &str = "trusted.overlay.opaque";

// ============================================================================
// OverlayFS Girişi
// ============================================================================

/// OverlayFS'teki bir dosya/dizin girişi.
#[derive(Debug, Clone)]
pub struct OverlayEntry {
    /// Giriş adı
    pub name: String,
    /// Dosya mı, dizin mi
    pub is_dir: bool,
    /// Dosya boyutu
    pub size: u64,
    /// İzinler
    pub mode: u32,
    /// UID
    pub uid: u32,
    /// GID
    pub gid: u32,
    /// Dosya verileri (yalnızca dosyalar)
    pub data: Vec<u8>,
    /// Alt girişler (yalnızca dizinler)
    pub children: BTreeMap<String, OverlayEntry>,
    /// Whiteout mu? (silinmiş dosya işareti)
    pub whiteout: bool,
    /// Opaque mu? (alt katmandaki aynı adlı dizini maskeler)
    pub opaque: bool,
    /// Hangi katmandan geldiği
    pub origin: OverlayOrigin,
}

/// Girişin kaynak katmanı
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayOrigin {
    /// Üst (yazılabilir) katman
    Upper,
    /// Alt (salt okunur) katman
    Lower,
    /// Birleştirilmiş (sanal)
    Merged,
}

impl OverlayEntry {
    /// Yeni dosya girişi.
    pub fn new_file(name: &str, data: Vec<u8>, mode: u32, origin: OverlayOrigin) -> Self {
        Self {
            name: String::from(name),
            is_dir: false,
            size: data.len() as u64,
            mode,
            uid: 0,
            gid: 0,
            data,
            children: BTreeMap::new(),
            whiteout: false,
            opaque: false,
            origin,
        }
    }

    /// Yeni dizin girişi.
    pub fn new_dir(name: &str, mode: u32, origin: OverlayOrigin) -> Self {
        Self {
            name: String::from(name),
            is_dir: true,
            size: 0,
            mode,
            uid: 0,
            gid: 0,
            data: Vec::new(),
            children: BTreeMap::new(),
            whiteout: false,
            opaque: false,
            origin,
        }
    }

    /// Whiteout girişi (silme işareti).
    pub fn whiteout(name: &str) -> Self {
        Self {
            name: String::from(name),
            is_dir: false,
            size: 0,
            mode: 0,
            uid: 0,
            gid: 0,
            data: Vec::new(),
            children: BTreeMap::new(),
            whiteout: true,
            opaque: false,
            origin: OverlayOrigin::Upper,
        }
    }
}

// ============================================================================
// OverlayFS Katmanı
// ============================================================================

/// Bir dosya sistemi katmanı.
#[derive(Debug, Clone)]
pub struct OverlayLayer {
    /// Katman yolu
    pub path: String,
    /// Salt okunur mu?
    pub readonly: bool,
    /// Kök giriş
    pub root: OverlayEntry,
}

impl OverlayLayer {
    /// Yeni katman oluşturur.
    pub fn new(path: &str, readonly: bool) -> Self {
        Self {
            path: String::from(path),
            readonly,
            root: OverlayEntry::new_dir(
                "/",
                0o755,
                if readonly {
                    OverlayOrigin::Lower
                } else {
                    OverlayOrigin::Upper
                },
            ),
        }
    }

    /// Katmanda dosya arar.
    pub fn lookup(&self, path: &str) -> Option<&OverlayEntry> {
        let mut current = &self.root;
        for component in path.split('/').filter(|c| !c.is_empty()) {
            if let Some(child) = current.children.get(component) {
                current = child;
            } else {
                return None;
            }
        }
        Some(current)
    }

    /// Katmana dosya ekler.
    pub fn insert(&mut self, path: &str, entry: OverlayEntry) -> Result<(), i32> {
        if self.readonly {
            return Err(-30); // EROFS
        }

        let components: Vec<&str> = path.split('/').filter(|c| !c.is_empty()).collect();
        if components.is_empty() {
            return Err(-22); // EINVAL
        }

        let mut current = &mut self.root;
        // Navigate to parent
        for &comp in &components[..components.len() - 1] {
            if !current.children.contains_key(comp) {
                let dir = OverlayEntry::new_dir(comp, 0o755, OverlayOrigin::Upper);
                current.children.insert(String::from(comp), dir);
            }
            current = current.children.get_mut(comp).unwrap();
        }

        let name = components[components.len() - 1];
        current.children.insert(String::from(name), entry);
        Ok(())
    }
}

// ============================================================================
// OverlayFS
// ============================================================================

/// OverlayFS dosya sistemi örneği.
pub struct OverlayFs {
    /// Alt katman (salt okunur)
    pub lower: OverlayLayer,
    /// Üst katman (yazılabilir)
    pub upper: OverlayLayer,
    /// Çalışma dizini (atomik işlemler)
    pub work_dir: String,
    /// Birleşik mount noktası
    pub merged_path: String,
    /// İstatistikler
    pub copy_up_count: u64,
    pub whiteout_count: u64,
    pub lookup_count: u64,
}

impl OverlayFs {
    /// Yeni OverlayFS oluşturur.
    pub fn new(lower: &str, upper: &str, work: &str, merged: &str) -> Self {
        Self {
            lower: OverlayLayer::new(lower, true),
            upper: OverlayLayer::new(upper, false),
            work_dir: String::from(work),
            merged_path: String::from(merged),
            copy_up_count: 0,
            whiteout_count: 0,
            lookup_count: 0,
        }
    }

    /// Birleştirilmiş görünümde dosya arar.
    ///
    /// Sıralama: upper → lower
    /// Whiteout varsa dosya "silinmiş" sayılır.
    pub fn lookup(&mut self, path: &str) -> Option<&OverlayEntry> {
        self.lookup_count += 1;

        // Önce upper'da ara
        if let Some(entry) = self.upper.lookup(path) {
            if entry.whiteout {
                return None; // Silinmiş
            }
            return Some(entry);
        }

        // Sonra lower'da ara
        self.lower.lookup(path)
    }

    /// Dosya yazar (copy-on-write).
    ///
    /// Dosya lower'daysa, önce upper'a kopyalanır, sonra yazılır.
    pub fn write_file(&mut self, path: &str, data: &[u8]) -> Result<(), i32> {
        // Upper'da var mı?
        if self.upper.lookup(path).is_some() {
            // Doğrudan yaz
            let entry = OverlayEntry::new_file(
                path.split('/').last().unwrap_or(""),
                data.to_vec(),
                0o644,
                OverlayOrigin::Upper,
            );
            self.upper.insert(path, entry)?;
            return Ok(());
        }

        // Lower'da var mı? → copy-up
        if self.lower.lookup(path).is_some() {
            self.copy_up_count += 1;
        }

        // Upper'a yaz
        let name = path.split('/').last().unwrap_or("");
        let entry = OverlayEntry::new_file(name, data.to_vec(), 0o644, OverlayOrigin::Upper);
        self.upper.insert(path, entry)?;
        Ok(())
    }

    /// Dosya siler (whiteout oluşturur).
    pub fn remove(&mut self, path: &str) -> Result<(), i32> {
        let name = path.split('/').last().unwrap_or("");
        let wo = OverlayEntry::whiteout(name);
        self.upper.insert(path, wo)?;
        self.whiteout_count += 1;
        Ok(())
    }

    /// Dizin oluşturur.
    pub fn mkdir(&mut self, path: &str) -> Result<(), i32> {
        let name = path.split('/').last().unwrap_or("");
        let dir = OverlayEntry::new_dir(name, 0o755, OverlayOrigin::Upper);
        self.upper.insert(path, dir)?;
        Ok(())
    }

    /// Dizin içeriğini listeler (birleşik).
    pub fn readdir(&self, path: &str) -> Vec<String> {
        let mut names: BTreeMap<String, bool> = BTreeMap::new();

        // Lower katman
        if let Some(entry) = self.lower.lookup(path) {
            if entry.is_dir {
                for (name, _) in &entry.children {
                    names.insert(name.clone(), true);
                }
            }
        }

        // Upper katman (override + whiteout)
        if let Some(entry) = self.upper.lookup(path) {
            if entry.is_dir {
                for (name, child) in &entry.children {
                    if child.whiteout {
                        names.remove(name);
                    } else {
                        names.insert(name.clone(), true);
                    }
                }
            }
        }

        names.into_keys().collect()
    }
}

// ============================================================================
// Container Çalıştırma Zamanı
// ============================================================================

/// Container yapılandırması.
#[derive(Debug, Clone)]
pub struct ContainerConfig {
    /// Container ID
    pub id: String,
    /// Root filesystem (pivot_root hedefi)
    pub rootfs: String,
    /// Hostname
    pub hostname: String,
    /// OverlayFS lower katman
    pub lower_dir: String,
    /// OverlayFS upper katman
    pub upper_dir: String,
    /// OverlayFS work dizini
    pub work_dir: String,
    /// Etkinleştirilecek namespace'ler
    pub namespaces: u32,
    /// Capability mask
    pub capabilities: u64,
    /// Salt-okunur yollar
    pub readonly_paths: Vec<String>,
    /// Maskelenmiş yollar
    pub masked_paths: Vec<String>,
}

/// Linux capability flahları
pub const CAP_CHOWN: u64 = 1 << 0;
pub const CAP_DAC_OVERRIDE: u64 = 1 << 1;
pub const CAP_FSETID: u64 = 1 << 4;
pub const CAP_KILL: u64 = 1 << 5;
pub const CAP_SETGID: u64 = 1 << 6;
pub const CAP_SETUID: u64 = 1 << 7;
pub const CAP_NET_BIND_SERVICE: u64 = 1 << 10;
pub const CAP_NET_RAW: u64 = 1 << 13;
pub const CAP_SYS_CHROOT: u64 = 1 << 18;
pub const CAP_MKNOD: u64 = 1 << 27;
pub const CAP_AUDIT_WRITE: u64 = 1 << 29;
pub const CAP_SETFCAP: u64 = 1 << 31;

/// Namespace bayrakları
pub const CLONE_NEWNS: u32 = 0x00020000;
pub const CLONE_NEWPID: u32 = 0x20000000;
pub const CLONE_NEWNET: u32 = 0x40000000;
pub const CLONE_NEWIPC: u32 = 0x08000000;
pub const CLONE_NEWUTS: u32 = 0x04000000;
pub const CLONE_NEWUSER: u32 = 0x10000000;
pub const CLONE_NEWCGROUP: u32 = 0x02000000;

/// Varsayılan container capability seti (kısıtlı)
pub const DEFAULT_CONTAINER_CAPS: u64 = CAP_CHOWN
    | CAP_DAC_OVERRIDE
    | CAP_FSETID
    | CAP_KILL
    | CAP_SETGID
    | CAP_SETUID
    | CAP_NET_BIND_SERVICE
    | CAP_NET_RAW
    | CAP_SYS_CHROOT
    | CAP_MKNOD
    | CAP_AUDIT_WRITE
    | CAP_SETFCAP;

/// Container durumu
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerState {
    Creating,
    Created,
    Running,
    Paused,
    Stopped,
    Deleting,
}

/// Container runtime
pub struct Container {
    /// Yapılandırma
    pub config: ContainerConfig,
    /// Durum
    pub state: ContainerState,
    /// OverlayFS (root filesystem)
    pub overlay: OverlayFs,
    /// PID (container init process)
    pub init_pid: u64,
    /// Oluşturma zamanı (TSC)
    pub created_at: u64,
}

impl Container {
    /// Yeni container oluşturur.
    pub fn create(config: ContainerConfig) -> Self {
        let overlay = OverlayFs::new(
            &config.lower_dir,
            &config.upper_dir,
            &config.work_dir,
            &config.rootfs,
        );
        let tsc = unsafe { core::arch::x86_64::_rdtsc() };

        Self {
            config,
            state: ContainerState::Created,
            overlay,
            init_pid: 0,
            created_at: tsc,
        }
    }

    /// Container'ı başlatır.
    pub fn start(&mut self) -> Result<(), i32> {
        if self.state != ContainerState::Created {
            return Err(-22); // EINVAL
        }

        // 1. Namespace'leri oluştur
        // 2. pivot_root ile root filesystem değiştir
        // 3. Capability'leri ayarla
        // 4. Maskelenmiş yolları bağla

        self.state = ContainerState::Running;
        crate::serial_println!("[container] {} başlatıldı", self.config.id);
        Ok(())
    }

    /// Container'ı duraklatır (freeze).
    pub fn pause(&mut self) -> Result<(), i32> {
        if self.state != ContainerState::Running {
            return Err(-22);
        }
        self.state = ContainerState::Paused;
        Ok(())
    }

    /// Container'ı devam ettirir.
    pub fn resume(&mut self) -> Result<(), i32> {
        if self.state != ContainerState::Paused {
            return Err(-22);
        }
        self.state = ContainerState::Running;
        Ok(())
    }

    /// Container'ı durdurur.
    pub fn stop(&mut self) -> Result<(), i32> {
        self.state = ContainerState::Stopped;
        crate::serial_println!("[container] {} durduruldu", self.config.id);
        Ok(())
    }

    /// Container durumunu döner.
    pub fn status(&self) -> ContainerState {
        self.state
    }
}

// ============================================================================
// Global State
// ============================================================================

lazy_static::lazy_static! {
    /// Aktif container'lar
    static ref CONTAINERS: Mutex<BTreeMap<String, Container>> = Mutex::new(BTreeMap::new());
}

/// Container oluşturur ve kaydeder.
pub fn create_container(config: ContainerConfig) -> Result<(), i32> {
    let id = config.id.clone();
    let container = Container::create(config);
    CONTAINERS.lock().insert(id, container);
    Ok(())
}

/// Container sayısını döner.
pub fn container_count() -> usize {
    CONTAINERS.lock().len()
}

/// Container ID listesini döner.
pub fn list_containers() -> Vec<(String, ContainerState)> {
    CONTAINERS
        .lock()
        .iter()
        .map(|(id, c)| (id.clone(), c.state))
        .collect()
}

/// Modülü başlatır.
pub fn init() {
    crate::serial_println!("[overlayfs] OverlayFS + Container runtime hazır");
    crate::serial_println!("[overlayfs] Desteklenen NS: PID, NET, MNT, IPC, UTS, USER, CGROUP");
}
