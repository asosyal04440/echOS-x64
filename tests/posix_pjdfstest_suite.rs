//! # pjdfstest — Endüstriyel POSIX Dosya Sistemi Test Süiti
//!
//! `pjd/pjdfstest` (https://github.com/pjd/pjdfstest) POSIX dosya sistemi test
//! süitinden esinlenilerek echOS VFS katmanı için hazırlanmıştır.
//!
//! Test kategorileri (pjdfstest ile aynı sırada):
//!   1. chmod    — mod bitleri, setuid/setgid/sticky, izin kontrolleri
//!   2. chown    — uid/gid değişikliği, ownership semantics
//!   3. ftruncate— açık fd üzerinden truncate
//!   4. link     — hard link: nlink artışı, dizine link engeli, EEXIST
//!   5. mkdir    — dizin oluşturma, EEXIST, ENOENT (eksik üst), iç içe
//!   6. mkfifo   — FIFO özel dosyası oluşturma
//!   7. mknod    — blok/karakter cihaz dosyası oluşturma
//!   8. open     — O_CREAT, O_EXCL, O_TRUNC, O_APPEND, EISDIR
//!   9. rename   — dosya/dizin taşıma, üzerine yazma, ENOENT, cross-dir
//!  10. rmdir    — boş olmayan dizin, ENOTDIR, kök silinemez
//!  11. symlink  — sembolik link, dangling, readlink,循环
//!  12. truncate — path ile truncate, sıfıra indirme, büyütme (zero-fill)
//!  13. unlink   — hardlink sayısı ile silme, EPERM (dizin), EISDIR
//!  14. utimensat— erişim/değişim zamanı güncellemesi
//!
//! Bu dosya `cargo test --test posix_pjdfstest_suite` ile çalıştırılır.
//! `#![cfg(not(target_os = "none"))]` — host ortamında simüle edilir.

#![cfg(not(target_os = "none"))]

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};

// ============================================================================
// POSIX SABITLERI
// ============================================================================

const O_RDONLY: u32 = 0;
const O_WRONLY: u32 = 1;
const O_RDWR: u32 = 2;
const O_CREAT: u32 = 0o100;
const O_EXCL: u32 = 0o200;
const O_TRUNC: u32 = 0o1000;
const O_APPEND: u32 = 0o2000;

const S_IFMT: u32 = 0o170000;
const S_IFREG: u32 = 0o100000;
const S_IFDIR: u32 = 0o040000;
const S_IFLNK: u32 = 0o120000;
const S_IFIFO: u32 = 0o010000;
const S_IFCHR: u32 = 0o020000;
const S_IFBLK: u32 = 0o060000;
const S_ISUID: u32 = 0o4000;
const S_ISGID: u32 = 0o2000;
const S_ISVTX: u32 = 0o1000;

// ============================================================================
// INODE VE DOSYA SISTEMI
// ============================================================================

static NEXT_INO: AtomicU64 = AtomicU64::new(100);

fn alloc_ino() -> u64 {
    NEXT_INO.fetch_add(1, Ordering::SeqCst)
}

#[derive(Debug, Clone)]
enum InodeKind {
    Regular { data: Vec<u8>, nlink: u32 },
    Dir { entries: BTreeMap<String, u64> },
    Symlink { target: String },
    Fifo,
    CharDev { major: u32, minor: u32 },
    BlockDev { major: u32, minor: u32 },
}

#[derive(Debug, Clone)]
struct PosixInode {
    ino: u64,
    kind: InodeKind,
    mode: u32,   // permission bits + type bits
    uid: u32,
    gid: u32,
    size: u64,
    atime: u64,
    mtime: u64,
    ctime: u64,
}

#[derive(Debug, Clone)]
struct OpenFile {
    ino: u64,
    offset: usize,
    flags: u32,
    append: bool,
}

struct PosixFs {
    inodes: HashMap<u64, PosixInode>,
    open_files: HashMap<usize, OpenFile>,
    next_fd: usize,
    now: u64,
}

// ============================================================================
// YARDIMCI: test icin dosya/dizin olusturma
// ============================================================================

impl PosixFs {
    fn new() -> Self {
        let root_ino = 1;
        let mut inodes = HashMap::new();
        inodes.insert(root_ino, PosixInode {
            ino: root_ino,
            kind: InodeKind::Dir { entries: BTreeMap::new() },
            mode: S_IFDIR | 0o755,
            uid: 0, gid: 0,
            size: 0,
            atime: 0, mtime: 0, ctime: 0,
        });
        Self { inodes, open_files: HashMap::new(), next_fd: 3, now: 0 }
    }

    fn tick(&mut self) { self.now += 1; }

    // ---------- path çözümleme ----------

    fn path_parts(path: &str) -> Vec<&str> {
        path.split('/').filter(|s| !s.is_empty() && *s != ".").collect()
    }

    fn lookup(&self, path: &str) -> Option<u64> {
        if path == "/" || path.is_empty() { return Some(1); }
        let mut cur = 1u64;
        for part in Self::path_parts(path) {
            let inode = self.inodes.get(&cur)?;
            match &inode.kind {
                InodeKind::Dir { entries } => { cur = *entries.get(part)?; }
                _ => return None,
            }
        }
        Some(cur)
    }

    fn parent_path(path: &str) -> (String, String) {
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let name = parts.last().unwrap_or(&"").to_string();
        let parent = if parts.len() <= 1 {
            "/".to_string()
        } else {
            format!("/{}", parts[..parts.len()-1].join("/"))
        };
        (parent, name)
    }

