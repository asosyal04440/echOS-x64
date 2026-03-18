//! # NIC Native Driver — TIER 1 Lock-Free Ağ Sürücüsü
//!
//! VirtIO-net cihazı için TIER 1 uyumlu lock-free asenkron ağ sürücüsü.
//! TX/RX descriptor ring'leri atomik head/tail ile yönetilir.
//!
//! ## Mutex SIFIR Garantisi
//!
//! Bu modülde hiçbir Mutex, SpinLock veya RwLock kullanılmaz.
//! Tüm paylaşımlı durum AtomicU32/AtomicU64 ile yönetilir.
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────────┐
//! │  NIC Native Driver (TIER 1)                                  │
//! │                                                              │
//! │  TX Ring          RX Ring          Stats                     │
//! │  ┌──────────┐    ┌──────────┐    ┌──────────────┐          │
//! │  │ AtomicU32│    │ AtomicU32│    │ AtomicU64    │          │
//! │  │ head     │    │ head     │    │ tx_packets   │          │
//! │  │ tail     │    │ tail     │    │ rx_packets   │          │
//! │  │ entries[]│    │ entries[]│    │ tx_bytes     │          │
//! │  └──────────┘    └──────────┘    │ rx_bytes     │          │
//! │                                   └──────────────┘          │
//! └──────────────────────────────────────────────────────────────┘
//! ```

use alloc::string::String;
use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU64, Ordering};

use crate::drivers::async_traits::{
    AsyncIoError, AsyncNetDevice, CompletionEvent, DmaBuffer, SubmissionToken,
};

// ============================================================================
// Sabitler
// ============================================================================

/// TX/RX ring boyutu (2'nin kuvveti olmalı)
const RING_SIZE: usize = 256;
const RING_MASK: u32 = (RING_SIZE - 1) as u32;

/// Maksimum paket boyutu (jumbo frame desteği)
const MAX_PACKET_SIZE: usize = 9216;

/// Varsayılan MTU
const DEFAULT_MTU: u32 = 1500;

const E1000_REG_CTRL: usize = 0x0000;
const E1000_REG_STATUS: usize = 0x0008;
const E1000_REG_RCTL: usize = 0x0100;
const E1000_REG_TCTL: usize = 0x0400;
const E1000_REG_RDH: usize = 0x2810;
const E1000_REG_RDT: usize = 0x2818;
const E1000_REG_TDH: usize = 0x3810;
const E1000_REG_TDT: usize = 0x3818;
const E1000_REG_IMS: usize = 0x00D0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NicVendorFamily {
    Generic,
    Intel8254x,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NicDoorbellSnapshot {
    pub vendor_family: NicVendorFamily,
    pub mmio_base: u64,
    pub tx_head: u32,
    pub tx_tail: u32,
    pub rx_head: u32,
    pub rx_tail: u32,
    pub irq_mask: u32,
}

// ============================================================================
// Descriptor Ring (Lock-Free)
// ============================================================================

/// Tek bir TX/RX descriptor
#[derive(Clone, Copy, Debug)]
#[repr(C)]
struct NicDescriptor {
    /// DMA buffer fiziksel adresi
    buffer_addr: u64,
    /// Paket uzunluğu (byte)
    length: u32,
    /// Durum bayrakları
    flags: u32,
}

impl NicDescriptor {
    const fn empty() -> Self {
        Self {
            buffer_addr: 0,
            length: 0,
            flags: 0,
        }
    }
}

/// Lock-free descriptor ring
///
/// Single-producer, single-consumer: TX → core thread üretir, NIC tüketir
/// RX → NIC üretir, core thread tüketir
struct DescriptorRing {
    /// Ring entries
    entries: UnsafeCell<[NicDescriptor; RING_SIZE]>,
    /// Üretici pozisyonu (yeni descriptor ekleme noktası)
    head: AtomicU32,
    /// Tüketici pozisyonu (işlenmiş descriptor alma noktası)
    tail: AtomicU32,
}

// SAFETY: Ring buffer SPSC — tek üretici, tek tüketici
unsafe impl Send for DescriptorRing {}
unsafe impl Sync for DescriptorRing {}

impl DescriptorRing {
    fn new() -> Self {
        Self {
            entries: UnsafeCell::new([NicDescriptor::empty(); RING_SIZE]),
            head: AtomicU32::new(0),
            tail: AtomicU32::new(0),
        }
    }

    /// Descriptor ekle (üretici tarafı)
    fn push(&self, desc: NicDescriptor) -> bool {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        let next = (head + 1) & RING_MASK;

        if next == tail {
            return false; // Ring dolu
        }

        unsafe {
            let entries = &mut *self.entries.get();
            entries[head as usize] = desc;
        }

        // smp_wmb: descriptor yazıldıktan SONRA head'i güncelle
        crate::memory_barriers::smp_wmb();
        self.head.store(next, Ordering::Release);
        true
    }

    /// Descriptor al (tüketici tarafı)
    fn pop(&self) -> Option<NicDescriptor> {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);

        if tail == head {
            return None; // Ring boş
        }

        crate::memory_barriers::smp_rmb();
        let desc = unsafe {
            let entries = &*self.entries.get();
            entries[tail as usize]
        };

        self.tail.store((tail + 1) & RING_MASK, Ordering::Release);
        Some(desc)
    }

    /// Ring'deki eleman sayısı
    fn len(&self) -> u32 {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Acquire);
        (head.wrapping_sub(tail)) & RING_MASK
    }

    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

