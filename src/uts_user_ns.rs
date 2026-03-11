//! # UTS Namespace + User Namespace
//!
//! Linux container izolasyonu için iki kritik namespace türü:
//!
//! ## UTS Namespace (CLONE_NEWUTS)
//!
//! Her container'in kendi hostname/domainname değerine sahip olmasını sağlar.
//! uname() syscall'ı namespace-aware çalışır.
//!
//! ## User Namespace (CLONE_NEWUSER)
//!
//! UID/GID izolasyonu — container içindeki root (uid=0) dışarıda unprivileged
//! kullanıcıya eşlenir. uid_map/gid_map dosyaları ile mapping yapılır.
//!
//! ```text
//! Host:          Container:
//! uid=1000 ───── uid=0 (root)
//! uid=1001 ───── uid=1
//! gid=1000 ───── gid=0
//! ```

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;

// ============================================================================
// UTS Namespace
// ============================================================================

/// CLONE_NEWUTS flag
pub const CLONE_NEWUTS: u64 = 0x04000000;
/// CLONE_NEWUSER flag
pub const CLONE_NEWUSER: u64 = 0x10000000;

/// UTS Namespace — hostname/domainname izolasyonu
#[derive(Clone, Debug)]
pub struct UtsNamespace {
    /// Namespace ID
    pub ns_id: u32,
    /// Parent namespace (None = init)
    pub parent_ns: Option<u32>,
    /// Hostname (max 64 bayt)
    pub hostname: String,
    /// Domain name (NIS domain, max 64 bayt)
    pub domainname: String,
    /// Sistem adı (uname -s)
    pub sysname: String,
    /// Çekirdek sürümü (uname -r)
    pub release: String,
    /// Yapı sürümü (uname -v)
    pub version: String,
    /// Makine mimarisi (uname -m)
    pub machine: String,
}

impl UtsNamespace {
    /// Yeni UTS namespace oluşturur
    pub fn new(ns_id: u32) -> Self {
        Self {
            ns_id,
            parent_ns: None,
            hostname: String::from("echOS"),
            domainname: String::from("(none)"),
            sysname: String::from("echOS"),
            release: String::from("0.3.0"),
            version: String::from("#1 SMP PREEMPT_DYNAMIC"),
            machine: String::from("x86_64"),
        }
    }

    /// Parent'tan fork eder
    pub fn fork_from(ns_id: u32, parent: &UtsNamespace) -> Self {
        Self {
            ns_id,
            parent_ns: Some(parent.ns_id),
            hostname: parent.hostname.clone(),
            domainname: parent.domainname.clone(),
            sysname: parent.sysname.clone(),
            release: parent.release.clone(),
            version: parent.version.clone(),
            machine: parent.machine.clone(),
        }
    }

    /// sethostname(2)
    pub fn set_hostname(&mut self, name: &str) {
        if name.len() <= 64 {
            self.hostname = String::from(name);
            crate::serial_println!("[UTS ns={}] hostname set: {}", self.ns_id, name);
        }
    }

    /// setdomainname(2)
    pub fn set_domainname(&mut self, name: &str) {
        if name.len() <= 64 {
            self.domainname = String::from(name);
            crate::serial_println!("[UTS ns={}] domainname set: {}", self.ns_id, name);
        }
    }
}

// ============================================================================
// User Namespace
// ============================================================================

/// UID/GID mapping girdisi
#[derive(Clone, Debug)]
pub struct IdMapping {
    /// Container içindeki başlangıç ID
    pub inner_id: u32,
    /// Host tarafındaki başlangıç ID
    pub outer_id: u32,
    /// Mapping aralığı uzunluğu
    pub count: u32,
}

impl IdMapping {
    /// Container (inner) ID → Host (outer) ID çevirisi
    pub fn inner_to_outer(&self, inner: u32) -> Option<u32> {
        if inner >= self.inner_id && inner < self.inner_id + self.count {
            Some(self.outer_id + (inner - self.inner_id))
        } else {
            None
        }
    }

