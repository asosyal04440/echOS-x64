//! # echOS Fiziksel Bellek Yöneticisi (PMM) — Bitmap Tabanlı
//!
//! UEFI Memory Map üzerinde çalışan bit haritası tabanlı fiziksel frame ayırıcı.
//!
//! ## Bitmap PMM Veri Yapısı
//!
//! Her fiziksel 4 KB çerçeve için 1 bit kullanılır (0 = boş, 1 = dolu):
//!
//! ```
//! Fiziksel Bellek:
//!  [frame0][frame1][frame2][frame3] ... [frameN]
//!
//! Bitmap (bellekte tutulur):
//!  [1111 0001] [1011 1111] [0000 0000] ...
//!   │││└─────── frame[4] = boş
//!   ││└──────── frame[5] = dolu
//!   │└───────── frame[6] = dolu
//!   └────────── frame[7] = dolu
//!
//! 1 u64 = 64 frame = 64 × 4 KB = 256 KB
//! 1 GB RAM için bitmap boyutu = (1 GB / 4 KB) / 8 = 32 KB
//! ```
//!
//! ## Tahsis Algoritması (O(1) Amorti)
//!
//! `last_idx` değişkeni son başarılı tahsisin indeksini takip eder.
//! Bir sonraki tahsis oradan başlayarak ilerler (döngüsel arama):
//!
//! ```
//! last_idx = 42 (son tahsisin frame indeksi)
//!      ↓
//! frame[43], frame[44], ... → boş mu bakılır
//!      ↓
//! boş frame bulunduğunda: bit = 1 yap, last_idx = frame_index, döndür
//!      ↓
//! RAM sonuna ulaşıldığında başa sar (wrap-around)
//! ```
//!
//! ## Ardışık Tahsis (Contiguous Allocation)
//!
//! `allocate_contiguous(N)` N tane arka arkaya boş frame arar.
//! THP (Transparent Huge Pages) ve DMA tamponları için gereklidir:
//!
//! ```
//! N = 512 (2 MB büyük sayfa için)
//!
//! bitmap'te 512 ardışık 0 bit ara:
//!   [... 0000 0000 0000 ...] → bulundu, hepsini 1 yap ve ilk frame'i döndür
//! ```
//!
//! ## Frame 0 Koruması
//!
//! Frame 0 (fiziksel adres 0x0) her zaman "dolu" olarak işaretlenir.
//! Bu, null pointer hata ayıklamasını kolaylaştırır:
//! - Sıfır fiziksel adres hiçbir zaman geçerli bir frame olmaz
//! - NULL dereference hataları erken yakalanır
//!
//! ## İlgili Modüller:
//! - `fibonacci_pmm.rs`: Zone tabanlı PMM (UEFI için tercih edilen)
//! - `mod.rs`: `MemoryManager` — `FibonacciPmm`'i sarar; bu modül alternatif implementasyon
//! - `thp.rs`: `allocate_contiguous_huge_frame()` — 512 ardışık frame tahsis eder

use uefi::table::boot::{MemoryDescriptor, MemoryType};
use x86_64::structures::paging::{FrameAllocator, PhysFrame, Size4KiB};
use x86_64::PhysAddr;

/// Bitmap Physical Memory Manager.
/// Her 4KiB frame için 1 bit kullanır (0=Boş, 1=Dolu).
pub struct BitmapPmm {
    /// Bitmap'in bellekteki adresi
    bitmap_ptr: *mut u64,
    /// Bitmap boyutu (u64 sayısı olarak)
    bitmap_len: usize,
    /// Toplam frame sayısı
    total_frames: usize,
    /// Kullanılan frame sayısı
    used_frames: usize,
    /// Son allocation indeksi (arama optimizasyonu için)
    last_idx: usize,
}

unsafe impl Send for BitmapPmm {}
unsafe impl Sync for BitmapPmm {}

impl BitmapPmm {
    /// Yeni boş PMM oluşturur.
    pub const fn empty() -> Self {
        Self {
            bitmap_ptr: core::ptr::null_mut(),
            bitmap_len: 0,
            total_frames: 0,
            used_frames: 0,
            last_idx: 0,
        }
    }

