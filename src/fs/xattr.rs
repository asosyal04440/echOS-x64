//! # Genişletilmiş Öznitelikler (xattr)
//!
//! Dosyalar için POSIX genişletilmiş öznitelikler desteği.
//! Dosyalara anahtar-değer çiftleri olarak ek meta veri saklamayı sağlar.
//!
//! ## xattr Ad Alanı Ve Yapısı (ASCII Diyagram)
//! ```text
//! xattr Ad Alanları:
//!   user.*         - Kullanıcı alanı (herkes okuyabilir, dosya sahibi yazabilir)
//!   trusted.*      - Güvenilir alan (yalnızca CAP_SYS_ADMIN ile erişilebilir)
//!   security.*     - Güvenlik alanı (SELinux, capability gibi)
//!   system.*       - Sistem alanı (POSIX ACL'ler: posix_acl_access, posix_acl_default)
//!
//! Depolama Yapısı:
//!   XATTR_MANAGER
//!     └── storage: BTreeMap<inode, XattrStorage>
//!           └── XattrStorage
//!                 └── attrs: BTreeMap<String(isim), Vec<u8>(değer)>
//!
//! Sistem Çağrıları:
//!   setxattr / lsetxattr / fsetxattr  → öznitelik yaz
//!   getxattr / lgetxattr / fgetxattr  → öznitelik oku
//!   listxattr / llistxattr / flistxattr → öznitelik listele
//!   removexattr / lremovexattr / fremovexattr → öznitelik sil
//! ```

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// XATTR SABİTLERİ
// ============================================================================

/// Maksimum xattr adı uzunluğu (bayt)
pub const XATTR_NAME_MAX: usize = 255;
/// Maksimum xattr değer uzunluğu (bayt)
pub const XATTR_VALUE_MAX: usize = 65536;
/// Maksimum xattr liste boyutu (bayt)
pub const XATTR_LIST_MAX: usize = 65536;

/// xattr ad alanı önekleri
pub const XATTR_USER_PREFIX: &str = "user.";
pub const XATTR_TRUSTED_PREFIX: &str = "trusted.";
pub const XATTR_SECURITY_PREFIX: &str = "security.";
pub const XATTR_SYSTEM_PREFIX: &str = "system.";

/// setxattr bayrakları
pub const XATTR_CREATE: i32 = 1;  // Yalnızca oluştur (zaten varsa hata)
pub const XATTR_REPLACE: i32 = 2; // Yalnızca değiştir (yoksa hata)

// ============================================================================
// XATTR YAPISI
// ============================================================================

/// Tek bir genişletilmiş öznitelik kaydı
#[derive(Clone, Debug)]
pub struct Xattr {
    /// Öznitelik adı (ad alanı öneki dahil)
    pub name: String,
    /// Öznitelik değeri
    pub value: Vec<u8>,
    /// Bayraklar
    pub flags: u32,
}

impl Xattr {
    /// Yeni bir xattr kaydı oluşturur
    pub fn new(name: &str, value: &[u8]) -> Self {
        Self {
            name: String::from(name),
            value: Vec::from(value),
            flags: 0,
        }
    }

    /// Addan ad alanını belirler ve döndürür
    pub fn namespace(&self) -> &str {
        if self.name.starts_with(XATTR_USER_PREFIX) {
            "user"
        } else if self.name.starts_with(XATTR_TRUSTED_PREFIX) {
            "trusted"
        } else if self.name.starts_with(XATTR_SECURITY_PREFIX) {
            "security"
        } else if self.name.starts_with(XATTR_SYSTEM_PREFIX) {
            "system"
        } else {
            "unknown"
        }
    }

    /// Öznitelik adının geçerli olup olmadığını kontrol eder
    pub fn is_valid_name(name: &str) -> bool {
        if name.is_empty() || name.len() > XATTR_NAME_MAX {
            return false;
        }

        // Ad alanı öneki olmalı (nokta içermeli)
        name.contains('.')
    }
}

// ============================================================================
// XATTR YÖNETİCİSİ
// ============================================================================

/// İnode başına genişletilmiş öznitelik deposu
#[derive(Clone, Debug)]
pub struct XattrStorage {
    /// Inode numarası
    pub inode: u64,
    /// Öznitelikler (isim -> değer)
    attrs: BTreeMap<String, Vec<u8>>,
}

impl XattrStorage {
    /// Verilen inode için yeni bir öznitelik deposu oluşturur
    pub fn new(inode: u64) -> Self {
        Self {
            inode,
            attrs: BTreeMap::new(),
        }
    }

    /// Öznitelik değerini döndürür
    pub fn get(&self, name: &str) -> Option<&[u8]> {
        self.attrs.get(name).map(|v| v.as_slice())
    }

