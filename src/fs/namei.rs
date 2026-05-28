//! # Pathname Resolution (namei)
//!
//! VFS-level path resolution with component-by-component walking and
//! symlink following.  The echOS equivalent of Linux `fs/namei.c` —
//! REF-walk minus RCU-walk (single-threaded embedded kernel).
//!
//! ## Contract
//!
//! Every VFS operation that takes a pathname SHOULD go through this module
//! so that:
//!
//! 1. Symlinks in intermediate and (optionally) final components are followed
//! 2. Mount boundaries are crossed correctly when symlink targets land on
//!    a different filesystem
//! 3. The dentry cache is populated during the walk for later fast lookups

use alloc::string::String;
use alloc::vec::Vec;

use crate::fs::vfs_unified::{VfsFileInfo, normalize_vfs_path};

/// Maximum symlink traversals (Linux MAXSYMLINKS = 40, path_resolution(7))
pub(crate) const MAXSYMLINKS: usize = 40;

// POSIX file-type constants from mode bits
const S_IFMT: u32 = 0o170000;
const S_IFLNK: u32 = 0o120000;
const S_IFDIR: u32 = 0o040000;

/// Result of a full path resolution.
#[derive(Clone, Debug)]
pub struct ResolvedPath {
    /// The resolved, normalized path after symlink following.
    pub resolved_path: String,
    /// File information for the resolved entry.
    pub info: VfsFileInfo,
}

/// Check whether a VfsFileInfo represents a symbolic link.
fn is_symlink(info: &VfsFileInfo) -> bool {
    (info.mode & S_IFMT) == S_IFLNK
}

/// Return the parent directory of a normalized path.
pub(crate) fn parent_path(path: &str) -> String {
    let normalized = normalize_vfs_path(path);
    if normalized == "/" {
        return "/".into();
    }
    let trimmed = normalized.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(0) => "/".into(),
        Some(pos) => trimmed[..pos].into(),
        None => "/".into(),
    }
}

/// Split a normalized path into its components (excluding the leading "/").
fn split_components(path: &str) -> Vec<&str> {
    path.trim_matches('/')
        .split('/')
        .filter(|c| !c.is_empty())
        .collect()
}

/// Join a parent path and a component name into a single path.
fn join_path(parent: &str, name: &str) -> String {
    if parent == "/" {
        alloc::format!("/{}", name)
    } else {
        alloc::format!("{}/{}", parent, name)
    }
}

/// Internal recursive resolver.
///
/// Walks `path` component-by-component.  For each component:
/// 1. Call `lookup_component(parent_ino, current_path, name)`
/// 2. If the result is a symlink (and it should be followed):
///    a. Read symlink target via `readlink`
///    b. Resolve the target (absolute → absolute, relative → relative to parent)
///    c. Prepend remaining path components after the target
///    d. Recurse with depth+1
/// 3. Cache the result in the dentry cache
/// 4. Continue to next component
fn resolve_inner(
    lookup_component: &mut dyn FnMut(u64, &str, &str) -> Result<VfsFileInfo, &'static str>,
    readlink: &mut dyn FnMut(&str) -> Result<Vec<u8>, &'static str>,
    path: &str,
    follow_final: bool,
    depth: usize,
) -> Result<ResolvedPath, &'static str> {
    let normalized = normalize_vfs_path(path);

    // Root is a trivial case
    if normalized == "/" {
        let info = lookup_component(0, "", "/")?;
        return Ok(ResolvedPath {
            resolved_path: normalized,
            info,
        });
    }

    let components = split_components(&normalized);
    let mut current_path = String::from("/");
    let mut current_ino: u64 = 0;
    let mut last_info: Option<VfsFileInfo> = None;
    let mut i = 0;

    while i < components.len() {
        let component = components[i];
        let is_last = i == components.len() - 1;
        let component_path = join_path(&current_path, component);

        let info = lookup_component(current_ino, &current_path, component)?;

        if is_symlink(&info) && (follow_final || !is_last) {
            if depth >= MAXSYMLINKS {
                return Err("too many symbolic links (ELOOP)");
            }

            let target_bytes = readlink(&component_path)?;
            let target = core::str::from_utf8(&target_bytes)
                .map_err(|_| "symlink target is not valid UTF-8")?;

            if target.is_empty() {
                return Err("no path component was found");
            }

            let remaining: Vec<&str> = components[(i + 1)..].to_vec();

            let new_path = if target.starts_with('/') {
                let mut p = normalize_vfs_path(target);
                for &rem in &remaining {
                    p = join_path(&p, rem);
                }
                normalize_vfs_path(&p)
            } else {
                let parent = parent_path(&component_path);
                let mut p = join_path(&parent, target);
                for &rem in &remaining {
                    p = join_path(&p, rem);
                }
                normalize_vfs_path(&p)
            };

            return resolve_inner(lookup_component, readlink, &new_path, follow_final, depth + 1);
        }

        cache_one(&current_path, component, current_ino, &info);

        current_path = component_path;
        current_ino = info.inode;
        last_info = Some(info);
        i += 1;
    }

    let info = last_info.ok_or("empty path resolved to no entry")?;
    Ok(ResolvedPath {
        resolved_path: current_path,
        info,
    })
}

/// Resolve a pathname with symlink following.
///
/// Takes two callbacks:
/// - `lookup_component` — per-component directory lookup
///   (parent_ino, parent_path, name) → VfsFileInfo
/// - `readlink` — read the raw bytes of a symlink file (its target string)
///
/// Returns the final resolved path and file info.  The caller stores the
/// `ResolvedPath.resolved_path` in the fd table so that subsequent
/// `read` / `write` / `seek` operate on the target, not the symlink.
pub fn resolve(
    mut lookup_component: impl FnMut(u64, &str, &str) -> Result<VfsFileInfo, &'static str>,
    mut readlink: impl FnMut(&str) -> Result<Vec<u8>, &'static str>,
    path: &str,
    follow_final: bool,
) -> Result<ResolvedPath, &'static str> {
    resolve_inner(
        &mut lookup_component,
        &mut readlink,
        path,
        follow_final,
        0,
    )
}

/// Cache a single component in the dentry cache.
fn cache_one(parent_path: &str, name: &str, parent_ino: u64, info: &VfsFileInfo) {
    if name.is_empty() || name == "/" {
        return;
    }
    use crate::fs::dcache::Dentry;
    let mut cache = crate::fs::VFS_DCACHE.lock();
    if cache.lookup(parent_ino, name).is_some() {
        return;
    }
    let dentry = Dentry {
        name: name.into(),
        parent_ino,
        ino: info.inode,
        is_dir: (info.mode & S_IFDIR) != 0,
        mode: info.mode as u16,
        uid: info.uid,
        gid: info.gid,
        size: info.size,
        generation: 0,
    };
    cache.alloc(dentry);
}
