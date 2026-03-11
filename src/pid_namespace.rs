//! # PID Namespace — Süreç Kimlik İzolasyonu
//!
//! Linux PID namespace uyumlu süreç izolasyonu. Konteyner (container)
//! ve sandbox ortamları için PID alanı ayrıştırması sağlar.
//!
//! ## Mimari
//!
//! ```text
//! Host PID Namespace (pid_ns=0)
//! ├── PID 1 (init)
//! ├── PID 100 (shell)
//! ├── PID 200 (container runtime)
//! │   └── Child PID Namespace (pid_ns=1)
//! │       ├── PID 1 (container init)   ← host PID 201
//! │       ├── PID 2 (app)              ← host PID 202
//! │       └── PID 3 (worker)           ← host PID 203
//! └── PID 300 (container2)
//!     └── Child PID Namespace (pid_ns=2)
//!         ├── PID 1 (container init)   ← host PID 301
//!         └── PID 2 (service)          ← host PID 302
//! ```
//!
//! ## Kurallar
//!
//! 1. Her namespace'in kendi PID 1'i (init) vardır
//! 2. PID 1 ölürse tüm namespace yok edilir
//! 3. Üst namespace alt namespace'in PID'lerini görebilir
//! 4. Alt namespace üst namespace'i göremez
//! 5. `getpid()` namespace-local PID döner

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;

// ============================================================================
// PID Namespace Yapısı
// ============================================================================

/// PID namespace ID tipi
pub type PidNsId = u32;

/// Host PID tipi (gerçek kernel PID)
pub type HostPid = u32;

/// Namespace-local PID tipi (container içi PID)
pub type LocalPid = u32;

/// Tek bir PID namespace
#[derive(Clone, Debug)]
pub struct PidNamespace {
    /// Namespace ID (benzersiz)
    pub ns_id: PidNsId,
    /// Üst namespace ID (root=0 için None)
    pub parent_ns: Option<PidNsId>,
    /// İnsan okunabilir isim
    pub name: String,
    /// Sonraki local PID (namespace-local PID allocator)
    next_local_pid: u32,
    /// Local PID → Host PID eşlemesi
    local_to_host: BTreeMap<LocalPid, HostPid>,
    /// Host PID → Local PID eşlemesi (ters çözünürlük)
    host_to_local: BTreeMap<HostPid, LocalPid>,
    /// Namespace aktif mi?
    pub active: bool,
}

impl PidNamespace {
    pub fn new(ns_id: PidNsId, name: &str, parent_ns: Option<PidNsId>) -> Self {
        Self {
            ns_id,
            parent_ns,
            name: String::from(name),
            next_local_pid: 1, // PID 1 = namespace init
            local_to_host: BTreeMap::new(),
            host_to_local: BTreeMap::new(),
            active: true,
        }
    }

    /// Namespace'e yeni süreç ekler ve local PID atar.
    pub fn alloc_pid(&mut self, host_pid: HostPid) -> LocalPid {
        let local_pid = self.next_local_pid;
        self.next_local_pid += 1;

        self.local_to_host.insert(local_pid, host_pid);
        self.host_to_local.insert(host_pid, local_pid);

        local_pid
    }

    /// Local PID'den Host PID'e çevir.
    pub fn to_host_pid(&self, local_pid: LocalPid) -> Option<HostPid> {
        self.local_to_host.get(&local_pid).copied()
    }

    /// Host PID'den Local PID'e çevir.
    pub fn to_local_pid(&self, host_pid: HostPid) -> Option<LocalPid> {
        self.host_to_local.get(&host_pid).copied()
    }

    /// Süreci namespace'den çıkar.
    pub fn remove_pid(&mut self, host_pid: HostPid) -> Option<LocalPid> {
        if let Some(local_pid) = self.host_to_local.remove(&host_pid) {
            self.local_to_host.remove(&local_pid);

            // PID 1 çıkarsa namespace'i deaktive et
            if local_pid == 1 {
                self.active = false;
                crate::serial_println!(
                    "[PID-NS:{}] Init (PID 1) exited — namespace deactivated",
                    self.ns_id
                );
            }

            Some(local_pid)
        } else {
            None
        }
    }

    /// Namespace'teki tüm süreçleri listele.
    pub fn list_pids(&self) -> Vec<(LocalPid, HostPid)> {
        self.local_to_host.iter().map(|(&l, &h)| (l, h)).collect()
    }

    /// Toplam süreç sayısı.
    pub fn process_count(&self) -> usize {
        self.local_to_host.len()
    }
}

// ============================================================================
// Global PID Namespace Registry
// ============================================================================

/// Sonraki namespace ID
static NEXT_NS_ID: AtomicU32 = AtomicU32::new(1);

/// Sonraki host PID
static NEXT_HOST_PID: AtomicU32 = AtomicU32::new(1);

lazy_static::lazy_static! {
    /// Tüm PID namespace'lerin merkezi kaydı
    static ref PID_NS_TABLE: Mutex<BTreeMap<PidNsId, PidNamespace>> = {
        let mut table = BTreeMap::new();
        // Root (host) namespace
        let mut root = PidNamespace::new(0, "host", None);
        // PID 0 = idle, PID 1 = init (önceden atanmış)
        root.alloc_pid(0); // PID 1 → host PID 0 (idle)
        table.insert(0, root);
        Mutex::new(table)
    };

    /// Host PID → Namespace ID eşlemesi
    static ref HOST_PID_NS: Mutex<BTreeMap<HostPid, PidNsId>> = {
        let mut map = BTreeMap::new();
        map.insert(0, 0); // Host PID 0 → root namespace
        Mutex::new(map)
    };
}

