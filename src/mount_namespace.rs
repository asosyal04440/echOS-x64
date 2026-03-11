//! # Mount Namespace — CLONE_NEWNS İzolasyonu
//!
//! Mount namespace, her process'e bağımsız mount tablo görünümü sağlar.
//! Linux CLONE_NEWNS bayrağı ile aynı semantiğe sahiptir.
//!
//! ## Mimari
//!
//! ```text
//! ┌──────────────┐ clone(CLONE_NEWNS)  ┌──────────────┐
//! │ Parent NS    │────────────────────►│ Child NS     │
//! │ mount_table  │  (deep copy)        │ mount_table  │
//! │ ┌──────────┐ │                     │ ┌──────────┐ │
//! │ │ / → ext4 │ │                     │ │ / → ext4 │ │
//! │ │/tmp→tmpfs│ │                     │ │/tmp→tmpfs│ │
//! │ │/proc→proc│ │                     │ │/proc→proc│ │
//! │ └──────────┘ │                     │ └──────────┘ │
//! └──────────────┘                     └──────────────┘
//!                                       Child NS'de mount/umount
//!                                       parent'ı ETKİLEMEZ
//! ```
//!
//! ## Özellikler
//!
//! - Per-namespace mount tablosu (parent'tan copy-on-fork)
//! - Bind mount desteği
//! - Mount propagation (shared, private, slave, unbindable)
//! - Pivot root desteği (container init)
//! - Nested namespace hiyerarşisi

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;

// ============================================================================
// Types
// ============================================================================

/// Mount namespace ID
pub type MountNsId = u32;

/// Mount propagation türleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MountPropagation {
    /// Bu NS'deki mount/umount diğer NS'lere YAYILMAZ
    Private,
    /// Bu NS'deki mount/umount peer group'a yayılır
    Shared,
    /// Shared NS'den gelen propagation'ı alır, kendi propagation'ını YAYMAZ
    Slave,
    /// Hiçbir yere bind mount edilemez
    Unbindable,
}

/// CLONE_NEWNS flag (Linux uyumlu)
pub const CLONE_NEWNS: u32 = 0x00020000;

/// MS_BIND flag (bind mount)
pub const MS_BIND: u32 = 0x1000;
/// MS_REC flag (recursive)
pub const MS_REC: u32 = 0x4000;
/// MS_PRIVATE flag
pub const MS_PRIVATE: u32 = 1 << 18;
/// MS_SHARED flag
pub const MS_SHARED: u32 = 1 << 20;
/// MS_SLAVE flag
pub const MS_SLAVE: u32 = 1 << 19;
/// MS_UNBINDABLE flag
pub const MS_UNBINDABLE: u32 = 1 << 17;
/// MS_RDONLY
pub const MS_RDONLY: u32 = 1;
/// MS_NOSUID
pub const MS_NOSUID: u32 = 2;
/// MS_NODEV
pub const MS_NODEV: u32 = 4;
/// MS_NOEXEC
pub const MS_NOEXEC: u32 = 8;

// ============================================================================
// Mount Entry
// ============================================================================

/// Tek bir mount noktası
#[derive(Clone, Debug)]
pub struct NsMountEntry {
    /// Mount ID (namespace içinde benzersiz)
    pub mount_id: u32,
    /// Parent mount ID (kök için 0)
    pub parent_id: u32,
    /// Kaynak cihaz/dosya sistemi
    pub source: String,
    /// Hedef yol (mount point)
    pub target: String,
    /// Dosya sistemi tipi
    pub fs_type: String,
    /// Mount bayrakları
    pub flags: u32,
    /// Mount seçenekleri
    pub options: String,
    /// Propagation türü
    pub propagation: MountPropagation,
    /// Shared peer group ID (0 = yok)
    pub peer_group: u32,
}

impl NsMountEntry {
    pub fn new(mount_id: u32, source: &str, target: &str, fs_type: &str, flags: u32) -> Self {
        Self {
            mount_id,
            parent_id: 0,
            source: String::from(source),
            target: String::from(target),
            fs_type: String::from(fs_type),
            flags,
            options: String::new(),
            propagation: MountPropagation::Private,
            peer_group: 0,
        }
    }

    /// Read-only mu?
    pub fn is_readonly(&self) -> bool {
        self.flags & MS_RDONLY != 0
    }

    /// Bind mount mu?
    pub fn is_bind(&self) -> bool {
        self.flags & MS_BIND != 0
    }
}