    /// Host (outer) ID → Container (inner) ID çevirisi
    pub fn outer_to_inner(&self, outer: u32) -> Option<u32> {
        if outer >= self.outer_id && outer < self.outer_id + self.count {
            Some(self.inner_id + (outer - self.outer_id))
        } else {
            None
        }
    }
}

/// User Namespace — UID/GID izolasyonu
#[derive(Clone, Debug)]
pub struct UserNamespace {
    /// Namespace ID
    pub ns_id: u32,
    /// Parent namespace
    pub parent_ns: Option<u32>,
    /// UID mapping tablosu (birden fazla mapping olabilir)
    pub uid_map: Vec<IdMapping>,
    /// GID mapping tablosu
    pub gid_map: Vec<IdMapping>,
    /// Namespace sahibinin host UID'si
    pub owner_uid: u32,
    /// Namespace sahibinin host GID'si
    pub owner_gid: u32,
    /// setgroups izni ("allow" veya "deny")
    pub setgroups_allowed: bool,
}

impl UserNamespace {
    pub fn new(ns_id: u32, owner_uid: u32, owner_gid: u32) -> Self {
        Self {
            ns_id,
            parent_ns: None,
            uid_map: Vec::new(),
            gid_map: Vec::new(),
            owner_uid,
            owner_gid,
            setgroups_allowed: false, // Güvenlik: başlangıçta kapalı
        }
    }

    /// Init user namespace (host — identity mapping)
    pub fn init_ns() -> Self {
        let mut ns = Self::new(0, 0, 0);
        // Identity mapping: inner 0 → outer 0, 65536 adet
        ns.uid_map.push(IdMapping {
            inner_id: 0,
            outer_id: 0,
            count: 65536,
        });
        ns.gid_map.push(IdMapping {
            inner_id: 0,
            outer_id: 0,
            count: 65536,
        });
        ns.setgroups_allowed = true;
        ns
    }

    /// UID mapping ekler (uid_map yazma)
    pub fn add_uid_mapping(
        &mut self,
        inner: u32,
        outer: u32,
        count: u32,
    ) -> Result<(), &'static str> {
        // Aynı inner/outer aralıklarında çakışma kontrolü
        for existing in &self.uid_map {
            let e_end = existing.inner_id + existing.count;
            let n_end = inner + count;
            if inner < e_end && n_end > existing.inner_id {
                return Err("Overlapping UID mapping");
            }
        }

        self.uid_map.push(IdMapping {
            inner_id: inner,
            outer_id: outer,
            count,
        });
        crate::serial_println!(
            "[User ns={}] uid_map: inner {}..{} -> outer {}..{}",
            self.ns_id,
            inner,
            inner + count - 1,
            outer,
            outer + count - 1
        );
        Ok(())
    }

    /// GID mapping ekler
    pub fn add_gid_mapping(
        &mut self,
        inner: u32,
        outer: u32,
        count: u32,
    ) -> Result<(), &'static str> {
        for existing in &self.gid_map {
            let e_end = existing.inner_id + existing.count;
            let n_end = inner + count;
            if inner < e_end && n_end > existing.inner_id {
                return Err("Overlapping GID mapping");
            }
        }

        self.gid_map.push(IdMapping {
            inner_id: inner,
            outer_id: outer,
            count,
        });
        crate::serial_println!(
            "[User ns={}] gid_map: inner {}..{} -> outer {}..{}",
            self.ns_id,
            inner,
            inner + count - 1,
            outer,
            outer + count - 1
        );
        Ok(())
    }

    /// Container UID → Host UID çevirisi
    pub fn map_uid_to_host(&self, inner_uid: u32) -> Option<u32> {
        for mapping in &self.uid_map {
            if let Some(outer) = mapping.inner_to_outer(inner_uid) {
                return Some(outer);
            }
        }
        None
    }

    /// Host UID → Container UID çevirisi
    pub fn map_uid_from_host(&self, outer_uid: u32) -> Option<u32> {
        for mapping in &self.uid_map {
            if let Some(inner) = mapping.outer_to_inner(outer_uid) {
                return Some(inner);
            }
        }
        None
    }

    /// Container GID → Host GID
    pub fn map_gid_to_host(&self, inner_gid: u32) -> Option<u32> {
        for mapping in &self.gid_map {
            if let Some(outer) = mapping.inner_to_outer(inner_gid) {
                return Some(outer);
            }
        }
        None
    }

    /// Container'da root yetkisi var mı? (inner uid=0 ise)
    pub fn has_root_in_ns(&self, inner_uid: u32) -> bool {
        inner_uid == 0 && !self.uid_map.is_empty()
    }
}

