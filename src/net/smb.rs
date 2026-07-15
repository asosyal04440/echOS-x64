//! SMB/CIFS share abstraction for echOS.

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use super::NetError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmbDialect {
    Cifs,
    Smb2,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SmbSession {
    pub session_id: u64,
    pub user: String,
    pub dialect: SmbDialect,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SmbTreeConnect {
    pub tree_id: u32,
    pub session_id: u64,
    pub share: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SmbFileHandle {
    pub file_id: u64,
    pub tree_id: u32,
    pub path: String,
    pub offset: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct MemoryShare {
    files: BTreeMap<String, Vec<u8>>,
    directories: BTreeSet<String>,
}

static SHARES: Mutex<BTreeMap<String, MemoryShare>> = Mutex::new(BTreeMap::new());
static SESSIONS: Mutex<BTreeMap<u64, SmbSession>> = Mutex::new(BTreeMap::new());
static TREES: Mutex<BTreeMap<u32, SmbTreeConnect>> = Mutex::new(BTreeMap::new());
static HANDLES: Mutex<BTreeMap<u64, SmbFileHandle>> = Mutex::new(BTreeMap::new());

static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_TREE_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_FILE_ID: AtomicU64 = AtomicU64::new(1);

pub fn register_memory_share(name: &str) {
    SHARES.lock().entry(String::from(name)).or_insert_with(|| {
        let mut share = MemoryShare::default();
        share.directories.insert(String::from("/"));
        share
    });
}

fn normalize_path(path: &str) -> String {
    if path.is_empty() || path == "/" {
        return String::from("/");
    }
    let mut out = String::from("/");
    out.push_str(path.trim_matches('/'));
    out
}

fn parent_dir(path: &str) -> &str {
    if path == "/" {
        return "/";
    }
    match path.rfind('/') {
        Some(0) | None => "/",
        Some(idx) => &path[..idx],
    }
}

pub fn negotiate(prefer_smb2: bool) -> SmbDialect {
    if prefer_smb2 {
        SmbDialect::Smb2
    } else {
        SmbDialect::Cifs
    }
}

pub fn session_setup(user: &str, prefer_smb2: bool) -> SmbSession {
    let session = SmbSession {
        session_id: NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed),
        user: String::from(user),
        dialect: negotiate(prefer_smb2),
    };
    SESSIONS.lock().insert(session.session_id, session.clone());
    session
}

pub fn tree_connect(session_id: u64, share: &str) -> Result<SmbTreeConnect, NetError> {
    if !SESSIONS.lock().contains_key(&session_id) {
        return Err(NetError::InvalidFd);
    }
    if !SHARES.lock().contains_key(share) {
        return Err(NetError::AddrNotAvailable);
    }
    let tree = SmbTreeConnect {
        tree_id: NEXT_TREE_ID.fetch_add(1, Ordering::Relaxed) as u32,
        session_id,
        share: String::from(share),
    };
    TREES.lock().insert(tree.tree_id, tree.clone());
    Ok(tree)
}

pub fn create(tree_id: u32, path: &str) -> Result<SmbFileHandle, NetError> {
    let tree = TREES.lock().get(&tree_id).cloned().ok_or(NetError::InvalidFd)?;
    let mut shares = SHARES.lock();
    let share = shares.get_mut(&tree.share).ok_or(NetError::AddrNotAvailable)?;
    let normalized = normalize_path(path);
    if !share.directories.contains(parent_dir(&normalized)) {
        return Err(NetError::AddrNotAvailable);
    }
    share.files.entry(normalized.clone()).or_insert_with(Vec::new);
    let handle = SmbFileHandle {
        file_id: NEXT_FILE_ID.fetch_add(1, Ordering::Relaxed),
        tree_id,
        path: normalized,
        offset: 0,
    };
    HANDLES.lock().insert(handle.file_id, handle.clone());
    Ok(handle)
}

pub fn write(file_id: u64, data: &[u8]) -> Result<usize, NetError> {
    let mut handles = HANDLES.lock();
    let handle = handles.get_mut(&file_id).ok_or(NetError::InvalidFd)?;
    let tree = TREES.lock().get(&handle.tree_id).cloned().ok_or(NetError::InvalidFd)?;
    let mut shares = SHARES.lock();
    let share = shares.get_mut(&tree.share).ok_or(NetError::AddrNotAvailable)?;
    let entry = share.files.entry(handle.path.clone()).or_insert_with(Vec::new);
    if handle.offset > entry.len() {
        entry.resize(handle.offset, 0);
    }
    if handle.offset + data.len() > entry.len() {
        entry.resize(handle.offset + data.len(), 0);
    }
    entry[handle.offset..handle.offset + data.len()].copy_from_slice(data);
    handle.offset += data.len();
    Ok(data.len())
}

pub fn read(file_id: u64, len: usize) -> Result<Vec<u8>, NetError> {
    let mut handles = HANDLES.lock();
    let handle = handles.get_mut(&file_id).ok_or(NetError::InvalidFd)?;
    let tree = TREES.lock().get(&handle.tree_id).cloned().ok_or(NetError::InvalidFd)?;
    let shares = SHARES.lock();
    let share = shares.get(&tree.share).ok_or(NetError::AddrNotAvailable)?;
    let entry = share.files.get(&handle.path).ok_or(NetError::AddrNotAvailable)?;
    let end = core::cmp::min(handle.offset + len, entry.len());
    let out = entry[handle.offset..end].to_vec();
    handle.offset = end;
    Ok(out)
}

pub fn seek(file_id: u64, offset: usize) -> Result<(), NetError> {
    let mut handles = HANDLES.lock();
    let handle = handles.get_mut(&file_id).ok_or(NetError::InvalidFd)?;
    handle.offset = offset;
    Ok(())
}

pub fn list_dir(tree_id: u32, prefix: &str) -> Result<Vec<String>, NetError> {
    let tree = TREES.lock().get(&tree_id).cloned().ok_or(NetError::InvalidFd)?;
    let shares = SHARES.lock();
    let share = shares.get(&tree.share).ok_or(NetError::AddrNotAvailable)?;
    let normalized = normalize_path(prefix);
    if !share.directories.contains(&normalized) {
        return Err(NetError::AddrNotAvailable);
    }
    let base = if normalized == "/" {
        String::from("/")
    } else {
        alloc::format!("{normalized}/")
    };
    let mut entries = BTreeSet::new();
    for dir in &share.directories {
        if dir != &normalized && dir.starts_with(&base) {
            let rest = &dir[base.len()..];
            if !rest.is_empty() && !rest.contains('/') {
                entries.insert(dir.clone());
            }
        }
    }
    for path in share.files.keys() {
        if path.starts_with(&base) {
            let rest = &path[base.len()..];
            if !rest.is_empty() && !rest.contains('/') {
                entries.insert(path.clone());
            }
        }
    }
    let mut entries: Vec<String> = entries.into_iter().collect();
    entries.sort();
    Ok(entries)
}

pub fn unlink(tree_id: u32, path: &str) -> Result<(), NetError> {
    let tree = TREES.lock().get(&tree_id).cloned().ok_or(NetError::InvalidFd)?;
    let mut shares = SHARES.lock();
    let share = shares.get_mut(&tree.share).ok_or(NetError::AddrNotAvailable)?;
    let normalized = normalize_path(path);
    share.files.remove(&normalized).ok_or(NetError::AddrNotAvailable)?;
    HANDLES
        .lock()
        .retain(|_, handle| !(handle.tree_id == tree_id && handle.path == normalized));
    Ok(())
}

pub fn mkdir(tree_id: u32, path: &str) -> Result<(), NetError> {
    let tree = TREES.lock().get(&tree_id).cloned().ok_or(NetError::InvalidFd)?;
    let mut shares = SHARES.lock();
    let share = shares.get_mut(&tree.share).ok_or(NetError::AddrNotAvailable)?;
    let normalized = normalize_path(path);
    if !share.directories.contains(parent_dir(&normalized)) {
        return Err(NetError::AddrNotAvailable);
    }
    share.directories.insert(normalized);
    Ok(())
}

pub fn rmdir(tree_id: u32, path: &str) -> Result<(), NetError> {
    let tree = TREES.lock().get(&tree_id).cloned().ok_or(NetError::InvalidFd)?;
    let mut shares = SHARES.lock();
    let share = shares.get_mut(&tree.share).ok_or(NetError::AddrNotAvailable)?;
    let normalized = normalize_path(path);
    if normalized == "/" {
        return Err(NetError::InvalidParam);
    }
    let prefix = alloc::format!("{normalized}/");
    if share.files.keys().any(|file| file.starts_with(&prefix))
        || share
            .directories
            .iter()
            .any(|dir| dir != &normalized && dir.starts_with(&prefix))
    {
        return Err(NetError::InvalidParam);
    }
    if !share.directories.remove(&normalized) {
        return Err(NetError::AddrNotAvailable);
    }
    Ok(())
}

pub fn rename(tree_id: u32, old_path: &str, new_path: &str) -> Result<(), NetError> {
    let tree = TREES.lock().get(&tree_id).cloned().ok_or(NetError::InvalidFd)?;
    let mut shares = SHARES.lock();
    let share = shares.get_mut(&tree.share).ok_or(NetError::AddrNotAvailable)?;
    let old_path = normalize_path(old_path);
    let new_path = normalize_path(new_path);
    if !share.directories.contains(parent_dir(&new_path)) {
        return Err(NetError::AddrNotAvailable);
    }
    if let Some(data) = share.files.remove(&old_path) {
        share.files.insert(new_path.clone(), data);
        for handle in HANDLES.lock().values_mut() {
            if handle.tree_id == tree_id && handle.path == old_path {
                handle.path = new_path.clone();
            }
        }
        return Ok(());
    }
    if share.directories.remove(&old_path) {
        share.directories.insert(new_path.clone());
        let old_prefix = alloc::format!("{old_path}/");
        let new_prefix = alloc::format!("{new_path}/");
        let moved_dirs: Vec<String> = share
            .directories
            .iter()
            .filter(|dir| dir.starts_with(&old_prefix))
            .cloned()
            .collect();
        for dir in moved_dirs {
            share.directories.remove(&dir);
            share.directories.insert(dir.replacen(&old_prefix, &new_prefix, 1));
        }
        let moved_files: Vec<(String, Vec<u8>)> = share
            .files
            .iter()
            .filter(|(path, _)| path.starts_with(&old_prefix))
            .map(|(path, data)| (path.clone(), data.clone()))
            .collect();
        for (path, _) in &moved_files {
            share.files.remove(path);
        }
        for (path, data) in moved_files {
            share.files.insert(path.replacen(&old_prefix, &new_prefix, 1), data);
        }
        for handle in HANDLES.lock().values_mut() {
            if handle.tree_id == tree_id && handle.path.starts_with(&old_prefix) {
                handle.path = handle.path.replacen(&old_prefix, &new_prefix, 1);
            }
        }
        return Ok(());
    }
    Err(NetError::AddrNotAvailable)
}

pub fn tree_disconnect(tree_id: u32) {
    TREES.lock().remove(&tree_id);
    HANDLES.lock().retain(|_, handle| handle.tree_id != tree_id);
}

pub fn logoff(session_id: u64) {
    SESSIONS.lock().remove(&session_id);
    let tree_ids: Vec<u32> = TREES
        .lock()
        .iter()
        .filter(|(_, tree)| tree.session_id == session_id)
        .map(|(id, _)| *id)
        .collect();
    for tree_id in tree_ids {
        tree_disconnect(tree_id);
    }
}

pub fn close(file_id: u64) {
    HANDLES.lock().remove(&file_id);
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn smb_memory_share_roundtrip() {
        register_memory_share("public");
        let session = session_setup("bahadir", true);
        let tree = tree_connect(session.session_id, "public").unwrap();
        mkdir(tree.tree_id, "/docs").unwrap();
        let handle = create(tree.tree_id, "/docs/hello.txt").unwrap();
        write(handle.file_id, b"hello smb").unwrap();
        seek(handle.file_id, 0).unwrap();
        assert_eq!(read(handle.file_id, 32).unwrap(), b"hello smb");
        rename(tree.tree_id, "/docs/hello.txt", "/docs/readme.txt").unwrap();
        assert_eq!(list_dir(tree.tree_id, "/").unwrap(), vec![String::from("/docs")]);
        assert_eq!(list_dir(tree.tree_id, "/docs").unwrap(), vec![String::from("/docs/readme.txt")]);
        close(handle.file_id);
        unlink(tree.tree_id, "/docs/readme.txt").unwrap();
        rmdir(tree.tree_id, "/docs").unwrap();
        tree_disconnect(tree.tree_id);
        logoff(session.session_id);
    }
}
