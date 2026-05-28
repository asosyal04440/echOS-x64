//! # Wave 5.9.2 — VFS Corpus
//!
//! Host-side simulation of VFS lifecycle operations: open/read/write/close,
//! rename, truncate, stat, mkdir/rmdir, link/unlink, symlink/readlink.

#![cfg(not(target_os = "none"))]

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_INODE: AtomicU64 = AtomicU64::new(2);

fn alloc_inode() -> u64 {
    NEXT_INODE.fetch_add(1, Ordering::SeqCst)
}

#[derive(Debug, Clone)]
enum InodeType {
    File { data: Vec<u8>, nlink: u32 },
    Dir { entries: BTreeMap<String, u64> },
    Symlink { target: String },
}

impl InodeType {
    fn entries_mut(&mut self) -> &mut BTreeMap<String, u64> {
        match self {
            InodeType::Dir { entries } => entries,
            _ => panic!("not a directory"),
        }
    }
}

#[derive(Debug, Clone)]
struct Inode {
    ino: u64,
    itype: InodeType,
    mode: u32,
    uid: u32,
    gid: u32,
    size: u64,
    atime: u64,
    mtime: u64,
    ctime: u64,
}

struct SimVfs {
    inodes: HashMap<u64, Inode>,
    open_files: HashMap<usize, OpenFile>,
    next_fd: usize,
    time: u64,
}

#[derive(Debug, Clone)]
struct OpenFile {
    ino: u64,
    offset: usize,
    flags: u32,
}

impl SimVfs {
    fn new() -> Self {
        let root_ino = 1;
        let mut inodes = HashMap::new();
        inodes.insert(
            root_ino,
            Inode {
                ino: root_ino,
                itype: InodeType::Dir {
                    entries: BTreeMap::new(),
                },
                mode: 0o040755,
                uid: 0,
                gid: 0,
                size: 0,
                atime: 0,
                mtime: 0,
                ctime: 0,
            },
        );
        Self {
            inodes,
            open_files: HashMap::new(),
            next_fd: 3,
            time: 0,
        }
    }

    fn tick(&mut self) {
        self.time += 1;
    }

