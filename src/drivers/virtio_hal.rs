//! # VirtIO HAL (Hardware Abstraction Layer)
//!
//! Bu modül, `virtio_drivers` Rust kütüphanesinin gerektirdiği `Hal` trait'ini
//! echOS bellek yönetim altyapısına bağlar.
//!
//! ## HAL Nedir?
//!
//! VirtIO sürücüsü, DMA bellek yönetimi için platform bağımsız bir arayüz
//! bekler. `Hal` trait'i bu arayüzü tanımlar; biz de echOS'un bellek
//! fonksiyonlarıyla karşılık veririz.
//!
//! ## DMA (Direct Memory Access) Kavramı
//!
//! ```
//!  CPU → virtio_drivers → Hal::dma_alloc() → PhysAddr
//!                                    │
//!                          ┌─────────▼──────────────────┐
//!                          │   echOS DMA Allocator       │
//!                          │   (fiziksel bellek havuzu)  │
//!                          └────────────────────────────-┘
//!                                    │
//!  Donanım ◄─── PCI DMA ◄─── PhysAddr ─┘
//! ```
//!
//! TLSF (Two-Level Segregated Fit) heap DMA için uygun DEĞİLDİR;
//! çünkü fiziksel adres garantisi vermez. Bu yüzden ayrı DMA havuzu kullanılır.
//!
//! ## DMA Domain Kavramı
//!
//! SMP (çok işlemcili) sistemlerde her CPU çekirdeğinin ayrı bir DMA domain'i
//! olabilir. `current_dma_domain()` hangi domain üzerinde çalıştığımızı
//! döndürür. Bu, Simics sanal ortamında bellek izolasyonu sağlar.
//!
//! ## share / unshare
//!
//! VirtIO bazı tamponları eş zamanlı olarak hem CPU hem donanım cihazıyla
//! paylaşır. IOMMU'lu sistemlerde `share()` DMA eşlemesi oluşturur;
//! `unshare()` ise kaldırır. Bu implementasyonda IOMMU olmadığından
//! doğrudan sanal adres → fiziksel adres dönüşümü yapılır.

use crate::memory::{
    dma_alloc_for_domain, dma_dealloc_for_domain, dma_share_for_domain, dma_unshare_for_domain,
    map_mmio,
};
use core::ptr::NonNull;
use virtio_drivers::{BufferDirection, Hal, PhysAddr};

/// VirtIO HAL implementasyonu - echOS bellek altyapısını virtio_drivers'a sunar.
///
/// `VirtioHal` boş bir yapıdır (zero-sized type); tüm davranış
/// `Hal` trait metodlarında tanımlanır.
pub struct VirtioHal;

/// `Hal` trait unsafe implementasyonu.
///
/// `unsafe impl`: Bu trait metodları raw pointer döndürdüğünden ve doğrudan
/// fiziksel bellek üzerinde çalıştığından Rust unsafe garantisi gerektirir.
/// Derleyici bu kodu güvenli sayamaz; güvenliği biz garanti ederiz.
unsafe impl Hal for VirtioHal {
    /// DMA uyumlu bellek tahsis eder.
    ///
    /// # Parametreler
    /// - `pages`: Tahsis edilecek sayfa sayısı (her sayfa = 4096 byte)
    /// - `_direction`: Tampon yönü (IN/OUT/BOTH) — bu implementasyonda göz ardı edilir
    ///
    /// # Dönüş Değeri
    /// `(PhysAddr, NonNull<u8>)`: (fiziksel adres, sanal adres pointer'ı)
    ///
    /// DMA, donanımın CPU'dan bağımsız olarak belleğe erişmesini sağlar.
    /// Bu nedenle fiziksel adres (donanım görür) ve sanal adres (CPU görür)
    /// ikisi de gereklidir.
    fn dma_alloc(pages: usize, _direction: BufferDirection) -> (PhysAddr, NonNull<u8>) {
        let domain = crate::cpu::smp::current_dma_domain();
        crate::serial_println!("[VirtioHal] dma_alloc: {} pages, domain={}", pages, domain);

        match dma_alloc_for_domain(domain, pages) {
            Some((paddr, vaddr)) => {
                crate::serial_println!("[VirtioHal] dma_alloc OK: paddr={:#x}", paddr);
                // paddr=0 olmaması zorunludur: hardware'in 0 adresine DMA yapması
                // undefined behavior'a yol açar
                assert!(paddr != 0, "DMA alloc returned physical address 0x0");
                (paddr, vaddr)
            }
            None => {
                crate::serial_println!("[VirtioHal] dma_alloc FAILED: {} pages for domain {}", pages, domain);
                // Bellek tükenirse panik: daha güvenli bir geri dönüş yolu yok
                panic!("[VirtioHal] DMA allocation failed")
            }
        }
    }

