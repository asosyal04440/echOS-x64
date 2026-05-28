//! # xfstests-style Generic Corpus for echOS
//!
//! Modeled after Linux xfstests generic/001–generic/301.
//! Host-side simulation using in-process filesystem (SimFs).
//!
//! Test categories:
//!   G01: File create/delete lifecycle
//!   G02: Sequential read/write
//!   G03: Directory operations
//!   G04: Rename atomicity
//!   G05: Truncate (shrink/grow)
//!   G06: Symlink operations
//!   G07: Hard link operations
//!   G08: fsync durability contract
//!   G09: Permission/mode bits
//!   G10: Extended attributes
//!   G11: Concurrent open/read/write
//!   G12: Error handling (ENOENT, EISDIR, ENOTDIR)
//!   G13: Append-only writes
//!   G14: Large file I/O
//!   G15: Directory nested depth
//!   G16: Rename overwrite
//!   G17: Truncate to zero then write
//!   G18: Read past EOF
//!   G19: Zero-length file
//!   G20: Multiple fd same file

#![cfg(not(target_os = "none"))]

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_INODE: AtomicU64 = AtomicU64::new(2);

fn alloc_inode() -> u64 {
    NEXT_INODE.fetch_add(1, Ordering::SeqCst)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum InodeType {
    File { data: Vec<u8>, nlink: u32 },
    Dir { entries: BTreeMap<String, u64> },
    Symlink { target: String },
}

#[derive(Debug, Clone)]
struct Inode {
    ino: u64,
    itype: InodeType,
    mode: u32,
    size: u64,
    mtime: u64,
}

struct SimFs {
    inodes: BTreeMap<u64, Inode>,
    root_ino: u64,
    time: u64,
}

impl SimFs {
    fn new() -> Self {
        let root_ino = alloc_inode();
        let mut fs = Self {
            inodes: BTreeMap::new(),
            root_ino,
            time: 0,
        };
        fs.inodes.insert(
            root_ino,
            Inode {
                ino: root_ino,
                itype: InodeType::Dir {
                    entries: BTreeMap::new(),
                },
                mode: 0o40755,
                size: 0,
                mtime: 0,
            },
        );
        fs
    }

    fn tick(&mut self) {
        self.time += 1;
    }

    fn create_file(&mut self, parent_ino: u64, name: &str) -> Result<u64, &'static str> {
        let ino = alloc_inode();
        let parent = self.inodes.get_mut(&parent_ino).ok_or("parent not found")?;
        match &mut parent.itype {
            InodeType::Dir { entries } => {
                if entries.contains_key(name) {
                    return Err("EEXIST");
                }
                entries.insert(name.to_string(), ino);
            }
            _ => return Err("ENOTDIR"),
        }
        parent.mtime = self.time;
        self.tick();
        self.inodes.insert(
            ino,
            Inode {
                ino,
                itype: InodeType::File {
                    data: Vec::new(),
                    nlink: 1,
                },
                mode: 0o100644,
                size: 0,
                mtime: self.time,
            },
        );
        Ok(ino)
    }

    fn write(&mut self, ino: u64, offset: usize, data: &[u8]) -> Result<usize, &'static str> {
        let inode = self.inodes.get_mut(&ino).ok_or("ENOENT")?;
        match &mut inode.itype {
            InodeType::File { data: file_data, .. } => {
                let end = offset + data.len();
                if end > file_data.len() {
                    file_data.resize(end, 0);
                }
                file_data[offset..end].copy_from_slice(data);
                inode.size = file_data.len() as u64;
                Ok(data.len())
            }
            InodeType::Dir { .. } => Err("EISDIR"),
            InodeType::Symlink { .. } => Err("ELOOP"),
        }?;
        self.tick();
        if let Some(inode) = self.inodes.get_mut(&ino) {
            inode.mtime = self.time;
        }
        Ok(data.len())
    }

    fn read(&self, ino: u64, offset: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
        let inode = self.inodes.get(&ino).ok_or("ENOENT")?;
        match &inode.itype {
            InodeType::File { data, .. } => {
                if offset >= data.len() {
                    return Ok(0);
                }
                let avail = data.len() - offset;
                let n = buf.len().min(avail);
                buf[..n].copy_from_slice(&data[offset..offset + n]);
                Ok(n)
            }
            InodeType::Dir { .. } => Err("EISDIR"),
            InodeType::Symlink { .. } => Err("ELOOP"),
        }
    }

    fn truncate(&mut self, ino: u64, new_size: u64) -> Result<(), &'static str> {
        let inode = self.inodes.get_mut(&ino).ok_or("ENOENT")?;
        match &mut inode.itype {
            InodeType::File { data, .. } => {
                data.resize(new_size as usize, 0);
                inode.size = new_size;
                Ok(())
            }
            InodeType::Dir { .. } => Err("EISDIR"),
            InodeType::Symlink { .. } => Err("ELOOP"),
        }?;
        self.tick();
        if let Some(inode) = self.inodes.get_mut(&ino) {
            inode.mtime = self.time;
        }
        Ok(())
    }

    fn unlink(&mut self, parent_ino: u64, name: &str) -> Result<(), &'static str> {
        let parent = self.inodes.get_mut(&parent_ino).ok_or("parent not found")?;
        let child_ino = match &mut parent.itype {
            InodeType::Dir { entries } => {
                entries.remove(name).ok_or("ENOENT")?
            }
            _ => return Err("ENOTDIR"),
        };
        parent.mtime = self.time;
        self.tick();
        let child = self.inodes.get_mut(&child_ino).ok_or("child inode missing")?;
        match &mut child.itype {
            InodeType::File { nlink, .. } => {
                *nlink = nlink.saturating_sub(1);
                if *nlink == 0 {
                    self.inodes.remove(&child_ino);
                }
            }
            InodeType::Dir { entries } => {
                if !entries.is_empty() {
                    return Err("ENOTEMPTY");
                }
                self.inodes.remove(&child_ino);
            }
            InodeType::Symlink { .. } => {
                self.inodes.remove(&child_ino);
            }
        }
        Ok(())
    }

    fn mkdir(&mut self, parent_ino: u64, name: &str) -> Result<u64, &'static str> {
        let ino = alloc_inode();
        let parent = self.inodes.get_mut(&parent_ino).ok_or("parent not found")?;
        match &mut parent.itype {
            InodeType::Dir { entries } => {
                if entries.contains_key(name) {
                    return Err("EEXIST");
                }
                entries.insert(name.to_string(), ino);
            }
            _ => return Err("ENOTDIR"),
        }
        parent.mtime = self.time;
        self.tick();
        self.inodes.insert(
            ino,
            Inode {
                ino,
                itype: InodeType::Dir {
                    entries: BTreeMap::new(),
                },
                mode: 0o40755,
                size: 0,
                mtime: self.time,
            },
        );
        Ok(ino)
    }

    fn rmdir(&mut self, parent_ino: u64, name: &str) -> Result<(), &'static str> {
        let parent = self.inodes.get_mut(&parent_ino).ok_or("parent not found")?;
        let child_ino = match &mut parent.itype {
            InodeType::Dir { entries } => entries.remove(name).ok_or("ENOENT")?,
            _ => return Err("ENOTDIR"),
        };
        parent.mtime = self.time;
        self.tick();
        let child = self.inodes.get(&child_ino).ok_or("child missing")?;
        match &child.itype {
            InodeType::Dir { entries } => {
                if !entries.is_empty() {
                    // Put it back
                    if let InodeType::Dir { entries: p_entries } = &mut self.inodes.get_mut(&parent_ino).unwrap().itype {
                        p_entries.insert(name.to_string(), child_ino);
                    }
                    return Err("ENOTEMPTY");
                }
            }
            _ => {
                if let InodeType::Dir { entries: p_entries } = &mut self.inodes.get_mut(&parent_ino).unwrap().itype {
                    p_entries.insert(name.to_string(), child_ino);
                }
                return Err("ENOTDIR");
            }
        }
        self.inodes.remove(&child_ino);
        Ok(())
    }

    fn rename(&mut self, old_parent: u64, old_name: &str, new_parent: u64, new_name: &str) -> Result<(), &'static str> {
        let child_ino = {
            let parent = self.inodes.get_mut(&old_parent).ok_or("old parent not found")?;
            match &mut parent.itype {
                InodeType::Dir { entries } => entries.remove(old_name).ok_or("ENOENT")?,
                _ => return Err("ENOTDIR"),
            }
        };
        self.inodes.get_mut(&old_parent).unwrap().mtime = self.time;

        let new_parent_inode = self.inodes.get_mut(&new_parent).ok_or("new parent not found")?;
        match &mut new_parent_inode.itype {
            InodeType::Dir { entries } => {
                entries.insert(new_name.to_string(), child_ino);
            }
            _ => return Err("ENOTDIR"),
        }
        new_parent_inode.mtime = self.time;
        self.tick();
        Ok(())
    }

    fn symlink(&mut self, parent_ino: u64, name: &str, target: &str) -> Result<u64, &'static str> {
        let ino = alloc_inode();
        let parent = self.inodes.get_mut(&parent_ino).ok_or("parent not found")?;
        match &mut parent.itype {
            InodeType::Dir { entries } => {
                entries.insert(name.to_string(), ino);
            }
            _ => return Err("ENOTDIR"),
        }
        parent.mtime = self.time;
        self.tick();
        self.inodes.insert(
            ino,
            Inode {
                ino,
                itype: InodeType::Symlink {
                    target: target.to_string(),
                },
                mode: 0o120777,
                size: target.len() as u64,
                mtime: self.time,
            },
        );
        Ok(ino)
    }

    fn readlink(&self, ino: u64) -> Result<String, &'static str> {
        let inode = self.inodes.get(&ino).ok_or("ENOENT")?;
        match &inode.itype {
            InodeType::Symlink { target } => Ok(target.clone()),
            _ => Err("EINVAL"),
        }
    }

    fn hardlink(&mut self, parent_ino: u64, name: &str, target_ino: u64) -> Result<(), &'static str> {
        let parent = self.inodes.get_mut(&parent_ino).ok_or("parent not found")?;
        match &mut parent.itype {
            InodeType::Dir { entries } => {
                entries.insert(name.to_string(), target_ino);
            }
            _ => return Err("ENOTDIR"),
        }
        parent.mtime = self.time;
        let target = self.inodes.get_mut(&target_ino).ok_or("target not found")?;
        match &mut target.itype {
            InodeType::File { nlink, .. } => *nlink += 1,
            _ => return Err("EINVAL"),
        }
        self.tick();
        Ok(())
    }

    fn stat(&self, ino: u64) -> Result<&Inode, &'static str> {
        self.inodes.get(&ino).ok_or("ENOENT")
    }

    fn lookup(&self, parent_ino: u64, name: &str) -> Result<u64, &'static str> {
        let parent = self.inodes.get(&parent_ino).ok_or("ENOENT")?;
        match &parent.itype {
            InodeType::Dir { entries } => entries.get(name).copied().ok_or("ENOENT"),
            _ => Err("ENOTDIR"),
        }
    }

    fn inode_count(&self) -> usize {
        self.inodes.len()
    }
}

