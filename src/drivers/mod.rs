//! # echOS Donanım Sürücüleri
//!
//! Bu modül, sistem donanım sürücülerini içerir.
//!
//! ## Sürücü Katmanı Mimarisi
//!
//! echOS'ta sürücüler iki katmanlı bir yapıda organize edilmiştir:
//!
//! ```
//!   Üst Katman (linux submodülü)
//!     |-- LinuxDriver trait  -> probe() + attach() arayüzü
//!     |-- DRIVER_REGISTRY    -> kayıtlı sürücüler listesi
//!     |-- DEVICE_REGISTRY    -> keşfedilen cihazlar listesi
//!     +-- probe_and_attach() -> sürücü-cihaz eşleştirme döngüsü
//!
//!   Alt Katman (donanım sürücüleri)
//!     |-- ps2   -> PS/2 klavye denetleyicisi (IRQ1)
//!     |-- mouse -> PS/2 mouse sürücüsü (IRQ12)
//!     |-- ata   -> IDE/ATA HDD sürücüsü (PIO mod, DMA yok)
//!     |-- apic  -> Yerel APIC ve IO-APIC yönetimi
//!     |-- pci   -> PCI konfigürasyon uzayı tarayıcı
//!     |-- usb   -> xHCI USB host controller
//!     |-- nvme  -> NVMe SSD sürücüsü (PCIe)
//!     +-- audio -> Intel HDA ses sürücüsü
//! ```
//!
//! ## Aygıt Başlatma Sırası
//!
//! `init_linux_driver_layer()` çağrıldığında şu adımlar gerçekleşir:
//!   1. PS/2 platform cihazları kayıt edilir (sabit major:minor numaralarıyla)
//!   2. PCI bus taranır; class_code'a göre cihazlar sınıflandırılır
//!   3. VirtIO blok/ağ cihazları legacy I/O BAR ile başlatılır
//!   4. Sürücüler kayıt edilir (probe_and_attach döngüsüne girer)
//!   5. Her sürücünün probe() fonksiyonu her cihaza karşı çalıştırılır
//!   6. Eşleşme varsa attach() çağrılır ve ATTACHMENTS listesine eklenir

/// Input event kuyruğu (keyboard, mouse): IRQ işleyicilerinden gelen olayları tampona alır
pub mod input;

/// SPSC Lock-Free Ring Buffer
pub mod spsc;

/// PS/2 controller sürücüsü: i8042 denetleyicisi aracılığıyla klavye/mouse iletişimi
pub mod ps2;

/// PS/2 mouse sürücüsü: IRQ12 ile mouse paketlerini alır ve pozisyonu günceller
pub mod mouse;

/// Gesture tanıma modülü
pub mod gesture;

/// ATA/IDE disk sürücüsü: PIO mod okuma/yazma (DMA yok, senkron)
pub mod ata;

/// Advanced PIC (Local APIC): modern x86 kesme kontrolörü; 8259 PIC'in yerine geçer
pub mod apic;

// PCI yapılandırma uzayı tarayıcısı ve BAR okuma yardımcıları
pub mod pci;
// PCI kök otobüs yöneticisi (PCI Root Bridge)
pub mod pci_root;

/// USB (xHCI) sürücüsü: USB 3.0 eXtensible Host Controller Interface
pub mod usb;

/// Audio (Intel HDA) sürücüsü: High Definition Audio codec yönetimi
pub mod audio;

/// AHCI sürücüsü: SATA disk erişimi (Intel ICH10 uyumlu)
pub mod ahci;

/// Bluetooth sürücüsü: HCI katmanı ve temel bağlantı yönetimi
pub mod bluetooth;

/// NVMe sürücüsü: PCIe üzerinden Non-Volatile Memory Express SSD erişimi
pub mod nvme;

/// VirtIO-Net network driver: QEMU/KVM sanal ağ kartı sürücüsü
pub mod virtio_net;

/// VirGL 3D acceleration: QEMU virgl aracılığıyla konaktan GPU komutları
pub mod virgl;

// VirtIO blok cihazı sürücüsü (disk R/W)
pub mod virtio_blk;
// VirtIO FFI köprüsü: C uyumlu VirtIO sürücü arayüzü
pub mod virtio_ffi;
// VirtIO GPU sürücüsü (sanal grafik kartı)
pub mod virtio_gpu;
// VirtIO HAL (Hardware Abstraction Layer): sanal DMA/bellek yönetimi
pub mod virtio_hal;

/// Blok cihaz soyutlaması: tüm disk türleri için ortak trait (BlockDevice)
pub mod block;

/// Sysfs-benzeri genel cihaz kaydı ve sürücü bağlama modeli
pub mod driver_model;

/// Dosya-backed image'leri genel blok aygıtı olarak sunan loopback sürücüsü
pub mod loopback;

/// RTC (Real-Time Clock) sürücsü: CMOS port 0x70/0x71 üzerinden tarih/saat okuma
pub mod rtc;

