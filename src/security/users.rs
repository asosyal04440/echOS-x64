//! # echOS Kullanici Yonetimi
//!
//! UNIX-tarz kullanici ve grup sistemi.
//! /etc/passwd ve /etc/group benzeri yapilar.
//!
//! ## Ozellikler
//! - UID/GID tabanli erisim kontrolu
//! - Root (uid=0) ve normal kullanici ayrimi  
//! - Parola hash dogrulamasi (SHA-256)
//! - Oturum (session) yonetimi
//! - Grup uyeligi

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use lazy_static::lazy_static;
use spin::Mutex;

// ============================================================================
// TIPLER
// ============================================================================

/// Kullanici kimlik numarasi
pub type Uid = u32;
/// Grup kimlik numarasi
pub type Gid = u32;

/// Kullanici kaydi (/etc/passwd satiri)
#[derive(Debug, Clone)]
pub struct UserEntry {
    /// Kullanici adi
    pub username: String,
    /// UID
    pub uid: Uid,
    /// Birincil GID
    pub gid: Gid,
    /// Tam ad / GECOS
    pub gecos: String,
    /// Ev dizini
    pub home: String,
    /// Kabuk yolu
    pub shell: String,
    /// Parola hash (SHA-256 hex)
    pub password_hash: String,
    /// Hesap aktif mi?
    pub enabled: bool,
}

/// Grup kaydi (/etc/group satiri)
#[derive(Debug, Clone)]
pub struct GroupEntry {
    /// Grup adi
    pub name: String,
    /// GID
    pub gid: Gid,
    /// Uye kullanici adlari
    pub members: Vec<String>,
}

/// Aktif oturum
#[derive(Debug, Clone)]
pub struct Session {
    /// Oturum ID'si
    pub session_id: u32,
    /// Giriş yapan UID
    pub uid: Uid,
    /// Aktif GID
    pub gid: Gid,
    /// Kullanıcı adı
    pub username: String,
    /// Oturum başlangıç zamanı (tick)
    pub login_tick: u64,
    /// TTY
    pub tty: String,
}

// ============================================================================
// KULLANICI VERİTABANI
// ============================================================================

/// Kullanıcı ve grup veritabanı yöneticisi
pub struct UserDatabase {
    users: Mutex<BTreeMap<Uid, UserEntry>>,
    groups: Mutex<BTreeMap<Gid, GroupEntry>>,
    sessions: Mutex<BTreeMap<u32, Session>>,
    next_session_id: AtomicU32,
    current_uid: AtomicU32,
}

impl UserDatabase {
    pub const fn new() -> Self {
        Self {
            users: Mutex::new(BTreeMap::new()),
            groups: Mutex::new(BTreeMap::new()),
            sessions: Mutex::new(BTreeMap::new()),
            next_session_id: AtomicU32::new(1),
            current_uid: AtomicU32::new(0), // root
        }
    }

    /// Varsayılan kullanıcıları ve grupları ekle
    pub fn init_defaults(&self) {
        // root kullanıcısı
        self.add_user(UserEntry {
            username: "root".to_string(),
            uid: 0,
            gid: 0,
            gecos: "System Administrator".to_string(),
            home: "/root".to_string(),
            shell: "/bin/sh".to_string(),
            password_hash: String::new(), // bos parola
            enabled: true,
        });

        // sistem kullanıcısı
        self.add_user(UserEntry {
            username: "system".to_string(),
            uid: 1,
            gid: 1,
            gecos: "System Services".to_string(),
            home: "/".to_string(),
            shell: "/bin/nologin".to_string(),
            password_hash: String::new(),
            enabled: false, // login yapilamaz
        });

        // normal kullanıcı
        self.add_user(UserEntry {
            username: "user".to_string(),
            uid: 1000,
            gid: 1000,
            gecos: "Default User".to_string(),
            home: "/home/user".to_string(),
            shell: "/bin/sh".to_string(),
            password_hash: String::new(),
            enabled: true,
        });

        // masaustu operator kullanicisi
        self.add_user(UserEntry {
            username: "operator".to_string(),
            uid: 1001,
            gid: 1000,
            gecos: "Desktop Operator".to_string(),
            home: "/home/operator".to_string(),
            shell: "/bin/sh".to_string(),
            password_hash: "448a12e0417da78f1e6c48413d81954f4a732f95c1ebeed940c778bc34d6ceac"
                .to_string(),
            enabled: true,
        });

        // root grubu
        self.add_group(GroupEntry {
            name: "root".to_string(),
            gid: 0,
            members: alloc::vec!["root".to_string()],
        });

        // system grubu
        self.add_group(GroupEntry {
            name: "system".to_string(),
            gid: 1,
            members: alloc::vec!["system".to_string()],
        });

        // users grubu
        self.add_group(GroupEntry {
            name: "users".to_string(),
            gid: 1000,
            members: alloc::vec!["user".to_string(), "operator".to_string()],
        });

        // wheel grubu (sudo yetkisi)
        self.add_group(GroupEntry {
            name: "wheel".to_string(),
            gid: 10,
            members: alloc::vec![
                "root".to_string(),
                "user".to_string(),
                "operator".to_string()
            ],
        });
    }

