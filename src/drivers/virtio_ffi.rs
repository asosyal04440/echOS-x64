//! # VirtIO FFI (Foreign Function Interface) Katmanı
//!
//! Bu modül, VirtIO blok cihazı ile Rust kodunu C ABI üzerinden bağlayan
//! transport-gorunurluk katmanıdır. Gerçek queue/DMA arka ucu bağlı
//! olmadığında veri yolu explicit hata ile kapanır.
//!
//! ## VirtIO Nedir?
//!
//! VirtIO, sanallaştırma ortamlarında (QEMU, KVM, Simics) kullanılan
//! standart bir aygıt arabirimi protokolüdür. Misafir işletim sistemi ile
//! ana bilgisayar arasında verimli I/O sağlar.
//!
//! ## Mimari Diyagramı
//!
//! ```
//!  ┌───────────────────────────────────────────────┐
//!  │              Rust Çekirdek (echOS)             │
//!  │                                               │
//!  │   VirtioBlock::read_sector()                  │
//!  │   VirtioBlock::write_sector()                 │
//!  │           │                                   │
//!  │           ▼                                   │
//!  │   virtio_ffi::init()  ──────► BASE_PORT       │
//!  │   virtio_ffi::device() ◄──── VirtioBlock      │
//!  │           │                                   │
//!  │           ▼   (FFI çağrısı)                   │
//!  │   virtio_disk_rw(sector, buf, write)          │
//!  │           │                                   │
//!  └───────────│───────────────────────────────────┘
//!              ▼  (queue/DMA backend bagli degilse explicit hata)
//!         serial_println! (degraded durum raporu)
//! ```
//!
//! ## virt_to_phys_c Fonksiyonu
//!
//! C kodu fiziksel adreslere ihtiyaç duyar. Bu fonksiyon, sanal adresi
//! bellek çeviri tablosundan (page table) fiziksel adrese çevirir.
//!
//! ## Sektör Tabanlı Okuma/Yazma
//!
//! Disk, 512 byte'lık sektörler halinde organize edilir.
//! read_at/write_at, byte offset üzerinden sektör hesabı yaparak
//! çoklu sektör transferi gerçekleştirir.
//!
//! ```
//! offset=768  → sektör=1 (768/512), within=256 (768%512)
//!    sektör 0  sektör 1
//!   [       ] [  ▲    ]  ← within=256 pozisyonundan başla
//!               └─ veri buradan kopyalanır
//! ```

use core::sync::atomic::{AtomicBool, AtomicU16, AtomicU8, Ordering};
use rcore_fs::dev::{DevError, Device, Result as DevResult};
use spin::Mutex;
use virtio_drivers::transport::pci::bus::DeviceFunction;
use virtio_drivers::transport::pci::PciTransport;

const INVALID_PHYS_ADDR: u64 = u64::MAX;
const LEGACY_STATUS_OFFSET: u16 = 18;
const STATUS_ACKNOWLEDGE: u8 = 1;
const STATUS_DRIVER: u8 = 2;
const STATUS_DRIVER_OK: u8 = 4;
const STATUS_FEATURES_OK: u8 = 8;
const STATUS_FAILED: u8 = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LegacyStatusSequence {
    reset: u8,
    acknowledge: u8,
    driver: u8,
    features_ok: u8,
    driver_ok: u8,
}

const fn legacy_status_sequence() -> LegacyStatusSequence {
    LegacyStatusSequence {
        reset: 0,
        acknowledge: STATUS_ACKNOWLEDGE,
        driver: STATUS_ACKNOWLEDGE | STATUS_DRIVER,
        features_ok: STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK,
        driver_ok: STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK | STATUS_DRIVER_OK,
    }
}

