//! # echOS io_uring Gerçekleştirimi
//!
//! Yüksek performanslı asenkron G/Ç, gönderim/tamamlama kuyrukları kullanır.
//! Linux uyumlu io_uring arayüzü.
//!
//! ## io_uring Nedir?
//!
//! io_uring, Linux 5.1 ile eklenen bir çekirdek-kullanıcı uzayı G/Ç arayüzüdür.
//! Geleneksel `read()/write()` çağrılarına göre çok daha verimlidir:
//! - Sistem çağrısı sayısını dramatik biçimde azaltır
//! - Sıfır kopyalama (zero-copy) destekler
//! - Çekirdek ile tek mmap üzerinden iletişim kurar
//!
//! ## Temel Mimari
//!
//! ```
//! Kullanıcı Uzayı              Çekirdek
//! ┌──────────────────┐         ┌──────────────────┐
//! │  Uygulama        │         │  io_uring sürücü │
//! │                  │         │                  │
//! │  SQ (Gönderim)   │──mmap──►│  SQE işleme      │
//! │  [SQE][SQE][SQE] │         │  (read/write/...) │
//! │                  │         │                  │
//! │  CQ (Tamamlama)  │◄─mmap──│  CQE üret        │
//! │  [CQE][CQE][CQE] │         │  (sonuç + hata)  │
//! └──────────────────┘         └──────────────────┘
//!
//! SQ  = Submission Queue  (Gönderim Kuyruğu)
//! CQ  = Completion Queue  (Tamamlama Kuyruğu)
//! SQE = Submission Queue Entry  (Gönderim Kuyruğu Girdisi)
//! CQE = Completion Queue Entry  (Tamamlama Kuyruğu Girdisi)
//! ```
//!
//! ## Tipik Kullanım Akışı
//!
//! ```
//! 1. io_uring_setup(entries, params) → fd
//!                    │
//! 2. get_sqe(fd)     ▼
//!    SQE doldur (opcode, fd, buf, len...)
//!                    │
//! 3. submit_sqe(fd, sqe)
//!    SQ kuyruğuna ekle
//!                    │
//! 4. io_uring_enter(fd, to_submit, min_complete, flags)
//!    Çekirdeğe bildir, isteğe bağlı bekle
//!                    │
//! 5. get_cqe(fd)     ▼
//!    CQE'yi oku → res (dönüş değeri veya -errno)
//! ```

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::mem::size_of;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// IO_URING SABİTLERİ
// ============================================================================

/// io_uring işlem kodları (opcode'lar)
///
/// Her SQE'nin `opcode` alanı hangi sistem çağrısının asenkron olarak
/// yapılacağını belirtir. Linux io_uring API'siyle tam uyumludur.
pub const IORING_OP_NOP: u8 = 0; // Hiçbir şey yapma (test için)
pub const IORING_OP_READV: u8 = 1; // Dağıtık okuma (scatter read)
pub const IORING_OP_WRITEV: u8 = 2; // Dağıtık yazma (gather write)
pub const IORING_OP_FSYNC: u8 = 3; // Dosya senkronizasyonu
pub const IORING_OP_READ_FIXED: u8 = 4; // Kayıtlı tamponla okuma
pub const IORING_OP_WRITE_FIXED: u8 = 5; // Kayıtlı tamponla yazma
pub const IORING_OP_POLL_ADD: u8 = 6; // Olay izlemeye ekle (epoll benzeri)
pub const IORING_OP_POLL_REMOVE: u8 = 7; // Olay izlemeden çıkar
pub const IORING_OP_SENDMSG: u8 = 8; // Socket mesajı gönder
pub const IORING_OP_RECVMSG: u8 = 9; // Socket mesajı al
pub const IORING_OP_TIMEOUT: u8 = 11; // Zaman aşımı ekle
pub const IORING_OP_TIMEOUT_REMOVE: u8 = 12; // Zaman aşımını iptal et
pub const IORING_OP_ACCEPT: u8 = 13; // TCP bağlantısı kabul et
pub const IORING_OP_ASYNC_CANCEL: u8 = 14; // Bekleyen işlemi iptal et
pub const IORING_OP_LINK_TIMEOUT: u8 = 15; // Bağlı işlem için zaman aşımı
pub const IORING_OP_CONNECT: u8 = 16; // TCP bağlantısı başlat
pub const IORING_OP_SEND: u8 = 17; // Socket'e veri gönder
pub const IORING_OP_RECV: u8 = 18; // Socket'ten veri al
pub const IORING_OP_OPENAT: u8 = 19; // Dosya aç
pub const IORING_OP_CLOSE: u8 = 20; // Dosya tanımlayıcısını kapat
pub const IORING_OP_STATX: u8 = 21; // Dosya meta verisi al
pub const IORING_OP_READ: u8 = 22; // Tek arabellekli okuma (pread64 eşdeğeri)
pub const IORING_OP_WRITE: u8 = 23; // Tek arabellekli yazma (pwrite64 eşdeğeri)
pub const IORING_OP_SOCKET: u8 = 26; // Socket oluştur
pub const IORING_OP_PROVIDE_BUFFERS: u8 = 31; // Tampon sağla
pub const IORING_OP_REMOVE_BUFFERS: u8 = 32; // Tamponu geri al

/// io_uring SQE bayrakları (sqe flags)
///
/// Bu bayraklar SQE'nin nasıl işleneceğini kontrol eder:
pub const IOSQE_FIXED_FILE: u8 = 1 << 0; // Kayıtlı dosya tablosu kullan
pub const IOSQE_ASYNC: u8 = 1 << 1; // Her zaman asenkron işle
pub const IOSQE_IO_LINK: u8 = 1 << 2; // Sonraki SQE'yi bu işleme bağla
pub const IOSQE_IO_HARDLINK: u8 = 1 << 3; // Hatalarda da zinciri devam ettir
pub const IOSQE_ASYNC_NORMAL: u8 = 1 << 4; // Asenkron modda normal öncelik
pub const IOSQE_BUFFER_SELECT: u8 = 1 << 5; // Sağlanan tampon grubundan seç

