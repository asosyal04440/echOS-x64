//! # echOS Bump Allocator
//!
//! Basit ve hızlı bir doğrusal (linear) bellek ayırıcı.
//! Bellek iadesi (deallocation) desteklemez, sadece ileriye doğru büyür.
//! Küçük ve kısa ömürlü kernel projeleri veya boot aşaması için uygundur.
//!
//! ## Bump (Tampon) Allocator Nedir?
//!
//! Bump allocator, bir "sonraki boş adres" işaretçisi tutarak çalışır.
//! Her yeni allocation bu işaretçiyi (next) ilerletir ("bump" eder).
//! Bireysel blokları serbest bırakmak mümkün değildir; yalnızca TÜM
//! allocasyonlar tamamlandığında işaretçi başa sarılabilir.
//!
//! ## Bellek Düzeni (Heap Görünümü):
//!
//! ```
//! heap_start                    next        heap_end
//!     |                          |               |
//!     v                          v               v
//!     +----------+-------+-------+~~~~~~~~~~~~~~~+
//!     | blok [0] | bl[1] | bl[2] |  (boş alan)  |
//!     +----------+-------+-------+~~~~~~~~~~~~~~~+
//!                                 ^-- buraya kadar kullanıldı
//! ```
//!
//! ## Zaman Karmaşıklığı:
//! - Allocation : O(1) — sadece işaretçi ilerletme
//! - Deallocation: O(1) — sayaç azaltma (gerçek serbest bırakma yok)
//!
//! ## Neden Bu Yaklaşım?
//! Boot aşamasında heap henüz hazır değilken bu allocator devreye girer.
//! Bağlı liste veya TLSF gibi karmaşık yapılar kurulmadan önce kernel,
//! bump allocator sayesinde geçici bellek kullanabilir.

use core::alloc::{GlobalAlloc, Layout};
use core::ptr;

/// Bump Allocator yapısı.
///
/// Doğrusal bellek ayırma için gereken dört alanı içerir:
/// - `heap_start` : Heap'in başlangıç adresi (sabit)
/// - `heap_end`   : Heap'in bitiş adresi (sabit)
/// - `next`       : Bir sonraki boş bellek adresi (her alloc'ta ilerler)
/// - `allocations`: Şu an aktif allocasyon sayısı (dealloc için takip)
pub struct BumpAllocator {
    heap_start: usize,
    heap_end: usize,
    next: usize,
    allocations: usize,
}

impl BumpAllocator {
    /// Yeni boş bir Bump Allocator oluşturur.
    ///
    /// Tüm alanlar sıfırlanmış olarak başlar; `init` çağrılana kadar
    /// bu allocator kullanılamaz.
    pub const fn new() -> Self {
        BumpAllocator {
            heap_start: 0,
            heap_end: 0,
            next: 0,
            allocations: 0,
        }
    }

    /// Allocator'ı verilen heap aralığı ile başlatır.
    ///
    /// `heap_start` adresinden başlayarak `heap_size` byte büyüklüğünde
    /// bir bellek bölgesi kullanıma hazır hale getirilir.
    ///
    /// # Güvenlik
    /// Çağıran kişi, verilen bellek aralığının başka bir yapı tarafından
    /// kullanılmadığından emin olmalıdır. Çakışan aralıklar bellek bozulmasına
    /// (memory corruption) yol açar.
    pub unsafe fn init(&mut self, heap_start: usize, heap_size: usize) {
        self.heap_start = heap_start;
        self.heap_end = heap_start + heap_size;
        self.next = heap_start;
    }
}