    fn parent_ino(&self, path: &str) -> Result<u64, &'static str> {
        let (parent, _) = Self::parent_path(path);
        self.lookup(&parent).ok_or("ENOENT")
    }

    // ---------- open ----------

    fn open(&mut self, path: &str, flags: u32) -> Result<usize, &'static str> {
        let creat = flags & O_CREAT != 0;
        let excl  = flags & O_EXCL  != 0;
        let trunc = flags & O_TRUNC != 0;
        let append = flags & O_APPEND != 0;

        let ino = match self.lookup(path) {
            Some(ino) => {
                if excl && creat { return Err("EEXIST"); }
                // O_TRUNC: mevcut dosyayı sıfırla
                if trunc {
                    let inode = self.inodes.get_mut(&ino).ok_or("ENOENT")?;
                    match &mut inode.kind {
                        InodeKind::Regular { data, .. } => {
                            data.clear();
                            inode.size = 0;
                            inode.mtime = self.now;
                            inode.ctime = self.now;
                        }
                        InodeKind::Dir { .. } => { return Err("EISDIR"); }
                        _ => {}
                    }
                }
                ino
            }
            None => {
                if !creat { return Err("ENOENT"); }
                // Yeni dosya oluştur
                let p_ino = self.parent_ino(path)?;
                let (_, name) = Self::parent_path(path);
                let new_ino = alloc_ino();
                let parent = self.inodes.get_mut(&p_ino).ok_or("ENOENT")?;
                match &mut parent.kind {
                    InodeKind::Dir { entries } => {
                        entries.insert(name, new_ino);
                    }
                    _ => { return Err("ENOTDIR"); }
                }
                parent.ctime = self.now;
                parent.mtime = self.now;
                self.inodes.insert(new_ino, PosixInode {
                    ino: new_ino,
                    kind: InodeKind::Regular { data: Vec::new(), nlink: 1 },
                    mode: S_IFREG | 0o644,
                    uid: 0, gid: 0,
                    size: 0,
                    atime: self.now, mtime: self.now, ctime: self.now,
                });
                new_ino
            }
        };

        let fd = self.next_fd;
        self.next_fd += 1;
        self.open_files.insert(fd, OpenFile {
            ino, offset: 0, flags, append,
        });
        if let Some(inode) = self.inodes.get_mut(&ino) {
            inode.atime = self.now;
        }
        Ok(fd)
    }

    fn read(&mut self, fd: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
        let of = self.open_files.get(&fd).ok_or("EBADF")?;
        let ino = of.ino;
        let off = of.offset;
        let inode = self.inodes.get(&ino).ok_or("ENOENT")?;
        match &inode.kind {
            InodeKind::Regular { data, .. } => {
                let avail = data.len().saturating_sub(off);
                let n = buf.len().min(avail);
                buf[..n].copy_from_slice(&data[off..off+n]);
                let of = self.open_files.get_mut(&fd).unwrap();
                of.offset += n;
                Ok(n)
            }
            InodeKind::Dir { .. } => Err("EISDIR"),
            _ => Err("EINVAL"),
        }
    }

    fn write(&mut self, fd: usize, data: &[u8]) -> Result<usize, &'static str> {
        let of = self.open_files.get(&fd).ok_or("EBADF")?;
        if of.flags & O_WRONLY == 0 && of.flags & O_RDWR == 0 {
            return Err("EBADF"); // read-only fd
        }
        let ino = of.ino;
        let append = of.append;
        let inode = self.inodes.get_mut(&ino).ok_or("ENOENT")?;
        match &mut inode.kind {
            InodeKind::Regular { data: file_data, .. } => {
                let off = if append { file_data.len() } else { of.offset };
                let end = off + data.len();
                if end > file_data.len() { file_data.resize(end, 0); }
                file_data[off..end].copy_from_slice(data);
                inode.size = file_data.len() as u64;
                inode.mtime = self.now;
                let of = self.open_files.get_mut(&fd).unwrap();
                of.offset = end;
                Ok(data.len())
            }
            InodeKind::Dir { .. } => Err("EISDIR"),
            _ => Err("EINVAL"),
        }
    }

    fn close(&mut self, fd: usize) -> Result<(), &'static str> {
        self.open_files.remove(&fd).ok_or("EBADF")?;
        Ok(())
    }

    fn ftruncate(&mut self, fd: usize, new_size: u64) -> Result<(), &'static str> {
        let of = self.open_files.get(&fd).ok_or("EBADF")?;
        if of.flags & O_WRONLY == 0 && of.flags & O_RDWR == 0 {
            return Err("EBADF");
        }
        let ino = of.ino;
        let inode = self.inodes.get_mut(&ino).ok_or("ENOENT")?;
        match &mut inode.kind {
            InodeKind::Regular { data, .. } => {
                data.resize(new_size as usize, 0);
                inode.size = new_size;
                inode.mtime = self.now;
                inode.ctime = self.now;
                Ok(())
            }
            InodeKind::Dir { .. } => Err("EINVAL"),
            _ => Err("EINVAL"),
        }
    }

    // ---------- chmod ----------

    fn chmod(&mut self, path: &str, mode: u32) -> Result<(), &'static str> {
        let ino = self.lookup(path).ok_or("ENOENT")?;
        let inode = self.inodes.get_mut(&ino).ok_or("ENOENT")?;
        // Type bits korunur, permission + special bits güncellenir
        let type_bits = inode.mode & S_IFMT;
        inode.mode = type_bits | (mode & !S_IFMT);
        inode.ctime = self.now;
        Ok(())
    }

    // ---------- chown ----------

    fn chown(&mut self, path: &str, uid: u32, gid: u32) -> Result<(), &'static str> {
        let ino = self.lookup(path).ok_or("ENOENT")?;
        let inode = self.inodes.get_mut(&ino).ok_or("ENOENT")?;
        // POSIX: chown setuid/setgid bitlerini temizler (non-root için)
        if inode.uid != 0 { // root değilse
            inode.mode &= !(S_ISUID | S_ISGID);
        }
        inode.uid = uid;
        inode.gid = gid;
        inode.ctime = self.now;
        Ok(())
    }

    // ---------- truncate (path ile) ----------

    fn truncate(&mut self, path: &str, new_size: u64) -> Result<(), &'static str> {
        let ino = self.lookup(path).ok_or("ENOENT")?;
        let inode = self.inodes.get_mut(&ino).ok_or("ENOENT")?;
        match &mut inode.kind {
            InodeKind::Regular { data, .. } => {
                data.resize(new_size as usize, 0);
                inode.size = new_size;
                inode.mtime = self.now;
                inode.ctime = self.now;
                Ok(())
            }
            InodeKind::Dir { .. } => Err("EISDIR"),
            _ => Err("EINVAL"),
        }
    }

    // ---------- mkdir ----------

    fn mkdir(&mut self, path: &str, mode: u32) -> Result<(), &'static str> {
        let p_ino = self.parent_ino(path)?;
        let (_, name) = Self::parent_path(path);
        let parent = self.inodes.get_mut(&p_ino).ok_or("ENOENT")?;
        match &mut parent.kind {
            InodeKind::Dir { entries } => {
                if entries.contains_key(&name) { return Err("EEXIST"); }
                let new_ino = alloc_ino();
                entries.insert(name, new_ino);
                parent.ctime = self.now;
                parent.mtime = self.now;
                self.inodes.insert(new_ino, PosixInode {
                    ino: new_ino,
                    kind: InodeKind::Dir { entries: BTreeMap::new() },
                    mode: S_IFDIR | (mode & 0o7777),
                    uid: 0, gid: 0,
                    size: 0,
                    atime: self.now, mtime: self.now, ctime: self.now,
                });
                Ok(())
            }
            _ => Err("ENOTDIR"),
        }
    }

    // ---------- rmdir ----------

    fn rmdir(&mut self, path: &str) -> Result<(), &'static str> {
        if path == "/" { return Err("EBUSY"); }
        let ino = self.lookup(path).ok_or("ENOENT")?;
        let inode = self.inodes.get(&ino).ok_or("ENOENT")?;
        match &inode.kind {
            InodeKind::Dir { entries } => {
                if !entries.is_empty() { return Err("ENOTEMPTY"); }
            }
            _ => { return Err("ENOTDIR"); }
        }
        let (parent, name) = Self::parent_path(path);
        let p_ino = self.lookup(&parent).ok_or("ENOENT")?;
        let parent_inode = self.inodes.get_mut(&p_ino).ok_or("ENOENT")?;
        if let InodeKind::Dir { entries } = &mut parent_inode.kind {
            entries.remove(&name);
        }
        self.inodes.remove(&ino);
        Ok(())
    }

    // ---------- link (hard link) ----------

    fn link(&mut self, old: &str, new: &str) -> Result<(), &'static str> {
        let old_ino = self.lookup(old).ok_or("ENOENT")?;
        // POSIX: dizine hard link yasak
        let inode = self.inodes.get(&old_ino).ok_or("ENOENT")?;
        match &inode.kind {
            InodeKind::Dir { .. } => { return Err("EPERM"); }
            _ => {}
        }
        let p_ino = self.parent_ino(new)?;
        let (_, name) = Self::parent_path(new);
        let parent = self.inodes.get_mut(&p_ino).ok_or("ENOENT")?;
        match &mut parent.kind {
            InodeKind::Dir { entries } => {
                if entries.contains_key(&name) { return Err("EEXIST"); }
                entries.insert(name, old_ino);
            }
            _ => { return Err("ENOTDIR"); }
        }
        let inode = self.inodes.get_mut(&old_ino).ok_or("ENOENT")?;
        if let InodeKind::Regular { nlink, .. } = &mut inode.kind {
            *nlink += 1;
        }
        inode.ctime = self.now;
        Ok(())
    }

    // ---------- unlink ----------

    fn unlink(&mut self, path: &str) -> Result<(), &'static str> {
        let ino = self.lookup(path).ok_or("ENOENT")?;
        let inode = self.inodes.get(&ino).ok_or("ENOENT")?;
        match &inode.kind {
            InodeKind::Dir { .. } => { return Err("EISDIR"); }
            InodeKind::Regular { nlink, .. } => {
                let last = *nlink <= 1;
                if !last {
                    let inode = self.inodes.get_mut(&ino).unwrap();
                    if let InodeKind::Regular { nlink: nl, .. } = &mut inode.kind {
                        *nl -= 1;
                    }
                }
                if last { self.inodes.remove(&ino); }
            }
            _ => {
                // Symlink, FIFO vs. — sadece directory entry silinir
                self.inodes.remove(&ino);
            }
        }
        let (parent, name) = Self::parent_path(path);
        let p_ino = self.lookup(&parent).ok_or("ENOENT")?;
        let parent = self.inodes.get_mut(&p_ino).ok_or("ENOENT")?;
        if let InodeKind::Dir { entries } = &mut parent.kind {
            entries.remove(&name);
        }
        Ok(())
    }

    // ---------- symlink ----------

    fn symlink(&mut self, target: &str, linkpath: &str) -> Result<(), &'static str> {
        let p_ino = self.parent_ino(linkpath)?;
        let (_, name) = Self::parent_path(linkpath);
        let parent = self.inodes.get_mut(&p_ino).ok_or("ENOENT")?;
        match &mut parent.kind {
            InodeKind::Dir { entries } => {
                if entries.contains_key(&name) { return Err("EEXIST"); }
                let new_ino = alloc_ino();
                entries.insert(name, new_ino);
                parent.mtime = self.now;
                self.inodes.insert(new_ino, PosixInode {
                    ino: new_ino,
                    kind: InodeKind::Symlink { target: target.to_string() },
                    mode: S_IFLNK | 0o777,
                    uid: 0, gid: 0,
                    size: target.len() as u64,
                    atime: self.now, mtime: self.now, ctime: self.now,
                });
                Ok(())
            }
            _ => Err("ENOTDIR"),
        }
    }

    fn readlink(&self, path: &str) -> Result<String, &'static str> {
        let ino = self.lookup(path).ok_or("ENOENT")?;
        let inode = self.inodes.get(&ino).ok_or("ENOENT")?;
        match &inode.kind {
            InodeKind::Symlink { target } => Ok(target.clone()),
            _ => Err("EINVAL"),
        }
    }

    // ---------- rename ----------

    fn rename(&mut self, old: &str, new: &str) -> Result<(), &'static str> {
        let old_ino = self.lookup(old).ok_or("ENOENT")?;
        let (old_parent, old_name) = Self::parent_path(old);
        let (new_parent, new_name) = Self::parent_path(new);

        let new_p_ino = self.lookup(&new_parent).ok_or("ENOENT")?;
        // Hedef varsa sil (aynı tür kontrolü)
        let existing = {
            let np = self.inodes.get(&new_p_ino).ok_or("ENOENT")?;
            match &np.kind {
                InodeKind::Dir { entries } => entries.get(&new_name).copied(),
                _ => { return Err("ENOTDIR"); }
            }
        };
        if let Some(exist_ino) = existing {
            let exist_inode = self.inodes.get(&exist_ino).ok_or("ENOENT")?;
            let old_inode = self.inodes.get(&old_ino).ok_or("ENOENT")?;
            // POSIX: dizin -> dosya rename yasak, tam tersi de
            match (&old_inode.kind, &exist_inode.kind) {
                (InodeKind::Dir { .. }, InodeKind::Dir { entries }) => {
                    if !entries.is_empty() { return Err("ENOTEMPTY"); }
                }
                (InodeKind::Dir { .. }, _) => { return Err("ENOTDIR"); }
                (_, InodeKind::Dir { .. }) => { return Err("EISDIR"); }
                _ => {}
            }
            // Mevcut hedefi sil
            let np = self.inodes.get_mut(&new_p_ino).unwrap();
            if let InodeKind::Dir { entries } = &mut np.kind {
                entries.remove(&new_name);
            }
            self.inodes.remove(&exist_ino);
        }

        // Yeni parent'a ekle
        let np = self.inodes.get_mut(&new_p_ino).ok_or("ENOENT")?;
        if let InodeKind::Dir { entries } = &mut np.kind {
            entries.insert(new_name.clone(), old_ino);
        }
        np.mtime = self.now;

        // Eski parent'tan sil
        let old_p_ino = self.lookup(&old_parent).ok_or("ENOENT")?;
        let op = self.inodes.get_mut(&old_p_ino).ok_or("ENOENT")?;
        if let InodeKind::Dir { entries } = &mut op.kind {
            entries.remove(&old_name);
        }
        op.mtime = self.now;

        Ok(())
    }

    // ---------- mkfifo ----------

    fn mkfifo(&mut self, path: &str, mode: u32) -> Result<(), &'static str> {
        let p_ino = self.parent_ino(path)?;
        let (_, name) = Self::parent_path(path);
        let parent = self.inodes.get_mut(&p_ino).ok_or("ENOENT")?;
        match &mut parent.kind {
            InodeKind::Dir { entries } => {
                if entries.contains_key(&name) { return Err("EEXIST"); }
                let new_ino = alloc_ino();
                entries.insert(name, new_ino);
                parent.mtime = self.now;
                self.inodes.insert(new_ino, PosixInode {
                    ino: new_ino,
                    kind: InodeKind::Fifo,
                    mode: S_IFIFO | (mode & 0o7777),
                    uid: 0, gid: 0,
                    size: 0,
                    atime: self.now, mtime: self.now, ctime: self.now,
                });
                Ok(())
            }
            _ => Err("ENOTDIR"),
        }
    }

    // ---------- mknod ----------

    fn mknod_chr(&mut self, path: &str, mode: u32, major: u32, minor: u32) -> Result<(), &'static str> {
        let p_ino = self.parent_ino(path)?;
        let (_, name) = Self::parent_path(path);
        let parent = self.inodes.get_mut(&p_ino).ok_or("ENOENT")?;
        match &mut parent.kind {
            InodeKind::Dir { entries } => {
                if entries.contains_key(&name) { return Err("EEXIST"); }
                let new_ino = alloc_ino();
                entries.insert(name, new_ino);
                parent.mtime = self.now;
                self.inodes.insert(new_ino, PosixInode {
                    ino: new_ino,
                    kind: InodeKind::CharDev { major, minor },
                    mode: S_IFCHR | (mode & 0o7777),
                    uid: 0, gid: 0,
                    size: 0,
                    atime: self.now, mtime: self.now, ctime: self.now,
                });
                Ok(())
            }
            _ => Err("ENOTDIR"),
        }
    }

    fn mknod_blk(&mut self, path: &str, mode: u32, major: u32, minor: u32) -> Result<(), &'static str> {
        let p_ino = self.parent_ino(path)?;
        let (_, name) = Self::parent_path(path);
        let parent = self.inodes.get_mut(&p_ino).ok_or("ENOENT")?;
        match &mut parent.kind {
            InodeKind::Dir { entries } => {
                if entries.contains_key(&name) { return Err("EEXIST"); }
                let new_ino = alloc_ino();
                entries.insert(name, new_ino);
                parent.mtime = self.now;
                self.inodes.insert(new_ino, PosixInode {
                    ino: new_ino,
                    kind: InodeKind::BlockDev { major, minor },
                    mode: S_IFBLK | (mode & 0o7777),
                    uid: 0, gid: 0,
                    size: 0,
                    atime: self.now, mtime: self.now, ctime: self.now,
                });
                Ok(())
            }
            _ => Err("ENOTDIR"),
        }
    }

    // ---------- utimensat ----------

    fn utimensat(&mut self, path: &str, atime: u64, mtime: u64) -> Result<(), &'static str> {
        let ino = self.lookup(path).ok_or("ENOENT")?;
        let inode = self.inodes.get_mut(&ino).ok_or("ENOENT")?;
        inode.atime = atime;
        inode.mtime = mtime;
        inode.ctime = self.now;
        Ok(())
    }

    // ---------- stat ----------

    fn stat(&self, path: &str) -> Result<PosixInode, &'static str> {
        let ino = self.lookup(path).ok_or("ENOENT")?;
        self.inodes.get(&ino).cloned().ok_or("ENOENT")
    }

    fn stat_fd(&self, fd: usize) -> Result<PosixInode, &'static str> {
        let of = self.open_files.get(&fd).ok_or("EBADF")?;
        self.inodes.get(&of.ino).cloned().ok_or("ENOENT")
    }

    fn nlink(&self, path: &str) -> Result<u32, &'static str> {
        let inode = self.stat(path)?;
        match &inode.kind {
            InodeKind::Regular { nlink, .. } => Ok(*nlink),
            InodeKind::Dir { .. } => Ok(2), // simplified
            _ => Ok(1),
        }
    }
}

