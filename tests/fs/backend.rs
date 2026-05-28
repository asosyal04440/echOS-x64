//! # Wave 5.9.3 — Backend Corpus
//!
//! Host-side simulation of filesystem backend operations: mount/unmount,
//! root inode access, file reads, directory listing, and feature gate rejection.

#![cfg(not(target_os = "none"))]

use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum FsBackend {
    F2fs,
    Ext4,
    Fat32,
    ExFat,
    Ntfs,
    Btrfs,
    Erofs,
    Squashfs,
    Tmpfs,
}

impl FsBackend {
    fn as_str(&self) -> &'static str {
        match self {
            Self::F2fs => "f2fs",
            Self::Ext4 => "ext4",
            Self::Fat32 => "fat32",
            Self::ExFat => "exfat",
            Self::Ntfs => "ntfs",
            Self::Btrfs => "btrfs",
            Self::Erofs => "erofs",
            Self::Squashfs => "squashfs",
            Self::Tmpfs => "tmpfs",
        }
    }
}

#[derive(Debug, Clone)]
struct BackendMount {
    fs_type: FsBackend,
    source: String,
    mount_point: String,
    features: HashSet<String>,
    known_features: HashSet<String>,
    root_entries: BTreeMap<String, DirEntry>,
    files: HashMap<String, Vec<u8>>,
}

#[derive(Debug, Clone)]
struct DirEntry {
    name: String,
    is_dir: bool,
}

struct BackendManager {
    mounts: HashMap<String, BackendMount>,
}

impl BackendManager {
    fn new() -> Self {
        Self {
            mounts: HashMap::new(),
        }
    }

    fn mount(
        &mut self,
        mount_point: &str,
        fs_type: FsBackend,
        source: &str,
        features: HashSet<String>,
        known_features: HashSet<String>,
        root_entries: BTreeMap<String, DirEntry>,
        files: HashMap<String, Vec<u8>>,
    ) -> Result<(), &'static str> {
        if self.mounts.contains_key(mount_point) {
            return Err("EBUSY");
        }

        for feat in &features {
            if !known_features.contains(feat) {
                return Err("EINVAL: unknown incompat feature");
            }
        }

        self.mounts.insert(
            mount_point.to_string(),
            BackendMount {
                fs_type,
                source: source.to_string(),
                mount_point: mount_point.to_string(),
                features,
                known_features,
                root_entries,
                files,
            },
        );
        Ok(())
    }

    fn umount(&mut self, mount_point: &str) -> Result<(), &'static str> {
        if self.mounts.remove(mount_point).is_some() {
            Ok(())
        } else {
            Err("EINVAL")
        }
    }

    fn get_mount(&self, mount_point: &str) -> Option<&BackendMount> {
        self.mounts.get(mount_point)
    }

    fn read_file(&self, mount_point: &str, path: &str) -> Result<Vec<u8>, &'static str> {
        let m = self.mounts.get(mount_point).ok_or("ENOENT")?;
        m.files.get(path).cloned().ok_or("ENOENT")
    }

    fn list_dir(&self, mount_point: &str) -> Result<Vec<DirEntry>, &'static str> {
        let m = self.mounts.get(mount_point).ok_or("ENOENT")?;
        Ok(m.root_entries.values().cloned().collect())
    }
}

fn make_known_features(fs: &FsBackend) -> HashSet<String> {
    let mut set = HashSet::new();
    match fs {
        FsBackend::F2fs => {
            set.insert("encrypt".to_string());
            set.insert("compression".to_string());
        }
        FsBackend::Ext4 => {
            set.insert("has_journal".to_string());
            set.insert("extents".to_string());
            set.insert("flex_bg".to_string());
        }
        FsBackend::Fat32 => {
            set.insert("lfn".to_string());
        }
        FsBackend::ExFat => {
            set.insert("exfat".to_string());
        }
        FsBackend::Ntfs => {
            set.insert("compression".to_string());
            set.insert("encryption".to_string());
        }
        FsBackend::Btrfs => {
            set.insert("mixed_backref".to_string());
            set.insert("extref".to_string());
            set.insert("skinny_metadata".to_string());
        }
        FsBackend::Erofs => {
            set.insert("lz4".to_string());
            set.insert("lzma".to_string());
        }
        FsBackend::Squashfs => {
            set.insert("xz".to_string());
            set.insert("zstd".to_string());
        }
        FsBackend::Tmpfs => {
            set.insert("huge".to_string());
        }
    }
    set
}

