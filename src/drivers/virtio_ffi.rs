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

use core::sync::atomic::{AtomicU16, Ordering};
use rcore_fs::dev::{DevError, Device, Result as DevResult};
use spin::Mutex;

const INVALID_PHYS_ADDR: u64 = u64::MAX;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VirtioIoError {
    DeviceNotInitialized,
    BackendUnavailable,
    AddressTranslationFailed,
}

impl VirtioIoError {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DeviceNotInitialized => "virtio-ffi: device not initialized",
            Self::BackendUnavailable => {
                "virtio-ffi: transport visible but data path backend is unavailable"
            }
            Self::AddressTranslationFailed => "virtio-ffi: virtual address translation failed",
        }
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
    // VirtIO cihaz başlatma:
    // 1. Status yazmacını sıfırla (reset)
    unsafe {
        use x86_64::instructions::port::Port;
        let mut status_port = Port::<u8>::new(base_port + 18);
        status_port.write(0); // Reset
                              // 2. ACKNOWLEDGE bit ayarla
        status_port.write(1);
        // 3. DRIVER bit ayarla
        status_port.write(1 | 2);
        // 4. Feature negotiation (basitleştirilmiş)
        let mut features_port = Port::<u32>::new(base_port as u16 + 4);
        let features = features_port.read();
        crate::serial_println!("VIRTIO FFI: Device features: {:#x}", features);
        // 5. FEATURES_OK ayarla
        status_port.write(1 | 2 | 8);
        // 6. DRIVER_OK ayarla
        status_port.write(1 | 2 | 8 | 4);
    }
    crate::serial_println!("VIRTIO FFI: virtio-blk initialized at {:#x}", base_port);
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

    let op = if write != 0 { "write" } else { "read" };
    crate::serial_println!(
        "VIRTIO FFI: disk {} sector={} buf={:p} rejected: no real backend",
        op,
        sector,
        buf
    );
    Err(VirtioIoError::BackendUnavailable)
}

fn translate_phys_addr(ptr: *const u8) -> Result<u64, VirtioIoError> {
    crate::memory::translate_addr(ptr as u64).ok_or(VirtioIoError::AddressTranslationFailed)
}

// ---------------------------------------------------------------------------
// Global durum değişkenleri
// LOCK: aynı anda yalnızca bir okuma/yazma operasyonuna izin verir (mutual exclusion)
// BASE_PORT: VirtIO cihazının I/O port adresi (atomic = iş parçacığı güvenli)
// ---------------------------------------------------------------------------

static LOCK: Mutex<()> = Mutex::new(());
static BASE_PORT: AtomicU16 = AtomicU16::new(0);

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

/// VirtIO FFI katmanını başlatır.
///
/// # Adımlar
/// 1. C ABI başlatma fonksiyonunu çağır
/// 2. BASE_PORT atomik değişkenine portu kaydet
///
/// Atomik `SeqCst` (Sequentially Consistent) sıralama kullanılır:
/// Bu en güçlü sıralamadır; tüm CPU çekirdekleri aynı sırayı görür.
pub fn init(base_port: u16) {
    crate::serial_println!("VIRTIO FFI: init base_port=0x{:x}", base_port);
    unsafe {
        virtio_disk_init(base_port);
    }
    BASE_PORT.store(base_port, Ordering::SeqCst);
    crate::serial_println!("VIRTIO FFI: init done");
}

/// Başlatılmış VirtIO blok cihazını döndürür.
///
/// BASE_PORT sıfırsa cihaz henüz başlatılmamıştır → None döner.
/// Bu `Option<T>` kalıbı, Rust'ta null pointer kullanımından kaçınmanın
/// idiomatik (deyimsel) yoludur.
pub fn device() -> Option<VirtioBlock> {
    let base_port = BASE_PORT.load(Ordering::SeqCst);
    if base_port == 0 {
        None
    } else {
        crate::serial_println!(
            "VIRTIO FFI: transport visible at base_port=0x{:x}; data path requires a real backend",
            base_port
        );
        Some(VirtioBlock { base_port })
    }
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
    use super::{virt_to_phys_c, VirtioBlock, VirtioIoError, INVALID_PHYS_ADDR};

    #[test]
    fn sector_io_reports_missing_backend() {
        let block = VirtioBlock { base_port: 0x1000 };
        let mut buf = [0u8; 512];
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
        let bogus = usize::MAX as *const u8;
        assert_eq!(virt_to_phys_c(bogus), INVALID_PHYS_ADDR);
    }
}