/// io_uring özellik bayrakları (features)
///
/// Çekirdek bu bayraklarla hangi optimizasyonların desteklendiğini bildirir.
pub const IORING_FEAT_SINGLE_MMAP: u32 = 1 << 0; // Tek mmap yeterli
pub const IORING_FEAT_NODROP: u32 = 1 << 1; // CQE düşürmeme garantisi
pub const IORING_FEAT_SUBMIT_STABLE: u32 = 1 << 2; // Gönderim sonrası tampon serbest bırakılabilir
pub const IORING_FEAT_RW_CUR_POS: u32 = 1 << 3; // -1 offset = mevcut konum
pub const IORING_FEAT_CUR_PERSONALITY: u32 = 1 << 4; // Kimlik kalıtımı
pub const IORING_FEAT_FAST_POLL: u32 = 1 << 5; // Hızlı poll modu
pub const IORING_FEAT_POLL_32BITS: u32 = 1 << 6; // 32 bitlik poll olayları

/// io_uring kurulum bayrakları (setup flags)
///
/// io_uring_setup() çağrısında kullanılır:
pub const IORING_SETUP_IOPOLL: u32 = 1 << 0; // Yoklama (polling) modu - interrupt yok
pub const IORING_SETUP_SQPOLL: u32 = 1 << 1; // SQ'yu ayrı çekirdek iş parçacığıyla yokla
pub const IORING_SETUP_SQ_AFF: u32 = 1 << 2; // SQ iş parçacığını CPU'ya bağla
pub const IORING_SETUP_CQSIZE: u32 = 1 << 3; // CQ boyutunu params'tan al
pub const IORING_SETUP_CLAMP: u32 = 1 << 4; // Boyutu en fazla desteklenenle sınırla
pub const IORING_SETUP_ATTACH_WQ: u32 = 1 << 5; // Mevcut io_uring'e bağlan
pub const IORING_SETUP_R_DISABLED: u32 = 1 << 6; // Devre dışı başlat

// ============================================================================
// IO_URING VERİ YAPILARI
// ============================================================================

/// Gönderim Kuyruğu Girdisi (Submission Queue Entry - SQE)
///
/// Kullanıcı, tek bir asenkron işlem istemek için bu yapıyı doldurur.
/// `user_data` alanı tamamlanınca CQE'de aynen geri döner (istek takibi için).
///
/// ```
/// ┌─────────────────────────────────────────────────┐
/// │ opcode  (1B): İşlem kodu (IORING_OP_*)          │
/// │ flags   (1B): SQE bayrakları (IOSQE_*)          │
/// │ ioprio  (2B): G/Ç önceliği                      │
/// │ fd      (4B): Dosya tanımlayıcısı               │
/// │ off     (8B): Ofset (okuma/yazma için)          │
/// │ addr    (8B): Tampon adresi veya iovec işareti  │
/// │ len     (4B): Tampon boyutu veya iovec sayısı   │
/// │ rw_flags(4B): İşleme özel bayraklar             │
/// │ user_data(8B): Kullanıcı verisi (CQE'ye aktarılır)│
/// │ ...     : Ek alanlar                            │
/// └─────────────────────────────────────────────────┘
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct IoUringSqe {
    /// Opcode (IORING_OP_*)
    pub opcode: u8,
    /// Flags (IOSQE_*)
    pub flags: u8,
    /// I/O priority
    pub ioprio: u16,
    /// File descriptor
    pub fd: i32,
    /// Offset (for read/write)
    pub off: u64,
    /// Address (buffer or iovec)
    pub addr: u64,
    /// Length (buffer size or iovec count)
    pub len: u32,
    /// Operation-specific data (rw flags, etc.)
    pub rw_flags: u32,
    /// User data (passed to completion)
    pub user_data: u64,
    /// Buffer selection
    pub buf_group: u16,
    /// Personality
    pub personality: u16,
    /// Splice file descriptor
    pub splice_fd_in: i32,
    /// Padding
    pub pad: u32,
}

/// Tamamlama Kuyruğu Girdisi (Completion Queue Entry - CQE)
///
/// Çekirdek, bir işlem tamamlandığında bu yapıyı CQ'ya yazar.
/// `res` alanı başarıda dönüş değerini, hatalarda `-errno` içerir.
///
/// ```
/// ┌──────────────────────────────────────────┐
/// │ user_data (8B): SQE'deki user_data aynen │
/// │ res       (4B): Sonuç (>=0) veya -errno  │
/// │ flags     (4B): Tamamlama bayrakları      │
/// └──────────────────────────────────────────┘
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct IoUringCqe {
    /// User data (from SQE)
    pub user_data: u64,
    /// Result (return value or -errno)
    pub res: i32,
    /// Flags
    pub flags: u32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct IoUringRegisteredBuffer {
    pub addr: u64,
    pub len: u32,
    pub bgid: u16,
}

#[derive(Clone, Copy, Debug)]
pub struct IoUringPendingOp {
    pub sqe: IoUringSqe,
    pub submitted_tick: u64,
    pub zero_syscall_fast_path: bool,
    pub completed: bool,
    pub result: i32,
}

/// io_uring params yapısı
///
/// `io_uring_setup()` çağrısına hem giriş hem çıkış olarak geçirilir.
/// Giriş: kaç giriş isteniyor, hangi bayraklar kullanılacak.
/// Çıkış: çekirdek gerçekte ne tahsis ettiğini ve hangi özellikleri desteklediğini yazar.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct IoUringParams {
    /// Number of SQ entries
    pub sq_entries: u32,
    /// Number of CQ entries
    pub cq_entries: u32,
    /// Flags
    pub flags: u32,
    /// SQ thread CPU affinity
    pub sq_thread_cpu: u32,
    /// SQ thread idle timeout (ms)
    pub sq_thread_idle: u32,
    /// Features
    pub features: u32,
    /// Reserved
    pub reserved: [u32; 4],
    /// SQ ring offset (mmap)
    pub sq_off: IoUringSqOffsets,
    /// CQ ring offset (mmap)
    pub cq_off: IoUringCqOffsets,
}