    fn lookup_path(&self, path: &str) -> Option<u64> {
        if path == "/" || path.is_empty() {
            return Some(1);
        }
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty() && *s != ".").collect();
        let mut current = 1u64;
        for part in parts {
            let inode = self.inodes.get(&current)?;
            if let InodeType::Dir { entries } = &inode.itype {
                current = *entries.get(part)?;
            } else {
                return None;
            }
        }
        Some(current)
    }

    fn open(&mut self, path: &str, flags: u32) -> Result<usize, &'static str> {
        let ino = self.lookup_path(path).ok_or("ENOENT")?;
        let fd = self.next_fd;
        self.next_fd += 1;
        self.open_files.insert(
            fd,
            OpenFile {
                ino,
                offset: 0,
                flags,
            },
        );
        if let Some(inode) = self.inodes.get_mut(&ino) {
            inode.atime = self.time;
        }
        Ok(fd)
    }

    fn read(&mut self, fd: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
        let of = self.open_files.get(&fd).ok_or("EBADF")?;
        let ino = of.ino;
        let offset = of.offset;
        let inode = self.inodes.get(&ino).ok_or("ENOENT")?;
        match &inode.itype {
            InodeType::File { data, .. } => {
                let available = data.len().saturating_sub(offset);
                let to_read = buf.len().min(available);
                buf[..to_read].copy_from_slice(&data[offset..offset + to_read]);
                let of = self.open_files.get_mut(&fd).unwrap();
                of.offset += to_read;
                Ok(to_read)
            }
            _ => Err("EISDIR"),
        }
    }

    fn write(&mut self, fd: usize, data: &[u8]) -> Result<usize, &'static str> {
        let of = self.open_files.get(&fd).ok_or("EBADF")?;
        let ino = of.ino;
        let offset = of.offset;
        let inode = self.inodes.get_mut(&ino).ok_or("ENOENT")?;
        match &mut inode.itype {
            InodeType::File { data: file_data, .. } => {
                let end = offset + data.len();
                if end > file_data.len() {
                    file_data.resize(end, 0);
                }
                file_data[offset..end].copy_from_slice(data);
                inode.size = file_data.len() as u64;
                inode.mtime = self.time;
                let of = self.open_files.get_mut(&fd).unwrap();
                of.offset = end;
                Ok(data.len())
            }
            _ => Err("EISDIR"),
        }
    }

    fn close(&mut self, fd: usize) -> Result<(), &'static str> {
        self.open_files.remove(&fd).ok_or("EBADF")?;
        Ok(())
    }

    fn rename(&mut self, old: &str, new: &str) -> Result<(), &'static str> {
        let old_ino = self.lookup_path(old).ok_or("ENOENT")?;
        let old_parts: Vec<&str> = old.split('/').filter(|s| !s.is_empty()).collect();
        let new_parts: Vec<&str> = new.split('/').filter(|s| !s.is_empty()).collect();

        let old_parent_path = if old_parts.len() > 1 {
            format!("/{}", old_parts[..old_parts.len() - 1].join("/"))
        } else {
            "/".to_string()
        };
        let old_name = old_parts.last().ok_or("EINVAL")?;

        let new_parent_path = if new_parts.len() > 1 {
            format!("/{}", new_parts[..new_parts.len() - 1].join("/"))
        } else {
            "/".to_string()
        };
        let new_name = new_parts.last().ok_or("EINVAL")?;

        let parent_ino = self.lookup_path(&new_parent_path).ok_or("ENOENT")?;
        let inode = self.inodes.get_mut(&parent_ino).ok_or("ENOENT")?;
        if let InodeType::Dir { entries } = &mut inode.itype {
            entries.remove(*new_name);
            entries.insert(new_name.to_string(), old_ino);
        }

        let old_parent_ino = self.lookup_path(&old_parent_path).ok_or("ENOENT")?;
        let old_parent = self.inodes.get_mut(&old_parent_ino).ok_or("ENOENT")?;
        if let InodeType::Dir { entries } = &mut old_parent.itype {
            if old_parent_path != new_parent_path || old_name != new_name {
                entries.remove(*old_name);
            }
        }

        self.inodes
            .get_mut(&parent_ino)
            .unwrap()
            .mtime = self.time;
        Ok(())
    }

    fn truncate(&mut self, path: &str, new_size: u64) -> Result<(), &'static str> {
        let ino = self.lookup_path(path).ok_or("ENOENT")?;
        let inode = self.inodes.get_mut(&ino).ok_or("ENOENT")?;
        match &mut inode.itype {
            InodeType::File { data, .. } => {
                data.resize(new_size as usize, 0);
                inode.size = new_size;
                inode.mtime = self.time;
                Ok(())
            }
            _ => Err("EINVAL"),
        }
    }

    fn stat(&self, path: &str) -> Result<Inode, &'static str> {
        let ino = self.lookup_path(path).ok_or("ENOENT")?;
        self.inodes.get(&ino).cloned().ok_or("ENOENT")
    }

    fn mkdir(&mut self, path: &str) -> Result<(), &'static str> {
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let parent_path = if parts.len() > 1 {
            format!("/{}", parts[..parts.len() - 1].join("/"))
        } else {
            "/".to_string()
        };
        let name = parts.last().ok_or("EINVAL")?;

        let parent_ino = self.lookup_path(&parent_path).ok_or("ENOENT")?;
        let new_ino = alloc_inode();

        let parent = self.inodes.get_mut(&parent_ino).ok_or("ENOENT")?;
        if let InodeType::Dir { entries } = &mut parent.itype {
            if entries.contains_key(*name) {
                return Err("EEXIST");
            }
            entries.insert(name.to_string(), new_ino);
        }

        self.inodes.insert(
            new_ino,
            Inode {
                ino: new_ino,
                itype: InodeType::Dir {
                    entries: BTreeMap::new(),
                },
                mode: 0o040755,
                uid: 0,
                gid: 0,
                size: 0,
                atime: self.time,
                mtime: self.time,
                ctime: self.time,
            },
        );
        Ok(())
    }

    fn rmdir(&mut self, path: &str) -> Result<(), &'static str> {
        let ino = self.lookup_path(path).ok_or("ENOENT")?;
        let inode = self.inodes.get(&ino).ok_or("ENOENT")?;
        match &inode.itype {
            InodeType::Dir { entries } => {
                if !entries.is_empty() {
                    return Err("ENOTEMPTY");
                }
            }
            _ => return Err("ENOTDIR"),
        }

        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let parent_path = if parts.len() > 1 {
            format!("/{}", parts[..parts.len() - 1].join("/"))
        } else {
            "/".to_string()
        };
        let name = parts.last().ok_or("EINVAL")?;

        let parent_ino = self.lookup_path(&parent_path).ok_or("ENOENT")?;
        let parent = self.inodes.get_mut(&parent_ino).ok_or("ENOENT")?;
        if let InodeType::Dir { entries } = &mut parent.itype {
            entries.remove(*name);
        }
        self.inodes.remove(&ino);
        Ok(())
    }

    fn link(&mut self, old: &str, new: &str) -> Result<(), &'static str> {
        let old_ino = self.lookup_path(old).ok_or("ENOENT")?;
        let parts: Vec<&str> = new.split('/').filter(|s| !s.is_empty()).collect();
        let parent_path = if parts.len() > 1 {
            format!("/{}", parts[..parts.len() - 1].join("/"))
        } else {
            "/".to_string()
        };
        let name = parts.last().ok_or("EINVAL")?;

        let parent_ino = self.lookup_path(&parent_path).ok_or("ENOENT")?;
        let parent = self.inodes.get_mut(&parent_ino).ok_or("ENOENT")?;
        if let InodeType::Dir { entries } = &mut parent.itype {
            if entries.contains_key(*name) {
                return Err("EEXIST");
            }
            entries.insert(name.to_string(), old_ino);
        }

        let inode = self.inodes.get_mut(&old_ino).ok_or("ENOENT")?;
        match &mut inode.itype {
            InodeType::File { nlink, .. } => {
                *nlink += 1;
            }
            _ => return Err("EPERM"),
        }
        inode.ctime = self.time;
        Ok(())
    }

    fn unlink(&mut self, path: &str) -> Result<(), &'static str> {
        let ino = self.lookup_path(path).ok_or("ENOENT")?;
        let inode = self.inodes.get(&ino).ok_or("ENOENT")?;
        let should_remove = match &inode.itype {
            InodeType::File { nlink, .. } => {
                if *nlink <= 1 {
                    true
                } else {
                    let inode = self.inodes.get_mut(&ino).unwrap();
                    if let InodeType::File { nlink: nl, .. } = &mut inode.itype {
                        *nl -= 1;
                    }
                    false
                }
            }
            _ => return Err("EPERM"),
        };

        if should_remove {
            self.inodes.remove(&ino);
        }

        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let parent_path = if parts.len() > 1 {
            format!("/{}", parts[..parts.len() - 1].join("/"))
        } else {
            "/".to_string()
        };
        let name = parts.last().ok_or("EINVAL")?;

        let parent_ino = self.lookup_path(&parent_path).ok_or("ENOENT")?;
        let parent = self.inodes.get_mut(&parent_ino).ok_or("ENOENT")?;
        if let InodeType::Dir { entries } = &mut parent.itype {
            entries.remove(*name);
        }
        Ok(())
    }

    fn symlink(&mut self, target: &str, linkpath: &str) -> Result<(), &'static str> {
        let parts: Vec<&str> = linkpath.split('/').filter(|s| !s.is_empty()).collect();
        let parent_path = if parts.len() > 1 {
            format!("/{}", parts[..parts.len() - 1].join("/"))
        } else {
            "/".to_string()
        };
        let name = parts.last().ok_or("EINVAL")?;

        let parent_ino = self.lookup_path(&parent_path).ok_or("ENOENT")?;
        let new_ino = alloc_inode();

        let parent = self.inodes.get_mut(&parent_ino).ok_or("ENOENT")?;
        if let InodeType::Dir { entries } = &mut parent.itype {
            if entries.contains_key(*name) {
                return Err("EEXIST");
            }
            entries.insert(name.to_string(), new_ino);
        }

        self.inodes.insert(
            new_ino,
            Inode {
                ino: new_ino,
                itype: InodeType::Symlink {
                    target: target.to_string(),
                },
                mode: 0o120755,
                uid: 0,
                gid: 0,
                size: target.len() as u64,
                atime: self.time,
                mtime: self.time,
                ctime: self.time,
            },
        );
        Ok(())
    }

    fn readlink(&self, path: &str) -> Result<String, &'static str> {
        let ino = self.lookup_path(path).ok_or("ENOENT")?;
        let inode = self.inodes.get(&ino).ok_or("ENOENT")?;
        match &inode.itype {
            InodeType::Symlink { target } => Ok(target.clone()),
            _ => Err("EINVAL"),
        }
    }
}

