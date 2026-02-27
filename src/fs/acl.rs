//! # POSIX ACL (Erişim Kontrol Listeleri)
//!
//! Sahip/grup/diğerül modelinin ötesinde dosyalar için ayrıntılı izin denetimi.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// ACL SABİTLERİ
// ============================================================================

/// ACL etiket türleri
pub const ACL_USER_OBJ: u32 = 0x01;
pub const ACL_USER: u32 = 0x02;
pub const ACL_GROUP_OBJ: u32 = 0x04;
pub const ACL_GROUP: u32 = 0x08;
pub const ACL_MASK: u32 = 0x10;
pub const ACL_OTHER: u32 = 0x20;

/// ACL izinleri
pub const ACL_READ: u32 = 0x04;
pub const ACL_WRITE: u32 = 0x02;
pub const ACL_EXECUTE: u32 = 0x01;

/// ACL türleri
pub const ACL_TYPE_ACCESS: u32 = 0x8000_0000;
pub const ACL_TYPE_DEFAULT: u32 = 0x4000_0000;

/// ACL komutları
pub const ACL_GET_TYPE: u32 = 0x0001;
pub const ACL_SET_TYPE: u32 = 0x0002;
pub const ACL_GET_FILE: u32 = 0x0003;
pub const ACL_SET_FILE: u32 = 0x0004;
pub const ACL_DELETE_FILE: u32 = 0x0005;

// ============================================================================
// ACL GİRİŞİ
// ============================================================================

/// ACL giriş yapısı
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AclEntry {
    /// Etiket türü (USER_OBJ, USER, GROUP_OBJ, GROUP, MASK, OTHER)
    pub tag: u32,
    /// İzin bitleri
    pub perm: u32,
    /// Niteleyici (USER için UID, GROUP için GID)
    pub qualifier: u32,
}

impl AclEntry {
    pub fn new(tag: u32, perm: u32, qualifier: u32) -> Self {
        Self { tag, perm, qualifier }
    }

    /// Mode'dan user_obj girişi oluşturur
    pub fn user_obj_from_mode(mode: u32) -> Self {
        Self::new(ACL_USER_OBJ, (mode >> 6) & 0x7, 0)
    }

    /// Mode'dan group_obj girişi oluşturur
    pub fn group_obj_from_mode(mode: u32) -> Self {
        Self::new(ACL_GROUP_OBJ, (mode >> 3) & 0x7, 0)
    }

    /// Mode'dan other girişi oluşturur
    pub fn other_from_mode(mode: u32) -> Self {
        Self::new(ACL_OTHER, mode & 0x7, 0)
    }

    /// Girişin izin verip vermediğini kontrol eder
    pub fn grants(&self, perm: u32) -> bool {
        (self.perm & perm) == perm
    }
}

// ============================================================================
// ACL
// ============================================================================

/// Dosya için tam ACL yapısı
#[derive(Clone, Debug)]
pub struct Acl {
    /// Bu ACL'nin ait olduğu inode numarası
    pub inode: u64,
    /// Erişim ACL girişleri
    pub access: Vec<AclEntry>,
    /// Varsayılan ACL girişleri (dizinler için)
    pub default: Vec<AclEntry>,
}

impl Acl {
    pub fn new(inode: u64) -> Self {
        Self {
            inode,
            access: Vec::new(),
            default: Vec::new(),
        }
    }

    /// Mode bitlerinden minimal ACL oluşturur
    pub fn from_mode(inode: u64, mode: u32) -> Self {
        let mut acl = Self::new(inode);
        acl.access.push(AclEntry::user_obj_from_mode(mode));
        acl.access.push(AclEntry::group_obj_from_mode(mode));
        acl.access.push(AclEntry::other_from_mode(mode));
        acl
    }

    /// Erişim ACL'ye giriş ekler
    pub fn add_access(&mut self, entry: AclEntry) {
        self.access.push(entry);
    }

    /// Varsayılan ACL'ye giriş ekler
    pub fn add_default(&mut self, entry: AclEntry) {
        self.default.push(entry);
    }