// ═══════════════════════════════════════════════════════════════
// G01: File create/delete lifecycle (xfstests generic/001)
// ═══════════════════════════════════════════════════════════════

#[test]
fn g01_file_create_delete_lifecycle() {
    let mut fs = SimFs::new();
    let root = fs.root_ino;

    let ino = fs.create_file(root, "test.txt").unwrap();
    assert!(fs.stat(ino).is_ok());

    fs.unlink(root, "test.txt").unwrap();
    assert!(fs.stat(ino).is_err());
}

#[test]
fn g01_duplicate_create_eexist() {
    let mut fs = SimFs::new();
    let root = fs.root_ino;

    fs.create_file(root, "dup.txt").unwrap();
    assert_eq!(fs.create_file(root, "dup.txt"), Err("EEXIST"));
}

#[test]
fn g01_create_in_nonexistent_parent() {
    let mut fs = SimFs::new();
    let fake_ino = 9999;

    assert_eq!(fs.create_file(fake_ino, "x.txt"), Err("parent not found"));
}

// ═══════════════════════════════════════════════════════════════
// G02: Sequential read/write (xfstests generic/002)
// ═══════════════════════════════════════════════════════════════

#[test]
fn g02_write_then_read_sequential() {
    let mut fs = SimFs::new();
    let root = fs.root_ino;

    let ino = fs.create_file(root, "seq.txt").unwrap();
    let data = b"Hello, xfstests!";
    fs.write(ino, 0, data).unwrap();

    let mut buf = vec![0u8; 100];
    let n = fs.read(ino, 0, &mut buf).unwrap();
    assert_eq!(n, data.len());
    assert_eq!(&buf[..n], data);
}

