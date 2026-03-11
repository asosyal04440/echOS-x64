//! # echOS VirtIO-Net Sürücüsü
//!
//! VirtIO protokolü üzerinden çalışan ağ kartı sürücüsü.
//! TX/RX halka tamponları kullanır ve TLSF heap yerine doğrudan
//! fiziksel bellek (DMA-safe) ile çalışır.
//!
//! ## VirtIO-Net Katman Mimarisi
//!
//! ```
//!  ┌───────────────────────────────────────────────────────┐
//!  │  Uygulama / Ağ Yığını (TCP/IP, ARP, DHCP ...)        │
//!  ├───────────────────────────────────────────────────────┤
//!  │  send_packet() / recv_packet() / poll_rx()            │
//!  ├───────────────────────────────────────────────────────┤
//!  │  VirtioNetDevice (TX kuyruğu + RX kuyruğu)           │
//!  ├───────────────────────────────────────────────────────┤
//!  │  VirtIONet<VirtioHal, PciTransport, 16>              │
//!  │  (virtio_drivers Rust kütüphanesi)                   │
//!  ├───────────────────────────────────────────────────────┤
//!  │  VirtIO Virtqueue  →  PCI Bus  →  QEMU/Simics TAP    │
//!  └───────────────────────────────────────────────────────┘
//! ```
//!
//! ## DMA-Safe Halka Tamponu
//!
//! Normal heap (TLSF) fiziksel adres süreklilik garantisi vermediğinden
//! donanım DMA'sı için kullanılamaz. `DmaRingBuffer`, DMA havuzundan tahsis
//! edilmiş sabit bir fiziksel bellek bölgesinde çalışır.
//!
//! ```
//! Halka Tamponu Bellek Düzeni:
//!   paddr (fiziksel)
//!   ┌────────────────────────────────────────┐
//!   │ slot[0]: [len:usize | data: packet_size] │
//!   │ slot[1]: [len:usize | data: packet_size] │
//!   │ ...                                      │
//!   │ slot[255]: [len | data]                  │
//!   └──────────────────────────────────────────┘
//!     read_idx → tüketici konumu
//!     write_idx → üretici konumu
//! ```
//!
//! ## MAC Adresi
//!
//! VirtIO-Net cihazı GPU'ya benzer şekilde özellik müzakeresi (feature
//! negotiation) yapar. `VIRTIO_NET_F_MAC` özelliği varsa cihaz kendine
//! özgü 6-byte Ethernet MAC adresini bildirir.
//!
//! ## TX/RX Veri Akışı
//!
//! ```
//! TX: send() → TxBuffer::from(data) → driver.send() → PCI → Ağ
//! RX: driver.receive() → RxBuffer → NetPacket → rx_queue
//!   poll_rx()  →  rx_queue doldurulur
//!   pop_rx()   →  rx_queue'dan paket alınır
//! ```
//!
//! ## DMA Domain Geçişi
//!
//! VirtIONet çağrıları sırasında doğru DMA domain'i seçilmelidir.
//! Bu yüzden `send()`/`recv()` öncesi `set_current_dma_domain()` çağrılır
//! ve işlem sonrası önceki domain geri yüklenir (RAII benzeri pattern).

use alloc::collections::VecDeque;
use alloc::vec;
use alloc::vec::Vec;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use spin::Mutex;

use virtio_drivers::device::net::{TxBuffer, VirtIONet};
use virtio_drivers::transport::pci::PciTransport;

use super::virtio_hal::VirtioHal;
use crate::net::{MacAddr, NetError};

// ============================================================================
// VIRTIO-NET SABİTLERİ
// ============================================================================

/// Maksimum TX (gönderme) kuyruğu derinliği
const TX_QUEUE_SIZE: usize = 256;
/// Maksimum RX (alma) kuyruğu derinliği
const RX_QUEUE_SIZE: usize = 256;
/// Maksimum paket boyutu: standart Ethernet MTU (1500) + başlık (14) = 1514
const MAX_PACKET_SIZE: usize = 1514;
/// IEEE 802.3 minimumu: 64 byte (padding ile)
const MIN_PACKET_SIZE: usize = 64;

