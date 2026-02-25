//! # echOS Dosya Sistemi
//!
//! Dosya sistemi desteği. Şu anda F2FS, FAT32/exFAT, ext4 ve NTFS implementasyonu mevcut.

pub mod f2fs;
pub mod fat;
pub mod ext4;
pub mod ext4_journal;
pub mod ntfs;
pub mod file_lock;
pub mod inotify;
pub mod xattr;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use lazy_static::lazy_static;
use spin::Mutex;
use rcore_fs::vfs::{FileSystem, FileType, FsError, FsInfo, INode, Metadata, PollStatus, Timespec};

use crate::drivers::ata::BLOCK_SIZE;
use crate::fs::f2fs::{read_f2fs_file_at, write_f2fs_file_at};

struct F2fsVfs;

struct F2fsRootInode;

lazy_static! {
    static ref F2FS_ROOT_INODE: Arc<dyn INode> = Arc::new(F2fsRootInode);
    static ref F2FS_VFS_INSTANCE: Arc<dyn FileSystem> = Arc::new(F2fsVfs);
    /// Global timestamp counter (boot time'dan beri saniye)
    static ref GLOBAL_TIME: Mutex<u64> = Mutex::new(0);
}

/// Global zamanı günceller (her saniye çağrılmalı)
pub fn update_global_time() {
    let mut time = GLOBAL_TIME.lock();
    *time += 1;
}

/// Global zamanı alır
pub fn get_global_time() -> Timespec {
    let time = GLOBAL_TIME.lock();
    Timespec { sec: *time as i64, nsec: 0 }
}

/// Mount point resolution - path'i mount table'a göre çözer
pub fn resolve_mount_path(path: &str) -> String {
    let mounts = crate::fs::f2fs::list_mounts();
    let mut resolved = path.to_string();
    
    // En uzun match'i bul
    for m in mounts {
        if path.starts_with(&m.mountpoint) && m.mountpoint.len() > 1 {
            // Mount point altındaki path'i device path'ine çevir
            let sub_path = &path[m.mountpoint.len()..];
            resolved = format!("{}{}", m.device, sub_path);
            break;
        }
    }
    
    resolved
}

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

fn current_timespec() -> Timespec {
    get_global_time()
}

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
// FILE DESCRIPTOR TABLE (Per-Process)
// ============================================================================

/// Açık dosya bilgisi
pub struct OpenFile {
    pub path: String,
    pub offset: usize,
    pub flags: u32,  // O_RDONLY, O_WRONLY, O_RDWR
}

/// Process başına dosya descriptor table
pub struct FileDescriptorTable {
    pub files: Vec<Option<OpenFile>>,
    pub next_fd: usize,
}

impl FileDescriptorTable {
    pub fn new() -> Self {
        let mut files = Vec::new();
        // stdin, stdout, stderr
        files.push(Some(OpenFile { path: "/dev/stdin".to_string(), offset: 0, flags: 0 }));
        files.push(Some(OpenFile { path: "/dev/stdout".to_string(), offset: 0, flags: 1 }));
        files.push(Some(OpenFile { path: "/dev/stderr".to_string(), offset: 0, flags: 1 }));
        Self { files, next_fd: 3 }
    }
    
    /// Dosya aç, fd döndür
    pub fn open(&mut self, path: &str, flags: u32) -> usize {
        let fd = self.next_fd;
        self.files.push(Some(OpenFile {
            path: path.to_string(),
            offset: 0,
            flags,
        }));
        self.next_fd += 1;
        fd
    }
    
    /// Dosya kapat
    pub fn close(&mut self, fd: usize) -> bool {
        if fd < self.files.len() {
            self.files[fd] = None;
            true
        } else {
            false
        }
    }
    
    /// Offset ayarla (seek)
    pub fn seek(&mut self, fd: usize, offset: usize) -> bool {
        if let Some(Some(file)) = self.files.get_mut(fd) {
            file.offset = offset;
            true
        } else {
            false
        }
    }
    
    /// Offset oku (tell)
    pub fn tell(&self, fd: usize) -> Option<usize> {
        self.files.get(fd).and_then(|f| f.as_ref().map(|f| f.offset))
    }
    
    /// Dosya bilgisini al
    pub fn get(&self, fd: usize) -> Option<&OpenFile> {
        self.files.get(fd).and_then(|f| f.as_ref())
    }
}

lazy_static! {
    /// Global FD table (şimdilik tek process)
    static ref GLOBAL_FD_TABLE: Mutex<FileDescriptorTable> = Mutex::new(FileDescriptorTable::new());
}

/// Global FD table'den dosya aç
pub fn sys_open(path: &str, flags: u32) -> usize {
    GLOBAL_FD_TABLE.lock().open(path, flags)
}

/// Global FD table'den dosya kapat
pub fn sys_close(fd: usize) -> bool {
    GLOBAL_FD_TABLE.lock().close(fd)
}

/// Seek
pub fn sys_seek(fd: usize, offset: usize) -> bool {
    GLOBAL_FD_TABLE.lock().seek(fd, offset)
}

/// Tell
pub fn sys_tell(fd: usize) -> Option<usize> {
    GLOBAL_FD_TABLE.lock().tell(fd)
}

/// FD'den oku
pub fn sys_read(fd: usize, buf: &mut [u8]) -> Result<usize, FsError> {
    let table = GLOBAL_FD_TABLE.lock();
    if let Some(file) = table.get(fd) {
        let path = file.path.clone();
        let offset = file.offset;
        drop(table);
        
        let read = read_f2fs_file_at(&path, offset, buf)?;
        
        // Offset güncelle
        GLOBAL_FD_TABLE.lock().seek(fd, offset + read);
        Ok(read)
    } else {
        Err(FsError::NotFile)
    }
}

/// FD'ye yaz
pub fn sys_write(fd: usize, buf: &[u8]) -> Result<usize, FsError> {
    let table = GLOBAL_FD_TABLE.lock();
    if let Some(file) = table.get(fd) {
        let path = file.path.clone();
        let offset = file.offset;
        drop(table);
        
        let written = write_f2fs_file_at(&path, offset, buf)?;
        
        // Offset güncelle
        GLOBAL_FD_TABLE.lock().seek(fd, offset + written);
        Ok(written)
    } else {
        Err(FsError::NotFile)
    }
}

fn open_inode_by_path(path: &str) -> Result<Arc<dyn INode>, FsError> {
    if path.trim_start_matches('/').is_empty() {
        return Ok(F2FS_ROOT_INODE.clone());
    }
    open_f2fs_inode_by_path(path)
}

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

struct F2fsDirInode {
    path: String,
}

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

pub fn vfs_open_inode(path: &str) -> Result<Arc<dyn INode>, FsError> {
    open_inode_by_path(path)
}

pub fn vfs_inode_metadata(inode: &Arc<dyn INode>) -> Result<Metadata, FsError> {
    inode.metadata()
}

pub fn vfs_read_at(
    inode: &Arc<dyn INode>,
    offset: usize,
    buf: &mut [u8],
) -> Result<usize, FsError> {
    inode.read_at(offset, buf)
}

pub fn vfs_write_at(inode: &Arc<dyn INode>, offset: usize, buf: &[u8]) -> Result<usize, FsError> {
    inode.write_at(offset, buf)
}

pub fn vfs_file_system() -> Arc<dyn FileSystem> {
    F2FS_VFS_INSTANCE.clone()
}
