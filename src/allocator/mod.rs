//! # echOS Heap Allocator
//!
//! Kernel heap bellek yönetimi.
//! TLSF (Two-Level Segregated Fit) algoritması kullanır.
//! O(1) zaman karmaşıklığı ile allocation/deallocation.
//!
//! ## Allocator Mimarisi: Erken Heap + Ana Heap
//!
//! echOS iki aşamalı bir heap stratejisi kullanır:
//!
//! ```
//!  Sistem Başlangıcı
//!        |
//!        v
//!  [ERKEN HEAP (Early Heap)]
//!   - Static BSS bölgesinde 512 KiB
//!   - Sayfa tablosu kurulmadan önce devreye girer
//!   - Bump benzeri doğrusal allocasyon (lock-free CAS)
//!   - Hiçbir zaman serbest bırakılmaz
//!        |
//!        v  (init_heap() çağrısı)
//!  [ANA HEAP (Main Heap)]
//!   - Sanal adreste 100 MiB (0x4444_4444_0000)
//!   - TLSF algoritması: O(1) alloc/dealloc
//!   - Ana heap hazır olunca tüm yeni alloc'lar buraya yönlenir
//! ```
//!
//! ## Sanal Bellek Haritası:
//!
//! ```
//! 0x0000_0000_0000  --> Kernel kodu
//! ...
//! 0x4444_4444_0000  --> Heap başlangıcı (HEAP_START)
//! 0x4444_4444_0000
//! + 100 MiB         --> Heap sonu (HEAP_START + HEAP_SIZE)
//! ```

pub mod tlsf;
pub mod stack;
use tlsf::LockedTlsf;

use crate::memory::paging;
use core::alloc::Layout;
use core::ptr;
use core::sync::atomic::{AtomicBool, Ordering};
use x86_64::{
    structures::paging::{
        mapper::MapToError, FrameAllocator, Mapper, Page, PageTableFlags, Size4KiB,
    },
    VirtAddr,
};

/// Heap başlangıç adresi (sanal bellek)
///
/// 0x4444_4444_0000 değeri kasıtlı olarak seçilmiştir: kernel kodu, stack'ler
/// ve diğer bellek bölgelerinden uzakta, belirgin bir adreste konumlandırılır.
/// Bu sayede debugging sırasında heap pointer'ları kolayca tanınabilir.
pub const HEAP_START: usize = 0x_4444_4444_0000;

/// Heap boyutu (100 MiB)
///
/// 100 MiB çoğu kernel operasyonu için yeterlidir. İleride dinamik
/// büyüme (heap expansion) mekanizmasıyla genişletilebilir.
pub const HEAP_SIZE: usize = 100 * 1024 * 1024;

/// Global TLSF allocator
///
/// `#[global_allocator]` niteliği sayesinde Rust'ın `alloc` crate'i
/// (Box, Vec, String vb.) bu allocator'ı otomatik olarak kullanır.
#[global_allocator]
pub static ALLOCATOR: LockedTlsf = LockedTlsf::new();

/// Heap bütünlüğünü kontrol eder (genel sarmalayıcı).
///
/// TLSF katmanındaki bütünlük kontrolünü dışarıya açar.
/// Döndürülen değer: tespit edilen bozulma (corruption) sayısı.
pub fn check_heap_integrity() -> usize {
    LockedTlsf::check_heap_integrity()
}

/// Bellek istatistiklerini döndürür (genel sarmalayıcı).
///
/// Aktif allocasyon sayısı, toplam ayrılan bayt, tepe kullanım ve
/// bozulma sayısı gibi bilgileri içeren `AllocStats` yapısını döndürür.
pub fn get_alloc_stats() -> tlsf::AllocStats {
    LockedTlsf::get_stats()
}

