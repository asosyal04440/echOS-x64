//! # echOS Dosya Sistemi
//!
//! Dosya sistemi desteği. Şu anda F2FS, FAT32/exFAT, ext4 ve NTFS implementasyonu mevcut.
//!
//! ## Sanal Dosya Sistemi (VFS) Katman Mimarisi
//!
//! ```
//!  Kullanıcı Alanı Sistem Çağrıları
//!  open() / read() / write() / close()
//!          │
//!          ▼
//!  ┌────────────────────────────────────────────────┐
//!  │          VFS Katmanı (mod.rs)                  │
//!  │  sys_open / sys_read / sys_write / sys_close   │
//!  │  FileDescriptorTable  (fd → OpenFile)          │
//!  └───────────────┬────────────────────────────────┘
//!                  │
//!       ┌──────────┴──────────┐
//!       ▼                     ▼
//!  ┌─────────┐          ┌──────────┐
//!  │  F2FS   │          │  diğer   │
//!  │ (yerli) │          │ (ext4,   │
//!  │  mod    │          │  NTFS,   │
//!  └────┬────┘          │  FAT32)  │
//!       │               └──────────┘
//!       ▼
//!  ┌────────────────┐
//!  │  INode Arayüzü │  ← rcore-fs::vfs::INode
//!  │  read_at()     │
//!  │  write_at()    │
//!  │  metadata()    │
//!  │  find()        │
//!  └────────────────┘
//!          │
//!          ▼
//!  ┌───────────────────┐
//!  │  ATA/Blok Sürücüsü│
//!  │  (disk I/O)       │
//!  └───────────────────┘
//!
//! ## Dosya Tanımlayıcısı Yaşam Döngüsü
//!
//!  sys_open("dosya", O_RDWR) → fd = 3
//!      │
//!      ├─ FileDescriptorTable'a OpenFile eklenir
//!      │  { path, offset: 0, flags }
//!      │
//!  sys_read(fd=3, buf, 512)
//!      │
//!      ├─ fd → OpenFile.path → F2FS okuma
//!      └─ offset += okundu
//!
//!  sys_close(fd=3)
//!      └─ OpenFile = None (tablo girişi boşaltıldı)
//! ```

pub mod btrfs;
pub mod devfs;
pub mod ext4;
pub mod ext4_journal;
pub mod f2fs;
pub mod fat;
pub mod file_lock;
pub mod inotify;
pub mod mount;
pub mod ntfs;
pub mod overlayfs;
pub mod procfs;
pub mod sysfs;
pub mod tmpfs;
pub mod vfs_unified;
pub mod xattr;
pub mod xfs;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use lazy_static::lazy_static;
use rcore_fs::vfs::{FileSystem, FileType, FsError, FsInfo, INode, Metadata, PollStatus, Timespec};
use spin::Mutex;

use crate::drivers::ata::BLOCK_SIZE;
use crate::fs::f2fs::{read_f2fs_file_at, write_f2fs_file_at};

/// F2FS için dahili sanal dosya sistemi uygulayıcısı
struct F2fsVfs;

/// F2FS kök dizin inode'u — dosya ağacının kökünü temsil eder
struct F2fsRootInode;

lazy_static! {
    /// Kök inode singleton — tüm path çözümlemeleri buradan başlar
    static ref F2FS_ROOT_INODE: Arc<dyn INode> = Arc::new(F2fsRootInode);
    /// VFS singleton — FileSystem trait nesnesi
    static ref F2FS_VFS_INSTANCE: Arc<dyn FileSystem> = Arc::new(F2fsVfs);
    /// Global zaman sayacı (önyüklemeden beri saniye)
    static ref GLOBAL_TIME: Mutex<u64> = Mutex::new(0);
}

/// Global sistem saatini bir saniye ilerletir.
/// Periyodik zamanlayıcı kesmesinden çağrılmalıdır.
pub fn update_global_time() {
    let mut time = GLOBAL_TIME.lock();
    *time += 1;
}

/// Mevcut sistem zamanını POSIX Timespec biçiminde döndürür
pub fn get_global_time() -> Timespec {
    let time = GLOBAL_TIME.lock();
    Timespec {
        sec: *time as i64,
        nsec: 0,
    }
}

