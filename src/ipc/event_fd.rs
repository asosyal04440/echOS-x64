//! # Eventfd / Signalfd / Timerfd — Linux IPC Dosya Tanımlayıcıları
//!
//! Bu modül, Linux'un özel dosya tanımlayıcılarını implement eder:
//!
//! - **eventfd**: Süreçler/thread'ler arası olay bildirimi (counter-based)
//! - **signalfd**: Sinyal teslimatını fd üzerinden okuma
//! - **timerfd**: Timer olaylarını fd üzerinden poll/read
//!
//! ## Kullanım Senaryoları
//!
//! ```text
//! eventfd:   epoll/poll ile entegrasyon, thread senkronizasyonu
//! signalfd:  sinyal güvenli (signal-safe) olay döngüsü
//! timerfd:   periyodik görevler, timeout'lar
//! ```

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// Eventfd
// ============================================================================

/// eventfd bayrakları
pub const EFD_CLOEXEC: u32 = 0o2000000;
pub const EFD_NONBLOCK: u32 = 0o4000;
pub const EFD_SEMAPHORE: u32 = 1;

/// Eventfd — 64-bit counter üzerinden olay bildirimi
///
/// write() counter'a ekler, read() counter'ı sıfırlar ve değer döner.
/// EFD_SEMAPHORE modunda read() 1 azaltır.
pub struct EventFd {
    /// 64-bit counter (write ile artır, read ile sıfırla)
    counter: AtomicU64,
    /// Bayraklar (EFD_NONBLOCK, EFD_SEMAPHORE, EFD_CLOEXEC)
    flags: u32,
    /// Dosya tanımlayıcı numarası
    fd: i32,
}

impl EventFd {
    pub fn new(initval: u64, flags: u32, fd: i32) -> Self {
        Self {
            counter: AtomicU64::new(initval),
            flags,
            fd,
        }
    }

    /// Counter > 0 ise değeri okur. Semaphore modunda 1 azaltır.
    pub fn read(&self) -> Result<u64, EventFdError> {
        let val = self.counter.load(Ordering::SeqCst);
        if val == 0 {
            if self.flags & EFD_NONBLOCK != 0 {
                return Err(EventFdError::WouldBlock);
            }
            // Blocking modda bekleme gerekir (stub)
            return Err(EventFdError::WouldBlock);
        }

        if self.flags & EFD_SEMAPHORE != 0 {
            // Semaphore modu: 1 azalt, 1 döndür
            self.counter.fetch_sub(1, Ordering::SeqCst);
            Ok(1)
        } else {
            // Normal mod: counter'ı sıfırla, eski değeri döndür
            let old = self.counter.swap(0, Ordering::SeqCst);
            Ok(old)
        }
    }

    /// Counter'a val ekler. u64::MAX taşarsa hata döner.
    pub fn write(&self, val: u64) -> Result<(), EventFdError> {
        if val == u64::MAX {
            return Err(EventFdError::Overflow);
        }
        let old = self.counter.fetch_add(val, Ordering::SeqCst);
        if old.checked_add(val).is_none() {
            // Taşma — geri al
            self.counter.fetch_sub(val, Ordering::SeqCst);
            return Err(EventFdError::Overflow);
        }
        Ok(())
    }

    /// Counter > 0 ise readable
    pub fn is_readable(&self) -> bool {
        self.counter.load(Ordering::SeqCst) > 0
    }
}

#[derive(Debug, Clone, Copy)]
pub enum EventFdError {
    WouldBlock,
    Overflow,
    InvalidFd,
}

// ============================================================================
// Signalfd
// ============================================================================

/// Signal bilgisi (signalfd_siginfo benzeri)
#[derive(Clone, Debug)]
pub struct SignalfdSiginfo {
    /// Sinyal numarası
    pub signo: u32,
    /// Hata kodu
    pub errno: i32,
    /// Sinyal kodu (SI_USER, SI_KERNEL, vs.)
    pub code: i32,
    /// Gönderen PID
    pub pid: u32,
    /// Gönderen UID
    pub uid: u32,
    /// Sinyal değeri
    pub sival: i64,
}