const fn legacy_status_failed(current: u8) -> u8 {
    current | STATUS_FAILED
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VirtioIoError {
    DeviceNotInitialized,
    BackendUnavailable,
    AddressTranslationFailed,
    Timeout,
    CompletionMismatch,
    InvalidBuffer,
    IoPathFault,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum VirtioPathState {
    Uninitialized = 0,
    TransportVisible = 1,
    BackendReady = 2,
    IoReady = 3,
}

impl VirtioPathState {
    fn from_u8(raw: u8) -> Self {
        match raw {
            1 => Self::TransportVisible,
            2 => Self::BackendReady,
            3 => Self::IoReady,
            _ => Self::Uninitialized,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Uninitialized => "uninitialized",
            Self::TransportVisible => "transport-visible",
            Self::BackendReady => "backend-ready",
            Self::IoReady => "io-ready",
        }
    }
}

impl VirtioIoError {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DeviceNotInitialized => "virtio-ffi: device not initialized",
            Self::BackendUnavailable => {
                "virtio-ffi: transport visible but data path backend is unavailable"
            }
            Self::AddressTranslationFailed => "virtio-ffi: virtual address translation failed",
            Self::Timeout => "virtio-ffi: request timed out while waiting completion",
            Self::CompletionMismatch => {
                "virtio-ffi: used-ring completion token mismatch on single-flight request"
            }
            Self::InvalidBuffer => "virtio-ffi: invalid sector buffer contract",
            Self::IoPathFault => "virtio-ffi: backend reported generic I/O path fault",
        }
    }
}

fn map_blk_error(err: &'static str) -> VirtioIoError {
    match err {
        "Timeout" => VirtioIoError::Timeout,
        "CompletionMismatch" => VirtioIoError::CompletionMismatch,
        "Invalid buffer size" => VirtioIoError::InvalidBuffer,
        "Device not initialized" => VirtioIoError::DeviceNotInitialized,
        "DmaAllocFailed" => VirtioIoError::IoPathFault,
        "DmaLayoutOverflow" => VirtioIoError::IoPathFault,
        "QueueSubmitFailed" => VirtioIoError::IoPathFault,
        "CompletionIoError" => VirtioIoError::IoPathFault,
        _ => VirtioIoError::IoPathFault,
    }
}

// ---------------------------------------------------------------------------
// C ABI baslatma koprusu.
// Gercek queue/DMA backend baglandiginda veri yolu bu katmandan acilabilir.
// ---------------------------------------------------------------------------

fn virtio_disk_init(base_port: u16) {
    crate::serial_println!(
        "VIRTIO FFI: Initializing virtio-blk at port {:#x}",
        base_port
    );
    let status_seq = legacy_status_sequence();
    // VirtIO cihaz başlatma:
    // 1. Status yazmacını sıfırla (reset)
    unsafe {
        use x86_64::instructions::port::Port;
        let mut status_port = Port::<u8>::new(base_port + LEGACY_STATUS_OFFSET);
        status_port.write(status_seq.reset); // Reset
                                             // 2. ACKNOWLEDGE bit ayarla
        status_port.write(status_seq.acknowledge);
        // 3. DRIVER bit ayarla
        status_port.write(status_seq.driver);
        // 4. Feature negotiation (basitleştirilmiş)
        let mut features_port = Port::<u32>::new(base_port as u16 + 4);
        let features = features_port.read();
        crate::serial_println!("VIRTIO FFI: Device features: {:#x}", features);
        // 5. FEATURES_OK ayarla
        status_port.write(status_seq.features_ok);
        // 6. Per VirtIO spec §3.1 step 6: re-read status to verify device accepted features
        // If device rejected, FAILED bit will be set
        for _ in 0..100 {
            let status = status_port.read();
            if (status & STATUS_FAILED) != 0 {
                crate::serial_println!("VIRTIO FFI: device rejected features (status={:#x})", status);
                return; // Device set FAILED — abort initialization
            }
            if (status & STATUS_FEATURES_OK) != 0 {
                break; // Device accepted
            }
            core::hint::spin_loop();
        }
        // 7. DRIVER_OK ayarla
        status_port.write(status_seq.driver_ok);
    }
    crate::serial_println!("VIRTIO FFI: virtio-blk initialized at {:#x}", base_port);
}

#[cfg(not(any(test, target_os = "windows")))]
fn read_legacy_status(base_port: u16) -> u8 {
    unsafe {
        use x86_64::instructions::port::Port;
        let mut status_port = Port::<u8>::new(base_port + LEGACY_STATUS_OFFSET);
        status_port.read()
    }
}