/// SQ halka ofsetleri
///
/// Belleğin hangi ofsettinde SQ halkasının hangi parçasının bulunduğunu
/// tanımlar; mmap yapıldıktan sonra bu ofsetler kullanılarak erişilir.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct IoUringSqOffsets {
    pub head: u32,
    pub tail: u32,
    pub ring_mask: u32,
    pub ring_entries: u32,
    pub flags: u32,
    pub dropped: u32,
    pub array: u32,
    pub resv1: u32,
    pub resv2: u64,
}

/// CQ halka ofsetleri
///
/// Tamamlama kuyruğu belleğinin düzenini tanımlar.
/// `overflow` alanı, CQ dolu olduğunda düşürülen CQE sayısını izler.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct IoUringCqOffsets {
    pub head: u32,
    pub tail: u32,
    pub ring_mask: u32,
    pub ring_entries: u32,
    pub overflow: u32, // CQ taştığında bu sayaç artar
    pub cqes: u32,
    pub flags: u32,
    pub resv1: u32,
    pub resv2: u64,
}

// ============================================================================
// IO_URING HALKA TAMPONU
// ============================================================================

/// SQ/CQ için genel amaçlı dairesel halka tamponu (ring buffer)
///
/// Dairesel tampon, üretici-tüketici modelinde paylaşılan bellek üzerinde
/// senkronizasyonsuz çalışmayı mümkün kılar.
///
/// ```
/// entries = 8  (2'nin kuvveti olmalı)
/// mask    = 7  (entries - 1)
///
///   head                    tail
///    │                       │
///    ▼                       ▼
/// ┌────┬────┬────┬────┬────┬────┬────┬────┐
/// │[0] │[1] │[2] │[3] │[4] │[5] │[6] │[7] │
/// └────┴────┴────┴────┴────┴────┴────┴────┘
///  gerçek indeks = sayaç & mask
///  sayaç sonsuz artabilir, wrap-around sorun değil
/// ```
pub struct IoUringRing<T> {
    /// Ring buffer memory
    buffer: *mut T,
    /// Physical address
    paddr: usize,
    /// Number of entries
    entries: u32,
    /// Ring mask (entries - 1)
    mask: u32,
    /// Head index
    head: u32,
    /// Tail index
    tail: u32,
}

impl<T> Clone for IoUringRing<T> {
    fn clone(&self) -> Self {
        IoUringRing {
            buffer: self.buffer,
            paddr: self.paddr,
            entries: self.entries,
            mask: self.mask,
            head: self.head,
            tail: self.tail,
        }
    }
}

impl<T> IoUringRing<T> {
    /// Yeni bir halka tamponu oluşturur.
    ///
    /// `entries` sayısı 2'nin kuvveti olmalıdır; mask = entries - 1 ile
    /// modulo işlemi yerine verimli AND maskesi kullanılır.
    pub fn new(entries: u32) -> Option<Self> {
        let size = entries as usize * size_of::<T>();
        let pages = (size + 4095) / 4096;

        let (paddr, vaddr) = crate::memory::dma_alloc(pages)?;

        // Zero the buffer
        unsafe {
            core::ptr::write_bytes(vaddr.as_ptr(), 0, size);
        }

        Some(IoUringRing {
            buffer: vaddr.as_ptr() as *mut T,
            paddr,
            entries,
            mask: entries - 1,
            head: 0,
            tail: 0,
        })
    }

    /// `index & mask` ile gerçek bellek konumunu hesaplar ve girdiye referans verir.
    pub unsafe fn get(&self, index: u32) -> &T {
        &*(self.buffer.add((index & self.mask) as usize))
    }

    /// `index & mask` ile gerçek bellek konumunu hesaplar ve değiştirilebilir referans verir.
    pub unsafe fn get_mut(&mut self, index: u32) -> &mut T {
        &mut *(self.buffer.add((index & self.mask) as usize))
    }

    /// Mevcut okuma başını döner.
    pub fn head(&self) -> u32 {
        self.head
    }

    /// Mevcut yazma kuyruğunu döner.
    pub fn tail(&self) -> u32 {
        self.tail
    }

    /// Okuma başını bir ilerletir (tüketici bir girdi okuduğunda).
    pub fn advance_head(&mut self) {
        self.head = self.head.wrapping_add(1);
    }

    /// Yazma kuyruğunu bir ilerletir (üretici bir girdi eklediğinde).
    pub fn advance_tail(&mut self) {
        self.tail = self.tail.wrapping_add(1);
    }

    /// Kuyruk boş mu? (head == tail)
    pub fn is_empty(&self) -> bool {
        self.head == self.tail
    }

    /// Kuyruk dolu mu? (tail - head == entries)
    pub fn is_full(&self) -> bool {
        self.tail.wrapping_sub(self.head) == self.entries
    }

    /// Kuyrukta kaç girdi var?
    pub fn count(&self) -> u32 {
        self.tail.wrapping_sub(self.head)
    }

    /// Toplam kapasite.
    pub fn entries(&self) -> u32 {
        self.entries
    }

    /// İndeks maskesi (AND ile hızlı modulo için).
    pub fn mask(&self) -> u32 {
        self.mask
    }
}

impl<T> Drop for IoUringRing<T> {
    fn drop(&mut self) {
        if self.paddr != 0 {
            let size = self.entries as usize * size_of::<T>();
            let pages = (size + 4095) / 4096;
            crate::memory::dma_dealloc(self.paddr, pages);
        }
    }
}

unsafe impl<T: Send> Send for IoUringRing<T> {}
unsafe impl<T: Sync> Sync for IoUringRing<T> {}

// ============================================================================
// IO_URING ÖRNEĞİ
// ============================================================================

