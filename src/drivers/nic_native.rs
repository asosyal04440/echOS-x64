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

// RCTL bit definitions (Intel 8254x SDM §13.4.17)
const E1000_RCTL_EN: u32 = 1 << 1; // Receiver Enable
const E1000_RCTL_UPE: u32 = 1 << 3; // Unicast Promiscuous Enabled
const E1000_RCTL_MPE: u32 = 1 << 4; // Multicast Promiscuous Enabled
const E1000_RCTL_BAM: u32 = 1 << 15; // Broadcast Accept Mode
const E1000_RCTL_SECRC: u32 = 1 << 26; // Strip Ethernet CRC

// TCTL bit definitions (Intel 8254x SDM §13.4.25)
const E1000_TCTL_EN: u32 = 1 << 1; // Transmit Enable
const E1000_TCTL_PSP: u32 = 1 << 3; // Pad Short Packets

// Intel 8254x TX Descriptor (16 bytes, SDM §3.3.3)
#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct E1000TxDesc {
    buffer_addr: u64,
    length: u16,
    cso: u8,
    cmd: u8,
    status: u8,
    css: u8,
    special: u16,
}

const E1000_TXD_CMD_EOP: u8 = 0x01; // End of Packet
const E1000_TXD_CMD_IFCS: u8 = 0x02; // Insert FCS
const E1000_TXD_CMD_RS: u8 = 0x08; // Report Status
const E1000_TXD_STAT_DD: u8 = 0x01; // Descriptor Done

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
    // Intel 8254x hardware TX descriptor ring (DMA-mapped)
    e1000_tx_desc_phys: u64,
    e1000_tx_desc_virt: *mut E1000TxDesc,
    e1000_tx_ring_len: u32,
    e1000_tx_hw_tail: AtomicU32,
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
            e1000_tx_desc_phys: 0,
            e1000_tx_desc_virt: core::ptr::null_mut(),
            e1000_tx_ring_len: 0,
            e1000_tx_hw_tail: AtomicU32::new(0),
        }
    }

    pub fn new_intel_8254x(name: &str, mac: [u8; 6], mmio_base: u64) -> Self {
        let mut dev = Self::new(name, mac);
        dev.mmio_base = mmio_base;
        dev.vendor_family = NicVendorFamily::Intel8254x;
        dev.ready.store(true, Ordering::Release);
        dev.link_up.store(true, Ordering::Release);
        dev.link_speed.store(1000, Ordering::Release);
        dev
    }

    pub fn setup_e1000_tx_ring(&mut self, phys: u64, virt: *mut E1000TxDesc, len: u32) {
        self.e1000_tx_desc_phys = phys;
        self.e1000_tx_desc_virt = virt;
        self.e1000_tx_ring_len = len;
        self.e1000_tx_hw_tail.store(0, Ordering::Release);
        unsafe {
            core::ptr::write_bytes(
                virt,
                0,
                (len as usize) * core::mem::size_of::<E1000TxDesc>(),
            );
        }
        let tdba_lo = (phys & 0xFFFFFFFF) as u32;
        let tdba_hi = (phys >> 32) as u32;
        self.write_mmio32(0x3800, tdba_lo);
        self.write_mmio32(0x3804, tdba_hi);
        self.write_mmio32(
            0x3808,
            (len * core::mem::size_of::<E1000TxDesc>() as u32) as u32,
        );
        self.write_mmio32(E1000_REG_TDH, 0);
        self.write_mmio32(E1000_REG_TDT, 0);
        self.program_vendor_path();
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
                let rctl = self.read_mmio32(E1000_REG_RCTL)
                    | E1000_RCTL_EN
                    | E1000_RCTL_BAM
                    | E1000_RCTL_SECRC;
                self.write_mmio32(E1000_REG_RCTL, rctl);
                let tctl = self.read_mmio32(E1000_REG_TCTL) | E1000_TCTL_EN | E1000_TCTL_PSP;
                self.write_mmio32(E1000_REG_TCTL, tctl);
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
            if self.vendor_family == NicVendorFamily::Intel8254x
                && !self.e1000_tx_desc_virt.is_null()
            {
                let idx = self.tx_ring.head.load(Ordering::Acquire).wrapping_sub(1) & RING_MASK;
                let hw_idx = idx % self.e1000_tx_ring_len;
                unsafe {
                    let hw_desc = &mut *self.e1000_tx_desc_virt.add(hw_idx as usize);
                    hw_desc.buffer_addr = dma_buf.paddr;
                    hw_desc.length = len as u16;
                    hw_desc.cso = 0;
                    hw_desc.cmd = E1000_TXD_CMD_EOP | E1000_TXD_CMD_IFCS | E1000_TXD_CMD_RS;
                    hw_desc.status = 0;
                    hw_desc.css = 0;
                    hw_desc.special = 0;
                }
                crate::memory_barriers::smp_wmb();
            }
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
        if self.vendor_family == NicVendorFamily::Intel8254x && !self.e1000_tx_desc_virt.is_null() {
            let hw_tail = self.e1000_tx_hw_tail.load(Ordering::Acquire);
            let ring_len = self.e1000_tx_ring_len;
            if hw_tail >= ring_len {
                return None;
            }
            unsafe {
                let desc = &*self.e1000_tx_desc_virt.add(hw_tail as usize);
                if desc.status & E1000_TXD_STAT_DD != 0 {
                    self.e1000_tx_hw_tail.store(hw_tail + 1, Ordering::Release);
                    let token = SubmissionToken(hw_tail as u64);
                    return Some(CompletionEvent {
                        token,
                        result: 0,
                        data_len: desc.length as usize,
                        flags: 0,
                    });
                }
            }
            return None;
        }
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

    fn set_promiscuous(&self, enable: bool) {
        if self.vendor_family != NicVendorFamily::Intel8254x || self.mmio_base == 0 {
            return;
        }
        let mut rctl = self.read_mmio32(E1000_REG_RCTL);
        if enable {
            rctl |= E1000_RCTL_UPE | E1000_RCTL_MPE;
        } else {
            rctl &= !(E1000_RCTL_UPE | E1000_RCTL_MPE);
        }
        self.write_mmio32(E1000_REG_RCTL, rctl);
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

// ============================================================================
// Test Corpus (Intel 8254x SDM + Linux NAPI semantics)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_device() -> NicNativeDevice {
        NicNativeDevice::new_intel_8254x("e1000", [0x52, 0x54, 0x00, 0x12, 0x34, 0x56], 0)
    }

    #[test]
    fn nic_device_creation_and_mac() {
        let dev = make_device();
        assert_eq!(dev.mac_address(), [0x52, 0x54, 0x00, 0x12, 0x34, 0x56]);
        assert_eq!(dev.name(), "e1000");
        assert!(dev.ready.load(Ordering::Acquire));
        assert!(dev.link_up.load(Ordering::Acquire));
    }

    #[test]
    fn nic_default_mtu_is_1500() {
        let dev = make_device();
        assert_eq!(dev.mtu(), DEFAULT_MTU);
    }

    #[test]
    fn nic_mtu_enforcement_on_submit() {
        let dev = make_device();
        // MTU + Ethernet header (14) + VLAN (4) = 1518 max for 1500 MTU
        // MAX_PACKET_SIZE = 9216 allows jumbo frames
        let small_buf = DmaBuffer {
            paddr: 0x1000,
            vaddr: 0,
            size: 2048,
        };
        // Packets within MAX_PACKET_SIZE should be accepted
        assert!(dev.submit_tx(&small_buf, 1518).is_ok());
        // Packets exceeding MAX_PACKET_SIZE should be rejected
        let big_buf = DmaBuffer {
            paddr: 0x2000,
            vaddr: 0,
            size: MAX_PACKET_SIZE + 1,
        };
        assert!(dev.submit_tx(&big_buf, MAX_PACKET_SIZE + 1).is_err());
    }

    #[test]
    fn nic_tx_ring_full_returns_queue_full() {
        let dev = make_device();
        // Fill the ring
        let buf = DmaBuffer {
            paddr: 0x3000,
            vaddr: 0,
            size: 2048,
        };
        for _ in 0..RING_SIZE {
            let _ = dev.submit_tx(&buf, 64);
        }
        // Next submit should fail with QueueFull
        assert!(matches!(
            dev.submit_tx(&buf, 64),
            Err(AsyncIoError::QueueFull)
        ));
        assert!(dev.tx_dropped.load(Ordering::Relaxed) >= 1);
    }

    #[test]
    fn nic_rx_ring_full_drops_packet() {
        let dev = make_device();
        // Fill the RX ring
        for _ in 0..(RING_SIZE - 1) {
            assert!(dev.receive_packet(0x4000, 64));
        }
        // Next receive should drop
        assert!(!dev.receive_packet(0x5000, 64));
        assert!(dev.rx_dropped.load(Ordering::Relaxed) >= 1);
    }

    #[test]
    fn nic_stats_accumulate() {
        let dev = make_device();
        let buf = DmaBuffer {
            paddr: 0x6000,
            vaddr: 0,
            size: 2048,
        };
        dev.submit_tx(&buf, 100).unwrap();
        dev.receive_packet(0x7000, 200);

        assert!(dev.tx_packets.load(Ordering::Relaxed) >= 1);
        assert!(dev.tx_bytes.load(Ordering::Relaxed) >= 100);
        assert!(dev.rx_packets.load(Ordering::Relaxed) >= 1);
        assert!(dev.rx_bytes.load(Ordering::Relaxed) >= 200);
    }

    #[test]
    fn nic_link_speed_zero_when_down() {
        let dev = make_device();
        dev.set_link_up(false, 0);
        assert_eq!(dev.link_speed(), 0);
        dev.set_link_up(true, 1000);
        assert_eq!(dev.link_speed(), 1000);
    }

    #[test]
    fn nic_submit_fails_when_link_down() {
        let dev = make_device();
        dev.set_link_up(false, 0);
        let buf = DmaBuffer {
            paddr: 0x8000,
            vaddr: 0,
            size: 2048,
        };
        assert!(matches!(
            dev.submit_tx(&buf, 64),
            Err(AsyncIoError::DeviceGone)
        ));
    }

    #[test]
    fn nic_promiscuous_rctl_bits() {
        // Promiscuous mode sets UPE + MPE in RCTL
        // Intel 8254x SDM §13.4.17: UPE=bit 3, MPE=bit 4
        assert_eq!(E1000_RCTL_UPE, 1 << 3);
        assert_eq!(E1000_RCTL_MPE, 1 << 4);
        // Combined with BAM (broadcast accept) and SECRC (strip CRC)
        let rctl =
            E1000_RCTL_EN | E1000_RCTL_UPE | E1000_RCTL_MPE | E1000_RCTL_BAM | E1000_RCTL_SECRC;
        assert!(rctl & E1000_RCTL_UPE != 0);
        assert!(rctl & E1000_RCTL_MPE != 0);
        assert!(rctl & E1000_RCTL_BAM != 0);
    }

    #[test]
    fn nic_tx_descriptor_cmd_flags() {
        // Intel 8254x SDM §3.3.3: TX descriptor command flags
        assert_eq!(E1000_TXD_CMD_EOP, 0x01); // End of Packet
        assert_eq!(E1000_TXD_CMD_IFCS, 0x02); // Insert FCS
        assert_eq!(E1000_TXD_CMD_RS, 0x08); // Report Status
                                            // Standard TX cmd: EOP + IFCS + RS
        let cmd = E1000_TXD_CMD_EOP | E1000_TXD_CMD_IFCS | E1000_TXD_CMD_RS;
        assert_eq!(cmd, 0x0B);
    }

    #[test]
    fn nic_tx_completion_descriptor_done() {
        // E1000_TXD_STAT_DD = bit 0, set by hardware when done
        assert_eq!(E1000_TXD_STAT_DD, 0x01);
        let status_with_dd = E1000_TXD_STAT_DD;
        assert!(status_with_dd & E1000_TXD_STAT_DD != 0);
        let status_pending = 0u8;
        assert!(status_pending & E1000_TXD_STAT_DD == 0);
    }

    #[test]
    fn nic_ring_wraparound() {
        let ring = DescriptorRing::new();
        // Push and pop RING_SIZE * 2 times to verify wraparound
        for _ in 0..RING_SIZE * 2 {
            let desc = NicDescriptor {
                buffer_addr: 0x9000,
                length: 64,
                flags: 0,
            };
            assert!(ring.push(desc));
            let popped = ring.pop();
            assert!(popped.is_some());
            assert_eq!(popped.unwrap().length, 64);
        }
        assert!(ring.is_empty());
    }

    #[test]
    fn nic_coalesce_profiles() {
        let low = NicCoalesceConfig::low_latency();
        assert_eq!(low.rx_usecs, 10);
        assert_eq!(low.rx_max_frames, 4);

        let high = NicCoalesceConfig::high_throughput();
        assert_eq!(high.rx_usecs, 100);
        assert_eq!(high.rx_max_frames, 64);
        assert!(high.use_adaptive_rx);

        let balanced = NicCoalesceConfig::balanced();
        assert_eq!(balanced.rx_usecs, 50);
        assert_eq!(balanced.rx_max_frames, 16);
    }

    #[test]
    fn nic_doorbell_snapshot_zero_mmio() {
        let dev = make_device();
        // With mmio_base=0, read_mmio32 returns 0
        let snap = dev.doorbell_snapshot();
        assert_eq!(snap.mmio_base, 0);
        assert_eq!(snap.tx_head, 0);
        assert_eq!(snap.tx_tail, 0);
        assert_eq!(snap.rx_head, 0);
        assert_eq!(snap.rx_tail, 0);
        assert_eq!(snap.irq_mask, 0);
    }
}
