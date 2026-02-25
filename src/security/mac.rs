//! # SELinux-like Mandatory Access Control
//!
//! Policy-based access control for processes and resources.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;
use spin::Mutex;

/// Security context (like SELinux context)
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SecurityContext {
    pub user: String,
    pub role: String,
    pub type_: String,
    pub level: SecurityLevel,
}

impl SecurityContext {
    pub fn new(user: &str, role: &str, type_: &str, level: SecurityLevel) -> Self {
        SecurityContext {
            user: String::from(user),
            role: String::from(role),
            type_: String::from(type_),
            level,
        }
    }

    pub fn system_u() -> Self {
        SecurityContext::new("system_u", "system_r", "kernel_t", SecurityLevel::SystemHigh)
    }

    pub fn user_u() -> Self {
        SecurityContext::new("user_u", "user_r", "user_t", SecurityLevel::Low)
    }

    pub fn unconfined_u() -> Self {
        SecurityContext::new("unconfined_u", "unconfined_r", "unconfined_t", SecurityLevel::Low)
    }
}

/// Security level (MLS/MCS)
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SecurityLevel {
    pub sensitivity: u8,
    pub categories: u32,  // Bitmask for categories
}

impl SecurityLevel {
    pub const SystemHigh: Self = SecurityLevel { sensitivity: 255, categories: 0xFFFFFFFF };
    pub const SystemLow: Self = SecurityLevel { sensitivity: 0, categories: 0 };
    pub const Low: Self = SecurityLevel { sensitivity: 1, categories: 0 };
    pub const Medium: Self = SecurityLevel { sensitivity: 2, categories: 0 };
    pub const High: Self = SecurityLevel { sensitivity: 3, categories: 0 };
    pub const Secret: Self = SecurityLevel { sensitivity: 4, categories: 0 };
    pub const TopSecret: Self = SecurityLevel { sensitivity: 5, categories: 0 };

    pub fn dominates(&self, other: &SecurityLevel) -> bool {
        self.sensitivity >= other.sensitivity && (self.categories & other.categories) == other.categories
    }
}

/// Access vector (permissions)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AccessVector {
    pub permissions: u32,
}

impl AccessVector {
    // File permissions
    pub const FILE_READ: u32 = 1 << 0;
    pub const FILE_WRITE: u32 = 1 << 1;
    pub const FILE_EXECUTE: u32 = 1 << 2;
    pub const FILE_APPEND: u32 = 1 << 3;
    pub const FILE_CREATE: u32 = 1 << 4;
    pub const FILE_DELETE: u32 = 1 << 5;
    pub const FILE_RENAME: u32 = 1 << 6;
    pub const FILE_LINK: u32 = 1 << 7;
    pub const FILE_CHMOD: u32 = 1 << 8;
    pub const FILE_CHOWN: u32 = 1 << 9;

    // Process permissions
    pub const PROCESS_FORK: u32 = 1 << 0;
    pub const PROCESS_TRANSITION: u32 = 1 << 1;
    pub const PROCESS_SIGCHLD: u32 = 1 << 2;
    pub const PROCESS_SIGKILL: u32 = 1 << 3;
    pub const PROCESS_SIGSTOP: u32 = 1 << 4;
    pub const PROCESS_SIGINJECT: u32 = 1 << 5;
    pub const PROCESS_PTRACE: u32 = 1 << 6;
    pub const PROCESS_EXECMEM: u32 = 1 << 7;
    pub const PROCESS_EXECSTACK: u32 = 1 << 8;
    pub const PROCESS_NOATSECURE: u32 = 1 << 9;

    // Socket permissions
    pub const SOCKET_READ: u32 = 1 << 0;
    pub const SOCKET_WRITE: u32 = 1 << 1;
    pub const SOCKET_CONNECT: u32 = 1 << 2;
    pub const SOCKET_BIND: u32 = 1 << 3;
    pub const SOCKET_LISTEN: u32 = 1 << 4;
    pub const SOCKET_ACCEPT: u32 = 1 << 5;

    pub const NONE: Self = AccessVector { permissions: 0 };
    pub const ALL: Self = AccessVector { permissions: 0xFFFFFFFF };

    pub fn new(permissions: u32) -> Self {
        AccessVector { permissions }
    }

    pub fn has(&self, perm: u32) -> bool {
        (self.permissions & perm) != 0
    }

    pub fn union(&self, other: &AccessVector) -> AccessVector {
        AccessVector::new(self.permissions | other.permissions)
    }

    pub fn intersect(&self, other: &AccessVector) -> AccessVector {
        AccessVector::new(self.permissions & other.permissions)
    }
}