// ============================================================================
// 1. CHMOD TESTLERI
// ============================================================================

#[test]
fn chmod_basic_permission_change() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/chd", 0o755).unwrap();
    fs.tick();
    let _ = fs.open("/chd/f.txt", O_CREAT | O_WRONLY).unwrap();

    fs.chmod("/chd/f.txt", 0o600).unwrap();
    let st = fs.stat("/chd/f.txt").unwrap();
    assert_eq!(st.mode & 0o777, 0o600);
    // Type bit korunmuş olmalı
    assert_eq!(st.mode & S_IFMT, S_IFREG);
}

#[test]
fn chmod_setuid_setgid_sticky() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/chd2", 0o755).unwrap();
    fs.tick();
    let _ = fs.open("/chd2/suid", O_CREAT | O_WRONLY).unwrap();

    // setuid + executable
    fs.chmod("/chd2/suid", S_ISUID | 0o755).unwrap();
    let st = fs.stat("/chd2/suid").unwrap();
    assert_ne!(st.mode & S_ISUID, 0);
    assert_eq!(st.mode & 0o777, 0o755);

    // setgid
    fs.chmod("/chd2/suid", S_ISGID | 0o750).unwrap();
    let st = fs.stat("/chd2/suid").unwrap();
    assert_ne!(st.mode & S_ISGID, 0);
    assert_eq!(st.mode & S_ISUID, 0); // setuid temizlenmiş

    // sticky bit (dizin)
    fs.chmod("/chd2", S_ISVTX | 0o1777).unwrap();
    let st = fs.stat("/chd2").unwrap();
    assert_ne!(st.mode & S_ISVTX, 0);
}

