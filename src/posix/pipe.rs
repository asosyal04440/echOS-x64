//! # Boru ve FIFO (Pipe and FIFO)
//!
//! İsimsiz (anonymous) pipe ve isimli pipe (FIFO/named pipe) implementasyonları.
//! Tek yönlü IPC kanalı: bir taraf yazar (write end), diğer taraf okur (read end).

use crate::task::scheduler::WaitQueue;
use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// PIPE SABİTLERİ
// ============================================================================

/// Varsayılan pipe tampon boyutu (64 KB)
/// Linux'ta /proc/sys/fs/pipe-max-size ile değiştirilebilir
pub const PIPE_BUF_SIZE: usize = 65536; // 64KB

/// Dosya erişim bayrakları
/// O_RDONLY: Sadece okuma, O_WRONLY: Sadece yazma, O_RDWR: Okuma/Yazma
pub const O_RDONLY: u32 = 0;
pub const O_WRONLY: u32 = 1;
pub const O_RDWR: u32 = 2;
/// O_NONBLOCK: Engellenmeden hata döndür
pub const O_NONBLOCK: u32 = 0x800;

/// Pipe tampon boyutu sınırları
/// PIPE_MIN_BUF_SIZE: Minimum tampon (1 sayfa = 4 KB)
/// PIPE_MAX_BUF_SIZE: Maximum tampon (1 MB)
pub const PIPE_MIN_BUF_SIZE: usize = 4096;
pub const PIPE_MAX_BUF_SIZE: usize = 1048576; // 1MB

// ============================================================================
// PIPE TAMPONU
// ============================================================================

/// Pipe'ın veri tamponu
/// VecDeque kullanılır çünkü hem baştan okuma hem sondan yazma O(1) verir
/// Okuyucu ve yazıcı sayısı atomik olarak takip edilir
pub struct PipeBuffer {
    /// Dairesel tampon (ring buffer) olarak VecDeque
    buffer: Mutex<VecDeque<u8>>,
    /// Maximum tampon boyutu
    max_size: usize,
    /// Aktif okuyucu sayısı (0 olursa yazıcı SIGPIPE alır)
    readers: AtomicU32,
    /// Aktif yazıcı sayısı (0 olursa okuyucu EOF alır)
    writers: AtomicU32,
    /// Bloke olmayan mod aktif mi
    nonblocking: AtomicBool,
    /// Toplam yazılan bayt sayısı (istatistik)
    bytes_written: AtomicU64,
    /// Toplam okunan bayt sayısı (istatistik)
    bytes_read: AtomicU64,
    /// Veri bekleyen okuyucu sayısı
    waiting_readers: AtomicU32,
    /// Yer bekleyen yazıcı sayısı
    waiting_writers: AtomicU32,
    /// Veri bekleyen okuyucular için WaitQueue
    read_wq: WaitQueue,
    /// Yer bekleyen yazıcılar için WaitQueue
    write_wq: WaitQueue,
}

impl PipeBuffer {
    pub fn new(size: usize) -> Self {
        Self {
            buffer: Mutex::new(VecDeque::with_capacity(size)),
            max_size: size.max(PIPE_MIN_BUF_SIZE).min(PIPE_MAX_BUF_SIZE),
            readers: AtomicU32::new(0),
            writers: AtomicU32::new(0),
            nonblocking: AtomicBool::new(false),
            bytes_written: AtomicU64::new(0),
            bytes_read: AtomicU64::new(0),
            waiting_readers: AtomicU32::new(0),
            waiting_writers: AtomicU32::new(0),
            read_wq: WaitQueue::new(),
            write_wq: WaitQueue::new(),
        }
    }

    /// Okuyucu sayısını artır (dup/fork sonrası çağrılır)
    pub fn add_reader(&self) {
        self.readers.fetch_add(1, Ordering::SeqCst);
    }

    /// Okuyucu sayısını azalt (close() çağrısında)
    pub fn remove_reader(&self) {
        self.readers.fetch_sub(1, Ordering::SeqCst);
    }

    /// Yazıcı sayısını artır
    pub fn add_writer(&self) {
        self.writers.fetch_add(1, Ordering::SeqCst);
    }

    /// Yazıcı sayısını azalt (close() çağrısında)
    pub fn remove_writer(&self) {
        self.writers.fetch_sub(1, Ordering::SeqCst);
    }

    /// Aktif okuyucu sayısını döndür
    pub fn get_readers(&self) -> u32 {
        self.readers.load(Ordering::SeqCst)
    }

    /// Aktif yazıcı sayısını döndür
    pub fn get_writers(&self) -> u32 {
        self.writers.load(Ordering::SeqCst)
    }