/// Heap'in başlatılıp başlatılmadığını takip eden atomik bayrak.
///
/// AtomicBool kullanılır çünkü çoklu çekirdekli (SMP) ortamda farklı
/// CPU'lar aynı anda init_heap() çağırabilir; bu bayraksız ikinci başlatma
/// sayfa tablosunu bozabilir.
static HEAP_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Heap bellek alanını başlatır.
///
/// Sayfa tablosunda gerekli mapping'leri yapar ve
/// TLSF allocator'a serbest bölgeyi ekler.
///
/// ## Başlatma Akışı:
/// ```
/// init_heap() çağrısı
///      |
///      v
/// [HEAP_INITIALIZED kontrol] --> zaten başlatıldı --> Ok(()) dön
///      |
///     hayır
///      v
/// [Heap sayfa aralığını hesapla]
///      |
///      v
/// [Her sayfa için fiziksel frame tahsis et + map et]
///      |
///      v
/// [TLSF allocator'a serbest bölge ekle]
///      |
///      v
/// [HEAP_INITIALIZED = true]
///      |
///      v
/// Ok(())
/// ```
///
/// # Parametreler
/// - `mapper`: Sayfa tablosu mapper'ı
/// - `frame_allocator`: Fiziksel frame allocator
pub fn init_heap(
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) -> Result<(), MapToError<Size4KiB>> {
    // Daha önce başlatıldıysa tekrar başlatma (idempotent koruma)
    if HEAP_INITIALIZED.load(Ordering::Acquire) {
        return Ok(());
    }

    // Heap sayfa aralığını hesapla
    let page_range = {
        let heap_start = VirtAddr::new(HEAP_START as u64);
        let heap_end = heap_start + HEAP_SIZE as u64 - 1u64;
        let heap_start_page = Page::containing_address(heap_start);
        let heap_end_page = Page::containing_address(heap_end);
        Page::range_inclusive(heap_start_page, heap_end_page)
    };

    // Her sayfa için fiziksel frame map et
    // WP (Write Protect) biti geçici olarak devre dışı bırakılır; bazı
    // sayfa tablosu yapıları read-only olarak işaretlenmiş olabilir.
    let map_result: Result<(), MapToError<Size4KiB>> = paging::with_wp_disabled(|| {
        let mut mapped_pages: usize = 0;
        for page in page_range {
            if mapped_pages == 0 {
                crate::serial_println!("[HEAP] mapping start");
                crate::serial_println!("[HEAP] allocating first frame");
            }
            let frame = frame_allocator
                .allocate_frame()
                .ok_or(MapToError::FrameAllocationFailed)?;
            if mapped_pages == 0 {
                crate::serial_println!("[HEAP] allocated first frame");
            }
            let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
            let _ = unsafe { mapper.map_to(page, frame, flags, frame_allocator) }?;
            mapped_pages = mapped_pages.saturating_add(1);
            if mapped_pages == 1 {
                crate::serial_println!("[HEAP] mapped first page");
            }
            // Her 4096 sayfada bir ilerleme mesajı yaz (100 MiB / 4 KiB = ~25600 sayfa)
            if (mapped_pages & 0xFFF) == 0 {
                crate::serial_println!("[HEAP] mapped pages: {}", mapped_pages);
            }
        }
        Ok(())
    });
    map_result?;

    // TLSF allocator'a serbest bölgeyi ekle: artık heap kullanıma hazır
    unsafe {
        ALLOCATOR.insert_free_region_ptr(HEAP_START as *mut u8, HEAP_SIZE);
    }

    // Başlatıldı olarak işaretle (Release: önceki tüm yazma işlemleri görünür olur)
    HEAP_INITIALIZED.store(true, Ordering::Release);

    Ok(())
}

/// Ham bellek ayırır: boyutu başlık ile birlikte saklar.
///
/// Bu fonksiyon, boyutu pointer'ın hemen önüne (başlık olarak) yazar.
/// Bu, `heap_free` sırasında Layout'u yeniden oluşturabilmek için gereklidir.
///
/// ## Bellek Düzeni:
/// ```
/// +----------+------------------------------+
/// | size hdr |   kullanıcıya döndürülen alan |
/// +----------+------------------------------+
/// ^-- raw     ^-- döndürülen ptr
/// ```
pub unsafe fn heap_alloc(size: usize) -> *mut u8 {
    if size == 0 {
        return ptr::null_mut();
    }
    let header = core::mem::size_of::<usize>();
    let total = size.saturating_add(header);
    let layout = Layout::from_size_align(total, core::mem::align_of::<usize>()).unwrap();
    let raw = alloc::alloc::alloc(layout);
    if raw.is_null() {
        return ptr::null_mut();
    }
    (raw as *mut usize).write(size);
    raw.add(header)
}

/// Ham bellek serbest bırakır: başlıktan boyutu okuyarak Layout'u yeniden oluşturur.
///
/// `heap_alloc` ile ayrılmış pointer'ları serbest bırakmak için kullanılır.
/// Başlık yoksa veya pointer geçersizse tanımsız davranış oluşur — bu yüzden
/// yalnızca `heap_alloc` tarafından döndürülen pointer'larla çağrılmalıdır.
pub unsafe fn heap_free(ptr: *mut u8) {
    if ptr.is_null() {
        return;
    }
    let header = core::mem::size_of::<usize>();
    let raw = ptr.sub(header);
    let size = (raw as *mut usize).read();
    let total = size.saturating_add(header);
    let layout = Layout::from_size_align(total, core::mem::align_of::<usize>()).unwrap();
    alloc::alloc::dealloc(raw, layout);
}
