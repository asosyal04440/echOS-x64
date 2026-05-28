//! # Dosya Kilitleme Uygulaması
//!
//! POSIX dosya kilitleme desteği (flock, fcntl kilitleri).
//! Danışma ve zorunlu dosya kilitlemeyi destekler.
//!
//! ## Kilit Çakışma Matrisi
//!
//! ```text
//!  flock kilit uyumluluğu:
//!  ┌──────────────┬──────────┬──────────┐
//!  │  İstek ▼     │  LOCK_SH │  LOCK_EX │
//!  ├──────────────┼──────────┼──────────┤
//!  │  LOCK_SH var │    ✓     │    ✗     │
//!  │  LOCK_EX var │    ✗     │    ✗     │
//!  └──────────────┴──────────┴──────────┘
//!
//!  fcntl / POSIX kilit uyumluluğu:
//!  ┌──────────────┬──────────┬──────────┐
//!  │  İstek ▼     │  F_RDLCK │  F_WRLCK │
//!  ├──────────────┼──────────┼──────────┤
//!  │  F_RDLCK var │    ✓     │    ✗     │
//!  │  F_WRLCK var │    ✗     │    ✗     │
//!  └──────────────┴──────────┴──────────┘
//!  (Aynı PID'in kilitleri birbirleriyle çakışmaz.)
//!
//!  Zorunlu kilitleme (mandatory locking):
//!  mode & 0o2000 (setgid) != 0  VE  mode & 0o040 (group exec) == 0
//!
//!  fcntl kilit byte aralığı çakışması:
//!  kilit_A: [start_A ... start_A + len_A)
//!  kilit_B: [start_B ... start_B + len_B)
//!  Çakışma: start_A < end_B  &&  start_B < end_A
//!           (len == 0 ise dosya sonuna kadar = u64::MAX)
//! ```

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// DOSYA KİLİT SABİTLERİ
// ============================================================================

/// flock() için kilit türleri
pub const LOCK_SH: i32 = 1; // Paylaşımlı kilit
pub const LOCK_EX: i32 = 2; // Özel kilit
pub const LOCK_UN: i32 = 8; // Kilidi kaldır
pub const LOCK_NB: i32 = 4; // Bloklamayan mod

/// fcntl için kilit türleri (F_SETLK, F_SETLKW, F_GETLK)
pub const F_RDLCK: i32 = 0; // Okuma kilidi
pub const F_WRLCK: i32 = 1; // Yazma kilidi
pub const F_UNLCK: i32 = 2; // Kilidi kaldır

/// fcntl kilit komutları
pub const F_SETLK: i32 = 6; // Kilidi ayarla (bloklamayan)
pub const F_SETLKW: i32 = 7; // Kilidi ayarla (bloklayan)
pub const F_GETLK: i32 = 5; // Kilit bilgisini al
/// OFD (open file description) lock commands (Linux 3.15+)
pub const F_OFD_GETLK: i32 = 36;
pub const F_OFD_SETLK: i32 = 37;
pub const F_OFD_SETLKW: i32 = 38;

// ============================================================================
// DOSYA KİLİT YAPILARI
// ============================================================================

/// Bir dosya kilidi (POSIX fcntl stil / OFD)
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
    /// Kilidi tutan proses ID'si (POSIX) veya fd (OFD)
    pub l_pid: u64,
    /// true = OFD (open file description) lock, false = POSIX lock
    pub is_ofd: bool,
}

impl FileLock {
    pub fn new(l_type: i32, l_whence: i32, l_start: u64, l_len: u64, l_pid: u64) -> Self {
        Self {
            l_type,
            l_whence,
            l_start,
            l_len,
            l_pid,
            is_ofd: false,
        }
    }