/// ACPI güç yönetimi sürücüsü: PM1a_CNT yazmaçları, S-state geçişleri,
/// cihaz askıya alma/devam ettirme zinciri
pub mod power;

/// İki Katmanlı Sürücü Kast Sistemi: PCI cihaz sınıflandırıcı (TIER 1 Native / TIER 2 Jail)
pub mod tier;

/// Lock-free async I/O trait tanımları: AsyncBlockDevice, AsyncNetDevice, AsyncGpuDevice
pub mod async_traits;

/// TIER 2 Jail SPSC Ring Buffer: Core↔Jail lock-free iletişim kanalı
pub mod jail_ring;

/// TIER 2 Jail Worker Thread: İzole sürücü çalıştırma ortamı
pub mod jail;

/// Jail↔Core Dispatcher: Otomatik TIER 1/TIER 2 sürücü yönlendirme
pub mod dispatcher;

/// Linux Driver Onboarding: Sürücü profilleme, ABI çeviri, yaşam döngüsü
pub mod linux_onboarding;

/// TIER 1 NIC Native Driver: Lock-free ağ kartı sürücüsü
pub mod nic_native;

/// TIER 2 USB XHCI Jail Driver: USB host controller jail sandbox
pub mod usb_jail;

/// TIER 1 GPU Native Driver: Lock-free GPU sürücüsü (AsyncGpuDevice)
pub mod gpu_native;

/// DRM/KMS ekran topolojisi ve atomic commit denetleyicisi
pub mod drm;

/// IOMMU/SVA/PASID/ATS/PRI bookkeeping ve DMA domain yönetimi
pub mod iommu;

/// TIER 2 USB Mass Storage Jail: BBB/SCSI protokol jail sandbox
pub mod usb_msc_jail;

/// TIER 2 Audio/ALSA Jail: Ses donanımı jail sandbox
pub mod audio_jail;

/// TIER 2 WiFi Jail: 802.11ax kablosuz ağ adaptörü jail sandbox
/// Wi-Fi Jail sürücüsü (TIER 2)
pub mod wifi_jail;

/// TIER 2 Bluetooth Jail: HCI transport, LE advertising, scanning
pub mod bluetooth_jail;

/// Watchdog — donanım/yazılım gözcüsü (NMI + soft watchdog)
pub mod watchdog;

/// PCI Express Hot-Plug — Native hot-plug ve surprise removal protokolü
pub mod pci_hotplug;

/// DMA Ownership Contract — unified lifecycle, bounce buffer, cache coherency
pub mod dma;

/// GPT Partition Parser — UEFI Spec 2.10 GUID Partition Table
pub mod gpt;

/// MBR Partition Parser — Master Boot Record partition table
pub mod mbr;

/// qcow2 Image Parser — QEMU Copy-On-Write v2/v3 image format
pub mod qcow2;

/// Block I/O scheduler layer: mq-deadline + blk-mq multi-queue dispatch
pub mod sched;

// Sık kullanılan blok cihaz türlerini doğrudan dışa aktar
pub use block::{BlockDevice, BlockDeviceError, BlockDeviceType};

pub mod linux {
    use crate::drivers::ata::{AtaDrive, BLOCK_SIZE};
    use crate::drivers::virtio_ffi;
    use crate::drivers::{mouse, pci, ps2, usb};
    use alloc::boxed::Box;
    use alloc::format;
    use alloc::string::{String, ToString};
    use alloc::vec;
    use alloc::vec::Vec;
    use core::sync::atomic::{AtomicBool, Ordering};
    use lazy_static::lazy_static;
    use spin::Mutex;

    // ============================================================================
    // LİNUX CİHAZ MODELİ (LINUX DEVICE MODEL)
    // ============================================================================

    // Bu alt modül, Linux çekirdek aygıt modelinin probe/attach akışını
    // doğrudan kayıt defteri ve sürücü eşlemesi üstünden uygular. Amacı:
    //   - PCI taramasından gelen cihazları kayıt etmek
    //   - Her cihaz için doğru sürücüyü bulmak (probe)
    //   - Bulunan sürücüyü başlatmak (attach)
    //
    // Cihaz türleri Linux major/minor numaralandırmasına uygundur:
    //   PS/2 klavye : major=10, minor=1  (misc device)
    //   PS/2 mouse  : major=13, minor=0  (input)
    //   Blok cihaz  : major=8, minor=0-N (sd, nvme...)

    /// Linux cihaz sınıflandırması: karakter, blok veya diğer
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum LinuxDeviceKind {
        Character, // /dev/tty, /dev/input/... byte bazlı erişim
        Block,     // /dev/sda, /dev/nvme0n1 sektör bazlı erişim
        Other,     // Ağ, ses vb. özel cihazlar
    }