/// Bağlama noktası çözümlemesi — path'i bağlama tablosuna göre çözer.
///
/// Örnek:
/// ```
/// /mnt/usb/dosya.txt  →  bağlama tablosu: /mnt/usb → /dev/sdb
///                     →  çözümlendi: /dev/sdb/dosya.txt
/// ```
///
/// En uzun önek eşleşmesi (longest prefix match) algoritması kullanılır.
pub fn resolve_mount_path(path: &str) -> String {
    let mounts = crate::fs::f2fs::list_mounts();
    let mut resolved = path.to_string();

    // En uzun önek eşleşmesini bul
    for m in mounts {
        if path.starts_with(&m.mountpoint) && m.mountpoint.len() > 1 {
            // Bağlama noktası altındaki yolu cihaz yoluna dönüştür
            let sub_path = &path[m.mountpoint.len()..];
            resolved = format!("{}{}", m.device, sub_path);
            break;
        }
    }

    resolved
}

/// Dosya sistemi bilgisi döndürür — statfs() çağrısının temeli
fn fs_info() -> FsInfo {
    FsInfo {
        bsize: BLOCK_SIZE,
        frsize: BLOCK_SIZE,
        blocks: 0,
        bfree: 0,
        bavail: 0,
        files: 0,
        ffree: 0,
        namemax: 255,
    }
}

/// Mevcut zamanı Timespec olarak döndürür (atime/mtime/ctime için)
fn current_timespec() -> Timespec {
    get_global_time()
}

/// Normal dosya meta verisi oluşturur.
/// mode 0o100644 = düzenli dosya, sahibi okuma/yazma, diğerleri okuma izni
fn file_metadata(size: usize) -> Metadata {
    let time = current_timespec();
    Metadata {
        dev: 0,
        inode: 1,
        size,
        blk_size: BLOCK_SIZE,
        blocks: 0,
        atime: time,
        mtime: time,
        ctime: time,
        type_: FileType::File,
        mode: 0o100644,
        nlinks: 1,
        uid: 0,
        gid: 0,
        rdev: 0,
    }
}

/// Dizin meta verisi oluşturur.
/// mode 0o040755 = dizin, sahibi tam izin, diğerleri okuma+çalıştırma
fn dir_metadata() -> Metadata {
    let time = current_timespec();
    Metadata {
        dev: 0,
        inode: 1,
        size: 0,
        blk_size: BLOCK_SIZE,
        blocks: 0,
        atime: time,
        mtime: time,
        ctime: time,
        type_: FileType::Dir,
        mode: 0o040755,
        nlinks: 2,
        uid: 0,
        gid: 0,
        rdev: 0,
    }
}

// ============================================================================
// DOSYA TANIM LAYICISI TABLOSU (Her İşlem İçin Ayrı)
// ============================================================================

/// Açık bir dosyayı temsil eden kayıt.
/// Unix'te her açık dosya bu kayıtla izlenir.
pub struct OpenFile {
    pub path: String,
    /// Mevcut okuma/yazma konumu (her read/write sonrası güncellenir)
    pub offset: usize,
    pub flags: u32, // O_RDONLY=0, O_WRONLY=1, O_RDWR=2
}

/// İşlem başına dosya tanımlayıcı tablosu.
///
/// ```
/// Tablo Yapısı:
/// ┌────┬────────────────────────────────────────────┐
/// │ fd │ OpenFile                                   │
/// ├────┼────────────────────────────────────────────┤
/// │  0 │ /dev/stdin   (flags=O_RDONLY, offset=0)    │
/// │  1 │ /dev/stdout  (flags=O_WRONLY, offset=0)    │
/// │  2 │ /dev/stderr  (flags=O_WRONLY, offset=0)    │
/// │  3 │ /home/user/dosya.txt (uygulama açtı)       │
/// │  4 │ None  (kapatıldı)                          │
/// │  5 │ /var/log/app.log                           │
/// └────┴────────────────────────────────────────────┘
/// next_fd = 6 (bir sonraki open() bu fd'yi tahsis eder)
/// ```
pub struct FileDescriptorTable {
    pub files: Vec<Option<OpenFile>>,
    pub next_fd: usize,
}

impl FileDescriptorTable {
    pub fn new() -> Self {
        let mut files = Vec::new();
        // stdin, stdout, stderr — standart akışlar daima 0, 1, 2 fd değerlerini alır
        files.push(Some(OpenFile {
            path: "/dev/stdin".to_string(),
            offset: 0,
            flags: 0,
        }));
        files.push(Some(OpenFile {
            path: "/dev/stdout".to_string(),
            offset: 0,
            flags: 1,
        }));
        files.push(Some(OpenFile {
            path: "/dev/stderr".to_string(),
            offset: 0,
            flags: 1,
        }));
        Self { files, next_fd: 3 }
    }