    /// Erişim ACL'den giriş kaldırır
    pub fn remove_access(&mut self, tag: u32, qualifier: u32) -> bool {
        let len = self.access.len();
        self.access.retain(|e| !(e.tag == tag && e.qualifier == qualifier));
        self.access.len() != len
    }

    /// Kullanıcı için izni kontrol eder
    pub fn check_permission(&self, uid: u32, gid: u32, mask: u32, perm: u32) -> bool {
        // Kullanıcı girişlerini kontrol et
        for entry in &self.access {
            match entry.tag {
                ACL_USER_OBJ => {
                    // Sahip izni - başka yerde kontrol edilir
                }
                ACL_USER => {
                    if entry.qualifier == uid {
                        return self.apply_mask(entry.perm, mask) & perm == perm;
                    }
                }
                ACL_GROUP_OBJ => {
                    // Grup izni - başka yerde kontrol edilir
                }
                ACL_GROUP => {
                    if entry.qualifier == gid {
                        if self.apply_mask(entry.perm, mask) & perm == perm {
                            return true;
                        }
                    }
                }
                ACL_MASK => {}
                ACL_OTHER => {
                    // Diğer izni - en sona kontrol edilir
                }
                _ => {}
            }
        }
        false
    }

    /// Maskeyi izinlere uygular
    fn apply_mask(&self, perm: u32, mask: u32) -> u32 {
        // Maske girişini bul
        for entry in &self.access {
            if entry.tag == ACL_MASK {
                return perm & entry.perm & mask;
            }
        }
        perm & mask
    }

    /// Maske girişini döndürür
    pub fn get_mask(&self) -> Option<&AclEntry> {
        self.access.iter().find(|e| e.tag == ACL_MASK)
    }

    /// ACL'nin minimal olup olmadığını kontrol eder (yalnızca user_obj, group_obj, other)
    pub fn is_minimal(&self) -> bool {
        self.access.len() == 3 &&
        self.access.iter().all(|e| e.tag == ACL_USER_OBJ || 
                                   e.tag == ACL_GROUP_OBJ || 
                                   e.tag == ACL_OTHER)
    }

    /// Mode bitlerini döndürür
    pub fn to_mode(&self) -> u32 {
        let mut mode = 0u32;
        
        for entry in &self.access {
            match entry.tag {
                ACL_USER_OBJ => mode |= entry.perm << 6,
                ACL_GROUP_OBJ => mode |= entry.perm << 3,
                ACL_OTHER => mode |= entry.perm,
                _ => {}
            }
        }
        
        mode
    }

    /// İkili (binary) formatına diziştirir
    pub fn to_binary(&self) -> Vec<u8> {
        let mut data = Vec::new();
        
        // Başlık
        data.extend_from_slice(&(self.access.len() as u32).to_le_bytes());
        data.extend_from_slice(&(self.default.len() as u32).to_le_bytes());
        
        // Erişim girişleri
        for entry in &self.access {
            data.extend_from_slice(&entry.tag.to_le_bytes());
            data.extend_from_slice(&entry.perm.to_le_bytes());
            data.extend_from_slice(&entry.qualifier.to_le_bytes());
        }
        
        // Varsayılan girişler
        for entry in &self.default {
            data.extend_from_slice(&entry.tag.to_le_bytes());
            data.extend_from_slice(&entry.perm.to_le_bytes());
            data.extend_from_slice(&entry.qualifier.to_le_bytes());
        }
        
        data
    }

    /// İkili formattan ayrıştırır
    pub fn from_binary(inode: u64, data: &[u8]) -> Option<Self> {
        if data.len() < 8 {
            return None;
        }
        
        let access_count = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
        let default_count = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;
        
        let mut acl = Self::new(inode);
        let mut offset = 8;
        
        for _ in 0..access_count {
            if offset + 12 > data.len() {
                return None;
            }
            let tag = u32::from_le_bytes([data[offset], data[offset+1], data[offset+2], data[offset+3]]);
            let perm = u32::from_le_bytes([data[offset+4], data[offset+5], data[offset+6], data[offset+7]]);
            let qual = u32::from_le_bytes([data[offset+8], data[offset+9], data[offset+10], data[offset+11]]);
            acl.access.push(AclEntry::new(tag, perm, qual));
            offset += 12;
        }
        
        for _ in 0..default_count {
            if offset + 12 > data.len() {
                return None;
            }
            let tag = u32::from_le_bytes([data[offset], data[offset+1], data[offset+2], data[offset+3]]);
            let perm = u32::from_le_bytes([data[offset+4], data[offset+5], data[offset+6], data[offset+7]]);
            let qual = u32::from_le_bytes([data[offset+8], data[offset+9], data[offset+10], data[offset+11]]);
            acl.default.push(AclEntry::new(tag, perm, qual));
            offset += 12;
        }
        
        Some(acl)
    }
}

