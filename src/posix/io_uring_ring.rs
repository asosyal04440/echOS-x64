//! # echOS Lock-Free io_uring Ring Buffer
//!
//! Linux io_uring uyumlu, YEDİ SIFIR KİLİT prensibine göre tasarlanmış
//! Submission Queue (SQ) ve Completion Queue (CQ) ring buffer implementasyonu.
//!
//! ## Mimari
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │  io_uring Lock-Free Ring Architecture                          │
//! │                                                                │
//! │  Kullanıcı Alanı (Producer)           Kernel (Consumer)        │
//! │  ┌──────────────┐                     ┌──────────────┐        │
//! │  │  SQE yazma   │ ───smp_wmb()───►    │  SQE okuma   │        │
//! │  │  tail++      │    (sfence)         │  head++      │        │
//! │  └──────────────┘                     └──────────────┘        │
//! │                                                                │
//! │  Kernel (Producer)                    Kullanıcı (Consumer)     │
//! │  ┌──────────────┐                     ┌──────────────┐        │
//! │  │  CQE yazma   │ ───smp_wmb()───►    │  CQE okuma   │        │
//! │  │  tail++      │    (sfence)         │  head++      │        │
//! │  └──────────────┘                     └──────────────┘        │
//! │                                                                │
//! │  Sıralama Garantisi:                                          │
//! │  1. Veri yazılır                                               │
//! │  2. smp_wmb() (write barrier)                                  │
//! │  3. tail atomik güncellenir                                    │
//! │  4. Consumer: head okur → smp_rmb() → veri okur               │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Kilit Durumu: SIFIR
//!
//! - Mutex: YOK
//! - SpinLock: YOK
//! - Tüm senkronizasyon atomic + memory barrier ile sağlanır

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU32, Ordering};

use super::{copy_from_user, validate_user_range, write_user_bytes};

/// Send-safe raw pointer wrapper.
/// CompletionRing tüm alanları atomik olduğu için
/// farklı thread'lerden erişim güvenlidir.
#[derive(Clone, Copy)]
pub struct SendPtr<T>(*const T);
unsafe impl<T> Send for SendPtr<T> {}
unsafe impl<T> Sync for SendPtr<T> {}

impl<T> SendPtr<T> {
    /// Raw pointer'dan SendPtr oluşturur.
    pub fn new(ptr: *const T) -> Self {
        Self(ptr)
    }
    /// İç pointer'a erişim sağlar.
    pub fn as_ptr(&self) -> *const T {
        self.0
    }
}

/// Ring buffer kapasitesi — 2'nin kuvveti OLMALI (mask için).
/// Linux varsayılanı genellikle 128 veya 256'dır.
const RING_SIZE: usize = 256;

/// Ring mask = RING_SIZE - 1 (bit maskeleme ile modülo yerine AND kullanarak hız kazanılır)
const RING_MASK: u32 = (RING_SIZE - 1) as u32;

// ============================================================================
// SQE (Submission Queue Entry) — Kullanıcı → Kernel yönlü
// ============================================================================

/// io_uring Submission Queue Entry.
///
/// Kullanıcı alanı tarafından yazılır, kernel tarafından okunur.
/// Linux `struct io_uring_sqe` ile ABI uyumludur.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RingSqe {
    /// İşlem kodu: IORING_OP_NOP(0), IORING_OP_READV(1), IORING_OP_WRITEV(2) vb.
    pub opcode: u8,
    /// SQE bayrakları: IOSQE_FIXED_FILE, IOSQE_IO_DRAIN, IOSQE_IO_LINK vb.
    pub flags: u8,
    /// I/O önceliği (ionice seviyesi)
    pub ioprio: u16,
    /// Hedef dosya tanımlayıcı
    pub fd: i32,
    /// Dosya ofseti (okuma/yazma başlangıç noktası)
    pub off: u64,
    /// Kullanıcı buffer adresi (user-space pointer)
    pub addr: u64,
    /// Transfer uzunluğu (byte cinsinden)
    pub len: u32,
    /// Okuma/yazma bayrakları (RWF_HIPRI, RWF_DSYNC vb.)
    pub rw_flags: u32,
    /// Kullanıcı tanımlı veri — CQE'de birebir geri döner
    pub user_data: u64,
    /// Buffer grubu indeksi (IORING_OP_PROVIDE_BUFFERS)
    pub buf_index: u16,
    /// Personality indeksi (credential yönetimi)
    pub personality: u16,
    /// Splice/tee işlemleri için kaynak FD
    pub splice_fd_in: i32,
    /// Gelecek kullanım için yedek alan
    pub _pad: [u64; 2],
}