    /// Pipe'tan veri oku (read() sistem çağrısına karşılık gelir)
    /// - Tampon boş && yazıcı yok => EOF döndür (0 bayt)
    /// - Tampon boş && yazıcı var && nonblocking => EAGAIN döndür
    /// - Tampon boş && yazıcı var && blocking => WaitQueue'da uyut
    pub fn read(&self, buf: &mut [u8]) -> Result<usize, PipeError> {
        if self.readers.load(Ordering::SeqCst) == 0 {
            return Err(PipeError::NoReader);
        }

        loop {
            {
                let mut buffer = self.buffer.lock();

                if !buffer.is_empty() {
                    let to_read = buf.len().min(buffer.len());
                    for i in 0..to_read {
                        buf[i] = buffer.pop_front().unwrap();
                    }
                    self.bytes_read.fetch_add(to_read as u64, Ordering::SeqCst);
                    drop(buffer);
                    // Yer açıldı — bekleyen yazıcıları uyandır
                    self.write_wq.wake_one();
                    return Ok(to_read);
                }

                if self.writers.load(Ordering::SeqCst) == 0 {
                    return Ok(0); // EOF: tüm yazıcılar kapandı
                }

                if self.nonblocking.load(Ordering::SeqCst) {
                    return Err(PipeError::WouldBlock);
                }
            }
            // Blocking mod: veri gelene kadar WaitQueue'da uyut
            self.waiting_readers.fetch_add(1, Ordering::SeqCst);
            self.read_wq.sleep();
            self.waiting_readers.fetch_sub(1, Ordering::SeqCst);
        }
    }

    /// Pipe'a veri yaz (write() sistem çağrısına karşılık gelir)
    /// - Okuyucu yoksa => BrokenPipe + SIGPIPE
    /// - Tampon doluysa && nonblocking => EAGAIN
    /// - Tampon doluysa && blocking => WaitQueue'da uyut
    pub fn write(&self, buf: &[u8]) -> Result<usize, PipeError> {
        if self.writers.load(Ordering::SeqCst) == 0 {
            return Err(PipeError::NoWriter);
        }

        if self.readers.load(Ordering::SeqCst) == 0 {
            // SIGPIPE gönder — yazıcı process'e bildir
            crate::task::signal::send_signal_to_current(crate::task::signal::Signal::SIGPIPE);
            return Err(PipeError::BrokenPipe);
        }

        loop {
            {
                let mut buffer = self.buffer.lock();
                let available = self.max_size.saturating_sub(buffer.len());

                if available > 0 {
                    let to_write = buf.len().min(available);
                    for i in 0..to_write {
                        buffer.push_back(buf[i]);
                    }
                    self.bytes_written
                        .fetch_add(to_write as u64, Ordering::SeqCst);
                    drop(buffer);
                    // Veri geldi — bekleyen okuyucuları uyandır
                    self.read_wq.wake_one();
                    return Ok(to_write);
                }

                if self.nonblocking.load(Ordering::SeqCst) {
                    return Err(PipeError::WouldBlock);
                }
            }
            // Blocking mod: yer açılana kadar WaitQueue'da uyut
            self.waiting_writers.fetch_add(1, Ordering::SeqCst);
            self.write_wq.sleep();
            self.waiting_writers.fetch_sub(1, Ordering::SeqCst);

            // Uyandıktan sonra reader kapanmış olabilir — tekrar kontrol et
            if self.readers.load(Ordering::SeqCst) == 0 {
                crate::task::signal::send_signal_to_current(crate::task::signal::Signal::SIGPIPE);
                return Err(PipeError::BrokenPipe);
            }
        }
    }

    /// Kalan boş alan miktarını döndür
    pub fn space(&self) -> usize {
        let buffer = self.buffer.lock();
        self.max_size.saturating_sub(buffer.len())
    }

    /// Tamponda bekleyen veri boyutunu döndür
    pub fn len(&self) -> usize {
        self.buffer.lock().len()
    }

    /// Tampon boş mu kontrolü
    pub fn is_empty(&self) -> bool {
        self.buffer.lock().is_empty()
    }

    /// Bloke olmayan modu aç/kapat
    pub fn set_nonblocking(&self, nonblock: bool) {
        self.nonblocking.store(nonblock, Ordering::SeqCst);
    }

