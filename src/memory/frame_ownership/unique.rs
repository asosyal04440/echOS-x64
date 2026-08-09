use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};
use x86_64::structures::paging::PhysFrame;
use x86_64::PhysAddr;

use super::shared_atomic::SharedAtomicFrame;
use super::shared_refcache::SharedRefCacheFrame;
use super::FRAME_TABLE;

#[derive(Debug)]
pub(crate) struct UniqueFrame<T = [u8; 4096]> {
    phys: u64,
    _phantom: PhantomData<T>,
}

impl<T> UniqueFrame<T> {
    pub(crate) fn from_phys(phys: u64) -> Self {
        FRAME_TABLE.lock().init_unique(phys);
        Self {
            phys: phys & !(0xFFF),
            _phantom: PhantomData,
        }
    }

    pub(crate) fn from_phys_alloc(pframe: Option<PhysFrame>) -> Option<Self> {
        pframe.map(|f| {
            let phys = f.start_address().as_u64();
            FRAME_TABLE.lock().init_unique(phys);
            Self {
                phys: phys & !(0xFFF),
                _phantom: PhantomData,
            }
        })
    }

    pub(crate) fn phys(&self) -> u64 {
        self.phys
    }

    pub(crate) fn start_address(&self) -> PhysAddr {
        PhysAddr::new(self.phys)
    }

    pub(crate) fn as_phys_frame(&self) -> PhysFrame {
        PhysFrame::containing_address(PhysAddr::new(self.phys))
    }

    pub(crate) fn into_phys_frame(self) -> PhysFrame {
        let phys = self.phys;
        core::mem::forget(self);
        PhysFrame::containing_address(PhysAddr::new(phys))
    }

    pub(crate) fn into_shared(self) -> SharedAtomicFrame<T> {
        let phys = self.phys;
        FRAME_TABLE.lock().share(phys);
        core::mem::forget(self);
        SharedAtomicFrame::from_phys_inner(phys)
    }

    /// Per-CPU delta refcache'e geçir: FRAME_TABLE'da REFCACHE flag'i ayarlanır
    /// (refcount=0) ve sayfa tablosundaki ilk referans için `inc` yapılır (+1 delta).
    /// UniqueFrame sızdırılır. Dönen `SharedRefCacheFrame` önbellekte saklanabilir;
    /// clone() refcache::inc, drop() refcache::dec yapar.
    /// tick() periyodik flush + epoch-based review ile frame'i serbest bırakır.
    pub(crate) fn into_shared_refcache(self) -> SharedRefCacheFrame<T> {
        let phys = self.phys;
        FRAME_TABLE.lock().refcache_init(phys);
        super::refcache_inc(phys);
        core::mem::forget(self);
        SharedRefCacheFrame::from_phys(phys)
    }

    /// Tüketiciye ait referansı global tabloya işler (SHARED, ref=1) ve UniqueFrame'i
    /// sızdırır — böylece Drop ne ZOMBIE işareti koyar ne de belleği serbest bırakır.
    /// Çağıran, fiziksel sayfanın artık sayfa tablosu girişine ait olduğunu garanti eder;
    /// ileride SharedAtomicFrame::decref / SharedAtomicFrame::from_phys_inner ile temizlenmelidir.
    pub(crate) fn leak_as_shared(self) {
        FRAME_TABLE.lock().share(self.phys);
        core::mem::forget(self);
    }
}

impl<T> Drop for UniqueFrame<T> {
    fn drop(&mut self) {
        FRAME_TABLE.lock().mark_zombie(self.phys);
        unsafe {
            let frame = PhysFrame::containing_address(PhysAddr::new(self.phys));
            crate::memory::deallocate_contiguous_frames(frame, 1);
        }
    }
}

impl<T> Deref for UniqueFrame<T> {
    type Target = T;
    fn deref(&self) -> &T {
        let hhdm = crate::memory::active_physical_offset();
        let ptr = (hhdm + self.phys) as usize as *const T;
        unsafe { &*ptr }
    }
}

impl<T> DerefMut for UniqueFrame<T> {
    fn deref_mut(&mut self) -> &mut T {
        let hhdm = crate::memory::active_physical_offset();
        let ptr = (hhdm + self.phys) as usize as *mut T;
        unsafe { &mut *ptr }
    }
}