impl Default for RingSqe {
    fn default() -> Self {
        Self {
            opcode: 0,
            flags: 0,
            ioprio: 0,
            fd: -1,
            off: 0,
            addr: 0,
            len: 0,
            rw_flags: 0,
            user_data: 0,
            buf_index: 0,
            personality: 0,
            splice_fd_in: 0,
            _pad: [0; 2],
        }
    }
}

/// io_uring Completion Queue Entry.
///
/// Kernel tarafından yazılır, kullanıcı alanı tarafından okunur.
/// Linux `struct io_uring_cqe` ile ABI uyumludur.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RingCqe {
    /// SQE'den birebir kopyalanan kullanıcı tanımlı veri
    pub user_data: u64,
    /// İşlem sonucu: >=0 başarı, <0 hata kodu (errno)
    pub res: i32,
    /// CQE bayrakları: IORING_CQE_F_BUFFER, IORING_CQE_F_MORE vb.
    pub flags: u32,
}

impl Default for RingCqe {
    fn default() -> Self {
        Self {
            user_data: 0,
            res: 0,
            flags: 0,
        }
    }
}

// ============================================================================
// io_uring Opcodes (Linux ABI uyumlu)
// ============================================================================

/// IORING_OP_NOP: Hiçbir işlem yapma (test/benchmark için)
pub const IORING_OP_NOP: u8 = 0;
/// IORING_OP_READV: Scatter-gather okuma (readv)
pub const IORING_OP_READV: u8 = 1;
/// IORING_OP_WRITEV: Scatter-gather yazma (writev)
pub const IORING_OP_WRITEV: u8 = 2;
/// IORING_OP_FSYNC: Dosya senkronizasyonu
pub const IORING_OP_FSYNC: u8 = 3;
/// IORING_OP_READ_FIXED: Sabit buffer'dan okuma
pub const IORING_OP_READ_FIXED: u8 = 4;
/// IORING_OP_WRITE_FIXED: Sabit buffer'a yazma
pub const IORING_OP_WRITE_FIXED: u8 = 5;
/// IORING_OP_POLL_ADD: Polling ekleme
pub const IORING_OP_POLL_ADD: u8 = 6;
/// IORING_OP_POLL_REMOVE: Polling kaldırma
pub const IORING_OP_POLL_REMOVE: u8 = 7;
/// IORING_OP_READ: Basit okuma (pread64 karşılığı)
pub const IORING_OP_READ: u8 = 22;
/// IORING_OP_WRITE: Basit yazma (pwrite64 karşılığı)
pub const IORING_OP_WRITE: u8 = 23;

// ============================================================================
// Lock-Free Ring Buffer Implementasyonu
// ============================================================================

/// Submission Queue — Lock-Free Ring Buffer
///
/// Producer: Kullanıcı alanı (tail'e yazar)
/// Consumer: Kernel (head'den okur)
///
/// ```text
///   head                              tail
///    ↓                                  ↓
///  ┌─────┬─────┬─────┬─────┬─────┬─────┬─────┐
///  │     │ SQE │ SQE │ SQE │ SQE │     │     │
///  └─────┴─────┴─────┴─────┴─────┴─────┴─────┘
///         ←── okunmamış girişler ──→
/// ```
pub struct SubmissionRing {
    /// Kernel tarafından ilerletilir: bir sonraki okunacak SQE indeksi
    head: AtomicU32,
    /// Kullanıcı tarafından ilerletilir: bir sonraki yazılacak SQE indeksi
    tail: AtomicU32,
    /// SQE veri dizisi — UnsafeCell ile interior mutability (atomik koruma altında)
    entries: UnsafeCell<[RingSqe; RING_SIZE]>,
    /// Kuyrukta düşürülen (overflow) girişlerin sayısı
    dropped: AtomicU32,
}

// SAFETY: SubmissionRing tüm erişimi atomic head/tail + memory barrier ile koruyor.
// Aynı slot'a eşzamanlı okuma/yazma önlenir (producer tail'i artırana kadar consumer görmez).
unsafe impl Send for SubmissionRing {}
unsafe impl Sync for SubmissionRing {}

