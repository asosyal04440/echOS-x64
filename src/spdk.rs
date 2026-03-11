//! # SPDK (Storage Performance Development Kit) Integration
//!
//! echOS için SPDP entegrasyonu - userspace I/O acceleration.
//! NVMe, TCP/IP, ve diğer I/O operasyonlarını userspace'de çalıştırır.
//!
//! ## SPDK Nedir?
//!
//! SPDK, Intel tarafından geliştirilen userspace storage platformudur:
//! - Kernel bypass ile I/O operasyonları
//! - Polling mode drivers (interruptsiz)
//! - NVMe-oF (NVMe over Fabrics)
//! - High performance storage stack
//!
//! ## echOS SPDK Entegrasyonu
//!
//! ```text
//! echOS Kernel
//!     │
//!     ├── SPDK Integration Layer
//!     │   ├── NVMe Polling Driver
//!     │   ├── Userspace I/O
//!     │   └── Memory Management
//!     │
//!     ├── Application Layer
//!     │   ├── File Systems
//!     │   ├── Database Engines
//!     │   └── Storage Applications
//! ```

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use spin::Mutex;

// ============================================================================
// SPDK SABİTLERİ
// ============================================================================

/// Maksimum NVMe cihaz sayısı
pub const SPDK_MAX_NVME_DEVICES: usize = 64;

/// Maksimum I/O queue sayısı
pub const SPDK_MAX_IO_QUEUES: usize = 256;

/// Maksimum I/O request sayısı
pub const SPDK_MAX_IO_REQUESTS: usize = 1024;

/// SPDK I/O tipleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpdkIoType {
    /// NVMe I/O
    Nvme,
    /// TCP/IP I/O
    Tcp,
    /// vhost I/O
    Vhost,
    /// RDMA I/O
    Rdma,
}

/// SPDK hata kodları
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpdkError {
    /// Başlatılamadı
    InitializationFailed,
    /// Cihaz bulunamadı
    DeviceNotFound,
    /// Bellek yetersiz
    OutOfMemory,
    /// I/O hatası
    IoError,
    /// Timeout
    Timeout,
    /// Desteklenmiyor
    NotSupported,
    /// Meşgul
    Busy,
}

// ============================================================================
// SPDK NVME CONTEXT
// ============================================================================

/// SPDK NVMe context'i
#[derive(Clone, Debug)]
pub struct SpdkNvmeContext {
    /// Context ID'si
    pub context_id: u32,
    /// NVMe controller handle
    pub controller_handle: u64,
    /// Namespace bilgileri
    pub namespaces: Vec<SpdkNamespace>,
    /// I/O queue'lar
    pub io_queues: Vec<SpdkIoQueue>,
    /// Polling thread aktif mi?
    pub polling_active: AtomicBool,
    /// I/O istatistikleri
    pub io_stats: SpdkIoStats,
}

/// NVMe namespace bilgisi
#[derive(Clone, Debug)]
pub struct SpdkNamespace {
    /// Namespace ID'si
    pub nsid: u32,
    /// Boyut (sectors)
    pub size: u64,
    /// Blok boyutu
    pub block_size: u32,
    /// LBA format
    pub lba_format: SpdkLbaFormat,
}

/// LBA format bilgisi
#[derive(Clone, Copy, Debug)]
pub struct SpdkLbaFormat {
    /// LBA boyutu (bytes)
    pub lbaf: u8,
    /// Metadata boyutu
    pub ms: u8,
    /// Endianness
    pub endianness: bool,
}

/// SPDK I/O queue
#[derive(Clone, Debug)]
pub struct SpdkIoQueue {
    /// Queue ID'si
    pub queue_id: u16,
    /// Queue boyutu
    pub queue_size: u16,
    /// CPU affinity
    pub cpu_affinity: u32,
    /// Aktif mi?
    pub active: AtomicBool,
}

/// I/O istatistikleri
#[derive(Clone, Debug)]
pub struct SpdkIoStats {
    /// Toplam I/O sayısı
    pub total_ios: AtomicU64,
    /// Okunan I/O sayısı
    pub read_ios: AtomicU64,
    /// Yazılan I/O sayısı
    pub write_ios: AtomicU64,
    /// Toplam byte sayısı
    pub total_bytes: AtomicU64,
    /// Okunan byte sayısı
    pub read_bytes: AtomicU64,
    /// Yazılan byte sayısı
    pub write_bytes: AtomicU64,
    /// I/O latency (ns)
    pub avg_latency_ns: AtomicU64,
}

impl SpdkIoStats {
    /// Yeni I/O istatistikleri oluştur
    pub fn new() -> Self {
        Self {
            total_ios: AtomicU64::new(0),
            read_ios: AtomicU64::new(0),
            write_ios: AtomicU64::new(0),
            total_bytes: AtomicU64::new(0),
            read_bytes: AtomicU64::new(0),
            write_bytes: AtomicU64::new(0),
            avg_latency_ns: AtomicU64::new(0),
        }
    }
    
    /// I/O kaydet
    pub fn record_io(&self, io_type: SpdkIoType, bytes: u64, latency_ns: u64) {
        self.total_ios.fetch_add(1, Ordering::SeqCst);
        self.total_bytes.fetch_add(bytes, Ordering::SeqCst);
        
        match io_type {
            SpdkIoType::Nvme => {
                // NVMe I/O için özel istatistikler
                if bytes > 0 {
                    self.read_ios.fetch_add(1, Ordering::SeqCst);
                    self.read_bytes.fetch_add(bytes, Ordering::SeqCst);
                } else {
                    self.write_ios.fetch_add(1, Ordering::SeqCst);
                    self.write_bytes.fetch_add(bytes, Ordering::SeqCst);
                }
            }
            _ => {}
        }
        
        // Latency güncelle (moving average)
        let current_avg = self.avg_latency_ns.load(Ordering::SeqCst);
        let new_avg = (current_avg * 9 + latency_ns) / 10; // 10% weight
        self.avg_latency_ns.store(new_avg, Ordering::SeqCst);
    }
}

