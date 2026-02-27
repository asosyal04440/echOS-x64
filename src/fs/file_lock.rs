//! # Dosya Kilitleme Uygulaması
//!
//! POSIX dosya kilitleme desteği (flock, fcntl kilitleri).
//! Danışma ve zorunlu dosya kilitlemeyi destekler.

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// DOSYA KİLİT SABİTLERİ
// ============================================================================

/// flock() için kilit türleri
pub const LOCK_SH: i32 = 1;      // Paylaşımlı kilit
pub const LOCK_EX: i32 = 2;      // Özel kilit
pub const LOCK_UN: i32 = 8;      // Kilidi kaldır
pub const LOCK_NB: i32 = 4;      // Bloklamayan mod

/// fcntl için kilit türleri (F_SETLK, F_SETLKW, F_GETLK)
pub const F_RDLCK: i32 = 0;      // Okuma kilidi
pub const F_WRLCK: i32 = 1;      // Yazma kilidi
pub const F_UNLCK: i32 = 2;      // Kilidi kaldır

/// fcntl kilit komutları
pub const F_SETLK: i32 = 6;      // Kilidi ayarla (bloklamayan)
pub const F_SETLKW: i32 = 7;     // Kilidi ayarla (bloklayan)
pub const F_GETLK: i32 = 5;      // Kilit bilgisini al

// ============================================================================
// DOSYA KİLİT YAPILARI
// ============================================================================

/// Bir dosya kilidi (POSIX fcntl stil)
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileLock {
    /// Kilit türü (F_RDLCK, F_WRLCK, F_UNLCK)
    pub l_type: i32,
    /// Kilit başlangıcı (SEEK_SET, SEEK_CUR, SEEK_END)
    pub l_whence: i32,
    /// Başlangıcı ofseti
    pub l_start: u64,
    /// Uzunluk (0 = dosya sonuna kadar)
    pub l_len: u64,
    /// Kilidi tutan proses ID'si
    pub l_pid: u64,
}

impl FileLock {
    pub fn new(l_type: i32, l_whence: i32, l_start: u64, l_len: u64, l_pid: u64) -> Self {
        Self {
            l_type,
            l_whence,
            l_start,
            l_len,
            l_pid,
        }
    }

    /// Bu kilidin başkasıyla çakışıp çakışmadığını kontrol eder
    pub fn conflicts_with(&self, other: &FileLock) -> bool {
        // Farklı prosesler
        if self.l_pid == other.l_pid {
            return false;
        }

        // Kilit kaldırma hiçbir şeyle çakışmaz
        if self.l_type == F_UNLCK || other.l_type == F_UNLCK {
            return false;
        }

        // İki okuma kilidi çakışmaz
        if self.l_type == F_RDLCK && other.l_type == F_RDLCK {
            return false;
        }

        // Aralık örtüşmesini kontrol et
        self.overlaps(other)
    }

    /// İki kilit aralığının örtüşüp örtüşmediğini kontrol eder
    pub fn overlaps(&self, other: &FileLock) -> bool {
        let self_end = if self.l_len == 0 {
            u64::MAX
        } else {
            self.l_start.saturating_add(self.l_len)
        };

        let other_end = if other.l_len == 0 {
            u64::MAX
        } else {
            other.l_start.saturating_add(other.l_len)
        };

        self.l_start < other_end && other.l_start < self_end
    }

    /// Verilen ofseti kilidin kapsayıp kapsamadığını kontrol eder
    pub fn contains_offset(&self, offset: u64) -> bool {
        let end = if self.l_len == 0 {
            u64::MAX
        } else {
            self.l_start.saturating_add(self.l_len)
        };
        offset >= self.l_start && offset < end
    }
}

/// flock tarzı kilit (tüm dosya)
#[derive(Clone, Debug)]
pub struct FlockLock {
    /// Dosya tanımlayıcısı
    pub fd: u64,
    /// Kilit türü (LOCK_SH, LOCK_EX)
    pub lock_type: i32,
    /// Proses ID'si
    pub pid: u64,
}

