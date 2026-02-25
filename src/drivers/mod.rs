//! # echOS Donanım Sürücüleri
//!
//! Bu modül, sistem donanım sürücülerini içerir.
//! PS/2 keyboard/mouse, ATA disk ve APIC desteği.

/// Input event kuyruğu (keyboard, mouse)
pub mod input;

/// PS/2 controller sürücüsü
pub mod ps2;

/// PS/2 mouse sürücüsü
pub mod mouse;

/// ATA disk sürücüsü
pub mod ata;

/// Advanced PIC (Local APIC)
pub mod apic;

pub mod pci;
pub mod pci_root;

/// USB (xHCI) sürücüsü
pub mod usb;

/// Audio (Intel HDA) sürücüsü
pub mod audio;

/// Bluetooth sürücüsü
pub mod bluetooth;

/// NVMe sürücüsü
pub mod nvme;

/// VirtIO-Net network driver
pub mod virtio_net;

/// VirGL 3D acceleration
pub mod virgl;

pub mod virtio_blk;
pub mod virtio_ffi;
pub mod virtio_gpu;
pub mod virtio_hal;

/// Block device abstraction
pub mod block;

// Re-export block device types for convenience
pub use block::{BlockDevice, BlockDeviceError, BlockDeviceType};

pub mod linux {
    use crate::drivers::ata::{AtaDrive, BLOCK_SIZE};
    use crate::drivers::virtio_ffi;
    use crate::drivers::{mouse, pci, ps2, usb};
    use alloc::boxed::Box;
    use alloc::format;
    use alloc::string::{String, ToString};
    use alloc::vec::Vec;
    use core::sync::atomic::{AtomicBool, Ordering};
    use lazy_static::lazy_static;
    use spin::Mutex;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum LinuxDeviceKind {
        Character,
        Block,
        Other,
    }

    #[derive(Debug, Clone)]
    pub struct LinuxDevice {
        pub name: String,
        pub major: u16,
        pub minor: u16,
        pub kind: LinuxDeviceKind,
        pub bus: u8,
        pub device: u8,
        pub function: u8,
        pub class_code: u8,
        pub subclass: u8,
        pub prog_if: u8,
        pub vendor_id: u16,
        pub device_id: u16,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum LinuxDriverError {
        NotSupported,
        Io,
        Busy,
        Invalid,
        NotFound,
        Unknown,
    }

    pub trait LinuxDriver: Send + Sync {
        fn name(&self) -> &str;
        fn probe(&self, device: &LinuxDevice) -> bool;
        fn attach(&self, device: &LinuxDevice) -> Result<(), LinuxDriverError>;
        fn detach(&self, _device: &LinuxDevice) -> Result<(), LinuxDriverError> {
            Ok(())
        }
    }

    pub trait BlockDevice: Send {
        fn read_sectors(&mut self, lba: u32, count: u8) -> Vec<u8>;
        fn write_sectors(&mut self, lba: u32, data: &[u8]) -> Result<(), ()>;
    }

    pub struct VirtioBlockDevice {
        inner: virtio_ffi::VirtioBlock,
    }

    impl BlockDevice for AtaDrive {
        fn read_sectors(&mut self, lba: u32, count: u8) -> Vec<u8> {
            AtaDrive::read_sectors(self, lba, count)
        }

        fn write_sectors(&mut self, lba: u32, data: &[u8]) -> Result<(), ()> {
            AtaDrive::write_sectors(self, lba, data).map_err(|_| ())
        }
    }

    impl BlockDevice for VirtioBlockDevice {
        fn read_sectors(&mut self, lba: u32, count: u8) -> Vec<u8> {
            let mut buffer = Vec::with_capacity(count as usize * BLOCK_SIZE);
            for i in 0..count {
                let mut sector = [0u8; BLOCK_SIZE];
                self.inner.read_sector(lba as u64 + i as u64, &mut sector);
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
                self.inner.write_sector(lba as u64 + i as u64, &sector);
            }
            Ok(())
        }
    }