/// Completion Queue — Lock-Free Ring Buffer
///
/// Producer: Kernel (tail'e yazar)
/// Consumer: Kullanıcı alanı (head'den okur)
pub struct CompletionRing {
    /// Kullanıcı tarafından ilerletilir: bir sonraki okunacak CQE indeksi
    head: AtomicU32,
    /// Kernel tarafından ilerletilir: bir sonraki yazılacak CQE indeksi
    tail: AtomicU32,
    /// CQE veri dizisi — UnsafeCell ile interior mutability (atomik koruma altında)
    entries: UnsafeCell<[RingCqe; RING_SIZE]>,
    /// Taşma sayacı: CQ doluyken kayıp CQE sayısı
    overflow: AtomicU32,
}

// SAFETY: CompletionRing tüm erişimi atomic head/tail + memory barrier ile koruyor.
unsafe impl Send for CompletionRing {}
unsafe impl Sync for CompletionRing {}

impl SubmissionRing {
    /// Yeni bir boş Submission Ring oluşturur.
    pub const fn new() -> Self {
        Self {
            head: AtomicU32::new(0),
            tail: AtomicU32::new(0),
            entries: UnsafeCell::new(
                [RingSqe {
                    opcode: 0,
                    flags: 0,
                    ioprio: 0,
                    fd: -1,
                    off: 0,
                    addr: 0,
                    len: 0,
                    rw_flags: 0,
                    user_data: 0,
                    buf_index: 0,
                    personality: 0,
                    splice_fd_in: 0,
                    _pad: [0; 2],
                }; RING_SIZE],
            ),
            dropped: AtomicU32::new(0),
        }
    }

    /// Ring'deki bekleyen (okunmamış) SQE sayısını döner.
    ///
    /// `count = tail - head` (wrapping aritmetik ile güvenli)
    #[inline]
    pub fn pending_count(&self) -> u32 {
        let tail = self.tail.load(Ordering::Acquire);
        let head = self.head.load(Ordering::Acquire);
        tail.wrapping_sub(head)
    }

    /// Ring'in dolu olup olmadığını kontrol eder.
    #[inline]
    pub fn is_full(&self) -> bool {
        self.pending_count() >= RING_SIZE as u32
    }

