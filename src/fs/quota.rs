//! # Quota Management
//!
//! Disk quota support for users and groups.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// QUOTA CONSTANTS
// ============================================================================

/// Quota types
pub const USRQUOTA: u32 = 0;
pub const GRPQUOTA: u32 = 1;
pub const PRJQUOTA: u32 = 2;

/// Quota commands
pub const Q_SYNC: u32 = 0x800001;
pub const Q_GETQUOTA: u32 = 0x800002;
pub const Q_SETQUOTA: u32 = 0x800003;
pub const Q_GETINFO: u32 = 0x800004;
pub const Q_SETINFO: u32 = 0x800005;
pub const Q_GETFMT: u32 = 0x800006;
pub const Q_GETNEXTQUOTA: u32 = 0x800007;

/// Quota formats
pub const QFMT_VFS_OLD: u32 = 1;
pub const QFMT_VFS_V0: u32 = 2;
pub const QFMT_VFS_V1: u32 = 4;

/// Quota flags
pub const Q_QUOTA_ENFD: u32 = 0x01;
pub const Q_QUOTA_OFF: u32 = 0x02;
pub const Q_FAKE_QUOTA: u32 = 0x04;

// ============================================================================
// QUOTA STRUCTURES
// ============================================================================

/// Quota information for a user/group
#[derive(Clone, Debug)]
pub struct QuotaDqblk {
    /// Hard limit (blocks)
    pub dqb_bhardlimit: u64,
    /// Soft limit (blocks)
    pub dqb_bsoftlimit: u64,
    /// Current usage (blocks)
    pub dqb_curspace: u64,
    /// Hard limit (inodes)
    pub dqb_ihardlimit: u64,
    /// Soft limit (inodes)
    pub dqb_isoftlimit: u64,
    /// Current usage (inodes)
    pub dqb_curinodes: u64,
    /// Time limit for soft block limit
    pub dqb_btime: u64,
    /// Time limit for soft inode limit
    pub dqb_itime: u64,
    /// Valid fields
    pub dqb_valid: u32,
}

impl Default for QuotaDqblk {
    fn default() -> Self {
        Self {
            dqb_bhardlimit: 0,
            dqb_bsoftlimit: 0,
            dqb_curspace: 0,
            dqb_ihardlimit: 0,
            dqb_isoftlimit: 0,
            dqb_curinodes: 0,
            dqb_btime: 0,
            dqb_itime: 0,
            dqb_valid: 0,
        }
    }
}

/// Quota info structure
#[derive(Clone, Debug)]
pub struct QuotaInfo {
    /// Number of blocks in filesystem
    pub dqi_bgrace: u64,
    /// Number of inodes in filesystem
    pub dqi_igrace: u64,
    /// Quota flags
    pub dqi_flags: u32,
    /// Quota format
    pub dqi_fmt: u32,
}

impl Default for QuotaInfo {
    fn default() -> Self {
        Self {
            dqi_bgrace: 7 * 24 * 3600, // 7 days
            dqi_igrace: 7 * 24 * 3600,
            dqi_flags: 0,
            dqi_fmt: QFMT_VFS_V1,
        }
    }
}

// ============================================================================
// QUOTA ENTRY
// ============================================================================

#[derive(Clone, Debug)]
pub struct QuotaEntry {
    pub id: u32,
    pub quota_type: u32,
    pub block_usage: AtomicU64,
    pub inode_usage: AtomicU64,
    pub block_hard: u64,
    pub block_soft: u64,
    pub inode_hard: u64,
    pub inode_soft: u64,
    pub block_time: AtomicI64,
    pub inode_time: AtomicI64,
}

impl QuotaEntry {
    pub fn new(id: u32, quota_type: u32) -> Self {
        Self {
            id,
            quota_type,
            block_usage: AtomicU64::new(0),
            inode_usage: AtomicU64::new(0),
            block_hard: 0,
            block_soft: 0,
            inode_hard: 0,
            inode_soft: 0,
            block_time: AtomicI64::new(0),
            inode_time: AtomicI64::new(0),
        }
    }

    /// Check if block quota is exceeded
    pub fn is_block_exceeded(&self) -> bool {
        let usage = self.block_usage.load(Ordering::Relaxed);
        usage > self.block_hard && self.block_hard > 0
    }

    /// Check if inode quota is exceeded
    pub fn is_inode_exceeded(&self) -> bool {
        let usage = self.inode_usage.load(Ordering::Relaxed);
        usage > self.inode_hard && self.inode_hard > 0
    }