    #[derive(Debug, Clone)]
    pub struct LinuxAttachment {
        pub device: String,
        pub driver: String,
    }

    lazy_static! {
        static ref DRIVER_REGISTRY: Mutex<Vec<Box<dyn LinuxDriver>>> = Mutex::new(Vec::new());
        static ref DEVICE_REGISTRY: Mutex<Vec<LinuxDevice>> = Mutex::new(Vec::new());
        static ref ATTACHMENTS: Mutex<Vec<LinuxAttachment>> = Mutex::new(Vec::new());
    }
    static INIT_DONE: AtomicBool = AtomicBool::new(false);

    pub fn register_driver(driver: Box<dyn LinuxDriver>) {
        DRIVER_REGISTRY.lock().push(driver);
    }

    pub fn register_device(device: LinuxDevice) -> usize {
        let mut devices = DEVICE_REGISTRY.lock();
        devices.push(device);
        devices.len() - 1
    }

    pub fn list_devices() -> Vec<LinuxDevice> {
        DEVICE_REGISTRY.lock().clone()
    }

    pub fn list_drivers() -> Vec<String> {
        DRIVER_REGISTRY
            .lock()
            .iter()
            .map(|driver| driver.name().to_string())
            .collect()
    }

    pub fn list_attachments() -> Vec<LinuxAttachment> {
        ATTACHMENTS.lock().clone()
    }

    pub fn select_block_device() -> Result<Box<dyn BlockDevice>, LinuxDriverError> {
        let devices = list_devices();
        crate::serial_println!("BLOCK DEV COUNT: {}", devices.len());
        if devices.is_empty() {
            crate::serial_println!("BLOCK DEV: no devices registered");
        }
        for device in devices.iter() {
            crate::serial_println!(
                "DEBUG: Block scan device {} kind={:?} vendor={:04x} device={:04x} class={:02x}",
                device.name,
                device.kind,
                device.vendor_id,
                device.device_id,
                device.class_code
            );
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
            crate::serial_println!("BLOCK DEVICE NOT FOUND: virtio block missing");
            return try_ata_block_device();
        }
        crate::serial_println!("BLOCK DEVICE FOUND: {}", virtio_devices.len());
        crate::serial_println!("TRYING TO INIT DRIVER for device...");
        let mut probe = [0u8; BLOCK_SIZE];
        let virtio = match virtio_ffi::device() {
            Some(dev) => dev,
            None => {
                crate::serial_println!("BLOCK DEVICE INIT FAILED: VirtIO not initialized");
                return try_ata_block_device().map_err(|_| LinuxDriverError::NotFound);
            }
        };
        crate::serial_println!("VIRTIO FFI: read sector=0 start");
        virtio.read_sector(0, &mut probe);
        crate::serial_println!("VIRTIO FFI: read sector=0 done");
        let mut backup = [0u8; BLOCK_SIZE];
        let mut test = [0u8; BLOCK_SIZE];
        let mut verify = [0u8; BLOCK_SIZE];
        let test_lba = 1u64;
        virtio.read_sector(test_lba, &mut backup);
        for i in 0..BLOCK_SIZE {
            test[i] = (i as u8) ^ 0xA5;
        }
        virtio.write_sector(test_lba, &test);
        virtio.read_sector(test_lba, &mut verify);
        if verify == test {
            crate::serial_println!("VIRTIO FFI: write/read test OK lba={}", test_lba);
        } else {
            crate::serial_println!("VIRTIO FFI: write/read test FAILED lba={}", test_lba);
        }
        virtio.write_sector(test_lba, &backup);
        crate::serial_println!("VIRTIO FFI: write/read test restore done lba={}", test_lba);
        if probe[510] != 0x55 || probe[511] != 0xAA {
            crate::serial_println!("BLOCK DEVICE INIT FAILED: MBR signature invalid");
            return try_ata_block_device().map_err(|_| LinuxDriverError::Io);
        }
        crate::serial_println!("BLOCK DEVICE MBR OK: 0x55AA");
        crate::serial_println!("BLOCK DEVICE INIT OK");
        Ok(Box::new(VirtioBlockDevice { inner: virtio }))
    }

