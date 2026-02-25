use crate::memory::{global_memory_manager_mut, PHYSICAL_MEMORY_OFFSET};
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::ops::{Deref, DerefMut};
use core::ptr::{self, NonNull};
use core::slice;
use x86_64::structures::paging::{PhysFrame, Size4KiB};
use x86_64::PhysAddr;

/// Kernel Stack Allocator.
///
/// Allocates contiguous physical memory pages for kernel stacks directly from PMM,
/// bypassing the global heap allocator to prevent fragmentation.
///
/// Uses direct mapping (Physical Address + Offset) to access memory.
#[derive(Debug)]
pub struct KernelStack {
    ptr: NonNull<u8>,
    pages: usize,
    layout: core::alloc::Layout, // Not used, but kept for future alignment
}

// Send + Sync because it owns the memory
unsafe impl Send for KernelStack {}
unsafe impl Sync for KernelStack {}

impl KernelStack {
    /// Allocates a new Kernel Stack with the given size (in bytes).
    /// Size will be rounded up to page boundaries.
    pub fn new(size_in_bytes: usize) -> Option<Self> {
        if size_in_bytes == 0 {
            return None;
        }

        let pages = (size_in_bytes + 4095) / 4096;
        
        let mm = unsafe { global_memory_manager_mut() }?;
        let frame = mm.allocate_contiguous_frames(pages)?;
        
        let phys_addr = frame.start_address();
        let virt_addr = phys_addr.as_u64() + PHYSICAL_MEMORY_OFFSET;
        
        let ptr = NonNull::new(virt_addr as *mut u8)?;

        // Zero the memory for security and determinism
        unsafe {
            ptr::write_bytes(ptr.as_ptr(), 0, pages * 4096);
        }
        
        Some(Self {
            ptr,
            pages,
            layout: core::alloc::Layout::from_size_align(pages * 4096, 4096).unwrap(),
        })
    }

    /// Returns the physical address of the stack start.
    pub fn phys_addr(&self) -> PhysAddr {
        let virt_addr = self.ptr.as_ptr() as u64;
        
        if virt_addr >= PHYSICAL_MEMORY_OFFSET {
            // HHDM-mapped stack: use direct offset calculation
            PhysAddr::new(virt_addr - PHYSICAL_MEMORY_OFFSET)
        } else {
            // Heap-allocated stack: use page table translation
            use x86_64::VirtAddr;
            crate::memory::paging::translate_addr(VirtAddr::new(virt_addr))
                .expect("KernelStack virtual address is not mapped")
        }
    }

    /// Returns the virtual address of the stack start.
    pub fn as_ptr(&self) -> *const u8 {
        self.ptr.as_ptr()
    }

    /// Returns the mutable virtual address of the stack start.
    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.ptr.as_ptr()
    }

    /// Returns the size of the stack in bytes.
    pub fn len(&self) -> usize {
        self.pages * 4096
    }
}

impl Drop for KernelStack {
    fn drop(&mut self) {
        let mm = unsafe { global_memory_manager_mut() };
        if let Some(mm) = mm {
            let start_frame = PhysFrame::containing_address(self.phys_addr());
            mm.deallocate_contiguous_frames(start_frame, self.pages);
        }
    }
}

impl Deref for KernelStack {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        unsafe { slice::from_raw_parts(self.ptr.as_ptr(), self.pages * 4096) }
    }
}

impl DerefMut for KernelStack {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { slice::from_raw_parts_mut(self.ptr.as_ptr(), self.pages * 4096) }
    }
}

impl Clone for KernelStack {
    fn clone(&self) -> Self {
        // Deep copy needed because we own the memory
        let new_stack = Self::new(self.len()).expect("Failed to allocate stack clone");
        unsafe {
            ptr::copy_nonoverlapping(self.as_ptr(), new_stack.ptr.as_ptr(), self.len());
        }
        new_stack
    }
}