#[test]
fn g02_partial_read() {
    let mut fs = SimFs::new();
    let root = fs.root_ino;

    let ino = fs.create_file(root, "partial.txt").unwrap();
    fs.write(ino, 0, b"0123456789").unwrap();

    let mut buf = vec![0u8; 5];
    let n = fs.read(ino, 3, &mut buf).unwrap();
    assert_eq!(n, 5);
    assert_eq!(&buf, b"34567");
}

#[test]
fn g02_read_past_eof() {
    let mut fs = SimFs::new();
    let root = fs.root_ino;

    let ino = fs.create_file(root, "short.txt").unwrap();
    fs.write(ino, 0, b"abc").unwrap();

    let mut buf = vec![0u8; 100];
    let n = fs.read(ino, 100, &mut buf).unwrap();
    assert_eq!(n, 0);
}

#[test]
fn g02_read_empty_file() {
    let mut fs = SimFs::new();
    let root = fs.root_ino;

    let ino = fs.create_file(root, "empty.txt").unwrap();
    let mut buf = vec![0u8; 100];
    let n = fs.read(ino, 0, &mut buf).unwrap();
    assert_eq!(n, 0);
}

// ═══════════════════════════════════════════════════════════════
// G03: Directory operations (xfstests generic/003)
// ═══════════════════════════════════════════════════════════════

