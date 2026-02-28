//! # echOS Linked List Allocator
//!
//! Bağlı liste tabanlı dinamik bellek ayırıcı.
//! Serbest bırakılan bellek bloklarını bir listede tutar ve tekrar kullanılmasını sağlar.
//! Bump allocator'dan daha esnektir ancak harici fragmantasyona (external fragmentation) açıktır.
//!
//! ## Bağlı Liste Allocator Nedir?
//!
//! Heap'teki serbest (boş) bellek blokları, birbirini işaret eden bir bağlı liste
//! olarak organize edilir. Her serbest bloğun başına bir `ListNode` başlığı yazılır;
//! bu başlık bloğun boyutunu ve bir sonraki serbest bloğun adresini içerir.
//!
//! ## Serbest Liste Bellek Düzeni:
//!
//! ```
//!  head (dummy)
//!    |
//!    v
//!  +----------+     +----------+     +----------+
//!  | size:  0 | --> | size: 64 | --> | size: 32 | --> None
//!  | next: *  |     | next: *  |     | next: -- |
//!  +----------+     +----------+     +----------+
//!      (sentinel)   (serbest blok1)  (serbest blok2)
//! ```
//!
//! ## Allocation Sırasında:
//! ```
//! find_region(size, align) çağrısı
//!        |
//!        v
//!  head -> blok1 -> blok2 -> ... -> None
//!        |           |
//!        |           v
//!        |       [uygun mu?] EVET --> listeden çıkar, geri kalan parçayı ekle
//!        |           |
//!        |          HAYIR --> sonraki bloğa geç
//!        v
//!     None --> null_mut() döndür (bellek yetersiz)
//! ```
//!
//! ## Deallocation Sırasında:
//! ```
//! dealloc(ptr, size) çağrısı
//!        |
//!        v
//!  ptr adresine ListNode başlığı yaz
//!        |
//!        v
//!  head'in önüne yeni serbest bloğu ekle (O(1) liste başına ekleme)
//! ```
//!
//! ## Zaman Karmaşıklığı:
//! - Allocation : O(n) — uygun bloğu bulmak için liste taranır (n = serbest blok sayısı)
//! - Deallocation: O(1) — blok listenin başına eklenir
//!
//! ## Fragmantasyon Sorunu:
//! Zamanla çok sayıda küçük serbest blok oluşur. Komşu boş blokların
//! birleştirilmesi (coalescing) bu implementasyonda yapılmamıştır.
//! TLSF allocator bu sorunu daha iyi yönetir.
//!
//! ## Neden Bu Yaklaşım?
//! Bump allocator'ın aksine gerçek deallocation destekler. Kernel içinde
//! farklı boyutlarda sık sık allocasyon/deallocation yapılan senaryolar için
//! uygundur. TLSF kadar hızlı değildir ama anlaması ve doğrulaması kolaydır.

use core::alloc::{GlobalAlloc, Layout};
use core::{mem, ptr};

/// Serbest bellek bloğunu temsil eden bağlı liste düğümü.
///
/// Her serbest bellek bloğunun başına bu yapı yazılır.
/// Yapı, bloğun boyutunu ve bir sonraki serbest bloğun referansını tutar.
///
/// ## Bellek İçi Görünüm (bir serbest blok):
/// ```
/// adres X:
/// +--------+----------+-------------------------+
/// | size   | next ptr |  kullanılabilir ham alan |
/// +--------+----------+-------------------------+
/// ^-- ListNode başlığı (bu struct)
/// ```
struct ListNode {
    size: usize,
    next: Option<&'static mut ListNode>,
}

impl ListNode {
    /// Belirtilen boyutta, sonraki düğümü olmayan yeni bir ListNode oluşturur.
    const fn new(size: usize) -> Self {
        ListNode { size, next: None }
    }

    /// Bu düğümün (yani bu serbest bloğun) başlangıç adresini döndürür.
    /// Düğümün kendisi bellekte durduğu için kendi adresi = blok başlangıcı.
    fn start_addr(&self) -> usize {
        self as *const Self as usize
    }

    /// Bu düğümün temsil ettiği serbest bloğun bitiş adresini döndürür.
    /// start_addr + size = blok sonu
    fn end_addr(&self) -> usize {
        self.start_addr() + self.size
    }
}