fn sample_root_entries(fs: &FsBackend) -> BTreeMap<String, DirEntry> {
    let mut entries = BTreeMap::new();
    entries.insert(
        "readme.txt".to_string(),
        DirEntry {
            name: "readme.txt".to_string(),
            is_dir: false,
        },
    );
    entries.insert(
        "data".to_string(),
        DirEntry {
            name: "data".to_string(),
            is_dir: true,
        },
    );
    match fs {
        FsBackend::Ext4 => {
            entries.insert(
                "lost+found".to_string(),
                DirEntry {
                    name: "lost+found".to_string(),
                    is_dir: true,
                },
            );
        }
        FsBackend::Ntfs => {
            entries.insert(
                "$MFT".to_string(),
                DirEntry {
                    name: "$MFT".to_string(),
                    is_dir: false,
                },
            );
        }
        _ => {}
    }
    entries
}

fn sample_files() -> HashMap<String, Vec<u8>> {
    let mut files = HashMap::new();
    files.insert("readme.txt".to_string(), b"hello from backend".to_vec());
    files
}

#[test]
fn mount_unmount() {
    let mut mgr = BackendManager::new();
    let fs = FsBackend::Ext4;
    let known = make_known_features(&fs);
    let features = HashSet::new();
    let entries = sample_root_entries(&fs);
    let files = sample_files();

    assert!(mgr
        .mount("/mnt/ext4", fs.clone(), "/dev/sda1", features, known, entries, files)
        .is_ok());
    assert!(mgr.get_mount("/mnt/ext4").is_some());
    assert!(mgr.umount("/mnt/ext4").is_ok());
    assert!(mgr.get_mount("/mnt/ext4").is_none());
}

#[test]
fn root_inode() {
    let mut mgr = BackendManager::new();
    let fs = FsBackend::F2fs;
    let known = make_known_features(&fs);
    let entries = sample_root_entries(&fs);
    let files = sample_files();

    mgr.mount(
        "/mnt/f2fs",
        fs,
        "/dev/sdb1",
        HashSet::new(),
        known,
        entries,
        files,
    )
    .unwrap();

    let dir = mgr.list_dir("/mnt/f2fs").unwrap();
    assert!(!dir.is_empty());
    assert!(dir.iter().any(|e| e.name == "readme.txt"));
    assert!(dir.iter().any(|e| e.name == "data"));
}

#[test]
fn read_file() {
    let mut mgr = BackendManager::new();
    let fs = FsBackend::Btrfs;
    let known = make_known_features(&fs);
    let entries = sample_root_entries(&fs);
    let files = sample_files();

    mgr.mount(
        "/mnt/btrfs",
        fs,
        "/dev/sdc1",
        HashSet::new(),
        known,
        entries,
        files,
    )
    .unwrap();

    let content = mgr.read_file("/mnt/btrfs", "readme.txt").unwrap();
    assert_eq!(content, b"hello from backend");
}

#[test]
fn list_dir() {
    let mut mgr = BackendManager::new();
    let fs = FsBackend::Ntfs;
    let known = make_known_features(&fs);
    let entries = sample_root_entries(&fs);
    let files = sample_files();

    mgr.mount(
        "/mnt/ntfs",
        fs,
        "/dev/sdd1",
        HashSet::new(),
        known,
        entries,
        files,
    )
    .unwrap();

    let dir = mgr.list_dir("/mnt/ntfs").unwrap();
    assert!(dir.iter().any(|e| e.name == "$MFT"));
    assert!(dir.iter().any(|e| e.name == "readme.txt"));
    assert!(dir.iter().any(|e| e.name == "data" && e.is_dir));
}

#[test]
fn feature_gate() {
    let mut mgr = BackendManager::new();
    let fs = FsBackend::Ext4;
    let known = make_known_features(&fs);
    let entries = sample_root_entries(&fs);
    let files = sample_files();

    let mut unknown_features = HashSet::new();
    unknown_features.insert("unknown_future_feature".to_string());

    let result = mgr.mount(
        "/mnt/ext4_bad",
        fs,
        "/dev/sde1",
        unknown_features,
        known,
        entries,
        files,
    );
    assert!(result.is_err());
}

#[test]
fn all_backends_mount_cycle() {
    let backends = vec![
        FsBackend::F2fs,
        FsBackend::Ext4,
        FsBackend::Fat32,
        FsBackend::ExFat,
        FsBackend::Ntfs,
        FsBackend::Btrfs,
        FsBackend::Erofs,
        FsBackend::Squashfs,
        FsBackend::Tmpfs,
    ];

    for (i, fs) in backends.iter().enumerate() {
        let mut mgr = BackendManager::new();
        let known = make_known_features(fs);
        let entries = sample_root_entries(fs);
        let files = sample_files();
        let mp = format!("/mnt/{}", fs.as_str());

        assert!(
            mgr.mount(&mp, fs.clone(), "/dev/test", HashSet::new(), known, entries, files)
                .is_ok(),
            "mount failed for {}",
            fs.as_str()
        );
        assert!(mgr.umount(&mp).is_ok(), "umount failed for {}", fs.as_str());
    }
}