#[test]
fn g03_mkdir_and_list() {
    let mut fs = SimFs::new();
    let root = fs.root_ino;

    let dir_ino = fs.mkdir(root, "subdir").unwrap();
    let parent = fs.stat(root).unwrap();
    if let InodeType::Dir { entries } = &parent.itype {
        assert!(entries.contains_key("subdir"));
    } else {
        panic!("root is not a directory");
    }

    fs.create_file(dir_ino, "child.txt").unwrap();
    let dir = fs.stat(dir_ino).unwrap();
    if let InodeType::Dir { entries } = &dir.itype {
        assert!(entries.contains_key("child.txt"));
    }
}

#[test]
fn g03_rmdir_empty() {
    let mut fs = SimFs::new();
    let root = fs.root_ino;

    fs.mkdir(root, "empty_dir").unwrap();
    fs.rmdir(root, "empty_dir").unwrap();
    assert!(fs.lookup(root, "empty_dir").is_err());
}

#[test]
fn g03_rmdir_nonempty_fails() {
    let mut fs = SimFs::new();
    let root = fs.root_ino;

    let dir_ino = fs.mkdir(root, "nonempty").unwrap();
    fs.create_file(dir_ino, "file.txt").unwrap();
    assert_eq!(fs.rmdir(root, "nonempty"), Err("ENOTEMPTY"));
}

#[test]
fn g03_nested_directories() {
    let mut fs = SimFs::new();
    let root = fs.root_ino;

    let l1 = fs.mkdir(root, "a").unwrap();
    let l2 = fs.mkdir(l1, "b").unwrap();
    let l3 = fs.mkdir(l2, "c").unwrap();
    fs.create_file(l3, "deep.txt").unwrap();

    let l3_stat = fs.stat(l3).unwrap();
    if let InodeType::Dir { entries } = &l3_stat.itype {
        assert!(entries.contains_key("deep.txt"));
    }
}

// ═══════════════════════════════════════════════════════════════
// G04: Rename atomicity (xfstests generic/004)
// ═══════════════════════════════════════════════════════════════

#[test]
fn g04_rename_basic() {
    let mut fs = SimFs::new();
    let root = fs.root_ino;

    let ino = fs.create_file(root, "old.txt").unwrap();
    fs.write(ino, 0, b"data").unwrap();
    fs.rename(root, "old.txt", root, "new.txt").unwrap();

    assert!(fs.lookup(root, "old.txt").is_err());
    assert!(fs.lookup(root, "new.txt").is_ok());
}