    /// UEFI Memory Map kullanarak PMM'i başlatır.
    ///
    /// # Güvenlik
    /// Memory map geçerli olmalıdır.
    pub unsafe fn init<'a, I>(&mut self, map_iter: I)
    where
        I: Iterator<Item = &'a MemoryDescriptor> + Clone,
    {
        // 1. Toplam bellek boyutunu hesapla
        let mut max_phys_addr = 0;
        for desc in map_iter.clone() {
            let end = desc.phys_start + desc.page_count * 4096;
            if end > max_phys_addr {
                max_phys_addr = end;
            }
        }

        self.total_frames = (max_phys_addr / 4096) as usize;
        let bitmap_size_bits = self.total_frames;
        let bitmap_size_u64 = (bitmap_size_bits + 63) / 64;
        let bitmap_size_bytes = bitmap_size_u64 * 8;

        // 2. Bitmap için uygun boş bir alan bul (CONVENTIONAL Memory)
        let mut bitmap_phys_start: Option<u64> = None;

        for desc in map_iter.clone() {
            if desc.ty == MemoryType::CONVENTIONAL {
                if desc.phys_start == 0 {
                    continue;
                } // Null pointer koruması

                let region_size = desc.page_count * 4096;
                if region_size >= bitmap_size_bytes as u64 {
                    bitmap_phys_start = Some(desc.phys_start);
                    break;
                }
            }
        }

        let bitmap_start = bitmap_phys_start.expect("PMM Bitmap için yeterli bellek yok!");
        self.bitmap_ptr = bitmap_start as *mut u64;
        self.bitmap_len = bitmap_size_u64;

        // 3. Bitmap'i başlat: Önce hepsini DOLU (1) işaretle.
        core::ptr::write_bytes(self.bitmap_ptr as *mut u8, 0xFF, bitmap_size_bytes);
        self.used_frames = self.total_frames;

        // 4. Memory Map'e göre BOŞ (0) olan alanları işaretle.
        for desc in map_iter {
            if desc.ty == MemoryType::CONVENTIONAL {
                let start_frame = (desc.phys_start / 4096) as usize;
                let end_frame = start_frame + desc.page_count as usize;

                for frame_idx in start_frame..end_frame {
                    self.free_frame_internal(frame_idx);
                }
            }
        }

        // 5. Bitmap'in kendi bulunduğu alanı DOLU olarak işaretle (Korumak için).
        let bitmap_start_frame = (bitmap_start / 4096) as usize;
        let bitmap_page_count = (bitmap_size_bytes + 4095) / 4096;
        for i in 0..bitmap_page_count {
            self.mark_frame_used(bitmap_start_frame + i);
        }
    }

    /// Dahili: Frame'i boşaltır (Bit = 0).
    fn free_frame_internal(&mut self, frame_idx: usize) {
        if frame_idx == 0 {
            return;
        } // Frame 0'ı asla boşaltma (Null koruması)
        if frame_idx >= self.total_frames {
            return;
        }

        let u64_idx = frame_idx / 64;
        let bit_idx = frame_idx % 64;

        unsafe {
            let chunk = self.bitmap_ptr.add(u64_idx);
            let mask = !(1u64 << bit_idx);
            // Eğer doluysa boşalt
            if (*chunk & (1 << bit_idx)) != 0 {
                *chunk &= mask;
                self.used_frames -= 1;
            }
        }
    }

    /// Dahili: Frame'i dolu işaretle (Bit = 1).
    fn mark_frame_used(&mut self, frame_idx: usize) {
        if frame_idx >= self.total_frames {
            return;
        }

        let u64_idx = frame_idx / 64;
        let bit_idx = frame_idx % 64;

        unsafe {
            let chunk = self.bitmap_ptr.add(u64_idx);
            let mask = 1u64 << bit_idx;
            if (*chunk & mask) == 0 {
                *chunk |= mask;
                self.used_frames += 1;
            }
        }
    }

    fn is_frame_free(&self, frame_idx: usize) -> bool {
        if frame_idx == 0 || frame_idx >= self.total_frames {
            return false;
        }
        let u64_idx = frame_idx / 64;
        let bit_idx = frame_idx % 64;
        unsafe {
            let chunk = *self.bitmap_ptr.add(u64_idx);
            (chunk & (1u64 << bit_idx)) == 0
        }
    }

    pub fn allocate_contiguous(&mut self, pages: usize) -> Option<PhysFrame> {
        if pages == 0 {
            return None;
        }
        let mut idx = 1usize;
        while idx + pages <= self.total_frames {
            let mut ok = true;
            let mut offset = 0usize;
            while offset < pages {
                if !self.is_frame_free(idx + offset) {
                    idx = idx + offset + 1;
                    ok = false;
                    break;
                }
                offset += 1;
            }
            if ok {
                for i in 0..pages {
                    self.mark_frame_used(idx + i);
                }
                let addr = PhysAddr::new((idx as u64) * 4096);
                return Some(PhysFrame::containing_address(addr));
            }
        }
        None
    }

    pub fn deallocate_contiguous(&mut self, start: PhysFrame, pages: usize) {
        if pages == 0 {
            return;
        }
        let start_idx = (start.start_address().as_u64() / 4096) as usize;
        for i in 0..pages {
            self.free_frame_internal(start_idx + i);
        }
    }
}

unsafe impl FrameAllocator<Size4KiB> for BitmapPmm {
    /// Boş bir frame ayırır.
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
        let start_idx = self.last_idx;

        // Arama yardımcı closure'ı
        let search = |start: usize, end: usize| -> Option<usize> {
            for i in start..end {
                unsafe {
                    let chunk = *self.bitmap_ptr.add(i);
                    if chunk != !0u64 {
                        // Hepsi 1 değilse boş yer var
                        let mut bit_idx = 0;
                        while bit_idx < 64 {
                            if (chunk & (1 << bit_idx)) == 0 {
                                let frame_idx = i * 64 + bit_idx;
                                if frame_idx < self.total_frames {
                                    return Some(frame_idx);
                                }
                            }
                            bit_idx += 1;
                        }
                    }
                }
            }
            None
        };

        // last_idx'ten sonuna kadar ara
        let mut frame_result = search(start_idx, self.bitmap_len);

        // Bulunamazsa başa dön
        if frame_result.is_none() && start_idx > 0 {
            frame_result = search(0, start_idx);
        }

        if let Some(frame_idx) = frame_result {
            self.mark_frame_used(frame_idx);
            self.last_idx = frame_idx / 64;

            let addr = PhysAddr::new(frame_idx as u64 * 4096);
            return Some(PhysFrame::containing_address(addr));
        }

        None
    }
}