#[test]
fn open_read_write_close() {
    let mut vfs = SimVfs::new();
    vfs.tick();

    vfs.mkdir("/test").unwrap();
    vfs.tick();

    let parent_ino = vfs.lookup_path("/test").unwrap();
    let file_ino = alloc_inode();
    let parent = vfs.inodes.get_mut(&parent_ino).unwrap();
    if let std::collections::btree_map::Entry::Vacant(e) =
        parent.itype.entries_mut().entry("data.txt".to_string())
    {
        e.insert(file_ino);
    }
    vfs.inodes.insert(
        file_ino,
        Inode {
            ino: file_ino,
            itype: InodeType::File {
                data: Vec::new(),
                nlink: 1,
            },
            mode: 0o100644,
            uid: 0,
            gid: 0,
            size: 0,
            atime: vfs.time,
            mtime: vfs.time,
            ctime: vfs.time,
        },
    );

    let fd = vfs.open("/test/data.txt", 2).unwrap();
    let written = vfs.write(fd, b"hello world").unwrap();
    assert_eq!(written, 11);

    vfs.close(fd).unwrap();

    let fd2 = vfs.open("/test/data.txt", 0).unwrap();
    let mut buf = [0u8; 20];
    let n = vfs.read(fd2, &mut buf).unwrap();
    assert_eq!(&buf[..n], b"hello world");
    vfs.close(fd2).unwrap();
}