// VirtIO-Net özellik bitleri (VirtIO spesifikasyonu Bölüm 5.1.3)
const VIRTIO_NET_F_CSUM: u64 = 1 << 0; // Donanım checksum hesaplama
const VIRTIO_NET_F_GUEST_CSUM: u64 = 1 << 1; // Misafir checksum kısmi
const VIRTIO_NET_F_MAC: u64 = 1 << 5; // Cihazın MAC adresi var
const VIRTIO_NET_F_GSO: u64 = 1 << 6; // Generic Segmentation Offload
const VIRTIO_NET_F_GUEST_TSO4: u64 = 1 << 7; // Misafir IPv4 TCP Segmentation Offload
const VIRTIO_NET_F_GUEST_TSO6: u64 = 1 << 8; // Misafir IPv6 TCP Segmentation Offload
const VIRTIO_NET_F_CTRL_VQ: u64 = 1 << 17; // Kontrol kuyruğu mevcut
const VIRTIO_NET_F_CTRL_RX: u64 = 1 << 18; // Kontrol kuyruğu RX modu değiştirme
const VIRTIO_NET_F_CTRL_VLAN: u64 = 1 << 19; // VLAN filtreleme
const VIRTIO_NET_F_MQ: u64 = 1 << 22; // Çok kuyruklu (multiqueue)

// ============================================================================
// DMA-SAFE HALKA TAMPONU
// TLSF heap atlanır, fiziksel bellek doğrudan kullanılır
// ============================================================================

/// DMA uyumlu halka (ring) tamponu.
///
/// Donanım DMA'sı için fiziksel adres sürekli bellek gereklidir.
/// Bu yapı echOS'un DMA havuzundan tahsis edilmiş bellek üzerinde çalışır.
///
/// ## Ring Buffer Algoritması
///
/// ```
/// Boş:    read_idx == write_idx, count == 0
/// Dolu:   count == RX_QUEUE_SIZE
///
/// push(): write_idx = (write_idx + 1) % RX_QUEUE_SIZE
/// pop():  read_idx = (read_idx + 1) % RX_QUEUE_SIZE
/// ```
pub struct DmaRingBuffer {
    /// Tampon başlangıç adresi (sanal, CPU erişimi için)
    base: *mut u8,
    /// Tampon fiziksel adresi (donanım DMA için)
    paddr: usize,
    /// Tampon toplam boyutu (byte)
    size: usize,
    /// Okuma konumu (tüketici indeksi)
    read_idx: usize,
    /// Yazma konumu (üretici indeksi)
    write_idx: usize,
    /// Tamponda bulunan paket sayısı
    count: usize,
}

impl DmaRingBuffer {
    /// Yeni bir DMA-safe halka tamponu oluşturur.
    ///
    /// `packet_count * packet_size` byte'lık fiziksel bellek tahsis eder.
    /// `pages = ceil(total_size / 4096)` şeklinde sayfa sayısı hesaplanır.
    pub fn new(packet_count: usize, packet_size: usize) -> Option<Self> {
        let total_size = packet_count * packet_size;
        let pages = (total_size + 4095) / 4096;

        // TLSF heap'i atla; fiziksel bellekten ayır (DMA için uygun)
        let (paddr, vaddr) = crate::memory::dma_alloc(pages)?;

        // Yeni tahsis edilen belleği sıfırla (önceki içerik gizliliği + güvenlik)
        unsafe {
            core::ptr::write_bytes(vaddr.as_ptr(), 0, total_size);
        }

        Some(DmaRingBuffer {
            base: vaddr.as_ptr(),
            paddr,
            size: total_size,
            read_idx: 0,
            write_idx: 0,
            count: 0,
        })
    }

    /// Her slot'un byte boyutunu hesaplar (total_size / RX_QUEUE_SIZE)
    pub fn packet_size(&self) -> usize {
        self.size / RX_QUEUE_SIZE
    }

    /// Tampon boş mu? (count == 0)
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Tampon dolu mu? (count >= RX_QUEUE_SIZE)
    pub fn is_full(&self) -> bool {
        self.count >= RX_QUEUE_SIZE
    }

    /// Tamponda kaç paket var?
    pub fn len(&self) -> usize {
        self.count
    }