    /// Kullanıcı ekle
    pub fn add_user(&self, entry: UserEntry) {
        let uid = entry.uid;
        self.users.lock().insert(uid, entry);
    }

    /// UID ile kullanıcı bul
    pub fn get_user(&self, uid: Uid) -> Option<UserEntry> {
        self.users.lock().get(&uid).cloned()
    }

    /// Kullanıcı adıyla kullanıcı bul
    pub fn get_user_by_name(&self, name: &str) -> Option<UserEntry> {
        self.users
            .lock()
            .values()
            .find(|u| u.username == name)
            .cloned()
    }

    pub fn set_password_hash(
        &self,
        username: &str,
        password_hash: String,
    ) -> Result<(), &'static str> {
        let mut users = self.users.lock();
        let user = users
            .values_mut()
            .find(|u| u.username == username)
            .ok_or("Kullanici bulunamadi")?;
        user.password_hash = password_hash;
        Ok(())
    }

    /// Grup ekle
    pub fn add_group(&self, entry: GroupEntry) {
        let gid = entry.gid;
        self.groups.lock().insert(gid, entry);
    }

    /// GID ile grup bul
    pub fn get_group(&self, gid: Gid) -> Option<GroupEntry> {
        self.groups.lock().get(&gid).cloned()
    }

    /// Kullanıcının tüm gruplarını getir
    pub fn get_user_groups(&self, username: &str) -> Vec<Gid> {
        self.groups
            .lock()
            .iter()
            .filter(|(_, g)| g.members.iter().any(|m| m == username))
            .map(|(gid, _)| *gid)
            .collect()
    }

    /// Oturum aç
    pub fn login(&self, username: &str, _password: &str) -> Result<Session, &'static str> {
        let user = self
            .get_user_by_name(username)
            .ok_or("Kullanici bulunamadi")?;

        if !user.enabled {
            return Err("Hesap devre disi");
        }

        // Parola doğrulama: SHA-256 hash karşılaştırması
        if !user.password_hash.is_empty() {
            let input_hash = crate::net::quic::sha256_hash(_password.as_bytes());
            let input_hex = input_hash
                .iter()
                .map(|b| {
                    let hi = b >> 4;
                    let lo = b & 0x0f;
                    let to_hex = |n: u8| if n < 10 { b'0' + n } else { b'a' + n - 10 };
                    [to_hex(hi) as char, to_hex(lo) as char]
                })
                .flatten()
                .collect::<String>();
            if input_hex != user.password_hash {
                return Err("Yanlis parola");
            }
        }
        // Boş password_hash = parolasız giriş kabul

        let session_id = self.next_session_id.fetch_add(1, Ordering::SeqCst);
        let session = Session {
            session_id,
            uid: user.uid,
            gid: user.gid,
            username: user.username.clone(),
            login_tick: crate::task::scheduler::get_ticks() as u64,
            tty: "tty0".to_string(),
        };

        self.current_uid.store(user.uid, Ordering::SeqCst);
        self.sessions.lock().insert(session_id, session.clone());

        crate::serial_println!(
            "[AUTH] User '{}' logged in (uid={}, session={})",
            user.username,
            user.uid,
            session_id
        );

        Ok(session)
    }

    /// Oturum kapat
    pub fn logout(&self, session_id: u32) -> Result<(), &'static str> {
        let session = self
            .sessions
            .lock()
            .remove(&session_id)
            .ok_or("Oturum bulunamadi")?;

        crate::serial_println!(
            "[AUTH] User '{}' logged out (session={})",
            session.username,
            session_id
        );

        // Aktif oturum yoksa root'a dön
        if self.sessions.lock().is_empty() {
            self.current_uid.store(0, Ordering::SeqCst);
        }

        Ok(())
    }

    /// Mevcut UID'yi getir
    pub fn current_uid(&self) -> Uid {
        self.current_uid.load(Ordering::SeqCst)
    }

    /// Mevcut kullanıcı root mu?
    pub fn is_root(&self) -> bool {
        self.current_uid() == 0
    }

    /// Aktif oturumları listele
    pub fn list_sessions(&self) -> Vec<Session> {
        self.sessions.lock().values().cloned().collect()
    }

    /// Tüm kullanıcıları listele
    pub fn list_users(&self) -> Vec<UserEntry> {
        self.users.lock().values().cloned().collect()
    }
}

lazy_static! {
    /// Global kullanıcı veritabanı
    pub static ref USER_DB: UserDatabase = UserDatabase::new();
}

/// Kullanıcı yönetimini başlat
pub fn init_users() {
    USER_DB.init_defaults();
    crate::serial_println!(
        "[AUTH] User database initialized ({} users, {} groups)",
        USER_DB.list_users().len(),
        USER_DB.groups.lock().len()
    );

    // Root olarak otomatik oturum aç
    let _ = USER_DB.login("root", "");
}