/// Signalfd — sinyal teslimatını dosya tanımlayıcısı üzerinden okuma
pub struct SignalFd {
    /// Dosya tanımlayıcı
    fd: i32,
    /// Maskelenmiş (dinlenen) sinyal seti
    sigmask: u64,
    /// Bayraklar
    flags: u32,
    /// Bekleyen sinyaller kuyruğu
    pending: Mutex<Vec<SignalfdSiginfo>>,
}

impl SignalFd {
    pub fn new(fd: i32, sigmask: u64, flags: u32) -> Self {
        Self {
            fd,
            sigmask,
            flags,
            pending: Mutex::new(Vec::new()),
        }
    }

    /// Sinyal geldiğinde kuyruğa ekler
    pub fn deliver_signal(&self, signo: u32, pid: u32, uid: u32) {
        // Sinyal maskede mi kontrol et
        if (self.sigmask >> signo) & 1 == 0 {
            return;
        }

        self.pending.lock().push(SignalfdSiginfo {
            signo,
            errno: 0,
            code: 0, // SI_USER
            pid,
            uid,
            sival: 0,
        });
    }

    /// Kuyruktan sinyal okur
    pub fn read(&self) -> Option<SignalfdSiginfo> {
        let mut pending = self.pending.lock();
        if pending.is_empty() {
            None
        } else {
            Some(pending.remove(0))
        }
    }

    /// Bekleyen sinyal var mı?
    pub fn is_readable(&self) -> bool {
        !self.pending.lock().is_empty()
    }

    /// Sinyal maskesini günceller
    pub fn set_sigmask(&mut self, new_mask: u64) {
        self.sigmask = new_mask;
    }
}

// ============================================================================
// Timerfd
// ============================================================================

/// Timer tipleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimerfdClockId {
    /// CLOCK_REALTIME — duvar saati
    Realtime,
    /// CLOCK_MONOTONIC — monoton saat (suspend'den etkilenmez)
    Monotonic,
    /// CLOCK_BOOTTIME — boot'tan beri geçen süre (suspend dahil)
    Boottime,
    /// CLOCK_REALTIME_ALARM — alarm (suspend'den uyandırır)
    RealtimeAlarm,
}

/// Timerfd bayrakları
pub const TFD_CLOEXEC: u32 = 0o2000000;
pub const TFD_NONBLOCK: u32 = 0o4000;
pub const TFD_TIMER_ABSTIME: u32 = 1;

/// İtimerspec — timer başlangıç + periyod
#[derive(Clone, Copy, Debug)]
pub struct ItimerSpec {
    /// İlk tetikleme süresi (nanosaniye)
    pub it_value_ns: u64,
    /// Periyod (0 = tek seferlik)
    pub it_interval_ns: u64,
}

/// Timerfd — zamanlayıcı olaylarını fd üzerinden read/poll
pub struct TimerFd {
    /// Dosya tanımlayıcı
    fd: i32,
    /// Clock tipi
    clock_id: TimerfdClockId,
    /// Bayraklar
    flags: u32,
    /// Timer ayarları
    spec: Mutex<ItimerSpec>,
    /// Armed (aktif) mi?
    armed: AtomicBool,
    /// Kaç kez tetiklendi (read ile sıfırlanır)
    expirations: AtomicU64,
    /// Son tetiklenme zamanı (TSC)
    last_trigger_tsc: AtomicU64,
}

impl TimerFd {
    pub fn new(fd: i32, clock_id: TimerfdClockId, flags: u32) -> Self {
        Self {
            fd,
            clock_id,
            flags,
            spec: Mutex::new(ItimerSpec {
                it_value_ns: 0,
                it_interval_ns: 0,
            }),
            armed: AtomicBool::new(false),
            expirations: AtomicU64::new(0),
            last_trigger_tsc: AtomicU64::new(0),
        }
    }