// ============================================================================
// ACL YÖNETİCİSİ
// ============================================================================

pub struct AclManager {
    acls: Mutex<BTreeMap<u64, Acl>>,
    total_acls: AtomicU64,
}

impl AclManager {
    pub const fn new() -> Self {
        Self {
            acls: Mutex::new(BTreeMap::new()),
            total_acls: AtomicU64::new(0),
        }
    }

    /// Inode için ACL getirir
    pub fn get_acl(&self, inode: u64) -> Option<Acl> {
        self.acls.lock().get(&inode).cloned()
    }

    /// Inode için ACL ayarlar
    pub fn set_acl(&self, inode: u64, acl: Acl) {
        self.acls.lock().insert(inode, acl);
        self.total_acls.fetch_add(1, Ordering::Relaxed);
    }

    /// Inode'un ACL'sini kaldırır
    pub fn remove_acl(&self, inode: u64) {
        self.acls.lock().remove(&inode);
    }

    /// İzni kontrol eder
    pub fn check_permission(&self, inode: u64, uid: u32, gid: u32, mask: u32, perm: u32) -> bool {
        if let Some(acl) = self.get_acl(inode) {
            acl.check_permission(uid, gid, mask, perm)
        } else {
            // ACL yok, standart izinler kullanılır
            true
        }
    }

    /// İstatistikleri döndürür
    pub fn get_stats(&self) -> AclStats {
        AclStats {
            total_acls: self.total_acls.load(Ordering::Relaxed),
        }
    }
}

lazy_static::lazy_static! {
    pub static ref ACL_MANAGER: AclManager = AclManager::new();
}

pub struct AclStats {
    pub total_acls: u64,
}

// ============================================================================
// SİSTEM ÇAĞRISI ARAYÜZÜ
// ============================================================================

pub fn sys_acl_get_file(path: &str, acl_type: u32) -> i64 {
    let inode = hash_path(path);
    
    match ACL_MANAGER.get_acl(inode) {
        Some(acl) => {
            let data = if acl_type == ACL_TYPE_ACCESS {
                acl.to_binary()
            } else {
                // Varsayılan ACL'yi döndür
                let mut data = Vec::new();
                data.extend_from_slice(&0u32.to_le_bytes());
                data.extend_from_slice(&(acl.default.len() as u32).to_le_bytes());
                for entry in &acl.default {
                    data.extend_from_slice(&entry.tag.to_le_bytes());
                    data.extend_from_slice(&entry.perm.to_le_bytes());
                    data.extend_from_slice(&entry.qualifier.to_le_bytes());
                }
                data
            };
            data.len() as i64
        }
        None => -61, // ENODATA
    }
}

pub fn sys_acl_set_file(path: &str, acl_type: u32, data: &[u8]) -> i32 {
    let inode = hash_path(path);
    
    match Acl::from_binary(inode, data) {
        Some(acl) => {
            ACL_MANAGER.set_acl(inode, acl);
            0
        }
        None => -22,
    }
}

pub fn sys_acl_delete_file(path: &str, acl_type: u32) -> i32 {
    let inode = hash_path(path);
    ACL_MANAGER.remove_acl(inode);
    0
}

fn hash_path(path: &str) -> u64 {
    let mut hash: u64 = 5381;
    for byte in path.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(byte as u64);
    }
    hash
}

pub fn init() {
    crate::serial_println!("[ACL] Alt sistemi başlatıldı");
}
