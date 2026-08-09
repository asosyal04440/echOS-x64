use core::marker::PhantomData;
use core::ops::Deref;
use x86_64::structures::paging::PhysFrame;
use x86_64::PhysAddr;

use super::unique::UniqueFrame;
use super::FRAME_TABLE;

#[derive(Debug)]
pub(crate) struct SharedAtomicFrame<T = [u8; 4096]> {
    phys: u64,
    _phantom: PhantomData<T>,
}

impl<T> SharedAtomicFrame<T> {
    pub(crate) fn from_phys(phys: u64) -> Self {
        FRAME_TABLE.lock().share(phys);
        Self {
            phys: phys & !(0xFFF),
            _phantom: PhantomData,
        }
    }

    pub(crate) fn from_phys_inner(phys: u64) -> Self {
        Self {
            phys: phys & !(0xFFF),
            _phantom: PhantomData,
        }
    }

    pub(crate) fn phys(&self) -> u64 {
        self.phys
    }

    pub(crate) fn start_address(&self) -> PhysAddr {
        PhysAddr::new(self.phys)
    }

    pub(crate) fn refcount(&self) -> u32 {
        FRAME_TABLE.lock().refcount(self.phys)
    }

    /// Atomically increment the global refcount for a physical address.
    /// Typed static helper — caller must ensure a matching decrement
    /// (via `decref` or `frame_ownership::dec_frame_ref`) when the reference is released.
    pub(crate) fn incref(phys: u64) {
        FRAME_TABLE.lock().share(phys & !(0xFFF));
    }

    /// Atomically decrement the global refcount for a physical address.
    /// Returns the remaining count. If 0, the frame will be freed (ZOMBIE
    /// lifecycle) and must not be accessed again.
    /// Typed equivalent of `dec_frame_ref`.
    pub(crate) fn decref(phys: u64) -> u32 {
        FRAME_TABLE.lock().unshare(phys & !(0xFFF))
    }
}

impl<T> Clone for SharedAtomicFrame<T> {
    fn clone(&self) -> Self {
        FRAME_TABLE.lock().share(self.phys);
        Self {
            phys: self.phys,
            _phantom: PhantomData,
        }
    }
}

impl<T> Drop for SharedAtomicFrame<T> {
    fn drop(&mut self) {
        let remaining = FRAME_TABLE.lock().unshare(self.phys);
        if remaining == 0 {
            unsafe {
                let frame = PhysFrame::containing_address(PhysAddr::new(self.phys));
                crate::memory::deallocate_contiguous_frames(frame, 1);
            }
        }
    }
}

impl<T> Deref for SharedAtomicFrame<T> {
    type Target = T;
    fn deref(&self) -> &T {
        let hhdm = crate::memory::active_physical_offset();
        let ptr = (hhdm + self.phys) as usize as *const T;
        unsafe { &*ptr }
    }
}