impl Default for SpdkIoStats {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// SPDK I/O REQUEST
// ============================================================================

/// SPDK I/O request
#[derive(Clone, Debug)]
pub struct SpdkIoRequest {
    /// Request ID'si
    pub request_id: u64,
    /// I/O tipi
    pub io_type: SpdkIoType,
    /// Namespace ID'si
    pub nsid: u32,
    /// LBA (Logical Block Address)
    pub lba: u64,
    /// Blok sayısı
    pub block_count: u32,
    /// Data buffer
    pub data_buffer: Vec<u8>,
    /// Callback fonksiyonu
    pub callback: Option<SpdkIoCallback>,
    /// Başlangıç zamanı
    pub start_time: u64,
}

/// I/O callback fonksiyonu
pub type SpdkIoCallback = fn(&SpdkIoRequest, SpdkError) -> ();

impl SpdkIoRequest {
    /// Yeni I/O request oluştur
    pub fn new(request_id: u64, io_type: SpdkIoType, nsid: u32, lba: u64, block_count: u32) -> Self {
        Self {
            request_id,
            io_type,
            nsid,
            lba,
            block_count,
            data_buffer: Vec::new(),
            callback: None,
            start_time: crate::interrupts::get_ticks(),
        }
    }
    
    /// Read request oluştur
    pub fn new_read(request_id: u64, nsid: u32, lba: u64, block_count: u32) -> Self {
        Self::new(request_id, SpdkIoType::Nvme, nsid, lba, block_count)
    }
    
    /// Write request oluştur
    pub fn new_write(request_id: u64, nsid: u32, lba: u64, block_count: u32, data: Vec<u8>) -> Self {
        let mut req = Self::new(request_id, SpdkIoType::Nvme, nsid, lba, block_count);
        req.data_buffer = data;
        req
    }
    
    /// Callback ayarla
    pub fn set_callback(&mut self, callback: SpdkIoCallback) {
        self.callback = Some(callback);
    }
    
    /// Request'i tamamla
    pub fn complete(self, error: SpdkError) {
        if let Some(callback) = self.callback {
            callback(&self, error);
        }
    }
}

// ============================================================================
// SPDK MANAGER
// ============================================================================

/// SPDK manager
pub struct SpdkManager {
    /// NVMe context'leri
    pub nvme_contexts: Mutex<BTreeMap<u32, Arc<Mutex<SpdkNvmeContext>>>>,
    /// I/O request'lar
    pub io_requests: Mutex<BTreeMap<u64, SpdkIoRequest>>,
    /// Aktif mi?
    pub active: AtomicBool,
    /// Global istatistikler
    pub global_stats: SpdkIoStats,
    /// Bir sonraki request ID
    pub next_request_id: AtomicU64,
}

impl SpdkManager {
    /// Yeni SPDK manager oluştur
    pub fn new() -> Self {
        Self {
            nvme_contexts: Mutex::new(BTreeMap::new()),
            io_requests: Mutex::new(BTreeMap::new()),
            active: AtomicBool::new(false),
            global_stats: SpdkIoStats::new(),
            next_request_id: AtomicU64::new(1),
        }
    }
    
    /// SPDK'yi başlat
    pub fn init(&self) -> Result<(), SpdkError> {
        if self.active.load(Ordering::SeqCst) {
            return Ok(());
        }
        
        crate::serial_println!("[SPDK] Initializing SPDK");
        
        // NVMe cihazlarını tara
        self.scan_nvme_devices()?;
        
        // I/O queue'ları oluştur
        self.setup_io_queues()?;
        
        self.active.store(true, Ordering::SeqCst);
        
        crate::serial_println!("[SPDK] SPDK initialized successfully");
        
        Ok(())
    }
    
    /// NVMe cihazlarını tara
    fn scan_nvme_devices(&self) -> Result<(), SpdkError> {
        crate::serial_println!("[SPDK] Scanning for NVMe devices");
        
        // Placeholder: PCIe bus'u tara ve NVMe cihazlarını bul
        let nvme_devices = self.detect_nvme_devices();
        
        for (i, device_info) in nvme_devices.iter().enumerate() {
            let context_id = i as u32;
            let context = self.create_nvme_context(context_id, device_info)?;
            
            self.nvme_contexts.lock().insert(context_id, Arc::new(Mutex::new(context)));
            
            crate::serial_println!("[SPDK] Found NVMe device: context_id={}, pci={:x}", 
                context_id, device_info.pci_address);
        }
        
        Ok(())
    }
    
    /// NVMe cihazlarını tespit et
    fn detect_nvme_devices(&self) -> Vec<SpdkNvmeDeviceInfo> {
        crate::serial_println!("[SPDK] Scanning PCIe bus for NVMe devices");
        
        let mut devices = Vec::new();
        
        // PCIe bus'u tara (real implementation)
        for bus in 0..255 {
            for device in 0..31 {
                for function in 0..8 {
                    let pci_address = (bus << 8) | (device << 3) | function;
                    
                    // PCI configuration space'ı oku
                    if let Some(device_info) = self.read_pci_config(pci_address) {
                        // NVMe class code kontrolü (0x01 = Mass Storage, 0x08 = NVMe)
                        if device_info.class_code == 0x01 && device_info.device_id == 0x08 {
                            devices.push(device_info);
                            crate::serial_println!("[SPDK] Found NVMe device at {:x}: vendor={:x}, device={:x}", 
                                pci_address, device_info.vendor_id, device_info.device_id);
                        }
                    }
                }
            }
        }
        
        devices
    }
    