    /// Sistem üzerinde keşfedilen cihaz kaydı.
    /// PCI taramasından veya platform (ACPI) tablolarından doldurulur.
    #[derive(Debug, Clone)]
    pub struct LinuxDevice {
        pub name: String, // Cihaz adı (örn. "pci-00:02.0")
        pub major: u16,   // Linux major numarası (cihaz türü)
        pub minor: u16,   // Linux minor numarası (cihaz örneği)
        pub kind: LinuxDeviceKind,
        pub bus: u8,        // PCI bus numarası
        pub device: u8,     // PCI cihaz numarası
        pub function: u8,   // PCI fonksiyon numarası
        pub class_code: u8, // PCI class (0x01=depolama, 0x0C=seri bus...)
        pub subclass: u8,   // PCI alt sınıf
        pub prog_if: u8,    // PCI programlama arayüzü
        pub vendor_id: u16, // PCI üretici ID (örn. 0x8086=Intel)
        pub device_id: u16, // PCI cihaz ID
    }

    /// Sürücü işlem hataları; Linux errno değerleriyle kavramsal uyum
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum LinuxDriverError {
        NotSupported, // -ENOSYS: donanım desteklenmiyor
        Io,           // -EIO:    I/O hatası
        Busy,         // -EBUSY:  cihaz meşgul
        Invalid,      // -EINVAL: geçersiz parametre
        NotFound,     // -ENODEV: cihaz bulunamadı
        Unknown,      // Bilinmeyen hata
    }

    /// Sürücü arayüzü: her sürücü bu trait'i uygular.
    ///
    /// Linux'taki `struct device_driver` yapısının Rust karşılığı:
    ///   probe()  -> cihazın bu sürücüyle uyumlu olup olmadığını kontrol eder
    ///   attach() -> sürücüyü başlatır, donanımı kullanıma hazırlar
    ///   detach() -> sürücüyü kapatır, kaynakları serbest bırakır
    pub trait LinuxDriver: Send + Sync {
        fn name(&self) -> &str;
        fn probe(&self, device: &LinuxDevice) -> bool;
        fn attach(&self, device: &LinuxDevice) -> Result<(), LinuxDriverError>;
        fn detach(&self, _device: &LinuxDevice) -> Result<(), LinuxDriverError> {
            Ok(())
        }
    }

    /// Blok cihaz okuma/yazma arayüzü (disk soyutlama katmanı)
    /// Deep web: Linux kernel include/linux/blkdev.h (REQ_PREFLUSH, REQ_FUA)
    pub trait BlockDevice: Send {
        fn read_sectors(&mut self, lba: u32, count: u8) -> Vec<u8>;
        fn write_sectors(&mut self, lba: u32, data: &[u8]) -> Result<(), ()>;

        /// Aygıtın flush destekleyip desteklemediğini söyler
        fn supports_flush(&self) -> bool {
            false
        }

        /// Aygıtın FUA destekleyip desteklemediğini söyler
        fn supports_fua(&self) -> bool {
            false
        }

        fn flush(&mut self) -> Result<(), ()> {
            if self.supports_flush() {
                Ok(())
            } else {
                Err(())
            }
        }
    }

    /// VirtIO blok cihazı sarmalayıcısı (QEMU sanal disk)
    pub struct VirtioBlockDevice {
        inner: virtio_ffi::VirtioBlock,
    }

    // ATA sürücüsü için BlockDevice trait implementasyonu
    impl BlockDevice for AtaDrive {
        fn read_sectors(&mut self, lba: u32, count: u8) -> Vec<u8> {
            AtaDrive::read_sectors(self, lba, count)
        }

        fn write_sectors(&mut self, lba: u32, data: &[u8]) -> Result<(), ()> {
            AtaDrive::write_sectors(self, lba, data).map_err(|_| ())
        }

        fn flush(&mut self) -> Result<(), ()> {
            AtaDrive::flush(self).map_err(|_| ())
        }
    }

    // VirtIO blok cihazı için BlockDevice trait implementasyonu.
    // Dahili olarak sector bazlı VirtIO FFI fonksiyonlarını kullanır.
    impl BlockDevice for VirtioBlockDevice {
        fn read_sectors(&mut self, lba: u32, count: u8) -> Vec<u8> {
            let mut buffer = Vec::with_capacity(count as usize * BLOCK_SIZE);
            for i in 0..count {
                let mut sector = [0u8; BLOCK_SIZE];
                if let Err(err) = self.inner.read_sector(lba as u64 + i as u64, &mut sector) {
                    crate::serial_println!(
                        "VIRTIO FFI: read_sectors aborted at lba={} err={}",
                        lba as u64 + i as u64,
                        err.as_str()
                    );
                    return Vec::new();
                }
                buffer.extend_from_slice(&sector);
            }
            buffer
        }

        fn write_sectors(&mut self, lba: u32, data: &[u8]) -> Result<(), ()> {
            if data.len() % BLOCK_SIZE != 0 {
                return Err(());
            }
            let count = data.len() / BLOCK_SIZE;
            for i in 0..count {
                let start = i * BLOCK_SIZE;
                let mut sector = [0u8; BLOCK_SIZE];
                sector.copy_from_slice(&data[start..start + BLOCK_SIZE]);
                self.inner
                    .write_sector(lba as u64 + i as u64, &sector)
                    .map_err(|err| {
                        crate::serial_println!(
                            "VIRTIO FFI: write_sectors aborted at lba={} err={}",
                            lba as u64 + i as u64,
                            err.as_str()
                        );
                    })?;
            }
            Ok(())
        }
    }

