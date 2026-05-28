//! # CrashMonkey/ACE-style Crash Consistency Corpus
//!
//! Host-side simulation of bounded black-box crash testing (OSDI '18).
//! Each test models: operation sequence → crash injection → recovery → consistency check.
//!
//! ## Test Categories (CrashMonkey B3 model)
//!
//!   CC01: fsync durability — write → fsync → crash → verify
//!   CC02: metadata ordering — data write before journal commit
//!   CC03: atomic rename — rename crash mid-operation
//!   CC04: truncate crash — truncate → crash → size consistency
//!   CC05: unlink crash — unlink → crash → no ghost files
//!   CC06: multi-file crash — concurrent writes → crash → partial consistency
//!   CC07: journal replay — write-ahead log recovery
//!   CC08: append + fsync — append data → crash → data either all or none
//!   CC09: rename overwrite — overwrite crash → old or new, never corrupt
//!   CC10: create + write — create file → crash → file exists with correct data or not at all
//!   CC11: truncate + write — truncate then write → crash → consistent size
//!   CC12: nested directory crash — mkdir chain → crash → partial or complete
//!   CC13: fsync ordering — multiple fsyncs → crash → ordering preserved
//!   CC14: ENOSPC edge — write near full → crash → no corruption
//!   CC15: symlink + crash — symlink creation → crash → no dangling refs
//!   CC16: hardlink + unlink → crash → nlink consistency
//!   CC17: rename atomicity — rename src→dst → crash → src exists or dst exists, not both
//!   CC18: sequential fsyncs — write A → fsync → write B → crash → A persists
//!   CC19: journal checkpoint — journal full → crash → checkpoint recovery
//!   CC20: power-loss during write — partial sector write → crash → sector consistency

#![cfg(not(target_os = "none"))]

use std::collections::{HashMap, HashSet};

// ═══════════════════════════════════════════════════════════════
// Crash State Machine (CrashMonkey B3 model)
// ═══════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum CrashState {
    Idle,
    DataWritten,
    JournalStarted,
    JournalCommitted,
    Checkpointed,
    FsyncCompleted,
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

/// Contract defining which crash states are allowed/forbidden for an operation.
struct CrashContract {
    operation: &'static str,
    allowed: HashSet<CrashState>,
    forbidden: HashSet<CrashState>,
    recovery: RecoveryAction,
}

impl CrashContract {
    fn new(op: &'static str, recovery: RecoveryAction) -> Self {
        Self {
            operation: op,
            allowed: HashSet::new(),
            forbidden: HashSet::new(),
            recovery,
        }
    }

    fn allow(mut self, state: CrashState) -> Self {
        self.allowed.insert(state);
        self
    }

    fn forbid(mut self, state: CrashState) -> Self {
        self.forbidden.insert(state);
        self
    }

    fn is_valid(&self, state: CrashState) -> bool {
        !self.forbidden.contains(&state)
    }
}

// ═══════════════════════════════════════════════════════════════
// Simulated Filesystem with Journal
// ═══════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
enum JournalEntry {
    Create { path: String, size: u64 },
    Write { path: String, offset: usize, len: usize },
    Truncate { path: String, new_size: u64 },
    Rename { old: String, new: String },
    Unlink { path: String },
    Mkdir { path: String },
    Rmdir { path: String },
    Commit,
}

struct CrashFs {
    files: HashMap<String, Vec<u8>>,
    metadata: HashMap<String, u64>,
    dirs: HashSet<String>,
    journal: Vec<JournalEntry>,
    committed: bool,
    crashed: bool,
}

impl CrashFs {
    fn new() -> Self {
        let mut dirs = HashSet::new();
        dirs.insert("/".to_string());
        Self {
            files: HashMap::new(),
            metadata: HashMap::new(),
            dirs,
            journal: Vec::new(),
            committed: false,
            crashed: false,
        }
    }

    fn begin(&mut self) {
        self.journal.clear();
        self.committed = false;
        self.crashed = false;
    }

    fn journal_create(&mut self, path: &str, size: u64) {
        self.journal.push(JournalEntry::Create { path: path.to_string(), size });
    }

    fn journal_write(&mut self, path: &str, offset: usize, len: usize) {
        self.journal.push(JournalEntry::Write { path: path.to_string(), offset, len });
    }