    /// Dosyayı açar ve yeni fd döndürür.
    /// Tablo büyür; kapatılan fd numaraları yeniden kullanılmaz (basit uygulama).
    pub fn open(&mut self, path: &str, flags: u32) -> usize {
        // FD slot recycling: önce None olan slotları tara (0-2 stdin/stdout/stderr atla)
        for i in 3..self.files.len() {
            if self.files[i].is_none() {
                self.files[i] = Some(OpenFile {
                    path: path.to_string(),
                    offset: 0,
                    flags,
                });
                return i;
            }
        }
        // Boş slot yoksa tabloyu genişlet
        let fd = self.files.len();
        self.files.push(Some(OpenFile {
            path: path.to_string(),
            offset: 0,
            flags,
        }));
        self.next_fd = fd + 1;
        fd
    }

    /// Dosyayı kapatır — tablo girişini None yapar, fd numarası serbest kalır
    pub fn close(&mut self, fd: usize) -> bool {
        if fd < self.files.len() {
            self.files[fd] = None;
            true
        } else {
            false
        }
    }

    /// Dosya konumunu ayarlar (lseek benzeri)
    pub fn seek(&mut self, fd: usize, offset: usize) -> bool {
        if let Some(Some(file)) = self.files.get_mut(fd) {
            file.offset = offset;
            true
        } else {
            false
        }
    }

    /// Mevcut dosya konumunu döndürür (tell)
    pub fn tell(&self, fd: usize) -> Option<usize> {
        self.files
            .get(fd)
            .and_then(|f| f.as_ref().map(|f| f.offset))
    }

    /// fd'ye ait OpenFile kaydını okuma amaçlı getirir
    pub fn get(&self, fd: usize) -> Option<&OpenFile> {
        self.files.get(fd).and_then(|f| f.as_ref())
    }
}

lazy_static! {
    /// Global FD tablosu — şu anlık tek işlem varsayımıyla global tutulur.
    /// Çok işlemli bir sistemde bu tablo her process'e özel olmalıdır.
    static ref GLOBAL_FD_TABLE: Mutex<FileDescriptorTable> = Mutex::new(FileDescriptorTable::new());
}

/// Dosya açar, tahsis edilen fd numarasını döndürür (open syscall)
pub fn sys_open(path: &str, flags: u32) -> usize {
    GLOBAL_FD_TABLE.lock().open(path, flags)
}

/// Dosyayı kapatır (close syscall)
pub fn sys_close(fd: usize) -> bool {
    GLOBAL_FD_TABLE.lock().close(fd)
}

/// Dosya konumunu değiştirir (lseek syscall)
pub fn sys_seek(fd: usize, offset: usize) -> bool {
    GLOBAL_FD_TABLE.lock().seek(fd, offset)
}

/// Mevcut dosya konumunu döndürür (tell / lseek SEEK_CUR benzeri)
pub fn sys_tell(fd: usize) -> Option<usize> {
    GLOBAL_FD_TABLE.lock().tell(fd)
}

/// fd'den okur ve offset'i günceller (read syscall).
///
/// Path/offset'i lock altında alır, I/O'yu lock dışı yapar, sonra offset'i günceller.
pub fn sys_read(fd: usize, buf: &mut [u8]) -> Result<usize, FsError> {
    let (path, offset) = {
        let table = GLOBAL_FD_TABLE.lock();
        let file = table
            .files
            .get(fd)
            .and_then(|f| f.as_ref())
            .ok_or(FsError::NotFile)?;
        (file.path.clone(), file.offset)
    };

    let read = read_f2fs_file_at(&path, offset, buf)?;

    let mut table = GLOBAL_FD_TABLE.lock();
    if let Some(Some(file)) = table.files.get_mut(fd) {
        file.offset = offset + read;
    }
    Ok(read)
}