    /// Özniteliği ayarlar; XATTR_CREATE ve XATTR_REPLACE bayraklarını destekler
    pub fn set(&mut self, name: &str, value: &[u8], flags: i32) -> Result<(), XattrError> {
        if !Xattr::is_valid_name(name) {
            return Err(XattrError::InvalidName);
        }

        if value.len() > XATTR_VALUE_MAX {
            return Err(XattrError::ValueTooLarge);
        }

        let exists = self.attrs.contains_key(name);

        // Bayrakları kontrol et
        if flags & XATTR_CREATE != 0 && exists {
            return Err(XattrError::AlreadyExists);
        }
        if flags & XATTR_REPLACE != 0 && !exists {
            return Err(XattrError::NotFound);
        }

        self.attrs.insert(String::from(name), Vec::from(value));
        Ok(())
    }

    /// Özniteliği kaldırır
    pub fn remove(&mut self, name: &str) -> Result<(), XattrError> {
        if self.attrs.remove(name).is_some() {
            Ok(())
        } else {
            Err(XattrError::NotFound)
        }
    }

    /// Tüm öznitelik adlarını null ile ayrılmış bayt listesi olarak döndürür
    pub fn list(&self) -> Vec<u8> {
        let mut result = Vec::new();
        for name in self.attrs.keys() {
            result.extend_from_slice(name.as_bytes());
            result.push(0); // null sonlandırıcı
        }
        result
    }

    /// Öznitelik sayısını döndürür
    pub fn count(&self) -> usize {
        self.attrs.len()
    }

    /// Tüm özniteliklerin toplam boyutunu döndürür (isim + değer)
    pub fn total_size(&self) -> usize {
        self.attrs.iter()
            .map(|(k, v)| k.len() + 1 + v.len())
            .sum()
    }
}

/// Global xattr yöneticisi - tüm inode'ların özniteliklerini merkezi olarak yönetir
pub struct XattrManager {
    /// Inode başına depolama
    storage: Mutex<BTreeMap<u64, XattrStorage>>,
    /// Toplam xattr sayısı
    total_xattrs: AtomicU64,
    /// Toplam boyut
    total_size: AtomicU64,
}

impl XattrManager {
    /// Sabit zamanda yeni bir xattr yöneticisi oluşturur
    pub const fn new() -> Self {
        Self {
            storage: Mutex::new(BTreeMap::new()),
            total_xattrs: AtomicU64::new(0),
            total_size: AtomicU64::new(0),
        }
    }

    /// Inode için depolama alanını döndürür; yoksa yeni oluşturur
    fn get_or_create_storage(&self, inode: u64) -> XattrStorage {
        let mut storage = self.storage.lock();
        storage.entry(inode).or_insert_with(|| XattrStorage::new(inode)).clone()
    }

    /// Belirtilen inode'un özniteliğini döndürür
    pub fn get(&self, inode: u64, name: &str) -> Option<Vec<u8>> {
        let storage = self.storage.lock();
        storage.get(&inode).and_then(|s| s.get(name).map(|v| v.to_vec()))
    }

    /// Belirtilen inode'a öznitelik atar ve istatistikleri günceller
    pub fn set(&self, inode: u64, name: &str, value: &[u8], flags: i32) -> Result<(), XattrError> {
        let mut storage = self.storage.lock();
        let entry = storage.entry(inode).or_insert_with(|| XattrStorage::new(inode));

        let old_size = entry.get(name).map(|v| v.len()).unwrap_or(0);
        entry.set(name, value, flags)?;

        // İstatistikleri güncelle
        self.total_xattrs.fetch_add(1, Ordering::Relaxed);
        self.total_size.fetch_add((value.len() - old_size) as u64, Ordering::Relaxed);

        crate::serial_println!(
            "[XATTR] '{}' inode {:#x} üzerine ayarlandı ({} bayt)",
            name, inode, value.len()
        );

        Ok(())
    }

    /// Belirtilen inode'dan özniteliği kaldırır
    pub fn remove(&self, inode: u64, name: &str) -> Result<(), XattrError> {
        let mut storage = self.storage.lock();

        if let Some(entry) = storage.get_mut(&inode) {
            let size = entry.get(name).map(|v| v.len()).unwrap_or(0);
            entry.remove(name)?;

            self.total_xattrs.fetch_sub(1, Ordering::Relaxed);
            self.total_size.fetch_sub(size as u64, Ordering::Relaxed);

            crate::serial_println!("[XATTR] '{}' inode {:#x}'den kaldırıldı", name, inode);
            Ok(())
        } else {
            Err(XattrError::NotFound)
        }
    }

    /// Belirtilen inode'un tüm öznitelik adlarını listeler
    pub fn list(&self, inode: u64) -> Vec<u8> {
        let storage = self.storage.lock();
        storage.get(&inode).map(|s| s.list()).unwrap_or_default()
    }

