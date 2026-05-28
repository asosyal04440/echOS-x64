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
use alloc::format;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::drivers::linux::BlockDevice;

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
pub const XATTR_CREATE: i32 = 1; // Yalnızca oluştur (zaten varsa hata)
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
        self.attrs.iter().map(|(k, v)| k.len() + 1 + v.len()).sum()
    }

    /// Tüm (isim, değer) çiftlerini döndürür
    pub fn all_entries(&self) -> Vec<(String, Vec<u8>)> {
        self.attrs.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
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

    /// Inode için xattr'ların önceden yüklenip yüklenmediğini kontrol eder
    pub fn has_cached(&self, inode: u64) -> bool {
        let storage = self.storage.lock();
        storage.contains_key(&inode)
    }

    /// Inode için xattr'ları toplu olarak yükler (FS backend tarafından çağrılır)
    pub fn populate(&self, inode: u64, attrs: Vec<(String, Vec<u8>)>) {
        let mut storage = self.storage.lock();
        let entry = storage.entry(inode).or_insert_with(|| XattrStorage::new(inode));
        for (name, value) in attrs {
            let _ = entry.set(&name, &value, 0);
        }
        self.total_xattrs.fetch_add(entry.count() as u64, Ordering::Relaxed);
        self.total_size.fetch_add(entry.total_size() as u64, Ordering::Relaxed);
    }

    /// Inode için depolama alanını döndürür; yoksa yeni oluşturur
    fn get_or_create_storage(&self, inode: u64) -> XattrStorage {
        let mut storage = self.storage.lock();
        storage
            .entry(inode)
            .or_insert_with(|| XattrStorage::new(inode))
            .clone()
    }

    /// Belirtilen inode'un özniteliğini döndürür
    pub fn get(&self, inode: u64, name: &str) -> Option<Vec<u8>> {
        let storage = self.storage.lock();
        storage
            .get(&inode)
            .and_then(|s| s.get(name).map(|v| v.to_vec()))
    }

    /// Belirtilen inode'a öznitelik atar ve istatistikleri günceller
    pub fn set(&self, inode: u64, name: &str, value: &[u8], flags: i32) -> Result<(), XattrError> {
        let mut storage = self.storage.lock();
        let entry = storage
            .entry(inode)
            .or_insert_with(|| XattrStorage::new(inode));

        let old_size = entry.get(name).map(|v| v.len()).unwrap_or(0);
        entry.set(name, value, flags)?;

        // İstatistikleri güncelle
        self.total_xattrs.fetch_add(1, Ordering::Relaxed);
        self.total_size
            .fetch_add((value.len() - old_size) as u64, Ordering::Relaxed);

        crate::serial_println!(
            "[XATTR] '{}' inode {:#x} üzerine ayarlandı ({} bayt)",
            name,
            inode,
            value.len()
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

    /// Belirtilen inode'un tüm (isim, değer) çiftlerini döndürür
    pub fn get_all(&self, inode: u64) -> Vec<(String, Vec<u8>)> {
        let storage = self.storage.lock();
        storage.get(&inode).map(|s| s.all_entries()).unwrap_or_default()
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

/// ext4 diskinden xattr'ları okuyup XATTR_MANAGER'a yükler
fn ensure_ext4_xattrs_loaded(inode: u64) {
    if XATTR_MANAGER.has_cached(inode) {
        return;
    }

    let mut drive = match crate::drivers::linux::select_block_device() {
        Ok(d) => d,
        Err(_) => return,
    };
    let data = drive.read_sectors(0, 8);
    if data.len() < 4096 {
        return;
    }
    use crate::fs::ext4::{Ext4FileSystem, SUPERBLOCK_OFFSET};
    let mut fs = Ext4FileSystem::new();
    if fs.init(&data).is_err() {
        return;
    }
    let xattrs = fs.read_xattrs(inode as u32, &data);
    if !xattrs.is_empty() {
        XATTR_MANAGER.populate(inode, xattrs);
    }
}

/// XATTR_MANAGER'daki değişiklikleri ext4 diskine yazar
fn flush_ext4_xattrs(inode: u64) {
    use alloc::sync::Arc;
    use crate::fs::ext4::{Ext4FileSystem, Ext4Storage};

    let mut drive = match crate::drivers::linux::select_block_device() {
        Ok(d) => d,
        Err(_) => return,
    };

    let attrs = XATTR_MANAGER.get_all(inode);

    // Read a reasonable chunk (superblock + GDT + inode tables + potential EA block)
    let max_sectors: u32 = 16384; // 8MB in 512-byte sectors
    let sector_sz = 512usize;
    let mut image = Vec::with_capacity(max_sectors as usize * sector_sz);
    for lba in 0..max_sectors {
        let buf = drive.read_sectors(lba, 1);
        if buf.len() < sector_sz {
            break;
        }
        image.extend_from_slice(&buf);
    }
    if image.len() < 4096 {
        return;
    }
    let original = image.clone();

    let mut storage = Ext4Storage::Resident(Arc::new(image));
    let mut fs = Ext4FileSystem::new();
    if fs.init(&original).is_err() {
        return;
    }
    if fs.write_xattrs_to_storage(inode as u32, &attrs, &mut storage).is_err() {
        return;
    }

    // Write modified blocks back
    if let Ext4Storage::Resident(modified) = &storage {
        let modified = &**modified;
        for lba in 0..max_sectors {
            let off = lba as usize * sector_sz;
            if off + sector_sz > modified.len() || off + sector_sz > original.len() {
                break;
            }
            let old_slice = &original[off..off + sector_sz];
            let new_slice = &modified[off..off + sector_sz];
            if old_slice != new_slice {
                let _ = drive.write_sectors(lba, new_slice);
            }
        }
    }
}

/// setxattr sistem çağrısı uygulaması - yola göre öznitelik ayarlar
pub fn sys_setxattr(path: &str, name: &str, value: &[u8], flags: i32) -> i32 {
    if !Xattr::is_valid_name(name) {
        return -22; // EINVAL
    }

    if value.len() > XATTR_VALUE_MAX {
        return -7; // E2BIG
    }

    // Yoldan gerçek inode numarasını al — hash_path kullanma (C7 düzeltme)
    // Cephanelik f2fs-tools xattr.c: get_node_info(sbi, ino, &ni) ile gerçek inode kullanılır
    let inode = match resolve_path_to_inode(path) {
        Some(ino) => ino,
        None => return -2, // ENOENT
    };

    // Ad alanı izin kontrolü
    if name.starts_with(XATTR_TRUSTED_PREFIX) || name.starts_with(XATTR_SECURITY_PREFIX) {
        // Bunlar için CAP_SYS_ADMIN gerekir
        // Şimdilik herkese izin verilmektedir
    }

    match XATTR_MANAGER.set(inode, name, value, flags) {
        Ok(()) => {
            flush_ext4_xattrs(inode);
            0
        }
        Err(XattrError::AlreadyExists) => -17, // EEXIST
        Err(XattrError::NotFound) => -2,       // ENOENT
        Err(XattrError::InvalidName) => -22,   // EINVAL
        Err(XattrError::ValueTooLarge) => -7,  // E2BIG
        Err(_) => -5,                          // EIO
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
        Ok(()) => {
            flush_ext4_xattrs(inode);
            0
        }
        Err(_) => -5, // EIO
    }
}

/// getxattr sistem çağrısı uygulaması - yola göre öznitelik okur
pub fn sys_getxattr(path: &str, name: &str, buf: &mut [u8]) -> i64 {
    if !Xattr::is_valid_name(name) {
        return -22; // EINVAL
    }

    let inode = match resolve_path_to_inode(path) {
        Some(ino) => ino,
        None => return -2, // ENOENT
    };

    // Lazy-load ext4 xattrs from disk
    ensure_ext4_xattrs_loaded(inode);

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
    let inode = match resolve_path_to_inode(path) {
        Some(ino) => ino,
        None => return -2, // ENOENT
    };

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

    let inode = match resolve_path_to_inode(path) {
        Some(ino) => ino,
        None => return -2, // ENOENT
    };

    match XATTR_MANAGER.remove(inode, name) {
        Ok(()) => {
            flush_ext4_xattrs(inode);
            0
        }
        Err(XattrError::NotFound) => -61, // ENODATA
        Err(_) => -5,                     // EIO
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
        Ok(()) => {
            flush_ext4_xattrs(inode);
            0
        }
        Err(XattrError::NotFound) => -61, // ENODATA
        Err(_) => -5,                     // EIO
    }
}

// ============================================================================
// YARDIMCI FONKSİYONLAR
// ============================================================================

/// Yol için gerçek inode numarasını çözer — önce f2fs dener, sonra ext4.
fn resolve_path_to_inode(path: &str) -> Option<u64> {
    let mut drive = match crate::drivers::linux::select_block_device() {
        Ok(d) => d,
        Err(_) => return None,
    };
    // Try F2FS first
    if let Ok(ctx) = crate::fs::f2fs::load_context(&mut *drive) {
        if let Ok(info) = crate::fs::f2fs::open_inode_by_path(&mut *drive, &ctx, path) {
            return Some(info.ino as u64);
        }
    }
    // Try ext4
    let data = drive.read_sectors(0, 8);
    if data.len() >= 4096 {
        use crate::fs::ext4::{Ext4FileSystem, Ext4Superblock, SUPERBLOCK_OFFSET};
        let sb_data = &data[SUPERBLOCK_OFFSET as usize..];
        let sb = Ext4Superblock::parse(sb_data)?;
        let mut fs = Ext4FileSystem::new();
        if fs.init(&data).is_ok() {
            let root_inode = fs.read_inode(2, &data).ok()?;
            let entries = fs.read_dir(&root_inode, &data).ok()?;
            // Simple path walk (single component for now)
            let path_trimmed = path.trim_start_matches('/');
            for entry in &entries {
                if entry.name == path_trimmed {
                    return Some(entry.inode as u64);
                }
            }
        }
    }
    None
}

/// Yol için basit hash fonksiyonu — artık kullanılmıyor, sadece geriye uyumluluk için
#[deprecated(since = "2026-05-17", note = "Use resolve_path_to_inode instead")]
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
    name == XATTR_NAME_POSIX_ACL_ACCESS
        || name == XATTR_NAME_POSIX_ACL_DEFAULT
        || name == XATTR_NAME_CAPS
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

// ============================================================================
// POSIX ACL — Erişim kontrol listeleri
// Deep web: Linux kernel fs/posix_acl.c, include/linux/posix_acl.h
//           POSIX.1-2024 acl_get_file, acl_set_file
//           ext4: system.posix_acl_access, system.posix_acl_default
//
// POSIX ACL yapısı (Linux):
//   acl_header (2 bytes): version (2 bytes)
//   acl_entry[] (8 bytes each):
//     e_tag: u16   — ACL_USER_OBJ, ACL_USER, ACL_GROUP_OBJ, ACL_GROUP, ACL_MASK, ACL_OTHER_OBJ
//     e_perm: u16  — rwx permission bits
//     e_id: u32    — uid/gid (only for ACL_USER, ACL_GROUP)
//
// ACL tag değerleri — include/uapi/linux/posix_acl.h
// ============================================================================

/// ACL tag değerleri
pub const ACL_USER_OBJ: u16 = 0x0001;   // Dosya sahibi
pub const ACL_USER: u16 = 0x0002;       // Belirli kullanıcı
pub const ACL_GROUP_OBJ: u16 = 0x0004;  // Dosya grubu
pub const ACL_GROUP: u16 = 0x0008;      // Belirli grup
pub const ACL_MASK: u16 = 0x0010;       // Mask (en fazla izin)
pub const ACL_OTHER_OBJ: u16 = 0x0020;  // Diğer herkes

/// ACL izin bitleri
pub const ACL_READ: u16 = 0x0004;
pub const ACL_WRITE: u16 = 0x0002;
pub const ACL_EXECUTE: u16 = 0x0001;

/// ACL versiyonu (Linux: 2)
pub const ACL_VERSION: u16 = 2;

/// POSIX ACL girişi
#[derive(Debug, Clone, Copy)]
pub struct AclEntry {
    pub e_tag: u16,
    pub e_perm: u16,
    pub e_id: u32,
}

/// POSIX ACL yapısı
#[derive(Debug, Clone)]
pub struct PosixAcl {
    pub version: u16,
    pub entries: alloc::vec::Vec<AclEntry>,
}

impl PosixAcl {
    /// Yeni boş ACL oluştur
    pub fn new() -> Self {
        Self {
            version: ACL_VERSION,
            entries: alloc::vec::Vec::new(),
        }
    }

    /// Ham baytlardan ACL oluştur (system.posix_acl_access formatı)
    /// Deep web: Linux kernel fs/posix_acl.c posix_acl_from_disk()
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 4 {
            return None;
        }

        let version = u16::from_le_bytes([data[0], data[1]]);
        if version != ACL_VERSION {
            crate::serial_println!("[acl] UYARI: ACL versiyonu={} (beklenen 2)", version);
        }

        let num_entries = u16::from_le_bytes([data[2], data[3]]) as usize;
        let expected_size = 4 + num_entries * 8;
        if data.len() < expected_size {
            crate::serial_println!("[acl] HATA: ACL boyutu yetersiz ({} < {})", data.len(), expected_size);
            return None;
        }

        let mut entries = alloc::vec::Vec::with_capacity(num_entries);
        for i in 0..num_entries {
            let offset = 4 + i * 8;
            let e_tag = u16::from_le_bytes([data[offset], data[offset + 1]]);
            let e_perm = u16::from_le_bytes([data[offset + 2], data[offset + 3]]);
            let e_id = u32::from_le_bytes([
                data[offset + 4],
                data[offset + 5],
                data[offset + 6],
                data[offset + 7],
            ]);
            entries.push(AclEntry { e_tag, e_perm, e_id });
        }

        Some(Self { version, entries })
    }

    /// ACL'i ham baytlara dönüştür
    /// Deep web: Linux kernel fs/posix_acl.c posix_acl_to_disk()
    pub fn to_bytes(&self) -> alloc::vec::Vec<u8> {
        let num_entries = self.entries.len() as u16;
        let mut data = alloc::vec::Vec::with_capacity(4 + self.entries.len() * 8);

        data.extend_from_slice(&self.version.to_le_bytes());
        data.extend_from_slice(&num_entries.to_le_bytes());

        for entry in &self.entries {
            data.extend_from_slice(&entry.e_tag.to_le_bytes());
            data.extend_from_slice(&entry.e_perm.to_le_bytes());
            data.extend_from_slice(&entry.e_id.to_le_bytes());
        }

        data
    }

    /// ACL'e giriş ekle
    pub fn add_entry(&mut self, e_tag: u16, e_perm: u16, e_id: u32) {
        self.entries.push(AclEntry { e_tag, e_perm, e_id });
    }

    /// ACL'den giriş sil
    pub fn remove_entry(&mut self, e_tag: u16, e_id: u32) -> bool {
        if let Some(pos) = self.entries.iter().position(|e| e.e_tag == e_tag && e.e_id == e_id) {
            self.entries.remove(pos);
            true
        } else {
            false
        }
    }

    /// Varsayılan ACL oluştur (normal dosya: owner/group/other)
    pub fn default_from_mode(mode: u16, uid: u32, gid: u32) -> Self {
        let mut acl = Self::new();

        // Owner entry
        let owner_perm = ((mode >> 6) & 0x07) as u16;
        acl.add_entry(ACL_USER_OBJ, owner_perm << 6, uid);

        // Group owner entry
        let group_perm = ((mode >> 3) & 0x07) as u16;
        acl.add_entry(ACL_GROUP_OBJ, group_perm << 3, gid);

        // Mask entry (tüm group izinleri için üst sınır)
        acl.add_entry(ACL_MASK, 0x0007, 0);

        // Other entry
        let other_perm = (mode & 0x07) as u16;
        acl.add_entry(ACL_OTHER_OBJ, other_perm, 0);

        acl
    }

    /// ACL'den izin kontrolü yap
    /// Deep web: Linux kernel fs/posix_acl.c posix_acl_permission()
    pub fn check_permission(&self, uid: u32, gid: u32, access: u32) -> bool {
        // Root her zaman izin verir
        if uid == 0 {
            return true;
        }

        let mut owner_perm = 0u16;
        let mut group_perm = 0u16;
        let mut mask_perm = 0u16;
        let mut other_perm = 0u16;
        let mut named_user_perm = None;
        let mut named_group_perm = None;

        for entry in &self.entries {
            match entry.e_tag {
                ACL_USER_OBJ => owner_perm = entry.e_perm,
                ACL_USER if entry.e_id == uid => {
                    named_user_perm = Some(entry.e_perm);
                }
                ACL_GROUP_OBJ => group_perm = entry.e_perm,
                ACL_GROUP if entry.e_id == gid => {
                    named_group_perm = Some(entry.e_perm);
                }
                ACL_MASK => mask_perm = entry.e_perm,
                ACL_OTHER_OBJ => other_perm = entry.e_perm,
                _ => {}
            }
        }

        // Mask'i uygula
        group_perm &= mask_perm;
        if let Some(p) = named_user_perm {
            // Named user: owner kontrolü, sonra other
            if uid == owner_perm as u32 {
                return (owner_perm & access as u16) == access as u16;
            }
            return (p & mask_perm & access as u16) == access as u16;
        }

        if let Some(p) = named_group_perm {
            return (p & mask_perm & access as u16) == access as u16;
        }

        // Normal POSIX izin kontrolü
        let mode = (owner_perm << 6) | (group_perm << 3) | other_perm;
        let r_bit = (access & 0x04) != 0;
        let w_bit = (access & 0x02) != 0;
        let x_bit = (access & 0x01) != 0;

        if uid == owner_perm as u32 {
            return true; // Owner her zaman izin verir
        }

        if (mode & 0x020) != 0 && r_bit { return true; } // Group read
        if (mode & 0x010) != 0 && w_bit { return true; } // Group write
        if (mode & 0x008) != 0 && x_bit { return true; } // Group execute
        if (mode & 0x004) != 0 && r_bit { return true; } // Other read
        if (mode & 0x002) != 0 && w_bit { return true; } // Other write
        if (mode & 0x001) != 0 && x_bit { return true; } // Other execute

        false
    }
}

/// Dosya için POSIX ACL oku (system.posix_acl_access xattr'ından)
pub fn get_posix_acl(inode: u64) -> Option<PosixAcl> {
    let data = XATTR_MANAGER.get(inode, "system.posix_acl_access")?;
    PosixAcl::from_bytes(&data)
}

/// Dosya için POSIX ACL yaz (system.posix_acl_access xattr'ına)
pub fn set_posix_acl(inode: u64, acl: &PosixAcl) -> bool {
    let data = acl.to_bytes();
    XATTR_MANAGER.set(inode, "system.posix_acl_access", &data, 0).is_ok()
}

/// Dosya için varsayılan ACL oku (dizinler için, system.posix_acl_default)
pub fn get_default_acl(inode: u64) -> Option<PosixAcl> {
    let data = XATTR_MANAGER.get(inode, "system.posix_acl_default")?;
    PosixAcl::from_bytes(&data)
}

/// Dosya için varsayılan ACL yaz (dizinler için)
pub fn set_default_acl(inode: u64, acl: &PosixAcl) -> bool {
    let data = acl.to_bytes();
    XATTR_MANAGER.set(inode, "system.posix_acl_default", &data, 0).is_ok()
}
