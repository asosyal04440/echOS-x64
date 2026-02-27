//! # Denetim Alt Sistemi (Audit Subsystem)
//!
//! Güvenlik olaylarının günlüklenmesi ve denetimi.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// DENETİM SABİTLERİ
// ============================================================================

/// Denetim ileti türleri
pub const AUDIT_SYSCALL: u16 = 1300;
pub const AUDIT_FS_WATCH: u16 = 1301;
pub const AUDIT_PATH: u16 = 1302;
pub const AUDIT_LOGIN: u16 = 1303;
pub const AUDIT_USER: u16 = 1304;
pub const AUDIT_KERNEL: u16 = 1305;
pub const AUDIT_ANOM_ABEND: u16 = 1701;
pub const AUDIT_ANOM_LINK: u16 = 1702;
pub const AUDIT_INTEGRITY_DATA: u16 = 1800;
pub const AUDIT_INTEGRITY_METADATA: u16 = 1801;
pub const AUDIT_INTEGRITY_STATUS: u16 = 1802;

/// Denetim filtresi eylemleri
pub const AUDIT_NEVER: u32 = 0;
pub const AUDIT_POSSIBLE: u32 = 1;
pub const AUDIT_ALWAYS: u32 = 2;

/// Denetim filtresi kuralları
pub const AUDIT_FILTER_USER: u32 = 0;
pub const AUDIT_FILTER_TASK: u32 = 1;
pub const AUDIT_FILTER_ENTRY: u32 = 2;
pub const AUDIT_FILTER_WATCH: u32 = 3;
pub const AUDIT_FILTER_EXIT: u32 = 4;
pub const AUDIT_FILTER_PREPEND: u32 = 0x80000000;

// ============================================================================
// DENETİM KAYDI
// ============================================================================

#[derive(Clone, Debug)]
pub struct AuditRecord {
    /// Kayıt türü
    pub msg_type: u16,
    /// Sıra numarası
    pub serial: u32,
    /// Zaman damgası
    pub timestamp: u64,
    /// Oturum kimliği
    pub session_id: u32,
    /// İşlem kimliği
    pub pid: u32,
    /// Kullanıcı kimliği
    pub uid: u32,
    /// Etkin kullanıcı kimliği
    pub euid: u32,
    /// Grup kimliği
    pub gid: u32,
    /// Etkin grup kimliği
    pub egid: u32,
    /// Sistem çağrısı numarası
    pub syscall: i32,
    /// Dönüş değeri
    pub ret: i64,
    /// Çalıştırılabilir dosya yolu
    pub exe: String,
    /// Komut satırı
    pub comm: String,
    /// Ek alanlar
    pub fields: Vec<(String, String)>,
}

impl AuditRecord {
    pub fn new(msg_type: u16, serial: u32) -> Self {
        Self {
            msg_type,
            serial,
            timestamp: crate::task::scheduler::get_ticks(),
            session_id: 0,
            pid: 0,
            uid: 0,
            euid: 0,
            gid: 0,
            egid: 0,
            syscall: -1,
            ret: 0,
            exe: String::new(),
            comm: String::new(),
            fields: Vec::new(),
        }
    }

    /// Alan ekler
    pub fn add_field(&mut self, name: &str, value: &str) {
        self.fields.push((String::from(name), String::from(value)));
    }

    /// Dizeye biçimlendirir
    pub fn format(&self) -> String {
        let mut s = alloc::format!(
            "audit({}.{}:{}): type={} serial={} pid={} uid={} euid={} gid={} egid={} syscall={} ret={}",
            self.timestamp / 1_000_000_000,
            self.timestamp % 1_000_000_000,
            self.session_id,
            self.msg_type,
            self.serial,
            self.pid,
            self.uid,
            self.euid,
            self.gid,
            self.egid,
            self.syscall,
            self.ret
        );

        for (name, value) in &self.fields {
            s.push_str(&alloc::format!(" {}={}", name, value));
        }

        s.push_str(&alloc::format!(" exe=\"{}\" comm=\"{}\"", self.exe, self.comm));

        s
    }
}

// ============================================================================
// DENETİM KURALI
// ============================================================================

#[derive(Clone, Debug)]
pub struct AuditRule {
    /// Kural kimliği
    pub id: u32,
    /// Filtre türü
    pub filter: u32,
    /// Eylem
    pub action: u32,
    /// Sistem çağrısı maskesi
    pub syscall_mask: [u64; 2],
    /// Alan sayısı
    pub field_count: u32,
    /// Alanlar
    pub fields: Vec<AuditRuleField>,
    /// Etkin mi
    pub enabled: AtomicBool,
}