/// io_uring örneği
///
/// Tek bir io_uring bağlamını temsil eder.
/// Her bağlam bağımsız SQ ve CQ halkalarına sahiptir.
///
/// ```
/// IoUring
/// ├── sq_ring: IoUringRing<IoUringSqe>  ← Kullanıcı buraya SQE ekler
/// ├── cq_ring: IoUringRing<IoUringCqe>  ← Kullanıcı buradan CQE okur
/// ├── sq_array: Vec<u32>                ← SQE indeks dizisi
/// ├── pending: BTreeMap<u64, IoUringSqe> ← İşlenmekte olan istekler
/// └── params: IoUringParams              ← Yapılandırma
/// ```
#[derive(Clone)]
pub struct IoUring {
    /// Instance ID
    pub id: u32,
    /// SQ ring
    pub sq_ring: IoUringRing<IoUringSqe>,
    /// CQ ring
    pub cq_ring: IoUringRing<IoUringCqe>,
    /// SQ array (index to SQE) - stored as Vec for thread safety
    pub sq_array: Vec<u32>,
    /// Parameters
    pub params: IoUringParams,
    /// Pending operations
    pub pending: BTreeMap<u64, IoUringPendingOp>,
    /// Registered file table for fixed-file submissions
    pub registered_files: Vec<i32>,
    /// Registered buffers for zero-syscall style fixed-buffer flows
    pub registered_buffers: Vec<IoUringRegisteredBuffer>,
    /// Next user data
    pub next_user_data: u64,
    /// SQ poll thread active
    pub sq_poll_active: bool,
    /// Number of SQ entries auto-drained by SQPOLL
    pub sq_poll_processed: u64,
    /// Count of submissions that stay on the fixed-resource SQPOLL path
    pub zero_syscall_submissions: u64,
    /// Count of completions produced on the fixed-resource SQPOLL path
    pub zero_syscall_completions: u64,
    pub submit_batches: u64,
    pub completion_batches: u64,
    pub max_submit_batch: u32,
    pub max_completion_batch: u32,
}

impl IoUring {
    /// Yeni bir io_uring örneği oluşturur.
    ///
    /// `entries` sayısı istenilen SQ boyutunu belirtir; 2'nin kuvvetine yuvarlanır.
    /// CQ, SQ'nun 2 katı büyüklüktedir (ekstra tampon).
    pub fn new(entries: u32, params: Option<IoUringParams>) -> Option<Self> {
        let sq_entries = entries.next_power_of_two();
        let cq_entries = sq_entries * 2; // CQ is usually 2x SQ size

        let sq_ring = IoUringRing::new(sq_entries)?;
        let cq_ring = IoUringRing::new(cq_entries)?;

        // Allocate SQ array as Vec
        let sq_array = alloc::vec![0u32; sq_entries as usize];

        let mut io_uring_params = params.unwrap_or_default();
        io_uring_params.sq_entries = sq_entries;
        io_uring_params.cq_entries = cq_entries;
        // Desteklenen özellikleri bildir
        io_uring_params.features =
            IORING_FEAT_SINGLE_MMAP | IORING_FEAT_NODROP | IORING_FEAT_FAST_POLL;

        Some(IoUring {
            id: 0,
            sq_ring,
            cq_ring,
            sq_array,
            params: io_uring_params,
            pending: BTreeMap::new(),
            registered_files: Vec::new(),
            registered_buffers: Vec::new(),
            next_user_data: 1,
            sq_poll_active: io_uring_params.flags & IORING_SETUP_SQPOLL != 0,
            sq_poll_processed: 0,
            zero_syscall_submissions: 0,
            zero_syscall_completions: 0,
            submit_batches: 0,
            completion_batches: 0,
            max_submit_batch: 0,
            max_completion_batch: 0,
        })
    }

    /// Bir sonraki benzersiz kullanıcı verisini döner ve sayacı artırır.
    ///
    /// `user_data` değeri SQE ile CQE arasındaki bağı kurar:
    /// hangi tamamlanma hangi isteğe aittir?
    pub fn next_user_data(&mut self) -> u64 {
        let ud = self.next_user_data;
        self.next_user_data += 1;
        ud
    }

    fn resolve_fd(&self, sqe: &IoUringSqe) -> Result<u32, i32> {
        let fd = if sqe.flags & IOSQE_FIXED_FILE != 0 {
            let index = sqe.fd as usize;
            *self.registered_files.get(index).ok_or(-9)?
        } else {
            sqe.fd
        };

        if fd < 0 {
            Err(-9)
        } else {
            Ok(fd as u32)
        }
    }

    fn registered_buffer_by_index(&self, index: usize) -> Option<IoUringRegisteredBuffer> {
        self.registered_buffers.get(index).copied()
    }

    fn selected_buffer_for_group(&self, bgid: u16) -> Option<IoUringRegisteredBuffer> {
        self.registered_buffers
            .iter()
            .find(|buffer| buffer.bgid == bgid)
            .copied()
    }

    fn resolve_buffer_window(&self, sqe: &IoUringSqe) -> Result<(*mut u8, usize), i32> {
        let requested_len = sqe.len as usize;

        match sqe.opcode {
            IORING_OP_READ_FIXED | IORING_OP_WRITE_FIXED => {
                let buffer = self
                    .registered_buffer_by_index(sqe.buf_group as usize)
                    .ok_or(-22)?;
                let len = core::cmp::min(requested_len, buffer.len as usize);
                if len == 0 {
                    return Err(-22);
                }
                Ok((buffer.addr as *mut u8, len))
            }
            _ if sqe.flags & IOSQE_BUFFER_SELECT != 0 => {
                let buffer = self.selected_buffer_for_group(sqe.buf_group).ok_or(-22)?;
                let len = core::cmp::min(requested_len, buffer.len as usize);
                if len == 0 {
                    return Err(-22);
                }
                Ok((buffer.addr as *mut u8, len))
            }
            _ => {
                if sqe.addr == 0 || requested_len == 0 {
                    Err(-22)
                } else {
                    Ok((sqe.addr as *mut u8, requested_len))
                }
            }
        }
    }