    /// Bu kilidin başkasıyla çakışıp çakışmadığını kontrol eder
    pub fn conflicts_with(&self, other: &FileLock) -> bool {
        // man fcntl_locking(2): OFD vs POSIX always conflict (even same process, same fd)
        if self.is_ofd != other.is_ofd {
            // One is OFD, other is POSIX → always conflict
        } else if self.l_pid == other.l_pid {
            // Same lock type (both POSIX or both OFD) AND same owner → compatible
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
    /// Wait-for graph: waiter_pid -> set of holder_pids (deadlock detection)
    wait_for_graph: Mutex<BTreeMap<u64, BTreeSet<u64>>>,
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
            wait_for_graph: Mutex::new(BTreeMap::new()),
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
                existing_locks.retain(|l| l.l_pid != lock.l_pid || !l.overlaps(&lock));
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
        // POSIX kilitlerini serbest bırak (OFD kilitleri hariç)
        {
            let mut locks = self.posix_locks.lock();
            for (_, lock_list) in locks.iter_mut() {
                lock_list.retain(|l| l.is_ofd || l.l_pid != pid);
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

    /// Belirtilen open file description'a ait OFD kilitlerini serbest bırakır
    pub fn release_ofd_locks(&self, fd: u64) {
        let mut locks = self.posix_locks.lock();
        for (_, lock_list) in locks.iter_mut() {
            lock_list.retain(|l| !(l.is_ofd && l.l_pid == fd));
        }
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

    /// F_SETLKW için deadlock tespiti: waiter_pid, holder_pid için bekliyorsa
    /// ve bu bir cycle oluşturuyorsa true döner.
    pub fn detect_deadlock(&self, waiter_pid: u64, inode: u64, lock: &FileLock) -> bool {
        // OFD lock'lar için deadlock detection yok (man fcntl_locking(2))
        if lock.is_ofd {
            return false;
        }
        let locks = self.posix_locks.lock();
        // Çakışan kilitleri tutan PID'leri bul
        let holders: BTreeSet<u64> = locks.get(&inode).map(|existing| {
            existing.iter()
                .filter(|l| lock.conflicts_with(l))
                .map(|l| l.l_pid)
                .collect()
        }).unwrap_or_default();

        drop(locks);

        // DFS: her holder'dan başlayarak wait-for graph'ı traverse et
        let graph = self.wait_for_graph.lock();
        let mut visited = BTreeSet::new();
        let mut stack: Vec<u64> = holders.into_iter().collect();
        while let Some(pid) = stack.pop() {
            if pid == waiter_pid {
                return true; // cycle: waiter kendini bekliyor
            }
            if !visited.insert(pid) {
                continue;
            }
            if let Some(waiting_for) = graph.get(&pid) {
                for &w in waiting_for {
                    stack.push(w);
                }
            }
        }
        false
    }

    /// Wait-for graph'a edge ekler: waiter_pid, holder_pid'leri bekliyor
    pub fn add_wait_edges(&self, waiter_pid: u64, inode: u64, lock: &FileLock) {
        if lock.is_ofd {
            return;
        }
        let locks = self.posix_locks.lock();
        let holders: BTreeSet<u64> = locks.get(&inode).map(|existing| {
            existing.iter()
                .filter(|l| lock.conflicts_with(l))
                .map(|l| l.l_pid)
                .collect()
        }).unwrap_or_default();
        drop(locks);

        let mut graph = self.wait_for_graph.lock();
        graph.insert(waiter_pid, holders);
    }

    /// Wait-for graph'dan edge'leri kaldırır (kilit alındığında veya vazgeçildiğinde)
    pub fn remove_wait_edges(&self, pid: u64) {
        let mut graph = self.wait_for_graph.lock();
        graph.remove(&pid);
        // Diğer processlerin bu PID'i beklemesini de kaldır
        for (_, holders) in graph.iter_mut() {
            holders.remove(&pid);
        }
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
                crate::serial_println!("[FLOCK] fd={} type={} pid={}", fd, lock_type, pid);
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

    let is_ofd = cmd == F_OFD_GETLK || cmd == F_OFD_SETLK || cmd == F_OFD_SETLKW;
    // man fcntl_locking(2): OFD commands require l_pid == 0
    if is_ofd && lock.l_pid != 0 {
        return -22; // EINVAL
    }

    // Try to map fd -> path -> inode (simple path hash). Falls back to fd
    let inode = {
        // Attempt to read path from current process's FD table (per-process)
        let fd_arc = crate::fs::current_fd_table();
        let table = fd_arc.lock();
        if let Some(file) = table.get(fd as usize) {
            // Simple djb2-style hash to produce a stable inode-like value
            let mut hash: u64 = 5381;
            for b in file.path.bytes() {
                hash = hash.wrapping_mul(33).wrapping_add(b as u64);
            }
            hash
        } else {
            fd as u64
        }
    };

    // Set owner: OFD uses fd, POSIX uses PID
    lock.is_ofd = is_ofd;
    if is_ofd {
        lock.l_pid = fd as u64;
    } else {
        lock.l_pid = crate::task::scheduler::current_task_id() as u64;
    }
    // OFD cmd'lerini POSIX eşdeğerlerine normalize et
    let posix_cmd = match cmd {
        F_OFD_GETLK => F_GETLK,
        F_OFD_SETLK => F_SETLK,
        F_OFD_SETLKW => F_SETLKW,
        _ => cmd,
    };
    match posix_cmd {
        F_GETLK => {
            // Çakışan kilidi bul
            if let Some(conflict) = FILE_LOCK_MANAGER.get_conflicting_lock(inode, lock) {
                *lock = conflict;
                // man fcntl_locking(2): OFD conflicting lock → l_pid = -1
                if lock.is_ofd {
                    lock.l_pid = u64::MAX; // -1 as u64
                }
            } else {
                lock.l_type = F_UNLCK;
            }
            0
        }
        F_SETLK => {
            // Bloklamayan kilit ayarla
            match FILE_LOCK_MANAGER.acquire_posix_lock(inode, lock.clone()) {
                Ok(()) => 0,
                Err(FileLockError::Conflict) => -11, // EAGAIN
                Err(_) => -22,                       // EINVAL
            }
        }
        F_SETLKW => {
            // Bloklayan kilit ayarla — önce deadlock kontrolü
            if FILE_LOCK_MANAGER.detect_deadlock(lock.l_pid, inode, lock) {
                return -35; // EDEADLK
            }
            FILE_LOCK_MANAGER.add_wait_edges(lock.l_pid, inode, lock);
            loop {
                match FILE_LOCK_MANAGER.acquire_posix_lock(inode, lock.clone()) {
                    Ok(()) => {
                        FILE_LOCK_MANAGER.remove_wait_edges(lock.l_pid);
                        return 0;
                    }
                    Err(FileLockError::Conflict) => {
                        crate::task::scheduler::schedule();
                        continue;
                    }
                    Err(_) => {
                        FILE_LOCK_MANAGER.remove_wait_edges(lock.l_pid);
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

/// OFD kilitlerini fd bazında serbest bırakır (fd kapatılırken çağrılır)
pub fn release_ofd_locks(fd: u64) {
    FILE_LOCK_MANAGER.release_ofd_locks(fd);
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