/// Yeni bir PID namespace oluşturur.
///
/// `parent_ns`: Üst namespace (genellikle 0 = host)
/// Döner: (namespace_id, init sürecin host_pid'i)
pub fn create_namespace(
    name: &str,
    parent_ns: PidNsId,
) -> Result<(PidNsId, HostPid), &'static str> {
    let ns_id = NEXT_NS_ID.fetch_add(1, Ordering::Relaxed);

    let mut table = PID_NS_TABLE.lock();

    // Üst namespace var mı?
    if !table.contains_key(&parent_ns) {
        return Err("Parent namespace not found");
    }

    let mut ns = PidNamespace::new(ns_id, name, Some(parent_ns));

    // Namespace'in init sürecini (PID 1) oluştur
    let init_host_pid = NEXT_HOST_PID.fetch_add(1, Ordering::Relaxed);
    let local_pid = ns.alloc_pid(init_host_pid);
    assert_eq!(local_pid, 1, "First PID in namespace must be 1");

    table.insert(ns_id, ns);

    drop(table);
    HOST_PID_NS.lock().insert(init_host_pid, ns_id);

    crate::serial_println!(
        "[PID-NS] Created namespace '{}' (id={}, parent={}, init_host_pid={})",
        name,
        ns_id,
        parent_ns,
        init_host_pid
    );

    Ok((ns_id, init_host_pid))
}

/// Namespace'e yeni süreç ekler.
///
/// Döner: (local_pid, host_pid)
pub fn fork_in_namespace(ns_id: PidNsId) -> Result<(LocalPid, HostPid), &'static str> {
    let host_pid = NEXT_HOST_PID.fetch_add(1, Ordering::Relaxed);

    let mut table = PID_NS_TABLE.lock();
    let ns = table.get_mut(&ns_id).ok_or("Namespace not found")?;

    if !ns.active {
        return Err("Namespace is not active (init exited)");
    }

    let local_pid = ns.alloc_pid(host_pid);

    drop(table);
    HOST_PID_NS.lock().insert(host_pid, ns_id);

    Ok((local_pid, host_pid))
}

/// Süreç namespace'den çıkar (exit/kill).
pub fn exit_pid(host_pid: HostPid) {
    let ns_id = match HOST_PID_NS.lock().remove(&host_pid) {
        Some(id) => id,
        None => return,
    };

    let mut table = PID_NS_TABLE.lock();
    if let Some(ns) = table.get_mut(&ns_id) {
        if let Some(local_pid) = ns.remove_pid(host_pid) {
            crate::serial_println!(
                "[PID-NS:{}] Process exited: local_pid={} host_pid={}",
                ns_id,
                local_pid,
                host_pid
            );

            // Namespace deaktive edildiyse tüm süreçleri öldür
            if !ns.active {
                let remaining: Vec<HostPid> = ns.host_to_local.keys().copied().collect();
                for hpid in remaining {
                    ns.remove_pid(hpid);
                    HOST_PID_NS.lock().remove(&hpid);
                    crate::serial_println!("[PID-NS:{}] Killing orphan: host_pid={}", ns_id, hpid);
                }
            }
        }
    }
}

/// `getpid()` implementasyonu: namespace-local PID döner.
pub fn getpid(host_pid: HostPid) -> LocalPid {
    let ns_id = match HOST_PID_NS.lock().get(&host_pid) {
        Some(&id) => id,
        None => return host_pid, // Namespace yoksa host PID döner
    };

    let table = PID_NS_TABLE.lock();
    if let Some(ns) = table.get(&ns_id) {
        ns.to_local_pid(host_pid).unwrap_or(host_pid)
    } else {
        host_pid
    }
}

/// Bir namespace'teki tüm süreçleri listele.
pub fn list_namespace_pids(ns_id: PidNsId) -> Vec<(LocalPid, HostPid)> {
    let table = PID_NS_TABLE.lock();
    if let Some(ns) = table.get(&ns_id) {
        ns.list_pids()
    } else {
        Vec::new()
    }
}

/// Tüm namespace'leri listele.
pub fn list_namespaces() -> Vec<(PidNsId, String, usize)> {
    PID_NS_TABLE
        .lock()
        .iter()
        .map(|(&id, ns)| (id, ns.name.clone(), ns.process_count()))
        .collect()
}

/// PID namespace bilgisini yazdır.
pub fn print_namespace_info(ns_id: PidNsId) {
    let table = PID_NS_TABLE.lock();
    if let Some(ns) = table.get(&ns_id) {
        crate::serial_println!("=== PID Namespace '{}' (id={}) ===", ns.name, ns.ns_id);
        crate::serial_println!("  Parent: {:?}", ns.parent_ns);
        crate::serial_println!("  Active: {}", ns.active);
        crate::serial_println!("  Processes: {}", ns.process_count());
        for (local, host) in ns.list_pids() {
            crate::serial_println!("    PID {} → host PID {}", local, host);
        }
    }
}

/// PID namespace alt sistemini başlatır.
pub fn init() {
    crate::serial_println!("[PID-NS] PID namespace subsystem initialized");
    crate::serial_println!("[PID-NS]   Host namespace: id=0 (root)");
    crate::serial_println!("[PID-NS]   Supports: nested namespaces, PID translation, init reaping");
}