/// Bağlı liste tabanlı heap allocator.
///
/// `head` alanı, gerçek bir serbest blok değil; listenin başını işaret etmek
/// için kullanılan dummy (kukla) bir düğümdür. Bu teknik, "sentinel node"
/// deseni olarak bilinir ve liste başını özel bir durum olarak ele almayı önler.
pub struct LinkedListAllocator {
    head: ListNode,
}

impl LinkedListAllocator {
    /// Yeni boş allocator oluşturur.
    ///
    /// `head` dummy düğümü boyut=0 ile başlatılır. `init` çağrılana kadar
    /// bu allocator kullanılamaz.
    pub const fn new() -> Self {
        Self {
            head: ListNode::new(0),
        }
    }

    /// Allocator'ı başlatır: verilen bellek bölgesini serbest liste olarak kaydeder.
    ///
    /// İlk çağrıda tüm heap tek büyük serbest blok olarak listeye eklenir.
    pub unsafe fn init(&mut self, heap_start: usize, heap_size: usize) {
        unsafe {
            self.add_free_region(heap_start, heap_size);
        }
    }

    /// Serbest bellek bölgesini bağlı listeye başa ekler.
    ///
    /// ## Gereksinimler:
    /// - `addr` en az `ListNode`'un hizalamasına uygun olmalıdır.
    /// - `size` en az `ListNode`'un boyutu kadar olmalıdır.
    ///
    /// ## Ekleme Mekanizması (O(1) — her zaman başa eklenir):
    /// ```
    ///  Önce: head -> eski_ilk -> ...
    ///  Sonra: head -> yeni_blok -> eski_ilk -> ...
    /// ```
    unsafe fn add_free_region(&mut self, addr: usize, size: usize) {
        // Hizalama ve minimum boyut kontrolü — ListNode bu alana sığmalı
        assert_eq!(align_up(addr, mem::align_of::<ListNode>()), addr);
        assert!(size >= mem::size_of::<ListNode>());

        // Yeni düğüm oluştur ve listeye başa ekle
        let mut node = ListNode::new(size);
        // Yeni düğümün "next"'i eski head.next olur
        node.next = self.head.next.take();
        // Düğüm yapısını verilen adrese yaz
        let node_ptr = addr as *mut ListNode;
        unsafe {
            node_ptr.write(node);
            // head artık yeni düğümü gösterir
            self.head.next = Some(&mut *node_ptr)
        }
    }

    /// İstenen boyut ve hizalamaya uygun ilk serbest bloğu arar (First-Fit stratejisi).
    ///
    /// Bulunan blok listeden çıkarılır ve döndürülür.
    ///
    /// ## First-Fit Stratejisi:
    /// Listeyi baştan sona tarar, koşulu sağlayan ilk bloğu kullanır.
    /// En iyi seçim (best-fit) stratejisine göre daha hızlıdır ama daha fazla
    /// fragmantasyona yol açabilir.
    ///
    /// Zaman karmaşıklığı: O(n) — n serbest blok sayısı
    fn find_region(&mut self, size: usize, align: usize) -> Option<(&'static mut ListNode, usize)> {
        let mut current = &mut self.head;

        // Bağlı listeyi baştan sona tara
        while let Some(ref mut region) = current.next {
            if let Ok(alloc_start) = Self::alloc_from_region(&region, size, align) {
                // Uygun bölge bulundu: listeden çıkar
                let next = region.next.take();
                let ret = Some((current.next.take().unwrap(), alloc_start));
                current.next = next;
                return ret;
            } else {
                // Bu blok uygun değil, bir sonrakine geç
                current = current.next.as_mut().unwrap();
            }
        }

        // Hiçbir uygun blok bulunamadı
        None
    }