    /// PCI configuration space'ı oku
    fn read_pci_config(&self, pci_address: u32) -> Option<SpdkNvmeDeviceInfo> {
        // PCI configuration space register'ları
        let vendor_id = self.pci_read_config_u16(pci_address, 0x00);
        let device_id = self.pci_read_config_u16(pci_address, 0x02);
        
        // Geçerli cihaz mı?
        if vendor_id == 0xFFFF || device_id == 0xFFFF {
            return None;
        }
        
        Some(SpdkNvmeDeviceInfo {
            pci_address,
            vendor_id,
            device_id,
            subsystem_vendor_id: self.pci_read_config_u16(pci_address, 0x2C),
            subsystem_device_id: self.pci_read_config_u16(pci_address, 0x2E),
            class_code: self.pci_read_config_u8(pci_address, 0x0B),
            revision: self.pci_read_config_u8(pci_address, 0x08),
        })
    }
    
    /// PCI configuration'dan 16-bit oku
    fn pci_read_config_u16(&self, pci_address: u32, offset: u8) -> u16 {
        // Real implementation would use PCI configuration space access
        // For now, return simulated values for known NVMe controllers
        match offset {
            0x00 => 0x8086, // Intel vendor ID
            0x02 => 0x5845, // Intel NVMe controller
            0x2C => 0x8086, // Intel subsystem vendor
            0x2E => 0x5845, // Intel subsystem device
            0x0B => 0x01,   // Mass storage class
            0x08 => 0x01,   // Revision 1
            _ => 0,
        }
    }
    
    /// PCI configuration'dan 8-bit oku
    fn pci_read_config_u8(&self, pci_address: u32, offset: u8) -> u8 {
        // Real implementation would use PCI configuration space access
        match offset {
            0x0B => 0x01, // Mass storage class
            0x08 => 0x01, // Revision 1
            _ => 0,
        }
    }
    
    /// NVMe context oluştur
    fn create_nvme_context(&self, context_id: u32, device_info: &SpdkNvmeDeviceInfo) -> Result<SpdkNvmeContext, SpdkError> {
        crate::serial_println!("[SPDK] Creating NVMe context for device {:x}", device_info.pci_address);
        
        // Placeholder: gerçek implementasyonda NVMe controller'ı başlat
        let controller_handle = self.init_nvme_controller(device_info)?;
        
        // Namespace'ları tara
        let namespaces = self.scan_namespaces(controller_handle)?;
        
        // I/O queue'ları oluştur
        let mut io_queues = Vec::new();
        for i in 0..4 { // 4 I/O queue
            io_queues.push(SpdkIoQueue {
                queue_id: i as u16,
                queue_size: 1024,
                cpu_affinity: i as u32,
                active: AtomicBool::new(true),
            });
        }
        
        Ok(SpdkNvmeContext {
            context_id,
            controller_handle,
            namespaces,
            io_queues,
            polling_active: AtomicBool::new(false),
            io_stats: SpdkIoStats::new(),
        })
    }
    
    /// NVMe controller'ı başlat
    fn init_nvme_controller(&self, device_info: &SpdkNvmeDeviceInfo) -> Result<u64, SpdkError> {
        crate::serial_println!("[SPDK] Initializing NVMe controller at {:x}", device_info.pci_address);
        
        // PCIe BAR'ları (Base Address Registers) al
        let bar0 = self.pci_read_config_u32(device_info.pci_address, 0x10);
        let bar1 = self.pci_read_config_u32(device_info.pci_address, 0x14);
        
        // NVMe controller'ı enable et
        self.enable_nvme_controller(device_info.pci_address)?;
        
        // NVMe registers'ı map'le
        let nvme_regs = self.map_nvme_registers(bar0)?;
        
        // Controller reset
        self.reset_nvme_controller(nvme_regs)?;
        
        // Controller configuration
        self.configure_nvme_controller(nvme_regs)?;
        
        crate::serial_println!("[SPDK] NVMe controller initialized successfully");
        
        Ok(nvme_regs as u64)
    }
    
    /// PCI configuration'dan 32-bit oku
    fn pci_read_config_u32(&self, pci_address: u32, offset: u8) -> u32 {
        // Real implementation would use PCI configuration space access
        match offset {
            0x10 => 0xF0000000, // BAR0 - NVMe registers
            0x14 => 0x00000000, // BAR1 - Not used
            _ => 0,
        }
    }
    
    /// NVMe controller'ı enable et
    fn enable_nvme_controller(&self, pci_address: u32) -> Result<(), SpdkError> {
        crate::serial_println!("[SPDK] Enabling NVMe controller");
        
        // Command register (offset 0x04)
        let command = self.pci_read_config_u16(pci_address, 0x04);
        let new_command = command | 0x0006; // Memory Space + Bus Master
        
        // Real implementation would write to PCI command register
        crate::serial_println!("[SPDK] PCI command: 0x{:x} -> 0x{:x}", command, new_command);
        
        Ok(())
    }
    
    /// NVMe registers'ı map'le
    fn map_nvme_registers(&self, bar0: u32) -> Result<usize, SpdkError> {
        crate::serial_println!("[SPDK] Mapping NVMe registers at BAR0: 0x{:x}", bar0);
        
        // Real implementation would use memory mapping
        // For now, return a simulated address
        let nvme_regs = 0xF0000000;
        
        crate::serial_println!("[SPDK] NVMe registers mapped to: 0x{:x}", nvme_regs);
        
        Ok(nvme_regs)
    }
    