    fn try_ata_block_device() -> Result<Box<dyn BlockDevice>, LinuxDriverError> {
        crate::serial_println!("BLOCK DEVICE FALLBACK: ATA");
        let mut drive = AtaDrive::new(0x1F0);
        match drive.detect() {
            Ok(true) => {
                crate::serial_println!("BLOCK DEVICE ATA OK");
                Ok(Box::new(drive))
            }
            Ok(false) => {
                crate::serial_println!("BLOCK DEVICE ATA NOT FOUND");
                Err(LinuxDriverError::NotFound)
            }
            Err(_) => {
                crate::serial_println!("BLOCK DEVICE ATA ERROR");
                Err(LinuxDriverError::Io)
            }
        }
    }

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

    struct Ps2ControllerDriver;

    impl LinuxDriver for Ps2ControllerDriver {
        fn name(&self) -> &str {
            "ps2_controller"
        }

        fn probe(&self, device: &LinuxDevice) -> bool {
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

    struct Ps2MouseDriver;

    impl LinuxDriver for Ps2MouseDriver {
        fn name(&self) -> &str {
            "ps2_mouse"
        }

        fn probe(&self, device: &LinuxDevice) -> bool {
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

    struct StoragePciDriver;

    impl LinuxDriver for StoragePciDriver {
        fn name(&self) -> &str {
            "storage_pci"
        }

        fn probe(&self, device: &LinuxDevice) -> bool {
            device.class_code == 0x01
        }

        fn attach(&self, _device: &LinuxDevice) -> Result<(), LinuxDriverError> {
            Ok(())
        }
    }

    struct XhciPciDriver;

    impl LinuxDriver for XhciPciDriver {
        fn name(&self) -> &str {
            "xhci_pci"
        }

        fn probe(&self, device: &LinuxDevice) -> bool {
            device.class_code == 0x0C && device.subclass == 0x03 && device.prog_if == 0x30
        }

        fn attach(&self, _device: &LinuxDevice) -> Result<(), LinuxDriverError> {
            usb::init();
            Ok(())
        }
    }

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
            if dev.class_code == 0x01 && dev.subclass == 0x01 {
                crate::serial_println!(
                    "Skipping IDE controller {:02x}:{:02x}.{}",
                    dev.bus,
                    dev.device,
                    dev.function
                );
                continue;
            }
            if dev.vendor_id == 0x1AF4 && (dev.device_id == 0x1001 || dev.device_id == 0x1042) {
                let mut command = pci::read_config_dword(dev.bus, dev.device, dev.function, 0x04);
                command |= (1 << 0) | (1 << 1) | (1 << 2);
                pci::write_config_dword(dev.bus, dev.device, dev.function, 0x04, command);
                if let Some(bar) = pci::read_bar_io(dev.bus, dev.device, dev.function, 0) {
                    if bar.base != 0 {
                        virtio_ffi::init(bar.base as u16);
                        crate::serial_println!(
                            "VIRTIO BLK: init via legacy io base=0x{:x}",
                            bar.base
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
            let kind = match dev.class_code {
                0x01 => LinuxDeviceKind::Block,
                0x0C => LinuxDeviceKind::Character,
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

    pub fn init_linux_driver_layer() -> usize {
        if INIT_DONE.swap(true, Ordering::SeqCst) {
            return 0;
        }
        register_device(LinuxDevice {
            name: "ps2".to_string(),
            major: 10,
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
            major: 13,
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
        register_pci_devices();
        register_driver(Box::new(Ps2ControllerDriver));
        register_driver(Box::new(Ps2MouseDriver));
        register_driver(Box::new(StoragePciDriver));
        register_driver(Box::new(XhciPciDriver));
        probe_and_attach()
    }
}