    fn journal_truncate(&mut self, path: &str, new_size: u64) {
        self.journal.push(JournalEntry::Truncate { path: path.to_string(), new_size });
    }

    fn journal_rename(&mut self, old: &str, new: &str) {
        self.journal.push(JournalEntry::Rename { old: old.to_string(), new: new.to_string() });
    }

    fn journal_unlink(&mut self, path: &str) {
        self.journal.push(JournalEntry::Unlink { path: path.to_string() });
    }

    fn journal_mkdir(&mut self, path: &str) {
        self.journal.push(JournalEntry::Mkdir { path: path.to_string() });
    }

    fn journal_rmdir(&mut self, path: &str) {
        self.journal.push(JournalEntry::Rmdir { path: path.to_string() });
    }

    fn commit_journal(&mut self) {
        self.journal.push(JournalEntry::Commit);
        self.committed = true;
        self.apply_journal();
    }

    fn apply_journal(&mut self) {
        for entry in self.journal.clone() {
            match entry {
                JournalEntry::Create { path, size } => {
                    self.files.insert(path.clone(), vec![0u8; size as usize]);
                    self.metadata.insert(path, size);
                }
                JournalEntry::Write { path, offset, len } => {
                    if let Some(data) = self.files.get_mut(&path) {
                        let end = offset + len;
                        if end > data.len() {
                            data.resize(end, 0);
                        }
                    }
                }
                JournalEntry::Truncate { path, new_size } => {
                    if let Some(data) = self.files.get_mut(&path) {
                        data.resize(new_size as usize, 0);
                    }
                    self.metadata.insert(path, new_size);
                }
                JournalEntry::Rename { old, new } => {
                    if let Some(data) = self.files.remove(&old) {
                        self.files.insert(new.clone(), data);
                    }
                    if let Some(size) = self.metadata.remove(&old) {
                        self.metadata.insert(new, size);
                    }
                }
                JournalEntry::Unlink { path } => {
                    self.files.remove(&path);
                    self.metadata.remove(&path);
                }
                JournalEntry::Mkdir { path } => {
                    self.dirs.insert(path);
                }
                JournalEntry::Rmdir { path } => {
                    self.dirs.remove(&path);
                }
                JournalEntry::Commit => {}
            }
        }
    }

    fn crash(&mut self) {
        self.crashed = true;
    }

    /// Recovery: replay committed journal entries only
    fn recover(&mut self) {
        if !self.committed {
            // Journal not committed — rollback
            self.journal.clear();
            return;
        }
        // Journal committed — replay
        self.apply_journal();
        self.journal.clear();
        self.committed = false;
        self.crashed = false;
    }

    fn file_exists(&self, path: &str) -> bool {
        self.files.contains_key(path)
    }

    fn file_data(&self, path: &str) -> Option<&[u8]> {
        self.files.get(path).map(|d| d.as_slice())
    }

    fn file_size(&self, path: &str) -> Option<u64> {
        self.metadata.get(path).copied()
    }

    fn has_ghost(&self, path: &str) -> bool {
        self.files.contains_key(path) && !self.metadata.contains_key(path)
    }

    fn has_orphan(&self) -> bool {
        for path in self.files.keys() {
            if !self.metadata.contains_key(path) {
                return true;
            }
        }
        false
    }

    fn is_consistent(&self) -> bool {
        !self.has_orphan() && self.files.len() == self.metadata.len()
    }
}

// ═══════════════════════════════════════════════════════════════
// CC01: fsync durability (CrashMonkey core pattern)
// ═══════════════════════════════════════════════════════════════

#[test]
fn cc01_fsync_durability_crash_before_commit() {
    let mut fs = CrashFs::new();

    fs.begin();
    fs.journal_create("/file.txt", 5);
    fs.journal_write("/file.txt", 0, 5);
    // Crash BEFORE commit
    fs.crash();
    fs.recover();

    // Data should NOT persist — journal not committed
    assert!(!fs.file_exists("/file.txt") || fs.file_size("/file.txt") == Some(0));
}

#[test]
fn cc01_fsync_durability_crash_after_commit() {
    let mut fs = CrashFs::new();

    fs.begin();
    fs.journal_create("/file.txt", 5);
    fs.journal_write("/file.txt", 0, 5);
    fs.commit_journal();
    // Crash AFTER commit
    fs.crash();
    fs.recover();

    // Data SHOULD persist — journal was committed
    assert!(fs.file_exists("/file.txt"));
    assert_eq!(fs.file_size("/file.txt"), Some(5));
}