    /// NVMe controller'ı reset'le
    fn reset_nvme_controller(&self, regs: usize) -> Result<(), SpdkError> {
        crate::serial_println!("[SPDK] Resetting NVMe controller");
        
        // NVMe Controller Capabilities (offset 0x00)
        let caps = self.read_nvme_register_u32(regs, 0x00);
        crate::serial_println!("[SPDK] NVMe Capabilities: 0x{:x}", caps);
        
        // NVMe Controller Configuration (offset 0x14)
        let mut cc = self.read_nvme_register_u32(regs, 0x14);
        
        // Reset bit set et
        cc |= 0x00000001; // Enable bit
        self.write_nvme_register_u32(regs, 0x14, cc);
        
        // Reset'i bekle
        let mut timeout = 1000;
        while timeout > 0 {
            let status = self.read_nvme_register_u32(regs, 0x1C);
            if (status & 0x00000001) == 0 {
                break; // Ready
            }
            timeout -= 1;
        }
        
        if timeout == 0 {
            return Err(SpdkError::Timeout);
        }
        
        crate::serial_println!("[SPDK] NVMe controller reset completed");
        
        Ok(())
    }
    
    /// NVMe controller'ı yapılandır
    fn configure_nvme_controller(&self, regs: usize) -> Result<(), SpdkError> {
        crate::serial_println!("[SPDK] Configuring NVMe controller");
        
        // Controller Configuration register'ı ayarla
        let mut cc = self.read_nvme_register_u32(regs, 0x14);
        
        // 4KB page size, I/O submission queue size, I/O completion queue size
        cc = (cc & 0xFFFFFFF0) | 0x00000001; // Enable
        cc = (cc & 0xFFFFF0FF) | 0x00000100; // I/O queue entry size = 64 bytes
        cc = (cc & 0xFFFFFF0F) | 0x00000010; // I/O queue entry size = 64 bytes
        
        self.write_nvme_register_u32(regs, 0x14, cc);
        
        crate::serial_println!("[SPDK] NVMe controller configured");
        
        Ok(())
    }
    
    /// NVMe register'ından 32-bit oku
    fn read_nvme_register_u32(&self, regs: usize, offset: usize) -> u32 {
        // Real implementation would read from memory-mapped registers
        // For now, return simulated values
        match offset {
            0x00 => 0x12345678, // Capabilities
            0x14 => 0x00000000, // Configuration
            0x1C => 0x00000000, // Status
            _ => 0,
        }
    }
    
    /// NVMe register'ına 32-bit yaz
    fn write_nvme_register_u32(&self, regs: usize, offset: usize, value: u32) {
        // Real implementation would write to memory-mapped registers
        crate::serial_println!("[SPDK] NVMe reg[{:x}] = 0x{:x}", offset, value);
    }
    
    /// Namespace'ları tara
    fn scan_namespaces(&self, controller_handle: u64) -> Result<Vec<SpdkNamespace>, SpdkError> {
        crate::serial_println!("[SPDK] Scanning namespaces for controller {}", controller_handle);
        
        let regs = controller_handle as usize;
        let mut namespaces = Vec::new();
        
        // NVMe Identify Namespace command'ı gönder
        for nsid in 1..=32 { // Maximum 32 namespace
            if let Some(namespace) = self.identify_namespace(regs, nsid)? {
                namespaces.push(namespace);
                crate::serial_println!("[SPDK] Found namespace {}: size={}GB, block_size={}B", 
                    nsid, namespace.size / (1024 * 1024 * 1024), namespace.block_size);
            }
        }
        
        if namespaces.is_empty() {
            return Err(SpdkError::DeviceNotFound);
        }
        
        Ok(namespaces)
    }
    
    /// Namespace'i identify et
    fn identify_namespace(&self, regs: usize, nsid: u32) -> Result<Option<SpdkNamespace>, SpdkError> {
        // NVMe Identify Namespace command (opcode 0x06)
        let mut admin_queue = self.create_admin_queue(regs)?;
        
        // Identify command'ı oluştur
        let command = NvmeCommand {
            opcode: 0x06, // Identify
            flags: 0,
            command_specific: nsid,
            metadata: 0,
            prp1: 0xF1000000, // Data buffer
            prp2: 0,
            cdw10: 0x00000000, // CNS = 0 (Identify Namespace)
            cdw11: 0x00000000,
            cdw12: 0x00000000,
            cdw13: 0x00000000,
            cdw14: 0x00000000,
            cdw15: 0x00000000,
        };
        
        // Command'ı gönder
        if let Some(result) = self.submit_admin_command(&mut admin_queue, command)? {
            if result.status == 0 {
                // Namespace data'yı parse et
                let namespace = self.parse_namespace_data(nsid, &result.data)?;
                Ok(Some(namespace))
            } else {
                Ok(None) // Namespace mevcut değil
            }
        } else {
            Err(SpdkError::IoError)
        }
    }
    
    /// Admin queue oluştur
    fn create_admin_queue(&self, regs: usize) -> Result<NvmeAdminQueue, SpdkError> {
        // Admin Submission Queue (SQ) ve Completion Queue (CQ) oluştur
        let sq_size = 64; // 64 entries
        let cq_size = 64; // 64 entries
        
        // Memory allocation (real implementation would use DMA)
        let sq_addr = 0xF2000000;
        let cq_addr = 0xF2004000;
        
        // Admin Queue Attributes'ı ayarla
        let aqa = ((cq_size - 1) << 16) | (sq_size - 1);
        self.write_nvme_register_u32(regs, 0x24, aqa);
        
        // Admin Submission Queue Address
        self.write_nvme_register_u32(regs, 0x28, (sq_addr & 0xFFFFFFFF) as u32);
        self.write_nvme_register_u32(regs, 0x2C, (sq_addr >> 32) as u32);
        
        // Admin Completion Queue Address
        self.write_nvme_register_u32(regs, 0x30, (cq_addr & 0xFFFFFFFF) as u32);
        self.write_nvme_register_u32(regs, 0x34, (cq_addr >> 32) as u32);
        
        Ok(NvmeAdminQueue {
            sq_addr,
            cq_addr,
            sq_size,
            cq_size,
            sq_head: 0,
            sq_tail: 0,
            cq_head: 0,
            cq_tail: 0,
        })
    }
    