    /// Tampona paket yazar.
    ///
    /// Her slot'un başına uzunluk bilgisi (usize) kaydedilir,
    /// ardından gerçek paket verisi kopyalanır.
    ///
    /// ```
    /// slot[n]:  [len: usize (8 byte)] [data: packet_size byte]
    /// ```
    pub fn push(&mut self, data: &[u8]) -> bool {
        if self.is_full() {
            return false;
        }

        let pkt_size = self.packet_size();
        let offset = self.write_idx * pkt_size;

        if offset + pkt_size > self.size {
            return false;
        }

        // Veriyi tampona kopyala
        let len = data.len().min(pkt_size);
        unsafe {
            let dst = self.base.add(offset);
            core::ptr::copy_nonoverlapping(data.as_ptr(), dst, len);
            // Uzunluğu slot başına kaydet (pop() sırasında okunacak)
            *(dst as *const usize as *mut usize) = len;
        }

        self.write_idx = (self.write_idx + 1) % RX_QUEUE_SIZE;
        self.count += 1;
        true
    }

    /// Tampondan bir paket okur.
    ///
    /// Slot başındaki uzunluk değerini okur, ardından veriyi kopyalar.
    /// Geçersiz paketler (len=0 veya len>pkt_size) atlanır.
    pub fn pop(&mut self) -> Option<Vec<u8>> {
        if self.is_empty() {
            return None;
        }

        let pkt_size = self.packet_size();
        let offset = self.read_idx * pkt_size;

        unsafe {
            let src = self.base.add(offset);
            let len = *(src as *const usize);

            if len > pkt_size || len == 0 {
                // Geçersiz paket: atla
                self.read_idx = (self.read_idx + 1) % RX_QUEUE_SIZE;
                self.count = self.count.saturating_sub(1);
                return None;
            }

            let mut data = vec![0u8; len];
            core::ptr::copy_nonoverlapping(src, data.as_mut_ptr(), len);

            self.read_idx = (self.read_idx + 1) % RX_QUEUE_SIZE;
            self.count = self.count.saturating_sub(1);

            Some(data)
        }
    }

    /// Donanım DMA transferi için fiziksel adres döndürür.
    pub fn phys_addr(&self) -> usize {
        self.paddr
    }
}

impl Drop for DmaRingBuffer {
    /// Yapı düşürüldüğünde DMA belleğini serbest bırakır.
    ///
    /// `paddr=0` kontrolü: yapı hiç başlatılmamışsa serbest bırakma atlanır.
    fn drop(&mut self) {
        if self.paddr != 0 {
            let pages = (self.size + 4095) / 4096;
            crate::memory::dma_dealloc(self.paddr, pages);
        }
    }
}

/// DMA tamponunun farklı thread'ler arasında güvenle paylaşılabilmesi.
/// Raw pointer içerdiği için Rust bunu otomatik implement etmez;
/// biz mutex koruması altında erişileceğini garanti ederiz.
unsafe impl Send for DmaRingBuffer {}
unsafe impl Sync for DmaRingBuffer {}

// ============================================================================
// AĞ PAKETİ TAMPONU
// ============================================================================

/// Ağ paketi: veri + uzunluk.
///
/// `Vec<u8>` kullanılır; lifetime yönetimi Rust'ın sahiplik sistemi tarafından yapılır.
#[derive(Clone, Debug)]
pub struct NetPacket {
    pub data: Vec<u8>,
    pub len: usize,
}

impl NetPacket {
    /// Byte diliminden paket oluşturur (veriyi kopyalar).
    pub fn new(data: &[u8]) -> Self {
        NetPacket {
            data: data.to_vec(),
            len: data.len(),
        }
    }

    /// Paketi `len` byte'lık dilim olarak döndürür.
    pub fn as_slice(&self) -> &[u8] {
        &self.data[..self.len]
    }

    /// Paket uzunluğunu döndürür.
    pub fn len(&self) -> usize {
        self.len
    }
}

// ============================================================================
// VirtIO-NET CİHAZ DURUMU
// ============================================================================