#[derive(Clone, Debug)]
pub struct AuditRuleField {
    pub field: u32,
    pub op: u32,
    pub value: u32,
    pub value_str: String,
}

/// Denetim alanı türleri
pub const AUDIT_PID: u32 = 0;
pub const AUDIT_UID: u32 = 1;
pub const AUDIT_GID: u32 = 2;
pub const AUDIT_LOGINUID: u32 = 3;
pub const AUDIT_PERS: u32 = 4;
pub const AUDIT_ARCH: u32 = 5;
pub const AUDIT_EXIT: u32 = 6;
pub const AUDIT_SUCCESS: u32 = 7;
pub const AUDIT_WATCH: u32 = 8;
pub const AUDIT_PERM: u32 = 9;
pub const AUDIT_DIR: u32 = 10;
pub const AUDIT_FILETYPE: u32 = 11;
pub const AUDIT_ARG0: u32 = 12;
pub const AUDIT_EXE: u32 = 100;

impl AuditRule {
    pub fn new(id: u32, filter: u32, action: u32) -> Self {
        Self {
            id,
            filter,
            action,
            syscall_mask: [0; 2],
            field_count: 0,
            fields: Vec::new(),
            enabled: AtomicBool::new(true),
        }
    }

    /// Sistem çağrısının eşleşip eşleşmediğini kontrol eder
    pub fn matches_syscall(&self, syscall: i32) -> bool {
        if syscall < 0 || syscall >= 128 {
            return false;
        }

        let word = syscall as usize / 64;
        let bit = syscall as usize % 64;

        (self.syscall_mask[word] & (1 << bit)) != 0
    }

    /// Sistem çağrısını maskeye ekler
    pub fn add_syscall(&mut self, syscall: i32) {
        if syscall < 0 || syscall >= 128 {
            return;
        }

        let word = syscall as usize / 64;
        let bit = syscall as usize % 64;

        self.syscall_mask[word] |= 1 << bit;
    }

    /// Alan ekler
    pub fn add_field(&mut self, field: AuditRuleField) {
        self.fields.push(field);
        self.field_count += 1;
    }
}

// ============================================================================
// DENETİM İZLEME
// ============================================================================

#[derive(Clone, Debug)]
pub struct AuditWatch {
    /// İzleme kimliği
    pub id: u32,
    /// İzlenecek yol
    pub path: String,
    /// İzlenecek izinler (r=4, w=2, x=1)
    pub perms: u32,
    /// Filtre anahtarı
    pub key: String,
    /// Dizin mi
    pub is_dir: bool,
}

impl AuditWatch {
    pub fn new(id: u32, path: &str, perms: u32, key: &str) -> Self {
        Self {
            id,
            path: String::from(path),
            perms,
            key: String::from(key),
            is_dir: path.ends_with('/'),
        }
    }

    /// Erişimin eşleşip eşleşmediğini kontrol eder
    pub fn matches(&self, path: &str, perms: u32) -> bool {
        path.starts_with(&self.path) && (perms & self.perms) != 0
    }
}

// ============================================================================
// DENETİM YÖNETİCİSİ
// ============================================================================

pub struct AuditManager {
    /// Etkin mi
    pub enabled: AtomicBool,
    /// Denetim kuralları
    pub rules: Mutex<Vec<AuditRule>>,
    /// Denetim izlemeleri
    pub watches: Mutex<Vec<AuditWatch>>,
    /// Denetim günlüğü
    pub log: Mutex<Vec<AuditRecord>>,
    /// Sonraki sıra numarası
    pub next_serial: AtomicU32,
    /// Sonraki kural kimliği
    pub next_rule_id: AtomicU32,
    /// Sonraki izleme kimliği
    pub next_watch_id: AtomicU32,
    /// Hız sınırı (kayıt/saniye)
    pub rate_limit: AtomicU32,
    /// Biriktirme sınırı
    pub backlog_limit: AtomicU32,
    /// İstatistikler
    pub stats: Mutex<AuditStats>,
}

#[derive(Clone, Debug, Default)]
pub struct AuditStats {
    pub records_sent: u64,
    pub records_lost: u64,
    pub rules_count: u32,
    pub watches_count: u32,
}