// ============================================================================
// KİLİT YÖNETİCİSİ
// ============================================================================

/// Global dosya kilit yöneticisi
pub struct FileLockManager {
    /// İnode'a göre POSIX kilitleri
    posix_locks: Mutex<BTreeMap<u64, Vec<FileLock>>>,
    /// Dosya tanımlayıcısına göre flock kilitleri
    flock_locks: Mutex<BTreeMap<u64, Vec<FlockLock>>>,
    /// Toplam kilit sayısı
    total_locks: AtomicU64,
    /// Toplam çakışma sayısı
    total_conflicts: AtomicU64,
}

impl FileLockManager {
    pub const fn new() -> Self {
        Self {
            posix_locks: Mutex::new(BTreeMap::new()),
            flock_locks: Mutex::new(BTreeMap::new()),
            total_locks: AtomicU64::new(0),
            total_conflicts: AtomicU64::new(0),
        }
    }

    /// POSIX kilidin alınıp alınamayacağını kontrol eder
    pub fn can_acquire_posix_lock(&self, inode: u64, lock: &FileLock) -> bool {
        let locks = self.posix_locks.lock();
        
        if let Some(existing_locks) = locks.get(&inode) {
            for existing in existing_locks {
                if lock.conflicts_with(existing) {
                    return false;
                }
            }
        }
        
        true
    }

    /// POSIX kilidi alır
    pub fn acquire_posix_lock(&self, inode: u64, lock: FileLock) -> Result<(), FileLockError> {
        let mut locks = self.posix_locks.lock();
        
        // Çakışmaları kontrol et
        if let Some(existing_locks) = locks.get(&inode) {
            for existing in existing_locks {
                if lock.conflicts_with(existing) {
                    self.total_conflicts.fetch_add(1, Ordering::Relaxed);
                    return Err(FileLockError::Conflict);
                }
            }
        }
        
        // Prosesten kalan örtüşen kilidi kaldır
        if lock.l_type == F_UNLCK {
            // Kilidi kaldır: örtüşen kilitleri temizle
            if let Some(existing_locks) = locks.get_mut(&inode) {
                existing_locks.retain(|l| {
                    l.l_pid != lock.l_pid || !l.overlaps(&lock)
                });
            }
        } else {
            // Yeni kilit ekle
            let entry = locks.entry(inode).or_insert_with(Vec::new);
            entry.push(lock);
            self.total_locks.fetch_add(1, Ordering::Relaxed);
        }
        
        Ok(())
    }

    /// Çakışan kilidi getirir (F_GETLK için)
    pub fn get_conflicting_lock(&self, inode: u64, lock: &FileLock) -> Option<FileLock> {
        let locks = self.posix_locks.lock();
        
        if let Some(existing_locks) = locks.get(&inode) {
            for existing in existing_locks {
                if lock.conflicts_with(existing) {
                    return Some(existing.clone());
                }
            }
        }
        
        None
    }

    /// Prosesin tüm kilitlerini serbest bırakır
    pub fn release_all_locks(&self, pid: u64) {
        // POSIX kilitlerini serbest bırak
        {
            let mut locks = self.posix_locks.lock();
            for (_, lock_list) in locks.iter_mut() {
                lock_list.retain(|l| l.l_pid != pid);
            }
        }
        
        // flock kilitlerini serbest bırak
        {
            let mut locks = self.flock_locks.lock();
            for (_, lock_list) in locks.iter_mut() {
                lock_list.retain(|l| l.pid != pid);
            }
        }
        
        crate::serial_println!("[FILELOCK] Released all locks for PID {}", pid);
    }