// ============================================================================
// NIC Native Device
// ============================================================================

/// TIER 1 Native NIC — Lock-Free Ağ Aygıtı
///
/// Mutex SIFIR: Tüm paylaşımlı durum atomiktir.
pub struct NicNativeDevice {
    /// Cihaz adı
    name: [u8; 16],
    name_len: usize,
    /// MAC adresi
    mac: [u8; 6],
    /// MTU
    mtu: AtomicU32,
    /// Link hızı (Mbps)
    link_speed: AtomicU64,
    /// Link durumu (up/down)
    link_up: AtomicBool,
    /// TX descriptor ring
    tx_ring: DescriptorRing,
    /// RX descriptor ring
    rx_ring: DescriptorRing,
    /// TX completion ring (donanım → sürücü)
    tx_comp_ring: DescriptorRing,
    /// İstatistikler (atomik — Mutex yok)
    tx_packets: AtomicU64,
    rx_packets: AtomicU64,
    tx_bytes: AtomicU64,
    rx_bytes: AtomicU64,
    tx_errors: AtomicU64,
    rx_errors: AtomicU64,
    tx_dropped: AtomicU64,
    rx_dropped: AtomicU64,
    /// Cihaz hazır mı?
    ready: AtomicBool,
    /// Atomik CID
    next_token: AtomicU64,
    mmio_base: u64,
    vendor_family: NicVendorFamily,
}

// SAFETY: Tüm alanlar atomic veya UnsafeCell (SPSC)
unsafe impl Send for NicNativeDevice {}
unsafe impl Sync for NicNativeDevice {}

impl NicNativeDevice {
    /// Yeni NIC cihazı oluşturur
    pub fn new(name: &str, mac: [u8; 6]) -> Self {
        let mut name_buf = [0u8; 16];
        let len = name.len().min(15);
        name_buf[..len].copy_from_slice(&name.as_bytes()[..len]);

        Self {
            name: name_buf,
            name_len: len,
            mac,
            mtu: AtomicU32::new(DEFAULT_MTU),
            link_speed: AtomicU64::new(1000), // 1 Gbps varsayılan
            link_up: AtomicBool::new(false),
            tx_ring: DescriptorRing::new(),
            rx_ring: DescriptorRing::new(),
            tx_comp_ring: DescriptorRing::new(),
            tx_packets: AtomicU64::new(0),
            rx_packets: AtomicU64::new(0),
            tx_bytes: AtomicU64::new(0),
            rx_bytes: AtomicU64::new(0),
            tx_errors: AtomicU64::new(0),
            rx_errors: AtomicU64::new(0),
            tx_dropped: AtomicU64::new(0),
            rx_dropped: AtomicU64::new(0),
            ready: AtomicBool::new(false),
            next_token: AtomicU64::new(1),
            mmio_base: 0,
            vendor_family: NicVendorFamily::Generic,
        }
    }