#[test]
fn chmod_enoent_on_missing_path() {
    let mut fs = PosixFs::new();
    fs.tick();
    let res = fs.chmod("/nonexistent", 0o644);
    assert_eq!(res, Err("ENOENT"));
}

#[test]
fn chmod_updates_ctime() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/ct", 0o755).unwrap();
    fs.tick();
    let _ = fs.open("/ct/f", O_CREAT | O_WRONLY).unwrap();
    let st_before = fs.stat("/ct/f").unwrap();

    fs.tick();
    fs.chmod("/ct/f", 0o700).unwrap();
    let st_after = fs.stat("/ct/f").unwrap();
    assert!(st_after.ctime > st_before.ctime);
}

// ============================================================================
// 2. CHOWN TESTLERI
// ============================================================================

#[test]
fn chown_changes_uid_gid() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/cho", 0o755).unwrap();
    fs.tick();
    let _ = fs.open("/cho/f", O_CREAT | O_WRONLY).unwrap();

    fs.chown("/cho/f", 1000, 1000).unwrap();
    let st = fs.stat("/cho/f").unwrap();
    assert_eq!(st.uid, 1000);
    assert_eq!(st.gid, 1000);
}

#[test]
fn chown_clears_setuid_setgid_for_nonroot() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/cho2", 0o755).unwrap();
    fs.tick();
    let _ = fs.open("/cho2/f", O_CREAT | O_WRONLY).unwrap();
    // Setuid+setgid ayarla, uid=1000 (non-root)
    fs.chown("/cho2/f", 1000, 0).unwrap();
    fs.chmod("/cho2/f", S_ISUID | S_ISGID | 0o755).unwrap();
    let st = fs.stat("/cho2/f").unwrap();
    assert_ne!(st.mode & S_ISUID, 0);

    // chown çağrılınca (uid!=0 olduğu için) setuid/setgid temizlenmeli
    fs.chown("/cho2/f", 2000, 2000).unwrap();
    let st = fs.stat("/cho2/f").unwrap();
    assert_eq!(st.mode & S_ISUID, 0);
    assert_eq!(st.mode & S_ISGID, 0);
    assert_eq!(st.uid, 2000);
}

#[test]
fn chown_enoent_missing_path() {
    let mut fs = PosixFs::new();
    let res = fs.chown("/no_such", 0, 0);
    assert_eq!(res, Err("ENOENT"));
}

#[test]
fn chown_updates_ctime() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/cho3", 0o755).unwrap();
    fs.tick();
    let _ = fs.open("/cho3/f", O_CREAT | O_WRONLY).unwrap();
    let ct_before = fs.stat("/cho3/f").unwrap().ctime;
    fs.tick();
    fs.chown("/cho3/f", 500, 500).unwrap();
    let ct_after = fs.stat("/cho3/f").unwrap().ctime;
    assert!(ct_after > ct_before);
}

// ============================================================================
// 3. FTRUNCATE TESTLERI
// ============================================================================

#[test]
fn ftruncate_shrink() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/ft", 0o755).unwrap();
    fs.tick();
    let fd = fs.open("/ft/big", O_CREAT | O_WRONLY).unwrap();
    fs.write(fd, b"0123456789").unwrap();

    fs.ftruncate(fd, 5).unwrap();
    let st = fs.stat_fd(fd).unwrap();
    assert_eq!(st.size, 5);

    fs.close(fd).unwrap();
    let fd2 = fs.open("/ft/big", O_RDONLY).unwrap();
    let mut buf = [0u8; 10];
    let n = fs.read(fd2, &mut buf).unwrap();
    assert_eq!(&buf[..n], b"01234");
    fs.close(fd2).unwrap();
}