    /// flock kilidi alır
    pub fn acquire_flock(&self, fd: u64, lock_type: i32, pid: u64) -> Result<(), FileLockError> {
        let mut locks = self.flock_locks.lock();
        
        // Çakışmaları kontrol et
        if let Some(existing_locks) = locks.get(&fd) {
            for existing in existing_locks {
                if existing.pid == pid {
                    continue; // Aynı proses yeniden kilitleyebilir
                }
                
                // Özel kilit her şeyle çakışır
                if lock_type == LOCK_EX || existing.lock_type == LOCK_EX {
                    self.total_conflicts.fetch_add(1, Ordering::Relaxed);
                    return Err(FileLockError::Conflict);
                }
                
                // İki paylaşımlı kilit çakışmaz
            }
        }
        
        // Bu prosesten mevcut kilidi kaldır
        if let Some(existing_locks) = locks.get_mut(&fd) {
            existing_locks.retain(|l| l.pid != pid);
        }
        
        // Yeni kilit ekle (kilit kaldırma değilse)
        if lock_type != LOCK_UN {
            let entry = locks.entry(fd).or_insert_with(Vec::new);
            entry.push(FlockLock { fd, lock_type, pid });
            self.total_locks.fetch_add(1, Ordering::Relaxed);
        }
        
        Ok(())
    }

    /// Dosyanın kilitli olup olmadığını kontrol eder (zorunlu kilitleme için)
    pub fn is_locked(&self, inode: u64, offset: u64, write: bool) -> bool {
        let locks = self.posix_locks.lock();
        
        if let Some(existing_locks) = locks.get(&inode) {
            for lock in existing_locks {
                if lock.contains_offset(offset) {
                    // Yazma kilidi hem okuma hem de yazmayı engeller
                    if lock.l_type == F_WRLCK {
                        return true;
                    }
                    // Okuma kilidi yalnızca yazmayı engeller
                    if lock.l_type == F_RDLCK && write {
                        return true;
                    }
                }
            }
        }
        
        false
    }
}

lazy_static::lazy_static! {
    /// Global dosya kilit yöneticisi
    static ref FILE_LOCK_MANAGER: FileLockManager = FileLockManager::new();
}

// ============================================================================
// HATA TÜRÜ
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileLockError {
    Conflict,
    InvalidLock,
    Deadlock,
    PermissionDenied,
}

// ============================================================================
// SİSTEM ÇAĞRISI ARAYÜZÜ
// ============================================================================

/// flock sistem çağrısı uygulaması
/// 
/// # Argümanlar
/// - `fd`: Dosya tanımlayıcısı
/// - `operation`: Kilit işlemi (LOCK_SH | LOCK_EX | LOCK_UN | LOCK_NB)
/// 
/// # Döndürür
/// Başarıda 0, hata durumunda negatif errno
pub fn sys_flock(fd: i32, operation: i32) -> i32 {
    if fd < 0 {
        return -9; // EBADF
    }
    
    let lock_type = operation & !LOCK_NB;
    let nonblock = (operation & LOCK_NB) != 0;
    
    if lock_type != LOCK_SH && lock_type != LOCK_EX && lock_type != LOCK_UN {
        return -22; // EINVAL
    }
    
    let pid = crate::task::scheduler::current_task_id() as u64;
    
    loop {
        match FILE_LOCK_MANAGER.acquire_flock(fd as u64, lock_type, pid) {
            Ok(()) => {
                crate::serial_println!(
                    "[FLOCK] fd={} type={} pid={}",
                    fd, lock_type, pid
                );
                return 0;
            }
            Err(FileLockError::Conflict) => {
                if nonblock {
                    return -11; // EAGAIN
                }
                // Bekle ve yeniden dene
                // Gerçek uygulamada uyuyup yeniden denenecek
                crate::task::sleep(10);
            }
            Err(_) => {
                return -22; // EINVAL
            }
        }
    }
}