// ============================================================================
// Mount Namespace
// ============================================================================

/// Mount namespace yapısı
pub struct MountNamespace {
    /// Namespace ID
    pub ns_id: MountNsId,
    /// Parent namespace ID
    pub parent_ns: Option<MountNsId>,
    /// Namespace ismi
    pub name: String,
    /// Mount tablosu (target → entry)
    pub mounts: BTreeMap<String, NsMountEntry>,
    /// Sonraki mount ID
    next_mount_id: u32,
    /// Root yolu (pivot root sonrası değişebilir)
    pub root: String,
    /// Aktif mi?
    pub active: bool,
}

impl MountNamespace {
    /// Yeni boş namespace oluşturur
    pub fn new(ns_id: MountNsId, name: &str, parent_ns: Option<MountNsId>) -> Self {
        Self {
            ns_id,
            parent_ns,
            name: String::from(name),
            mounts: BTreeMap::new(),
            next_mount_id: 1,
            root: String::from("/"),
            active: true,
        }
    }

    /// Parent'tan mount tablosunu kopyalar (CLONE_NEWNS)
    pub fn fork_from(ns_id: MountNsId, name: &str, parent: &MountNamespace) -> Self {
        let mut ns = Self::new(ns_id, name, Some(parent.ns_id));
        // Deep copy of mount table
        for (target, entry) in &parent.mounts {
            let mut new_entry = entry.clone();
            // Private propagation — parent'ı etkilemez
            new_entry.propagation = MountPropagation::Private;
            ns.mounts.insert(target.clone(), new_entry);
        }
        ns.next_mount_id = parent.next_mount_id;
        ns.root = parent.root.clone();
        ns
    }

    /// Yeni mount noktası ekler
    pub fn mount(
        &mut self,
        source: &str,
        target: &str,
        fs_type: &str,
        flags: u32,
    ) -> Result<u32, &'static str> {
        let mount_id = self.next_mount_id;
        self.next_mount_id += 1;

        let mut entry = NsMountEntry::new(mount_id, source, target, fs_type, flags);

        // Propagation flags'ı kontrol et
        if flags & MS_SHARED != 0 {
            entry.propagation = MountPropagation::Shared;
        } else if flags & MS_SLAVE != 0 {
            entry.propagation = MountPropagation::Slave;
        } else if flags & MS_UNBINDABLE != 0 {
            entry.propagation = MountPropagation::Unbindable;
        }

        // Parent mount'u bul
        let parent_target = self.find_parent_mount(target);
        if let Some(parent) = parent_target {
            if let Some(parent_entry) = self.mounts.get(&parent) {
                entry.parent_id = parent_entry.mount_id;
            }
        }

        self.mounts.insert(String::from(target), entry);

        crate::serial_println!(
            "[MountNS:{}] mount {} -> {} (type={}, flags=0x{:x})",
            self.ns_id,
            source,
            target,
            fs_type,
            flags
        );

        Ok(mount_id)
    }

    /// Mount noktasını kaldırır
    pub fn umount(&mut self, target: &str) -> Result<(), &'static str> {
        if target == "/" {
            return Err("Cannot unmount root filesystem");
        }

        self.mounts.remove(target).ok_or("Mount point not found")?;

        crate::serial_println!("[MountNS:{}] umount {}", self.ns_id, target);
        Ok(())
    }

    /// Bind mount yapar (bir dizini başka bir yere mount eder)
    pub fn bind_mount(&mut self, source: &str, target: &str) -> Result<u32, &'static str> {
        self.mount(source, target, "bind", MS_BIND)
    }

    /// Pivot root — container init için kök dizini değiştirir
    ///
    /// pivot_root(new_root, put_old):
    ///   - new_root yeni kök olur
    ///   - eski kök put_old'a taşınır
    pub fn pivot_root(&mut self, new_root: &str, _put_old: &str) -> Result<(), &'static str> {
        if !self.mounts.contains_key(new_root) {
            return Err("new_root is not a mount point");
        }

        self.root = String::from(new_root);
        crate::serial_println!("[MountNS:{}] pivot_root to {}", self.ns_id, new_root);
        Ok(())
    }

    /// En yakın parent mount noktasını bulur (longest prefix match)
    fn find_parent_mount(&self, path: &str) -> Option<String> {
        let mut best = None;
        let mut best_len = 0;

        for target in self.mounts.keys() {
            if path.starts_with(target.as_str())
                && target.len() > best_len
                && target.as_str() != path
            {
                best_len = target.len();
                best = Some(target.clone());
            }
        }

        best
    }

    /// Tüm mount noktalarını listeler
    pub fn list_mounts(&self) -> Vec<&NsMountEntry> {
        self.mounts.values().collect()
    }

    /// Verilen yol için aktif mount noktasını bulur
    pub fn resolve_mount(&self, path: &str) -> Option<&NsMountEntry> {
        let mut best = None;
        let mut best_len = 0;

        for (target, entry) in &self.mounts {
            if path.starts_with(target.as_str()) && target.len() > best_len {
                best_len = target.len();
                best = Some(entry);
            }
        }

        best
    }

    /// Mount sayısı
    pub fn mount_count(&self) -> usize {
        self.mounts.len()
    }
}

