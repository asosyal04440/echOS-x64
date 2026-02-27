use crate::memory::{
    dma_alloc_for_domain, dma_dealloc_for_domain, dma_share_for_domain, dma_unshare_for_domain,
    map_mmio,
};
use core::ptr::NonNull;
use virtio_drivers::{BufferDirection, Hal, PhysAddr};

pub struct VirtioHal;

unsafe impl Hal for VirtioHal {
    fn dma_alloc(pages: usize, _direction: BufferDirection) -> (PhysAddr, NonNull<u8>) {
        let domain = crate::cpu::smp::current_dma_domain();
        crate::serial_println!("[VirtioHal] dma_alloc: {} pages, domain={}", pages, domain);
        
        match dma_alloc_for_domain(domain, pages) {
            Some((paddr, vaddr)) => {
                crate::serial_println!("[VirtioHal] dma_alloc OK: paddr={:#x}", paddr);
                assert!(paddr != 0, "DMA alloc returned physical address 0x0");
                (paddr, vaddr)
            }
            None => {
                crate::serial_println!("[VirtioHal] dma_alloc FAILED: {} pages for domain {}", pages, domain);
                panic!("[VirtioHal] DMA allocation failed")
            }
        }
    }

    unsafe fn dma_dealloc(paddr: PhysAddr, _vaddr: NonNull<u8>, pages: usize) -> i32 {
        if paddr == 0 || pages == 0 {
            return -1;
        }
        let domain = crate::cpu::smp::current_dma_domain();
        dma_dealloc_for_domain(domain, paddr, pages);
        0
    }

    unsafe fn mmio_phys_to_virt(paddr: PhysAddr, _size: usize) -> NonNull<u8> {
        if !crate::ironshim_bridge::is_mmio_allowed(paddr as usize, _size) {
            crate::serial_println!(
                "[IronShim/MMIO] map denied: base={:#x} size={}",
                paddr,
                _size
            );
            return NonNull::dangling();
        }
        let ptr = map_mmio(paddr as u64, _size);
        NonNull::new(ptr).unwrap()
    }

    unsafe fn share(buffer: NonNull<[u8]>, _direction: BufferDirection) -> PhysAddr {
        let domain = crate::cpu::smp::current_dma_domain();
        dma_share_for_domain(domain, buffer)
            .expect("[VIRTIO] DMA share failed: unmapped buffer — potential memory corruption")
    }

    unsafe fn unshare(_paddr: PhysAddr, buffer: NonNull<[u8]>, _direction: BufferDirection) {
        let domain = crate::cpu::smp::current_dma_domain();
        dma_unshare_for_domain(domain, buffer);
    }
}