#[cfg(not(any(test, target_os = "windows")))]
fn write_legacy_status(base_port: u16, status: u8) {
    unsafe {
        use x86_64::instructions::port::Port;
        let mut status_port = Port::<u8>::new(base_port + LEGACY_STATUS_OFFSET);
        status_port.write(status);
    }
}

#[cfg(not(any(test, target_os = "windows")))]
fn mark_device_failed(base_port: u16, reason: &'static str) {
    if base_port == 0 {
        return;
    }
    let current = read_legacy_status(base_port);
    let failed = legacy_status_failed(current);
    write_legacy_status(base_port, failed);
    crate::serial_println!(
        "VIRTIO FFI: status->FAILED (reason={}, old={:#x}, new={:#x})",
        reason,
        current,
        failed
    );
}

fn virtio_disk_rw(
    base_port: u16,
    sector: u64,
    buf: *mut u8,
    write: i32,
) -> Result<(), VirtioIoError> {
    let _lock = LOCK.lock();
    if base_port == 0 {
        crate::serial_println!("VIRTIO FFI: disk op rejected, device not initialized");
        return Err(VirtioIoError::DeviceNotInitialized);
    }
    if buf.is_null() {
        return Err(VirtioIoError::AddressTranslationFailed);
    }
    if !BACKEND_READY.v.load(Ordering::Acquire) {
        let op = if write != 0 { "write" } else { "read" };
        crate::serial_println!(
            "VIRTIO FFI: disk {} sector={} buf={:p} rejected: backend not ready",
            op,
            sector,
            buf
        );
        return Err(VirtioIoError::BackendUnavailable);
    }
    if !IO_PATH_READY.v.load(Ordering::Acquire) && !refresh_io_path_state() {
        return Err(VirtioIoError::IoPathFault);
    }

    let sector_buf = unsafe { core::slice::from_raw_parts_mut(buf, 512) };
    let io_result = if write != 0 {
        super::virtio_blk::write_sector(sector, sector_buf).map_err(map_blk_error)
    } else {
        super::virtio_blk::read_sector(sector, sector_buf).map_err(map_blk_error)
    };
    if io_result.is_ok() {
        IO_PATH_READY.v.store(true, Ordering::Release);
        set_path_state(VirtioPathState::IoReady);
    } else {
        #[cfg(not(any(test, target_os = "windows")))]
        if matches!(
            io_result,
            Err(VirtioIoError::CompletionMismatch | VirtioIoError::IoPathFault)
        ) {
            mark_device_failed(base_port, "io-path-fatal");
        }
        IO_PATH_READY.v.store(false, Ordering::Release);
        set_path_state(VirtioPathState::BackendReady);
    }
    io_result
}

fn translate_phys_addr(ptr: *const u8) -> Result<u64, VirtioIoError> {
    #[cfg(any(test, target_os = "windows"))]
    {
        let _ = ptr;
        return Err(VirtioIoError::AddressTranslationFailed);
    }
    #[cfg(not(any(test, target_os = "windows")))]
    {
        crate::memory::translate_addr(ptr as u64).ok_or(VirtioIoError::AddressTranslationFailed)
    }
}

// ---------------------------------------------------------------------------
// Global durum değişkenleri
// LOCK: aynı anda yalnızca bir okuma/yazma operasyonuna izin verir (mutual exclusion)
// BASE_PORT: VirtIO cihazının I/O port adresi (atomic = iş parçacığı güvenli)
// ---------------------------------------------------------------------------

static LOCK: Mutex<()> = Mutex::new(());
#[repr(align(64))]
struct CacheLineAtomicU16 {
    v: AtomicU16,
}

impl CacheLineAtomicU16 {
    const fn new(value: u16) -> Self {
        Self {
            v: AtomicU16::new(value),
        }
    }
}

#[repr(align(64))]
struct CacheLineAtomicBool {
    v: AtomicBool,
}

impl CacheLineAtomicBool {
    const fn new(value: bool) -> Self {
        Self {
            v: AtomicBool::new(value),
        }
    }
}

