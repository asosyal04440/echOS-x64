//! # echOS VirtIO-Net Driver
//!
//! VirtIO network device driver with TX/RX ring buffers
//! Uses physical memory for DMA-safe operation (bypasses TLSF heap)

use alloc::vec::Vec;
use alloc::vec;
use alloc::collections::VecDeque;
use spin::Mutex;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use core::ptr::NonNull;

use virtio_drivers::device::net::{VirtIONet, TxBuffer};
use virtio_drivers::transport::pci::PciTransport;

use super::virtio_hal::VirtioHal;
use crate::net::{NetError, MacAddr};

// ============================================================================
// VIRTIO-NET CONSTANTS
// ============================================================================

/// Maximum TX queue size
const TX_QUEUE_SIZE: usize = 256;
/// Maximum RX queue size
const RX_QUEUE_SIZE: usize = 256;
/// Maximum packet size (MTU + headers)
const MAX_PACKET_SIZE: usize = 1514;
/// Minimum packet size
const MIN_PACKET_SIZE: usize = 64;

// VirtIO-Net feature bits
const VIRTIO_NET_F_CSUM: u64 = 1 << 0;
const VIRTIO_NET_F_GUEST_CSUM: u64 = 1 << 1;
const VIRTIO_NET_F_MAC: u64 = 1 << 5;
const VIRTIO_NET_F_GSO: u64 = 1 << 6;
const VIRTIO_NET_F_GUEST_TSO4: u64 = 1 << 7;
const VIRTIO_NET_F_GUEST_TSO6: u64 = 1 << 8;
const VIRTIO_NET_F_CTRL_VQ: u64 = 1 << 17;
const VIRTIO_NET_F_CTRL_RX: u64 = 1 << 18;
const VIRTIO_NET_F_CTRL_VLAN: u64 = 1 << 19;
const VIRTIO_NET_F_MQ: u64 = 1 << 22;

// ============================================================================
// DMA-SAFE RING BUFFER (bypasses TLSF heap)
// ============================================================================

/// DMA-safe ring buffer using physical memory
pub struct DmaRingBuffer {
    /// Buffer base address (virtual)
    base: *mut u8,
    /// Buffer physical address
    paddr: usize,
    /// Buffer size in bytes
    size: usize,
    /// Read index
    read_idx: usize,
    /// Write index
    write_idx: usize,
    /// Number of packets
    count: usize,
}

impl DmaRingBuffer {
    /// Create a new DMA-safe ring buffer
    pub fn new(packet_count: usize, packet_size: usize) -> Option<Self> {
        let total_size = packet_count * packet_size;
        let pages = (total_size + 4095) / 4096;
        
        // Allocate from physical memory (bypasses TLSF)
        let (paddr, vaddr) = crate::memory::dma_alloc(pages)?;
        
        // Zero the buffer
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
    
    /// Get packet size
    pub fn packet_size(&self) -> usize {
        self.size / RX_QUEUE_SIZE
    }
    
    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
    
    /// Check if full
    pub fn is_full(&self) -> bool {
        self.count >= RX_QUEUE_SIZE
    }
    
    /// Get count
    pub fn len(&self) -> usize {
        self.count
    }
    
    /// Write packet to buffer
    pub fn push(&mut self, data: &[u8]) -> bool {
        if self.is_full() {
            return false;
        }
        
        let pkt_size = self.packet_size();
        let offset = self.write_idx * pkt_size;
        
        if offset + pkt_size > self.size {
            return false;
        }
        
        // Copy data to buffer
        let len = data.len().min(pkt_size);
        unsafe {
            let dst = self.base.add(offset);
            core::ptr::copy_nonoverlapping(data.as_ptr(), dst, len);
            // Store length at start of packet slot
            *(dst as *const usize as *mut usize) = len;
        }
        
        self.write_idx = (self.write_idx + 1) % RX_QUEUE_SIZE;
        self.count += 1;
        true
    }
    
    /// Read packet from buffer
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
                // Invalid packet, skip
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
    
    /// Get physical address for DMA
    pub fn phys_addr(&self) -> usize {
        self.paddr
    }
}

impl Drop for DmaRingBuffer {
    fn drop(&mut self) {
        if self.paddr != 0 {
            let pages = (self.size + 4095) / 4096;
            crate::memory::dma_dealloc(self.paddr, pages);
        }
    }
}

unsafe impl Send for DmaRingBuffer {}
unsafe impl Sync for DmaRingBuffer {}

// ============================================================================
// NETWORK PACKET BUFFER
// ============================================================================

/// Network packet buffer
#[derive(Clone, Debug)]
pub struct NetPacket {
    pub data: Vec<u8>,
    pub len: usize,
}

impl NetPacket {
    pub fn new(data: &[u8]) -> Self {
        NetPacket {
            data: data.to_vec(),
            len: data.len(),
        }
    }
    
    pub fn as_slice(&self) -> &[u8] {
        &self.data[..self.len]
    }
    
    pub fn len(&self) -> usize {
        self.len
    }
}

// ============================================================================
// VIRTIO-NET DEVICE
// ============================================================================

/// VirtIO-Net device state
pub struct VirtioNetDevice {
    /// Underlying VirtIO-Net driver
    driver: Option<VirtIONet<VirtioHal, PciTransport, 16>>,
    /// MAC address
    mac: MacAddr,
    /// TX queue (outgoing packets)
    tx_queue: VecDeque<NetPacket>,
    /// RX queue (incoming packets)
    rx_queue: VecDeque<NetPacket>,
    /// DMA domain
    dma_domain: u32,
    /// Device active
    active: bool,
    /// TX packets sent
    tx_count: u64,
    /// RX packets received
    rx_count: u64,
    /// TX bytes
    tx_bytes: u64,
    /// RX bytes
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
    