/// fd'ye yazar ve offset'i günceller (write syscall).
///
/// Path/offset'i lock altında alır, I/O'yu lock dışı yapar, sonra offset'i günceller.
pub fn sys_write(fd: usize, buf: &[u8]) -> Result<usize, FsError> {
    let (path, offset) = {
        let table = GLOBAL_FD_TABLE.lock();
        let file = table
            .files
            .get(fd)
            .and_then(|f| f.as_ref())
            .ok_or(FsError::NotFile)?;
        (file.path.clone(), file.offset)
    };

    let written = write_f2fs_file_at(&path, offset, buf)?;

    let mut table = GLOBAL_FD_TABLE.lock();
    if let Some(Some(file)) = table.files.get_mut(fd) {
        file.offset = offset + written;
    }
    Ok(written)
}

/// Path'e göre inode açar — kök dizin özel durumu vardır
fn open_inode_by_path(path: &str) -> Result<Arc<dyn INode>, FsError> {
    if path.trim_start_matches('/').is_empty() {
        return Ok(F2FS_ROOT_INODE.clone());
    }
    open_f2fs_inode_by_path(path)
}

/// F2FS dosya sistemi üzerinde path'e karşılık gelen inode'u döndürür.
/// Dizin ise F2fsDirInode, dosyaysa F2fsFileInode döndürülür.
fn open_f2fs_inode_by_path(path: &str) -> Result<Arc<dyn INode>, FsError> {
    let entry = crate::fs::f2fs::open_entry(path)?;
    if entry.is_dir {
        Ok(Arc::new(F2fsDirInode { path: entry.name }))
    } else {
        Ok(Arc::new(F2fsFileInode {
            path: entry.name,
            size: entry.size as usize,
        }))
    }
}

impl FileSystem for F2fsVfs {
    fn sync(&self) -> Result<(), FsError> {
        Ok(())
    }

    fn root_inode(&self) -> Arc<dyn INode> {
        F2FS_ROOT_INODE.clone()
    }

    fn info(&self) -> FsInfo {
        fs_info()
    }
}

impl INode for F2fsRootInode {
    fn read_at(&self, _offset: usize, _buf: &mut [u8]) -> Result<usize, FsError> {
        Err(FsError::NotFile)
    }

    fn write_at(&self, _offset: usize, _buf: &[u8]) -> Result<usize, FsError> {
        Err(FsError::NotSupported)
    }

    fn poll(&self) -> Result<PollStatus, FsError> {
        Ok(PollStatus {
            read: false,
            write: false,
            error: false,
        })
    }

    fn metadata(&self) -> Result<Metadata, FsError> {
        Ok(dir_metadata())
    }

    fn find(&self, name: &str) -> Result<Arc<dyn INode>, FsError> {
        if name.is_empty() || name == "." {
            return Ok(F2FS_ROOT_INODE.clone());
        }
        let normalized = if name.starts_with('/') {
            name.to_string()
        } else {
            format!("/{}", name)
        };
        open_f2fs_inode_by_path(&normalized)
    }

    fn as_any_ref(&self) -> &dyn core::any::Any {
        self
    }
}

/// F2FS dizin inode'u — bir dizini temsil eder
struct F2fsDirInode {
    path: String,
}

/// F2FS dosya inode'u — belirli boyuttaki bir dosyayı temsil eder
struct F2fsFileInode {
    path: String,
    size: usize,
}

impl INode for F2fsDirInode {
    fn read_at(&self, _offset: usize, _buf: &mut [u8]) -> Result<usize, FsError> {
        Err(FsError::NotFile)
    }

    fn write_at(&self, _offset: usize, _buf: &[u8]) -> Result<usize, FsError> {
        Err(FsError::NotSupported)
    }

    fn poll(&self) -> Result<PollStatus, FsError> {
        Ok(PollStatus {
            read: false,
            write: false,
            error: false,
        })
    }

    fn metadata(&self) -> Result<Metadata, FsError> {
        Ok(dir_metadata())
    }

    fn find(&self, name: &str) -> Result<Arc<dyn INode>, FsError> {
        if name.is_empty() || name == "." {
            return Ok(Arc::new(F2fsDirInode {
                path: self.path.clone(),
            }));
        }
        let normalized = if self.path.ends_with('/') {
            format!("{}{}", self.path, name)
        } else {
            format!("{}/{}", self.path, name)
        };
        open_f2fs_inode_by_path(&normalized)
    }

    fn as_any_ref(&self) -> &dyn core::any::Any {
        self
    }
}