    fn zero_syscall_eligible(&self, sqe: &IoUringSqe) -> bool {
        if !self.sq_poll_active {
            return false;
        }

        match sqe.opcode {
            IORING_OP_NOP => true,
            IORING_OP_READ_FIXED | IORING_OP_WRITE_FIXED => {
                sqe.flags & IOSQE_FIXED_FILE != 0
                    && self.resolve_fd(sqe).is_ok()
                    && self
                        .registered_buffer_by_index(sqe.buf_group as usize)
                        .is_some()
            }
            IORING_OP_READ | IORING_OP_WRITE | IORING_OP_SEND | IORING_OP_RECV => {
                let has_fixed_file =
                    sqe.flags & IOSQE_FIXED_FILE != 0 && self.resolve_fd(sqe).is_ok();
                let has_selected_buffer = sqe.flags & IOSQE_BUFFER_SELECT != 0
                    && self.selected_buffer_for_group(sqe.buf_group).is_some();
                has_fixed_file && (has_selected_buffer || (sqe.addr != 0 && sqe.len != 0))
            }
            _ => false,
        }
    }

    /// SQ kuyruğuna yeni bir SQE ekler.
    ///
    /// Kuyruğa ekleme başarılı olursa, işlem `pending` haritasında takip edilir.
    /// Kuyruk doluysa `QueueFull` hatası döner.
    fn enqueue_sqe(&mut self, sqe: IoUringSqe, auto_poll: bool) -> Result<(), IoUringError> {
        if self.sq_ring.is_full() {
            return Err(IoUringError::QueueFull);
        }

        // Get next SQ slot
        let tail = self.sq_ring.tail();
        let idx = (tail & self.sq_ring.mask()) as usize;

        // Write SQE
        unsafe {
            *self.sq_ring.get_mut(idx as u32) = sqe;
        }

        // Update SQ array
        self.sq_array[idx] = idx as u32;

        // Advance tail
        self.sq_ring.advance_tail();

        let zero_syscall_fast_path = self.zero_syscall_eligible(&sqe);

        // Track pending
        self.pending.insert(
            sqe.user_data,
            IoUringPendingOp {
                sqe,
                submitted_tick: crate::interrupts::get_ticks(),
                zero_syscall_fast_path,
                completed: false,
                result: 0,
            },
        );

        if zero_syscall_fast_path {
            self.zero_syscall_submissions += 1;
        }

        // SQPOLL mode drains SQ without an explicit enter() syscall.
        if auto_poll && self.sq_poll_active {
            self.sq_poll_processed += self.process_pending() as u64;
        }

        Ok(())
    }

    pub fn submit_sqe(&mut self, sqe: IoUringSqe) -> Result<(), IoUringError> {
        self.enqueue_sqe(sqe, true)
    }

    pub fn submit_sqes(&mut self, sqes: &[IoUringSqe]) -> Result<usize, IoUringError> {
        let mut submitted = 0usize;
        for sqe in sqes.iter().copied() {
            if self.enqueue_sqe(sqe, false).is_err() {
                break;
            }
            submitted += 1;
        }
        if submitted == 0 {
            return Err(IoUringError::QueueFull);
        }
        self.submit_batches += 1;
        self.max_submit_batch = self
            .max_submit_batch
            .max(submitted.min(u32::MAX as usize) as u32);
        if self.sq_poll_active {
            let processed = self.process_pending_budgeted(submitted.min(u32::MAX as usize) as u32);
            self.sq_poll_processed += processed as u64;
        }
        Ok(submitted)
    }

    /// CQ kuyruğundan bir CQE alır ve kuyruktan çıkarır.
    ///
    /// Kuyruk boşsa `None` döner.
    /// Alınan CQE'nin `user_data`'sına göre `pending` haritasından SQE silinir.
    pub fn get_cqe(&mut self) -> Option<IoUringCqe> {
        if self.cq_ring.is_empty() {
            return None;
        }

        let head = self.cq_ring.head();
        let idx = head & self.cq_ring.mask();

        let cqe = unsafe { *self.cq_ring.get(idx) };

        // Remove from pending
        self.pending.remove(&cqe.user_data);

        // Advance head
        self.cq_ring.advance_head();

        Some(cqe)
    }

    /// CQ kuyruğuna bakar ama çıkarmaz (peek).
    pub fn peek_cqe(&self) -> Option<IoUringCqe> {
        if self.cq_ring.is_empty() {
            return None;
        }

        let head = self.cq_ring.head();
        let idx = head & self.cq_ring.mask();

        Some(unsafe { *self.cq_ring.get(idx) })
    }

    /// CQE için belirtilen milisaniye kadar bekler.
    ///
    /// `timeout_ms == 0` ise süresiz bekler.
    /// Zaman aşımında `None` döner.
    pub fn wait_cqe(&mut self, timeout_ms: u64) -> Option<IoUringCqe> {
        let start = crate::interrupts::get_ticks();

        loop {
            if let Some(cqe) = self.get_cqe() {
                return Some(cqe);
            }

            // Check timeout
            if timeout_ms > 0 {
                let elapsed = crate::interrupts::get_ticks() - start;
                if elapsed >= timeout_ms {
                    return None;
                }
            }

            // Yield CPU
            crate::task::scheduler::schedule();
        }
    }

    /// Bekleyen tüm SQE'leri işleyerek CQE üretir.
    ///
    /// Her SQE için `execute_op()` çağrılır, sonuç CQE olarak CQ'ya yazılır.
    /// Dönen değer işlenen SQE sayısıdır.
    pub fn process_pending(&mut self) -> u32 {
        self.process_pending_budgeted(u32::MAX)
    }