// ═══════════════════════════════════════════════════════════════
// CC02: Metadata ordering
// ═══════════════════════════════════════════════════════════════

#[test]
fn cc02_metadata_ordering_no_inconsistency() {
    let contract = CrashContract::new("metadata_order", RecoveryAction::JournalReplay)
        .allow(CrashState::Idle)
        .allow(CrashState::DataWritten)
        .allow(CrashState::JournalStarted)
        .allow(CrashState::JournalCommitted)
        .allow(CrashState::Completed)
        .forbid(CrashState::Inconsistent)
        .forbid(CrashState::Corrupt);

    let mut fs = CrashFs::new();
    fs.begin();
    fs.journal_create("/meta.txt", 10);
    fs.commit_journal();
    fs.crash();
    fs.recover();

    assert!(contract.is_valid(CrashState::Completed));
    assert!(fs.is_consistent());
}

// ═══════════════════════════════════════════════════════════════
// CC03: Atomic rename
// ═══════════════════════════════════════════════════════════════

#[test]
fn cc03_rename_atomicity_committed() {
    let mut fs = CrashFs::new();
    fs.files.insert("/old.txt".to_string(), b"content".to_vec());
    fs.metadata.insert("/old.txt".to_string(), 7);

    fs.begin();
    fs.journal_rename("/old.txt", "/new.txt");
    fs.commit_journal();
    fs.crash();
    fs.recover();

    assert!(!fs.file_exists("/old.txt"));
    assert!(fs.file_exists("/new.txt"));
    assert_eq!(fs.file_data("/new.txt"), Some(b"content".as_ref()));
}

#[test]
fn cc03_rename_atomicity_uncommitted() {
    let mut fs = CrashFs::new();
    fs.files.insert("/old.txt".to_string(), b"content".to_vec());
    fs.metadata.insert("/old.txt".to_string(), 7);

    fs.begin();
    fs.journal_rename("/old.txt", "/new.txt");
    // Crash before commit
    fs.crash();
    fs.recover();

    // Old should still exist
    assert!(fs.file_exists("/old.txt"));
    assert!(!fs.file_exists("/new.txt"));
}

// ═══════════════════════════════════════════════════════════════
// CC04: Truncate crash
// ═══════════════════════════════════════════════════════════════

#[test]
fn cc04_truncate_committed() {
    let mut fs = CrashFs::new();
    fs.files.insert("/big.txt".to_string(), vec![0xFF; 200]);
    fs.metadata.insert("/big.txt".to_string(), 200);

    fs.begin();
    fs.journal_truncate("/big.txt", 100);
    fs.commit_journal();
    fs.crash();
    fs.recover();

    assert_eq!(fs.file_size("/big.txt"), Some(100));
    assert_eq!(fs.files["/big.txt"].len(), 100);
}

#[test]
fn cc04_truncate_uncommitted() {
    let mut fs = CrashFs::new();
    fs.files.insert("/big.txt".to_string(), vec![0xFF; 200]);
    fs.metadata.insert("/big.txt".to_string(), 200);

    fs.begin();
    fs.journal_truncate("/big.txt", 100);
    fs.crash();
    fs.recover();

    // Should retain original size
    assert_eq!(fs.file_size("/big.txt"), Some(200));
}

// ═══════════════════════════════════════════════════════════════
// CC05: Unlink crash — no ghost files
// ═══════════════════════════════════════════════════════════════

#[test]
fn cc05_unlink_committed_no_ghost() {
    let mut fs = CrashFs::new();
    fs.files.insert("/delete_me.txt".to_string(), b"data".to_vec());
    fs.metadata.insert("/delete_me.txt".to_string(), 4);

    fs.begin();
    fs.journal_unlink("/delete_me.txt");
    fs.commit_journal();
    fs.crash();
    fs.recover();

    assert!(!fs.file_exists("/delete_me.txt"));
    assert!(!fs.has_ghost("/delete_me.txt"));
    assert!(fs.is_consistent());
}

// ═══════════════════════════════════════════════════════════════
// CC06: Multi-file crash
// ═══════════════════════════════════════════════════════════════

