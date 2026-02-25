use core::sync::atomic::{AtomicU32, Ordering};
use log::{error, info};
use spin::Mutex;
use virtio_drivers::device::blk::{BlkReq, BlkResp, VirtIOBlk};
use virtio_drivers::transport::pci::PciTransport;
use virtio_drivers::{BufferDirection, Hal};

use super::virtio_hal::VirtioHal;

static BLK_DEV: Mutex<Option<VirtIOBlk<VirtioHal, PciTransport>>> = Mutex::new(None);
static BLK_DMA_DOMAIN: AtomicU32 = AtomicU32::new(0);

const SECTOR_SIZE: usize = 512;

pub fn init(transport: PciTransport) -> bool {
    crate::serial_println!("VIRTIO BLK: init start");
    let df = transport.device_function();
    let domain = crate::memory::iommu_register_device(df.bus, df.device, df.function);
    BLK_DMA_DOMAIN.store(domain, Ordering::Release);
    let prev_domain = crate::cpu::smp::current_dma_domain();
    crate::cpu::smp::set_current_dma_domain(domain);
    let driver = match VirtIOBlk::<VirtioHal, _>::new(transport) {
        Ok(value) => value,
        Err(err) => {
            crate::cpu::smp::set_current_dma_domain(prev_domain);
            crate::serial_println!("VIRTIO BLK: init failed: {:?}", err);
            error!("VIRTIO BLK: init failed: {:?}", err);
            return false;
        }
    };
    crate::cpu::smp::set_current_dma_domain(prev_domain);
    let capacity = driver.capacity();
    *BLK_DEV.lock() = Some(driver);
    crate::serial_println!("VIRTIO BLK: init ok capacity={} sectors", capacity);
    info!("VIRTIO BLK: init ok capacity={} sectors", capacity);
    true
}

fn with_blk_domain<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    let prev_domain = crate::cpu::smp::current_dma_domain();
    let domain = BLK_DMA_DOMAIN.load(Ordering::Acquire);
    crate::cpu::smp::set_current_dma_domain(domain);
    let result = f();
    crate::cpu::smp::set_current_dma_domain(prev_domain);
    result
}

pub fn read_sector(lba: u64, buffer: &mut [u8]) -> Result<(), &'static str> {
    with_blk_domain(|| {
        crate::serial_println!("VIRTIO BLK: read start lba={} len={}", lba, buffer.len());
        if buffer.is_empty() || buffer.len() % SECTOR_SIZE != 0 {
            crate::serial_println!("VIRTIO BLK: read invalid buffer size");
            return Err("Invalid buffer size");
        }
        let mut guard = BLK_DEV.lock();
        let Some(device) = guard.as_mut() else {
            crate::serial_println!("VIRTIO BLK: read device not initialized");
            return Err("Device not initialized");
        };
        let sectors = buffer.len() / SECTOR_SIZE;
        let (paddr, vaddr) = <VirtioHal as Hal>::dma_alloc(1, BufferDirection::Both);
        if paddr == 0 {
            crate::serial_println!("VIRTIO BLK: read dma alloc failed");
            return Err("Disk Error");
        }
        let base = vaddr.as_ptr() as usize;
        let mut offset = 0usize;
        offset =
            (offset + core::mem::align_of::<BlkReq>() - 1) & !(core::mem::align_of::<BlkReq>() - 1);
        let req_ptr = (base + offset) as *mut BlkReq;
        offset += core::mem::size_of::<BlkReq>();
        offset = (offset + core::mem::align_of::<BlkResp>() - 1)
            & !(core::mem::align_of::<BlkResp>() - 1);
        let resp_ptr = (base + offset) as *mut BlkResp;
        offset += core::mem::size_of::<BlkResp>();
        if offset + SECTOR_SIZE > crate::memory::PAGE_SIZE {
            unsafe { <VirtioHal as Hal>::dma_dealloc(paddr, vaddr, 1) };
            crate::serial_println!("VIRTIO BLK: read dma buffer too small");
            return Err("Disk Error");
        }
        let dma_buf =
            unsafe { core::slice::from_raw_parts_mut((base + offset) as *mut u8, SECTOR_SIZE) };
        for i in 0..sectors {
            unsafe {
                core::ptr::write(req_ptr, BlkReq::default());
                core::ptr::write(resp_ptr, BlkResp::default());
            }
            let token = match unsafe {
                device.read_blocks_nb(
                    lba as usize + i,
                    unsafe { &mut *req_ptr },
                    dma_buf,
                    unsafe { &mut *resp_ptr },
                )
            } {
                Ok(value) => value,
                Err(_) => {
                    unsafe { <VirtioHal as Hal>::dma_dealloc(paddr, vaddr, 1) };
                    crate::serial_println!("VIRTIO BLK: read error lba={}", lba + i as u64);
                    return Err("Disk Error");
                }
            };
            let mut spins: u32 = 0;
            while device.peek_used() != Some(token) {
                if spins > 5_000_000 {
                    unsafe { <VirtioHal as Hal>::dma_dealloc(paddr, vaddr, 1) };
                    crate::serial_println!("VIRTIO BLK: read timeout lba={}", lba + i as u64);
                    return Err("Timeout");
                }
                spins = spins.wrapping_add(1);
                core::hint::spin_loop();
            }
            unsafe {
                device
                    .complete_read_blocks(token, &*req_ptr, dma_buf, &mut *resp_ptr)
                    .map_err(|_| {
                        unsafe { <VirtioHal as Hal>::dma_dealloc(paddr, vaddr, 1) };
                        crate::serial_println!("VIRTIO BLK: read error lba={}", lba + i as u64);
                        "Disk Error"
                    })?;
            }
            let start = i * SECTOR_SIZE;
            let end = start + SECTOR_SIZE;
            buffer[start..end].copy_from_slice(dma_buf);
        }
        unsafe { <VirtioHal as Hal>::dma_dealloc(paddr, vaddr, 1) };
        crate::serial_println!("VIRTIO BLK: read ok lba={} sectors={}", lba, sectors);
        Ok(())
    })
}

pub fn write_sector(lba: u64, buffer: &[u8]) -> Result<(), &'static str> {
    with_blk_domain(|| {
        crate::serial_println!("VIRTIO BLK: write start lba={} len={}", lba, buffer.len());
        if buffer.is_empty() || buffer.len() % SECTOR_SIZE != 0 {
            crate::serial_println!("VIRTIO BLK: write invalid buffer size");
            return Err("Invalid buffer size");
        }
        let mut guard = BLK_DEV.lock();
        let Some(device) = guard.as_mut() else {
            crate::serial_println!("VIRTIO BLK: write device not initialized");
            return Err("Device not initialized");
        };
        let sectors = buffer.len() / SECTOR_SIZE;
        for i in 0..sectors {
            let start = i * SECTOR_SIZE;
            let end = start + SECTOR_SIZE;
            device
                .write_blocks(lba as usize + i, &buffer[start..end])
                .map_err(|_| {
                    crate::serial_println!("VIRTIO BLK: write error lba={}", lba + i as u64);
                    "Disk Error"
                })?;
        }
        crate::serial_println!("VIRTIO BLK: write ok lba={} sectors={}", lba, sectors);
        Ok(())
    })
}
