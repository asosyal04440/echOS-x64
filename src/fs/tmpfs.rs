//! # tmpfs — RAM-Tabanlı Dosya Sistemi
//!
//! Tamamen bellekte yaşayan, disk I/O'su gerektirmeyen dosya sistemi.
//! `/tmp`, `/run`, `/dev/shm` ve benzeri geçici dizinler için kullanılır.
//!
//! ## Özellikler
//!
//! - **Sıfır disk I/O**: Tüm veriler RAM'de
//! - **Boyut sınırı**: Konfigüre edilebilir (varsayılan: 50% fiziksel RAM)
//! - **POSIX uyumlu**: inode, izinler, zaman damgaları
//! - **Otomatik temizlik**: Unmount'ta tüm veriler silinir

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// SABITLER
// ============================================================================

/// tmpfs magic sayısı
pub const TMPFS_MAGIC: u64 = 0x01021994;
/// Varsayılan boyut sınırı (256 MB)
pub const TMPFS_DEFAULT_SIZE: usize = 256 * 1024 * 1024;
/// Maksimum dosya adı uzunluğu
pub const TMPFS_NAME_MAX: usize = 255;
/// Maksimum inode sayısı
pub const TMPFS_MAX_INODES: usize = 65536;

// ============================================================================
// Inode Türleri
// ============================================================================

/// tmpfs inode türleri
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TmpfsNodeType {
    /// Normal dosya
    RegularFile,
    /// Dizin
    Directory,
    /// Sembolik link
    Symlink,
    /// Paylaşımlı bellek nesnesi (POSIX shm)
    SharedMemory,
    /// Soket dosyası
    Socket,
    /// FIFO (named pipe)
    Fifo,
}

// ============================================================================
// tmpfs Inode
// ============================================================================

/// tmpfs inode yapısı.
///
/// Her dosya/dizin/symlink bir inode ile temsil edilir.
/// Veri doğrudan heap'te `Vec<u8>` olarak saklanır.
#[derive(Debug, Clone)]
pub struct TmpfsInode {
    /// Inode numarası
    pub ino: u64,
    /// Düğüm türü
    pub node_type: TmpfsNodeType,
    /// Dosya izinleri (mode)
    pub mode: u32,
    /// Sahip UID
    pub uid: u32,
    /// Sahip GID
    pub gid: u32,
    /// Link sayısı
    pub nlink: u32,
    /// Dosya boyutu (bayt)
    pub size: u64,
    /// Dosya verileri (yalnızca RegularFile ve SharedMemory)
    pub data: Vec<u8>,
    /// Dizin girişleri (yalnızca Directory)
    pub entries: BTreeMap<String, u64>, // isim → inode no
    /// Sembolik link hedefi (yalnızca Symlink)
    pub symlink_target: String,
    /// Oluşturma zamanı (TSC)
    pub ctime: u64,
    /// Son değişiklik zamanı
    pub mtime: u64,
    /// Son erişim zamanı
    pub atime: u64,
}

impl TmpfsInode {
    /// Yeni dosya inode'u oluşturur.
    pub fn new_file(ino: u64, mode: u32) -> Self {
        let tsc = unsafe { core::arch::x86_64::_rdtsc() };
        Self {
            ino,
            node_type: TmpfsNodeType::RegularFile,
            mode,
            uid: 0,
            gid: 0,
            nlink: 1,
            size: 0,
            data: Vec::new(),
            entries: BTreeMap::new(),
            symlink_target: String::new(),
            ctime: tsc,
            mtime: tsc,
            atime: tsc,
        }
    }

    /// Yeni dizin inode'u oluşturur.
    pub fn new_dir(ino: u64, parent_ino: u64, mode: u32) -> Self {
        let tsc = unsafe { core::arch::x86_64::_rdtsc() };
        let mut entries = BTreeMap::new();
        entries.insert(String::from("."), ino);
        entries.insert(String::from(".."), parent_ino);

        Self {
            ino,
            node_type: TmpfsNodeType::Directory,
            mode,
            uid: 0,
            gid: 0,
            nlink: 2,
            size: 0,
            data: Vec::new(),
            entries,
            symlink_target: String::new(),
            ctime: tsc,
            mtime: tsc,
            atime: tsc,
        }
    }

    /// Yeni symlink inode'u oluşturur.
    pub fn new_symlink(ino: u64, target: &str) -> Self {
        let tsc = unsafe { core::arch::x86_64::_rdtsc() };
        Self {
            ino,
            node_type: TmpfsNodeType::Symlink,
            mode: 0o777,
            uid: 0,
            gid: 0,
            nlink: 1,
            size: target.len() as u64,
            data: Vec::new(),
            entries: BTreeMap::new(),
            symlink_target: String::from(target),
            ctime: tsc,
            mtime: tsc,
            atime: tsc,
        }
    }