#[test]
fn ftruncate_grow_zero_fill() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/ft2", 0o755).unwrap();
    fs.tick();
    let fd = fs.open("/ft2/small", O_CREAT | O_WRONLY).unwrap();
    fs.write(fd, b"hi").unwrap();

    fs.ftruncate(fd, 10).unwrap();
    let st = fs.stat_fd(fd).unwrap();
    assert_eq!(st.size, 10);

    fs.close(fd).unwrap();
    let fd2 = fs.open("/ft2/small", O_RDONLY).unwrap();
    let mut buf = [0u8; 10];
    let n = fs.read(fd2, &mut buf).unwrap();
    assert_eq!(n, 10);
    assert_eq!(&buf[..2], b"hi");
    assert_eq!(&buf[2..], &[0u8; 8]);
    fs.close(fd2).unwrap();
}

#[test]
fn ftruncate_to_zero() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/ft3", 0o755).unwrap();
    fs.tick();
    let fd = fs.open("/ft3/z", O_CREAT | O_WRONLY).unwrap();
    fs.write(fd, b"data").unwrap();

    fs.ftruncate(fd, 0).unwrap();
    let st = fs.stat_fd(fd).unwrap();
    assert_eq!(st.size, 0);
    fs.close(fd).unwrap();
}

#[test]
fn ftruncate_ebadf_on_readonly_fd() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/ft4", 0o755).unwrap();
    fs.tick();
    let fd_w = fs.open("/ft4/ro", O_CREAT | O_WRONLY).unwrap();
    fs.write(fd_w, b"x").unwrap();
    fs.close(fd_w).unwrap();

    let fd_r = fs.open("/ft4/ro", O_RDONLY).unwrap();
    let res = fs.ftruncate(fd_r, 0);
    assert_eq!(res, Err("EBADF"));
    fs.close(fd_r).unwrap();
}

// ============================================================================
// 4. LINK TESTLERI
// ============================================================================

#[test]
fn link_hard_link_increments_nlink() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/ln", 0o755).unwrap();
    fs.tick();
    let fd = fs.open("/ln/orig", O_CREAT | O_WRONLY).unwrap();
    fs.write(fd, b"shared").unwrap();
    fs.close(fd).unwrap();

    assert_eq!(fs.nlink("/ln/orig").unwrap(), 1);

    fs.link("/ln/orig", "/ln/hard").unwrap();
    assert_eq!(fs.nlink("/ln/orig").unwrap(), 2);
    assert_eq!(fs.nlink("/ln/hard").unwrap(), 2);

    // Aynı inode'a işaret eder
    let o_ino = fs.lookup("/ln/orig").unwrap();
    let h_ino = fs.lookup("/ln/hard").unwrap();
    assert_eq!(o_ino, h_ino);
}

#[test]
fn link_directory_is_eperm() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/ln2", 0o755).unwrap();
    fs.mkdir("/ln2/sub", 0o755).unwrap();
    fs.tick();

    let res = fs.link("/ln2/sub", "/ln2/sublink");
    assert_eq!(res, Err("EPERM"));
}

#[test]
fn link_eexist_when_target_exists() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/ln3", 0o755).unwrap();
    fs.tick();
    let _ = fs.open("/ln3/a", O_CREAT | O_WRONLY).unwrap();
    let _ = fs.open("/ln3/b", O_CREAT | O_WRONLY).unwrap();

    let res = fs.link("/ln3/a", "/ln3/b");
    assert_eq!(res, Err("EEXIST"));
}

#[test]
fn link_enoent_source_missing() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/ln4", 0o755).unwrap();
    fs.tick();
    let res = fs.link("/ln4/nope", "/ln4/link");
    assert_eq!(res, Err("ENOENT"));
}

// ============================================================================
// 5. MKDIR TESTLERI
// ============================================================================

#[test]
fn mkdir_creates_directory_with_correct_mode() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/md", 0o755).unwrap();
    let st = fs.stat("/md").unwrap();
    assert_eq!(st.mode & S_IFMT, S_IFDIR);
    assert_eq!(st.mode & 0o777, 0o755);
}

#[test]
fn mkdir_eexist_when_exists() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/md2", 0o755).unwrap();
    let res = fs.mkdir("/md2", 0o755);
    assert_eq!(res, Err("EEXIST"));
}

#[test]
fn mkdir_enoent_parent_missing() {
    let mut fs = PosixFs::new();
    fs.tick();
    let res = fs.mkdir("/noparent/child", 0o755);
    assert_eq!(res, Err("ENOENT"));
}

#[test]
fn mkdir_nested_path() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/a", 0o755).unwrap();
    fs.tick();
    fs.mkdir("/a/b", 0o755).unwrap();
    fs.tick();
    fs.mkdir("/a/b/c", 0o755).unwrap();

    assert!(fs.lookup("/a/b/c").is_some());
    let st = fs.stat("/a/b/c").unwrap();
    assert_eq!(st.mode & S_IFMT, S_IFDIR);
}

#[test]
fn mkdir_updates_parent_mtime() {
    let mut fs = PosixFs::new();
    fs.tick();
    let mt_before = fs.stat("/").unwrap().mtime;
    fs.tick();
    fs.mkdir("/mdt", 0o755).unwrap();
    let mt_after = fs.stat("/").unwrap().mtime;
    assert!(mt_after > mt_before);
}

// ============================================================================
// 6. MKFIFO TESTLERI
// ============================================================================

#[test]
fn mkfifo_creates_fifo_node() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/fifo", 0o755).unwrap();
    fs.tick();
    fs.mkfifo("/fifo/pipe", 0o644).unwrap();
    let st = fs.stat("/fifo/pipe").unwrap();
    assert_eq!(st.mode & S_IFMT, S_IFIFO);
    assert_eq!(st.mode & 0o777, 0o644);
    assert_eq!(st.size, 0);
}

#[test]
fn mkfifo_eexist_when_exists() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/fifo2", 0o755).unwrap();
    fs.tick();
    fs.mkfifo("/fifo2/p", 0o644).unwrap();
    let res = fs.mkfifo("/fifo2/p", 0o644);
    assert_eq!(res, Err("EEXIST"));
}

// ============================================================================
// 7. MKNOD TESTLERI
// ============================================================================

#[test]
fn mknod_creates_char_device() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/dev", 0o755).unwrap();
    fs.tick();
    fs.mknod_chr("/dev/null", 0o666, 1, 3).unwrap();
    let st = fs.stat("/dev/null").unwrap();
    assert_eq!(st.mode & S_IFMT, S_IFCHR);
    assert_eq!(st.mode & 0o777, 0o666);
}

#[test]
fn mknod_creates_block_device() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/dev2", 0o755).unwrap();
    fs.tick();
    fs.mknod_blk("/dev2/sda", 0o660, 8, 0).unwrap();
    let st = fs.stat("/dev2/sda").unwrap();
    assert_eq!(st.mode & S_IFMT, S_IFBLK);
    assert_eq!(st.mode & 0o777, 0o660);
}

#[test]
fn mknod_eexist() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/dev3", 0o755).unwrap();
    fs.tick();
    fs.mknod_chr("/dev3/x", 0o666, 1, 1).unwrap();
    let res = fs.mknod_chr("/dev3/x", 0o666, 1, 2);
    assert_eq!(res, Err("EEXIST"));
}

// ============================================================================
// 8. OPEN TESTLERI
// ============================================================================