// ============================================================================
// Global Registry
// ============================================================================

static NEXT_UTS_NS_ID: AtomicU32 = AtomicU32::new(1);
static NEXT_USER_NS_ID: AtomicU32 = AtomicU32::new(1);

lazy_static::lazy_static! {
    static ref UTS_NAMESPACES: Mutex<BTreeMap<u32, UtsNamespace>> = Mutex::new(BTreeMap::new());
    static ref USER_NAMESPACES: Mutex<BTreeMap<u32, UserNamespace>> = Mutex::new(BTreeMap::new());
}

/// Yeni UTS namespace oluşturur
pub fn create_uts_namespace(parent_id: Option<u32>) -> u32 {
    let ns_id = NEXT_UTS_NS_ID.fetch_add(1, Ordering::SeqCst);
    let mut namespaces = UTS_NAMESPACES.lock();

    let ns = if let Some(pid) = parent_id {
        if let Some(parent) = namespaces.get(&pid) {
            UtsNamespace::fork_from(ns_id, parent)
        } else {
            UtsNamespace::new(ns_id)
        }
    } else {
        UtsNamespace::new(ns_id)
    };

    crate::serial_println!(
        "[UTS] Created namespace ns_id={} parent={:?}",
        ns_id,
        parent_id
    );
    namespaces.insert(ns_id, ns);
    ns_id
}

/// UTS namespace'e hostname set eder
pub fn set_hostname(ns_id: u32, hostname: &str) -> Result<(), &'static str> {
    let mut namespaces = UTS_NAMESPACES.lock();
    if let Some(ns) = namespaces.get_mut(&ns_id) {
        ns.set_hostname(hostname);
        Ok(())
    } else {
        Err("UTS namespace not found")
    }
}

/// Yeni User namespace oluşturur
pub fn create_user_namespace(owner_uid: u32, owner_gid: u32) -> u32 {
    let ns_id = NEXT_USER_NS_ID.fetch_add(1, Ordering::SeqCst);
    let ns = UserNamespace::new(ns_id, owner_uid, owner_gid);

    crate::serial_println!(
        "[User NS] Created namespace ns_id={} owner={}:{}",
        ns_id,
        owner_uid,
        owner_gid
    );

    USER_NAMESPACES.lock().insert(ns_id, ns);
    ns_id
}

/// User namespace'e UID mapping ekler
pub fn add_uid_mapping(ns_id: u32, inner: u32, outer: u32, count: u32) -> Result<(), &'static str> {
    let mut namespaces = USER_NAMESPACES.lock();
    if let Some(ns) = namespaces.get_mut(&ns_id) {
        ns.add_uid_mapping(inner, outer, count)
    } else {
        Err("User namespace not found")
    }
}

/// Modülü başlatır
pub fn init() {
    // Init UTS namespace (ns_id=0)
    let init_uts = UtsNamespace::new(0);
    UTS_NAMESPACES.lock().insert(0, init_uts);

    // Init User namespace (ns_id=0, identity mapping)
    let init_user = UserNamespace::init_ns();
    USER_NAMESPACES.lock().insert(0, init_user);

    crate::serial_println!("[NS] UTS + User namespace module initialized");
}