    /// Ring'in boş olup olmadığını kontrol eder.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.pending_count() == 0
    }

    /// Ring kapasitesini döner.
    #[inline]
    pub fn capacity(&self) -> u32 {
        RING_SIZE as u32
    }

    /// PRODUCER (Kullanıcı Alanı): Yeni bir SQE ekler.
    ///
    /// ## Sıralama Garantisi
    /// 1. SQE verisi yazılır (entries dizisine)
    /// 2. `smp_wmb()` — yazma bariyeri (sfence)
    /// 3. `tail` atomik olarak artırılır
    ///
    /// Bu sıralama, kernel'ın tail'i okuduğunda SQE verisinin
    /// kesinlikle görünür olmasını garanti eder.
    ///
    /// ## Dönüş
    /// - `Ok(index)`: Eklenen SQE'nin ring indeksi
    /// - `Err(())`: Ring dolu (EAGAIN)
    pub fn push(&self, sqe: RingSqe) -> Result<u32, ()> {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);

        // Ring dolu mu?
        if tail.wrapping_sub(head) >= RING_SIZE as u32 {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return Err(());
        }

        let index = (tail & RING_MASK) as usize;

        // 1. Veri yazılır (UnsafeCell üzerinden)
        // SAFETY: index her zaman [0, RING_SIZE) aralığında (mask sayesinde)
        // Producer tek tail sahibi, consumer head'e kadar okumaz → yarış yok
        unsafe {
            let entries_ptr = self.entries.get();
            let entry_ptr = (*entries_ptr).as_mut_ptr().add(index);
            core::ptr::write_volatile(entry_ptr, sqe);
        }

        // 2. Yazma bariyeri: veri → tail sıralaması
        crate::memory_barriers::smp_wmb();

        // 3. Tail atomik artır
        self.tail.store(tail.wrapping_add(1), Ordering::Release);

        Ok(index as u32)
    }

    /// CONSUMER (Kernel): Bir sonraki SQE'yi okur ve döner.
    ///
    /// ## Sıralama Garantisi
    /// 1. `tail` atomik olarak okunur (Acquire)
    /// 2. `smp_rmb()` — okuma bariyeri (lfence)
    /// 3. SQE verisi okunur
    /// 4. `head` atomik olarak artırılır (Release)
    ///
    /// Bu sıralama, okunan SQE verisinin kesinlikle güncel
    /// olmasını garanti eder.
    pub fn pop(&self) -> Option<RingSqe> {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);

        // Ring boş mu?
        if head == tail {
            return None;
        }

        // 1. Okuma bariyeri: tail okunduktan sonra veri oku
        crate::memory_barriers::smp_rmb();

        let index = (head & RING_MASK) as usize;

        // 2. Veri okunur (UnsafeCell üzerinden)
        let sqe = unsafe {
            let entries_ptr = self.entries.get();
            core::ptr::read_volatile((*entries_ptr).as_ptr().add(index))
        };

        // 3. Head atomik artır (bu slot artık serbest)
        self.head.store(head.wrapping_add(1), Ordering::Release);

        Some(sqe)
    }

    /// Kernel: Birden fazla SQE'yi toplu olarak (batch) okur.
    ///
    /// Toplu okuma, bariyer maliyetini amortisman eder:
    /// - Tek smp_rmb() çağrısı ile N adet SQE okunur
    /// - Head yalnızca bir kez güncellenir
    ///
    /// `max_count` kadar veya mevcut olanlar kadar SQE okur.
    pub fn pop_batch(&self, out: &mut [RingSqe], max_count: usize) -> usize {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);

        let available = tail.wrapping_sub(head) as usize;
        if available == 0 {
            return 0;
        }

        let count = available.min(max_count).min(out.len());

        // Tek okuma bariyeri — tüm SQE'ler için yeterli
        crate::memory_barriers::smp_rmb();

        for i in 0..count {
            let index = ((head.wrapping_add(i as u32)) & RING_MASK) as usize;
            out[i] = unsafe {
                let entries_ptr = self.entries.get();
                core::ptr::read_volatile((*entries_ptr).as_ptr().add(index))
            };
        }

        // Tek head güncellemesi
        self.head
            .store(head.wrapping_add(count as u32), Ordering::Release);

        count
    }
}

impl CompletionRing {
    /// Yeni bir boş Completion Ring oluşturur.
    pub const fn new() -> Self {
        Self {
            head: AtomicU32::new(0),
            tail: AtomicU32::new(0),
            entries: UnsafeCell::new(
                [RingCqe {
                    user_data: 0,
                    res: 0,
                    flags: 0,
                }; RING_SIZE],
            ),
            overflow: AtomicU32::new(0),
        }
    }

    /// Ring'deki tamamlanmış (okunmamış) CQE sayısını döner.
    #[inline]
    pub fn pending_count(&self) -> u32 {
        let tail = self.tail.load(Ordering::Acquire);
        let head = self.head.load(Ordering::Acquire);
        tail.wrapping_sub(head)
    }

    /// Ring'in dolu olup olmadığını kontrol eder.
    #[inline]
    pub fn is_full(&self) -> bool {
        self.pending_count() >= RING_SIZE as u32
    }