/// VirtIO-Net ağ kartı cihaz durumu.
///
/// `driver`: virtio_drivers kütüphanesi VirtIONet örneği (Option, başlangıçta None)
/// `<VirtioHal, PciTransport, 16>`: HAL tipi, taşıma tipi, RX tampon kapasitesi
pub struct VirtioNetDevice {
    /// Alt seviye VirtIO-Net sürücüsü (başlatılmadan önce None)
    driver: Option<VirtIONet<VirtioHal, PciTransport, 16>>,
    /// Bu cihazın Ethernet MAC adresi
    mac: MacAddr,
    /// Gönderme kuyruğu: program tarafından gönderilecek paketler
    tx_queue: VecDeque<NetPacket>,
    /// Alma kuyruğu: donanımdan alınan paketler
    rx_queue: VecDeque<NetPacket>,
    /// DMA bellek domain numarası
    dma_domain: u32,
    /// Cihaz aktif (başlatıldı ve hazır) mı?
    active: bool,
    /// Toplam gönderilen paket sayısı (istatistik)
    tx_count: u64,
    /// Toplam alınan paket sayısı (istatistik)
    rx_count: u64,
    /// Toplam gönderilen byte (istatistik)
    tx_bytes: u64,
    /// Toplam alınan byte (istatistik)
    rx_bytes: u64,
}

impl VirtioNetDevice {
    pub fn new() -> Self {
        VirtioNetDevice {
            driver: None,
            mac: MacAddr::ZERO,
            tx_queue: VecDeque::with_capacity(TX_QUEUE_SIZE),
            rx_queue: VecDeque::with_capacity(RX_QUEUE_SIZE),
            dma_domain: 0,
            active: false,
            tx_count: 0,
            rx_count: 0,
            tx_bytes: 0,
            rx_bytes: 0,
        }
    }

    /// PCI taşıma katmanından cihazı başlatır.
    ///
    /// `VirtIONet::new(transport, 16)`: 16 adet RX descriptor ile başlatır.
    /// Başarılıysa MAC adresi okunur ve `active = true` yapılır.
    pub fn init(&mut self, transport: PciTransport) -> Result<(), NetError> {
        // VirtIO-Net için basitleştirilmiş DMA yönetimi: domain 0 kullan
        self.dma_domain = 0;

        crate::serial_println!("[VIRTIO-NET] Using DMA domain 0");

        let driver = VirtIONet::<VirtioHal, PciTransport, 16>::new(transport, 16).map_err(|e| {
            crate::serial_println!("[VIRTIO-NET] Init failed: {:?}", e);
            NetError::NoInterface
        })?;

        // MAC adresini cihazdan oku (VIRTIO_NET_F_MAC özelliği ile)
        let mac_bytes = driver.mac_address();
        self.mac = MacAddr::from_bytes(
            mac_bytes[0],
            mac_bytes[1],
            mac_bytes[2],
            mac_bytes[3],
            mac_bytes[4],
            mac_bytes[5],
        );

        self.driver = Some(driver);
        self.active = true;

        crate::serial_println!("[VIRTIO-NET] Initialized: MAC={:?}", self.mac);

        Ok(())
    }

    /// Cihazın Ethernet MAC adresini döndürür.
    pub fn mac(&self) -> MacAddr {
        self.mac
    }

    /// Cihazın aktif ve kullanılabilir olup olmadığını döndürür.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Ethernet paketi gönderir (TX - Transmit).
    ///
    /// # DMA Domain Yönetimi
    ///
    /// VirtIO DMA işlemleri sırasında doğru domain seçilmeli:
    /// 1. Mevcut domain'i kaydet (prev_domain)
    /// 2. Cihaz domain'ine geç
    /// 3. İşlemi gerçekleştir
    /// 4. Önceki domain'e geri dön
    ///
    /// Bu "save-restore" pattern, SMP sistemlerde domain karışıklığını önler.
    pub fn send(&mut self, data: &[u8]) -> Result<(), NetError> {
        if !self.active {
            return Err(NetError::NoInterface);
        }

        let prev_domain = crate::cpu::smp::current_dma_domain();
        crate::cpu::smp::set_current_dma_domain(self.dma_domain);

        if let Some(ref mut driver) = self.driver {
            // TX tampon oluştur (virtio_drivers bu tamponu donanıma iletir)
            let tx_buf = TxBuffer::from(data);

            // Paketi gönder
            driver.send(tx_buf).map_err(|_e| {
                crate::cpu::smp::set_current_dma_domain(prev_domain);
                NetError::ProtocolError
            })?;

            self.tx_count += 1;
            self.tx_bytes += data.len() as u64;

            crate::cpu::smp::set_current_dma_domain(prev_domain);
            return Ok(());
        }

        crate::cpu::smp::set_current_dma_domain(prev_domain);
        Err(NetError::NoInterface)
    }