unsafe impl GlobalAlloc for BumpAllocator {
    /// Bellek ayırma işlemi.
    ///
    /// ## Algoritma Akışı:
    /// ```
    ///  alloc(layout) çağrısı
    ///       |
    ///       v
    ///  [align_up ile hizala] --> alloc_start
    ///       |
    ///       v
    ///  alloc_start + size  --> alloc_end
    ///       |
    ///       v
    ///  alloc_end > heap_end? --> EVET --> null_mut() döndür (yetersiz bellek)
    ///       |
    ///      HAYIR
    ///       |
    ///       v
    ///  next = alloc_end, allocations += 1
    ///       |
    ///       v
    ///  alloc_start döndür  (başarılı)
    /// ```
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // Mevcut 'next' adresini istenen hizalamaya (alignment) göre yukarı
        // yuvarla. Örneğin 4-byte hizalamalı bir veri 0x03 adresine konamaz,
        // 0x04'e hizalanması gerekir.
        let alloc_start = align_up(self.next, layout.align());

        // Taşma (overflow) kontrolü: checked_add başarısız olursa null döndür
        let alloc_end = match alloc_start.checked_add(layout.size()) {
            Some(end) => end,
            None => return ptr::null_mut(),
        };

        if alloc_end > self.heap_end {
            ptr::null_mut() // Heap kapasitesi doldu
        } else {
            let next = alloc_end;
            let allocations = self.allocations + 1;

            // `self` immutable referans olduğu için GlobalAlloc trait'i bunu zorunlu kılar.
            // İç mutability (interior mutability) için raw pointer dönüşümü kullanılır.
            // Normalde bir LockedHeap sarmalayıcısı (Mutex) bu sorunu çözer;
            // bu basit implementasyonda doğrudan ham pointer ile erişim yapılmaktadır.
            let self_ptr = self as *const Self as *mut Self;
            unsafe {
                (*self_ptr).next = next;
                (*self_ptr).allocations = allocations;
            }

            alloc_start as *mut u8
        }
    }

    /// Bellek iade işlemi.
    ///
    /// Bump allocator bireysel blokları serbest bırakamaz; bu nedenle
    /// bu fonksiyon yalnızca aktif allocasyon sayacını azaltır.
    ///
    /// ## Özel Durum: Sayaç Sıfırlandığında
    /// Eğer tüm bloklar "serbest bırakıldı" olarak işaretlenirse (`allocations == 0`),
    /// `next` işaretçisi `heap_start`'a sarılır ve heap'in tamamı yeniden kullanılabilir.
    /// Bu yaklaşım, tek seferlik toplu allocasyon + toplu serbest bırakma
    /// senaryolarında (örn: boot aşaması) çok etkilidir.
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        let self_ptr = self as *const Self as *mut Self;
        unsafe {
            // Sayacı bir azalt; sıfırın altına düşmesini önlemek için saturating_sub kullanılır
            (*self_ptr).allocations = self.allocations.saturating_sub(1);

            // Eğer tüm objeler silindiyse başa sarabiliriz.
            // Bu an itibarıyla heap sanki hiç kullanılmamış gibi davranır.
            if self.allocations == 0 {
                (*self_ptr).next = self.heap_start;
            }
        }
    }
}

/// Adresi verilen hizalamaya (align) göre yukarı yuvarlar.
///
/// ## Algoritma:
/// `(addr + align - 1) & !(align - 1)`
///
/// Bu bit manipülasyonu şu şekilde çalışır:
/// - `align` her zaman 2'nin kuvveti olmalıdır (örn: 1, 2, 4, 8, 16...)
/// - `align - 1` bir "maske" oluşturur (örn: align=8 → maske=0b0111)
/// - `!(align - 1)` bu maskeyi tersler (örn: 0b...11111000)
/// - `addr + align - 1` adresi "bir sonraki hizalama sınırının hemen öncesine" taşır
/// - `& !(align - 1)` alt bitleri temizleyerek hizalar
///
/// ## Örnek:
/// addr=5, align=4 → (5+3) & !3 = 8 & 0b...11111100 = 8
/// addr=8, align=4 → (8+3) & !3 = 11 & 0b...11111100 = 8 (zaten hizalı)
///
/// Zaman karmaşıklığı: O(1)
fn align_up(addr: usize, align: usize) -> usize {
    (addr + align - 1) & !(align - 1)
}