    /// Bir inode'un tüm özniteliklerini kaldırır (dosya silindiğinde çağrılır)
    pub fn remove_all(&self, inode: u64) {
        let mut storage = self.storage.lock();
        if let Some(entry) = storage.remove(&inode) {
            let count = entry.count() as u64;
            let size = entry.total_size() as u64;
            self.total_xattrs.fetch_sub(count, Ordering::Relaxed);
            self.total_size.fetch_sub(size, Ordering::Relaxed);
        }
    }
}

lazy_static::lazy_static! {
    /// Global xattr yöneticisi
    static ref XATTR_MANAGER: XattrManager = XattrManager::new();
}

// ============================================================================
// HATA TÜRLERİ
// ============================================================================

/// xattr işlemlerinde oluşabilecek hata türleri
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XattrError {
    InvalidName,
    ValueTooLarge,
    NotFound,
    AlreadyExists,
    PermissionDenied,
    NotSupported,
}

// ============================================================================
// SİSTEM ÇAĞRISI ARABIRIMI
// ============================================================================

/// setxattr sistem çağrısı uygulaması - yola göre öznitelik ayarlar
pub fn sys_setxattr(path: &str, name: &str, value: &[u8], flags: i32) -> i32 {
    if !Xattr::is_valid_name(name) {
        return -22; // EINVAL
    }

    if value.len() > XATTR_VALUE_MAX {
        return -7; // E2BIG
    }

    // Yoldan inode al (yer tutucu)
    let inode = hash_path(path);

    // Ad alanı izin kontrolü
    if name.starts_with(XATTR_TRUSTED_PREFIX) || name.starts_with(XATTR_SECURITY_PREFIX) {
        // Bunlar için CAP_SYS_ADMIN gerekir
        // Şimdilik herkese izin verilmektedir
    }

    match XATTR_MANAGER.set(inode, name, value, flags) {
        Ok(()) => 0,
        Err(XattrError::AlreadyExists) => -17, // EEXIST
        Err(XattrError::NotFound) => -2, // ENOENT
        Err(XattrError::InvalidName) => -22, // EINVAL
        Err(XattrError::ValueTooLarge) => -7, // E2BIG
        Err(_) => -5, // EIO
    }
}

/// lsetxattr sistem çağrısı (setxattr ile aynı, sembolik bağları takip etmez)
pub fn sys_lsetxattr(path: &str, name: &str, value: &[u8], flags: i32) -> i32 {
    // Şimdilik setxattr ile aynı
    sys_setxattr(path, name, value, flags)
}

/// fsetxattr sistem çağrısı (dosya tanımlayıcısına göre)
pub fn sys_fsetxattr(fd: i32, name: &str, value: &[u8], flags: i32) -> i32 {
    if fd < 0 {
        return -9; // EBADF
    }

    // Dosya tanımlayıcısından inode al (yer tutucu)
    let inode = fd as u64;

    match XATTR_MANAGER.set(inode, name, value, flags) {
        Ok(()) => 0,
        Err(_) => -5, // EIO
    }
}

/// getxattr sistem çağrısı uygulaması - yola göre öznitelik okur
pub fn sys_getxattr(path: &str, name: &str, buf: &mut [u8]) -> i64 {
    if !Xattr::is_valid_name(name) {
        return -22; // EINVAL
    }

    let inode = hash_path(path);

    match XATTR_MANAGER.get(inode, name) {
        Some(value) => {
            if buf.is_empty() {
                return value.len() as i64;
            }

            if value.len() > buf.len() {
                return -34; // ERANGE
            }

            buf[..value.len()].copy_from_slice(&value);
            value.len() as i64
        }
        None => -61, // ENODATA
    }
}

/// lgetxattr sistem çağrısı (sembolik bağları takip etmez)
pub fn sys_lgetxattr(path: &str, name: &str, buf: &mut [u8]) -> i64 {
    sys_getxattr(path, name, buf)
}

/// fgetxattr sistem çağrısı (dosya tanımlayıcısına göre)
pub fn sys_fgetxattr(fd: i32, name: &str, buf: &mut [u8]) -> i64 {
    if fd < 0 {
        return -9; // EBADF
    }

    let inode = fd as u64;

    match XATTR_MANAGER.get(inode, name) {
        Some(value) => {
            if buf.is_empty() {
                return value.len() as i64;
            }

            if value.len() > buf.len() {
                return -34; // ERANGE
            }

            buf[..value.len()].copy_from_slice(&value);
            value.len() as i64
        }
        None => -61, // ENODATA
    }
}