    /// Convert to dqblk
    pub fn to_dqblk(&self) -> QuotaDqblk {
        QuotaDqblk {
            dqb_bhardlimit: self.block_hard,
            dqb_bsoftlimit: self.block_soft,
            dqb_curspace: self.block_usage.load(Ordering::Relaxed),
            dqb_ihardlimit: self.inode_hard,
            dqb_isoftlimit: self.inode_soft,
            dqb_curinodes: self.inode_usage.load(Ordering::Relaxed),
            dqb_btime: self.block_time.load(Ordering::Relaxed) as u64,
            dqb_itime: self.inode_time.load(Ordering::Relaxed) as u64,
            dqb_valid: 0xFF,
        }
    }

    /// Update from dqblk
    pub fn from_dqblk(&mut self, dqblk: &QuotaDqblk) {
        self.block_hard = dqblk.dqb_bhardlimit;
        self.block_soft = dqblk.dqb_bsoftlimit;
        self.inode_hard = dqblk.dqb_ihardlimit;
        self.inode_soft = dqblk.dqb_isoftlimit;
    }
}

// ============================================================================
// QUOTA FILESYSTEM
// ============================================================================

pub struct QuotaFilesystem {
    pub device: String,
    pub mount_point: String,
    pub enabled: AtomicU64,
    pub user_quotas: Mutex<BTreeMap<u32, QuotaEntry>>,
    pub group_quotas: Mutex<BTreeMap<u32, QuotaEntry>>,
    pub project_quotas: Mutex<BTreeMap<u32, QuotaEntry>>,
    pub info: Mutex<QuotaInfo>,
}

impl QuotaFilesystem {
    pub fn new(device: &str, mount: &str) -> Self {
        Self {
            device: String::from(device),
            mount_point: String::from(mount),
            enabled: AtomicU64::new(0),
            user_quotas: Mutex::new(BTreeMap::new()),
            group_quotas: Mutex::new(BTreeMap::new()),
            project_quotas: Mutex::new(BTreeMap::new()),
            info: Mutex::new(QuotaInfo::default()),
        }
    }

    /// Enable quota
    pub fn enable(&self, quota_type: u32) -> Result<(), QuotaError> {
        self.enabled.fetch_or(1 << quota_type, Ordering::SeqCst);
        crate::serial_println!("[QUOTA] Enabled {} quota on {}", 
            quota_name(quota_type), self.mount_point);
        Ok(())
    }

    /// Disable quota
    pub fn disable(&self, quota_type: u32) {
        self.enabled.fetch_and(!(1 << quota_type), Ordering::SeqCst);
    }

    /// Check if quota is enabled
    pub fn is_enabled(&self, quota_type: u32) -> bool {
        (self.enabled.load(Ordering::SeqCst) & (1 << quota_type)) != 0
    }

    /// Get quota entry
    pub fn get_quota(&self, quota_type: u32, id: u32) -> Option<QuotaEntry> {
        match quota_type {
            USRQUOTA => self.user_quotas.lock().get(&id).cloned(),
            GRPQUOTA => self.group_quotas.lock().get(&id).cloned(),
            PRJQUOTA => self.project_quotas.lock().get(&id).cloned(),
            _ => None,
        }
    }

    /// Set quota
    pub fn set_quota(&self, quota_type: u32, id: u32, dqblk: &QuotaDqblk) -> Result<(), QuotaError> {
        let entry = match quota_type {
            USRQUOTA => self.user_quotas.lock().entry(id).or_insert_with(|| QuotaEntry::new(id, quota_type)),
            GRPQUOTA => self.group_quotas.lock().entry(id).or_insert_with(|| QuotaEntry::new(id, quota_type)),
            PRJQUOTA => self.project_quotas.lock().entry(id).or_insert_with(|| QuotaEntry::new(id, quota_type)),
            _ => return Err(QuotaError::InvalidType),
        };
        
        entry.from_dqblk(dqblk);
        Ok(())
    }