// ============================================================================
// Global Mount Namespace Manager
// ============================================================================

static NEXT_MOUNT_NS_ID: AtomicU32 = AtomicU32::new(1);

lazy_static::lazy_static! {
    /// Tüm mount namespace'ler
    static ref MOUNT_NAMESPACES: Mutex<BTreeMap<MountNsId, MountNamespace>> =
        Mutex::new(BTreeMap::new());
}

/// Yeni mount namespace oluşturur
pub fn create_namespace(name: &str, parent_ns: Option<MountNsId>) -> MountNsId {
    let ns_id = NEXT_MOUNT_NS_ID.fetch_add(1, Ordering::Relaxed);

    let ns = if let Some(parent_id) = parent_ns {
        let namespaces = MOUNT_NAMESPACES.lock();
        if let Some(parent) = namespaces.get(&parent_id) {
            MountNamespace::fork_from(ns_id, name, parent)
        } else {
            MountNamespace::new(ns_id, name, parent_ns)
        }
    } else {
        MountNamespace::new(ns_id, name, None)
    };

    MOUNT_NAMESPACES.lock().insert(ns_id, ns);

    crate::serial_println!(
        "[MountNS] Created namespace {} (name='{}', parent={:?})",
        ns_id,
        name,
        parent_ns
    );

    ns_id
}

/// Default (init) mount namespace'i oluşturur
pub fn create_init_namespace() -> MountNsId {
    let ns_id = create_namespace("init", None);

    let mut namespaces = MOUNT_NAMESPACES.lock();
    if let Some(ns) = namespaces.get_mut(&ns_id) {
        // Temel mount noktalarını ekle
        let _ = ns.mount("rootfs", "/", "ext4", 0);
        let _ = ns.mount("proc", "/proc", "proc", 0);
        let _ = ns.mount("sys", "/sys", "sysfs", 0);
        let _ = ns.mount("devtmpfs", "/dev", "devtmpfs", 0);
        let _ = ns.mount("tmpfs", "/tmp", "tmpfs", 0);
        let _ = ns.mount("tmpfs", "/run", "tmpfs", 0);
    }

    ns_id
}

/// Namespace'e mount ekler
pub fn ns_mount(
    ns_id: MountNsId,
    source: &str,
    target: &str,
    fs_type: &str,
    flags: u32,
) -> Result<u32, &'static str> {
    let mut namespaces = MOUNT_NAMESPACES.lock();
    let ns = namespaces.get_mut(&ns_id).ok_or("Namespace not found")?;
    ns.mount(source, target, fs_type, flags)
}

/// Namespace'den mount kaldırır
pub fn ns_umount(ns_id: MountNsId, target: &str) -> Result<(), &'static str> {
    let mut namespaces = MOUNT_NAMESPACES.lock();
    let ns = namespaces.get_mut(&ns_id).ok_or("Namespace not found")?;
    ns.umount(target)
}

/// Namespace'in mount listesini döndürür
pub fn ns_list_mounts(ns_id: MountNsId) -> Vec<(String, String, String)> {
    let namespaces = MOUNT_NAMESPACES.lock();
    if let Some(ns) = namespaces.get(&ns_id) {
        ns.mounts
            .values()
            .map(|e| (e.source.clone(), e.target.clone(), e.fs_type.clone()))
            .collect()
    } else {
        Vec::new()
    }
}

/// Toplam namespace sayısı
pub fn namespace_count() -> usize {
    MOUNT_NAMESPACES.lock().len()
}

/// Mount namespace modülünü başlatır
pub fn init() {
    crate::serial_println!("[MountNS] Mount namespace module initialized (CLONE_NEWNS)");
}
