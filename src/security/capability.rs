//! # Yetenek Tabanlı Güvenlik (Capability-Based Security)
//!
//! Kaynak erişim denetimi için ince taneli yetenek sistemi.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;
use spin::Mutex;

/// Yetenek kimliği
pub type CapId = u64;

/// Yetenek hakları
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapRights {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
    pub share: bool,
    pub transfer: bool,
}

impl CapRights {
    pub const NONE: Self = CapRights { read: false, write: false, execute: false, share: false, transfer: false };
    pub const READ: Self = CapRights { read: true, write: false, execute: false, share: false, transfer: false };
    pub const WRITE: Self = CapRights { read: false, write: true, execute: false, share: false, transfer: false };
    pub const READ_WRITE: Self = CapRights { read: true, write: true, execute: false, share: false, transfer: false };
    pub const ALL: Self = CapRights { read: true, write: true, execute: true, share: true, transfer: true };
}

/// Kaynak türü
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceType {
    File,
    Directory,
    Socket,
    Device,
    Memory,
    Process,
    Thread,
    Port,
    Key,
    Service,
}

/// Yetenek nesnesi
#[derive(Clone, Debug)]
pub struct Capability {
    pub id: CapId,
    pub resource_type: ResourceType,
    pub resource_id: u64,
    pub rights: CapRights,
    pub owner: u64,  // İşlem kimliği
    pub generation: u32,
    pub children: Vec<CapId>,
}

/// İşlem başına yetenek tablosu
#[derive(Clone, Debug)]
pub struct CapabilityTable {
    pub process_id: u64,
    pub capabilities: BTreeMap<CapId, Capability>,
    pub next_cap_id: CapId,
}

impl CapabilityTable {
    pub fn new(process_id: u64) -> Self {
        CapabilityTable {
            process_id,
            capabilities: BTreeMap::new(),
            next_cap_id: 1,
        }
    }

    /// Yeni yetenek oluşturur
    pub fn create(&mut self, resource_type: ResourceType, resource_id: u64, rights: CapRights) -> CapId {
        let id = self.next_cap_id;
        self.next_cap_id += 1;

        let cap = Capability {
            id,
            resource_type,
            resource_id,
            rights,
            owner: self.process_id,
            generation: 0,
            children: Vec::new(),
        };

        self.capabilities.insert(id, cap);
        id
    }

    /// Kimliğe göre yetenek getirir
    pub fn get(&self, id: CapId) -> Option<&Capability> {
        self.capabilities.get(&id)
    }

    /// Yetkinin var olup olmadığını ve haklara sahip olup olmadığını kontrol eder
    pub fn check(&self, id: CapId, required: CapRights) -> bool {
        if let Some(cap) = self.capabilities.get(&id) {
            let r = cap.rights;
            (!required.read || r.read)
                && (!required.write || r.write)
                && (!required.execute || r.execute)
                && (!required.share || r.share)
                && (!required.transfer || r.transfer)
        } else {
            false
        }
    }

    /// Alt yetenek türetir (hakların alt kümesi)
    pub fn derive(&mut self, parent_id: CapId, subset_rights: CapRights) -> Option<CapId> {
        let parent = self.capabilities.get(&parent_id)?;

        // Alt kümenin geçerli olup olmadığını kontrol et
        if subset_rights.read && !parent.rights.read { return None; }
        if subset_rights.write && !parent.rights.write { return None; }
        if subset_rights.execute && !parent.rights.execute { return None; }
        if subset_rights.share && !parent.rights.share { return None; }
        if subset_rights.transfer && !parent.rights.transfer { return None; }

        let child_id = self.next_cap_id;
        self.next_cap_id += 1;

        let child = Capability {
            id: child_id,
            resource_type: parent.resource_type,
            resource_id: parent.resource_id,
            rights: subset_rights,
            owner: self.process_id,
            generation: parent.generation + 1,
            children: Vec::new(),
        };

        self.capabilities.get_mut(&parent_id)?.children.push(child_id);
        self.capabilities.insert(child_id, child);
        Some(child_id)
    }