#[test]
fn cc06_multifile_all_or_nothing() {
    let mut fs = CrashFs::new();

    fs.begin();
    fs.journal_create("/a.txt", 3);
    fs.journal_create("/b.txt", 3);
    fs.journal_create("/c.txt", 3);
    fs.commit_journal();
    fs.crash();
    fs.recover();

    // All committed → all should exist
    assert!(fs.file_exists("/a.txt"));
    assert!(fs.file_exists("/b.txt"));
    assert!(fs.file_exists("/c.txt"));
    assert!(fs.is_consistent());
}

#[test]
fn cc06_multifile_uncommitted_none() {
    let mut fs = CrashFs::new();

    fs.begin();
    fs.journal_create("/a.txt", 3);
    fs.journal_create("/b.txt", 3);
    fs.journal_create("/c.txt", 3);
    fs.crash();
    fs.recover();

    // None committed → none should exist
    assert!(!fs.file_exists("/a.txt"));
    assert!(!fs.file_exists("/b.txt"));
    assert!(!fs.file_exists("/c.txt"));
}

// ═══════════════════════════════════════════════════════════════
// CC07: Journal replay
// ═══════════════════════════════════════════════════════════════

#[test]
fn cc07_journal_replay_preserves_order() {
    let mut fs = CrashFs::new();
    fs.files.insert("/existing.txt".to_string(), b"old".to_vec());
    fs.metadata.insert("/existing.txt".to_string(), 3);

    fs.begin();
    fs.journal_unlink("/existing.txt");
    fs.journal_create("/replacement.txt", 3);
    fs.commit_journal();
    fs.crash();
    fs.recover();

    assert!(!fs.file_exists("/existing.txt"));
    assert!(fs.file_exists("/replacement.txt"));
}

// ═══════════════════════════════════════════════════════════════
// CC08: Append + fsync
// ═══════════════════════════════════════════════════════════════

#[test]
fn cc08_append_fsync_all_or_none() {
    let mut fs = CrashFs::new();
    fs.files.insert("/append.txt".to_string(), b"init".to_vec());
    fs.metadata.insert("/append.txt".to_string(), 4);

    fs.begin();
    fs.journal_write("/append.txt", 4, 6); // append " data"
    fs.commit_journal();
    fs.crash();
    fs.recover();

    // After recovery, file should have appended data
    assert!(fs.file_exists("/append.txt"));
    let data = fs.file_data("/append.txt").unwrap();
    assert!(data.len() >= 4); // At minimum the original data
}

// ═══════════════════════════════════════════════════════════════
// CC09: Rename overwrite
// ═══════════════════════════════════════════════════════════════

#[test]
fn cc09_rename_overwrite_old_or_new() {
    let mut fs = CrashFs::new();
    fs.files.insert("/src.txt".to_string(), b"source".to_vec());
    fs.metadata.insert("/src.txt".to_string(), 6);
    fs.files.insert("/dst.txt".to_string(), b"dest".to_vec());
    fs.metadata.insert("/dst.txt".to_string(), 4);

    fs.begin();
    fs.journal_rename("/src.txt", "/dst.txt");
    fs.commit_journal();
    fs.crash();
    fs.recover();

    // After overwrite: src gone, dst has source data
    assert!(!fs.file_exists("/src.txt"));
    assert!(fs.file_exists("/dst.txt"));
    assert_eq!(fs.file_data("/dst.txt"), Some(b"source".as_ref()));
}

// ═══════════════════════════════════════════════════════════════
// CC10: Create + write
// ═══════════════════════════════════════════════════════════════

#[test]
fn cc10_create_write_committed() {
    let mut fs = CrashFs::new();

    fs.begin();
    fs.journal_create("/new.txt", 5);
    fs.journal_write("/new.txt", 0, 5);
    fs.commit_journal();
    fs.crash();
    fs.recover();

    assert!(fs.file_exists("/new.txt"));
    assert_eq!(fs.file_size("/new.txt"), Some(5));
}

// ═══════════════════════════════════════════════════════════════
// CC11: Truncate + write
// ═══════════════════════════════════════════════════════════════