/// listxattr sistem çağrısı uygulaması - tüm öznitelik adlarını listeler
pub fn sys_listxattr(path: &str, buf: &mut [u8]) -> i64 {
    let inode = hash_path(path);

    let list = XATTR_MANAGER.list(inode);

    if buf.is_empty() {
        return list.len() as i64;
    }

    if list.len() > buf.len() {
        return -34; // ERANGE
    }

    buf[..list.len()].copy_from_slice(&list);
    list.len() as i64
}

/// llistxattr sistem çağrısı (sembolik bağları takip etmez)
pub fn sys_llistxattr(path: &str, buf: &mut [u8]) -> i64 {
    sys_listxattr(path, buf)
}

/// flistxattr sistem çağrısı (dosya tanımlayıcısına göre)
pub fn sys_flistxattr(fd: i32, buf: &mut [u8]) -> i64 {
    if fd < 0 {
        return -9; // EBADF
    }

    let inode = fd as u64;
    let list = XATTR_MANAGER.list(inode);

    if buf.is_empty() {
        return list.len() as i64;
    }

    if list.len() > buf.len() {
        return -34; // ERANGE
    }

    buf[..list.len()].copy_from_slice(&list);
    list.len() as i64
}

/// removexattr sistem çağrısı uygulaması - özniteliği kaldırır
pub fn sys_removexattr(path: &str, name: &str) -> i32 {
    if !Xattr::is_valid_name(name) {
        return -22; // EINVAL
    }

    let inode = hash_path(path);

    match XATTR_MANAGER.remove(inode, name) {
        Ok(()) => 0,
        Err(XattrError::NotFound) => -61, // ENODATA
        Err(_) => -5, // EIO
    }
}

/// lremovexattr sistem çağrısı (sembolik bağları takip etmez)
pub fn sys_lremovexattr(path: &str, name: &str) -> i32 {
    sys_removexattr(path, name)
}

/// fremovexattr sistem çağrısı (dosya tanımlayıcısına göre)
pub fn sys_fremovexattr(fd: i32, name: &str) -> i32 {
    if fd < 0 {
        return -9; // EBADF
    }

    let inode = fd as u64;

    match XATTR_MANAGER.remove(inode, name) {
        Ok(()) => 0,
        Err(XattrError::NotFound) => -61, // ENODATA
        Err(_) => -5, // EIO
    }
}

// ============================================================================
// YARDIMCI FONKSİYONLAR
// ============================================================================

/// Yol için basit hash fonksiyonu - inode yer tutucusu olarak kullanılır
fn hash_path(path: &str) -> u64 {
    let mut hash: u64 = 5381;
    for byte in path.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(byte as u64);
    }
    hash
}

// ============================================================================
// ÖZEL XATTR İŞLEME
// ============================================================================

/// POSIX ACL ile ilgili xattr'lar
pub const XATTR_NAME_POSIX_ACL_ACCESS: &str = "system.posix_acl_access";
pub const XATTR_NAME_POSIX_ACL_DEFAULT: &str = "system.posix_acl_default";

/// SELinux güvenlik bağlamı xattr'ı
pub const XATTR_NAME_SELINUX: &str = "security.selinux";

/// Yetenek (capability) xattr'ları
pub const XATTR_NAME_CAPS: &str = "security.capability";

/// Verilen adın özel bir sistem xattr'ı olup olmadığını kontrol eder
pub fn is_system_xattr(name: &str) -> bool {
    name == XATTR_NAME_POSIX_ACL_ACCESS ||
    name == XATTR_NAME_POSIX_ACL_DEFAULT ||
    name == XATTR_NAME_CAPS
}

/// Verilen adın güvenlik xattr'ı olup olmadığını kontrol eder
pub fn is_security_xattr(name: &str) -> bool {
    name.starts_with(XATTR_SECURITY_PREFIX)
}

// ============================================================================
// GENEL API
// ============================================================================

/// xattr alt sistemini başlatır
pub fn init() {
    crate::serial_println!("[XATTR] Alt sistem başlatıldı");
}

/// xattr istatistik yapısı
pub struct XattrStats {
    pub inode_count: usize,
    pub total_xattrs: u64,
    pub total_size: u64,
}

/// xattr istatistiklerini döndürür (inode sayısı, toplam öznitelik ve boyut)
pub fn get_stats() -> XattrStats {
    XattrStats {
        inode_count: XATTR_MANAGER.storage.lock().len(),
        total_xattrs: XATTR_MANAGER.total_xattrs.load(Ordering::Relaxed),
        total_size: XATTR_MANAGER.total_size.load(Ordering::Relaxed),
    }
}

/// Bir inode'un tüm xattr'larını kaldırır (dosya silindiğinde çağrılır)
pub fn remove_inode_xattrs(inode: u64) {
    XATTR_MANAGER.remove_all(inode);
}