#[test]
fn rename() {
    let mut vfs = SimVfs::new();
    vfs.tick();
    vfs.mkdir("/src").unwrap();
    vfs.mkdir("/dst").unwrap();
    vfs.tick();

    let src_ino = vfs.lookup_path("/src").unwrap();
    let file_ino = alloc_inode();
    let src = vfs.inodes.get_mut(&src_ino).unwrap();
    if let InodeType::Dir { entries } = &mut src.itype {
        entries.insert("old.txt".to_string(), file_ino);
    }
    vfs.inodes.insert(
        file_ino,
        Inode {
            ino: file_ino,
            itype: InodeType::File {
                data: b"content".to_vec(),
                nlink: 1,
            },
            mode: 0o100644,
            uid: 0,
            gid: 0,
            size: 7,
            atime: vfs.time,
            mtime: vfs.time,
            ctime: vfs.time,
        },
    );

    vfs.rename("/src/old.txt", "/dst/new.txt").unwrap();

    assert!(vfs.lookup_path("/src/old.txt").is_none());
    assert!(vfs.lookup_path("/dst/new.txt").is_some());
}

#[test]
fn rename_overwrite() {
    let mut vfs = SimVfs::new();
    vfs.tick();
    vfs.mkdir("/dir").unwrap();
    vfs.tick();

    let dir_ino = vfs.lookup_path("/dir").unwrap();
    let a_ino = alloc_inode();
    let b_ino = alloc_inode();
    let dir = vfs.inodes.get_mut(&dir_ino).unwrap();
    if let InodeType::Dir { entries } = &mut dir.itype {
        entries.insert("a.txt".to_string(), a_ino);
        entries.insert("b.txt".to_string(), b_ino);
    }
    vfs.inodes.insert(
        a_ino,
        Inode {
            ino: a_ino,
            itype: InodeType::File {
                data: b"aaa".to_vec(),
                nlink: 1,
            },
            mode: 0o100644,
            uid: 0,
            gid: 0,
            size: 3,
            atime: vfs.time,
            mtime: vfs.time,
            ctime: vfs.time,
        },
    );
    vfs.inodes.insert(
        b_ino,
        Inode {
            ino: b_ino,
            itype: InodeType::File {
                data: b"bbb".to_vec(),
                nlink: 1,
            },
            mode: 0o100644,
            uid: 0,
            gid: 0,
            size: 3,
            atime: vfs.time,
            mtime: vfs.time,
            ctime: vfs.time,
        },
    );

    vfs.rename("/dir/a.txt", "/dir/b.txt").unwrap();

    let stat = vfs.stat("/dir/b.txt").unwrap();
    assert_eq!(stat.size, 3);
    assert!(vfs.lookup_path("/dir/a.txt").is_none());
}