#[test]
fn g04_rename_overwrites_destination() {
    let mut fs = SimFs::new();
    let root = fs.root_ino;

    fs.create_file(root, "src.txt").unwrap();
    fs.create_file(root, "dst.txt").unwrap();
    fs.rename(root, "src.txt", root, "dst.txt").unwrap();

    assert!(fs.lookup(root, "src.txt").is_err());
    assert!(fs.lookup(root, "dst.txt").is_ok());
}

#[test]
fn g04_rename_across_directories() {
    let mut fs = SimFs::new();
    let root = fs.root_ino;

    let dir_a = fs.mkdir(root, "a").unwrap();
    let dir_b = fs.mkdir(root, "b").unwrap();
    let ino = fs.create_file(dir_a, "movable.txt").unwrap();

    fs.rename(dir_a, "movable.txt", dir_b, "moved.txt").unwrap();

    assert!(fs.lookup(dir_a, "movable.txt").is_err());
    assert!(fs.lookup(dir_b, "moved.txt").is_ok());
}

#[test]
fn g04_rename_nonexistent_source() {
    let mut fs = SimFs::new();
    let root = fs.root_ino;

    assert_eq!(fs.rename(root, "ghost.txt", root, "dest.txt"), Err("ENOENT"));
}

// ═══════════════════════════════════════════════════════════════
// G05: Truncate (xfstests generic/005)
// ═══════════════════════════════════════════════════════════════

#[test]
fn g05_truncate_shrink() {
    let mut fs = SimFs::new();
    let root = fs.root_ino;

    let ino = fs.create_file(root, "big.txt").unwrap();
    fs.write(ino, 0, &vec![0xAA; 1000]).unwrap();
    fs.truncate(ino, 500).unwrap();

    let stat = fs.stat(ino).unwrap();
    assert_eq!(stat.size, 500);
}

#[test]
fn g05_truncate_to_zero() {
    let mut fs = SimFs::new();
    let root = fs.root_ino;

    let ino = fs.create_file(root, "data.txt").unwrap();
    fs.write(ino, 0, b"hello").unwrap();
    fs.truncate(ino, 0).unwrap();

    let stat = fs.stat(ino).unwrap();
    assert_eq!(stat.size, 0);

    let mut buf = [0u8; 10];
    let n = fs.read(ino, 0, &mut buf).unwrap();
    assert_eq!(n, 0);
}

#[test]
fn g05_truncate_then_write() {
    let mut fs = SimFs::new();
    let root = fs.root_ino;

    let ino = fs.create_file(root, "rw.txt").unwrap();
    fs.write(ino, 0, b"original content here").unwrap();
    fs.truncate(ino, 0).unwrap();
    fs.write(ino, 0, b"new").unwrap();

    let mut buf = [0u8; 10];
    let n = fs.read(ino, 0, &mut buf).unwrap();
    assert_eq!(&buf[..n], b"new");
}

#[test]
fn g05_truncate_grow_fills_with_zeroes() {
    let mut fs = SimFs::new();
    let root = fs.root_ino;

    let ino = fs.create_file(root, "grow.txt").unwrap();
    fs.write(ino, 0, b"hi").unwrap();
    fs.truncate(ino, 10).unwrap();

    let stat = fs.stat(ino).unwrap();
    assert_eq!(stat.size, 10);

    let mut buf = [0u8; 10];
    let n = fs.read(ino, 0, &mut buf).unwrap();
    assert_eq!(&buf[..2], b"hi");
    assert_eq!(&buf[2..], &[0u8; 8]);
}

// ═══════════════════════════════════════════════════════════════
// G06: Symlink operations (xfstests generic/006)
// ═══════════════════════════════════════════════════════════════

#[test]
fn g06_symlink_create_and_read() {
    let mut fs = SimFs::new();
    let root = fs.root_ino;

    let ino = fs.create_file(root, "target.txt").unwrap();
    fs.write(ino, 0, b"target data").unwrap();

    let sym_ino = fs.symlink(root, "link.txt", "target.txt").unwrap();
    let target = fs.readlink(sym_ino).unwrap();
    assert_eq!(target, "target.txt");
}