    /// Initialize device from PCI transport
    pub fn init(&mut self, transport: PciTransport) -> Result<(), NetError> {
        // Use domain 0 for VirtIO-Net (simplified DMA management)
        self.dma_domain = 0;
        
        crate::serial_println!("[VIRTIO-NET] Using DMA domain 0");
        
        let driver = VirtIONet::<VirtioHal, PciTransport, 16>::new(transport, 16)
            .map_err(|e| {
                crate::serial_println!("[VIRTIO-NET] Init failed: {:?}", e);
                NetError::NoInterface
            })?;
        
        // Get MAC address
        let mac_bytes = driver.mac_address();
        self.mac = MacAddr::from_bytes(mac_bytes[0], mac_bytes[1], mac_bytes[2], mac_bytes[3], mac_bytes[4], mac_bytes[5]);
        
        self.driver = Some(driver);
        self.active = true;
        
        crate::serial_println!("[VIRTIO-NET] Initialized: MAC={:?}", self.mac);
        
        Ok(())
    }
    
    /// Get MAC address
    pub fn mac(&self) -> MacAddr {
        self.mac
    }
    
    /// Check if device is active
    pub fn is_active(&self) -> bool {
        self.active
    }
    
    /// Send packet (TX)
    pub fn send(&mut self, data: &[u8]) -> Result<(), NetError> {
        if !self.active {
            return Err(NetError::NoInterface);
        }
        
        let prev_domain = crate::cpu::smp::current_dma_domain();
        crate::cpu::smp::set_current_dma_domain(self.dma_domain);
        
        if let Some(ref mut driver) = self.driver {
            // Create TX buffer
            let tx_buf = TxBuffer::from(data);
            
            // Send packet
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
    
    /// Receive packet (RX)
    pub fn recv(&mut self) -> Option<NetPacket> {
        if !self.active {
            return None;
        }
        
        let prev_domain = crate::cpu::smp::current_dma_domain();
        crate::cpu::smp::set_current_dma_domain(self.dma_domain);
        
        if let Some(ref mut driver) = self.driver {
            // Try to receive packet
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
    
    /// Poll for incoming packets and queue them
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
    
    /// Get packet from RX queue
    pub fn pop_rx(&mut self) -> Option<NetPacket> {
        self.rx_queue.pop_front()
    }
    
    /// Get statistics
    pub fn stats(&self) -> (u64, u64, u64, u64) {
        (self.tx_count, self.rx_count, self.tx_bytes, self.rx_bytes)
    }
}

// ============================================================================
// GLOBAL DEVICE INSTANCE
// ============================================================================

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
static VIRTIO_NET_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Initialize VirtIO-Net driver
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

/// Check if VirtIO-Net is initialized
pub fn is_initialized() -> bool {
    VIRTIO_NET_INITIALIZED.load(Ordering::Acquire)
}

/// Auto-detect and initialize VirtIO-Net via PCI scan
pub fn auto_init() -> bool {
    crate::serial_println!("[VIRTIO-NET] Scanning PCI for VirtIO-Net device...");
    
    use virtio_drivers::transport::pci::bus::DeviceFunction;
    use virtio_drivers::transport::pci::PciTransport;
    
    // Create PciRoot using PIO mode
    let mut root = super::pci_root::create_pci_root();
    
    // PCI taraması yap
    let devices = crate::drivers::pci::scan();
    
    // VirtIO-Net device'ı ara
    // VirtIO Net: Vendor 0x1AF4, Device 0x1000 (legacy) or 0x1041 (modern)
    for dev in devices {
        if dev.vendor_id == 0x1AF4 && 
           (dev.device_id == 0x1000 || dev.device_id == 0x1041) {
            crate::serial_println!(
                "[VIRTIO-NET] Found VirtIO device at {:02x}:{:02x}.{} (vid={:04x}, did={:04x})",
                dev.bus, dev.device, dev.function, dev.vendor_id, dev.device_id
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

/// Send packet
pub fn send_packet(data: &[u8]) -> Result<(), NetError> {
    let mut dev = VIRTIO_NET_DEV.lock();
    dev.send(data)
}

/// Receive packet
pub fn recv_packet() -> Option<NetPacket> {
    let mut dev = VIRTIO_NET_DEV.lock();
    dev.recv()
}

/// Poll for RX packets
pub fn poll_rx() -> usize {
    let mut dev = VIRTIO_NET_DEV.lock();
    dev.poll_rx()
}

/// Pop packet from RX queue
pub fn pop_rx() -> Option<NetPacket> {
    let mut dev = VIRTIO_NET_DEV.lock();
    dev.pop_rx()
}

/// Get MAC address
pub fn get_mac() -> MacAddr {
    VIRTIO_NET_DEV.lock().mac()
}

/// Get statistics
pub fn get_stats() -> (u64, u64, u64, u64) {
    VIRTIO_NET_DEV.lock().stats()
}

/// Get VirtIO-Net device
pub fn get_device() -> Option<&'static Mutex<VirtioNetDevice>> {
    if VIRTIO_NET_INITIALIZED.load(Ordering::Acquire) {
        Some(&VIRTIO_NET_DEV)
    } else {
        None
    }
}