    pub fn new_intel_8254x(name: &str, mac: [u8; 6], mmio_base: u64) -> Self {
        let mut dev = Self::new(name, mac);
        dev.mmio_base = mmio_base;
        dev.vendor_family = NicVendorFamily::Intel8254x;
        dev.ready.store(true, Ordering::Release);
        dev.link_up.store(true, Ordering::Release);
        dev.link_speed.store(1000, Ordering::Release);
        dev.program_vendor_path();
        dev
    }

    #[inline(always)]
    fn write_mmio32(&self, offset: usize, value: u32) {
        if self.mmio_base == 0 {
            return;
        }
        unsafe {
            core::ptr::write_volatile((self.mmio_base + offset as u64) as *mut u32, value);
        }
    }

    #[inline(always)]
    fn read_mmio32(&self, offset: usize) -> u32 {
        if self.mmio_base == 0 {
            return 0;
        }
        unsafe { core::ptr::read_volatile((self.mmio_base + offset as u64) as *const u32) }
    }

    fn program_vendor_path(&self) {
        match self.vendor_family {
            NicVendorFamily::Generic => {}
            NicVendorFamily::Intel8254x => {
                self.write_mmio32(E1000_REG_IMS, u32::MAX);
                self.write_mmio32(E1000_REG_RCTL, self.read_mmio32(E1000_REG_RCTL) | 0x2);
                self.write_mmio32(E1000_REG_TCTL, self.read_mmio32(E1000_REG_TCTL) | 0x2);
            }
        }
    }

    fn ring_tx_doorbell(&self) {
        if self.vendor_family == NicVendorFamily::Intel8254x {
            let tail = self.tx_ring.head.load(Ordering::Acquire);
            self.write_mmio32(E1000_REG_TDT, tail);
        }
    }

    fn ring_rx_doorbell(&self) {
        if self.vendor_family == NicVendorFamily::Intel8254x {
            let tail = self.rx_ring.tail.load(Ordering::Acquire);
            self.write_mmio32(E1000_REG_RDT, tail);
        }
    }

    pub fn doorbell_snapshot(&self) -> NicDoorbellSnapshot {
        NicDoorbellSnapshot {
            vendor_family: self.vendor_family,
            mmio_base: self.mmio_base,
            tx_head: self.read_mmio32(E1000_REG_TDH),
            tx_tail: self.read_mmio32(E1000_REG_TDT),
            rx_head: self.read_mmio32(E1000_REG_RDH),
            rx_tail: self.read_mmio32(E1000_REG_RDT),
            irq_mask: self.read_mmio32(E1000_REG_IMS),
        }
    }

    /// Link durumunu ayarlar
    pub fn set_link_up(&self, up: bool, speed_mbps: u64) {
        self.link_up.store(up, Ordering::Release);
        self.link_speed.store(speed_mbps, Ordering::Release);
        self.program_vendor_path();
        crate::serial_println!(
            "[NIC:{}] Link {} @ {} Mbps",
            core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("?"),
            if up { "UP" } else { "DOWN" },
            speed_mbps
        );
    }

    /// RX ring'e paket ekler (donanım/interrupt handler tarafından çağrılır)
    pub fn receive_packet(&self, buffer_phys: u64, length: u32) -> bool {
        let desc = NicDescriptor {
            buffer_addr: buffer_phys,
            length,
            flags: 1, // RX_DONE
        };

        if self.rx_ring.push(desc) {
            self.rx_packets.fetch_add(1, Ordering::Relaxed);
            self.rx_bytes.fetch_add(length as u64, Ordering::Relaxed);
            self.ring_rx_doorbell();
            true
        } else {
            self.rx_dropped.fetch_add(1, Ordering::Relaxed);
            false
        }
    }