#[test]
fn open_creat_creates_new_file() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/op", 0o755).unwrap();
    fs.tick();
    let fd = fs.open("/op/new.txt", O_CREAT | O_WRONLY).unwrap();
    assert!(fd >= 3);
    fs.close(fd).unwrap();
    assert!(fs.lookup("/op/new.txt").is_some());
}

#[test]
fn open_creat_excl_fails_if_exists() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/op2", 0o755).unwrap();
    fs.tick();
    let fd = fs.open("/op2/f", O_CREAT | O_WRONLY).unwrap();
    fs.close(fd).unwrap();

    let res = fs.open("/op2/f", O_CREAT | O_EXCL | O_WRONLY);
    assert_eq!(res, Err("EEXIST"));
}

#[test]
fn open_enoent_without_creat() {
    let mut fs = PosixFs::new();
    let res = fs.open("/missing", O_RDONLY);
    assert_eq!(res, Err("ENOENT"));
}

#[test]
fn open_trunc_zeros_existing_file() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/op3", 0o755).unwrap();
    fs.tick();
    let fd = fs.open("/op3/t", O_CREAT | O_WRONLY).unwrap();
    fs.write(fd, b"some data").unwrap();
    fs.close(fd).unwrap();

    let fd2 = fs.open("/op3/t", O_WRONLY | O_TRUNC).unwrap();
    let st = fs.stat_fd(fd2).unwrap();
    assert_eq!(st.size, 0);
    fs.close(fd2).unwrap();
}

#[test]
fn open_append_writes_at_end() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/op4", 0o755).unwrap();
    fs.tick();
    let fd = fs.open("/op4/a", O_CREAT | O_WRONLY).unwrap();
    fs.write(fd, b"hello").unwrap();
    fs.close(fd).unwrap();

    let fd2 = fs.open("/op4/a", O_WRONLY | O_APPEND).unwrap();
    fs.write(fd2, b" world").unwrap();
    fs.close(fd2).unwrap();

    let fd3 = fs.open("/op4/a", O_RDONLY).unwrap();
    let mut buf = [0u8; 20];
    let n = fs.read(fd3, &mut buf).unwrap();
    assert_eq!(&buf[..n], b"hello world");
    fs.close(fd3).unwrap();
}

#[test]
fn open_directory_for_write_is_eisdir() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/op5", 0o755).unwrap();
    fs.tick();
    // O_TRUNC ile dizini açmaya çalış
    let res = fs.open("/op5", O_WRONLY | O_TRUNC);
    assert_eq!(res, Err("EISDIR"));
}

// ============================================================================
// 9. RENAME TESTLERI
// ============================================================================

#[test]
fn rename_moves_file_to_new_parent() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/src", 0o755).unwrap();
    fs.mkdir("/dst", 0o755).unwrap();
    fs.tick();
    let fd = fs.open("/src/f.txt", O_CREAT | O_WRONLY).unwrap();
    fs.write(fd, b"payload").unwrap();
    fs.close(fd).unwrap();

    fs.rename("/src/f.txt", "/dst/g.txt").unwrap();
    assert!(fs.lookup("/src/f.txt").is_none());
    assert!(fs.lookup("/dst/g.txt").is_some());
}

#[test]
fn rename_overwrites_existing_file() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/rn", 0o755).unwrap();
    fs.tick();
    let fd1 = fs.open("/rn/a", O_CREAT | O_WRONLY).unwrap();
    fs.write(fd1, b"aaa").unwrap();
    fs.close(fd1).unwrap();
    let fd2 = fs.open("/rn/b", O_CREAT | O_WRONLY).unwrap();
    fs.write(fd2, b"bbb").unwrap();
    fs.close(fd2).unwrap();

    fs.rename("/rn/a", "/rn/b").unwrap();
    assert!(fs.lookup("/rn/a").is_none());
    // b artık a'nın içeriğine sahip
    let fd3 = fs.open("/rn/b", O_RDONLY).unwrap();
    let mut buf = [0u8; 10];
    let n = fs.read(fd3, &mut buf).unwrap();
    assert_eq!(&buf[..n], b"aaa");
    fs.close(fd3).unwrap();
}

#[test]
fn rename_enoent_source_missing() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/rn2", 0o755).unwrap();
    fs.tick();
    let res = fs.rename("/rn2/nope", "/rn2/dest");
    assert_eq!(res, Err("ENOENT"));
}

#[test]
fn rename_directory() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/rn3", 0o755).unwrap();
    fs.mkdir("/rn3/olddir", 0o755).unwrap();
    fs.tick();
    let _ = fs.open("/rn3/olddir/child", O_CREAT | O_WRONLY).unwrap();

    fs.rename("/rn3/olddir", "/rn3/newdir").unwrap();
    assert!(fs.lookup("/rn3/olddir").is_none());
    assert!(fs.lookup("/rn3/newdir").is_some());
    assert!(fs.lookup("/rn3/newdir/child").is_some());
}

#[test]
fn rename_dir_over_nonempty_dir_is_enotempty() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/rn4", 0o755).unwrap();
    fs.mkdir("/rn4/a", 0o755).unwrap();
    fs.mkdir("/rn4/b", 0o755).unwrap();
    fs.tick();
    let _ = fs.open("/rn4/b/child", O_CREAT | O_WRONLY).unwrap();

    let res = fs.rename("/rn4/a", "/rn4/b");
    assert_eq!(res, Err("ENOTEMPTY"));
}

// ============================================================================
// 10. RMDIR TESTLERI
// ============================================================================

#[test]
fn rmdir_removes_empty_directory() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/rm", 0o755).unwrap();
    fs.tick();
    fs.mkdir("/rm/empty", 0o755).unwrap();

    fs.rmdir("/rm/empty").unwrap();
    assert!(fs.lookup("/rm/empty").is_none());
}

#[test]
fn rmdir_enotempty() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/rm2", 0o755).unwrap();
    fs.mkdir("/rm2/notempty", 0o755).unwrap();
    fs.tick();
    let _ = fs.open("/rm2/notempty/f", O_CREAT | O_WRONLY).unwrap();

    let res = fs.rmdir("/rm2/notempty");
    assert_eq!(res, Err("ENOTEMPTY"));
}

#[test]
fn rmdir_enotdir_on_file() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/rm3", 0o755).unwrap();
    fs.tick();
    let _ = fs.open("/rm3/afile", O_CREAT | O_WRONLY).unwrap();

    let res = fs.rmdir("/rm3/afile");
    assert_eq!(res, Err("ENOTDIR"));
}

#[test]
fn rmdir_root_is_ebusy() {
    let mut fs = PosixFs::new();
    let res = fs.rmdir("/");
    assert_eq!(res, Err("EBUSY"));
}

// ============================================================================
// 11. SYMLINK TESTLERI
// ============================================================================

#[test]
fn symlink_creates_symbolic_link() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/sl", 0o755).unwrap();
    fs.tick();
    fs.symlink("/sl/target.txt", "/sl/link").unwrap();

    let target = fs.readlink("/sl/link").unwrap();
    assert_eq!(target, "/sl/target.txt");

    let st = fs.stat("/sl/link").unwrap();
    assert_eq!(st.mode & S_IFMT, S_IFLNK);
    assert_eq!(st.size, "/sl/target.txt".len() as u64);
}