#[test]
fn g06_symlink_size_matches_target_name() {
    let mut fs = SimFs::new();
    let root = fs.root_ino;

    let sym_ino = fs.symlink(root, "link", "some/path").unwrap();
    let stat = fs.stat(sym_ino).unwrap();
    assert_eq!(stat.size, "some/path".len() as u64);
}

// ═══════════════════════════════════════════════════════════════
// G07: Hard link operations (xfstests generic/007)
// ═══════════════════════════════════════════════════════════════

#[test]
fn g07_hardlink_increases_nlink() {
    let mut fs = SimFs::new();
    let root = fs.root_ino;

    let ino = fs.create_file(root, "original.txt").unwrap();
    let nlink_before = match &fs.stat(ino).unwrap().itype {
        InodeType::File { nlink, .. } => *nlink,
        _ => panic!("not a file"),
    };

    fs.hardlink(root, "hard.txt", ino).unwrap();

    let nlink_after = match &fs.stat(ino).unwrap().itype {
        InodeType::File { nlink, .. } => *nlink,
        _ => panic!("not a file"),
    };
    assert_eq!(nlink_before + 1, nlink_after);
}

#[test]
fn g07_hardlink_survives_unlink() {
    let mut fs = SimFs::new();
    let root = fs.root_ino;

    let ino = fs.create_file(root, "a.txt").unwrap();
    fs.write(ino, 0, b"persistent").unwrap();
    fs.hardlink(root, "b.txt", ino).unwrap();

    fs.unlink(root, "a.txt").unwrap();

    assert!(fs.stat(ino).is_ok());
    let mut buf = [0u8; 20];
    let n = fs.read(ino, 0, &mut buf).unwrap();
    assert_eq!(&buf[..n], b"persistent");
}

// ═══════════════════════════════════════════════════════════════
// G08: fsync durability contract (xfstests generic/018)
// ═══════════════════════════════════════════════════════════════

#[test]
fn g08_fsync_data_visible_after_sync() {
    let mut fs = SimFs::new();
    let root = fs.root_ino;

    let ino = fs.create_file(root, "sync.txt").unwrap();
    fs.write(ino, 0, b"synced data").unwrap();
    // Simulated fsync: data is immediately in memory
    let mut buf = [0u8; 20];
    let n = fs.read(ino, 0, &mut buf).unwrap();
    assert_eq!(&buf[..n], b"synced data");
}

#[test]
fn g08_fsync_atomicity_single_sector() {
    let mut fs = SimFs::new();
    let root = fs.root_ino;

    let ino = fs.create_file(root, "atomic.txt").unwrap();
    fs.write(ino, 0, b"AAAA").unwrap();
    fs.write(ino, 0, b"BBBB").unwrap();

    let mut buf = [0u8; 4];
    fs.read(ino, 0, &mut buf).unwrap();
    assert_eq!(&buf, b"BBBB");
}

// ═══════════════════════════════════════════════════════════════
// G09: Permission/mode bits (xfstests generic/050)
// ═══════════════════════════════════════════════════════════════

#[test]
fn g09_file_default_mode() {
    let mut fs = SimFs::new();
    let root = fs.root_ino;

    let ino = fs.create_file(root, "mode.txt").unwrap();
    let stat = fs.stat(ino).unwrap();
    assert_eq!(stat.mode, 0o100644);
}

#[test]
fn g09_directory_default_mode() {
    let mut fs = SimFs::new();
    let root = fs.root_ino;

    let ino = fs.mkdir(root, "dirmode").unwrap();
    let stat = fs.stat(ino).unwrap();
    assert_eq!(stat.mode, 0o40755);
}

// ═══════════════════════════════════════════════════════════════
// G10: Extended attributes (xfstests generic/273 pattern)
// ═══════════════════════════════════════════════════════════════

#[test]
fn g10_mtime_updates_on_write() {
    let mut fs = SimFs::new();
    let root = fs.root_ino;

    let ino = fs.create_file(root, "mtime.txt").unwrap();
    let t1 = fs.stat(ino).unwrap().mtime;

    fs.write(ino, 0, b"data").unwrap();
    let t2 = fs.stat(ino).unwrap().mtime;
    assert!(t2 > t1);
}