    /// Poll olaylarını kontrol et (select/poll/epoll için)
    /// POLLIN=0x001: Okunacak veri var
    /// POLLOUT=0x004: Yazılabilir alan var
    /// POLLHUP=0x010: Yazıcı kapatıldı
    /// POLLERR=0x008: Okuyucu kapatıldı
    pub fn poll(&self, events: u32) -> u32 {
        let mut revents = 0u32;

        // POLLIN: Okunabilir veri var mı?
        if events & 0x001 != 0 && !self.is_empty() {
            revents |= 0x001;
        }

        // POLLOUT: Yazılabilir alan var mı?
        if events & 0x004 != 0 && self.space() > 0 {
            revents |= 0x004;
        }

        // POLLHUP: Tüm yazıcılar kapandı mı?
        if self.writers.load(Ordering::SeqCst) == 0 {
            revents |= 0x010;
        }

        // POLLERR: Tüm okuyucular kapandı mı?
        if self.readers.load(Ordering::SeqCst) == 0 {
            revents |= 0x008;
        }

        revents
    }
}

// ============================================================================
// PIPE (İSİMSİZ BORU)
// ============================================================================

/// İsimsiz pipe: pipe() sistem çağrısıyla oluşturulan tek yönlü iletişim kanalı
/// read_fd: okuma ucu (parent'ın child'dan veri aldığı taraf)
/// write_fd: yazma ucu (child'ın parent'a veri gönderdiği taraf)
pub struct Pipe {
    /// Paylaşılan tampon
    buffer: Arc<PipeBuffer>,
    /// Okuma ucu dosya tanımlayıcısı
    pub read_fd: i32,
    /// Yazma ucu dosya tanımlayıcısı
    pub write_fd: i32,
}

impl Pipe {
    pub fn new(size: usize) -> Self {
        let buffer = Arc::new(PipeBuffer::new(size));
        buffer.add_reader();
        buffer.add_writer();

        Self {
            buffer,
            read_fd: -1,
            write_fd: -1,
        }
    }

    /// Paylaşılan tampona referans döndür (dup/fork için)
    pub fn get_buffer(&self) -> Arc<PipeBuffer> {
        self.buffer.clone()
    }

    /// Okuma ucundan veri al
    pub fn read(&self, buf: &mut [u8]) -> Result<usize, PipeError> {
        self.buffer.read(buf)
    }

    /// Yazma ucuna veri gönder
    pub fn write(&self, buf: &[u8]) -> Result<usize, PipeError> {
        self.buffer.write(buf)
    }

    /// Okuma ucunu kapat (EOF koşulu yaratır)
    pub fn close_read(&self) {
        self.buffer.remove_reader();
    }

    /// Yazma ucunu kapat (yazıcı kapandı bildirimi)
    pub fn close_write(&self) {
        self.buffer.remove_writer();
    }
}

// ============================================================================
// FIFO (İSİMLİ BORU)
// ============================================================================

/// İsimli pipe (FIFO): dosya sisteminde bir ada sahip olan pipe
/// mkfifo() veya mknod() S_IFIFO ile oluşturulur
/// Birden fazla süreç aynı FIFO'yu açabilir
pub struct Fifo {
    /// FIFO'nun dosya sistemi yolu (örn: "/tmp/myfifo")
    pub path: String,
    /// Erişim izinleri (örn: 0o644)
    pub mode: u32,
    /// Paylaşılan veri tamponu
    buffer: Arc<PipeBuffer>,
    /// Okuma için açık mı
    open_read: AtomicBool,
    /// Yazma için açık mı
    open_write: AtomicBool,
}

impl Fifo {
    pub fn new(path: &str, mode: u32) -> Self {
        let buffer = Arc::new(PipeBuffer::new(PIPE_BUF_SIZE));

        Self {
            path: String::from(path),
            mode,
            buffer,
            open_read: AtomicBool::new(false),
            open_write: AtomicBool::new(false),
        }
    }