    /// Admin command gönder
    fn submit_admin_command(&self, queue: &mut NvmeAdminQueue, command: NvmeCommand) -> Result<Option<NvmeCompletion>, SpdkError> {
        crate::serial_println!("[SPDK] Submitting admin command: opcode=0x{:x}", command.opcode);
        
        // Command'ı submission queue'ya yaz
        self.write_sq_entry(queue, command)?;
        
        // Doorbell register'ına yaz
        self.write_nvme_register_u32(0xF0000000, 0x1000, queue.sq_tail as u32);
        
        // Completion'u bekle
        let timeout = 1000;
        for _ in 0..timeout {
            if let Some(completion) = self.check_cq_entry(queue)? {
                return Ok(Some(completion));
            }
        }
        
        Err(SpdkError::Timeout)
    }
    
    /// Submission Queue entry yaz
    fn write_sq_entry(&self, queue: &mut NvmeAdminQueue, command: NvmeCommand) {
        let entry_addr = queue.sq_addr + (queue.sq_tail * 16);
        
        // Real implementation would write to memory
        crate::serial_println!("[SPDK] SQ[{}] = opcode=0x{:x}", queue.sq_tail, command.opcode);
        
        queue.sq_tail = (queue.sq_tail + 1) % queue.sq_size;
    }
    
    /// Completion Queue entry kontrol et
    fn check_cq_entry(&self, queue: &mut NvmeAdminQueue) -> Result<Option<NvmeCompletion>, SpdkError> {
        if queue.cq_head != queue.cq_tail {
            let entry_addr = queue.cq_addr + (queue.cq_head * 16);
            
            // Real implementation would read from memory
            let completion = NvmeCompletion {
                command_id: queue.cq_head as u16,
                status: 0,
                result: 0,
                data: vec![0; 4096], // 4KB identify data
            };
            
            queue.cq_head = (queue.cq_head + 1) % queue.cq_size;
            
            // Doorbell register'ına yaz
            self.write_nvme_register_u32(0xF0000000, 0x1001, queue.cq_head as u32);
            
            Ok(Some(completion))
        } else {
            Ok(None)
        }
    }
    
    /// Namespace data'yı parse et
    fn parse_namespace_data(&self, nsid: u32, data: &[u8]) -> Result<SpdkNamespace, SpdkError> {
        // NVMe Identify Namespace data structure
        // Bytes 0-7: Namespace Size (in LBA)
        let ns_size = u64::from_le_bytes([data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7]]);
        
        // Bytes 8-11: Namespace Capacity (in LBA)
        let _ns_capacity = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
        
        // Bytes 12-15: Namespace Utilization (in LBA)
        let _ns_utilization = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
        
        // Bytes 16-19: Namespace Features
        let _ns_features = u32::from_le_bytes([data[16], data[17], data[18], data[19]]);
        
        // Bytes 20-23: Number of LBA Formats
        let _num_lba_formats = u8::from_le_bytes([data[23]]);
        
        // Bytes 24-25: Formatted LBA Size
        let formatted_lba_size = u8::from_le_bytes([data[24]]);
        
        // Calculate block size (2^formatted_lba_size)
        let block_size = 1u32 << formatted_lba_size;
        
        Ok(SpdkNamespace {
            nsid,
            size: ns_size,
            block_size,
            lba_format: SpdkLbaFormat {
                lbaf: formatted_lba_size,
                ms: 0, // Metadata size
                endianness: false,
            },
        })
    }
    
    /// I/O queue'ları kur
    fn setup_io_queues(&self) -> Result<(), SpdkError> {
        crate::serial_println!("[SPDK] Setting up I/O queues");
        
        // CPU sayısını al
        let cpu_count = self.get_cpu_count();
        
        // Her CPU için I/O queue oluştur
        for i in 0..cpu_count {
            let queue_id = i as u16;
            
            // I/O queue oluştur
            let io_queue = self.create_io_queue(queue_id, i)?;
            
            crate::serial_println!("[SPDK] Created I/O queue {} for CPU {}", queue_id, i);
        }
        
        Ok(())
    }
    
    /// CPU sayısını al
    fn get_cpu_count(&self) -> usize {
        // Real implementation would read CPU count from hardware
        // For now, return 4 CPUs
        4
    }
    
    /// I/O queue oluştur
    fn create_io_queue(&self, queue_id: u16, cpu_id: usize) -> Result<(), SpdkError> {
        crate::serial_println!("[SPDK] Creating I/O queue {} for CPU {}", queue_id, cpu_id);
        
        // I/O queue boyutu
        let queue_size = 1024;
        
        // Memory allocation (real implementation would use DMA)
        let sq_addr = 0xF5000000 + (queue_id as usize) * 0x10000; // 64KB per queue
        let cq_addr = sq_addr + 0x8000; // 32KB for CQ
        
        // Queue'ları context'lere ekle
        let contexts = self.nvme_contexts.lock();
        for context in contexts.values() {
            let mut ctx = context.lock();
            
            // I/O queue oluştur ve ekle
            let io_queue = SpdkIoQueue {
                queue_id,
                queue_size: queue_size as u16,
                cpu_affinity: cpu_id as u32,
                active: AtomicBool::new(true),
            };
            
            ctx.io_queues.push(io_queue);
        }
        
        crate::serial_println!("[SPDK] I/O queue {} created successfully", queue_id);
        
        Ok(())
    }
    
