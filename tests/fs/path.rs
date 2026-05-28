//! # Wave 5.9.1 — Path Resolution Corpus
//!
//! Host-side simulation of path resolution edge cases.
//! Validates normalization, length limits, symlink loops, and error codes.

#![cfg(not(target_os = "none"))]

use std::collections::HashMap;
use std::collections::HashSet;

const PATH_MAX: usize = 4096;
const NAME_MAX: usize = 255;
const MAXSYMLINKS: usize = 40;

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
enum PathError {
    ENOENT,
    EINVAL,
    ENAMETOOLONG,
    ELOOP,
    ENOTDIR,
}

fn normalize_path(path: &str) -> Result<String, PathError> {
    if path.is_empty() {
        return Err(PathError::ENOENT);
    }
    if path.contains('\0') {
        return Err(PathError::EINVAL);
    }
    if path.len() > PATH_MAX {
        return Err(PathError::ENAMETOOLONG);
    }

    let mut components: Vec<&str> = Vec::new();
    for raw in path.split('/') {
        if raw.is_empty() || raw == "." {
            continue;
        }
        if raw == ".." {
            if !components.is_empty() {
                components.pop();
            }
            continue;
        }
        if raw.len() > NAME_MAX {
            return Err(PathError::ENAMETOOLONG);
        }
        components.push(raw);
    }

    if components.is_empty() {
        return Ok("/".to_string());
    }

    let mut result = String::from("/");
    result.push_str(components[0]);
    for comp in components.iter().skip(1) {
        result.push('/');
        result.push_str(comp);
    }
    Ok(result)
}

fn resolve_symlinks(
    path: &str,
    symlink_table: &std::collections::HashMap<String, String>,
) -> Result<String, PathError> {
    let mut current = path.to_string();
    let mut depth = 0;

    loop {
        if depth > MAXSYMLINKS {
            return Err(PathError::ELOOP);
        }
        if let Some(target) = symlink_table.get(&current) {
            current = target.clone();
            depth += 1;
        } else {
            break;
        }
    }

    normalize_path(&current)
}

fn resolve_path_with_type(
    path: &str,
    symlink_table: &std::collections::HashMap<String, String>,
    file_set: &std::collections::HashSet<String>,
) -> Result<(String, bool), PathError> {
    let resolved = resolve_symlinks(path, symlink_table)?;

    if resolved == "/" {
        return Ok((resolved, true));
    }

    let is_file = file_set.contains(&resolved);
    let is_dir = !is_file && resolved != "/";

    if !is_file && !is_dir && resolved != "/" {
        return Err(PathError::ENOENT);
    }

    Ok((resolved, is_file))
}

#[test]
fn empty_path() {
    let symlinks = std::collections::HashMap::new();
    let files = std::collections::HashSet::new();
    let result = resolve_path_with_type("", &symlinks, &files);
    assert_eq!(result, Err(PathError::ENOENT));
}

#[test]
fn nul_byte() {
    let symlinks = std::collections::HashMap::new();
    let files = std::collections::HashSet::new();
    let result = resolve_path_with_type("/foo\0bar", &symlinks, &files);
    assert_eq!(result, Err(PathError::EINVAL));
}

#[test]
fn path_too_long() {
    let symlinks = std::collections::HashMap::new();
    let files = std::collections::HashSet::new();
    let long_path = format!("/{}", "a".repeat(PATH_MAX));
    let result = resolve_path_with_type(&long_path, &symlinks, &files);
    assert_eq!(result, Err(PathError::ENAMETOOLONG));
}

#[test]
fn name_too_long() {
    let symlinks = std::collections::HashMap::new();
    let files = std::collections::HashSet::new();
    let long_name = format!("/{}", "a".repeat(NAME_MAX + 1));
    let result = resolve_path_with_type(&long_name, &symlinks, &files);
    assert_eq!(result, Err(PathError::ENAMETOOLONG));
}

#[test]
fn symlink_loop() {
    let mut symlinks = std::collections::HashMap::new();
    let files = std::collections::HashSet::new();

    for i in 0..50 {
        symlinks.insert(format!("/link{}", i), format!("/link{}", i + 1));
    }

    let result = resolve_path_with_type("/link0", &symlinks, &files);
    assert_eq!(result, Err(PathError::ELOOP));
}

#[test]
fn trailing_slash_on_file() {
    let symlinks = std::collections::HashMap::new();
    let mut files = std::collections::HashSet::new();
    files.insert("/myfile".to_string());

    let result = resolve_path_with_type("/myfile/", &symlinks, &files);
    let (resolved, is_file) = result.unwrap();
    assert_eq!(resolved, "/myfile");
    assert!(is_file);
}

#[test]
fn double_slash() {
    let symlinks = std::collections::HashMap::new();
    let mut files = std::collections::HashSet::new();
    files.insert("/foo/bar".to_string());

    let result = resolve_path_with_type("//foo///bar", &symlinks, &files);
    assert!(result.is_ok());
    let (resolved, _) = result.unwrap();
    assert_eq!(resolved, "/foo/bar");
}

#[test]
fn dot_dot_escape() {
    let symlinks = std::collections::HashMap::new();
    let mut files = std::collections::HashSet::new();
    files.insert("/secret".to_string());

    let result = resolve_path_with_type("/a/b/c/../../../..", &symlinks, &files);
    assert!(result.is_ok());
    let (resolved, _) = result.unwrap();
    assert_eq!(resolved, "/");

    let result2 = resolve_path_with_type("/a/../../etc/passwd", &symlinks, &files);
    assert!(result2.is_ok());
    let (resolved2, _) = result2.unwrap();
    assert_eq!(resolved2, "/etc/passwd");
    assert!(!files.contains(&resolved2));
}

#[test]
fn absolute_vs_relative() {
    let symlinks = std::collections::HashMap::new();
    let mut files = std::collections::HashSet::new();
    files.insert("/a/b/c".to_string());

    let abs = resolve_path_with_type("/a/b/c", &symlinks, &files);
    let rel_from_root = resolve_path_with_type("a/b/c", &symlinks, &files);

    assert!(abs.is_ok());
    assert!(rel_from_root.is_ok());
    let (abs_resolved, _) = abs.unwrap();
    let (rel_resolved, _) = rel_from_root.unwrap();
    assert_eq!(abs_resolved, rel_resolved);
}