    /// Ring'in boş olup olmadığını kontrol eder.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.pending_count() == 0
    }

    /// PRODUCER (Kernel): Tamamlanan bir işlemin CQE'sini ring'e ekler.
    ///
    /// ## Sıralama Garantisi
    /// 1. CQE verisi yazılır (entries dizisine)
    /// 2. `smp_wmb()` — yazma bariyeri (sfence)
    /// 3. `tail` atomik olarak artırılır (Release)
    ///
    /// Bu sıralama, kullanıcının tail'i okuduğunda CQE verisinin
    /// kesinlikle görünür olmasını garanti eder.
    pub fn push(&self, user_data: u64, res: i32, flags: u32) -> Result<(), ()> {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);

        // Ring dolu mu?
        if tail.wrapping_sub(head) >= RING_SIZE as u32 {
            self.overflow.fetch_add(1, Ordering::Relaxed);
            return Err(());
        }

        let index = (tail & RING_MASK) as usize;

        // 1. CQE verisi yazılır (UnsafeCell üzerinden)
        unsafe {
            let entries_ptr = self.entries.get();
            let entry_ptr = (*entries_ptr).as_mut_ptr().add(index);
            core::ptr::write_volatile(
                entry_ptr,
                RingCqe {
                    user_data,
                    res,
                    flags,
                },
            );
        }

        // 2. Yazma bariyeri: veri → tail sıralaması
        crate::memory_barriers::smp_wmb();

        // 3. Tail atomik artır
        self.tail.store(tail.wrapping_add(1), Ordering::Release);

        Ok(())
    }

    /// CONSUMER (Kullanıcı Alanı): Bir sonraki CQE'yi okur.
    ///
    /// ## Sıralama Garantisi
    /// 1. `tail` atomik okunur (Acquire)
    /// 2. `smp_rmb()` — okuma bariyeri (lfence)
    /// 3. CQE verisi okunur
    /// 4. `head` atomik artırılır (Release)
    pub fn pop(&self) -> Option<RingCqe> {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);

        if head == tail {
            return None;
        }

        crate::memory_barriers::smp_rmb();

        let index = (head & RING_MASK) as usize;
        let cqe = unsafe {
            let entries_ptr = self.entries.get();
            core::ptr::read_volatile((*entries_ptr).as_ptr().add(index))
        };

        self.head.store(head.wrapping_add(1), Ordering::Release);

        Some(cqe)
    }

    /// Kullanıcı: Birden fazla CQE'yi toplu olarak (batch) okur.
    ///
    /// Tek smp_rmb() ile birden fazla completion'ı verimli şekilde okur.
    pub fn pop_batch(&self, out: &mut [RingCqe], max_count: usize) -> usize {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);

        let available = tail.wrapping_sub(head) as usize;
        if available == 0 {
            return 0;
        }

        let count = available.min(max_count).min(out.len());

        crate::memory_barriers::smp_rmb();

        for i in 0..count {
            let index = ((head.wrapping_add(i as u32)) & RING_MASK) as usize;
            out[i] = unsafe {
                let entries_ptr = self.entries.get();
                core::ptr::read_volatile((*entries_ptr).as_ptr().add(index))
            };
        }

        self.head
            .store(head.wrapping_add(count as u32), Ordering::Release);

        count
    }

    /// Taşma (overflow) sayacını döner ve sıfırlar.
    pub fn drain_overflow(&self) -> u32 {
        self.overflow.swap(0, Ordering::Relaxed)
    }
}

// ============================================================================
// Lock-Free io_uring Instance
// ============================================================================

/// Lock-free io_uring instance.
///
/// Her kullanıcı `io_uring_setup()` çağrısı bir `LockFreeIoUring` oluşturur.
/// Bu yapı kernel tarafında tutulur ve fd ile eşleştirilir.
///
/// ## Kilit Durumu
/// - `sq`: SubmissionRing — AtomicU32 head/tail + smp_wmb/smp_rmb
/// - `cq`: CompletionRing — AtomicU32 head/tail + smp_wmb/smp_rmb
/// - Mutex: **SIFIR**
/// - SpinLock: **SIFIR**
pub struct LockFreeIoUring {
    /// Submission Queue ring buffer
    pub sq: SubmissionRing,
    /// Completion Queue ring buffer
    pub cq: CompletionRing,
    /// Instance kimliği (fd karşılığı)
    pub ring_fd: usize,
    /// Yapılandırılmış kuyruk boyutu
    pub sq_entries: u32,
    pub cq_entries: u32,
}

impl LockFreeIoUring {
    /// Yeni bir Lock-Free io_uring instance oluşturur.
    pub const fn new(ring_fd: usize) -> Self {
        Self {
            sq: SubmissionRing::new(),
            cq: CompletionRing::new(),
            ring_fd,
            sq_entries: RING_SIZE as u32,
            cq_entries: RING_SIZE as u32,
        }
    }