#[test]
fn truncate_shrink() {
    let mut vfs = SimVfs::new();
    vfs.tick();
    vfs.mkdir("/t").unwrap();
    vfs.tick();

    let t_ino = vfs.lookup_path("/t").unwrap();
    let f_ino = alloc_inode();
    let t = vfs.inodes.get_mut(&t_ino).unwrap();
    if let InodeType::Dir { entries } = &mut t.itype {
        entries.insert("big.txt".to_string(), f_ino);
    }
    vfs.inodes.insert(
        f_ino,
        Inode {
            ino: f_ino,
            itype: InodeType::File {
                data: b"0123456789".to_vec(),
                nlink: 1,
            },
            mode: 0o100644,
            uid: 0,
            gid: 0,
            size: 10,
            atime: vfs.time,
            mtime: vfs.time,
            ctime: vfs.time,
        },
    );

    vfs.truncate("/t/big.txt", 5).unwrap();
    let stat = vfs.stat("/t/big.txt").unwrap();
    assert_eq!(stat.size, 5);

    let fd = vfs.open("/t/big.txt", 0).unwrap();
    let mut buf = [0u8; 10];
    let n = vfs.read(fd, &mut buf).unwrap();
    assert_eq!(&buf[..n], b"01234");
    vfs.close(fd).unwrap();
}

#[test]
fn truncate_grow() {
    let mut vfs = SimVfs::new();
    vfs.tick();
    vfs.mkdir("/t").unwrap();
    vfs.tick();

    let t_ino = vfs.lookup_path("/t").unwrap();
    let f_ino = alloc_inode();
    let t = vfs.inodes.get_mut(&t_ino).unwrap();
    if let InodeType::Dir { entries } = &mut t.itype {
        entries.insert("small.txt".to_string(), f_ino);
    }
    vfs.inodes.insert(
        f_ino,
        Inode {
            ino: f_ino,
            itype: InodeType::File {
                data: b"hi".to_vec(),
                nlink: 1,
            },
            mode: 0o100644,
            uid: 0,
            gid: 0,
            size: 2,
            atime: vfs.time,
            mtime: vfs.time,
            ctime: vfs.time,
        },
    );

    vfs.truncate("/t/small.txt", 10).unwrap();
    let stat = vfs.stat("/t/small.txt").unwrap();
    assert_eq!(stat.size, 10);

    let fd = vfs.open("/t/small.txt", 0).unwrap();
    let mut buf = [0u8; 10];
    let n = vfs.read(fd, &mut buf).unwrap();
    assert_eq!(&buf[..2], b"hi");
    assert_eq!(&buf[2..], &[0u8; 8]);
    vfs.close(fd).unwrap();
}