/// Object class
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectClass {
    File,
    Directory,
    Process,
    Socket,
    Device,
    Key,
    Port,
    Node,
    NetworkInterface,
    Security,
    Capability,
}

/// Access decision
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessDecision {
    Allow,
    Deny,
    AuditAllow,
    AuditDeny,
    DontAudit,
    NeverAllow,
}

/// TE rule (Type Enforcement)
#[derive(Clone, Debug)]
pub struct TeRule {
    pub source_type: String,
    pub target_type: String,
    pub object_class: ObjectClass,
    pub permissions: AccessVector,
    pub decision: AccessDecision,
}

/// Transition rule
#[derive(Clone, Debug)]
pub struct TransitionRule {
    pub source_type: String,
    pub target_type: String,
    pub object_class: ObjectClass,
    pub new_type: String,
}

/// Role allow rule
#[derive(Clone, Debug)]
pub struct RoleAllowRule {
    pub current_role: String,
    pub new_role: String,
}

/// Role transition rule
#[derive(Clone, Debug)]
pub struct RoleTransitionRule {
    pub current_role: String,
    pub source_type: String,
    pub new_role: String,
}

/// MAC Policy
#[derive(Clone, Debug)]
pub struct MacPolicy {
    pub name: String,
    pub version: u32,
    pub te_rules: Vec<TeRule>,
    pub transitions: Vec<TransitionRule>,
    pub role_allows: Vec<RoleAllowRule>,
    pub role_transitions: Vec<RoleTransitionRule>,
    pub default_user: String,
    pub default_role: String,
    pub default_type: String,
    pub enforce: bool,
}

impl MacPolicy {
    pub fn new(name: &str) -> Self {
        MacPolicy {
            name: String::from(name),
            version: 1,
            te_rules: Vec::new(),
            transitions: Vec::new(),
            role_allows: Vec::new(),
            role_transitions: Vec::new(),
            default_user: String::from("system_u"),
            default_role: String::from("system_r"),
            default_type: String::from("kernel_t"),
            enforce: true,
        }
    }

    /// Add TE rule
    pub fn add_rule(&mut self, source: &str, target: &str, class: ObjectClass, perms: AccessVector, decision: AccessDecision) {
        self.te_rules.push(TeRule {
            source_type: String::from(source),
            target_type: String::from(target),
            object_class: class,
            permissions: perms,
            decision,
        });
    }

    /// Add transition rule
    pub fn add_transition(&mut self, source: &str, target: &str, class: ObjectClass, new_type: &str) {
        self.transitions.push(TransitionRule {
            source_type: String::from(source),
            target_type: String::from(target),
            object_class: class,
            new_type: String::from(new_type),
        });
    }

    /// Check access
    pub fn check_access(&self, source_ctx: &SecurityContext, target_ctx: &SecurityContext, class: ObjectClass, requested: AccessVector) -> AccessDecision {
        // MLS check first
        if !source_ctx.level.dominates(&target_ctx.level) && requested.has(AccessVector::FILE_READ) {
            return AccessDecision::Deny;
        }

        // TE rules
        for rule in &self.te_rules {
            if rule.source_type == source_ctx.type_
                && rule.target_type == target_ctx.type_
                && rule.object_class == class
            {
                if rule.permissions.intersect(&requested).permissions != 0 {
                    return rule.decision;
                }
            }
        }

        // Default deny
        if self.enforce {
            AccessDecision::Deny
        } else {
            AccessDecision::Allow
        }
    }

    /// Get transition type
    pub fn get_transition(&self, source_type: &str, target_type: &str, class: ObjectClass) -> Option<&str> {
        for trans in &self.transitions {
            if trans.source_type == source_type && trans.target_type == target_type && trans.object_class == class {
                return Some(&trans.new_type);
            }
        }
        None
    }
}