impl AuditManager {
    pub const fn new() -> Self {
        Self {
            enabled: AtomicBool::new(false),
            rules: Mutex::new(Vec::new()),
            watches: Mutex::new(Vec::new()),
            log: Mutex::new(Vec::new()),
            next_serial: AtomicU32::new(1),
            next_rule_id: AtomicU32::new(1),
            next_watch_id: AtomicU32::new(1),
            rate_limit: AtomicU32::new(1000),
            backlog_limit: AtomicU32::new(10000),
            stats: Mutex::new(AuditStats::default()),
        }
    }

    /// Denetimi etkinleştirir
    pub fn enable(&self) {
        self.enabled.store(true, Ordering::SeqCst);
        crate::serial_println!("[AUDIT] Enabled");
    }

    /// Denetimi devre dışı bırakır
    pub fn disable(&self) {
        self.enabled.store(false, Ordering::SeqCst);
    }

    /// Etkin mi kontrol eder
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }

    /// Sistem çağrısını günlükler
    pub fn log_syscall(&self, syscall: i32, ret: i64, args: &[u64]) {
        if !self.is_enabled() {
            return;
        }

        let serial = self.next_serial.fetch_add(1, Ordering::SeqCst);
        let mut record = AuditRecord::new(AUDIT_SYSCALL, serial);
        record.syscall = syscall;
        record.ret = ret;

        // Bağımsız değişkenleri ekle
        for (i, arg) in args.iter().enumerate() {
            record.add_field(&alloc::format!("a{}", i), &arg.to_string());
        }

        self.log.lock().push(record);

        let mut stats = self.stats.lock();
        stats.records_sent += 1;
    }

    /// Dosya erişimini günlükler
    pub fn log_file_access(&self, path: &str, perms: u32, ret: i64) {
        if !self.is_enabled() {
            return;
        }

        // İzlemeleri kontrol et
        for watch in self.watches.lock().iter() {
            if watch.matches(path, perms) {
                let serial = self.next_serial.fetch_add(1, Ordering::SeqCst);
                let mut record = AuditRecord::new(AUDIT_FS_WATCH, serial);
                record.add_field("path", path);
                record.add_field("perms", &perms.to_string());
                record.add_field("key", &watch.key);
                record.ret = ret;

                self.log.lock().push(record);

                let mut stats = self.stats.lock();
                stats.records_sent += 1;
            }
        }
    }

    /// Kural ekler
    pub fn add_rule(&self, mut rule: AuditRule) -> u32 {
        let id = self.next_rule_id.fetch_add(1, Ordering::SeqCst);
        rule.id = id;
        self.rules.lock().push(rule);

        let mut stats = self.stats.lock();
        stats.rules_count += 1;

        id
    }

    /// Kural kaldırır
    pub fn remove_rule(&self, id: u32) {
        self.rules.lock().retain(|r| r.id != id);

        let mut stats = self.stats.lock();
        stats.rules_count = stats.rules_count.saturating_sub(1);
    }

    /// İzleme ekler
    pub fn add_watch(&self, path: &str, perms: u32, key: &str) -> u32 {
        let id = self.next_watch_id.fetch_add(1, Ordering::SeqCst);
        let watch = AuditWatch::new(id, path, perms, key);
        self.watches.lock().push(watch);

        let mut stats = self.stats.lock();
        stats.watches_count += 1;

        id
    }

    /// İzleme kaldırır
    pub fn remove_watch(&self, id: u32) {
        self.watches.lock().retain(|w| w.id != id);

        let mut stats = self.stats.lock();
        stats.watches_count = stats.watches_count.saturating_sub(1);
    }

    /// Kayıtları getirir
    pub fn get_records(&self) -> Vec<AuditRecord> {
        self.log.lock().drain(..).collect()
    }

    /// İstatistikleri getirir
    pub fn get_stats(&self) -> AuditStats {
        self.stats.lock().clone()
    }
}

lazy_static::lazy_static! {
    pub static ref AUDIT: AuditManager = AuditManager::new();
}

// ============================================================================
// SİSTEM ÇAĞRISI ARAYÜZÜ
// ============================================================================

pub fn sys_audit_write(msg_type: u16, data: &[u8]) -> i32 {
    if !AUDIT.is_enabled() {
        return 0;
    }

    let serial = AUDIT.next_serial.fetch_add(1, Ordering::SeqCst);
    let mut record = AuditRecord::new(msg_type, serial);
    record.add_field("data", &core::str::from_utf8(data).unwrap_or(""));

    AUDIT.log.lock().push(record);

    0
}

// ============================================================================
// BAŞLATMA
// ============================================================================

pub fn init() {
    crate::serial_println!("[AUDIT] Subsystem initialized");
}