#[repr(align(64))]
struct CacheLineAtomicU8 {
    v: AtomicU8,
}

impl CacheLineAtomicU8 {
    const fn new(value: u8) -> Self {
        Self {
            v: AtomicU8::new(value),
        }
    }
}

static BASE_PORT: CacheLineAtomicU16 = CacheLineAtomicU16::new(0);
static BACKEND_READY: CacheLineAtomicBool = CacheLineAtomicBool::new(false);
static IO_PATH_READY: CacheLineAtomicBool = CacheLineAtomicBool::new(false);
static PATH_STATE: CacheLineAtomicU8 = CacheLineAtomicU8::new(VirtioPathState::Uninitialized as u8);

fn set_path_state(state: VirtioPathState) {
    PATH_STATE.v.store(state as u8, Ordering::Release);
}

pub fn path_state() -> VirtioPathState {
    VirtioPathState::from_u8(PATH_STATE.v.load(Ordering::Acquire))
}

pub fn path_state_str() -> &'static str {
    path_state().as_str()
}

#[cfg(test)]
pub(crate) fn phase5_virtio_ffi_contract_green() -> bool {
    let seq = legacy_status_sequence();
    let status_handshake = seq.reset == 0
        && seq.acknowledge == STATUS_ACKNOWLEDGE
        && seq.driver == (STATUS_ACKNOWLEDGE | STATUS_DRIVER)
        && seq.features_ok == (STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK)
        && seq.driver_ok
            == (STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK | STATUS_DRIVER_OK);
    let failed_is_or_only = legacy_status_failed(seq.driver_ok) == (seq.driver_ok | STATUS_FAILED)
        && legacy_status_failed(STATUS_FAILED) == STATUS_FAILED;
    let typed_error_map = map_blk_error("Timeout") == VirtioIoError::Timeout
        && map_blk_error("CompletionMismatch") == VirtioIoError::CompletionMismatch
        && map_blk_error("DmaAllocFailed") == VirtioIoError::IoPathFault
        && map_blk_error("CompletionIoError") == VirtioIoError::IoPathFault;
    let cacheline_state = core::mem::align_of::<CacheLineAtomicU16>() == 64
        && core::mem::align_of::<CacheLineAtomicBool>() == 64
        && core::mem::align_of::<CacheLineAtomicU8>() == 64;

    status_handshake && failed_is_or_only && typed_error_map && cacheline_state
}

fn probe_io_path() -> Result<(), VirtioIoError> {
    #[cfg(any(test, target_os = "windows"))]
    {
        Err(VirtioIoError::BackendUnavailable)
    }
    #[cfg(not(any(test, target_os = "windows")))]
    {
        let mut probe = [0u8; 512];
        super::virtio_blk::read_sector(0, &mut probe).map_err(map_blk_error)
    }
}

fn refresh_io_path_state() -> bool {
    match probe_io_path() {
        Ok(()) => {
            IO_PATH_READY.v.store(true, Ordering::Release);
            set_path_state(VirtioPathState::IoReady);
            true
        }
        Err(err) => {
            IO_PATH_READY.v.store(false, Ordering::Release);
            set_path_state(VirtioPathState::BackendReady);
            crate::serial_println!("VIRTIO FFI: io probe failed: {}", err.as_str());
            false
        }
    }
}

/// Sanal adresi fiziksel adrese çeviren C-ABI fonksiyon.
///
/// `#[no_mangle]` ile Rust isim dönüşümü (name mangling) devre dışı bırakılır;
/// böylece C kodu bu fonksiyonu `virt_to_phys_c` ismiyle doğrudan çağırabilir.
///
/// `extern "C"` ile C çağrı kuralı (calling convention) kullanılır:
/// - Argümanlar: rdi, rsi, ... (x86-64 System V ABI)
/// - Dönüş değeri: rax
#[no_mangle]
pub extern "C" fn virt_to_phys_c(ptr: *const u8) -> u64 {
    let vaddr = ptr as u64;
    match translate_phys_addr(ptr) {
        Ok(paddr) => paddr,
        Err(err) => {
            crate::serial_println!(
                "[VIRTIO FFI] virt_to_phys_c failed for vaddr={:#x}: {}",
                vaddr,
                err.as_str()
            );
            INVALID_PHYS_ADDR
        }
    }
}