/// fcntl kilit uygulaması (F_SETLK/F_SETLKW/F_GETLK)
/// 
/// # Argümanlar
/// - `fd`: Dosya tanımlayıcısı
/// - `cmd`: Komut (F_SETLK, F_SETLKW, F_GETLK)
/// - `lock`: Kilit yapısı
/// 
/// # Döndürür
/// Başarıda 0, hata durumunda negatif errno
pub fn sys_fcntl_lock(fd: i32, cmd: i32, lock: &mut FileLock) -> i32 {
    if fd < 0 {
        return -9; // EBADF
    }
    
    // Validate lock type
    if lock.l_type != F_RDLCK && lock.l_type != F_WRLCK && lock.l_type != F_UNLCK {
        return -22; // EINVAL
    }
    
    // Get inode from fd (placeholder)
    let inode = fd as u64; // Gerçek uygulamada fd'den inode alınacak
    
    lock.l_pid = crate::task::scheduler::current_task_id() as u64;
    
    match cmd {
        F_GETLK => {
            // Çakışan kilidi bul
            if let Some(conflict) = FILE_LOCK_MANAGER.get_conflicting_lock(inode, lock) {
                *lock = conflict;
            } else {
                lock.l_type = F_UNLCK;
            }
            0
        }
        F_SETLK => {
            // Bloklamayan kilit ayarla
            match FILE_LOCK_MANAGER.acquire_posix_lock(inode, lock.clone()) {
                Ok(()) => {
                    crate::serial_println!(
                        "[FCNTL] SETLK: fd={} type={} start={} len={}",
                        fd, lock.l_type, lock.l_start, lock.l_len
                    );
                    0
                }
                Err(FileLockError::Conflict) => -11, // EAGAIN
                Err(_) => -22, // EINVAL
            }
        }
        F_SETLKW => {
            // Bloklayan kilit ayarla
            loop {
                match FILE_LOCK_MANAGER.acquire_posix_lock(inode, lock.clone()) {
                    Ok(()) => {
                        crate::serial_println!(
                            "[FCNTL] SETLKW: fd={} type={} start={} len={}",
                            fd, lock.l_type, lock.l_start, lock.l_len
                        );
                        return 0;
                    }
                    Err(FileLockError::Conflict) => {
                        // TODO: Kilitlenme (deadlock) kontrolü
                        crate::task::sleep(10);
                    }
                    Err(_) => {
                        return -22; // EINVAL
                    }
                }
            }
        }
        _ => -22, // EINVAL
    }
}

// ============================================================================
// GENEL (PUBLIC) API
// ============================================================================

/// Dosya kilitleme alt sistemini başlatır
pub fn init() {
    crate::serial_println!("[FILELOCK] Alt sistemi başlatıldı");
}

/// Prosesin tüm kilitlerini serbest bırakır (proses sonlandırıldığında çağrılır)
pub fn release_process_locks(pid: u64) {
    FILE_LOCK_MANAGER.release_all_locks(pid);
}

/// Dosya bölgesinin kilitli olup olmadığını kontrol eder (zorunlu kilitleme için)
pub fn check_lock(inode: u64, offset: u64, write: bool) -> bool {
    FILE_LOCK_MANAGER.is_locked(inode, offset, write)
}

/// Kilit istatistikleri
pub struct LockStats {
    pub total_locks: u64,
    pub total_conflicts: u64,
    pub posix_lock_count: usize,
    pub flock_lock_count: usize,
}

/// Kilit istatistiklerini döndürür
pub fn get_stats() -> LockStats {
    LockStats {
        total_locks: FILE_LOCK_MANAGER.total_locks.load(Ordering::Relaxed),
        total_conflicts: FILE_LOCK_MANAGER.total_conflicts.load(Ordering::Relaxed),
        posix_lock_count: FILE_LOCK_MANAGER.posix_locks.lock().len(),
        flock_lock_count: FILE_LOCK_MANAGER.flock_locks.lock().len(),
    }
}

/// Dosya için zorunlu kilitlemenin etkin olup olmadığını kontrol eder
/// (dosya setgid biti ayarlı ama grup çalıştırma devre dışı)
pub fn is_mandatory_locking_enabled(mode: u32) -> bool {
    // Zorunlu kilitleme: setgid biti ayarlı, grup çalıştırma devre dışı
    (mode & 0o2000) != 0 && (mode & 0o040) == 0
}