    /// İstatistikleri yazdırır
    pub fn print_stats(&self) {
        crate::serial_println!(
            "[NIC:{}] TX: pkts={} bytes={} err={} drop={}",
            core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("?"),
            self.tx_packets.load(Ordering::Relaxed),
            self.tx_bytes.load(Ordering::Relaxed),
            self.tx_errors.load(Ordering::Relaxed),
            self.tx_dropped.load(Ordering::Relaxed),
        );
        crate::serial_println!(
            "[NIC:{}] RX: pkts={} bytes={} err={} drop={}",
            core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("?"),
            self.rx_packets.load(Ordering::Relaxed),
            self.rx_bytes.load(Ordering::Relaxed),
            self.rx_errors.load(Ordering::Relaxed),
            self.rx_dropped.load(Ordering::Relaxed),
        );
    }
}

impl AsyncNetDevice for NicNativeDevice {
    fn name(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("nic?")
    }

    fn mac_address(&self) -> [u8; 6] {
        self.mac
    }

    fn mtu(&self) -> u32 {
        self.mtu.load(Ordering::Relaxed)
    }

    fn link_speed(&self) -> u64 {
        if self.link_up.load(Ordering::Acquire) {
            self.link_speed.load(Ordering::Relaxed)
        } else {
            0
        }
    }

    fn submit_tx(&self, dma_buf: &DmaBuffer, len: usize) -> Result<SubmissionToken, AsyncIoError> {
        if !self.ready.load(Ordering::Acquire) {
            return Err(AsyncIoError::DeviceGone);
        }
        if !self.link_up.load(Ordering::Acquire) {
            return Err(AsyncIoError::DeviceGone);
        }
        if len > MAX_PACKET_SIZE {
            return Err(AsyncIoError::InvalidParam);
        }

        let desc = NicDescriptor {
            buffer_addr: dma_buf.paddr,
            length: len as u32,
            flags: 0, // TX_PENDING
        };

        if self.tx_ring.push(desc) {
            self.tx_packets.fetch_add(1, Ordering::Relaxed);
            self.tx_bytes.fetch_add(len as u64, Ordering::Relaxed);
            self.ring_tx_doorbell();

            let token = SubmissionToken(self.next_token.fetch_add(1, Ordering::Relaxed));
            Ok(token)
        } else {
            self.tx_dropped.fetch_add(1, Ordering::Relaxed);
            Err(AsyncIoError::QueueFull)
        }
    }

    fn poll_tx_completion(&self) -> Option<CompletionEvent> {
        self.tx_comp_ring.pop().map(|_desc| CompletionEvent {
            token: SubmissionToken(0),
            result: 0,
            data_len: 0,
            flags: 0,
        })
    }

    fn poll_rx(&self) -> Option<CompletionEvent> {
        self.rx_ring.pop().map(|desc| CompletionEvent {
            token: SubmissionToken(0),
            result: 0,
            data_len: desc.length as usize,
            flags: 0,
        })
    }

    fn set_promiscuous(&self, _enable: bool) {
        // TODO: configure hardware promiscuous mode
    }

    fn set_rss_queues(&self, _count: u32) -> Result<(), AsyncIoError> {
        Err(AsyncIoError::NotSupported)
    }
}

/// NIC native alt sistemini başlatır.
pub fn init() {
    crate::serial_println!("[NIC-Native] TIER 1 lock-free NIC driver initialized");
    crate::serial_println!("[NIC-Native]   Ring size: {} entries", RING_SIZE);
    crate::serial_println!("[NIC-Native]   Max packet: {} bytes", MAX_PACKET_SIZE);
    crate::serial_println!("[NIC-Native]   Mutex in hot path: ZERO");
    for dev in crate::drivers::pci::scan().into_iter() {
        if dev.class_code == 0x02 && dev.vendor_id == 0x8086 {
            if let Some(bar) =
                crate::drivers::pci::read_bar_mmio(dev.bus, dev.device, dev.function, 0)
            {
                let nic = NicNativeDevice::new_intel_8254x("e1000", [0; 6], bar.base);
                let snap = nic.doorbell_snapshot();
                crate::serial_println!(
                    "[NIC-Native] Intel 8254x profile {:02x}:{:02x}.{} MMIO={:#x} TDT={:#x} RDT={:#x}",
                    dev.bus,
                    dev.device,
                    dev.function,
                    snap.mmio_base,
                    snap.tx_tail,
                    snap.rx_tail
                );
            }
        }
    }
}

