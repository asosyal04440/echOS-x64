//! # Per-CPU Slab Cache
//!
//! Küçük allocasyonlar (<= 512 byte) için sayfa tabanlı slab önbelleği.
//! Her slab backing sayfası 4 KiB hizalıdır ve global page-owner tablosunda
//! `PageOwner::Slab` olarak işaretlenir. Böylece `dealloc(ptr)` çağrısı
//! pointer'ı maskeleyip sayfa sahibine göre O(1) yönlendirme yapabilir.

use super::{slab_class_for_ptr, PageOwner, HEAP_PAGE_SIZE};
use core::alloc::Layout;
use core::mem::size_of;
use core::ptr;
use core::sync::atomic::{AtomicBool, Ordering};

const MAX_CPUS: usize = 64;
const NUM_CLASSES: usize = 5;
const CLASS_SIZES: [usize; NUM_CLASSES] = [32, 64, 128, 256, 512];

static SLAB_ACTIVE: AtomicBool = AtomicBool::new(false);

struct SlabClass {
    head: usize,
    pages: usize,
}

impl SlabClass {
    const fn new() -> Self {
        Self { head: 0, pages: 0 }
    }

    #[inline]
    unsafe fn push(&mut self, ptr: *mut u8) {
        debug_assert!((ptr as usize) != 0);
        *(ptr as *mut usize) = self.head;
        self.head = ptr as usize;
    }

    #[inline]
    unsafe fn pop(&mut self) -> Option<*mut u8> {
        if self.head == 0 {
            return None;
        }
        let ptr = self.head as *mut u8;
        self.head = *(ptr as *mut usize);
        Some(ptr)
    }
}

struct PerCpuSlab {
    classes: [SlabClass; NUM_CLASSES],
}

impl PerCpuSlab {
    const fn new() -> Self {
        Self {
            classes: [const { SlabClass::new() }; NUM_CLASSES],
        }
    }
}

static mut CPU_SLABS: [PerCpuSlab; MAX_CPUS] = [const { PerCpuSlab::new() }; MAX_CPUS];

#[inline]
fn size_to_class(size: usize) -> Option<usize> {
    match size {
        0..=32 => Some(0),
        33..=64 => Some(1),
        65..=128 => Some(2),
        129..=256 => Some(3),
        257..=512 => Some(4),
        _ => None,
    }
}

#[inline]
fn current_cpu_slot() -> usize {
    let cpu = crate::cpu::smp::current_cpu_id() as usize;
    cpu.min(MAX_CPUS - 1)
}

unsafe fn provision_slab_page(class: usize, slab: &mut SlabClass) -> bool {
    let class_size = CLASS_SIZES[class];
    let layout = match Layout::from_size_align(HEAP_PAGE_SIZE, HEAP_PAGE_SIZE) {
        Ok(layout) => layout,
        Err(_) => return false,
    };
    let page = super::ALLOCATOR.alloc_from_main_heap(layout);
    if page.is_null() {
        return false;
    }

    super::tag_heap_range(page, HEAP_PAGE_SIZE, PageOwner::Slab, Some(class));
    slab.pages = slab.pages.saturating_add(1);

    let slot_count = HEAP_PAGE_SIZE / class_size;
    if slot_count == 0 || class_size < size_of::<usize>() {
        return false;
    }

    for slot_index in (0..slot_count).rev() {
        let slot_ptr = page.add(slot_index * class_size);
        ptr::write_bytes(slot_ptr, 0, class_size);
        slab.push(slot_ptr);
    }

    true
}

#[inline]
pub unsafe fn slab_alloc(size: usize, align: usize) -> Option<*mut u8> {
    if !SLAB_ACTIVE.load(Ordering::Acquire) {
        return None;
    }
    if align > 16 {
        return None;
    }

    let size = (size + 7) & !7;
    let class = size_to_class(size)?;

    x86_64::instructions::interrupts::without_interrupts(|| {
        let cpu = current_cpu_slot();
        let slab = &mut CPU_SLABS[cpu].classes[class];
        if slab.head == 0 && !provision_slab_page(class, slab) {
            return None;
        }
        slab.pop()
    })
}

#[inline]
pub unsafe fn slab_dealloc(ptr: *mut u8) -> bool {
    if !SLAB_ACTIVE.load(Ordering::Acquire) || ptr.is_null() {
        return false;
    }

    let class = match slab_class_for_ptr(ptr as usize) {
        Some(class) if class < NUM_CLASSES => class,
        _ => return false,
    };

    x86_64::instructions::interrupts::without_interrupts(|| {
        let cpu = current_cpu_slot();
        let slab = &mut CPU_SLABS[cpu].classes[class];
        slab.push(ptr);
        true
    })
}

pub fn activate() {
    SLAB_ACTIVE.store(true, Ordering::Release);
    crate::serial_println!(
        "[SLAB] Per-CPU page slabs activated ({} classes, page owner routing enabled)",
        NUM_CLASSES
    );
}