    /// Verilen serbest bölgenin istenen boyut ve hizalamaya uyup uymadığını kontrol eder.
    ///
    /// ## Kontrol Adımları:
    /// 1. Bölgenin başlangıcını hizalama gereksinimlerine göre yukarı yuvarla.
    /// 2. Bölgenin sonunun yeterince büyük olup olmadığını kontrol et.
    /// 3. Artık yer bir ListNode tutmaya yetmeyecek kadar küçükse bu bloğu reddet.
    ///
    /// ## Neden 3. Kontrol Gerekli?
    /// Allocation sonrasında kalan küçük parça listeye serbest blok olarak
    /// eklenecektir. Bu parça `ListNode` sığmayacak kadar küçükse onu kaydedemeyiz,
    /// dolayısıyla bu blok kullanılamaz (iç fragmantasyon yerine reddetme tercih edilir).
    fn alloc_from_region(region: &ListNode, size: usize, align: usize) -> Result<usize, ()> {
        let alloc_start = align_up(region.start_addr(), align);
        let alloc_end = alloc_start.checked_add(size).ok_or(())?;

        if alloc_end > region.end_addr() {
            // Blok bu boyuta yetmiyor
            return Err(());
        }

        let excess_size = region.end_addr() - alloc_end;
        if excess_size > 0 && excess_size < mem::size_of::<ListNode>() {
            // Kalan parça ListNode tutamayacak kadar küçükse bu bloğu kullanma
            // (fragmantasyon önleme: kaybedilecek küçük artık parça yerine bloğu reddet)
            return Err(());
        }

        Ok(alloc_start)
    }

    /// Layout için gerekli boyut ve hizalamayı, ListNode gereksinimi ile uyumlu şekilde hesaplar.
    ///
    /// Serbest bırakma sırasında blok başına bir ListNode yazılacağından,
    /// en küçük allocation birimi `size_of::<ListNode>()` kadardır.
    /// Aynı şekilde hizalama da ListNode'un hizalamasından düşük olamaz.
    fn size_align(layout: Layout) -> (usize, usize) {
        let layout = layout
            .align_to(mem::align_of::<ListNode>())
            .expect("Hizalama ayarlanamadı")
            .pad_to_align();
        // Minimum boyut: ListNode boyutu (dealloc sırasında düğüm yazılabilmesi için)
        let size = layout.size().max(mem::size_of::<ListNode>());
        (size, layout.align())
    }
}

unsafe impl GlobalAlloc for LinkedListAllocator {
    /// Bellek ayırır: uygun serbest bloğu bulur, gerekirse artan kısmı geri ekler.
    ///
    /// ## Adımlar:
    /// 1. Layout'u ListNode uyumlu boyuta normalize et.
    /// 2. Serbest listede uygun blok ara.
    /// 3. Blok bulunduysa fazlalığı serbest listeye geri ekle.
    /// 4. Alloc başlangıç adresini döndür.
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let (size, align) = LinkedListAllocator::size_align(layout);
        // GlobalAlloc trait'i &self alır; iç mutability için raw pointer cast kullanılır
        let self_ptr = self as *const Self as *mut Self;

        if let Some((region, alloc_start)) = (*self_ptr).find_region(size, align) {
            let alloc_end = alloc_start.checked_add(size).expect("overflow");
            let excess_size = region.end_addr() - alloc_end;
            // Fazlalık varsa onu serbest listeye geri ekle (zaten ListNode sığacak büyüklükte)
            if excess_size > 0 {
                (*self_ptr).add_free_region(alloc_end, excess_size);
            }
            alloc_start as *mut u8
        } else {
            // Uygun blok bulunamadı: heap tükendi veya fragmantasyon çok fazla
            ptr::null_mut()
        }
    }

    /// Belleği serbest bırakır: bloğu serbest liste başına ekler.
    ///
    /// Zaman karmaşıklığı: O(1) — her zaman liste başına eklenir.
    /// Not: Komşu serbest blokların birleştirilmesi (coalescing) yapılmaz.
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let (size, _) = LinkedListAllocator::size_align(layout);
        let self_ptr = self as *const Self as *mut Self;
        // ptr adresine ListNode yaz ve serbest listeye ekle
        (*self_ptr).add_free_region(ptr as usize, size)
    }
}

/// Adresi verilen hizalamaya (align) göre yukarı yuvarlar.
///
/// `align` değeri 2'nin kuvveti olmalıdır.
/// Algoritma: `(addr + align - 1) & !(align - 1)`
/// Zaman karmaşıklığı: O(1)
fn align_up(addr: usize, align: usize) -> usize {
    (addr + align - 1) & !(align - 1)
}
