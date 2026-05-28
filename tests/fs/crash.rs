//! # Wave 5.9.6 — Crash Corpus
//!
//! Host-side simulation of crash consistency states for filesystem operations.
//! Validates allowed states, atomicity, and absence of ghost/orphan artifacts.

#![cfg(not(target_os = "none"))]

use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum CrashState {
    NotStarted,
    MetadataUpdated,
    DataWritten,
    JournalLogged,
    JournalCommitted,
    Checkpointed,
    Completed,
    Inconsistent,
    Corrupt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum RecoveryAction {
    None,
    JournalReplay,
    RollForward,
    Rollback,
    Fsck,
}

struct CrashContract {
    operation: &'static str,
    allowed_states: HashSet<CrashState>,
    forbidden_states: HashSet<CrashState>,
    recovery: RecoveryAction,
}

impl CrashContract {
    fn is_allowed(&self, state: CrashState) -> bool {
        self.allowed_states.contains(&state)
    }

    fn is_forbidden(&self, state: CrashState) -> bool {
        self.forbidden_states.contains(&state)
    }
}

struct SimFs {
    files: HashMap<String, Vec<u8>>,
    metadata: HashMap<String, u64>,
    journal: Vec<JournalEntry>,
    committed: bool,
}

#[derive(Debug, Clone)]
enum JournalEntry {
    Create { path: String, size: u64 },
    Write { path: String, offset: usize, len: usize },
    Truncate { path: String, new_size: u64 },
    Rename { old: String, new: String },
    Unlink { path: String },
    Commit,
}

impl SimFs {
    fn new() -> Self {
        Self {
            files: HashMap::new(),
            metadata: HashMap::new(),
            journal: Vec::new(),
            committed: false,
        }
    }

    fn begin_journal(&mut self) {
        self.journal.clear();
        self.committed = false;
    }

    fn journal_create(&mut self, path: &str, size: u64) {
        self.journal
            .push(JournalEntry::Create {
                path: path.to_string(),
                size,
            });
    }

    fn journal_write(&mut self, path: &str, offset: usize, len: usize) {
        self.journal.push(JournalEntry::Write {
            path: path.to_string(),
            offset,
            len,
        });
    }

    fn journal_truncate(&mut self, path: &str, new_size: u64) {
        self.journal.push(JournalEntry::Truncate {
            path: path.to_string(),
            new_size,
        });
    }

    fn journal_rename(&mut self, old: &str, new: &str) {
        self.journal.push(JournalEntry::Rename {
            old: old.to_string(),
            new: new.to_string(),
        });
    }

    fn journal_unlink(&mut self, path: &str) {
        self.journal
            .push(JournalEntry::Unlink {
                path: path.to_string(),
            });
    }

    fn commit(&mut self) {
        self.journal.push(JournalEntry::Commit);
        self.committed = true;
        self.apply_journal();
    }

    fn apply_journal(&mut self) {
        for entry in &self.journal {
            match entry {
                JournalEntry::Create { path, size } => {
                    self.files.insert(path.clone(), vec![0u8; *size as usize]);
                    self.metadata.insert(path.clone(), *size);
                }
                JournalEntry::Write { path, offset, len } => {
                    if let Some(data) = self.files.get_mut(path) {
                        let end = offset + len;
                        if end > data.len() {
                            data.resize(end, 0);
                        }
                    }
                }
                JournalEntry::Truncate { path, new_size } => {
                    if let Some(data) = self.files.get_mut(path) {
                        data.resize(*new_size as usize, 0);
                    }
                    self.metadata.insert(path.clone(), *new_size);
                }
                JournalEntry::Rename { old, new } => {
                    if let Some(data) = self.files.remove(old) {
                        self.files.insert(new.clone(), data);
                    }
                    if let Some(size) = self.metadata.remove(old) {
                        self.metadata.insert(new.clone(), size);
                    }
                }
                JournalEntry::Unlink { path } => {
                    self.files.remove(path);
                    self.metadata.remove(path);
                }
                JournalEntry::Commit => {}
            }
        }
    }

    fn rollback(&mut self) {
        self.journal.clear();
        self.committed = false;
    }

    fn has_ghost_file(&self, path: &str) -> bool {
        self.files.contains_key(path) && self.metadata.contains_key(path)
    }

    fn has_orphan_inode(&self) -> bool {
        for path in self.files.keys() {
            if !self.metadata.contains_key(path) {
                return true;
            }
        }
        false
    }
}

fn create_crash_contract() -> CrashContract {
    CrashContract {
        operation: "create",
        allowed_states: vec![
            CrashState::NotStarted,
            CrashState::MetadataUpdated,
            CrashState::JournalLogged,
            CrashState::JournalCommitted,
            CrashState::Completed,
        ]
        .into_iter()
        .collect(),
        forbidden_states: vec![CrashState::Corrupt].into_iter().collect(),
        recovery: RecoveryAction::JournalReplay,
    }
}

fn write_crash_contract() -> CrashContract {
    CrashContract {
        operation: "write",
        allowed_states: vec![
            CrashState::NotStarted,
            CrashState::DataWritten,
            CrashState::JournalLogged,
            CrashState::JournalCommitted,
            CrashState::Completed,
        ]
        .into_iter()
        .collect(),
        forbidden_states: vec![CrashState::Inconsistent, CrashState::Corrupt]
            .into_iter()
            .collect(),
        recovery: RecoveryAction::JournalReplay,
    }
}

fn truncate_crash_contract() -> CrashContract {
    CrashContract {
        operation: "truncate",
        allowed_states: vec![
            CrashState::NotStarted,
            CrashState::MetadataUpdated,
            CrashState::JournalLogged,
            CrashState::JournalCommitted,
            CrashState::Completed,
        ]
        .into_iter()
        .collect(),
        forbidden_states: vec![CrashState::Inconsistent].into_iter().collect(),
        recovery: RecoveryAction::Rollback,
    }
}

fn rename_crash_contract() -> CrashContract {
    CrashContract {
        operation: "rename",
        allowed_states: vec![
            CrashState::NotStarted,
            CrashState::MetadataUpdated,
            CrashState::JournalLogged,
            CrashState::JournalCommitted,
            CrashState::Completed,
        ]
        .into_iter()
        .collect(),
        forbidden_states: vec![CrashState::Inconsistent, CrashState::Corrupt]
            .into_iter()
            .collect(),
        recovery: RecoveryAction::JournalReplay,
    }
}

fn unlink_crash_contract() -> CrashContract {
    CrashContract {
        operation: "unlink",
        allowed_states: vec![
            CrashState::NotStarted,
            CrashState::MetadataUpdated,
            CrashState::JournalLogged,
            CrashState::JournalCommitted,
            CrashState::Completed,
        ]
        .into_iter()
        .collect(),
        forbidden_states: vec![CrashState::Corrupt].into_iter().collect(),
        recovery: RecoveryAction::JournalReplay,
    }
}

#[test]
fn create_crash_states() {
    let contract = create_crash_contract();

    assert!(contract.is_allowed(CrashState::NotStarted));
    assert!(contract.is_allowed(CrashState::MetadataUpdated));
    assert!(contract.is_allowed(CrashState::JournalLogged));
    assert!(contract.is_allowed(CrashState::JournalCommitted));
    assert!(contract.is_allowed(CrashState::Completed));

    assert!(contract.is_forbidden(CrashState::Corrupt));
    assert!(!contract.is_forbidden(CrashState::Inconsistent));
}

#[test]
fn write_crash_states() {
    let contract = write_crash_contract();

    assert!(contract.is_allowed(CrashState::DataWritten));
    assert!(contract.is_allowed(CrashState::JournalCommitted));

    assert!(contract.is_forbidden(CrashState::Inconsistent));
    assert!(contract.is_forbidden(CrashState::Corrupt));

    let mut fs = SimFs::new();
    fs.begin_journal();
    fs.files.insert("/test.txt".to_string(), vec![0u8; 10]);
    fs.journal_write("/test.txt", 0, 5);

    assert!(!fs.committed);
    fs.commit();
    assert!(fs.committed);
    assert_eq!(fs.files.get("/test.txt").unwrap().len(), 10);
}

#[test]
fn truncate_crash_states() {
    let contract = truncate_crash_contract();

    assert!(contract.is_allowed(CrashState::MetadataUpdated));
    assert!(contract.is_allowed(CrashState::JournalCommitted));
    assert!(contract.is_forbidden(CrashState::Inconsistent));

    let mut fs = SimFs::new();
    fs.begin_journal();
    fs.files.insert("/big.txt".to_string(), vec![1u8; 100]);
    fs.metadata.insert("/big.txt".to_string(), 100);

    fs.journal_truncate("/big.txt", 50);
    fs.commit();

    assert_eq!(fs.files.get("/big.txt").unwrap().len(), 50);
    assert_eq!(*fs.metadata.get("/big.txt").unwrap(), 50);
}

#[test]
fn rename_crash_states() {
    let contract = rename_crash_contract();

    assert!(contract.is_allowed(CrashState::MetadataUpdated));
    assert!(contract.is_forbidden(CrashState::Inconsistent));
    assert!(contract.is_forbidden(CrashState::Corrupt));

    let mut fs = SimFs::new();
    fs.begin_journal();
    fs.files.insert("/old.txt".to_string(), b"content".to_vec());
    fs.metadata.insert("/old.txt".to_string(), 7);

    fs.journal_rename("/old.txt", "/new.txt");
    fs.commit();

    assert!(!fs.has_ghost_file("/old.txt"));
    assert!(fs.has_ghost_file("/new.txt"));
    assert_eq!(fs.files.get("/new.txt").unwrap(), b"content");
}

#[test]
fn unlink_crash_states() {
    let contract = unlink_crash_contract();

    assert!(contract.is_allowed(CrashState::MetadataUpdated));
    assert!(contract.is_allowed(CrashState::JournalCommitted));
    assert!(contract.is_forbidden(CrashState::Corrupt));

    let mut fs = SimFs::new();
    fs.begin_journal();
    fs.files
        .insert("/to_delete.txt".to_string(), b"data".to_vec());
    fs.metadata.insert("/to_delete.txt".to_string(), 4);

    fs.journal_unlink("/to_delete.txt");
    fs.commit();

    assert!(!fs.has_ghost_file("/to_delete.txt"));
    assert!(!fs.has_orphan_inode());
}

#[test]
fn no_partial_writes_on_crash() {
    let mut fs = SimFs::new();
    fs.begin_journal();
    fs.files.insert("/file.txt".to_string(), vec![0u8; 100]);

    fs.journal_write("/file.txt", 0, 50);

    fs.rollback();

    assert_eq!(fs.files.get("/file.txt").unwrap().len(), 100);
    assert!(fs.journal.is_empty());
}

#[test]
fn atomic_size_change() {
    let mut fs = SimFs::new();
    fs.begin_journal();
    fs.files.insert("/data.bin".to_string(), vec![0xFFu8; 200]);
    fs.metadata.insert("/data.bin".to_string(), 200);

    fs.journal_truncate("/data.bin", 100);

    let pre_commit_size = fs.files.get("/data.bin").unwrap().len();
    assert_eq!(pre_commit_size, 200);

    fs.commit();

    let post_commit_size = fs.files.get("/data.bin").unwrap().len();
    assert_eq!(post_commit_size, 100);
    assert_eq!(*fs.metadata.get("/data.bin").unwrap(), 100);
}