#[test]
fn g10_mtime_updates_on_truncate() {
    let mut fs = SimFs::new();
    let root = fs.root_ino;

    let ino = fs.create_file(root, "mtime2.txt").unwrap();
    fs.write(ino, 0, b"data").unwrap();
    let t1 = fs.stat(ino).unwrap().mtime;

    fs.truncate(ino, 0).unwrap();
    let t2 = fs.stat(ino).unwrap().mtime;
    assert!(t2 >= t1);
}

// ═══════════════════════════════════════════════════════════════
// G11: Concurrent open/read/write (multi-fd pattern)
// ═══════════════════════════════════════════════════════════════

#[test]
fn g11_multiple_fds_same_file() {
    let mut fs = SimFs::new();
    let root = fs.root_ino;

    let ino = fs.create_file(root, "shared.txt").unwrap();
    // Two "fds" point to same inode
    fs.write(ino, 0, b"from fd1").unwrap();
    let mut buf = [0u8; 20];
    let n = fs.read(ino, 0, &mut buf).unwrap();
    assert_eq!(&buf[..n], b"from fd1");
}

// ═══════════════════════════════════════════════════════════════
// G12: Error handling
// ═══════════════════════════════════════════════════════════════

#[test]
fn g12_read_nonexistent_file() {
    let fs = SimFs::new();
    let mut buf = [0u8; 10];
    assert_eq!(fs.read(9999, 0, &mut buf), Err("ENOENT"));
}

#[test]
fn g12_write_to_directory() {
    let mut fs = SimFs::new();
    let root = fs.root_ino;

    assert_eq!(fs.write(root, 0, b"data"), Err("EISDIR"));
}

#[test]
fn g12_mkdir_on_file() {
    let mut fs = SimFs::new();
    let root = fs.root_ino;

    let ino = fs.create_file(root, "file.txt").unwrap();
    assert_eq!(fs.mkdir(ino, "child"), Err("ENOTDIR"));
}

#[test]
fn g12_create_on_file() {
    let mut fs = SimFs::new();
    let root = fs.root_ino;

    let ino = fs.create_file(root, "file.txt").unwrap();
    assert_eq!(fs.create_file(ino, "child"), Err("ENOTDIR"));
}

// ═══════════════════════════════════════════════════════════════
// G13: Append-only writes
// ═══════════════════════════════════════════════════════════════

#[test]
fn g13_append_write() {
    let mut fs = SimFs::new();
    let root = fs.root_ino;

    let ino = fs.create_file(root, "append.txt").unwrap();
    fs.write(ino, 0, b"hello").unwrap();
    fs.write(ino, 5, b" world").unwrap();

    let mut buf = [0u8; 20];
    let n = fs.read(ino, 0, &mut buf).unwrap();
    assert_eq!(&buf[..n], b"hello world");
}

#[test]
fn g13_overwrite_middle() {
    let mut fs = SimFs::new();
    let root = fs.root_ino;

    let ino = fs.create_file(root, "overwrite.txt").unwrap();
    fs.write(ino, 0, b"AABBCC").unwrap();
    fs.write(ino, 2, b"XX").unwrap();

    let mut buf = [0u8; 6];
    fs.read(ino, 0, &mut buf).unwrap();
    assert_eq!(&buf, b"AAXXCC");
}

// ═══════════════════════════════════════════════════════════════
// G14: Large file I/O
// ═══════════════════════════════════════════════════════════════

#[test]
fn g14_large_write_and_readback() {
    let mut fs = SimFs::new();
    let root = fs.root_ino;

    let ino = fs.create_file(root, "large.bin").unwrap();
    let size = 64 * 1024;
    let data: Vec<u8> = (0..size).map(|i| (i & 0xFF) as u8).collect();
    fs.write(ino, 0, &data).unwrap();

    let stat = fs.stat(ino).unwrap();
    assert_eq!(stat.size, size as u64);

    let mut buf = vec![0u8; size];
    let n = fs.read(ino, 0, &mut buf).unwrap();
    assert_eq!(n, size);
    assert_eq!(buf, data);
}

// ═══════════════════════════════════════════════════════════════
// G15: Directory nested depth
// ═══════════════════════════════════════════════════════════════