#[test]
fn stat() {
    let mut vfs = SimVfs::new();
    vfs.tick();
    vfs.mkdir("/s").unwrap();
    vfs.tick();

    let s_ino = vfs.lookup_path("/s").unwrap();
    let f_ino = alloc_inode();
    let s = vfs.inodes.get_mut(&s_ino).unwrap();
    if let InodeType::Dir { entries } = &mut s.itype {
        entries.insert("stat.txt".to_string(), f_ino);
    }
    let content = b"stat content";
    vfs.inodes.insert(
        f_ino,
        Inode {
            ino: f_ino,
            itype: InodeType::File {
                data: content.to_vec(),
                nlink: 1,
            },
            mode: 0o100644,
            uid: 1000,
            gid: 1000,
            size: content.len() as u64,
            atime: vfs.time,
            mtime: vfs.time,
            ctime: vfs.time,
        },
    );

    let meta = vfs.stat("/s/stat.txt").unwrap();
    assert_eq!(meta.size, content.len() as u64);
    assert_eq!(meta.mode, 0o100644);
    assert_eq!(meta.uid, 1000);
    assert_eq!(meta.gid, 1000);
    assert_eq!(meta.atime, vfs.time);
}

#[test]
fn mkdir_rmdir() {
    let mut vfs = SimVfs::new();
    vfs.tick();

    vfs.mkdir("/mydir").unwrap();
    assert!(vfs.lookup_path("/mydir").is_some());

    let meta = vfs.stat("/mydir").unwrap();
    assert_eq!(meta.mode, 0o040755);

    vfs.tick();
    vfs.rmdir("/mydir").unwrap();
    assert!(vfs.lookup_path("/mydir").is_none());
}

#[test]
fn link_unlink() {
    let mut vfs = SimVfs::new();
    vfs.tick();
    vfs.mkdir("/l").unwrap();
    vfs.tick();

    let l_ino = vfs.lookup_path("/l").unwrap();
    let f_ino = alloc_inode();
    let l = vfs.inodes.get_mut(&l_ino).unwrap();
    if let InodeType::Dir { entries } = &mut l.itype {
        entries.insert("orig.txt".to_string(), f_ino);
    }
    vfs.inodes.insert(
        f_ino,
        Inode {
            ino: f_ino,
            itype: InodeType::File {
                data: b"linked".to_vec(),
                nlink: 1,
            },
            mode: 0o100644,
            uid: 0,
            gid: 0,
            size: 6,
            atime: vfs.time,
            mtime: vfs.time,
            ctime: vfs.time,
        },
    );

    vfs.link("/l/orig.txt", "/l/hard.txt").unwrap();
    let stat = vfs.stat("/l/orig.txt").unwrap();
    if let InodeType::File { nlink, .. } = &vfs.inodes[&f_ino].itype {
        assert_eq!(*nlink, 2);
    }

    vfs.unlink("/l/orig.txt").unwrap();
    assert!(vfs.lookup_path("/l/orig.txt").is_none());
    assert!(vfs.lookup_path("/l/hard.txt").is_some());

    if let InodeType::File { nlink, .. } = &vfs.inodes[&f_ino].itype {
        assert_eq!(*nlink, 1);
    }
}

#[test]
fn symlink_readlink() {
    let mut vfs = SimVfs::new();
    vfs.tick();
    vfs.mkdir("/sl").unwrap();
    vfs.tick();

    vfs.symlink("/sl/target.txt", "/sl/link.txt").unwrap();

    let target = vfs.readlink("/sl/link.txt").unwrap();
    assert_eq!(target, "/sl/target.txt");

    let stat = vfs.stat("/sl/link.txt").unwrap();
    assert_eq!(stat.mode, 0o120755);
    assert_eq!(stat.size, "/sl/target.txt".len() as u64);
}