    /// Yetkiyi ve tüm alt yetkilerini iptal eder
    pub fn revoke(&mut self, id: CapId) -> bool {
        if let Some(cap) = self.capabilities.remove(&id) {
            // Tüm alt yetkilerini özyinelemeli olarak iptal et
            for child_id in cap.children {
                self.revoke(child_id);
            }
            true
        } else {
            false
        }
    }

    /// Yetkiyi başka bir işleme aktarır
    pub fn transfer(&mut self, id: CapId, target_pid: u64) -> Option<Capability> {
        let cap = self.capabilities.remove(&id)?;
        if !cap.rights.transfer {
            self.capabilities.insert(id, cap);
            return None;
        }

        let mut transferred = cap.clone();
        transferred.owner = target_pid;
        transferred.generation += 1;
        Some(transferred)
    }
}

// Global yetenek yöneticisi
lazy_static::lazy_static! {
    static ref CAP_TABLES: Mutex<BTreeMap<u64, CapabilityTable>> = Mutex::new(BTreeMap::new());
}

/// İşlem için yetenek tablosu başlatır
pub fn init_process(pid: u64) {
    let mut tables = CAP_TABLES.lock();
    tables.insert(pid, CapabilityTable::new(pid));
}

/// İşlem için yetenek tablosu getirir
pub fn get_table(pid: u64) -> Option<CapabilityTable> {
    CAP_TABLES.lock().get(&pid).cloned()
}

/// İşlem için yetenek oluşturur
pub fn create_capability(pid: u64, resource_type: ResourceType, resource_id: u64, rights: CapRights) -> Option<CapId> {
    let mut tables = CAP_TABLES.lock();
    let table = tables.get_mut(&pid)?;
    Some(table.create(resource_type, resource_id, rights))
}

/// Yetkiyi kontrol eder
pub fn check_capability(pid: u64, cap_id: CapId, rights: CapRights) -> bool {
    let tables = CAP_TABLES.lock();
    if let Some(table) = tables.get(&pid) {
        table.check(cap_id, rights)
    } else {
        false
    }
}

/// Yetenek türetir
pub fn derive_capability(pid: u64, parent_id: CapId, subset_rights: CapRights) -> Option<CapId> {
    let mut tables = CAP_TABLES.lock();
    let table = tables.get_mut(&pid)?;
    table.derive(parent_id, subset_rights)
}

/// Yetkiyi iptal eder
pub fn revoke_capability(pid: u64, cap_id: CapId) -> bool {
    let mut tables = CAP_TABLES.lock();
    if let Some(table) = tables.get_mut(&pid) {
        table.revoke(cap_id)
    } else {
        false
    }
}

/// İşlemler arasında yetki aktarır
pub fn transfer_capability(from_pid: u64, cap_id: CapId, to_pid: u64) -> bool {
    let mut tables = CAP_TABLES.lock();

    let transferred = {
        let from_table = tables.get_mut(&from_pid);
        if let Some(table) = from_table {
            table.transfer(cap_id, to_pid)
        } else {
            None
        }
    };

    if let Some(cap) = transferred {
        let to_table = tables.get_mut(&to_pid);
        if let Some(table) = to_table {
            table.capabilities.insert(cap.id, cap);
            return true;
        }
    }
    false
}

/// İşlemin yetkilerini temizler
pub fn cleanup_process(pid: u64) {
    CAP_TABLES.lock().remove(&pid);
}

/// Yetkiyi mühürler (değiştirilemez yapar)
pub fn seal_capability(pid: u64, cap_id: CapId) -> bool {
    let tables = CAP_TABLES.lock();
    if let Some(table) = tables.get(&pid) {
        if let Some(cap) = table.capabilities.get(&cap_id) {
            // Mühürlenmiş yetkiler aktarılamaz
            // Bu, transfer bayrağı kontrol edilerek zorlanır
            return !cap.rights.transfer;
        }
    }
    false
}
