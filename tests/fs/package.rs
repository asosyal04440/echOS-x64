//! # Wave 5.9.7 — Package Corpus
//!
//! Host-side simulation of package management: seed handling, manifest validation,
//! TUF verification, rollback on failure, and atomic commit semantics.

#![cfg(not(target_os = "none"))]

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
enum PackageError {
    NoSeed,
    InvalidManifest,
    VerificationFailed,
    CommitFailed,
    RollbackFailed,
}

#[derive(Debug, Clone, PartialEq)]
struct Manifest {
    package_name: String,
    version: String,
    files: HashMap<String, Vec<u8>>,
    checksums: HashMap<String, String>,
    signatures: Vec<String>,
}

impl Manifest {
    fn validate(&self) -> Result<(), PackageError> {
        if self.package_name.is_empty() {
            return Err(PackageError::InvalidManifest);
        }
        if self.version.is_empty() {
            return Err(PackageError::InvalidManifest);
        }
        if self.files.is_empty() {
            return Err(PackageError::InvalidManifest);
        }
        for (path, data) in &self.files {
            let expected = self.checksums.get(path);
            if let Some(expected_cs) = expected {
                let actual = format!("{:x}", md5_hash(data));
                if &actual != expected_cs {
                    return Err(PackageError::InvalidManifest);
                }
            }
        }
        Ok(())
    }
}

fn md5_hash(data: &[u8]) -> u64 {
    let mut hash: u64 = 0;
    for (i, &b) in data.iter().enumerate() {
        hash = hash.wrapping_add((b as u64).wrapping_mul((i as u64).wrapping_add(1)));
    }
    hash
}

struct TrustRoot {
    public_key: String,
}

struct TufVerifier {
    roots: HashMap<String, TrustRoot>,
}

impl TufVerifier {
    fn new() -> Self {
        Self {
            roots: HashMap::new(),
        }
    }

    fn add_root(&mut self, name: &str, public_key: &str) {
        self.roots.insert(
            name.to_string(),
            TrustRoot {
                public_key: public_key.to_string(),
            },
        );
    }

    fn verify(&self, manifest: &Manifest) -> Result<(), PackageError> {
        if manifest.signatures.is_empty() {
            return Err(PackageError::VerificationFailed);
        }
        for sig in &manifest.signatures {
            if !self.roots.values().any(|r| r.public_key == *sig) {
                return Err(PackageError::VerificationFailed);
            }
        }
        Ok(())
    }
}

struct PackageManager {
    installed: HashMap<String, Manifest>,
    staging: Option<Manifest>,
    previous_state: Option<HashMap<String, Manifest>>,
    verifier: TufVerifier,
    has_seed: bool,
}

impl PackageManager {
    fn new() -> Self {
        Self {
            installed: HashMap::new(),
            staging: None,
            previous_state: None,
            verifier: TufVerifier::new(),
            has_seed: false,
        }
    }

    fn set_seed(&mut self) {
        self.has_seed = true;
    }

    fn add_trust_root(&mut self, name: &str, key: &str) {
        self.verifier.add_root(name, key);
    }

    fn stage_package(&mut self, manifest: Manifest) -> Result<(), PackageError> {
        if !self.has_seed {
            return Err(PackageError::NoSeed);
        }

        manifest.validate()?;
        self.verifier.verify(&manifest)?;

        self.staging = Some(manifest);
        Ok(())
    }

    fn commit(&mut self) -> Result<(), PackageError> {
        let manifest = self.staging.take().ok_or(PackageError::CommitFailed)?;

        self.previous_state = Some(self.installed.clone());

        let pkg_name = manifest.package_name.clone();
        self.installed.insert(pkg_name, manifest);

        Ok(())
    }

    fn rollback(&mut self) -> Result<(), PackageError> {
        let prev = self.previous_state.take().ok_or(PackageError::RollbackFailed)?;
        self.installed = prev;
        self.staging = None;
        Ok(())
    }

    fn is_installed(&self, name: &str) -> bool {
        self.installed.contains_key(name)
    }