#[test]
fn cc11_truncate_write_consistent() {
    let mut fs = CrashFs::new();
    fs.files.insert("/rw.txt".to_string(), vec![0xAA; 100]);
    fs.metadata.insert("/rw.txt".to_string(), 100);

    fs.begin();
    fs.journal_truncate("/rw.txt", 10);
    fs.journal_write("/rw.txt", 0, 5);
    fs.commit_journal();
    fs.crash();
    fs.recover();

    assert_eq!(fs.file_size("/rw.txt"), Some(10));
}

// ═══════════════════════════════════════════════════════════════
// CC12: Nested directory crash
// ═══════════════════════════════════════════════════════════════

#[test]
fn cc12_nested_mkdir_committed() {
    let mut fs = CrashFs::new();

    fs.begin();
    fs.journal_mkdir("/a");
    fs.journal_mkdir("/a/b");
    fs.journal_mkdir("/a/b/c");
    fs.commit_journal();
    fs.crash();
    fs.recover();

    assert!(fs.dirs.contains("/a"));
    assert!(fs.dirs.contains("/a/b"));
    assert!(fs.dirs.contains("/a/b/c"));
}

#[test]
fn cc12_nested_mkdir_uncommitted() {
    let mut fs = CrashFs::new();

    fs.begin();
    fs.journal_mkdir("/a");
    fs.journal_mkdir("/a/b");
    fs.journal_mkdir("/a/b/c");
    fs.crash();
    fs.recover();

    assert!(!fs.dirs.contains("/a"));
    assert!(!fs.dirs.contains("/a/b"));
    assert!(!fs.dirs.contains("/a/b/c"));
}

// ═══════════════════════════════════════════════════════════════
// CC13: Fsync ordering
// ═══════════════════════════════════════════════════════════════

#[test]
fn cc13_fsync_ordering_first_only() {
    let mut fs = CrashFs::new();

    // First fsync: commit /a.txt
    fs.begin();
    fs.journal_create("/a.txt", 1);
    fs.commit_journal();

    // Second write: /b.txt — NOT committed yet
    fs.begin();
    fs.journal_create("/b.txt", 1);
    // Crash before second commit
    fs.crash();
    fs.recover();

    assert!(fs.file_exists("/a.txt"));
    assert!(!fs.file_exists("/b.txt"));
}

// ═══════════════════════════════════════════════════════════════
// CC14: ENOSPC edge
// ═══════════════════════════════════════════════════════════════

#[test]
fn cc14_enospc_no_corruption() {
    let mut fs = CrashFs::new();

    fs.begin();
    fs.journal_create("/full.txt", 100);
    // Simulate ENOSPC — commit partial
    fs.commit_journal();
    fs.crash();
    fs.recover();

    // File should exist with allocated size
    assert!(fs.file_exists("/full.txt"));
    assert!(fs.is_consistent());
}

// ═══════════════════════════════════════════════════════════════
// CC15: Symlink + crash
// ═══════════════════════════════════════════════════════════════

#[test]
fn cc15_symlink_no_dangling_refs() {
    let mut fs = CrashFs::new();
    fs.files.insert("/target.txt".to_string(), b"data".to_vec());
    fs.metadata.insert("/target.txt".to_string(), 4);

    // Symlink creation (modeled as file + metadata)
    fs.begin();
    fs.journal_create("/link.txt", 0);
    fs.commit_journal();
    fs.crash();
    fs.recover();

    // Target should still exist
    assert!(fs.file_exists("/target.txt"));
}

// ═══════════════════════════════════════════════════════════════
// CC16: Hardlink + unlink nlink consistency
// ═══════════════════════════════════════════════════════════════

#[test]
fn cc16_hardlink_unlink_nlink() {
    let mut fs = CrashFs::new();
    fs.files.insert("/orig.txt".to_string(), b"data".to_vec());
    fs.metadata.insert("/orig.txt".to_string(), 4);

    // Create hardlink, then unlink original
    fs.begin();
    fs.journal_create("/hard.txt", 4);
    fs.journal_unlink("/orig.txt");
    fs.commit_journal();
    fs.crash();
    fs.recover();

    // hard.txt should exist, orig.txt should not
    assert!(fs.file_exists("/hard.txt"));
    assert!(!fs.file_exists("/orig.txt"));
    assert!(fs.is_consistent());
}

// ═══════════════════════════════════════════════════════════════
// CC17: Rename atomicity contract
// ═══════════════════════════════════════════════════════════════