fn init_transport_backend(bus: u8, device: u8, function: u8) -> Result<(), VirtioIoError> {
    #[cfg(any(test, target_os = "windows"))]
    {
        let _ = (bus, device, function);
        return Err(VirtioIoError::BackendUnavailable);
    }
    #[cfg(not(any(test, target_os = "windows")))]
    {
        let mut root = super::pci_root::create_pci_root();
        let df = DeviceFunction {
            bus,
            device,
            function,
        };
        let transport = PciTransport::new::<super::virtio_hal::VirtioHal>(&mut root, df)
            .map_err(|_| VirtioIoError::BackendUnavailable)?;
        if super::virtio_blk::init(transport) {
            BACKEND_READY.v.store(true, Ordering::Release);
            Ok(())
        } else {
            Err(VirtioIoError::BackendUnavailable)
        }
    }
}

/// VirtIO FFI katmanını başlatır.
///
/// # Adımlar
/// 1. C ABI başlatma fonksiyonunu çağır
/// 2. BASE_PORT atomik değişkenine portu kaydet
///
/// Atomik `SeqCst` (Sequentially Consistent) sıralama kullanılır:
/// Bu en güçlü sıralamadır; tüm CPU çekirdekleri aynı sırayı görür.
pub fn init(bus: u8, device: u8, function: u8, base_port: u16) {
    crate::serial_println!("VIRTIO FFI: init base_port=0x{:x}", base_port);
    super::virtio_blk::reset();
    BACKEND_READY.v.store(false, Ordering::Release);
    IO_PATH_READY.v.store(false, Ordering::Release);
    #[cfg(any(test, target_os = "windows"))]
    {
        let _ = (bus, device, function);
        BASE_PORT.v.store(base_port, Ordering::SeqCst);
        if base_port == 0 {
            set_path_state(VirtioPathState::Uninitialized);
        } else {
            set_path_state(VirtioPathState::TransportVisible);
        }
        crate::serial_println!("VIRTIO FFI: host test target, port I/O disabled");
        return;
    }
    #[cfg(not(any(test, target_os = "windows")))]
    {
        unsafe {
            virtio_disk_init(base_port);
        }
        BASE_PORT.v.store(base_port, Ordering::SeqCst);
        if base_port == 0 {
            set_path_state(VirtioPathState::Uninitialized);
        } else {
            set_path_state(VirtioPathState::TransportVisible);
        }
        match init_transport_backend(bus, device, function) {
            Ok(()) => {
                set_path_state(VirtioPathState::BackendReady);
                crate::serial_println!(
                    "VIRTIO FFI: queue/DMA backend ready at {:02x}:{:02x}.{}",
                    bus,
                    device,
                    function
                );
                if refresh_io_path_state() {
                    crate::serial_println!("VIRTIO FFI: io path verified via sector probe");
                } else {
                    crate::serial_println!(
                        "VIRTIO FFI: backend ready but io path probe failed (fail-closed)"
                    );
                }
            }
            Err(err) => {
                #[cfg(not(any(test, target_os = "windows")))]
                mark_device_failed(base_port, "backend-init-failed");
                BACKEND_READY.v.store(false, Ordering::Release);
                IO_PATH_READY.v.store(false, Ordering::Release);
                crate::serial_println!(
                    "VIRTIO FFI: queue/DMA backend unavailable at {:02x}:{:02x}.{}: {}",
                    bus,
                    device,
                    function,
                    err.as_str()
                );
            }
        }
        crate::serial_println!("VIRTIO FFI: init done");
    }
}

/// Başlatılmış VirtIO blok cihazını döndürür.
///
/// BASE_PORT sıfırsa cihaz henüz başlatılmamıştır → None döner.
/// Bu `Option<T>` kalıbı, Rust'ta null pointer kullanımından kaçınmanın
/// idiomatik (deyimsel) yoludur.
pub fn device() -> Option<VirtioBlock> {
    let base_port = BASE_PORT.v.load(Ordering::SeqCst);
    if base_port == 0 {
        set_path_state(VirtioPathState::Uninitialized);
        None
    } else {
        crate::serial_println!(
            "VIRTIO FFI: transport visible at base_port=0x{:x}; path_state={}",
            base_port,
            path_state_str()
        );
        Some(VirtioBlock { base_port })
    }
}