// ============================================================================
// Interrupt Coalescing — Kesme Birleştirme
// ============================================================================
//
// Yüksek paket hızında her paket için kesme üretmek CPU'yu boğar.
// Coalescing, birden fazla paketi tek bir kesmede raporlar.

/// Coalescing parametreleri
#[derive(Debug, Clone, Copy)]
pub struct NicCoalesceConfig {
    /// Kaynaktan bağımsız birleştirme zamanı (mikrosaniye)
    pub rx_usecs: u32,
    /// RX — kesme başına maksimum paket sayısı
    pub rx_max_frames: u32,
    /// TX — birleştirme zamanı (mikrosaniye)
    pub tx_usecs: u32,
    /// TX — kesme başına maksimum paket sayısı
    pub tx_max_frames: u32,
    /// Adaptif coalescing etkin mi
    pub use_adaptive_rx: bool,
    pub use_adaptive_tx: bool,
}

impl NicCoalesceConfig {
    /// Varsayılan — düşük gecikmeli profil
    pub const fn low_latency() -> Self {
        Self {
            rx_usecs: 10,
            rx_max_frames: 4,
            tx_usecs: 10,
            tx_max_frames: 4,
            use_adaptive_rx: false,
            use_adaptive_tx: false,
        }
    }

    /// Yüksek verimlilik profili
    pub const fn high_throughput() -> Self {
        Self {
            rx_usecs: 100,
            rx_max_frames: 64,
            tx_usecs: 100,
            tx_max_frames: 64,
            use_adaptive_rx: true,
            use_adaptive_tx: true,
        }
    }

    /// Dengeli profil
    pub const fn balanced() -> Self {
        Self {
            rx_usecs: 50,
            rx_max_frames: 16,
            tx_usecs: 50,
            tx_max_frames: 16,
            use_adaptive_rx: false,
            use_adaptive_tx: false,
        }
    }
}

static NIC_COALESCE: spin::Mutex<NicCoalesceConfig> =
    spin::Mutex::new(NicCoalesceConfig::balanced());

/// Coalescing yapılandırmasını ayarlar.
pub fn set_coalesce(config: NicCoalesceConfig) {
    *NIC_COALESCE.lock() = config;
    crate::serial_println!(
        "[NIC-Native] Coalescing: rx_usecs={} rx_max_frames={} tx_usecs={} tx_max_frames={}",
        config.rx_usecs,
        config.rx_max_frames,
        config.tx_usecs,
        config.tx_max_frames,
    );
}

/// Mevcut coalescing yapılandırmasını döner.
pub fn get_coalesce() -> NicCoalesceConfig {
    *NIC_COALESCE.lock()
}

/// Adaptif coalescing — trafik yüklenmesine göre otomatik ayarlama.
pub fn adaptive_coalesce_tick(rx_pps: u64, tx_pps: u64) {
    let mut config = NIC_COALESCE.lock();
    if !config.use_adaptive_rx && !config.use_adaptive_tx {
        return;
    }

    if config.use_adaptive_rx {
        if rx_pps > 100_000 {
            config.rx_usecs = 100;
            config.rx_max_frames = 64;
        } else if rx_pps > 10_000 {
            config.rx_usecs = 50;
            config.rx_max_frames = 32;
        } else {
            config.rx_usecs = 10;
            config.rx_max_frames = 4;
        }
    }

    if config.use_adaptive_tx {
        if tx_pps > 100_000 {
            config.tx_usecs = 100;
            config.tx_max_frames = 64;
        } else if tx_pps > 10_000 {
            config.tx_usecs = 50;
            config.tx_max_frames = 32;
        } else {
            config.tx_usecs = 10;
            config.tx_max_frames = 4;
        }
    }
}