impl INode for F2fsFileInode {
    fn read_at(&self, offset: usize, buf: &mut [u8]) -> Result<usize, FsError> {
        read_f2fs_file_at(&self.path, offset, buf)
    }

    fn write_at(&self, offset: usize, buf: &[u8]) -> Result<usize, FsError> {
        write_f2fs_file_at(&self.path, offset, buf)
    }

    fn poll(&self) -> Result<PollStatus, FsError> {
        Ok(PollStatus {
            read: true,
            write: true,
            error: false,
        })
    }

    fn metadata(&self) -> Result<Metadata, FsError> {
        Ok(file_metadata(self.size))
    }

    fn as_any_ref(&self) -> &dyn core::any::Any {
        self
    }
}

/// VFS arayüzü — path üzerinden inode açar
/// /proc, /dev ve /sys sanal dosya sistemlerini kontrol eder;
/// bulunamazsa gerçek disk dosya sistemine (F2FS) yönlendirir
pub fn vfs_open_inode(path: &str) -> Result<Arc<dyn INode>, FsError> {
    // Sanal dosya sistemlerini önce kontrol et
    if procfs::is_proc_path(path) {
        return procfs::open_proc_inode(path);
    }
    if devfs::is_dev_path(path) {
        return devfs::open_dev_inode(path);
    }
    if sysfs::is_sys_path(path) {
        return sysfs::open_sys_inode(path);
    }
    // Gerçek dosya sistemi (F2FS, ext4, FAT32, NTFS)
    open_inode_by_path(path)
}

/// Bir inode'un meta verisini döndürür (stat benzeri)
pub fn vfs_inode_metadata(inode: &Arc<dyn INode>) -> Result<Metadata, FsError> {
    inode.metadata()
}

/// Inode üzerinden belirli bir ofsetten okur
pub fn vfs_read_at(
    inode: &Arc<dyn INode>,
    offset: usize,
    buf: &mut [u8],
) -> Result<usize, FsError> {
    inode.read_at(offset, buf)
}

/// Inode üzerinden belirli bir ofsete yazar
pub fn vfs_write_at(inode: &Arc<dyn INode>, offset: usize, buf: &[u8]) -> Result<usize, FsError> {
    inode.write_at(offset, buf)
}

/// Küresel VFS dosya sistemi örneğini döndürür
pub fn vfs_file_system() -> Arc<dyn FileSystem> {
    F2FS_VFS_INSTANCE.clone()
}

// ---------- Convenience VFS helpers for GUI apps ----------

/// Verilen yoldaki dosyayı okuyup String olarak döndürür.
/// Dosya bulunamazsa veya okunamazsa `None` döner.
pub fn read_to_string(path: &str) -> Option<String> {
    let root = F2FS_ROOT_INODE.clone();
    let inode = root.lookup(path).ok()?;
    let meta = inode.metadata().ok()?;
    let size = meta.size;
    let mut buf = alloc::vec![0u8; size];
    let n = inode.read_at(0, &mut buf).ok()?;
    buf.truncate(n);
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// Verilen yoldaki dosyaya metin yazar. Başarılıysa `true` döner.
pub fn write_string(path: &str, content: &str) -> bool {
    let root = F2FS_ROOT_INODE.clone();
    // Dosya yoksa oluştur
    let inode = match root.lookup(path) {
        Ok(i) => i,
        Err(_) => match root.create(path, FileType::File, 0o644) {
            Ok(i) => i,
            Err(_) => return false,
        },
    };
    inode.write_at(0, content.as_bytes()).is_ok()
}

/// Bir dizinin içeriğini `(isim, is_dir)` çiftleri olarak döndürür.
/// F2FS VFS read_dir_entries fonksiyonunu kullanarak gerçek dizin içeriğini okur.
pub fn read_dir(path: &str) -> Option<Vec<(String, bool)>> {
    let mut drive = crate::drivers::linux::select_block_device().ok()?;
    let ctx = f2fs::load_context(&mut *drive).ok()?;
    let inode = f2fs::open_inode_by_path(&mut *drive, &ctx, path).ok()?;
    if !inode.is_dir {
        return None;
    }
    let entries = f2fs::read_dir_entries(&mut *drive, &ctx, &inode).ok()?;
    let result: Vec<(String, bool)> = entries
        .into_iter()
        .filter(|e| e.name != "." && e.name != "..")
        .map(|e| (e.name, e.is_dir))
        .collect();
    Some(result)
}