pub fn reset() {
    let _lock = LOCK.lock();
    #[cfg(not(any(test, target_os = "windows")))]
    {
        let base_port = BASE_PORT.v.load(Ordering::Acquire);
        if base_port != 0 {
            write_legacy_status(base_port, 0);
        }
    }
    BASE_PORT.v.store(0, Ordering::SeqCst);
    BACKEND_READY.v.store(false, Ordering::Release);
    IO_PATH_READY.v.store(false, Ordering::Release);
    set_path_state(VirtioPathState::Uninitialized);
    super::virtio_blk::reset();
}

/// VirtIO blok cihazını temsil eden yapı.
///
/// Tek alan olan `base_port`, donanım registerlarına erişmek için
/// kullanılan I/O port tabanıdır.
pub struct VirtioBlock {
    base_port: u16,
}

impl VirtioBlock {
    /// Belirtilen sektörü okur.
    ///
    /// `buf`: 512 byte'lık sabit boyutlu dizi referansı ([u8; 512])
    /// Mutex kilidi alınarak eşzamanlı erişim engellenir.
    pub fn read_sector(&self, sector: u64, buf: &mut [u8; 512]) -> Result<(), VirtioIoError> {
        crate::serial_println!("VIRTIO FFI: read sector={}", sector);
        virtio_disk_rw(self.base_port, sector, buf.as_mut_ptr(), 0)
    }

    /// Belirtilen sektöre yazar.
    ///
    /// `buf`: değişmez (immutable) referans → `as *mut u8` cast'i gerekir
    /// write=1 parametresi C fonksiyonuna yazma işlemi olduğunu bildirir.
    pub fn write_sector(&self, sector: u64, buf: &[u8; 512]) -> Result<(), VirtioIoError> {
        crate::serial_println!("VIRTIO FFI: write sector={}", sector);
        virtio_disk_rw(self.base_port, sector, buf.as_ptr() as *mut u8, 1)
    }
}

/// rcore_fs `Device` trait implementasyonu.
///
/// Bu trait, dosya sistemi katmanının doğrudan ham sektörlere değil
/// byte offset üzerinden veri okumasını/yazmasını sağlar.
///
/// ## Sektör Hizalama Algoritması
///
/// ```
/// offset → sektör numarası: offset / 512
/// sektör içi offset: offset % 512
/// kopyalanacak byte: min(512 - within, kalan_buf)
/// ```
///
/// Döngü her iterasyonda en az 1 sektör işler ve `done` sayacını artırır.
impl Device for VirtioBlock {
    fn read_at(&self, offset: usize, buf: &mut [u8]) -> DevResult<usize> {
        crate::serial_println!("VIRTIO FFI: read_at offset={} len={}", offset, buf.len());
        let mut sector_buf = [0u8; 512];
        let mut done = 0usize;
        let mut cur_offset = offset;
        while done < buf.len() {
            let sector = (cur_offset / 512) as u64;
            let within = cur_offset % 512;
            self.read_sector(sector, &mut sector_buf)
                .map_err(|_| DevError)?;
            let to_copy = core::cmp::min(512 - within, buf.len() - done);
            buf[done..done + to_copy].copy_from_slice(&sector_buf[within..within + to_copy]);
            done += to_copy;
            cur_offset += to_copy;
        }
        Ok(done)
    }