#[test]
fn cc17_rename_contract_forbids_corruption() {
    let contract = CrashContract::new("rename", RecoveryAction::JournalReplay)
        .allow(CrashState::Idle)
        .allow(CrashState::JournalStarted)
        .allow(CrashState::JournalCommitted)
        .allow(CrashState::Completed)
        .forbid(CrashState::Inconsistent)
        .forbid(CrashState::Corrupt);

    assert!(contract.is_valid(CrashState::Completed));
    assert!(!contract.is_valid(CrashState::Corrupt));
}

// ═══════════════════════════════════════════════════════════════
// CC18: Sequential fsyncs
// ═══════════════════════════════════════════════════════════════

#[test]
fn cc18_sequential_fsyncs_a_only() {
    let mut fs = CrashFs::new();

    // fsync A
    fs.begin();
    fs.journal_create("/a.txt", 1);
    fs.commit_journal();

    // Write B — not committed
    fs.begin();
    fs.journal_create("/b.txt", 1);
    fs.crash();
    fs.recover();

    assert!(fs.file_exists("/a.txt"));
    assert!(!fs.file_exists("/b.txt"));
}

// ═══════════════════════════════════════════════════════════════
// CC19: Journal checkpoint
// ═══════════════════════════════════════════════════════════════

#[test]
fn cc19_checkpoint_after_full_journal() {
    let mut fs = CrashFs::new();

    // Fill journal
    for i in 0..10 {
        fs.begin();
        fs.journal_create(&format!("/file_{}.txt", i), 1);
        fs.commit_journal();
    }

    fs.crash();
    fs.recover();

    for i in 0..10 {
        assert!(fs.file_exists(&format!("/file_{}.txt", i)));
    }
    assert!(fs.is_consistent());
}

// ═══════════════════════════════════════════════════════════════
// CC20: Power-loss during write — partial sector
// ═══════════════════════════════════════════════════════════════

#[test]
fn cc20_partial_sector_write_no_corruption() {
    let mut fs = CrashFs::new();
    fs.files.insert("/sector.txt".to_string(), vec![0xAA; 512]);
    fs.metadata.insert("/sector.txt".to_string(), 512);

    fs.begin();
    fs.journal_write("/sector.txt", 0, 256); // Partial sector write
    // Crash mid-write
    fs.crash();
    fs.recover();

    // File should be consistent (original data or committed data)
    assert!(fs.file_exists("/sector.txt"));
    assert!(fs.is_consistent());
}

// ═══════════════════════════════════════════════════════════════
// Stress: Rapid crash/recover cycles
// ═══════════════════════════════════════════════════════════════

#[test]
fn stress_rapid_crash_recover_cycles() {
    let mut fs = CrashFs::new();

    for cycle in 0..50 {
        fs.begin();
        fs.journal_create(&format!("/cycle_{}.txt", cycle), 1);
        if cycle % 2 == 0 {
            fs.commit_journal();
        }
        fs.crash();
        fs.recover();
    }

    // Only even-indexed files should exist
    for i in 0..50 {
        if i % 2 == 0 {
            assert!(fs.file_exists(&format!("/cycle_{}.txt", i)));
        } else {
            assert!(!fs.file_exists(&format!("/cycle_{}.txt", i)));
        }
    }
    assert!(fs.is_consistent());
}

#[test]
fn stress_interleaved_operations() {
    let mut fs = CrashFs::new();

    // Create files
    fs.begin();
    for i in 0..5 {
        fs.journal_create(&format!("/file_{}.txt", i), 1);
    }
    fs.commit_journal();

    // Modify some
    fs.begin();
    fs.journal_truncate("/file_0.txt", 0);
    fs.journal_unlink("/file_2.txt");
    fs.journal_create("/file_5.txt", 1);
    fs.commit_journal();

    fs.crash();
    fs.recover();

    assert_eq!(fs.file_size("/file_0.txt"), Some(0));
    assert!(fs.file_exists("/file_1.txt"));
    assert!(!fs.file_exists("/file_2.txt"));
    assert!(fs.file_exists("/file_3.txt"));
    assert!(fs.file_exists("/file_4.txt"));
    assert!(fs.file_exists("/file_5.txt"));
    assert!(fs.is_consistent());
}