    /// Paylaşımlı bellek inode'u (POSIX shm).
    pub fn new_shm(ino: u64, size: usize) -> Self {
        let tsc = unsafe { core::arch::x86_64::_rdtsc() };
        Self {
            ino,
            node_type: TmpfsNodeType::SharedMemory,
            mode: 0o600,
            uid: 0,
            gid: 0,
            nlink: 1,
            size: size as u64,
            data: alloc::vec![0u8; size],
            entries: BTreeMap::new(),
            symlink_target: String::new(),
            ctime: tsc,
            mtime: tsc,
            atime: tsc,
        }
    }

    /// Dosyaya veri yazar (offset'ten itibaren).
    pub fn write(&mut self, offset: usize, buf: &[u8]) -> usize {
        if self.node_type != TmpfsNodeType::RegularFile
            && self.node_type != TmpfsNodeType::SharedMemory
        {
            return 0;
        }

        let end = offset + buf.len();
        if end > self.data.len() {
            self.data.resize(end, 0);
        }
        self.data[offset..end].copy_from_slice(buf);
        self.size = self.data.len() as u64;
        self.mtime = unsafe { core::arch::x86_64::_rdtsc() };
        buf.len()
    }

    /// Dosyadan veri okur.
    pub fn read(&mut self, offset: usize, buf: &mut [u8]) -> usize {
        if offset >= self.data.len() {
            return 0;
        }
        let avail = self.data.len() - offset;
        let len = buf.len().min(avail);
        buf[..len].copy_from_slice(&self.data[offset..offset + len]);
        self.atime = unsafe { core::arch::x86_64::_rdtsc() };
        len
    }

    /// Dosya boyutunu değiştirir (truncate).
    pub fn truncate(&mut self, new_size: u64) {
        self.data.resize(new_size as usize, 0);
        self.size = new_size;
        self.mtime = unsafe { core::arch::x86_64::_rdtsc() };
    }

    /// Dizin mi?
    pub fn is_dir(&self) -> bool {
        self.node_type == TmpfsNodeType::Directory
    }

    /// Normal dosya mı?
    pub fn is_file(&self) -> bool {
        self.node_type == TmpfsNodeType::RegularFile
    }
}

// ============================================================================
// tmpfs Dosya Sistemi
// ============================================================================

/// tmpfs dosya sistemi örneği.
///
/// Her mount noktası (`/tmp`, `/run`, `/dev/shm`) için ayrı bir örnek oluşturulur.
pub struct TmpfsFilesystem {
    /// Etiket (mount noktası)
    pub label: String,
    /// Inode tablosu
    pub inodes: BTreeMap<u64, TmpfsInode>,
    /// Sonraki inode numarası
    next_ino: u64,
    /// Toplam kullanılan bayt
    pub used_bytes: u64,
    /// Boyut sınırı
    pub max_bytes: u64,
    /// Toplam inode sınırı
    pub max_inodes: usize,
}

impl TmpfsFilesystem {
    /// Yeni tmpfs oluşturur.
    pub fn new(label: &str, max_bytes: u64) -> Self {
        let mut fs = Self {
            label: String::from(label),
            inodes: BTreeMap::new(),
            next_ino: 2,
            used_bytes: 0,
            max_bytes,
            max_inodes: TMPFS_MAX_INODES,
        };

        // Kök dizin (inode 1)
        let root = TmpfsInode::new_dir(1, 1, 0o1777);
        fs.inodes.insert(1, root);

        fs
    }

    /// Dosya oluşturur.
    pub fn create_file(&mut self, parent_ino: u64, name: &str, mode: u32) -> Result<u64, i32> {
        if self.inodes.len() >= self.max_inodes {
            return Err(-28); // ENOSPC
        }

        // Parent dizin mi?
        if let Some(parent) = self.inodes.get(&parent_ino) {
            if !parent.is_dir() {
                return Err(-20); // ENOTDIR
            }
            if parent.entries.contains_key(name) {
                return Err(-17); // EEXIST
            }
        } else {
            return Err(-2); // ENOENT
        }

        let ino = self.next_ino;
        self.next_ino += 1;

        let inode = TmpfsInode::new_file(ino, mode);
        self.inodes.insert(ino, inode);

        // Parent'a ekle
        if let Some(parent) = self.inodes.get_mut(&parent_ino) {
            parent.entries.insert(String::from(name), ino);
        }

        Ok(ino)
    }