    pub fn process_pending_budgeted(&mut self, budget: u32) -> u32 {
        let mut processed = 0u32;

        // Process all pending SQEs
        let pending: Vec<(u64, IoUringSqe)> = self
            .pending
            .iter()
            .filter_map(|(user_data, pending)| {
                (!pending.completed).then_some((*user_data, pending.sqe))
            })
            .collect();

        for (user_data, sqe) in pending {
            if processed >= budget {
                break;
            }
            let result = self.execute_op(&sqe);

            // Create CQE
            let cqe = IoUringCqe {
                user_data,
                res: result,
                flags: 0,
            };

            // Add to CQ
            if !self.cq_ring.is_full() {
                let tail = self.cq_ring.tail();
                let idx = tail & self.cq_ring.mask();
                unsafe {
                    *self.cq_ring.get_mut(idx) = cqe;
                }
                self.cq_ring.advance_tail();
                if let Some(pending) = self.pending.get_mut(&user_data) {
                    if pending.zero_syscall_fast_path {
                        self.zero_syscall_completions += 1;
                    }
                    pending.completed = true;
                    pending.result = result;
                }
                processed += 1;
            }
        }

        if processed > 0 {
            self.completion_batches += 1;
            self.max_completion_batch = self.max_completion_batch.max(processed);
        }

        processed
    }

    /// Tek bir SQE'yi çalıştırır ve sonucu döner.
    ///
    /// Başarıda >=0 (dönen bayt sayısı veya yeni fd), hatalarda -errno döner.
    /// Linux errno değerleri kullanılır: -5 = EIO, -11 = EAGAIN, -22 = EINVAL, vb.
    fn execute_op(&mut self, sqe: &IoUringSqe) -> i32 {
        match sqe.opcode {
            IORING_OP_NOP => 0,

            IORING_OP_READ | IORING_OP_READ_FIXED => {
                let fd = match self.resolve_fd(sqe) {
                    Ok(fd) => fd,
                    Err(errno) => return errno,
                };
                let (buf, len) = match self.resolve_buffer_window(sqe) {
                    Ok(window) => window,
                    Err(errno) => return errno,
                };

                if let Ok(n) = crate::net::socket::recv(
                    fd,
                    unsafe { core::slice::from_raw_parts_mut(buf, len) },
                    0,
                ) {
                    n as i32
                } else {
                    -5
                }
            }

            IORING_OP_WRITE | IORING_OP_WRITE_FIXED => {
                let fd = match self.resolve_fd(sqe) {
                    Ok(fd) => fd,
                    Err(errno) => return errno,
                };
                let (buf, len) = match self.resolve_buffer_window(sqe) {
                    Ok(window) => window,
                    Err(errno) => return errno,
                };

                if let Ok(n) = crate::net::socket::send(
                    fd,
                    unsafe { core::slice::from_raw_parts(buf as *const u8, len) },
                    0,
                ) {
                    n as i32
                } else {
                    -5
                }
            }

            IORING_OP_SEND => {
                let fd = match self.resolve_fd(sqe) {
                    Ok(fd) => fd,
                    Err(errno) => return errno,
                };
                let (buf, len) = match self.resolve_buffer_window(sqe) {
                    Ok(window) => window,
                    Err(errno) => return errno,
                };

                if let Ok(n) = crate::net::socket::send(
                    fd,
                    unsafe { core::slice::from_raw_parts(buf as *const u8, len) },
                    0,
                ) {
                    n as i32
                } else {
                    -5
                }
            }

            IORING_OP_RECV => {
                let fd = match self.resolve_fd(sqe) {
                    Ok(fd) => fd,
                    Err(errno) => return errno,
                };
                let (buf, len) = match self.resolve_buffer_window(sqe) {
                    Ok(window) => window,
                    Err(errno) => return errno,
                };

                if let Ok(n) = crate::net::socket::recv(
                    fd,
                    unsafe { core::slice::from_raw_parts_mut(buf, len) },
                    0,
                ) {
                    n as i32
                } else {
                    -5
                }
            }

            IORING_OP_ACCEPT => {
                let fd = match self.resolve_fd(sqe) {
                    Ok(fd) => fd,
                    Err(errno) => return errno,
                };
                match crate::net::socket::accept(fd) {
                    Ok((new_fd, _addr)) => new_fd as i32,
                    Err(_) => -11, // -EAGAIN
                }
            }

            IORING_OP_CONNECT => {
                let fd = match self.resolve_fd(sqe) {
                    Ok(fd) => fd,
                    Err(errno) => return errno,
                };
                // Parse sockaddr from sqe.addr
                // Simplified: assume IPv4
                let addr_bytes = unsafe { core::slice::from_raw_parts(sqe.addr as *const u8, 16) };
                let ip = crate::net::Ipv4Addr::from_bytes([
                    addr_bytes[4],
                    addr_bytes[5],
                    addr_bytes[6],
                    addr_bytes[7],
                ]);
                let port = u16::from_be_bytes([addr_bytes[2], addr_bytes[3]]);
                let addr = crate::net::SocketAddr::new(ip, crate::net::Port(port));

                match crate::net::socket::connect(fd, addr) {
                    Ok(()) => 0,
                    Err(_) => -111, // -ECONNREFUSED
                }
            }

            IORING_OP_SOCKET => {
                // Create socket
                let domain = sqe.fd;
                let sock_type = (sqe.len >> 16) as i32;
                let protocol = (sqe.len & 0xFFFF) as i32;

                let af = match domain {
                    2 => crate::net::socket::AddressFamily::IPV4,
                    10 => crate::net::socket::AddressFamily::IPV6,
                    _ => return -22, // -EINVAL
                };

                let st = match sock_type {
                    1 => crate::net::socket::SocketType::STREAM,
                    2 => crate::net::socket::SocketType::DGRAM,
                    _ => return -22,
                };

                let proto = match protocol {
                    0 => crate::net::socket::Protocol::DEFAULT,
                    6 => crate::net::socket::Protocol::TCP,
                    17 => crate::net::socket::Protocol::UDP,
                    _ => return -22,
                };

                match crate::net::socket::socket(af, st, proto) {
                    Ok(fd) => fd as i32,
                    Err(_) => -24, // -EMFILE
                }
            }

            IORING_OP_CLOSE => {
                let fd = match self.resolve_fd(sqe) {
                    Ok(fd) => fd,
                    Err(errno) => return errno,
                };
                match crate::net::socket::close(fd) {
                    Ok(()) => 0,
                    Err(_) => -9, // -EBADF
                }
            }

            IORING_OP_POLL_ADD => {
                let fd = match self.resolve_fd(sqe) {
                    Ok(fd) => fd,
                    Err(errno) => return errno,
                };
                let events = sqe.len as u16;

                // Check if events are ready
                let ready = if events & 1 != 0 {
                    crate::net::socket::can_read(fd)
                } else {
                    false
                };

                let ready = ready
                    || if events & 4 != 0 {
                        crate::net::socket::can_write(fd)
                    } else {
                        false
                    };

                if ready {
                    events as i32
                } else {
                    -11 // -EAGAIN
                }
            }

            IORING_OP_TIMEOUT => {
                // Timeout operation
                let ts = unsafe { &*(sqe.addr as *const IoUringTimeout) };
                let timeout_ns = ts.ts_nsec;
                let timeout_ms = timeout_ns / 1_000_000;

                // Wait for timeout
                let start = crate::interrupts::get_ticks();
                loop {
                    let elapsed = crate::interrupts::get_ticks() - start;
                    if elapsed >= timeout_ms as u64 {
                        break;
                    }
                    crate::task::scheduler::schedule();
                }

                -62 // -ETIME
            }

            IORING_OP_PROVIDE_BUFFERS => {
                let stride = size_of::<IoUringRegisteredBuffer>();
                let mut registered = 0i32;

                for index in 0..sqe.len as usize {
                    let ptr =
                        (sqe.addr as usize + index * stride) as *const IoUringRegisteredBuffer;
                    let mut buffer = unsafe { *ptr };
                    if buffer.bgid == 0 {
                        buffer.bgid = sqe.buf_group;
                    }
                    self.registered_buffers.push(buffer);
                    registered += 1;
                }

                registered
            }

            IORING_OP_REMOVE_BUFFERS => {
                let mut removed = 0usize;
                let wanted = sqe.len as usize;
                let bgid = sqe.buf_group;

                self.registered_buffers.retain(|buffer| {
                    let matches_group = bgid == 0 || buffer.bgid == bgid;
                    let keep = !(matches_group && removed < wanted);
                    if !keep {
                        removed += 1;
                    }
                    keep
                });

                removed as i32
            }

            _ => -22, // -EINVAL
        }
    }