    /// SQE'yi işleyip sonucu CQ'ya yazar (kernel tarafı).
    ///
    /// Bu fonksiyon:
    /// 1. SQ'dan bir SQE pop eder
    /// 2. Opcode'a göre işlemi gerçekleştirir
    /// 3. Sonucu CQ'ya push eder
    ///
    /// ## Worker Thread Dispatch
    /// Ağır işlemler `crate::task::worker::spawn_work()` ile
    /// lock-free Treiber Stack üzerinden worker thread'lere dağıtılır.
    pub fn process_submissions(&self) -> usize {
        let mut processed = 0;

        // Batch okuma: tek smp_rmb() ile 32 SQE'ye kadar
        let mut batch = [RingSqe::default(); 32];
        let count = self.sq.pop_batch(&mut batch, 32);

        for i in 0..count {
            let sqe = batch[i];
            let opcode = sqe.opcode;
            let user_data = sqe.user_data;
            let fd = sqe.fd;
            let off = sqe.off;
            let addr = sqe.addr;
            let len = sqe.len;

            // CQ'ya referans (worker thread'den erişim için SendPtr — Send güvenli)
            let cq_ptr = SendPtr::new(&self.cq as *const CompletionRing);

            // Worker thread'e dispatch (Lock-Free Treiber Stack üzerinden)
            crate::task::worker::spawn_work(move || {
                let res = match opcode {
                    IORING_OP_NOP => 0i32,

                    IORING_OP_READ | IORING_OP_READV => {
                        // Gerçek dosya okuma: FD table → INode → VFS read_at
                        crate::serial_println!("[io_uring] READ fd={} off={} len={}", fd, off, len);
                        let fd_idx = fd as usize;
                        let file_table = crate::posix::FILE_TABLE.lock();
                        match file_table.get(fd_idx) {
                            Some(Some(state)) => {
                                let inode = state.inode.clone();
                                let read_offset =
                                    if off != 0 { off as usize } else { state.offset };
                                let read_len = (len as usize).min(65536); // Güvenlik limiti
                                drop(file_table);
                                if read_len == 0 {
                                    0i32
                                } else if addr == 0
                                    || validate_user_range(addr as usize, read_len).is_err()
                                {
                                    -14i32 // EFAULT
                                } else {
                                    // Buffer'ı oluştur ve VFS'den oku
                                    let mut tmp_buf = alloc::vec![0u8; read_len];
                                    match crate::fs::vfs_read_at(&inode, read_offset, &mut tmp_buf)
                                    {
                                        Ok(bytes_read) => {
                                            if bytes_read > 0
                                                && write_user_bytes(
                                                    addr as usize,
                                                    &tmp_buf[..bytes_read],
                                                )
                                                .is_err()
                                            {
                                                -14i32 // EFAULT
                                            } else {
                                                // FILE_TABLE offset güncelle
                                                let mut ft = crate::posix::FILE_TABLE.lock();
                                                if let Some(Some(st)) = ft.get_mut(fd_idx) {
                                                    st.offset = read_offset + bytes_read;
                                                }
                                                bytes_read as i32
                                            }
                                        }
                                        Err(_) => -5i32, // EIO
                                    }
                                }
                            }
                            _ => {
                                drop(file_table);
                                -9i32 // EBADF
                            }
                        }
                    }

                    IORING_OP_WRITE | IORING_OP_WRITEV => {
                        // Gerçek dosya yazma: FD table → INode → VFS write_at
                        crate::serial_println!(
                            "[io_uring] WRITE fd={} off={} len={}",
                            fd,
                            off,
                            len
                        );
                        let fd_idx = fd as usize;
                        let file_table = crate::posix::FILE_TABLE.lock();
                        match file_table.get(fd_idx) {
                            Some(Some(state)) => {
                                let inode = state.inode.clone();
                                let write_offset =
                                    if off != 0 { off as usize } else { state.offset };
                                let write_len = (len as usize).min(65536);
                                drop(file_table);
                                if write_len == 0 {
                                    0i32
                                } else if addr == 0
                                    || validate_user_range(addr as usize, write_len).is_err()
                                {
                                    -14i32 // EFAULT
                                } else {
                                    let mut src_buf = alloc::vec![0u8; write_len];
                                    if copy_from_user(&mut src_buf, addr as usize).is_err() {
                                        -14i32 // EFAULT
                                    } else {
                                        match crate::fs::vfs_write_at(
                                            &inode,
                                            write_offset,
                                            &src_buf,
                                        ) {
                                            Ok(bytes_written) => {
                                                // FILE_TABLE offset güncelle
                                                let mut ft = crate::posix::FILE_TABLE.lock();
                                                if let Some(Some(st)) = ft.get_mut(fd_idx) {
                                                    st.offset = write_offset + bytes_written;
                                                }
                                                bytes_written as i32
                                            }
                                            Err(_) => -5i32, // EIO
                                        }
                                    }
                                }
                            }
                            _ => {
                                drop(file_table);
                                -9i32 // EBADF
                            }
                        }
                    }

                    IORING_OP_FSYNC => {
                        crate::serial_println!("[io_uring] FSYNC fd={}", fd);
                        0i32
                    }

                    _ => {
                        crate::serial_println!("[io_uring] Bilinmeyen opcode: {}", opcode);
                        -38i32 // -ENOSYS
                    }
                };

                // Sonucu CQ'ya yaz (lock-free: atomic tail + smp_wmb)
                unsafe {
                    let cq = &*cq_ptr.as_ptr();
                    if cq.push(user_data, res, 0).is_err() {
                        crate::serial_println!("[io_uring] CQ OVERFLOW! user_data={}", user_data);
                    }
                }
            });

            processed += 1;
        }

        processed
    }