    /// Donanımdan bir Ethernet paketi alır (RX - Receive).
    ///
    /// `driver.receive()`: virtqueue kullanılmış halkasını kontrol eder.
    /// Paket varsa `RxBuffer` döner; `rx_buf.packet()` gerçek Ethernet verisidir.
    pub fn recv(&mut self) -> Option<NetPacket> {
        if !self.active {
            return None;
        }

        let prev_domain = crate::cpu::smp::current_dma_domain();
        crate::cpu::smp::set_current_dma_domain(self.dma_domain);

        if let Some(ref mut driver) = self.driver {
            // Yeni paket var mı? (bloklamadan kontrol eder)
            if let Ok(rx_buf) = driver.receive() {
                let data: Vec<u8> = rx_buf.packet().to_vec();
                let len = data.len();

                self.rx_count += 1;
                self.rx_bytes += len as u64;

                crate::cpu::smp::set_current_dma_domain(prev_domain);
                return Some(NetPacket::new(&data));
            }
        }

        crate::cpu::smp::set_current_dma_domain(prev_domain);
        None
    }

    /// Donanımdan gelen paketleri RX kuyruğuna doldurur.
    ///
    /// RX kuyruğu dolana kadar veya donanımda paket kalmayana kadar
    /// döngü çalışır. Dönen değer: eklenen paket sayısı.
    pub fn poll_rx(&mut self) -> usize {
        let mut count = 0;

        while self.rx_queue.len() < RX_QUEUE_SIZE {
            if let Some(pkt) = self.recv() {
                self.rx_queue.push_back(pkt);
                count += 1;
            } else {
                break;
            }
        }

        count
    }

    /// RX kuyruğundan bir paket çıkarır (FIFO sırası: en eski önce).
    pub fn pop_rx(&mut self) -> Option<NetPacket> {
        self.rx_queue.pop_front()
    }

    /// İstatistikleri döndürür: (tx_paket, rx_paket, tx_byte, rx_byte)
    pub fn stats(&self) -> (u64, u64, u64, u64) {
        (self.tx_count, self.rx_count, self.tx_bytes, self.rx_bytes)
    }
}

// ============================================================================
// GLOBAL CİHAZ ÖRNEĞİ
// Çekirdek çapında tek VirtIO ağ kartı
// ============================================================================

/// Global VirtIO-Net cihaz örneği (Mutex ile korunur, iş parçacığı güvenli)
static VIRTIO_NET_DEV: Mutex<VirtioNetDevice> = Mutex::new(VirtioNetDevice {
    driver: None,
    mac: MacAddr::ZERO,
    tx_queue: VecDeque::new(),
    rx_queue: VecDeque::new(),
    dma_domain: 0,
    active: false,
    tx_count: 0,
    rx_count: 0,
    tx_bytes: 0,
    rx_bytes: 0,
});

/// Atomik bayrak: sürücü başlatıldı mı?
///
/// `AtomicBool` için bellek sıralama:
/// - `store(Ordering::Release)`: tüm önceki yazmaları görünür kılar
/// - `load(Ordering::Acquire)`: Release ile eşleşen yazmaları görür
static VIRTIO_NET_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// PCI taşıma katmanından VirtIO-Net sürücüsünü başlatır.
///
/// Başarılı olursa `VIRTIO_NET_INITIALIZED` `true` yapılır ve
/// `get_device()` geçerli bir referans döndürür.
pub fn init(transport: PciTransport) -> bool {
    crate::serial_println!("[VIRTIO-NET] Init start");

    let mut dev = VIRTIO_NET_DEV.lock();

    match dev.init(transport) {
        Ok(()) => {
            VIRTIO_NET_INITIALIZED.store(true, Ordering::Release);
            crate::serial_println!("[VIRTIO-NET] Init OK");
            true
        }
        Err(e) => {
            crate::serial_println!("[VIRTIO-NET] Init failed: {:?}", e);
            false
        }
    }
}

/// Sürücünün başlatılıp başlatılmadığını kontrol eder.
pub fn is_initialized() -> bool {
    VIRTIO_NET_INITIALIZED.load(Ordering::Acquire)
}

