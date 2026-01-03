//! # echOS Linked List Allocator
//! 
//! Bağlı liste tabanlı dinamik bellek ayırıcı.
//! Serbest bırakılan bellek bloklarını bir listede tutar ve tekrar kullanılmasını sağlar.
//! Bump allocator'dan daha esnektir ancak fragmantasyona açıktır.

use core::alloc::{GlobalAlloc, Layout};
use core::{mem, ptr};

/// Serbest bellek bloğunu temsil eden düğüm.
struct ListNode {
    size: usize,
    next: Option<&'static mut ListNode>,
}

impl ListNode {
    const fn new(size: usize) -> Self {
        ListNode { size, next: None }
    }

    fn start_addr(&self) -> usize {
        self as *const Self as usize
    }

    fn end_addr(&self) -> usize {
        self.start_addr() + self.size
    }
}

pub struct LinkedListAllocator {
    head: ListNode,
}

impl LinkedListAllocator {
    /// Yeni boş allocator oluşturur.
    pub const fn new() -> Self {
        Self {
            head: ListNode::new(0),
        }
    }

    /// Allocator'ı başlatır.
    pub unsafe fn init(&mut self, heap_start: usize, heap_size: usize) {
        unsafe {
            self.add_free_region(heap_start, heap_size);
        }
    }

    /// Serbest bir bölgeyi listeye ekler.
    unsafe fn add_free_region(&mut self, addr: usize, size: usize) {
        // Hizalama ve boyut kontrolü
        assert_eq!(align_up(addr, mem::align_of::<ListNode>()), addr);
        assert!(size >= mem::size_of::<ListNode>());

        // Yeni düğüm oluştur ve listeye ekle
        let mut node = ListNode::new(size);
        node.next = self.head.next.take();
        let node_ptr = addr as *mut ListNode;
        unsafe {
            node_ptr.write(node);
            self.head.next = Some(&mut *node_ptr)
        }
    }

    /// İstenen boyut ve hizalamaya uygun boş bir bölge arar.
    fn find_region(&mut self, size: usize, align: usize) -> Option<(&'static mut ListNode, usize)> {
        let mut current = &mut self.head;

        while let Some(ref mut region) = current.next {
            if let Ok(alloc_start) = Self::alloc_from_region(&region, size, align) {
                let next = region.next.take();
                let ret = Some((current.next.take().unwrap(), alloc_start));
                current.next = next;
                return ret;
            } else {
                current = current.next.as_mut().unwrap();
            }
        }

        None
    }

    /// Verilen bölgeden allocation yapmaya çalışır.
    fn alloc_from_region(region: &ListNode, size: usize, align: usize) -> Result<usize, ()> {
        let alloc_start = align_up(region.start_addr(), align);
        let alloc_end = alloc_start.checked_add(size).ok_or(())?;

        if alloc_end > region.end_addr() {
            return Err(());
        }

        let excess_size = region.end_addr() - alloc_end;
        if excess_size > 0 && excess_size < mem::size_of::<ListNode>() {
            // Kalan parça ListNode tutamayacak kadar küçükse bu bloğu kullanma (fragmantasyon önleme)
            return Err(());
        }

        Ok(alloc_start)
    }

    /// Layout için gerekli boyut ve hizalamayı hesaplar.
    fn size_align(layout: Layout) -> (usize, usize) {
        let layout = layout
            .align_to(mem::align_of::<ListNode>())
            .expect("Hizalama ayarlanamadı")
            .pad_to_align();
        let size = layout.size().max(mem::size_of::<ListNode>());
        (size, layout.align())
    }
}

unsafe impl GlobalAlloc for LinkedListAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let (size, align) = LinkedListAllocator::size_align(layout);
        let self_ptr = self as *const Self as *mut Self;

        if let Some((region, alloc_start)) = (*self_ptr).find_region(size, align) {
            let alloc_end = alloc_start.checked_add(size).expect("overflow");
            let excess_size = region.end_addr() - alloc_end;
            if excess_size > 0 {
                (*self_ptr).add_free_region(alloc_end, excess_size);
            }
            alloc_start as *mut u8
        } else {
            ptr::null_mut()
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let (size, _) = LinkedListAllocator::size_align(layout);
        let self_ptr = self as *const Self as *mut Self;
        (*self_ptr).add_free_region(ptr as usize, size)
    }
}

fn align_up(addr: usize, align: usize) -> usize {
    (addr + align - 1) & !(align - 1)
}