    /// DMA belleğini serbest bırakır.
    ///
    /// # Güvenlik
    /// - `paddr=0` veya `pages=0` ise hata kodu -1 döndürülür (çift serbest bırakmayı engeller)
    /// - Normal başarıda 0 döner (Linux kernel sözleşmesi)
    ///
    /// # DMA Domain
    /// Tahsis hangi domain'de yapılmışsa serbest bırakma da aynı domain'de yapılmalıdır.
    unsafe fn dma_dealloc(paddr: PhysAddr, _vaddr: NonNull<u8>, pages: usize) -> i32 {
        if paddr == 0 || pages == 0 {
            return -1;
        }
        let domain = crate::cpu::smp::current_dma_domain();
        dma_dealloc_for_domain(domain, paddr, pages);
        0
    }

    /// Fiziksel MMIO adresini sanal adrese çevirir.
    ///
    /// MMIO (Memory-Mapped I/O): Donanım register'larına normal bellek
    /// adresi gibi `read_volatile`/`write_volatile` ile erişilir.
    ///
    /// `map_mmio` fonksiyonu:
    /// 1. Fiziksel adresi sayfa tablosuna (page table) ekler
    /// 2. Karşılık gelen sanal adresi döndürür
    ///
    /// `NonNull::new(ptr).unwrap()`: ptr null ise panik; map_mmio
    /// başarısız olmamalıdır.
    unsafe fn mmio_phys_to_virt(paddr: PhysAddr, _size: usize) -> NonNull<u8> {
        let ptr = map_mmio(paddr as u64, _size);
        NonNull::new(ptr).unwrap()
    }

    /// Tamponu donanımla paylaşır; fiziksel adresi döndürür.
    ///
    /// VirtIO protokolünde sürücü, tampon adreslerini virtqueue descriptor'larına
    /// yazar. Donanım bu fiziksel adresleri kullanarak DMA yapar.
    ///
    /// IOMMU olmayan sistemlerde sanal adres = fiziksel adres (identity mapping),
    /// bu yüzden doğrudan pointer → fiziksel adres dönüşümü yapılabilir.
    ///
    /// Hata durumunda: bellek bozulmasına yol açabileceğinden panik uygundur.
    unsafe fn share(buffer: NonNull<[u8]>, _direction: BufferDirection) -> PhysAddr {
        let domain = crate::cpu::smp::current_dma_domain();
        dma_share_for_domain(domain, buffer)
            .expect("[VIRTIO] DMA share failed: unmapped buffer — potential memory corruption")
    }

    /// Donanım erişimini sonlandırır, tamponu CPU'ya iade eder.
    ///
    /// DMA işlemi bittikten sonra CPU, tampon belleğini güvenle okuyabilir.
    /// IOMMU'lu sistemlerde bu çağrı DMA eşlemesini kaldırır ve
    /// önbellek tutarlılığını (cache coherency) sağlar.
    unsafe fn unshare(_paddr: PhysAddr, buffer: NonNull<[u8]>, _direction: BufferDirection) {
        let domain = crate::cpu::smp::current_dma_domain();
        dma_unshare_for_domain(domain, buffer);
    }
}