    /// I/O request gönder
    pub fn submit_io(&self, request: SpdkIoRequest) -> Result<(), SpdkError> {
        if !self.active.load(Ordering::SeqCst) {
            return Err(SpdkError::InitializationFailed);
        }
        
        let request_id = request.request_id;
        
        // Request'i queue'ya ekle
        {
            let mut requests = self.io_requests.lock();
            requests.insert(request_id, request);
        }
        
        // I/O'yu işle
        self.process_io_request(request_id)?;
        
        Ok(())
    }
    
    /// I/O request'i işle
    fn process_io_request(&self, request_id: u64) -> Result<(), SpdkError> {
        let request = {
            let requests = self.io_requests.lock();
            requests.get(&request_id).cloned()
        };
        
        if let Some(request) = request {
            let start_time = crate::interrupts::get_ticks();
            
            // I/O'yu işle
            match request.io_type {
                SpdkIoType::Nvme => {
                    // NVMe I/O işle
                    self.process_nvme_io(&request)?;
                }
                SpdkIoType::Tcp => {
                    // TCP I/O işle
                    self.process_tcp_io(&request)?;
                }
                SpdkIoType::Rdma => {
                    // RDMA I/O işle
                    self.process_rdma_io(&request)?;
                }
                _ => return Err(SpdkError::NotSupported),
            }
            
            let end_time = crate::interrupts::get_ticks();
            let latency_ns = (end_time - start_time) * 1000; // ticks to ns (placeholder)
            
            // İstatistikleri güncelle
            self.global_stats.record_io(request.io_type, request.data_buffer.len() as u64, latency_ns);
            
            // Request'i tamamla
            request.complete(SpdkError::InitializationFailed); // Placeholder error
            
            // Request'i temizle
            {
                let mut requests = self.io_requests.lock();
                requests.remove(&request_id);
            }
        }
        
        Ok(())
    }
    
    /// TCP I/O işle
    fn process_tcp_io(&self, request: &SpdkIoRequest) -> Result<(), SpdkError> {
        crate::serial_println!("[SPDK] Processing TCP I/O: req_id={}", request.request_id);
        
        // TCP socket'i al
        let socket_id = request.nsid as u32; // Use nsid as socket_id for TCP
        
        // TCP I/O operation'ı gerçekleştir
        if request.data_buffer.is_empty() {
            // Read operation
            let read_result = self.tcp_read(socket_id, request.lba as usize, request.block_count as usize)?;
            crate::serial_println!("[SPDK] TCP read completed: {} bytes", read_result);
        } else {
            // Write operation
            let write_result = self.tcp_write(socket_id, request.lba as usize, &request.data_buffer)?;
            crate::serial_println!("[SPDK] TCP write completed: {} bytes", write_result);
        }
        
        Ok(())
    }
    
    /// TCP read
    fn tcp_read(&self, socket_id: u32, offset: usize, size: usize) -> Result<usize, SpdkError> {
        crate::serial_println!("[SPDK] TCP read: socket={}, offset={}, size={}", socket_id, offset, size);
        
        // Real implementation would use TCP socket API
        // For now, simulate read
        Ok(size)
    }
    
    /// TCP write
    fn tcp_write(&self, socket_id: u32, offset: usize, data: &[u8]) -> Result<usize, SpdkError> {
        crate::serial_println!("[SPDK] TCP write: socket={}, offset={}, size={}", socket_id, offset, data.len());
        
        // Real implementation would use TCP socket API
        // For now, simulate write
        Ok(data.len())
    }
    
    /// RDMA I/O işle
    fn process_rdma_io(&self, request: &SpdkIoRequest) -> Result<(), SpdkError> {
        crate::serial_println!("[SPDK] Processing RDMA I/O: req_id={}", request.request_id);
        
        // RDMA queue pair'i al
        let qp_id = request.nsid as u32; // Use nsid as QP ID for RDMA
        
        // RDMA operation'ı gerçekleştir
        if request.data_buffer.is_empty() {
            // RDMA Read
            let read_result = self.rdma_read(qp_id, request.lba, request.block_count)?;
            crate::serial_println!("[SPDK] RDMA read completed: {} bytes", read_result);
        } else {
            // RDMA Write
            let write_result = self.rdma_write(qp_id, request.lba, &request.data_buffer)?;
            crate::serial_println!("[SPDK] RDMA write completed: {} bytes", write_result);
        }
        
        Ok(())
    }
    
    /// RDMA read
    fn rdma_read(&self, qp_id: u32, remote_addr: u64, size: u32) -> Result<usize, SpdkError> {
        crate::serial_println!("[SPDK] RDMA read: qp={}, addr=0x{:x}, size={}", qp_id, remote_addr, size);
        
        // Real implementation would use RDMA verbs
        // For now, simulate read
        Ok(size as usize)
    }
    
    /// RDMA write
    fn rdma_write(&self, qp_id: u32, remote_addr: u64, data: &[u8]) -> Result<usize, SpdkError> {
        crate::serial_println!("[SPDK] RDMA write: qp={}, addr=0x{:x}, size={}", qp_id, remote_addr, data.len());
        
        // Real implementation would use RDMA verbs
        // For now, simulate write
        Ok(data.len())
    }
    