#[test]
fn symlink_dangling_target_is_ok() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/sl2", 0o755).unwrap();
    fs.tick();
    // Hedef yok — POSIX'e göre symlink oluşturulabilir
    fs.symlink("/nonexistent/target", "/sl2/dangling").unwrap();
    let target = fs.readlink("/sl2/dangling").unwrap();
    assert_eq!(target, "/nonexistent/target");
}

#[test]
fn symlink_eexist_when_linkpath_exists() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/sl3", 0o755).unwrap();
    fs.tick();
    let _ = fs.open("/sl3/exists", O_CREAT | O_WRONLY).unwrap();

    let res = fs.symlink("/anywhere", "/sl3/exists");
    assert_eq!(res, Err("EEXIST"));
}

#[test]
fn readlink_on_regular_file_is_einval() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/sl4", 0o755).unwrap();
    fs.tick();
    let _ = fs.open("/sl4/regular", O_CREAT | O_WRONLY).unwrap();

    let res = fs.readlink("/sl4/regular");
    assert_eq!(res, Err("EINVAL"));
}

#[test]
fn symlink_circular_reference() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/sl5", 0o755).unwrap();
    fs.tick();
    // a -> b, b -> a (döngüsel — POSIX'e göre oluşturulabilir, çözümleme sonsuz döngü)
    fs.symlink("/sl5/b", "/sl5/a").unwrap();
    fs.symlink("/sl5/a", "/sl5/b").unwrap();

    assert_eq!(fs.readlink("/sl5/a").unwrap(), "/sl5/b");
    assert_eq!(fs.readlink("/sl5/b").unwrap(), "/sl5/a");
}

// ============================================================================
// 12. TRUNCATE TESTLERI
// ============================================================================

#[test]
fn truncate_to_zero() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/tr", 0o755).unwrap();
    fs.tick();
    let fd = fs.open("/tr/f", O_CREAT | O_WRONLY).unwrap();
    fs.write(fd, b"some content here").unwrap();
    fs.close(fd).unwrap();

    fs.truncate("/tr/f", 0).unwrap();
    let st = fs.stat("/tr/f").unwrap();
    assert_eq!(st.size, 0);
}

#[test]
fn truncate_shrink_preserves_prefix() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/tr2", 0o755).unwrap();
    fs.tick();
    let fd = fs.open("/tr2/f", O_CREAT | O_WRONLY).unwrap();
    fs.write(fd, b"abcdefghij").unwrap();
    fs.close(fd).unwrap();

    fs.truncate("/tr2/f", 5).unwrap();
    let fd2 = fs.open("/tr2/f", O_RDONLY).unwrap();
    let mut buf = [0u8; 10];
    let n = fs.read(fd2, &mut buf).unwrap();
    assert_eq!(&buf[..n], b"abcde");
    fs.close(fd2).unwrap();
}

#[test]
fn truncate_grow_zero_fills() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/tr3", 0o755).unwrap();
    fs.tick();
    let fd = fs.open("/tr3/f", O_CREAT | O_WRONLY).unwrap();
    fs.write(fd, b"ab").unwrap();
    fs.close(fd).unwrap();

    fs.truncate("/tr3/f", 8).unwrap();
    let fd2 = fs.open("/tr3/f", O_RDONLY).unwrap();
    let mut buf = [0u8; 8];
    let n = fs.read(fd2, &mut buf).unwrap();
    assert_eq!(n, 8);
    assert_eq!(&buf[..2], b"ab");
    assert_eq!(&buf[2..], &[0u8; 6]);
    fs.close(fd2).unwrap();
}

#[test]
fn truncate_directory_is_eisdir() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/tr4", 0o755).unwrap();
    let res = fs.truncate("/tr4", 0);
    assert_eq!(res, Err("EISDIR"));
}

#[test]
fn truncate_enoent_missing_path() {
    let mut fs = PosixFs::new();
    let res = fs.truncate("/no_such_file", 10);
    assert_eq!(res, Err("ENOENT"));
}

#[test]
fn truncate_updates_mtime_and_ctime() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/tr5", 0o755).unwrap();
    fs.tick();
    let fd = fs.open("/tr5/f", O_CREAT | O_WRONLY).unwrap();
    fs.write(fd, b"data").unwrap();
    fs.close(fd).unwrap();
    let st_before = fs.stat("/tr5/f").unwrap();

    fs.tick();
    fs.truncate("/tr5/f", 2).unwrap();
    let st_after = fs.stat("/tr5/f").unwrap();
    assert!(st_after.mtime > st_before.mtime);
    assert!(st_after.ctime > st_before.ctime);
}

// ============================================================================
// 13. UNLINK TESTLERI
// ============================================================================

#[test]
fn unlink_removes_file() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/un", 0o755).unwrap();
    fs.tick();
    let _ = fs.open("/un/f", O_CREAT | O_WRONLY).unwrap();

    fs.unlink("/un/f").unwrap();
    assert!(fs.lookup("/un/f").is_none());
}

#[test]
fn unlink_directory_is_eisdir() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/un2", 0o755).unwrap();
    fs.mkdir("/un2/subdir", 0o755).unwrap();

    let res = fs.unlink("/un2/subdir");
    assert_eq!(res, Err("EISDIR"));
}

#[test]
fn unlink_hardlinked_file_keeps_data() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/un3", 0o755).unwrap();
    fs.tick();
    let fd = fs.open("/un3/orig", O_CREAT | O_WRONLY).unwrap();
    fs.write(fd, b"important").unwrap();
    fs.close(fd).unwrap();

    fs.link("/un3/orig", "/un3/backup").unwrap();
    assert_eq!(fs.nlink("/un3/orig").unwrap(), 2);

    fs.unlink("/un3/orig").unwrap();
    assert!(fs.lookup("/un3/orig").is_none());
    assert!(fs.lookup("/un3/backup").is_some());

    // Veri hala erişilebilir
    let fd2 = fs.open("/un3/backup", O_RDONLY).unwrap();
    let mut buf = [0u8; 20];
    let n = fs.read(fd2, &mut buf).unwrap();
    assert_eq!(&buf[..n], b"important");
    fs.close(fd2).unwrap();

    // nlink 1'e düştü
    assert_eq!(fs.nlink("/un3/backup").unwrap(), 1);
}

#[test]
fn unlink_enoent_missing() {
    let mut fs = PosixFs::new();
    let res = fs.unlink("/nothing");
    assert_eq!(res, Err("ENOENT"));
}

#[test]
fn unlink_symlink_removes_link_not_target() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/un4", 0o755).unwrap();
    fs.tick();
    let fd = fs.open("/un4/target", O_CREAT | O_WRONLY).unwrap();
    fs.write(fd, b"data").unwrap();
    fs.close(fd).unwrap();
    fs.symlink("/un4/target", "/un4/link").unwrap();

    fs.unlink("/un4/link").unwrap();
    assert!(fs.lookup("/un4/link").is_none());
    // Hedef dosya hala mevcut
    assert!(fs.lookup("/un4/target").is_some());
}

// ============================================================================
// 14. UTIMENSAT TESTLERI
// ============================================================================

#[test]
fn utimensat_updates_atime_and_mtime() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/ut", 0o755).unwrap();
    fs.tick();
    let _ = fs.open("/ut/f", O_CREAT | O_WRONLY).unwrap();

    fs.utimensat("/ut/f", 1000, 2000).unwrap();
    let st = fs.stat("/ut/f").unwrap();
    assert_eq!(st.atime, 1000);
    assert_eq!(st.mtime, 2000);
}