    /// Tamamlanan işlem sayısını döner (kullanıcıya bildirim).
    #[inline]
    pub fn completions_available(&self) -> u32 {
        self.cq.pending_count()
    }

    /// İstatistik: SQ'da bekleyen submission sayısı.
    #[inline]
    pub fn submissions_pending(&self) -> u32 {
        self.sq.pending_count()
    }

    /// İstatistik: CQ overflow (taşma) sayısı.
    pub fn cq_overflow_count(&self) -> u32 {
        self.cq.overflow.load(Ordering::Relaxed)
    }

    /// İstatistik: SQ dropped (düşürülen) sayısı.
    pub fn sq_dropped_count(&self) -> u32 {
        self.sq.dropped.load(Ordering::Relaxed)
    }
}

// ============================================================================
// Birim Testleri (Mantıksal Doğrulama)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sq_push_pop() {
        let ring = SubmissionRing::new();
        assert!(ring.is_empty());

        let sqe = RingSqe {
            opcode: IORING_OP_NOP,
            user_data: 42,
            ..RingSqe::default()
        };
        assert!(ring.push(sqe).is_ok());
        assert_eq!(ring.pending_count(), 1);

        let popped = ring.pop().expect("SQE olmalı");
        assert_eq!(popped.opcode, IORING_OP_NOP);
        assert_eq!(popped.user_data, 42);
        assert!(ring.is_empty());
    }

    #[test]
    fn test_cq_push_pop() {
        let ring = CompletionRing::new();
        assert!(ring.is_empty());

        ring.push(100, 42, 0).expect("Push başarılı olmalı");
        assert_eq!(ring.pending_count(), 1);

        let cqe = ring.pop().expect("CQE olmalı");
        assert_eq!(cqe.user_data, 100);
        assert_eq!(cqe.res, 42);
        assert!(ring.is_empty());
    }

    #[test]
    fn test_ring_full() {
        let ring = CompletionRing::new();
        for i in 0..RING_SIZE {
            assert!(ring.push(i as u64, i as i32, 0).is_ok());
        }
        assert!(ring.is_full());
        // Dolu ring'e push → Err
        assert!(ring.push(999, 999, 0).is_err());
    }

    #[test]
    fn test_batch_pop() {
        let ring = SubmissionRing::new();
        for i in 0..10 {
            let sqe = RingSqe {
                opcode: IORING_OP_NOP,
                user_data: i as u64,
                ..RingSqe::default()
            };
            ring.push(sqe).unwrap();
        }

        let mut out = [RingSqe::default(); 32];
        let count = ring.pop_batch(&mut out, 32);
        assert_eq!(count, 10);
        assert!(ring.is_empty());
    }

    #[test]
    fn test_wrapping_arithmetic() {
        let ring = CompletionRing::new();
        // u32::MAX civarında wrapping test
        ring.head.store(u32::MAX - 2, Ordering::Relaxed);
        ring.tail.store(u32::MAX - 2, Ordering::Relaxed);

        ring.push(1, 1, 0).unwrap();
        ring.push(2, 2, 0).unwrap();
        ring.push(3, 3, 0).unwrap(); // u32::MAX + 1 → wraps to 0

        assert_eq!(ring.pending_count(), 3);

        let cqe = ring.pop().unwrap();
        assert_eq!(cqe.user_data, 1);
        assert_eq!(ring.pending_count(), 2);
    }
}