/// Create default policy
pub fn create_default_policy() -> MacPolicy {
    let mut policy = MacPolicy::new("default");

    // Kernel can do everything
    policy.add_rule("kernel_t", "kernel_t", ObjectClass::Process, AccessVector::ALL, AccessDecision::Allow);
    policy.add_rule("kernel_t", "file_t", ObjectClass::File, AccessVector::ALL, AccessDecision::Allow);
    policy.add_rule("kernel_t", "dir_t", ObjectClass::Directory, AccessVector::ALL, AccessDecision::Allow);
    policy.add_rule("kernel_t", "device_t", ObjectClass::Device, AccessVector::ALL, AccessDecision::Allow);

    // User domain
    policy.add_rule("user_t", "user_home_t", ObjectClass::File, AccessVector::ALL, AccessDecision::Allow);
    policy.add_rule("user_t", "user_home_t", ObjectClass::Directory, AccessVector::ALL, AccessDecision::Allow);
    policy.add_rule("user_t", "user_tmp_t", ObjectClass::File, AccessVector::ALL, AccessDecision::Allow);
    policy.add_rule("user_t", "bin_t", ObjectClass::File, AccessVector::new(AccessVector::FILE_READ | AccessVector::FILE_EXECUTE), AccessDecision::Allow);
    policy.add_rule("user_t", "lib_t", ObjectClass::File, AccessVector::new(AccessVector::FILE_READ | AccessVector::FILE_EXECUTE), AccessDecision::Allow);

    // Process transitions
    policy.add_transition("user_t", "bin_t", ObjectClass::Process, "user_t");
    policy.add_transition("kernel_t", "init_t", ObjectClass::Process, "init_t");

    // Role transitions
    policy.role_allows.push(RoleAllowRule {
        current_role: String::from("system_r"),
        new_role: String::from("user_r"),
    });

    policy.role_transitions.push(RoleTransitionRule {
        current_role: String::from("system_r"),
        source_type: String::from("init_t"),
        new_role: String::from("user_r"),
    });

    policy
}

// Global policy
lazy_static::lazy_static! {
    static ref MAC_POLICY: Mutex<MacPolicy> = Mutex::new(create_default_policy());
    static ref PROCESS_CONTEXTS: Mutex<BTreeMap<u64, SecurityContext>> = Mutex::new(BTreeMap::new());
    static ref FILE_CONTEXTS: Mutex<BTreeMap<String, SecurityContext>> = Mutex::new(BTreeMap::new());
}

/// Initialize MAC for process
pub fn init_process_context(pid: u64, context: SecurityContext) {
    PROCESS_CONTEXTS.lock().insert(pid, context);
}

/// Get process context
pub fn get_process_context(pid: u64) -> Option<SecurityContext> {
    PROCESS_CONTEXTS.lock().get(&pid).cloned()
}

/// Set file context
pub fn set_file_context(path: &str, context: SecurityContext) {
    FILE_CONTEXTS.lock().insert(String::from(path), context);
}

/// Get file context
pub fn get_file_context(path: &str) -> Option<SecurityContext> {
    FILE_CONTEXTS.lock().get(path).cloned()
}

/// Check process-file access
pub fn check_file_access(pid: u64, path: &str, requested: AccessVector) -> AccessDecision {
    let process_ctx = match get_process_context(pid) {
        Some(ctx) => ctx,
        None => return AccessDecision::Deny,
    };

    let file_ctx = match get_file_context(path) {
        Some(ctx) => ctx,
        None => {
            // Default context
            SecurityContext::new("system_u", "object_r", "file_t", SecurityLevel::Low)
        }
    };

    let policy = MAC_POLICY.lock();
    policy.check_access(&process_ctx, &file_ctx, ObjectClass::File, requested)
}

/// Check process-process access
pub fn check_process_access(source_pid: u64, target_pid: u64, requested: AccessVector) -> AccessDecision {
    let source_ctx = match get_process_context(source_pid) {
        Some(ctx) => ctx,
        None => return AccessDecision::Deny,
    };

    let target_ctx = match get_process_context(target_pid) {
        Some(ctx) => ctx,
        None => return AccessDecision::Deny,
    };

    let policy = MAC_POLICY.lock();
    policy.check_access(&source_ctx, &target_ctx, ObjectClass::Process, requested)
}

/// Compute new context on transition
pub fn compute_transition(source_pid: u64, target_type: &str) -> Option<SecurityContext> {
    let source_ctx = get_process_context(source_pid)?;
    let policy = MAC_POLICY.lock();

    let new_type = policy.get_transition(&source_ctx.type_, target_type, ObjectClass::Process)?;
    
    Some(SecurityContext::new(&source_ctx.user, &source_ctx.role, new_type, source_ctx.level))
}

/// Load custom policy
pub fn load_policy(policy: MacPolicy) {
    *MAC_POLICY.lock() = policy;
}

/// Get current policy
pub fn get_policy() -> MacPolicy {
    MAC_POLICY.lock().clone()
}

/// Set enforcement mode
pub fn set_enforcing(enforce: bool) {
    MAC_POLICY.lock().enforce = enforce;
}

/// Check if enforcing
pub fn is_enforcing() -> bool {
    MAC_POLICY.lock().enforce
}

/// Audit access decision
pub fn audit_decision(decision: AccessDecision, source_pid: u64, target: &str, class: ObjectClass, perms: AccessVector) {
    match decision {
        AccessDecision::AuditAllow | AccessDecision::AuditDeny => {
            crate::serial_println!(
                "[MAC/AUDIT] {:?}: pid={} target={} class={:?} perms={:#x}",
                decision, source_pid, target, class, perms.permissions
            );
        }
        _ => {}
    }
}