    fn get_installed(&self, name: &str) -> Option<&Manifest> {
        self.installed.get(name)
    }
}

fn make_valid_manifest() -> Manifest {
    let mut files = HashMap::new();
    let data = b"package content";
    let cs = format!("{:x}", md5_hash(data));
    files.insert("bin/app".to_string(), data.to_vec());

    let mut checksums = HashMap::new();
    checksums.insert("bin/app".to_string(), cs);

    Manifest {
        package_name: "test-pkg".to_string(),
        version: "1.0.0".to_string(),
        files,
        checksums,
        signatures: vec!["root-key-1".to_string()],
    }
}

fn make_unsigned_manifest() -> Manifest {
    let mut files = HashMap::new();
    files.insert("bin/app".to_string(), b"content".to_vec());

    Manifest {
        package_name: "unsigned-pkg".to_string(),
        version: "1.0.0".to_string(),
        files,
        checksums: HashMap::new(),
        signatures: vec![],
    }
}

fn make_invalid_manifest() -> Manifest {
    Manifest {
        package_name: "".to_string(),
        version: "".to_string(),
        files: HashMap::new(),
        checksums: HashMap::new(),
        signatures: vec![],
    }
}

#[test]
fn no_seed() {
    let mut pm = PackageManager::new();
    let manifest = make_valid_manifest();

    let result = pm.stage_package(manifest);
    assert_eq!(result, Err(PackageError::NoSeed));
}

#[test]
fn invalid_manifest() {
    let mut pm = PackageManager::new();
    pm.set_seed();

    let manifest = make_invalid_manifest();
    let result = pm.stage_package(manifest);
    assert_eq!(result, Err(PackageError::InvalidManifest));
}

#[test]
fn unsigned_package() {
    let mut pm = PackageManager::new();
    pm.set_seed();
    pm.add_trust_root("root1", "root-key-1");

    let manifest = make_unsigned_manifest();
    let result = pm.stage_package(manifest);
    assert_eq!(result, Err(PackageError::VerificationFailed));
}

#[test]
fn rollback_on_failure() {
    let mut pm = PackageManager::new();
    pm.set_seed();
    pm.add_trust_root("root1", "root-key-1");

    let initial = make_valid_manifest();
    pm.stage_package(initial.clone()).unwrap();
    pm.commit().unwrap();
    assert!(pm.is_installed("test-pkg"));

    let prev_state = pm.installed.clone();

    let bad_manifest = make_unsigned_manifest();
    let stage_result = pm.stage_package(bad_manifest);
    assert!(stage_result.is_err());

    assert_eq!(pm.installed, prev_state);
}

#[test]
fn atomic_commit() {
    let mut pm = PackageManager::new();
    pm.set_seed();
    pm.add_trust_root("root1", "root-key-1");

    let manifest = make_valid_manifest();
    pm.stage_package(manifest.clone()).unwrap();

    assert!(!pm.is_installed("test-pkg"));
    assert!(pm.staging.is_some());

    pm.commit().unwrap();

    assert!(pm.is_installed("test-pkg"));
    assert!(pm.staging.is_none());

    let installed = pm.get_installed("test-pkg").unwrap();
    assert_eq!(installed.package_name, "test-pkg");
    assert_eq!(installed.version, "1.0.0");
}

#[test]
fn ghost_state_impossible() {
    let mut pm = PackageManager::new();
    pm.set_seed();
    pm.add_trust_root("root1", "root-key-1");

    let manifest = make_valid_manifest();
    pm.stage_package(manifest).unwrap();

    assert!(pm.staging.is_some());
    assert!(!pm.is_installed("test-pkg"));

    pm.commit().unwrap();

    assert!(pm.staging.is_none());
    assert!(pm.is_installed("test-pkg"));

    let prev = pm.previous_state.clone();
    pm.rollback().unwrap();

    assert!(pm.staging.is_none());
    assert!(!pm.is_installed("test-pkg"));
    assert_eq!(pm.installed, prev.unwrap_or_default());
}