    /// FIFO'yu okuma modunda aç
    pub fn open_read(&self) -> Result<(), PipeError> {
        self.buffer.add_reader();
        self.open_read.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// FIFO'yu yazma modunda aç
    pub fn open_write(&self) -> Result<(), PipeError> {
        self.buffer.add_writer();
        self.open_write.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// Okuma ucunu kapat
    pub fn close_read(&self) {
        if self.open_read.swap(false, Ordering::SeqCst) {
            self.buffer.remove_reader();
        }
    }

    /// Yazma ucunu kapat
    pub fn close_write(&self) {
        if self.open_write.swap(false, Ordering::SeqCst) {
            self.buffer.remove_writer();
        }
    }

    /// FIFO'dan oku
    pub fn read(&self, buf: &mut [u8]) -> Result<usize, PipeError> {
        self.buffer.read(buf)
    }

    /// FIFO'ya yaz
    pub fn write(&self, buf: &[u8]) -> Result<usize, PipeError> {
        self.buffer.write(buf)
    }

    /// Tampona referans döndür
    pub fn get_buffer(&self) -> Arc<PipeBuffer> {
        self.buffer.clone()
    }
}

// ============================================================================
// PIPE YÖNETİCİSİ
// ============================================================================

/// Hem isimsiz pipe'ları hem FIFO'ları yöneten merkezi yapı
pub struct PipeManager {
    /// İsimli pipe'lar (yol -> FIFO)
    fifos: Mutex<BTreeMap<String, Arc<Fifo>>>,
    /// İsimsiz pipe'lar (ID -> Pipe)
    pipes: Mutex<BTreeMap<u64, Arc<Pipe>>>,
    /// Sonraki pipe ID sayacı
    next_pipe_id: AtomicU64,
    /// İstatistikler
    stats: Mutex<PipeStats>,
}

use alloc::collections::BTreeMap;

/// Pipe istatistikleri
#[derive(Clone, Debug, Default)]
pub struct PipeStats {
    pub pipes_created: u64,
    pub fifos_created: u64,
    pub bytes_read: u64,
    pub bytes_written: u64,
}

impl PipeManager {
    pub fn new() -> Self {
        Self {
            fifos: Mutex::new(BTreeMap::new()),
            pipes: Mutex::new(BTreeMap::new()),
            next_pipe_id: AtomicU64::new(1),
            stats: Mutex::new(PipeStats::default()),
        }
    }

    /// İsimsiz pipe oluştur ve yöneticide kaydet
    pub fn create_pipe(&self, size: usize) -> Arc<Pipe> {
        let id = self.next_pipe_id.fetch_add(1, Ordering::SeqCst);
        let pipe = Arc::new(Pipe::new(size));

        self.pipes.lock().insert(id, pipe.clone());

        let mut stats = self.stats.lock();
        stats.pipes_created += 1;

        pipe
    }

    /// İsimli FIFO oluştur
    pub fn create_fifo(&self, path: &str, mode: u32) -> Result<Arc<Fifo>, PipeError> {
        let mut fifos = self.fifos.lock();

        if fifos.contains_key(path) {
            return Err(PipeError::AlreadyExists);
        }

        let fifo = Arc::new(Fifo::new(path, mode));
        fifos.insert(String::from(path), fifo.clone());

        let mut stats = self.stats.lock();
        stats.fifos_created += 1;

        Ok(fifo)
    }

    /// Yola göre FIFO bul
    pub fn get_fifo(&self, path: &str) -> Option<Arc<Fifo>> {
        self.fifos.lock().get(path).cloned()
    }

    /// FIFO'yu kaldır (unlink)
    pub fn remove_fifo(&self, path: &str) {
        self.fifos.lock().remove(path);
    }

    /// İstatistikleri döndür
    pub fn get_stats(&self) -> PipeStats {
        self.stats.lock().clone()
    }
}

lazy_static::lazy_static! {
    pub static ref PIPE_MANAGER: PipeManager = PipeManager::new();
}

// ============================================================================
// HATA TİPİ
// ============================================================================

/// Pipe hata kodları
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipeError {
    WouldBlock,    // EAGAIN: Bloke olunması gerekiyor, nonblocking modda hata
    BrokenPipe,    // EPIPE: Okuyucu kalmadı, yazma başarısız
    NoReader,      // Okuyucu ucu kapalı
    NoWriter,      // Yazıcı ucu kapalı
    AlreadyExists, // EEXIST: FIFO zaten mevcut
    NotFound,      // ENOENT: FIFO bulunamadı
}

// ============================================================================
// SİSTEM ÇAĞRISI ARAYÜZÜ
// ============================================================================

/// pipe(int pipefd[2]) sistem çağrısı
/// pipefd[0]: okuma ucu, pipefd[1]: yazma ucu
pub fn sys_pipe(fds: &mut [i32; 2]) -> i32 {
    let pipe = PIPE_MANAGER.create_pipe(PIPE_BUF_SIZE);

    // Dosya tanımlayıcıları atanır (gerçek uygulamada fd tablosuna eklenir)
    fds[0] = pipe.read_fd;
    fds[1] = pipe.write_fd;

    0
}

/// mkfifo(const char *pathname, mode_t mode) sistem çağrısı
/// İsimli pipe oluşturur, dosya sisteminde görünür
pub fn sys_mkfifo(path: &str, mode: u32) -> i32 {
    match PIPE_MANAGER.create_fifo(path, mode) {
        Ok(_) => 0,
        Err(PipeError::AlreadyExists) => -17, // -EEXIST
        Err(_) => -5,                         // -EIO
    }
}

// ============================================================================
// BAŞLATMA
// ============================================================================

/// Pipe/FIFO alt sistemini başlat
pub fn init() {
    crate::serial_println!("[PIPE] Pipe/FIFO initialized");
}