    /// Alt dizin oluşturur.
    pub fn mkdir(&mut self, parent_ino: u64, name: &str, mode: u32) -> Result<u64, i32> {
        if self.inodes.len() >= self.max_inodes {
            return Err(-28);
        }

        if let Some(parent) = self.inodes.get(&parent_ino) {
            if !parent.is_dir() {
                return Err(-20);
            }
            if parent.entries.contains_key(name) {
                return Err(-17);
            }
        } else {
            return Err(-2);
        }

        let ino = self.next_ino;
        self.next_ino += 1;

        let dir = TmpfsInode::new_dir(ino, parent_ino, mode);
        self.inodes.insert(ino, dir);

        if let Some(parent) = self.inodes.get_mut(&parent_ino) {
            parent.entries.insert(String::from(name), ino);
            parent.nlink += 1;
        }

        Ok(ino)
    }

    /// Dosya siler (unlink).
    pub fn unlink(&mut self, parent_ino: u64, name: &str) -> Result<(), i32> {
        let ino = {
            let parent = self.inodes.get(&parent_ino).ok_or(-2i32)?;
            *parent.entries.get(name).ok_or(-2i32)?
        };

        // Dizin kontrolü
        if let Some(inode) = self.inodes.get(&ino) {
            if inode.is_dir() {
                return Err(-21); // EISDIR
            }
            self.used_bytes = self.used_bytes.saturating_sub(inode.size);
        }

        self.inodes.remove(&ino);
        if let Some(parent) = self.inodes.get_mut(&parent_ino) {
            parent.entries.remove(name);
        }

        Ok(())
    }

    /// Dosayaya yazar.
    pub fn write_file(&mut self, ino: u64, offset: usize, data: &[u8]) -> Result<usize, i32> {
        // Boyut sınırı kontrolü
        let new_used = self.used_bytes + data.len() as u64;
        if new_used > self.max_bytes {
            return Err(-28); // ENOSPC
        }

        if let Some(inode) = self.inodes.get_mut(&ino) {
            let old_size = inode.size;
            let written = inode.write(offset, data);
            self.used_bytes = self.used_bytes + inode.size - old_size;
            Ok(written)
        } else {
            Err(-2)
        }
    }

    /// Dosyadan okur.
    pub fn read_file(&mut self, ino: u64, offset: usize, buf: &mut [u8]) -> Result<usize, i32> {
        if let Some(inode) = self.inodes.get_mut(&ino) {
            Ok(inode.read(offset, buf))
        } else {
            Err(-2)
        }
    }

    /// Dizin içeriğini listeler.
    pub fn readdir(&self, ino: u64) -> Result<Vec<(String, u64, TmpfsNodeType)>, i32> {
        if let Some(inode) = self.inodes.get(&ino) {
            if !inode.is_dir() {
                return Err(-20);
            }
            let mut result = Vec::new();
            for (name, &child_ino) in &inode.entries {
                let child_type = self
                    .inodes
                    .get(&child_ino)
                    .map(|n| n.node_type)
                    .unwrap_or(TmpfsNodeType::RegularFile);
                result.push((name.clone(), child_ino, child_type));
            }
            Ok(result)
        } else {
            Err(-2)
        }
    }

    /// Yol ile inode bul.
    pub fn lookup(&self, path: &str) -> Option<u64> {
        let mut current = 1u64; // root
        for component in path.split('/').filter(|c| !c.is_empty()) {
            if let Some(inode) = self.inodes.get(&current) {
                if let Some(&child) = inode.entries.get(component) {
                    current = child;
                } else {
                    return None;
                }
            } else {
                return None;
            }
        }
        Some(current)
    }

    /// Durum bilgisi
    pub fn statfs(&self) -> TmpfsStatfs {
        TmpfsStatfs {
            f_type: TMPFS_MAGIC,
            f_bsize: 4096,
            f_blocks: self.max_bytes / 4096,
            f_bfree: (self.max_bytes - self.used_bytes) / 4096,
            f_bavail: (self.max_bytes - self.used_bytes) / 4096,
            f_files: self.max_inodes as u64,
            f_ffree: (self.max_inodes - self.inodes.len()) as u64,
            f_namelen: TMPFS_NAME_MAX as u64,
        }
    }

    /// Toplam inode sayısı.
    pub fn inode_count(&self) -> usize {
        self.inodes.len()
    }
}

/// tmpfs statfs bilgisi
#[derive(Debug, Clone)]
pub struct TmpfsStatfs {
    pub f_type: u64,
    pub f_bsize: u64,
    pub f_blocks: u64,
    pub f_bfree: u64,
    pub f_bavail: u64,
    pub f_files: u64,
    pub f_ffree: u64,
    pub f_namelen: u64,
}

// ============================================================================
// Global State
// ============================================================================

lazy_static::lazy_static! {
    /// Mount edilmiş tmpfs örnekleri (her biri kendi per-instance lock'ına sahip)
    static ref TMPFS_INSTANCES: Mutex<BTreeMap<String, Arc<Mutex<TmpfsFilesystem>>>> = Mutex::new(BTreeMap::new());
}