#[test]
fn utimensat_enoent_missing_path() {
    let mut fs = PosixFs::new();
    let res = fs.utimensat("/no_such", 100, 200);
    assert_eq!(res, Err("ENOENT"));
}

#[test]
fn utimensat_updates_ctime() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/ut2", 0o755).unwrap();
    fs.tick();
    let _ = fs.open("/ut2/f", O_CREAT | O_WRONLY).unwrap();
    let ct_before = fs.stat("/ut2/f").unwrap().ctime;

    fs.tick();
    fs.utimensat("/ut2/f", 5000, 6000).unwrap();
    let ct_after = fs.stat("/ut2/f").unwrap().ctime;
    assert!(ct_after > ct_before);
}

// ============================================================================
// PJDFSTEST KARMA / SENARYO TESTLERI
// ============================================================================

#[test]
fn pjdfstest_scenario_open_write_chmod_chown_stat() {
    // Tam POSIX dosya yaşam döngüsü: oluştur -> yaz -> chmod -> chown -> stat
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/scenario1", 0o755).unwrap();
    fs.tick();

    let fd = fs.open("/scenario1/data.log", O_CREAT | O_WRONLY).unwrap();
    fs.write(fd, b"log entry 1\nlog entry 2\n").unwrap();
    fs.close(fd).unwrap();

    fs.chmod("/scenario1/data.log", 0o640).unwrap();
    fs.chown("/scenario1/data.log", 1000, 1000).unwrap();

    let st = fs.stat("/scenario1/data.log").unwrap();
    assert_eq!(st.mode & 0o777, 0o640);
    assert_eq!(st.uid, 1000);
    assert_eq!(st.gid, 1000);
    // "log entry 1\n" = 12 bytes, "log entry 2\n" = 12 bytes → 24 bytes
    assert_eq!(st.size, 24);
}

#[test]
fn pjdfstest_scenario_rename_then_truncate_and_unlink() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/s2", 0o755).unwrap();
    fs.tick();

    let fd = fs.open("/s2/tmp", O_CREAT | O_WRONLY).unwrap();
    fs.write(fd, b"temporary data").unwrap();
    fs.close(fd).unwrap();

    fs.rename("/s2/tmp", "/s2/final").unwrap();
    fs.truncate("/s2/final", 5).unwrap();
    let st = fs.stat("/s2/final").unwrap();
    assert_eq!(st.size, 5);

    fs.unlink("/s2/final").unwrap();
    assert!(fs.lookup("/s2/final").is_none());
}

#[test]
fn pjdfstest_scenario_hardlink_rename_unlink() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/s3", 0o755).unwrap();
    fs.tick();

    let fd = fs.open("/s3/orig", O_CREAT | O_WRONLY).unwrap();
    fs.write(fd, b"shared inode data").unwrap();
    fs.close(fd).unwrap();

    fs.link("/s3/orig", "/s3/hard1").unwrap();
    fs.link("/s3/orig", "/s3/hard2").unwrap();
    assert_eq!(fs.nlink("/s3/orig").unwrap(), 3);

    // hard1'i farklı isimle yeniden adlandır
    fs.rename("/s3/hard1", "/s3/hard1_renamed").unwrap();
    assert!(fs.lookup("/s3/hard1").is_none());
    assert!(fs.lookup("/s3/hard1_renamed").is_some());

    fs.unlink("/s3/orig").unwrap();
    assert_eq!(fs.nlink("/s3/hard1_renamed").unwrap(), 2);

    fs.unlink("/s3/hard2").unwrap();
    assert_eq!(fs.nlink("/s3/hard1_renamed").unwrap(), 1);

    fs.unlink("/s3/hard1_renamed").unwrap();
    assert!(fs.lookup("/s3/hard1_renamed").is_none());
}

#[test]
fn pjdfstest_scenario_symlink_then_unlink_target() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/s4", 0o755).unwrap();
    fs.tick();

    let fd = fs.open("/s4/real", O_CREAT | O_WRONLY).unwrap();
    fs.write(fd, b"real file").unwrap();
    fs.close(fd).unwrap();

    fs.symlink("/s4/real", "/s4/alias").unwrap();

    // Hedef silinse bile symlink hala mevcut (dangling)
    fs.unlink("/s4/real").unwrap();
    assert!(fs.lookup("/s4/real").is_none());
    let target = fs.readlink("/s4/alias").unwrap();
    assert_eq!(target, "/s4/real");
}

#[test]
fn pjdfstest_scenario_mknod_fifo_chmod_stat() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/dev4", 0o755).unwrap();
    fs.tick();

    // FIFO oluştur
    fs.mkfifo("/dev4/mypipe", 0o600).unwrap();
    fs.chmod("/dev4/mypipe", 0o666).unwrap();

    let st = fs.stat("/dev4/mypipe").unwrap();
    assert_eq!(st.mode & S_IFMT, S_IFIFO);
    assert_eq!(st.mode & 0o777, 0o666);

    // Char device
    fs.mknod_chr("/dev4/tty0", 0o660, 4, 0).unwrap();
    let st2 = fs.stat("/dev4/tty0").unwrap();
    assert_eq!(st2.mode & S_IFMT, S_IFCHR);

    // Block device
    fs.mknod_blk("/dev4/nvme0", 0o660, 259, 0).unwrap();
    let st3 = fs.stat("/dev4/nvme0").unwrap();
    assert_eq!(st3.mode & S_IFMT, S_IFBLK);
}

#[test]
fn pjdfstest_scenario_utimensat_then_stat_check() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/s5", 0o755).unwrap();
    fs.tick();

    let fd = fs.open("/s5/doc", O_CREAT | O_WRONLY).unwrap();
    fs.write(fd, b"document").unwrap();
    fs.close(fd).unwrap();

    // Zamanı belirli değere ayarla (küçük değerler ki write sonrası karşılaştırma yapılabilsin)
    fs.utimensat("/s5/doc", 100, 200).unwrap();
    let st = fs.stat("/s5/doc").unwrap();
    assert_eq!(st.atime, 100);
    assert_eq!(st.mtime, 200);

    // Yazma mtime'ı günceller (fs.now > 200 olana kadar tick)
    for _ in 0..250 { fs.tick(); }
    let fd2 = fs.open("/s5/doc", O_WRONLY).unwrap();
    fs.write(fd2, b"updated").unwrap();
    fs.close(fd2).unwrap();

    let st2 = fs.stat("/s5/doc").unwrap();
    assert!(st2.mtime > 200, "write mtime ({}) should be > 200", st2.mtime);
}

#[test]
fn pjdfstest_scenario_rmdir_after_unlink_all_children() {
    let mut fs = PosixFs::new();
    fs.tick();
    fs.mkdir("/s6", 0o755).unwrap();
    fs.mkdir("/s6/work", 0o755).unwrap();
    fs.tick();

    let _ = fs.open("/s6/work/a.tmp", O_CREAT | O_WRONLY).unwrap();
    let _ = fs.open("/s6/work/b.tmp", O_CREAT | O_WRONLY).unwrap();

    // Dizin dolu — rmdir başarısız
    assert_eq!(fs.rmdir("/s6/work"), Err("ENOTEMPTY"));

    // Çocukları sil
    fs.unlink("/s6/work/a.tmp").unwrap();
    fs.unlink("/s6/work/b.tmp").unwrap();

    // Artık boş — rmdir başarılı
    fs.rmdir("/s6/work").unwrap();
    assert!(fs.lookup("/s6/work").is_none());
}
