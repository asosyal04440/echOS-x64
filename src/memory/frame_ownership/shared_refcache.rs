use core::marker::PhantomData;
use core::ops::Deref;
use x86_64::PhysAddr;

use super::refcache;

#[derive(Debug)]
pub(crate) struct SharedRefCacheFrame<T = [u8; 4096]> {
    phys: u64,
    _phantom: PhantomData<T>,
}

impl<T> SharedRefCacheFrame<T> {
    pub(crate) fn from_phys(phys: u64) -> Self {
        refcache::init_frame(phys);
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
}

impl<T> Clone for SharedRefCacheFrame<T> {
    fn clone(&self) -> Self {
        refcache::inc(self.phys);
        Self {
            phys: self.phys,
            _phantom: PhantomData,
        }
    }
}

impl<T> Drop for SharedRefCacheFrame<T> {
    fn drop(&mut self) {
        refcache::dec(self.phys);
    }
}

impl<T> Deref for SharedRefCacheFrame<T> {
    type Target = T;
    fn deref(&self) -> &T {
        let hhdm = crate::memory::active_physical_offset();
        let ptr = (hhdm + self.phys) as usize as *const T;
        unsafe { &*ptr }
    }
}