    pub fn registered_file_count(&self) -> usize {
        self.registered_files.len()
    }

    pub fn registered_buffer_count(&self) -> usize {
        self.registered_buffers.len()
    }

    pub fn zero_syscall_ready(&self) -> bool {
        self.sq_poll_active
            && !self.registered_files.is_empty()
            && !self.registered_buffers.is_empty()
    }

    pub fn zero_syscall_submission_count(&self) -> u64 {
        self.zero_syscall_submissions
    }

    pub fn zero_syscall_completion_count(&self) -> u64 {
        self.zero_syscall_completions
    }

    pub fn batching_snapshot(&self) -> (u64, u64, u32, u32) {
        (
            self.submit_batches,
            self.completion_batches,
            self.max_submit_batch,
            self.max_completion_batch,
        )
    }
}

/// io_uring zaman aşımı yapısı
///
/// IORING_OP_TIMEOUT işleminde `sqe.addr` bu yapıya işaret eder.
/// saniye + nanosaniye çiftiyle hassas zamanlama sağlar.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct IoUringTimeout {
    pub ts_sec: u64,
    pub ts_nsec: u64,
    pub flags: u32,
    pub count: u32,
}

/// io_uring hata türleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IoUringError {
    QueueFull,    // SQ kuyruğu dolu
    QueueEmpty,   // CQ kuyruğu boş
    InvalidParam, // Geçersiz parametre
    NoMemory,     // Bellek yetersiz
    NotReady,     // Hazır değil
}

// ============================================================================
// IO_URING YÖNETİCİSİ
// ============================================================================

/// Global io_uring örne kleri haritası (fd → IoUring)
///
/// Her io_uring_setup() çağrısı yeni bir örnek oluşturur ve benzersiz ID döner.
/// Bu ID, sonraki tüm işlemlerde "fd" olarak kullanılır.
static IO_URING_INSTANCES: Mutex<BTreeMap<u32, Box<IoUring>>> = Mutex::new(BTreeMap::new());

/// Bir sonraki io_uring örneğine verilecek ID.
static NEXT_IO_URING_ID: AtomicU32 = AtomicU32::new(1);

/// Yeni bir io_uring örneği oluşturur.
///
/// `entries`: istenen kuyruk derinliği (2'nin kuvvetine yuvarlanır)
/// `params`: opsiyonel yapılandırma; `None` ise varsayılanlar kullanılır
///
/// Başarıda örneğin ID'sini (fd) döner.
pub fn io_uring_setup(entries: u32, params: Option<IoUringParams>) -> Result<u32, IoUringError> {
    let mut instances = IO_URING_INSTANCES.lock();

    let id = NEXT_IO_URING_ID.fetch_add(1, Ordering::Relaxed);
    let mut io_uring = IoUring::new(entries, params).ok_or(IoUringError::NoMemory)?;
    io_uring.id = id;

    instances.insert(id, Box::new(io_uring));

    Ok(id)
}