/// Yeni tmpfs mount eder.
pub fn mount(mount_point: &str, max_bytes: u64) -> Result<(), i32> {
    let mut instances = TMPFS_INSTANCES.lock();
    if instances.contains_key(mount_point) {
        return Err(-16); // EBUSY
    }
    let fs = Arc::new(Mutex::new(TmpfsFilesystem::new(mount_point, max_bytes)));
    instances.insert(String::from(mount_point), fs);
    Ok(())
}

/// tmpfs unmount eder — per-instance lock kullanılmaz (map'ten çıkarılır).
pub fn umount(mount_point: &str) -> Result<(), i32> {
    let mut instances = TMPFS_INSTANCES.lock();
    if instances.remove(mount_point).is_some() {
        Ok(())
    } else {
        Err(-22) // EINVAL
    }
}

/// Mount edilmiş tmpfs sayısını döner.
pub fn mounted_count() -> usize {
    TMPFS_INSTANCES.lock().len()
}

/// Belirtilen mount_point için TmpfsFilesystem referansını döndürür.
/// Per-instance lock'ı tutmaz — çağıran kısa süreli kullanım için lock'ı almalıdır.
fn get_fs(mount_point: &str) -> Result<Arc<Mutex<TmpfsFilesystem>>, i32> {
    let instances = TMPFS_INSTANCES.lock();
    instances.get(mount_point).cloned().ok_or(-2i32) // ENOENT
}

/// Dosya oluşturur (per-instance lock ile).
pub fn create_file(mount_point: &str, parent_ino: u64, name: &str, mode: u32) -> Result<u64, i32> {
    let fs = get_fs(mount_point)?;
    let mut guard = fs.lock();
    guard.create_file(parent_ino, name, mode)
}

/// Alt dizin oluşturur (per-instance lock ile).
pub fn mkdir(mount_point: &str, parent_ino: u64, name: &str, mode: u32) -> Result<u64, i32> {
    let fs = get_fs(mount_point)?;
    let mut guard = fs.lock();
    guard.mkdir(parent_ino, name, mode)
}

/// Dosya siler (per-instance lock ile).
pub fn unlink(mount_point: &str, parent_ino: u64, name: &str) -> Result<(), i32> {
    let fs = get_fs(mount_point)?;
    let mut guard = fs.lock();
    guard.unlink(parent_ino, name)
}

/// Dosyaya yazar (per-instance lock ile).
pub fn write_file(mount_point: &str, ino: u64, offset: usize, data: &[u8]) -> Result<usize, i32> {
    let fs = get_fs(mount_point)?;
    let mut guard = fs.lock();
    guard.write_file(ino, offset, data)
}

/// Dosyadan okur (per-instance lock ile).
pub fn read_file(mount_point: &str, ino: u64, offset: usize, buf: &mut [u8]) -> Result<usize, i32> {
    let fs = get_fs(mount_point)?;
    let mut guard = fs.lock();
    guard.read_file(ino, offset, buf)
}

/// Dizin içeriğini listeler (per-instance lock ile).
pub fn readdir(mount_point: &str, ino: u64) -> Result<Vec<(String, u64, TmpfsNodeType)>, i32> {
    let fs = get_fs(mount_point)?;
    let guard = fs.lock();
    guard.readdir(ino)
}

/// Yol ile inode bul (per-instance lock ile).
pub fn lookup(mount_point: &str, path: &str) -> Option<u64> {
    let fs = get_fs(mount_point).ok()?;
    let guard = fs.lock();
    guard.lookup(path)
}

/// Durum bilgisi (per-instance lock ile).
pub fn statfs(mount_point: &str) -> Result<TmpfsStatfs, i32> {
    let fs = get_fs(mount_point)?;
    let guard = fs.lock();
    Ok(guard.statfs())
}

/// Toplam inode sayısı (per-instance lock ile).
pub fn inode_count(mount_point: &str) -> Result<usize, i32> {
    let fs = get_fs(mount_point)?;
    let guard = fs.lock();
    Ok(guard.inode_count())
}

/// Modülü başlatır — varsayılan mount'ları oluşturur.
pub fn init() {
    let _ = mount("/tmp", TMPFS_DEFAULT_SIZE as u64);
    let _ = mount("/run", 64 * 1024 * 1024); // 64 MB
    let _ = mount("/dev/shm", 128 * 1024 * 1024); // 128 MB POSIX shm

    crate::serial_println!("[tmpfs] RAM dosya sistemi başlatıldı");
    crate::serial_println!(
        "[tmpfs] /tmp ({}MB), /run (64MB), /dev/shm (128MB)",
        TMPFS_DEFAULT_SIZE / 1024 / 1024
    );
}