    /// Sürücü-cihaz eşleştirme kaydı: hangi cihaza hangi sürücünün bağlandığını tutar
    #[derive(Debug, Clone)]
    pub struct LinuxAttachment {
        pub device: String,
        pub driver: String,
    }

    lazy_static! {
        // Kayıtlı sürücüler listesi (probe_and_attach tarafından taranır)
        static ref DRIVER_REGISTRY: Mutex<Vec<Box<dyn LinuxDriver>>> = Mutex::new(Vec::new());
        // Keşfedilen cihazlar listesi (PCI/platform taramasından doldurulur)
        static ref DEVICE_REGISTRY: Mutex<Vec<LinuxDevice>> = Mutex::new(Vec::new());
        // Başarılı sürücü bağlamalarının kaydı (debug/status için)
        static ref ATTACHMENTS: Mutex<Vec<LinuxAttachment>> = Mutex::new(Vec::new());
    }
    // init_linux_driver_layer() yalnızca bir kez çalışmasını garantiler
    static INIT_DONE: AtomicBool = AtomicBool::new(false);

    /// Sürücüyü kayıt eder; probe_and_attach() döngüsüne dahil edilir
    pub fn register_driver(driver: Box<dyn LinuxDriver>) {
        DRIVER_REGISTRY.lock().push(driver);
    }

    /// Cihazı kayıt eder; döner: cihazın kayıt dizisindeki indeksi
    pub fn register_device(device: LinuxDevice) -> usize {
        let mut devices = DEVICE_REGISTRY.lock();
        devices.push(device);
        devices.len() - 1
    }

    /// Kayıtlı tüm cihazların kopyasını döner
    pub fn list_devices() -> Vec<LinuxDevice> {
        DEVICE_REGISTRY.lock().clone()
    }

    /// Kayıtlı sürücü adlarını listeler
    pub fn list_drivers() -> Vec<String> {
        DRIVER_REGISTRY
            .lock()
            .iter()
            .map(|driver| driver.name().to_string())
            .collect()
    }

    /// Başarılı bağlamaların (attachment) listesini döner
    pub fn list_attachments() -> Vec<LinuxAttachment> {
        ATTACHMENTS.lock().clone()
    }

    pub const PHASE5_KICKOFF_FIDELITY_BOUNDARY: &str =
        "phase5_kickoff=host-corpus-green pcie_msi=partial iommu=partial virtio_rw_completion=partial boundary=real-hardware-trace-open";