/// PCI taramasıyla VirtIO-Net cihazını otomatik olarak bulur ve başlatır.
///
/// # VirtIO-Net PCI Kimliği
/// - Satıcı (Vendor): 0x1AF4 (VirtIO)
/// - Cihaz (Device): 0x1000 (eski/legacy) veya 0x1041 (modern)
///
/// # Adımlar
/// 1. PCI taraması yap
/// 2. VirtIO-Net kimliğini ara
/// 3. PCI Bus Master, Memory, I/O bitlerini etkinleştir
/// 4. PciTransport oluştur (VirtIO PCI taşıma katmanı)
/// 5. `init(transport)` çağır
pub fn auto_init() -> bool {
    crate::serial_println!("[VIRTIO-NET] Scanning PCI for VirtIO-Net device...");

    use virtio_drivers::transport::pci::bus::DeviceFunction;
    use virtio_drivers::transport::pci::PciTransport;

    // PIO (Port I/O) modunda PCI kök oluştur
    let mut root = super::pci_root::create_pci_root();

    // PCI taraması yap
    let devices = crate::drivers::pci::scan();

    // VirtIO-Net device'ı ara
    // VirtIO Net: Vendor 0x1AF4, Device 0x1000 (legacy) or 0x1041 (modern)
    for dev in devices {
        if dev.vendor_id == 0x1AF4 && (dev.device_id == 0x1000 || dev.device_id == 0x1041) {
            crate::serial_println!(
                "[VIRTIO-NET] Found VirtIO device at {:02x}:{:02x}.{} (vid={:04x}, did={:04x})",
                dev.bus,
                dev.device,
                dev.function,
                dev.vendor_id,
                dev.device_id
            );

            // Enable device (Bus Master, Memory, I/O)
            super::pci_root::enable_device(dev.bus, dev.device, dev.function);
            crate::serial_println!("[VIRTIO-NET] Device enabled");

            // Create DeviceFunction
            let df = DeviceFunction {
                bus: dev.bus,
                device: dev.device,
                function: dev.function,
            };

            // Create PciTransport
            match PciTransport::new::<super::virtio_hal::VirtioHal>(&mut root, df) {
                Ok(transport) => {
                    crate::serial_println!("[VIRTIO-NET] Transport created successfully");
                    return init(transport);
                }
                Err(e) => {
                    crate::serial_println!("[VIRTIO-NET] Transport failed: {:?}", e);
                    continue;
                }
            }
        }
    }

    crate::serial_println!("[VIRTIO-NET] No VirtIO-Net device found");
    false
}

/// Ethernet paketi gönderir (global API).
pub fn send_packet(data: &[u8]) -> Result<(), NetError> {
    let mut dev = VIRTIO_NET_DEV.lock();
    dev.send(data)
}

/// Donanımdan bir paket alır (bloklamadan).
pub fn recv_packet() -> Option<NetPacket> {
    let mut dev = VIRTIO_NET_DEV.lock();
    dev.recv()
}

/// RX kuyruğunu donanım paketleriyle doldurur.
pub fn poll_rx() -> usize {
    let mut dev = VIRTIO_NET_DEV.lock();
    dev.poll_rx()
}

/// RX kuyruğundan sıradaki paketi çıkarır.
pub fn pop_rx() -> Option<NetPacket> {
    let mut dev = VIRTIO_NET_DEV.lock();
    dev.pop_rx()
}

/// Cihazın MAC adresini döndürür.
pub fn get_mac() -> MacAddr {
    VIRTIO_NET_DEV.lock().mac()
}

/// İstatistikleri döndürür: (tx_paket, rx_paket, tx_byte, rx_byte).
pub fn get_stats() -> (u64, u64, u64, u64) {
    VIRTIO_NET_DEV.lock().stats()
}

/// Başlatılmış VirtIO-Net cihazına referans döndürür.
///
/// Başlatılmamışsa `None` döner; çağıran kod sürücünün hazır
/// olmadığını bu şekilde anlayabilir.
pub fn get_device() -> Option<&'static Mutex<VirtioNetDevice>> {
    if VIRTIO_NET_INITIALIZED.load(Ordering::Acquire) {
        Some(&VIRTIO_NET_DEV)
    } else {
        None
    }
}