    /// NVMe I/O işle
    fn process_nvme_io(&self, request: &SpdkIoRequest) -> Result<(), SpdkError> {
        crate::serial_println!("[SPDK] Processing NVMe I/O: req_id={}, nsid={}, lba={}, blocks={}", 
            request.request_id, request.nsid, request.lba, request.block_count);
        
        // NVMe I/O command'ı oluştur
        let opcode = if request.data_buffer.is_empty() {
            0x02 // Read
        } else {
            0x01 // Write
        };
        
        let command = NvmeCommand {
            opcode,
            flags: 0,
            command_specific: request.nsid,
            metadata: 0,
            prp1: 0xF3000000 + (request.lba * 4096) as u64, // Data buffer
            prp2: 0,
            cdw10: request.lba,
            cdw11: request.block_count - 1,
            cdw12: 0,
            cdw13: 0,
            cdw14: 0,
            cdw15: 0,
        };
        
        // I/O queue'yu al
        let io_queue = self.get_io_queue(0)?; // Queue 0
        
        // Command'ı gönder
        if let Some(completion) = self.submit_io_command(io_queue, command)? {
            if completion.status == 0 {
                crate::serial_println!("[SPDK] NVMe I/O completed successfully");
                Ok(())
            } else {
                crate::serial_println!("[SPDK] NVMe I/O failed with status: 0x{:x}", completion.status);
                Err(SpdkError::IoError)
            }
        } else {
            Err(SpdkError::Timeout)
        }
    }
    
    /// I/O queue al
    fn get_io_queue(&self, queue_id: u16) -> Result<&mut NvmeIoQueue, SpdkError> {
        // Real implementation would get the I/O queue from the context
        // For now, create a temporary queue
        Ok(&mut NvmeIoQueue {
            queue_id,
            queue_size: 1024,
            sq_addr: 0xF4000000,
            cq_addr: 0xF4040000,
            sq_head: 0,
            sq_tail: 0,
            cq_head: 0,
            cq_tail: 0,
        })
    }
    
    /// I/O command gönder
    fn submit_io_command(&self, queue: &mut NvmeIoQueue, command: NvmeCommand) -> Result<Option<NvmeCompletion>, SpdkError> {
        crate::serial_println!("[SPDK] Submitting I/O command: opcode=0x{:x}", command.opcode);
        
        // Command'ı submission queue'ya yaz
        self.write_io_sq_entry(queue, command)?;
        
        // Doorbell register'ına yaz
        self.write_nvme_register_u32(0xF0000000, 0x1008 + queue.queue_id as u32, queue.sq_tail as u32);
        
        // Completion'u bekle
        let timeout = 1000;
        for _ in 0..timeout {
            if let Some(completion) = self.check_io_cq_entry(queue)? {
                return Ok(Some(completion));
            }
        }
        
        Err(SpdkError::Timeout)
    }
    
    /// I/O Submission Queue entry yaz
    fn write_io_sq_entry(&self, queue: &mut NvmeIoQueue, command: NvmeCommand) {
        let entry_addr = queue.sq_addr + (queue.sq_tail * 16);
        
        // Real implementation would write to memory
        crate::serial_println!("[SPDK] I/O SQ[{}] = opcode=0x{:x}", queue.sq_tail, command.opcode);
        
        queue.sq_tail = (queue.sq_tail + 1) % queue.queue_size;
    }
    
    /// I/O Completion Queue entry kontrol et
    fn check_io_cq_entry(&self, queue: &mut NvmeIoQueue) -> Result<Option<NvmeCompletion>, SpdkError> {
        if queue.cq_head != queue.cq_tail {
            let entry_addr = queue.cq_addr + (queue.cq_head * 16);
            
            // Real implementation would read from memory
            let completion = NvmeCompletion {
                command_id: queue.cq_head as u16,
                status: 0,
                result: 0,
                data: vec![],
            };
            
            queue.cq_head = (queue.cq_head + 1) % queue.queue_size;
            
            // Doorbell register'ına yaz
            self.write_nvme_register_u32(0xF0000000, 0x1008 + queue.queue_id as u32 + 0x1000, queue.cq_head as u32);
            
            Ok(Some(completion))
        } else {
            Ok(None)
        }
    }
}

/// NVMe I/O Queue
#[derive(Clone, Debug)]
struct NvmeIoQueue {
    queue_id: u16,
    queue_size: usize,
    sq_addr: usize,
    cq_addr: usize,
    sq_head: usize,
    sq_tail: usize,
    cq_head: usize,
    cq_tail: usize,
}
    
    /// SPDK'yi durdur
    pub fn shutdown(&self) -> Result<(), SpdkError> {
        if !self.active.load(Ordering::SeqCst) {
            return Ok(());
        }
        
        crate::serial_println!("[SPDK] Shutting down SPDK");
        
        // I/O queue'ları durdur
        self.stop_io_queues()?;
        
        // NVMe context'lerini temizle
        {
            let mut contexts = self.nvme_contexts.lock();
            contexts.clear();
        }
        
        self.active.store(false, Ordering::SeqCst);
        
        crate::serial_println!("[SPDK] SPDK shutdown completed");
        
        Ok(())
    }
    
    /// I/O queue'larını durdur
    fn stop_io_queues(&self) -> Result<(), SpdkError> {
        crate::serial_println!("[SPDK] Stopping I/O queues");
        
        let contexts = self.nvme_contexts.lock();
        for context in contexts.values() {
            let ctx = context.lock();
            ctx.polling_active.store(false, Ordering::SeqCst);
        }
        
        Ok(())
    }
    
    /// İstatistikleri al
    pub fn get_stats(&self) -> SpdkStats {
        let contexts = self.nvme_contexts.lock();
        
        let mut total_contexts = 0;
        let mut active_contexts = 0;
        let mut total_namespaces = 0;
        let mut total_io_queues = 0;
        
        for context in contexts.values() {
            let ctx = context.lock();
            total_contexts += 1;
            
            if ctx.polling_active.load(Ordering::SeqCst) {
                active_contexts += 1;
            }
            
            total_namespaces += ctx.namespaces.len();
            total_io_queues += ctx.io_queues.len();
        }
        
        SpdkStats {
            active: self.active.load(Ordering::SeqCst),
            total_contexts,
            active_contexts,
            total_namespaces,
            total_io_queues,
            global_stats: self.global_stats.clone(),
        }
    }


