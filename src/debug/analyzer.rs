//! # echOS Debug Analyzer
//!
//! Sistem izleme ve hata ayıklama yardımcı araçları.
//! Şu an için basit loglama fonksiyonları içerir.
//!
//! ## Mimari Özet
//!
//! ```
//! [log!()] --> [log()] --> [LOG_RING (halka tampon)]
//!                    \--> [serial UART çıkışı]
//! ```
//!
//! Halka tampon (ring buffer) sabit bellekle çalışır;
//! dolunca en eski kaydın üstüne yazar.

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use core::sync::atomic::{AtomicU64, Ordering};
use lazy_static::lazy_static;
use spin::Mutex;

/// Log seviyesi: bir mesajın önemini belirtir.
/// Trace en düşük, Error en yüksek önceliğe sahiptir.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

/// Tek bir log kaydını temsil eder.
/// `seq` ile kayıtlar sıralı izlenebilir; `file`/`line`/`module`
/// hangi kaynak satırından üretildiğini gösterir.
#[derive(Clone)]
pub struct LogEntry {
    pub seq: u64,
    pub level: LogLevel,
    pub file: &'static str,
    pub line: u32,
    pub module: &'static str,
    pub message: String,
}

/// Sabit kapasiteli halka tampon (ring buffer) log deposu.
///
/// ## Çalışma Prensibi
///
/// ```
/// Kapasite = 4 örnek:
///
/// [A][B][C][D]   <- tampon dolu, next=0, filled=true
///  ^
///  Bir sonraki push buraya yazar (A'nın üstüne).
///
/// push(E) => [E][B][C][D], next=1
/// push(F) => [E][F][C][D], next=2
/// ```
///
/// `snapshot()` çağrıldığında `next` indeksinden başlayıp
/// kronolojik sırayla tüm kayıtlar döndürülür.
struct LogRing {
    entries: Vec<LogEntry>,
    capacity: usize,
    /// Bir sonraki yazma konumu (halkanın başı)
    next: usize,
    /// Tampon en az bir kez dolup taştı mı?
    filled: bool,
}

impl LogRing {
    /// Belirtilen kapasitede boş bir halka tampon oluşturur.
    fn new(capacity: usize) -> Self {
        Self {
            entries: Vec::with_capacity(capacity),
            capacity,
            next: 0,
            filled: false,
        }
    }

    /// Tampona yeni bir kayıt ekler.
    /// Dolu değilse sonuna, doluysa en eski kaydın üstüne yazar.
    fn push(&mut self, entry: LogEntry) {
        if self.capacity == 0 {
            return;
        }
        if self.entries.len() < self.capacity {
            self.entries.push(entry);
        } else {
            self.entries[self.next] = entry;
            self.next += 1;
            if self.next >= self.capacity {
                self.next = 0;
                self.filled = true;
            }
        }
    }

    /// Tamponu kronolojik sırayla klonlar ve döndürür.
    /// Tampon dolu ise `next` konumundan başlayarak sarma (wrap) yapar.
    fn snapshot(&self) -> Vec<LogEntry> {
        if !self.filled {
            return self.entries.clone();
        }
        let mut out = Vec::with_capacity(self.capacity);
        let start = self.next;
        for i in 0..self.capacity {
            let idx = (start + i) % self.capacity;
            out.push(self.entries[idx].clone());
        }
        out
    }

    /// Tamponu yeni kapasiteye göre yeniden boyutlandırır.
    /// Mevcut kayıtlar korunur; kapasiteden fazlası ise en eskiden başlanarak düşürülür.
    fn resize(&mut self, capacity: usize) {
        let mut snapshot = self.snapshot();
        if capacity == 0 {
            self.entries.clear();
            self.capacity = 0;
            self.next = 0;
            self.filled = false;
            return;
        }
        if snapshot.len() > capacity {
            let skip = snapshot.len() - capacity;
            snapshot = snapshot.split_off(skip);
        }
        self.entries = Vec::with_capacity(capacity);
        self.capacity = capacity;
        self.next = 0;
        self.filled = false;
        for entry in snapshot {
            self.push(entry);
        }
    }
}