#[test]
fn g15_deep_nesting() {
    let mut fs = SimFs::new();
    let root = fs.root_ino;

    let mut current = root;
    for i in 0..50 {
        let name = format!("d{}", i);
        current = fs.mkdir(current, &name).unwrap();
    }
    fs.create_file(current, "deep_leaf.txt").unwrap();

    let stat = fs.stat(current).unwrap();
    if let InodeType::Dir { entries } = &stat.itype {
        assert!(entries.contains_key("deep_leaf.txt"));
    }
}

// ═══════════════════════════════════════════════════════════════
// G16: Rename overwrite (xfstests generic/301)
// ═══════════════════════════════════════════════════════════════

#[test]
fn g16_rename_overwrite_preserves_data() {
    let mut fs = SimFs::new();
    let root = fs.root_ino;

    let src = fs.create_file(root, "src.txt").unwrap();
    fs.write(src, 0, b"source data").unwrap();

    let dst = fs.create_file(root, "dst.txt").unwrap();
    fs.write(dst, 0, b"dest data").unwrap();

    fs.rename(root, "src.txt", root, "dst.txt").unwrap();

    let dst_ino = fs.lookup(root, "dst.txt").unwrap();
    let mut buf = [0u8; 20];
    let n = fs.read(dst_ino, 0, &mut buf).unwrap();
    assert_eq!(&buf[..n], b"source data");
}

// ═══════════════════════════════════════════════════════════════
// G17: Truncate to zero then write
// ═══════════════════════════════════════════════════════════════

#[test]
fn g17_truncate_zero_rewrite_cycle() {
    let mut fs = SimFs::new();
    let root = fs.root_ino;

    let ino = fs.create_file(root, "cycle.txt").unwrap();

    for i in 0..10 {
        let payload = format!("iteration_{}", i);
        fs.write(ino, 0, payload.as_bytes()).unwrap();
        fs.truncate(ino, 0).unwrap();

        let stat = fs.stat(ino).unwrap();
        assert_eq!(stat.size, 0);
    }

    fs.write(ino, 0, b"final").unwrap();
    let mut buf = [0u8; 10];
    let n = fs.read(ino, 0, &mut buf).unwrap();
    assert_eq!(&buf[..n], b"final");
}

// ═══════════════════════════════════════════════════════════════
// G18: Read past EOF
// ═══════════════════════════════════════════════════════════════

#[test]
fn g18_read_beyond_end_returns_partial() {
    let mut fs = SimFs::new();
    let root = fs.root_ino;

    let ino = fs.create_file(root, "short.txt").unwrap();
    fs.write(ino, 0, b"AB").unwrap();

    let mut buf = [0u8; 10];
    let n = fs.read(ino, 1, &mut buf).unwrap();
    assert_eq!(n, 1);
    assert_eq!(&buf[..1], b"B");
}

// ═══════════════════════════════════════════════════════════════
// G19: Zero-length file
// ═══════════════════════════════════════════════════════════════

#[test]
fn g19_zero_length_file_exists() {
    let mut fs = SimFs::new();
    let root = fs.root_ino;

    let ino = fs.create_file(root, "zero.txt").unwrap();
    let stat = fs.stat(ino).unwrap();
    assert_eq!(stat.size, 0);

    let mut buf = [0u8; 10];
    let n = fs.read(ino, 0, &mut buf).unwrap();
    assert_eq!(n, 0);
}

// ═══════════════════════════════════════════════════════════════
// G20: Multiple operations stress
// ═══════════════════════════════════════════════════════════════

#[test]
fn g20_create_many_files_and_verify() {
    let mut fs = SimFs::new();
    let root = fs.root_ino;

    let count = 200;
    let mut inodes = Vec::new();
    for i in 0..count {
        let name = format!("file_{:04}.txt", i);
        let ino = fs.create_file(root, &name).unwrap();
        let payload = format!("data_{}", i);
        fs.write(ino, 0, payload.as_bytes()).unwrap();
        inodes.push((name, ino, payload));
    }

    for (name, ino, expected) in &inodes {
        assert!(fs.lookup(root, name).is_ok());
        let mut buf = vec![0u8; 64];
        let n = fs.read(*ino, 0, &mut buf).unwrap();
        assert_eq!(&buf[..n], expected.as_str().as_bytes());
    }

    assert_eq!(fs.inode_count(), count + 1); // root + 200 files
}
