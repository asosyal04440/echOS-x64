//! # Landlock-benzeri Yol Tabanli Erişim Kontrolu
//!
//! Süreç bazlı yol politikası uygular.
//! En uzun yol eşleşmesi kazanır; aynı uzunlukta `Deny` önceliklidir.

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Read,
    Write,
    Execute,
    Create,
    Delete,
    Rename,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleAction {
    Allow,
    Deny,
}

#[derive(Clone, Debug)]
pub struct PathRule {
    pub path: String,
    pub mask: u32,
    pub action: RuleAction,
}

#[derive(Default)]
struct TaskPolicy {
    enforce_default_deny: bool,
    rules: Vec<PathRule>,
}

pub const ACCESS_READ: u32 = 1 << 0;
pub const ACCESS_WRITE: u32 = 1 << 1;
pub const ACCESS_EXECUTE: u32 = 1 << 2;
pub const ACCESS_CREATE: u32 = 1 << 3;
pub const ACCESS_DELETE: u32 = 1 << 4;
pub const ACCESS_RENAME: u32 = 1 << 5;

fn access_bit(access: Access) -> u32 {
    match access {
        Access::Read => ACCESS_READ,
        Access::Write => ACCESS_WRITE,
        Access::Execute => ACCESS_EXECUTE,
        Access::Create => ACCESS_CREATE,
        Access::Delete => ACCESS_DELETE,
        Access::Rename => ACCESS_RENAME,
    }
}

fn normalize_path(path: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        if segment.is_empty() || segment == "." {
            continue;
        }
        if segment == ".." {
            let _ = out.pop();
            continue;
        }
        out.push(segment);
    }
    if out.is_empty() {
        return "/".to_string();
    }
    let mut normalized = String::from("/");
    normalized.push_str(&out.join("/"));
    normalized
}

fn path_matches(rule: &str, path: &str) -> bool {
    if rule == "/" {
        return true;
    }
    if path == rule {
        return true;
    }
    path.starts_with(rule)
        && path
            .as_bytes()
            .get(rule.len())
            .map(|b| *b == b'/')
            .unwrap_or(false)
}

lazy_static::lazy_static! {
    static ref POLICIES: Mutex<BTreeMap<usize, TaskPolicy>> = Mutex::new(BTreeMap::new());
}

static LANDLOCK_ENABLED: AtomicBool = AtomicBool::new(false);

fn current_pid() -> usize {
    crate::task::scheduler::current_task_id()
}

pub fn init() {
    LANDLOCK_ENABLED.store(true, Ordering::SeqCst);
    crate::serial_println!("[LANDLOCK] Path-based access control initialized");
}

pub fn is_enabled() -> bool {
    LANDLOCK_ENABLED.load(Ordering::SeqCst)
}

pub fn set_current_task_default_deny(enabled: bool) {
    let pid = current_pid();
    let mut policies = POLICIES.lock();
    let policy = policies.entry(pid).or_default();
    policy.enforce_default_deny = enabled;
}

pub fn add_rule_for_current_task(path: &str, mask: u32, action: RuleAction) {
    let pid = current_pid();
    let mut policies = POLICIES.lock();
    let policy = policies.entry(pid).or_default();
    policy.rules.push(PathRule {
        path: normalize_path(path),
        mask,
        action,
    });
}

pub fn clear_rules_for_current_task() {
    let pid = current_pid();
    POLICIES.lock().remove(&pid);
}

pub fn check_path_for_current_task(path: &str, access: Access) -> bool {
    if !is_enabled() {
        return true;
    }
    let pid = current_pid();
    check_path_for_pid(pid, path, access)
}

pub fn check_path_for_pid(pid: usize, path: &str, access: Access) -> bool {
    if !is_enabled() {
        return true;
    }

    let normalized = normalize_path(path);
    let bit = access_bit(access);
    let policies = POLICIES.lock();
    let policy = match policies.get(&pid) {
        Some(p) => p,
        None => return true,
    };

    let mut decision: Option<(usize, RuleAction)> = None;
    for rule in policy.rules.iter() {
        if rule.mask & bit == 0 {
            continue;
        }
        if !path_matches(&rule.path, &normalized) {
            continue;
        }

        let score = rule.path.len();
        match decision {
            None => {
                decision = Some((score, rule.action));
            }
            Some((best_score, best_action)) => {
                if score > best_score || (score == best_score && best_action != RuleAction::Deny) {
                    decision = Some((score, rule.action));
                }
            }
        }
    }

    match decision {
        Some((_, RuleAction::Allow)) => true,
        Some((_, RuleAction::Deny)) => false,
        None => !policy.enforce_default_deny,
    }
}