/// Global log halkası; 512 girdi kapasiteli, Mutex ile korumalı.
/// `lazy_static!` sayesinde ilk erişimde başlatılır.
lazy_static! {
    static ref LOG_RING: Mutex<LogRing> = Mutex::new(LogRing::new(512));
}

/// Monoton artan log sıra numarası. `Relaxed` sıralama yeterlidir
/// çünkü yalnızca tekil artış önemlidir, bellek senkronizasyonu değil.
static LOG_SEQ: AtomicU64 = AtomicU64::new(0);

/// Log seviyesini insan okunabilir ön ek dizgisine çevirir.
/// Seri portda/loglarda görünen `[INFO]`, `[ERROR]` gibi etiketleri üretir.
fn level_prefix(level: LogLevel) -> &'static str {
    match level {
        LogLevel::Trace => "TRACE",
        LogLevel::Debug => "DEBUG",
        LogLevel::Info => "INFO",
        LogLevel::Warn => "WARN",
        LogLevel::Error => "ERROR",
    }
}

/// Çekirdek log fonksiyonu: mesajı hem halka tampona hem seri porta gönderir.
///
/// ## Akış
/// ```
/// log() çağrısı
///   |
///   +---> seq numarası al (atomic fetch_add)
///   |
///   +---> LOG_RING.lock() -> push(entry)
///   |
///   +---> serial UART üzerinden yazdır
/// ```
pub fn log(
    level: LogLevel,
    args: fmt::Arguments,
    file: &'static str,
    line: u32,
    module: &'static str,
) {
    let seq = LOG_SEQ.fetch_add(1, Ordering::Relaxed);
    let message = alloc::format!("{}", args);
    {
        let mut ring = LOG_RING.lock();
        let entry = LogEntry {
            seq,
            level,
            file,
            line,
            module,
            message: message.clone(),
        };
        ring.push(entry);
    }
    let prefix = level_prefix(level);
    crate::serial::uart::_print_with_meta(
        format_args!("[{} {}] {}", prefix, seq, message),
        file,
        line,
        module,
    );
}

/// Kolaylık fonksiyonu: Trace seviyesinde statik mesaj gönderir.
/// Performans açısından kritik olmayan izleme noktaları için kullanılır.
pub fn trace(msg: &str) {
    log(
        LogLevel::Trace,
        format_args!("{}", msg),
        "debug::analyzer",
        0,
        "debug::analyzer",
    );
}

/// Halka tampondaki tüm kayıtların anlık görüntüsünü (snapshot) döndürür.
/// Döndürülen Vec, çağrı anındaki kronolojik sıradadır.
pub fn snapshot() -> Vec<LogEntry> {
    LOG_RING.lock().snapshot()
}

/// Halka tamponun kapasitesini ayarlar. Mevcut kayıtlar mümkün olduğunca korunur.
pub fn set_capacity(capacity: usize) {
    LOG_RING.lock().resize(capacity);
}

/// Son `max` adet log kaydını seri porta basar.
/// Hata ayıklamada sistem durumunu hızlıca görmek için kullanılır.
pub fn dump_recent(max: usize) {
    let snapshot = LOG_RING.lock().snapshot();
    if snapshot.is_empty() {
        crate::serial_println!("[LOG] boş");
        return;
    }
    let count = max.min(snapshot.len());
    crate::serial_println!("[LOG] başlangıç kayıt_sayısı={}", count);
    for entry in snapshot.into_iter().rev().take(count).rev() {
        crate::serial_println!(
            "[LOG] {} {} {}:{} {}",
            entry.seq,
            level_prefix(entry.level),
            entry.module,
            entry.line,
            entry.message
        );
    }
    crate::serial_println!("[LOG] son");
}