    /// Charge block usage
    pub fn charge_blocks(&self, quota_type: u32, id: u32, blocks: u64) -> Result<(), QuotaError> {
        if !self.is_enabled(quota_type) {
            return Ok(());
        }
        
        let quotas = match quota_type {
            USRQUOTA => &self.user_quotas,
            GRPQUOTA => &self.group_quotas,
            PRJQUOTA => &self.project_quotas,
            _ => return Err(QuotaError::InvalidType),
        };
        
        let mut quotas = quotas.lock();
        if let Some(entry) = quotas.get_mut(&id) {
            let new_usage = entry.block_usage.load(Ordering::Relaxed) + blocks;
            
            if entry.block_hard > 0 && new_usage > entry.block_hard {
                return Err(QuotaError::OverQuota);
            }
            
            if entry.block_soft > 0 && new_usage > entry.block_soft {
                // Start timer for soft limit
                if entry.block_time.load(Ordering::Relaxed) == 0 {
                    entry.block_time.store(
                        crate::task::scheduler::get_ticks() as i64 + 
                        self.info.lock().dqi_bgrace as i64,
                        Ordering::Relaxed
                    );
                }
            }
            
            entry.block_usage.store(new_usage, Ordering::Relaxed);
        }
        
        Ok(())
    }

    /// Charge inode usage
    pub fn charge_inodes(&self, quota_type: u32, id: u32, count: u64) -> Result<(), QuotaError> {
        if !self.is_enabled(quota_type) {
            return Ok(());
        }
        
        // Similar to charge_blocks
        Ok(())
    }

    /// Sync quota to disk
    pub fn sync(&self) -> Result<(), QuotaError> {
        // Write quota file to disk
        Ok(())
    }
}

fn quota_name(t: u32) -> &'static str {
    match t {
        USRQUOTA => "user",
        GRPQUOTA => "group",
        PRJQUOTA => "project",
        _ => "unknown",
    }
}

// ============================================================================
// QUOTA MANAGER
// ============================================================================

pub struct QuotaManager {
    filesystems: Mutex<BTreeMap<String, QuotaFilesystem>>,
}

impl QuotaManager {
    pub const fn new() -> Self {
        Self {
            filesystems: Mutex::new(BTreeMap::new()),
        }
    }

    pub fn register(&self, device: &str, mount: &str) -> QuotaFilesystem {
        let fs = QuotaFilesystem::new(device, mount);
        self.filesystems.lock().insert(String::from(mount), fs.clone());
        fs
    }

    pub fn get(&self, mount: &str) -> Option<QuotaFilesystem> {
        self.filesystems.lock().get(mount).cloned()
    }
}

// Clone for QuotaFilesystem
impl Clone for QuotaFilesystem {
    fn clone(&self) -> Self {
        Self {
            device: self.device.clone(),
            mount_point: self.mount_point.clone(),
            enabled: AtomicU64::new(self.enabled.load(Ordering::Relaxed)),
            user_quotas: Mutex::new(self.user_quotas.lock().clone()),
            group_quotas: Mutex::new(self.group_quotas.lock().clone()),
            project_quotas: Mutex::new(self.project_quotas.lock().clone()),
            info: Mutex::new(self.info.lock().clone()),
        }
    }
}

lazy_static::lazy_static! {
    pub static ref QUOTA_MANAGER: QuotaManager = QuotaManager::new();
}

// ============================================================================
// ERROR TYPE
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaError {
    InvalidType,
    OverQuota,
    NotEnabled,
    IoError,
}

// ============================================================================
// SYSCALL INTERFACE
// ============================================================================

pub fn sys_quotactl(cmd: u32, special: &str, id: u32, addr: u64) -> i32 {
    let quota_type = (cmd >> 8) & 0xFF;
    let command = cmd & 0xFF;
    
    match command {
        1 => { // Q_QUOTAON
            if let Some(fs) = QUOTA_MANAGER.get(special) {
                let _ = fs.enable(quota_type);
            }
            0
        }
        2 => { // Q_QUOTAOFF
            if let Some(fs) = QUOTA_MANAGER.get(special) {
                fs.disable(quota_type);
            }
            0
        }
        3 => { // Q_GETQUOTA
            if let Some(fs) = QUOTA_MANAGER.get(special) {
                if let Some(entry) = fs.get_quota(quota_type, id) {
                    // Copy dqblk to addr
                    return 0;
                }
            }
            -2 // ENOENT
        }
        4 => { // Q_SETQUOTA
            // Read dqblk from addr and set
            0
        }
        _ => -22, // EINVAL
    }
}

// ============================================================================
// INITIALIZATION
// ============================================================================

pub fn init() {
    crate::serial_println!("[QUOTA] Subsystem initialized");
}