    fn write_at(&self, offset: usize, buf: &[u8]) -> DevResult<usize> {
        crate::serial_println!("VIRTIO FFI: write_at offset={} len={}", offset, buf.len());
        let mut sector_buf = [0u8; 512];
        let mut done = 0usize;
        let mut cur_offset = offset;
        while done < buf.len() {
            let sector = (cur_offset / 512) as u64;
            let within = cur_offset % 512;
            // Önce mevcut sektörü oku (read-modify-write: kısmi yazma için gerekli)
            self.read_sector(sector, &mut sector_buf)
                .map_err(|_| DevError)?;
            let to_copy = core::cmp::min(512 - within, buf.len() - done);
            // Değiştirilecek kısmı güncelle
            sector_buf[within..within + to_copy].copy_from_slice(&buf[done..done + to_copy]);
            // Güncellenmiş sektörü geri yaz
            self.write_sector(sector, &sector_buf)
                .map_err(|_| DevError)?;
            done += to_copy;
            cur_offset += to_copy;
        }
        Ok(done)
    }

    /// Önbellek senkronizasyonu (bu implementasyonda gereksiz, her yazma anında senkron).
    fn sync(&self) -> DevResult<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use core::sync::atomic::Ordering;
    use spin::Mutex;

    use super::{
        path_state, reset, virt_to_phys_c, VirtioBlock, VirtioIoError, VirtioPathState,
        BACKEND_READY, BASE_PORT, INVALID_PHYS_ADDR,
    };

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn sector_io_reports_missing_backend() {
        let _guard = TEST_LOCK.lock();
        let block = VirtioBlock { base_port: 0x1000 };
        let mut buf = [0u8; 512];
        BACKEND_READY.v.store(false, Ordering::SeqCst);
        assert_eq!(
            block.read_sector(0, &mut buf),
            Err(VirtioIoError::BackendUnavailable)
        );
        assert_eq!(
            block.write_sector(0, &buf),
            Err(VirtioIoError::BackendUnavailable)
        );
    }

    #[test]
    fn virt_to_phys_returns_invalid_sentinel_when_unmapped() {
        let _guard = TEST_LOCK.lock();
        let bogus = usize::MAX as *const u8;
        assert_eq!(virt_to_phys_c(bogus), INVALID_PHYS_ADDR);
    }

    #[test]
    fn reset_clears_transport_and_state() {
        let _guard = TEST_LOCK.lock();
        BASE_PORT.v.store(0x1000, Ordering::SeqCst);
        BACKEND_READY.v.store(false, Ordering::SeqCst);
        reset();
        assert_eq!(BASE_PORT.v.load(Ordering::SeqCst), 0);
        assert_eq!(path_state(), VirtioPathState::Uninitialized);
    }

    #[test]
    fn host_init_marks_transport_visible_when_port_present() {
        let _guard = TEST_LOCK.lock();
        super::init(0, 0, 0, 0x1000);
        assert_eq!(path_state(), VirtioPathState::TransportVisible);
        reset();
    }

    #[test]
    fn host_init_keeps_uninitialized_when_port_missing() {
        let _guard = TEST_LOCK.lock();
        super::init(0, 0, 0, 0);
        assert_eq!(path_state(), VirtioPathState::Uninitialized);
    }

    #[test]
    fn map_blk_error_typed_io_faults_collapse_to_iopathfault() {
        let _guard = TEST_LOCK.lock();
        assert_eq!(
            super::map_blk_error("DmaAllocFailed"),
            VirtioIoError::IoPathFault
        );
        assert_eq!(
            super::map_blk_error("DmaLayoutOverflow"),
            VirtioIoError::IoPathFault
        );
        assert_eq!(
            super::map_blk_error("QueueSubmitFailed"),
            VirtioIoError::IoPathFault
        );
        assert_eq!(
            super::map_blk_error("CompletionIoError"),
            VirtioIoError::IoPathFault
        );
    }

    #[test]
    fn cacheline_wrappers_are_64byte_aligned() {
        let _guard = TEST_LOCK.lock();
        assert_eq!(core::mem::align_of::<super::CacheLineAtomicU16>(), 64);
        assert_eq!(core::mem::align_of::<super::CacheLineAtomicBool>(), 64);
        assert_eq!(core::mem::align_of::<super::CacheLineAtomicU8>(), 64);
    }

    #[test]
    fn phase5_virtio_ffi_contract_is_green() {
        let _guard = TEST_LOCK.lock();
        assert!(super::phase5_virtio_ffi_contract_green());
    }
}