/// Halka tamponu dosya sistemine kalıcı olarak yazar.
/// `path` bir VFS düğümüne işaret etmelidir.
/// Başarıda `true`, herhangi bir yazma hatasında `false` döner.
pub fn flush_to_path(path: &str) -> bool {
    let snapshot = LOG_RING.lock().snapshot();
    if snapshot.is_empty() {
        return true;
    }
    let inode = match crate::fs::vfs_open_inode(path) {
        Ok(value) => value,
        Err(_) => return false,
    };
    let mut offset = 0usize;
    for entry in snapshot {
        let line = alloc::format!(
            "{} {} {}:{} {}\n",
            entry.seq,
            level_prefix(entry.level),
            entry.module,
            entry.line,
            entry.message
        );
        if crate::fs::vfs_write_at(&inode, offset, line.as_bytes()).is_err() {
            return false;
        }
        offset = offset.saturating_add(line.len());
    }
    true
}

/// RAII (Resource Acquisition Is Initialization) tabanlı kapsam izleyici.
///
/// `TraceGuard` oluşturulduğunda "ENTER", düşürüldüğünde "EXIT" logu
/// otomatik atılır. Bu sayede kod bloğunun başını ve sonunu elle
/// loglamak gerekmez; Rust'ın `Drop` mekanizması bunu güvenle yapar.
///
/// ## Kullanım
/// ```rust
/// trace_scope!("my_function"); // makro aracılığıyla kullanılır
/// ```
pub struct TraceGuard {
    label: &'static str,
    file: &'static str,
    line: u32,
    module: &'static str,
}

impl TraceGuard {
    /// Yeni bir kapsam izleyici oluşturur ve ENTER logu atar.
    pub fn new(label: &'static str, file: &'static str, line: u32, module: &'static str) -> Self {
        log(
            LogLevel::Trace,
            format_args!("TRACE ENTER: {}", label),
            file,
            line,
            module,
        );
        Self {
            label,
            file,
            line,
            module,
        }
    }
}

/// `TraceGuard` kapsam dışına çıktığında otomatik EXIT logu atar.
impl Drop for TraceGuard {
    fn drop(&mut self) {
        log(
            LogLevel::Trace,
            format_args!("TRACE EXIT: {}", self.label),
            self.file,
            self.line,
            self.module,
        );
    }
}

/// Makro: Trace seviyesinde biçimlendirilmiş log mesajı gönderir.
/// `file!()`, `line!()`, `module_path!()` derleyici tarafından otomatik doldurulur.
#[macro_export]
macro_rules! trace {
    ($($arg:tt)*) => {
        $crate::debug::analyzer::log(
            $crate::debug::analyzer::LogLevel::Trace,
            format_args!($($arg)*),
            file!(),
            line!(),
            module_path!(),
        );
    };
}

/// Makro: Bulunulan kapsamı RAII ile izler (giriş/çıkış logları atar).
/// `_trace_guard` değişkeni kapsam sonunda `Drop` çalıştırır.
#[macro_export]
macro_rules! trace_scope {
    ($label:expr) => {
        let _trace_guard =
            $crate::debug::analyzer::TraceGuard::new($label, file!(), line!(), module_path!());
    };
}

/// Makro: Debug seviyesinde biçimlendirilmiş log mesajı gönderir.
#[macro_export]
macro_rules! log_debug {
    ($($arg:tt)*) => {
        $crate::debug::analyzer::log(
            $crate::debug::analyzer::LogLevel::Debug,
            format_args!($($arg)*),
            file!(),
            line!(),
            module_path!(),
        );
    };
}

/// Makro: Info seviyesinde biçimlendirilmiş log mesajı gönderir.
#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => {
        $crate::debug::analyzer::log(
            $crate::debug::analyzer::LogLevel::Info,
            format_args!($($arg)*),
            file!(),
            line!(),
            module_path!(),
        );
    };
}

/// Makro: Uyarı (Warn) seviyesinde biçimlendirilmiş log mesajı gönderir.
#[macro_export]
macro_rules! log_warn {
    ($($arg:tt)*) => {
        $crate::debug::analyzer::log(
            $crate::debug::analyzer::LogLevel::Warn,
            format_args!($($arg)*),
            file!(),
            line!(),
            module_path!(),
        );
    };
}

/// Makro: Hata (Error) seviyesinde biçimlendirilmiş log mesajı gönderir.
#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => {
        $crate::debug::analyzer::log(
            $crate::debug::analyzer::LogLevel::Error,
            format_args!($($arg)*),
            file!(),
            line!(),
            module_path!(),
        );
    };
}