    pub fn phase5_kickoff_fidelity_boundary() -> &'static str {
        PHASE5_KICKOFF_FIDELITY_BOUNDARY
    }

    /// Uygun blok cihazı seçer: önce VirtIO, bulamazsa ATA'ya düşer.
    ///
    /// VirtIO seçimi:
    ///   vendor_id=0x1AF4 (Red Hat/QEMU), device_id=0x1001 veya 0x1042
    ///   MBR geçerliliği (sektör 0'ın son 2 byte'ı 0x55 0xAA olmalı) doğrulanır
    pub fn select_block_device() -> Result<Box<dyn BlockDevice>, LinuxDriverError> {
        let devices = list_devices();
        crate::debug::serial::trace_raw(format_args!("[BLK] device count: {}\n", devices.len()));
        for device in devices.iter() {
            crate::debug::serial::trace_raw(format_args!("[BLK] {} kind={:?} ven={:04x} dev={:04x}\n", device.name, device.kind, device.vendor_id, device.device_id));
        }
        let virtio_devices: Vec<&LinuxDevice> = devices
            .iter()
            .filter(|device| {
                device.kind == LinuxDeviceKind::Block
                    && device.vendor_id == 0x1AF4
                    && (device.device_id == 0x1001 || device.device_id == 0x1042)
            })
            .collect();
        if virtio_devices.is_empty() {
            crate::debug::serial::trace_raw(format_args!("[BLK] virtio block not found, trying AHCI/NVMe/ATA\n"));
            return try_ata_block_device();
        }
        crate::debug::serial::trace_raw(format_args!("[BLK] virtio block found: {}\n", virtio_devices.len()));
        let mut probe = [0u8; BLOCK_SIZE];
        let virtio = match virtio_ffi::device() {
            Some(dev) => dev,
            None => {
                crate::debug::serial::trace_raw(format_args!("[BLK] VirtIO FFI not initialized\n"));
                return try_ata_block_device().map_err(|_| LinuxDriverError::NotFound);
            }
        };
        if let Err(err) = virtio.read_sector(0, &mut probe) {
            crate::debug::serial::trace_raw(format_args!("[BLK] VirtIO probe read failed: {}\n", err.as_str()));
            return try_ata_block_device().map_err(|_| LinuxDriverError::Io);
        }
        if probe[510] != 0x55 || probe[511] != 0xAA {
            crate::debug::serial::trace_raw(format_args!("[BLK] MBR signature invalid\n"));
            return try_ata_block_device().map_err(|_| LinuxDriverError::Io);
        }
        crate::serial_println!("BLOCK DEVICE MBR OK: 0x55AA");
        crate::serial_println!("BLOCK DEVICE INIT OK");
        Ok(Box::new(VirtioBlockDevice { inner: virtio }))
    }

    /// VirtIO bulunamazsa AHCI, NVMe, sonra ATA/IDE denetleyicisini dener.
    /// Sıralama: VirtIO -> AHCI -> NVMe -> ATA (en hızlıdan en yavaşa)
    fn try_ata_block_device() -> Result<Box<dyn BlockDevice>, LinuxDriverError> {
        crate::debug::serial::trace_raw(format_args!("[BLK] fallback: trying AHCI\n"));
        match try_ahci_block_device() {
            Ok(dev) => {
                crate::debug::serial::trace_raw(format_args!("[BLK] AHCI OK\n"));
                return Ok(dev);
            }
            Err(e) => {
                crate::debug::serial::trace_raw(format_args!("[BLK] AHCI failed: {:?}\n", e));
            }
        }

        crate::debug::serial::trace_raw(format_args!("[BLK] fallback: trying NVMe\n"));
        match try_nvme_block_device() {
            Ok(dev) => {
                crate::debug::serial::trace_raw(format_args!("[BLK] NVMe OK\n"));
                return Ok(dev);
            }
            Err(e) => {
                crate::debug::serial::trace_raw(format_args!("[BLK] NVMe failed: {:?}\n", e));
            }
        }

        // NVMe bulunamazsa ATA dene
        #[cfg(any(test, target_os = "windows"))]
        {
            crate::serial_println!(
                "BLOCK DEVICE FALLBACK: ATA skipped on host/test without ring0 port I/O"
            );
            return Err(LinuxDriverError::NotFound);
        }

        #[cfg(not(any(test, target_os = "windows")))]
        {
            crate::serial_println!("BLOCK DEVICE FALLBACK: trying ATA");
            let mut drive = AtaDrive::new(0x1F0); // Primary ATA I/O portu: 0x1F0
            match drive.detect() {
                Ok(true) => {
                    crate::serial_println!("BLOCK DEVICE ATA OK: drive detected");
                    Ok(Box::new(drive))
                }
                Ok(false) => {
                    crate::serial_println!("BLOCK DEVICE ATA NOT FOUND: no drive");
                    Err(LinuxDriverError::NotFound)
                }
                Err(e) => {
                    crate::serial_println!("BLOCK DEVICE ATA ERROR: {:?}", e);
                    Err(LinuxDriverError::Io)
                }
            }
        }
    }

    /// AHCI blok cihazı dener. SATA diskler için.
    fn try_ahci_block_device() -> Result<Box<dyn BlockDevice>, LinuxDriverError> {
        match crate::drivers::ahci::AhciBlockDevice::new() {
            Some(dev) => {
                crate::serial_println!("AHCI: found SATA disk");
                Ok(Box::new(AhciBlockDeviceWrapper {
                    inner: Mutex::new(dev),
                }))
            }
            None => {
                crate::serial_println!("AHCI: no SATA disk found");
                Err(LinuxDriverError::NotFound)
            }
        }
    }

    /// AHCI blok cihazı wrapper'ı - linux::BlockDevice trait'i için
    pub struct AhciBlockDeviceWrapper {
        inner: Mutex<crate::drivers::ahci::AhciBlockDevice>,
    }

    impl BlockDevice for AhciBlockDeviceWrapper {
        fn read_sectors(&mut self, lba: u32, count: u8) -> Vec<u8> {
            let mut buffer = vec![0u8; count as usize * BLOCK_SIZE];
            let mut dev = self.inner.lock();
            for i in 0..count as u64 {
                let offset = (i as usize) * BLOCK_SIZE;
                let sector_buf = &mut buffer[offset..offset + BLOCK_SIZE];
                let _ = crate::drivers::block::BlockDevice::read_block(
                    &mut *dev,
                    lba as u64 + i,
                    sector_buf,
                );
            }
            buffer
        }

        fn write_sectors(&mut self, lba: u32, data: &[u8]) -> Result<(), ()> {
            if data.len() % BLOCK_SIZE != 0 {
                return Err(());
            }
            let mut dev = self.inner.lock();
            let count = (data.len() / BLOCK_SIZE) as u8;
            for i in 0..count as u64 {
                let offset = (i as usize) * BLOCK_SIZE;
                let sector_data = &data[offset..offset + BLOCK_SIZE];
                let _ = crate::drivers::block::BlockDevice::write_block(
                    &mut *dev,
                    lba as u64 + i,
                    sector_data,
                );
            }
            Ok(())
        }
    }

    /// NVMe blok cihazı dener. Varsayılan controller ve nsid=1 kullanır.
    fn try_nvme_block_device() -> Result<Box<dyn BlockDevice>, LinuxDriverError> {
        let nsid = 1u32;
        crate::debug::serial::trace_raw(format_args!("[BLK] NVMe: probing namespace {}\n", nsid));
        let ctrl_count = crate::drivers::nvme::controller_count();
        crate::debug::serial::trace_raw(format_args!("[BLK] NVMe: controller count={}\n", ctrl_count));
        match crate::drivers::nvme::get_namespace_info(nsid) {
            Some((block_size, block_count, _capacity)) => {
                crate::debug::serial::trace_raw(format_args!("[BLK] NVMe: ns={} blocks={} size={}\n", nsid, block_count, block_size));
                Ok(Box::new(NvmeBlockDeviceWrapper { nsid }))
            }
            None => {
                crate::debug::serial::trace_raw(format_args!("[BLK] NVMe: no namespace found\n"));
                Err(LinuxDriverError::NotFound)
            }
        }
    }

    /// NVMe blok cihazı wrapper'ı - linux::BlockDevice trait'i için
    pub struct NvmeBlockDeviceWrapper {
        nsid: u32,
    }

    impl BlockDevice for NvmeBlockDeviceWrapper {
        fn read_sectors(&mut self, lba: u32, count: u8) -> Vec<u8> {
            let mut buffer = vec![0u8; count as usize * BLOCK_SIZE];
            crate::debug::serial::trace_raw(format_args!("[BLK] NVMe read_sectors: nsid={} lba={} count={}\n", self.nsid, lba, count));
            match crate::drivers::nvme::read(self.nsid, lba as u64, count as u16, &mut buffer) {
                Ok(()) => {
                    crate::debug::serial::trace_raw(format_args!("[BLK] NVMe read OK: {} bytes\n", buffer.len()));
                    buffer
                }
                Err(e) => {
                    crate::debug::serial::trace_raw(format_args!("[BLK] NVMe read failed: nsid={} lba={} err={:?}\n", self.nsid, lba, e));
                    Vec::new()
                }
            }
        }

        fn write_sectors(&mut self, lba: u32, data: &[u8]) -> Result<(), ()> {
            if data.len() % BLOCK_SIZE != 0 {
                return Err(());
            }
            let blocks = (data.len() / BLOCK_SIZE) as u16;
            crate::drivers::nvme::write(self.nsid, lba as u64, blocks, data).map_err(|e| {
                crate::debug::serial::trace_raw(format_args!("[BLK] NVMe write failed: nsid={} lba={} err={:?}\n", self.nsid, lba, e));
            })
        }

        fn flush(&mut self) -> Result<(), ()> {
            crate::drivers::nvme::flush(self.nsid).map_err(|e| {
                crate::debug::serial::trace_raw(format_args!("[BLK] NVMe flush failed: nsid={} err={:?}\n", self.nsid, e));
            })
        }
    }

    /// Sürücü-cihaz eşleştirme döngüsü.
    ///
    /// Her kayıtlı sürücü, her kayıtlı cihaza karşı test edilir:
    ///   1. driver.probe(device) -> uyum var mı?
    ///   2. driver.attach(device) -> sürücüyü başlat
    ///   3. Başarılı ise ATTACHMENTS listesine ekle
    ///
    /// Döner: başarılı bağlama (attachment) sayısı
    pub fn probe_and_attach() -> usize {
        let devices = DEVICE_REGISTRY.lock().clone();
        let drivers = DRIVER_REGISTRY.lock();
        let mut attached = 0;
        let mut attachments = ATTACHMENTS.lock();
        for device in devices.iter() {
            for driver in drivers.iter() {
                crate::serial_println!(
                    "DEBUG: Calling probe for driver {} on device {}",
                    driver.name(),
                    device.name
                );
                let probe_ok = driver.probe(device);
                crate::serial_println!(
                    "DEBUG: Probe returned for driver {} on device {}",
                    driver.name(),
                    device.name
                );
                if probe_ok {
                    if device.kind == LinuxDeviceKind::Block || device.class_code == 0x01 {
                        crate::serial_println!(
                            "TRYING TO INIT DRIVER for device {} driver {}",
                            device.name,
                            driver.name()
                        );
                    }
                    if driver.attach(device).is_ok() {
                        attached += 1;
                        attachments.push(LinuxAttachment {
                            device: device.name.clone(),
                            driver: driver.name().to_string(),
                        });
                    }
                }
            }
        }
        attached
    }

    // ============================================================================
    // PLATFORM SÜRÜCÜ UYGULAMALARI (PLATFORM DRIVER IMPLEMENTATIONS)
    // ============================================================================

    // Her platform sürücüsü LinuxDriver trait'ini uygular.
    // probe(): cihaz adı veya PCI kodu eşleşmesini kontrol eder
    // attach(): donanımı başlatır

    /// PS/2 i8042 kontrol cihazı sürücüsü
    struct Ps2ControllerDriver;

    impl LinuxDriver for Ps2ControllerDriver {
        fn name(&self) -> &str {
            "ps2_controller"
        }

        fn probe(&self, device: &LinuxDevice) -> bool {
            // Platforma kayıtlı "ps2" isimli cihazı tanır
            device.name == "ps2"
        }

        fn attach(&self, _device: &LinuxDevice) -> Result<(), LinuxDriverError> {
            if ps2::init() {
                Ok(())
            } else {
                Err(LinuxDriverError::Io)
            }
        }
    }

    /// PS/2 mouse sürücüsü (IRQ12 tabanlı)
    struct Ps2MouseDriver;

    impl LinuxDriver for Ps2MouseDriver {
        fn name(&self) -> &str {
            "ps2_mouse"
        }

        fn probe(&self, device: &LinuxDevice) -> bool {
            // "ps2-mouse" platform cihazını tanır
            device.name == "ps2-mouse"
        }

        fn attach(&self, _device: &LinuxDevice) -> Result<(), LinuxDriverError> {
            if mouse::init() {
                Ok(())
            } else {
                Err(LinuxDriverError::Io)
            }
        }
    }

    /// ATA PIO sürücüsü: PATA hard disk okuma/yazma (non-DMA)
    struct AtaPioDriver;

    impl LinuxDriver for AtaPioDriver {
        fn name(&self) -> &str {
            "ata_pio"
        }

        fn probe(&self, device: &LinuxDevice) -> bool {
            device.name == "ata0"
        }

        fn attach(&self, _device: &LinuxDevice) -> Result<(), LinuxDriverError> {
            let mut drive = AtaDrive::new(0x1F0);
            match drive.detect() {
                Ok(true) => Ok(()),
                Ok(false) => Err(LinuxDriverError::NotFound),
                Err(_) => Err(LinuxDriverError::Io),
            }
        }
    }

    /// PCI depolama cihazı sürücüsü: class_code=0x01 olan tüm PCI cihazları
    struct StoragePciDriver;

    impl LinuxDriver for StoragePciDriver {
        fn name(&self) -> &str {
            "storage_pci"
        }

        fn probe(&self, device: &LinuxDevice) -> bool {
            // PCI class 0x01 = Mass Storage Controller
            device.class_code == 0x01
        }

        fn attach(&self, _device: &LinuxDevice) -> Result<(), LinuxDriverError> {
            Ok(())
        }
    }

    /// xHCI (USB 3.0) PCI sürücüsü: class=0x0C (seri bus), sub=0x03 (USB), prog_if=0x30 (xHCI)
    struct XhciPciDriver;

    impl LinuxDriver for XhciPciDriver {
        fn name(&self) -> &str {
            "xhci_pci"
        }

        fn probe(&self, device: &LinuxDevice) -> bool {
            // PCI: class=0x0C (Serial Bus), subclass=0x03 (USB), prog_if=0x30 (xHCI)
            device.class_code == 0x0C && device.subclass == 0x03 && device.prog_if == 0x30
        }

        fn attach(&self, _device: &LinuxDevice) -> Result<(), LinuxDriverError> {
            usb::init();
            Ok(())
        }
    }

    /// PCI bus tarandıktan sonra bulunan cihazları kayıt eder.
    ///
    /// - IDE (subclass=0x01) denetleyicileri atlanır (ATA PIO ile yönetilir)
    /// - VirtIO blok cihazları (vendor=0x1AF4) için legacy I/O BAR başlatılır
    /// - Her cihaz için LinuxDevice kaydı oluşturulur
    fn register_pci_devices() {
        let mut storage_minor: u16 = 0;
        for dev in pci::scan() {
            crate::serial_println!(
                "PCI DEVICE: {:02x}:{:02x}.{} vendor={:04x} device={:04x} class={:02x} subclass={:02x} prog_if={:02x}",
                dev.bus,
                dev.device,
                dev.function,
                dev.vendor_id,
                dev.device_id,
                dev.class_code,
                dev.subclass,
                dev.prog_if
            );
            // IDE denetleyicisi (subclass=0x01): ATA PIO sürücüsü yönetir, burada atla
            if dev.class_code == 0x01 && dev.subclass == 0x01 {
                crate::serial_println!(
                    "Skipping IDE controller {:02x}:{:02x}.{}",
                    dev.bus,
                    dev.device,
                    dev.function
                );
                continue;
            }
            // VirtIO blok cihazı başlatma:
            // Bit 0: I/O Space Enable, Bit 1: Memory Space, Bit 2: Bus Master (DMA için)
            if dev.vendor_id == 0x1AF4 && (dev.device_id == 0x1001 || dev.device_id == 0x1042) {
                let mut command = pci::read_config_dword(dev.bus, dev.device, dev.function, 0x04);
                command |= (1 << 0) | (1 << 1) | (1 << 2);
                pci::write_config_dword(dev.bus, dev.device, dev.function, 0x04, command);
                if let Some(bar) = pci::read_bar_io(dev.bus, dev.device, dev.function, 0) {
                    if bar.base != 0 {
                        virtio_ffi::init(dev.bus, dev.device, dev.function, bar.base as u16);
                        crate::serial_println!(
                            "VIRTIO BLK: init via legacy io base=0x{:x} state={}",
                            bar.base,
                            virtio_ffi::path_state_str()
                        );
                    } else {
                        crate::serial_println!("VIRTIO BLK: io bar base is zero");
                    }
                } else {
                    crate::serial_println!("VIRTIO BLK: io bar not found");
                }
            }
            pci::init_driver(&dev);
            if dev.class_code == 0x01 {
                crate::serial_println!(
                    "PCI STORAGE FOUND: Bus/Dev/Fn {:02x}:{:02x}.{} ID={:04x}:{:04x}",
                    dev.bus,
                    dev.device,
                    dev.function,
                    dev.vendor_id,
                    dev.device_id
                );
            }
            // PCI class'a göre cihaz türü belirlenir
            let kind = match dev.class_code {
                0x01 => LinuxDeviceKind::Block,     // Depolama (ATA, NVMe, SCSI)
                0x0C => LinuxDeviceKind::Character, // Seri bus (USB, FireWire)
                _ => LinuxDeviceKind::Other,
            };
            register_device(LinuxDevice {
                name: format!("pci-{:02x}:{:02x}.{}", dev.bus, dev.device, dev.function),
                major: 0,
                minor: 0,
                kind,
                bus: dev.bus,
                device: dev.device,
                function: dev.function,
                class_code: dev.class_code,
                subclass: dev.subclass,
                prog_if: dev.prog_if,
                vendor_id: dev.vendor_id,
                device_id: dev.device_id,
            });
            // Depolama cihazı için ikinci bir kayıt: major=8 (block cihaz numarası)
            if dev.class_code == 0x01 {
                register_device(LinuxDevice {
                    name: format!("block-{:02x}:{:02x}.{}", dev.bus, dev.device, dev.function),
                    major: 8,
                    minor: storage_minor,
                    kind: LinuxDeviceKind::Block,
                    bus: dev.bus,
                    device: dev.device,
                    function: dev.function,
                    class_code: dev.class_code,
                    subclass: dev.subclass,
                    prog_if: dev.prog_if,
                    vendor_id: dev.vendor_id,
                    device_id: dev.device_id,
                });
                storage_minor = storage_minor.saturating_add(1);
            }
        }
    }

    /// Linux sürücü katmanını başlatır.
    ///
    /// INIT_DONE atomik bayrağıyla çift başlatma önlenir.
    /// Döner: başarılı probe_and_attach sayısı (0 = tüm sürücüler başlatılamadı)
    pub fn init_linux_driver_layer() -> usize {
        if INIT_DONE.swap(true, Ordering::SeqCst) {
            return 0; // Daha önce çağrıldı; yeniden başlatma atlandı
        }
        // Platform cihazlarını kayıt et (PCI'dan farklı; ACPI/sabit cihazlar)
        register_device(LinuxDevice {
            name: "ps2".to_string(),
            major: 10, // misc device major
            minor: 1,
            kind: LinuxDeviceKind::Character,
            bus: 0,
            device: 0,
            function: 0,
            class_code: 0,
            subclass: 0,
            prog_if: 0,
            vendor_id: 0,
            device_id: 0,
        });
        register_device(LinuxDevice {
            name: "ps2-mouse".to_string(),
            major: 13, // input device major
            minor: 0,
            kind: LinuxDeviceKind::Character,
            bus: 0,
            device: 0,
            function: 0,
            class_code: 0,
            subclass: 0,
            prog_if: 0,
            vendor_id: 0,
            device_id: 0,
        });
        // PCI bus'u tara ve cihazları kayıt et
        register_pci_devices();
        // Sürücüleri kayıt et (probe sırası önemli değil, hepsi denenir)
        register_driver(Box::new(Ps2ControllerDriver));
        register_driver(Box::new(Ps2MouseDriver));
        register_driver(Box::new(StoragePciDriver));
        register_driver(Box::new(XhciPciDriver));
        // NVMe alt sistemini başlat (controller keşif + namespace discovery)
        crate::debug::serial::trace_raw(format_args!("[DRV] NVMe subsystem init...\n"));
        crate::drivers::nvme::init();
        let ns_info = crate::drivers::nvme::get_namespace_info(1);
        crate::debug::serial::trace_raw(format_args!("[DRV] NVMe namespace 1 info: {:?}\n", ns_info));
        // Probe+attach döngüsünü çalıştır ve başarılı bağlama sayısını döndür
        probe_and_attach()
    }
}

#[cfg(test)]
mod phase5_kickoff_tests {
    #[test]
    fn phase5_kickoff_corpus_is_green() {
        assert!(
            super::pci::phase5_kickoff_contract_green(),
            "PCIe/MSI kickoff corpus not green"
        );
        assert!(
            super::iommu::phase5_kickoff_contract_green(),
            "IOMMU kickoff corpus not green"
        );
        assert!(
            super::virtio_blk::phase5_kickoff_contract_green(),
            "VirtIO kickoff corpus not green"
        );
    }
}
