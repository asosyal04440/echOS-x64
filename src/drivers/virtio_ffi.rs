use core::sync::atomic::{AtomicU16, Ordering};
use rcore_fs::dev::{Device, Result as DevResult};
use spin::Mutex;

fn virtio_disk_init(_base_port: u16) {
    crate::serial_println!("VIRTIO FFI: virtio_disk_init stub (no C backend)");
}

fn virtio_disk_rw(_sector: u64, _buf: *mut u8, _write: i32) {
    crate::serial_println!("VIRTIO FFI: virtio_disk_rw stub (no C backend)");
}

static LOCK: Mutex<()> = Mutex::new(());
static BASE_PORT: AtomicU16 = AtomicU16::new(0);

#[no_mangle]
pub extern "C" fn virt_to_phys_c(ptr: *const u8) -> u64 {
    let vaddr = ptr as u64;
    match crate::memory::translate_addr(vaddr) {
        Some(paddr) => paddr,
        None => {
            crate::serial_println!("[VIRTIO FFI] virt_to_phys_c failed for vaddr={:#x}", vaddr);
            panic!("virt_to_phys_c: unmapped virtual address");
        }
    }
}

pub fn init(base_port: u16) {
    crate::serial_println!("VIRTIO FFI: init base_port=0x{:x}", base_port);
    unsafe {
        virtio_disk_init(base_port);
    }
    BASE_PORT.store(base_port, Ordering::SeqCst);
    crate::serial_println!("VIRTIO FFI: init done");
}

pub fn device() -> Option<VirtioBlock> {
    let base_port = BASE_PORT.load(Ordering::SeqCst);
    if base_port == 0 {
        None
    } else {
        crate::serial_println!("VIRTIO FFI: device ready base_port=0x{:x}", base_port);
        Some(VirtioBlock { base_port })
    }
}

pub struct VirtioBlock {
    base_port: u16,
}

impl VirtioBlock {
    pub fn read_sector(&self, sector: u64, buf: &mut [u8; 512]) {
        let _guard = LOCK.lock();
        crate::serial_println!("VIRTIO FFI: read sector={}", sector);
        unsafe {
            virtio_disk_rw(sector, buf.as_mut_ptr(), 0);
        }
    }

    pub fn write_sector(&self, sector: u64, buf: &[u8; 512]) {
        let _guard = LOCK.lock();
        crate::serial_println!("VIRTIO FFI: write sector={}", sector);
        unsafe {
            virtio_disk_rw(sector, buf.as_ptr() as *mut u8, 1);
        }
    }
}

impl Device for VirtioBlock {
    fn read_at(&self, offset: usize, buf: &mut [u8]) -> DevResult<usize> {
        let _guard = LOCK.lock();
        crate::serial_println!("VIRTIO FFI: read_at offset={} len={}", offset, buf.len());
        let mut sector_buf = [0u8; 512];
        let mut done = 0usize;
        let mut cur_offset = offset;
        while done < buf.len() {
            let sector = (cur_offset / 512) as u64;
            let within = cur_offset % 512;
            unsafe {
                virtio_disk_rw(sector, sector_buf.as_mut_ptr(), 0);
            }
            let to_copy = core::cmp::min(512 - within, buf.len() - done);
            buf[done..done + to_copy].copy_from_slice(&sector_buf[within..within + to_copy]);
            done += to_copy;
            cur_offset += to_copy;
        }
        Ok(done)
    }

    fn write_at(&self, offset: usize, buf: &[u8]) -> DevResult<usize> {
        let _guard = LOCK.lock();
        crate::serial_println!("VIRTIO FFI: write_at offset={} len={}", offset, buf.len());
        let mut sector_buf = [0u8; 512];
        let mut done = 0usize;
        let mut cur_offset = offset;
        while done < buf.len() {
            let sector = (cur_offset / 512) as u64;
            let within = cur_offset % 512;
            unsafe {
                virtio_disk_rw(sector, sector_buf.as_mut_ptr(), 0);
            }
            let to_copy = core::cmp::min(512 - within, buf.len() - done);
            sector_buf[within..within + to_copy].copy_from_slice(&buf[done..done + to_copy]);
            unsafe {
                virtio_disk_rw(sector, sector_buf.as_mut_ptr(), 1);
            }
            done += to_copy;
            cur_offset += to_copy;
        }
        Ok(done)
    }

    fn sync(&self) -> DevResult<()> {
        Ok(())
    }
}