/// io_uring'e giriş: SQE gönderir ve/veya CQE bekler.
///
/// - `to_submit > 0`: bekleyen SQE'leri işle
/// - `min_complete > 0`: en az bu kadar CQE hazır olana dek bekle
/// - `flags & 1 != 0`: bloklamayan mod (CQE yoksa hemen dön)
pub fn io_uring_enter(
    fd: u32,
    to_submit: u32,
    min_complete: u32,
    flags: u32,
) -> Result<u32, IoUringError> {
    let mut instances = IO_URING_INSTANCES.lock();
    let io_uring = instances.get_mut(&fd).ok_or(IoUringError::InvalidParam)?;

    let mut submitted = 0u32;
    let mut completed = 0u32;

    // Submit SQEs
    if to_submit > 0 || io_uring.sq_poll_active {
        let budget = if to_submit > 0 { to_submit } else { u32::MAX };
        submitted = io_uring.process_pending_budgeted(budget);
        if io_uring.sq_poll_active {
            io_uring.sq_poll_processed += submitted as u64;
        }
    }

    // Wait for completions
    if min_complete > 0 {
        for _ in 0..min_complete {
            if io_uring.get_cqe().is_some() {
                completed += 1;
            } else if flags & 1 != 0 {
                // Non-blocking
                break;
            } else {
                // Blocking wait
                let start = crate::interrupts::get_ticks();
                loop {
                    if io_uring.get_cqe().is_some() {
                        completed += 1;
                        break;
                    }

                    // Timeout check (30 seconds)
                    if crate::interrupts::get_ticks() - start > 30000 {
                        break;
                    }

                    crate::task::scheduler::schedule();
                }
            }
        }
    }

    Ok(submitted.max(completed))
}

/// Tampon veya dosya kayıt eder.
///
/// Kayıtlı tamponlar/dosyalar io_uring'e önceden bildirilir;
/// bu sayede her işlemde bellek haritalama maliyeti ortadan kalkar.
pub fn io_uring_register(
    fd: u32,
    opcode: u32,
    arg: u64,
    nr_args: u32,
) -> Result<i32, IoUringError> {
    let mut instances = IO_URING_INSTANCES.lock();

    match opcode {
        0 => {
            // Register files: fd tablosunu io_uring örneğine bağla
            // arg = dosya descriptor dizisi pointer'ı, nr_args = dizi boyutu
            if let Some(instance) = instances.get_mut(&fd) {
                let files =
                    unsafe { core::slice::from_raw_parts(arg as *const i32, nr_args as usize) };
                instance.registered_files.clear();
                instance.registered_files.extend_from_slice(files);
                crate::serial_println!(
                    "[IO_URING] Registered {} files for ring fd={}",
                    nr_args,
                    fd
                );
                // Kayıtlı dosya sayısını dön
                Ok(nr_args as i32)
            } else {
                Err(IoUringError::InvalidParam)
            }
        }
        1 => {
            // Register buffers: sabit tampon havuzunu io_uring'e bildir
            // arg = iovec dizisi pointer'ı, nr_args = iovec sayısı
            if let Some(instance) = instances.get_mut(&fd) {
                let mut buffers = Vec::with_capacity(nr_args as usize);
                let stride = size_of::<IoUringRegisteredBuffer>();
                for index in 0..nr_args as usize {
                    let ptr = (arg as usize + index * stride) as *const IoUringRegisteredBuffer;
                    buffers.push(unsafe { *ptr });
                }
                instance.registered_buffers = buffers;
                crate::serial_println!(
                    "[IO_URING] Registered {} buffers for ring fd={}",
                    nr_args,
                    fd
                );
                Ok(nr_args as i32)
            } else {
                Err(IoUringError::InvalidParam)
            }
        }
        2 => {
            // Unregister files
            if let Some(instance) = instances.get_mut(&fd) {
                let released = instance.registered_files.len() as i32;
                instance.registered_files.clear();
                crate::serial_println!("[IO_URING] Unregistered files for ring fd={}", fd);
                Ok(released)
            } else {
                Err(IoUringError::InvalidParam)
            }
        }
        3 => {
            // Unregister buffers
            if let Some(instance) = instances.get_mut(&fd) {
                let released = instance.registered_buffers.len() as i32;
                instance.registered_buffers.clear();
                crate::serial_println!("[IO_URING] Unregistered buffers for ring fd={}", fd);
                Ok(released)
            } else {
                Err(IoUringError::InvalidParam)
            }
        }
        _ => Err(IoUringError::InvalidParam),
    }
}

/// io_uring örneğini siler ve kaynakları serbest bırakır.
pub fn io_uring_close(fd: u32) -> Result<(), IoUringError> {
    let mut instances = IO_URING_INSTANCES.lock();
    instances
        .remove(&fd)
        .map(|_| ())
        .ok_or(IoUringError::InvalidParam)
}

/// io_uring örneğinin bir kopyasını döner.
pub fn get_io_uring(fd: u32) -> Option<IoUring> {
    let instances = IO_URING_INSTANCES.lock();
    instances.get(&fd).map(|i| (**i).clone())
}

/// Doldurulmak üzere boş bir SQE şablonu döner.
///
/// Kuyruk doluysa `None` döner.
/// Dönen SQE doldurulduktan sonra `submit_sqe()` ile kuyruğa eklenir.
pub fn get_sqe(fd: u32) -> Option<IoUringSqe> {
    let instances = IO_URING_INSTANCES.lock();
    let io_uring = instances.get(&fd)?;

    if io_uring.sq_ring.is_full() {
        return None;
    }

    Some(IoUringSqe::default())
}

/// Doldurulmuş bir SQE'yi kuyruğa gönderir.
pub fn submit_sqe(fd: u32, sqe: IoUringSqe) -> Result<(), IoUringError> {
    let mut instances = IO_URING_INSTANCES.lock();
    let io_uring = instances.get_mut(&fd).ok_or(IoUringError::InvalidParam)?;
    io_uring.submit_sqe(sqe)
}

/// CQ kuyruğundan tamamlanmış bir işlemin sonucunu alır.
pub fn get_cqe(fd: u32) -> Option<IoUringCqe> {
    let mut instances = IO_URING_INSTANCES.lock();
    let io_uring = instances.get_mut(&fd)?;
    io_uring.get_cqe()
}

/// Belirtilen süre boyunca CQ kuyruğundan sonuç bekler.
pub fn wait_cqe(fd: u32, timeout_ms: u64) -> Option<IoUringCqe> {
    let mut instances = IO_URING_INSTANCES.lock();
    let io_uring = instances.get_mut(&fd)?;
    io_uring.wait_cqe(timeout_ms)
}

/// io_uring alt sistemini başlatır.
pub fn init() {
    crate::serial_println!("[IO_URING] Initialized");
}