/// NVMe command structure
#[derive(Clone, Debug)]
struct NvmeCommand {
    opcode: u8,
    flags: u8,
    command_specific: u32,
    metadata: u64,
    prp1: u64,
    prp2: u64,
    cdw10: u32,
    cdw11: u32,
    cdw12: u32,
    cdw13: u32,
    cdw14: u32,
    cdw15: u32,
}

/// NVMe completion structure
#[derive(Clone, Debug)]
struct NvmeCompletion {
    command_id: u16,
    status: u16,
    result: u32,
    data: Vec<u8>,
}

/// NVMe Admin Queue
#[derive(Clone, Debug)]
struct NvmeAdminQueue {
    sq_addr: usize,
    cq_addr: usize,
    sq_size: usize,
    cq_size: usize,
    sq_head: usize,
    sq_tail: usize,
    cq_head: usize,
    cq_tail: usize,
}

/// NVMe cihaz bilgisi
#[derive(Clone, Debug)]
pub struct SpdkNvmeDeviceInfo {
    pub pci_address: u32,
    pub vendor_id: u16,
    pub device_id: u16,
    pub subsystem_vendor_id: u16,
    pub subsystem_device_id: u16,
    pub class_code: u8,
    pub revision: u8,
}

/// SPDK istatistikleri
#[derive(Clone, Debug)]
pub struct SpdkStats {
    pub active: bool,
    pub total_contexts: usize,
    pub active_contexts: usize,
    pub total_namespaces: usize,
    pub total_io_queues: usize,
    pub global_stats: SpdkIoStats,
}

impl Default for SpdkManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// GLOBAL SPDK MANAGER
// ============================================================================

/// Global SPDK manager
static SPDK_MANAGER: SpdkManager = SpdkManager::new();

/// SPDK manager'ı al
pub fn get_manager() -> &'static SpdkManager {
    &SPDK_MANAGER
}

// ============================================================================
// PUBLIC API
// ============================================================================

/// SPDK'yi başlat
pub fn init_spdk() -> Result<(), SpdkError> {
    get_manager().init()
}

/// NVMe read I/O gönder
pub fn submit_nvme_read(nsid: u32, lba: u64, block_count: u32, callback: SpdkIoCallback) -> Result<u64, SpdkError> {
    let manager = get_manager();
    let request_id = manager.next_request_id.fetch_add(1, Ordering::SeqCst);
    
    let mut request = SpdkIoRequest::new_read(request_id, nsid, lba, block_count);
    request.set_callback(callback);
    
    manager.submit_io(request)?;
    
    Ok(request_id)
}

/// NVMe write I/O gönder
pub fn submit_nvme_write(nsid: u32, lba: u64, data: Vec<u8>, callback: SpdkIoCallback) -> Result<u64, SpdkError> {
    let manager = get_manager();
    let request_id = manager.next_request_id.fetch_add(1, Ordering::SeqCst);
    
    let block_count = (data.len() / 4096) as u32; // 4KB blok boyutu
    let mut request = SpdkIoRequest::new_write(request_id, nsid, lba, block_count, data);
    request.set_callback(callback);
    
    manager.submit_io(request)?;
    
    Ok(request_id)
}

/// SPDK istatistiklerini al
pub fn get_spdk_stats() -> SpdkStats {
    get_manager().get_stats()
}

/// SPDK testi
pub fn test_spdk() -> Result<(), SpdkError> {
    crate::serial_println!("[SPDK] Testing SPDK integration");
    
    // SPDK'yi başlat
    init_spdk()?;
    
    // Test callback
    let test_callback = |request: &SpdkIoRequest, error: SpdkError| {
        crate::serial_println!("[SPDK] I/O completed: req_id={}, error={:?}", 
            request.request_id, error);
    };
    
    // Test read I/O
    let read_id = submit_nvme_read(1, 0, 1, test_callback)?;
    crate::serial_println!("[SPDK] Submitted read I/O: id={}", read_id);
    
    // Test write I/O
    let test_data = vec![0xAA; 4096]; // 4KB test data
    let write_id = submit_nvme_write(1, 1000, test_data, test_callback)?;
    crate::serial_println!("[SPDK] Submitted write I/O: id={}", write_id);
    
    // İstatistikleri göster
    let stats = get_spdk_stats();
    crate::serial_println!("[SPDK] Stats:");
    crate::serial_println!("  Active: {}", stats.active);
    crate::serial_println!("  Contexts: {}/{}", stats.active_contexts, stats.total_contexts);
    crate::serial_println!("  Namespaces: {}", stats.total_namespaces);
    crate::serial_println!("  I/O Queues: {}", stats.total_io_queues);
    crate::serial_println!("  Total I/Os: {}", stats.global_stats.total_ios.load(Ordering::SeqCst));
    crate::serial_println!("  Avg Latency: {} ns", stats.global_stats.avg_latency_ns.load(Ordering::SeqCst));
    
    // SPDK'yi durdur
    get_manager().shutdown()?;
    
    crate::serial_println!("[SPDK] SPDK test completed");
    
    Ok(())
}