    /// Timer'ı ayarlar (timerfd_settime)
    pub fn settime(&self, new_spec: ItimerSpec, _flags: u32) -> ItimerSpec {
        let old = {
            let mut spec = self.spec.lock();
            let old = *spec;
            *spec = new_spec;
            old
        };

        if new_spec.it_value_ns > 0 {
            self.armed.store(true, Ordering::SeqCst);
            self.expirations.store(0, Ordering::SeqCst);
            self.last_trigger_tsc
                .store(unsafe { core::arch::x86_64::_rdtsc() }, Ordering::SeqCst);
            crate::serial_println!(
                "[TIMERFD] fd={} armed: value={}ns interval={}ns",
                self.fd,
                new_spec.it_value_ns,
                new_spec.it_interval_ns
            );
        } else {
            self.armed.store(false, Ordering::SeqCst);
            crate::serial_println!("[TIMERFD] fd={} disarmed", self.fd);
        }

        old
    }

    /// Mevcut timer ayarlarını döndürür (timerfd_gettime)
    pub fn gettime(&self) -> ItimerSpec {
        *self.spec.lock()
    }

    /// Timer tetiklenmelerini okur (8 byte u64)
    pub fn read(&self) -> Result<u64, EventFdError> {
        let exp = self.expirations.swap(0, Ordering::SeqCst);
        if exp == 0 {
            if self.flags & TFD_NONBLOCK != 0 {
                return Err(EventFdError::WouldBlock);
            }
            return Err(EventFdError::WouldBlock);
        }
        Ok(exp)
    }

    /// Timer tick — zamanlayıcı kesme handler'ından çağrılır
    pub fn tick(&self) {
        if !self.armed.load(Ordering::SeqCst) {
            return;
        }
        self.expirations.fetch_add(1, Ordering::SeqCst);
    }

    /// Readable mı (expirations > 0)?
    pub fn is_readable(&self) -> bool {
        self.expirations.load(Ordering::SeqCst) > 0
    }
}

// ============================================================================
// Global Registry
// ============================================================================

lazy_static::lazy_static! {
    static ref EVENTFDS: Mutex<BTreeMap<i32, EventFd>> = Mutex::new(BTreeMap::new());
    static ref SIGNALFDS: Mutex<BTreeMap<i32, SignalFd>> = Mutex::new(BTreeMap::new());
    static ref TIMERFDS: Mutex<BTreeMap<i32, TimerFd>> = Mutex::new(BTreeMap::new());
    static ref NEXT_SPECIAL_FD: Mutex<i32> = Mutex::new(1000);
}

fn alloc_fd() -> i32 {
    let mut next = NEXT_SPECIAL_FD.lock();
    let fd = *next;
    *next += 1;
    fd
}

/// eventfd2 syscall
pub fn sys_eventfd2(initval: u32, flags: u32) -> i32 {
    let fd = alloc_fd();
    let efd = EventFd::new(initval as u64, flags, fd);
    EVENTFDS.lock().insert(fd, efd);
    crate::serial_println!(
        "[EVENTFD] created fd={} initval={} flags={:#x}",
        fd,
        initval,
        flags
    );
    fd
}

/// signalfd4 syscall
pub fn sys_signalfd4(fd: i32, sigmask: u64, flags: u32) -> i32 {
    if fd != -1 {
        // Mevcut fd'yi güncelle
        if let Some(sfd) = SIGNALFDS.lock().get_mut(&fd) {
            sfd.set_sigmask(sigmask);
            return fd;
        }
    }

    let new_fd = alloc_fd();
    let sfd = SignalFd::new(new_fd, sigmask, flags);
    SIGNALFDS.lock().insert(new_fd, sfd);
    crate::serial_println!("[SIGNALFD] created fd={} sigmask={:#x}", new_fd, sigmask);
    new_fd
}

/// timerfd_create syscall
pub fn sys_timerfd_create(clockid: i32, flags: u32) -> i32 {
    let clock = match clockid {
        0 => TimerfdClockId::Realtime,
        1 => TimerfdClockId::Monotonic,
        7 => TimerfdClockId::Boottime,
        8 => TimerfdClockId::RealtimeAlarm,
        _ => TimerfdClockId::Monotonic,
    };

    let fd = alloc_fd();
    let tfd = TimerFd::new(fd, clock, flags);
    TIMERFDS.lock().insert(fd, tfd);
    crate::serial_println!("[TIMERFD] created fd={} clock={:?}", fd, clock);
    fd
}

/// Modülü başlatır
pub fn init() {
    crate::serial_println!("[IPC-FD] eventfd/signalfd/timerfd module initialized");
}
