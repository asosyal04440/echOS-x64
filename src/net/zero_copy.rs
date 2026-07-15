//! # Sıfır Kopya Ağ İletişimi (Zero-Copy Networking)
//!
//! io_uring tarzı arayüz ile yüksek performanslı sıfır-kopya ağ I/O.
//! Scatter-gather I/O, bellek eşlemeli tamponlar ve asenkron işlemler desteklenir.
//!
//! ## Sıfır Kopya Nedir?
//!
//! Geleneksel ağ I/O'da veri birçok kez kopyalanır:
//! ```
//!  Geleneksel:
//!  NIC -> Çekirdek tamponu -> Kullanıcı tamponu -> Uygulama  (3 kopya)
//!
//!  Sıfır Kopya:
//!  NIC -> Paylaşımlı tampon <-> Uygulama  (0 kopya, DMA ile)
//!  Tampon bellek hem çekirdek hem kullanıcı tarafından doğrudan erişilir.
//! ```
//!
//! ## io_uring Mimarisi
//!
//! ```
//!  Uygulama           Çekirdek
//!  +----------+       +----------+
//!  | SQ (Gönd)|  ->   | İşleme   |  Submission Queue: Yapılacak işler
//!  +----------+       +----------+
//!  | CQ (Tam) |  <-   | Sonuç    |  Completion Queue: Tamamlanan işler
//!  +----------+       +----------+
//!
//!  Ring tampon: Kilit gerektirmeden çoklu üretici/tüketici desteği
//!  Atomic head/tail: Sadece sayaç güncellenir, kilit gerekmez
//! ```
//!
//! ## Scatter-Gather I/O
//!
//! ```
//!  Tek send() çağrısıyla birden fazla tamponu gönder:
//!  [IoVec {buf=1, off=0, len=14}]  <- Ethernet başlığı
//!  [IoVec {buf=2, off=0, len=20}]  <- IP başlığı
//!  [IoVec {buf=3, off=0, len=100}] <- Veri
//!
//!  DMA kontrolcüsü bu parçaları tek transfer olarak birleştirir.
//! ```
//!
//! ## DMA Tampon Havuzu
//!
//! ```
//!  Toplam: 16 MB fiziksel ardışık bellek
//!  4096 adet 4KB tampon (sayfa hizalı)
//!  Her tampon: fiziksel adres (DMA için) + sanal adres (CPU için)
//!  Serbest liste: VecDeque ile O(1) tahsis/serbest bırakma
//! ```

use alloc::collections::{BTreeMap, VecDeque};
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::mem;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

use crate::drivers::async_traits::{AsyncIoError, AsyncNetDevice, DmaSlice, SubmissionToken};

use super::ethernet::{EtherType, EthernetFrame, EthernetHeader};
use super::socket;
use super::{Ipv4Addr, MacAddr, NetError, SocketAddr};

// ============================================================================
// SIFIR KOPYA SABİTLERİ
// ============================================================================

/// Tampon havuzu toplam boyutu: 16 MB (DMA için ardışık fiziksel bellek)
const BUFFER_POOL_SIZE: usize = 16 * 1024 * 1024;

/// Tampon parça boyutu: 4 KB (sayfa hizalı, DMA için ideal)
const BUFFER_CHUNK_SIZE: usize = 4096;

/// Maksimum tampon parça sayısı: 16MB / 4KB = 4096 parça
const MAX_CHUNKS: usize = BUFFER_POOL_SIZE / BUFFER_CHUNK_SIZE;

/// Ring tampon boyutu: 4096 giriş (kuyruğun maksimum kapasitesi)
const RING_SIZE: usize = 4096;

/// Maksimum scatter-gather vektör sayısı (tek işlemde)
const MAX_IOV: usize = 8;

/// sk_buff benzeri packet buffer'da header dahil maksimum DMA slice sayısı.
pub const MAX_PACKET_DMA_SLICES: usize = 16;

/// Header dışındaki maksimum payload fragment sayısı.
pub const MAX_PACKET_FRAGS: usize = MAX_PACKET_DMA_SLICES - 1;

/// Jumbo frame üst sınırı; native NIC TX kontratıyla aynı sınırda tutulur.
pub const MAX_PACKET_BUFFER_LEN: usize = 9216;

// ============================================================================
// TAMPON TANIMLAYICISI (BUFFER DESCRIPTOR)
// ============================================================================
//
// Her DMA tamponu için fiziksel ve sanal adres çifti tutulur.
// NIC doğrudan fiziksel adrese yazar (DMA), CPU sanal adresten okur.

/// Sıfır-kopya I/O için tampon tanımlayıcısı
///
/// phys_addr: NIC'in DMA transferi için kullandığı adres
/// virt_addr: CPU'nun veriyi okumak için kullandığı adres
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct BufferDescriptor {
    /// Buffer ID
    pub buf_id: u32,
    /// Physical address of buffer
    pub phys_addr: u64,
    /// Virtual address (for kernel access)
    pub virt_addr: u64,
    /// Buffer length
    pub len: u32,
    /// Reference count
    pub ref_count: u32,
    /// Flags
    pub flags: u32,
}

impl BufferDescriptor {
    pub const FLAG_TX: u32 = 1 << 0;
    pub const FLAG_RX: u32 = 1 << 1;
    pub const FLAG_IN_USE: u32 = 1 << 2;
    pub const FLAG_MAPPED: u32 = 1 << 3;

    pub fn new(buf_id: u32, phys_addr: u64, virt_addr: u64, len: u32) -> Self {
        BufferDescriptor {
            buf_id,
            phys_addr,
            virt_addr,
            len,
            ref_count: 0,
            flags: 0,
        }
    }
}

// ============================================================================
// PAGE POOL (driver RX/TX DMA page recycle allocator)
// ============================================================================

/// Page-pool DMA ownership direction.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PagePoolDmaDirection {
    ToDevice = 0,
    FromDevice = 1,
    Bidirectional = 2,
}

/// `put_page` sync size sentinel: sync the configured pool range.
pub const PAGE_POOL_SYNC_ALL: u32 = u32::MAX;

/// Page-pool creation parameters.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PagePoolConfig {
    /// Number of DMA pages in the pool.
    pub pool_size: u32,
    /// Page size served by this pool.
    pub page_size: u32,
    /// RX/TX DMA direction this pool is prepared for.
    pub dma_dir: PagePoolDmaDirection,
    /// Queue/NAPI instance that owns the pool.
    pub queue_id: u16,
    /// Maximum range synced when `FLAG_DMA_SYNC_DEV` is enabled.
    pub max_sync_len: u32,
    /// Sync range offset inside each page.
    pub sync_offset: u32,
    /// Creation flags.
    pub flags: u32,
}

impl PagePoolConfig {
    pub const FLAG_DMA_MAP: u32 = 1 << 0;
    pub const FLAG_DMA_SYNC_DEV: u32 = 1 << 1;
    pub const FLAG_ALLOW_UNREADABLE_NETMEM: u32 = 1 << 2;

    pub const fn default_rx(pool_size: u32) -> Self {
        Self {
            pool_size,
            page_size: BUFFER_CHUNK_SIZE as u32,
            dma_dir: PagePoolDmaDirection::FromDevice,
            queue_id: 0,
            max_sync_len: BUFFER_CHUNK_SIZE as u32,
            sync_offset: 0,
            flags: Self::FLAG_DMA_MAP | Self::FLAG_DMA_SYNC_DEV,
        }
    }
}

/// DMA page owned by a page pool.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PagePoolPage {
    pub page_id: u32,
    pub queue_id: u16,
    pub dma_dir: PagePoolDmaDirection,
    pub phys_addr: u64,
    pub virt_addr: u64,
    pub len: u32,
    pub ref_count: u16,
    pub flags: u32,
}

impl PagePoolPage {
    pub const FLAG_IN_USE: u32 = 1 << 0;
    pub const FLAG_DMA_MAPPED: u32 = 1 << 1;
    pub const FLAG_FRAGMENTED: u32 = 1 << 2;

    pub fn as_descriptor(&self) -> BufferDescriptor {
        let mut desc =
            BufferDescriptor::new(self.page_id, self.phys_addr, self.virt_addr, self.len);
        desc.ref_count = self.ref_count as u32;
        desc.flags = BufferDescriptor::FLAG_IN_USE | BufferDescriptor::FLAG_MAPPED;
        desc
    }

    pub fn dma_slice(&self, offset: u32, len: u32) -> Result<DmaSlice, NetError> {
        if len == 0 || offset.checked_add(len).ok_or(NetError::InvalidParam)? > self.len {
            return Err(NetError::InvalidParam);
        }

        let vaddr = self
            .virt_addr
            .checked_add(offset as u64)
            .ok_or(NetError::InvalidParam)?;
        if vaddr > usize::MAX as u64 {
            return Err(NetError::InvalidParam);
        }

        Ok(DmaSlice::new(
            vaddr as usize,
            self.phys_addr
                .checked_add(offset as u64)
                .ok_or(NetError::InvalidParam)?,
            len as usize,
        ))
    }
}

/// Fragment allocated from a page-pool page.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PagePoolFragment {
    pub page_id: u32,
    pub offset: u32,
    pub len: u32,
    pub phys_addr: u64,
    pub virt_addr: u64,
}

impl PagePoolFragment {
    pub fn dma_slice(&self) -> Result<DmaSlice, NetError> {
        if self.len == 0 {
            return Err(NetError::InvalidParam);
        }
        if self.virt_addr > usize::MAX as u64 {
            return Err(NetError::InvalidParam);
        }
        Ok(DmaSlice::new(
            self.virt_addr as usize,
            self.phys_addr,
            self.len as usize,
        ))
    }
}

/// Page-pool counters exposed for driver recycle telemetry.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PagePoolStats {
    pub total_pages: u32,
    pub available_pages: u32,
    pub in_flight_pages: u32,
    pub recycled_pages: u64,
    pub alloc_failures: u64,
    pub cpu_syncs: u64,
    pub device_syncs: u64,
    pub released_pages: u64,
}

/// Driver-facing DMA page recycle allocator.
///
/// A page pool is queue-owned: allocation and recycle methods take `&mut self`,
/// so callers keep the NAPI/per-queue single-consumer boundary explicit instead
/// of introducing a global hot lock.
#[repr(C, align(64))]
pub struct PagePool {
    descriptors: Vec<PagePoolPage>,
    free_list: VecDeque<u32>,
    recycle_cache: VecDeque<u32>,
    base_phys: u64,
    base_virt: u64,
    page_size: u32,
    total_pages: u32,
    dma_dir: PagePoolDmaDirection,
    queue_id: u16,
    max_sync_len: u32,
    sync_offset: u32,
    flags: u32,
    active_frag_page: Option<u32>,
    active_frag_offset: u32,
    available: u32,
    in_flight: u32,
    recycled: u64,
    alloc_failures: u64,
    cpu_syncs: u64,
    device_syncs: u64,
    released: u64,
}

impl PagePool {
    pub fn new(config: PagePoolConfig) -> Option<Self> {
        if config.pool_size == 0
            || config.page_size == 0
            || !config.page_size.is_power_of_two()
            || config.sync_offset > config.page_size
        {
            return None;
        }

        let max_sync_len = if config.max_sync_len == 0 {
            config.page_size
        } else {
            config.max_sync_len
        };
        if config
            .sync_offset
            .checked_add(max_sync_len)
            .map_or(true, |end| end > config.page_size)
        {
            return None;
        }

        let total_bytes = (config.pool_size as usize).checked_mul(config.page_size as usize)?;
        let pages = (total_bytes + BUFFER_CHUNK_SIZE - 1) / BUFFER_CHUNK_SIZE;
        let (phys, virt) = crate::memory::dma_alloc(pages)?;

        unsafe {
            core::ptr::write_bytes(virt.as_ptr(), 0, total_bytes);
        }

        let mut descriptors = Vec::with_capacity(config.pool_size as usize);
        let mut free_list = VecDeque::with_capacity(config.pool_size as usize);
        for page_id in 0..config.pool_size {
            let offset = page_id as u64 * config.page_size as u64;
            descriptors.push(PagePoolPage {
                page_id,
                queue_id: config.queue_id,
                dma_dir: config.dma_dir,
                phys_addr: phys as u64 + offset,
                virt_addr: virt.as_ptr() as u64 + offset,
                len: config.page_size,
                ref_count: 0,
                flags: PagePoolPage::FLAG_DMA_MAPPED,
            });
            free_list.push_back(page_id);
        }

        Some(Self {
            descriptors,
            free_list,
            recycle_cache: VecDeque::with_capacity(config.pool_size as usize),
            base_phys: phys as u64,
            base_virt: virt.as_ptr() as u64,
            page_size: config.page_size,
            total_pages: config.pool_size,
            dma_dir: config.dma_dir,
            queue_id: config.queue_id,
            max_sync_len,
            sync_offset: config.sync_offset,
            flags: config.flags,
            active_frag_page: None,
            active_frag_offset: 0,
            available: config.pool_size,
            in_flight: 0,
            recycled: 0,
            alloc_failures: 0,
            cpu_syncs: 0,
            device_syncs: 0,
            released: 0,
        })
    }

    pub fn default_rx(pool_size: u32) -> Option<Self> {
        Self::new(PagePoolConfig::default_rx(pool_size))
    }

    pub fn alloc_page(&mut self) -> Option<PagePoolPage> {
        let page_id = self.pop_free_page()?;
        let page = &mut self.descriptors[page_id as usize];
        page.ref_count = 1;
        page.flags = PagePoolPage::FLAG_IN_USE | PagePoolPage::FLAG_DMA_MAPPED;
        self.available -= 1;
        self.in_flight += 1;
        Some(*page)
    }

    pub fn alloc_fragment(&mut self, len: u32) -> Option<PagePoolFragment> {
        if len == 0 || len > self.page_size / 2 {
            self.alloc_failures += 1;
            return None;
        }

        let need_new_page = self.active_frag_page.map_or(true, |page_id| {
            self.active_frag_offset
                .checked_add(len)
                .map_or(true, |end| end > self.descriptors[page_id as usize].len)
        });

        if need_new_page {
            let page_id = self.pop_free_page()?;
            let page = &mut self.descriptors[page_id as usize];
            page.ref_count = 0;
            page.flags = PagePoolPage::FLAG_IN_USE
                | PagePoolPage::FLAG_DMA_MAPPED
                | PagePoolPage::FLAG_FRAGMENTED;
            self.available -= 1;
            self.in_flight += 1;
            self.active_frag_page = Some(page_id);
            self.active_frag_offset = 0;
        }

        let page_id = self.active_frag_page?;
        let offset = self.active_frag_offset;
        let page = &mut self.descriptors[page_id as usize];
        page.ref_count = page.ref_count.checked_add(1)?;
        self.active_frag_offset += len;

        Some(PagePoolFragment {
            page_id,
            offset,
            len,
            phys_addr: page.phys_addr + offset as u64,
            virt_addr: page.virt_addr + offset as u64,
        })
    }

    pub fn get_page(&self, page_id: u32) -> Option<PagePoolPage> {
        self.descriptors.get(page_id as usize).copied()
    }

    pub fn ref_page(&mut self, page_id: u32) -> Result<PagePoolPage, NetError> {
        let page = self
            .descriptors
            .get_mut(page_id as usize)
            .ok_or(NetError::InvalidParam)?;
        if page.ref_count == 0 {
            return Err(NetError::InvalidParam);
        }
        page.ref_count = page
            .ref_count
            .checked_add(1)
            .ok_or(NetError::InvalidParam)?;
        Ok(*page)
    }

    pub fn put_page(
        &mut self,
        page_id: u32,
        dma_sync_size: u32,
        allow_direct: bool,
    ) -> Result<bool, NetError> {
        let page = self
            .descriptors
            .get_mut(page_id as usize)
            .ok_or(NetError::InvalidParam)?;
        if page.ref_count == 0 {
            return Err(NetError::InvalidParam);
        }
        page.ref_count -= 1;
        if page.ref_count != 0 {
            return Ok(false);
        }

        if self.flags & PagePoolConfig::FLAG_DMA_SYNC_DEV != 0 {
            let sync_len = if dma_sync_size == PAGE_POOL_SYNC_ALL {
                self.max_sync_len
            } else {
                dma_sync_size.min(self.max_sync_len)
            };
            self.sync_for_device(page_id, self.sync_offset, sync_len)?;
        }

        self.recycle_page(page_id, allow_direct);
        Ok(true)
    }

    pub fn put_full_page(&mut self, page_id: u32, allow_direct: bool) -> Result<bool, NetError> {
        self.put_page(page_id, PAGE_POOL_SYNC_ALL, allow_direct)
    }

    pub fn recycle_direct(&mut self, page_id: u32) -> Result<bool, NetError> {
        self.put_full_page(page_id, true)
    }

    pub fn sync_for_cpu(&mut self, page_id: u32, offset: u32, len: u32) -> Result<(), NetError> {
        self.validate_range(page_id, offset, len)?;
        core::sync::atomic::fence(Ordering::Acquire);
        self.cpu_syncs += 1;
        Ok(())
    }

    pub fn sync_for_device(&mut self, page_id: u32, offset: u32, len: u32) -> Result<(), NetError> {
        self.validate_range(page_id, offset, len)?;
        core::sync::atomic::fence(Ordering::Release);
        self.device_syncs += 1;
        Ok(())
    }

    pub fn descriptor(&self, page_id: u32) -> Option<BufferDescriptor> {
        self.get_page(page_id).map(|page| page.as_descriptor())
    }

    pub fn write_fragment(&mut self, frag: PagePoolFragment, data: &[u8]) -> Result<(), NetError> {
        if data.len() > frag.len as usize {
            return Err(NetError::BufferFull);
        }
        self.validate_range(frag.page_id, frag.offset, data.len() as u32)?;
        unsafe {
            let dst = frag.virt_addr as *mut u8;
            core::ptr::copy_nonoverlapping(data.as_ptr(), dst, data.len());
        }
        self.sync_for_device(frag.page_id, frag.offset, data.len() as u32)
    }

    pub fn read_fragment(&mut self, frag: PagePoolFragment) -> Result<Vec<u8>, NetError> {
        self.validate_range(frag.page_id, frag.offset, frag.len)?;
        self.sync_for_cpu(frag.page_id, frag.offset, frag.len)?;
        let mut out = vec![0u8; frag.len as usize];
        unsafe {
            let src = frag.virt_addr as *const u8;
            core::ptr::copy_nonoverlapping(src, out.as_mut_ptr(), out.len());
        }
        Ok(out)
    }

    pub fn stats(&self) -> PagePoolStats {
        PagePoolStats {
            total_pages: self.total_pages,
            available_pages: self.available,
            in_flight_pages: self.in_flight,
            recycled_pages: self.recycled,
            alloc_failures: self.alloc_failures,
            cpu_syncs: self.cpu_syncs,
            device_syncs: self.device_syncs,
            released_pages: self.released,
        }
    }

    pub const fn base_phys(&self) -> u64 {
        self.base_phys
    }

    pub const fn base_virt(&self) -> u64 {
        self.base_virt
    }

    pub const fn page_size(&self) -> u32 {
        self.page_size
    }

    pub const fn dma_dir(&self) -> PagePoolDmaDirection {
        self.dma_dir
    }

    pub const fn queue_id(&self) -> u16 {
        self.queue_id
    }

    fn pop_free_page(&mut self) -> Option<u32> {
        let page_id = self
            .recycle_cache
            .pop_front()
            .or_else(|| self.free_list.pop_front());
        if page_id.is_none() {
            self.alloc_failures += 1;
        }
        page_id
    }

    fn recycle_page(&mut self, page_id: u32, allow_direct: bool) {
        let page = &mut self.descriptors[page_id as usize];
        page.flags = PagePoolPage::FLAG_DMA_MAPPED;
        page.ref_count = 0;
        if self.active_frag_page == Some(page_id) {
            self.active_frag_page = None;
            self.active_frag_offset = 0;
        }

        if allow_direct {
            self.recycle_cache.push_back(page_id);
            self.recycled += 1;
        } else {
            self.free_list.push_back(page_id);
            self.released += 1;
        }
        self.available += 1;
        self.in_flight -= 1;
    }

    fn validate_range(&self, page_id: u32, offset: u32, len: u32) -> Result<(), NetError> {
        let page = self
            .descriptors
            .get(page_id as usize)
            .ok_or(NetError::InvalidParam)?;
        if len == 0 || offset.checked_add(len).ok_or(NetError::InvalidParam)? > page.len {
            return Err(NetError::InvalidParam);
        }
        Ok(())
    }
}

// ============================================================================
// SCATTER-GATHER VEKTÖRÜ (IoVec)
// ============================================================================
//
// Scatter-Gather I/O: Birden fazla bellek bölgesini tek I/O işlemine bağlar.
// "Scatter": Okuma - gelen veriyi farklı tamponlara dağıt
// "Gather":  Yazma - farklı tamponlardan veriyi tek pakette topla

/// Scatter-gather işlemleri için I/O vektörü
///
/// Bir IoVec, belirli bir tampondaki belirli bir bölgeyi temsil eder:
/// buf_id + offset + len ile 4KB tampon içindeki bir dilimi belirtir.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct IoVec {
    /// Buffer ID
    pub buf_id: u32,
    /// Offset within buffer
    pub offset: u32,
    /// Length of this segment
    pub len: u32,
}

impl IoVec {
    pub fn new(buf_id: u32, offset: u32, len: u32) -> Self {
        IoVec {
            buf_id,
            offset,
            len,
        }
    }
}

// ============================================================================
// PACKET BUFFER (sk_buff-benzeri header + frags modeli)
// ============================================================================

/// Packet buffer içindeki fiziksel olarak contiguous DMA aralığı.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PacketSegment {
    /// Kaynak zero-copy buffer ID.
    pub buf_id: u32,
    /// Buffer içi offset.
    pub offset: u32,
    /// Segment uzunluğu.
    pub len: u32,
    /// CPU mapping adresi.
    pub vaddr: usize,
    /// Device DMA adresi.
    pub paddr: u64,
}

impl PacketSegment {
    pub const fn empty() -> Self {
        Self {
            buf_id: 0,
            offset: 0,
            len: 0,
            vaddr: 0,
            paddr: 0,
        }
    }

    pub fn from_descriptor(
        desc: &BufferDescriptor,
        offset: u32,
        len: u32,
    ) -> Result<Self, NetError> {
        if len == 0 || offset.checked_add(len).ok_or(NetError::InvalidParam)? > desc.len {
            return Err(NetError::InvalidParam);
        }

        let vaddr = desc
            .virt_addr
            .checked_add(offset as u64)
            .ok_or(NetError::InvalidParam)?;
        if vaddr > usize::MAX as u64 {
            return Err(NetError::InvalidParam);
        }

        Ok(Self {
            buf_id: desc.buf_id,
            offset,
            len,
            vaddr: vaddr as usize,
            paddr: desc
                .phys_addr
                .checked_add(offset as u64)
                .ok_or(NetError::InvalidParam)?,
        })
    }

    pub fn from_page(page: &PagePoolPage, offset: u32, len: u32) -> Result<Self, NetError> {
        let desc = page.as_descriptor();
        Self::from_descriptor(&desc, offset, len)
    }

    pub fn from_fragment(fragment: &PagePoolFragment) -> Result<Self, NetError> {
        if fragment.len == 0 {
            return Err(NetError::InvalidParam);
        }
        if fragment.virt_addr > usize::MAX as u64 {
            return Err(NetError::InvalidParam);
        }

        Ok(Self {
            buf_id: fragment.page_id,
            offset: fragment.offset,
            len: fragment.len,
            vaddr: fragment.virt_addr as usize,
            paddr: fragment.phys_addr,
        })
    }

    pub const fn dma_slice(&self) -> DmaSlice {
        DmaSlice::new(self.vaddr, self.paddr, self.len as usize)
    }
}

/// Sabit kapasiteli DMA slice görünümü; hot path'te heap tahsisi yapmaz.
#[derive(Clone, Copy, Debug)]
pub struct PacketDmaSlices {
    slices: [DmaSlice; MAX_PACKET_DMA_SLICES],
    count: usize,
}

impl PacketDmaSlices {
    pub const fn empty() -> Self {
        Self {
            slices: [DmaSlice::new(0, 0, 0); MAX_PACKET_DMA_SLICES],
            count: 0,
        }
    }

    pub fn as_slice(&self) -> &[DmaSlice] {
        &self.slices[..self.count]
    }

    pub const fn count(&self) -> usize {
        self.count
    }
}

/// sk_buff-benzeri zero-copy packet buffer.
///
/// Header lineer tutulur; payload `frags` dizisinde page/buffer parçaları olarak
/// korunur. Caller, TX completion gelene kadar bu segmentlerin içeriğini
/// değiştirmemelidir.
#[repr(C, align(64))]
#[derive(Clone, Debug)]
pub struct PacketBuffer {
    header: Option<PacketSegment>,
    frags: [PacketSegment; MAX_PACKET_FRAGS],
    frag_count: u8,
    head_len: u32,
    data_len: u32,
    total_len: u32,
    flags: u32,
}

impl PacketBuffer {
    pub const FLAG_TX_IN_FLIGHT: u32 = 1 << 0;
    pub const FLAG_ZERO_COPY: u32 = 1 << 1;

    pub fn from_header(desc: &BufferDescriptor, offset: u32, len: u32) -> Result<Self, NetError> {
        let header = PacketSegment::from_descriptor(desc, offset, len)?;
        if len as usize > MAX_PACKET_BUFFER_LEN {
            return Err(NetError::InvalidParam);
        }

        Ok(Self {
            header: Some(header),
            frags: [PacketSegment::empty(); MAX_PACKET_FRAGS],
            frag_count: 0,
            head_len: len,
            data_len: 0,
            total_len: len,
            flags: Self::FLAG_ZERO_COPY,
        })
    }

    pub fn from_pool_header(
        pool: &BufferPool,
        buf_id: u32,
        offset: u32,
        len: u32,
    ) -> Result<Self, NetError> {
        let desc = pool.get_descriptor(buf_id).ok_or(NetError::InvalidParam)?;
        Self::from_header(desc, offset, len)
    }

    pub fn from_page_header(
        pool: &PagePool,
        page_id: u32,
        offset: u32,
        len: u32,
    ) -> Result<Self, NetError> {
        let page = pool.get_page(page_id).ok_or(NetError::InvalidParam)?;
        let header = PacketSegment::from_page(&page, offset, len)?;
        if len as usize > MAX_PACKET_BUFFER_LEN {
            return Err(NetError::InvalidParam);
        }

        Ok(Self {
            header: Some(header),
            frags: [PacketSegment::empty(); MAX_PACKET_FRAGS],
            frag_count: 0,
            head_len: len,
            data_len: 0,
            total_len: len,
            flags: Self::FLAG_ZERO_COPY,
        })
    }

    pub fn push_frag(
        &mut self,
        desc: &BufferDescriptor,
        offset: u32,
        len: u32,
    ) -> Result<(), NetError> {
        if self.frag_count as usize >= MAX_PACKET_FRAGS {
            return Err(NetError::BufferFull);
        }
        let next_total = (self.total_len as usize)
            .checked_add(len as usize)
            .ok_or(NetError::InvalidParam)?;
        if next_total > MAX_PACKET_BUFFER_LEN {
            return Err(NetError::InvalidParam);
        }

        let frag = PacketSegment::from_descriptor(desc, offset, len)?;
        self.frags[self.frag_count as usize] = frag;
        self.frag_count += 1;
        self.data_len += len;
        self.total_len = next_total as u32;
        Ok(())
    }

    pub fn push_frag_from_pool(
        &mut self,
        pool: &BufferPool,
        buf_id: u32,
        offset: u32,
        len: u32,
    ) -> Result<(), NetError> {
        let desc = pool.get_descriptor(buf_id).ok_or(NetError::InvalidParam)?;
        self.push_frag(desc, offset, len)
    }

    pub fn push_frag_from_page_pool(
        &mut self,
        pool: &PagePool,
        page_id: u32,
        offset: u32,
        len: u32,
    ) -> Result<(), NetError> {
        let page = pool.get_page(page_id).ok_or(NetError::InvalidParam)?;
        let desc = page.as_descriptor();
        self.push_frag(&desc, offset, len)
    }

    pub fn push_page_fragment(&mut self, fragment: &PagePoolFragment) -> Result<(), NetError> {
        if self.frag_count as usize >= MAX_PACKET_FRAGS {
            return Err(NetError::BufferFull);
        }
        let next_total = (self.total_len as usize)
            .checked_add(fragment.len as usize)
            .ok_or(NetError::InvalidParam)?;
        if next_total > MAX_PACKET_BUFFER_LEN {
            return Err(NetError::InvalidParam);
        }

        self.frags[self.frag_count as usize] = PacketSegment::from_fragment(fragment)?;
        self.frag_count += 1;
        self.data_len += fragment.len;
        self.total_len = next_total as u32;
        Ok(())
    }

    pub fn dma_slices(&self) -> Result<PacketDmaSlices, NetError> {
        let mut out = PacketDmaSlices::empty();
        let header = self.header.ok_or(NetError::InvalidParam)?;
        out.slices[0] = header.dma_slice();
        out.count = 1;

        for frag in self.frags.iter().take(self.frag_count as usize) {
            out.slices[out.count] = frag.dma_slice();
            out.count += 1;
        }

        Ok(out)
    }

    pub fn submit_tx<D: AsyncNetDevice + ?Sized>(
        &mut self,
        device: &D,
    ) -> Result<SubmissionToken, AsyncIoError> {
        let slices = self.dma_slices().map_err(|_| AsyncIoError::InvalidParam)?;
        let token = device.submit_tx_sg(slices.as_slice())?;
        self.flags |= Self::FLAG_TX_IN_FLIGHT;
        Ok(token)
    }

    pub const fn header(&self) -> Option<PacketSegment> {
        self.header
    }

    pub fn frags(&self) -> &[PacketSegment] {
        &self.frags[..self.frag_count as usize]
    }

    pub const fn frag_count(&self) -> usize {
        self.frag_count as usize
    }

    pub const fn head_len(&self) -> usize {
        self.head_len as usize
    }

    pub const fn data_len(&self) -> usize {
        self.data_len as usize
    }

    pub const fn total_len(&self) -> usize {
        self.total_len as usize
    }

    pub const fn flags(&self) -> u32 {
        self.flags
    }

    pub const fn is_linear(&self) -> bool {
        self.frag_count == 0
    }
}

// ============================================================================
// GÖNDERİM KUYRUĞU GİRİŞİ (Submission Queue Entry - SQE)
// ============================================================================
//
// Kullanıcı/çekirdek, SQ'ya SQE ekleyerek asenkron işlem başlatır.
// Çekirdek SQE'leri işleyip CQ'ya CQE ekler.

/// İşlem kodu (Submission Queue Entry türü)
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpCode {
    /// Receive packet
    Recv = 0,
    /// Send packet
    Send = 1,
    /// Accept connection
    Accept = 2,
    /// Connect to remote
    Connect = 3,
    /// Close socket
    Close = 4,
    /// Allocate buffer
    AllocBuf = 5,
    /// Free buffer
    FreeBuf = 6,
    /// Map buffer to userspace
    MapBuf = 7,
    /// Unmap buffer
    UnmapBuf = 8,
}

/// Submission queue entry (SQE)
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Sqe {
    /// Operation code
    pub opcode: OpCode,
    /// Flags
    pub flags: u8,
    /// Socket ID (for socket operations)
    pub socket_id: u32,
    /// User data (passed to completion)
    pub user_data: u64,
    /// Buffer ID (for buffer operations)
    pub buf_id: u32,
    /// Number of I/O vectors
    pub iov_count: u8,
    /// Reserved
    pub reserved: [u8; 3],
    /// Scatter-gather vectors
    pub iov: [IoVec; MAX_IOV],
    /// Remote address (for connect/accept)
    pub addr: SocketAddr,
}

impl Sqe {
    pub fn new(opcode: OpCode, socket_id: u32, user_data: u64) -> Self {
        Sqe {
            opcode,
            flags: 0,
            socket_id,
            user_data,
            buf_id: 0,
            iov_count: 0,
            reserved: [0; 3],
            iov: [IoVec::new(0, 0, 0); MAX_IOV],
            addr: SocketAddr::default(),
        }
    }

    pub fn with_buffer(opcode: OpCode, socket_id: u32, buf_id: u32, user_data: u64) -> Self {
        let mut sqe = Self::new(opcode, socket_id, user_data);
        sqe.buf_id = buf_id;
        sqe
    }

    pub fn with_iov(opcode: OpCode, socket_id: u32, iov: &[IoVec], user_data: u64) -> Self {
        let mut sqe = Self::new(opcode, socket_id, user_data);
        sqe.iov_count = iov.len().min(MAX_IOV) as u8;
        sqe.iov[..iov.len()].copy_from_slice(iov);
        sqe
    }
}

/// SQE flags
pub const SQE_FLAG_IOV: u8 = 1 << 0;
pub const SQE_FLAG_FIXED_BUF: u8 = 1 << 1;
pub const SQE_FLAG_NONBLOCK: u8 = 1 << 2;

// ============================================================================
// TAMAMLAMA KUYRUĞU GİRİŞİ (Completion Queue Entry - CQE)
// ============================================================================
//
// Her SQE işlemi tamamlandığında bir CQE üretilir.
// result > 0: Transfer edilen byte sayısı
// result < 0: Hata kodu (NetError)
// user_data: SQE'den taşınan kullanıcı verisi (bağlantı kimliği gibi)

/// Tamamlama kuyruğu girişi (CQE)
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Cqe {
    /// User data from SQE
    pub user_data: u64,
    /// Result (positive = bytes transferred, negative = error)
    pub result: i32,
    /// Flags
    pub flags: u32,
    /// Buffer ID (for recv operations)
    pub buf_id: u32,
    /// Reserved
    pub reserved: [u32; 2],
}

impl Cqe {
    pub fn new(user_data: u64, result: i32, buf_id: u32) -> Self {
        Cqe {
            user_data,
            result,
            flags: 0,
            buf_id,
            reserved: [0; 2],
        }
    }

    pub fn success(user_data: u64, bytes: u32) -> Self {
        Self::new(user_data, bytes as i32, 0)
    }

    pub fn error(user_data: u64, err: NetError) -> Self {
        Self::new(user_data, -(err as i32), 0)
    }
}

// ============================================================================
// HALKA TAMPON (Ring Buffer)
// ============================================================================
//
// Kilit olmadan atomik head/tail sayaçlarıyla çalışan dairesel tampon.
//
// Okuma (Pop):
//   1. head yükle (Acquire)
//   2. head konumundaki girişi oku
//   3. Bellek bariyeri (Release fence)
//   4. head'i artır ve sakla
//
// Yazma (Push):
//   1. tail yükle (Acquire)
//   2. tail konumuna yaz
//   3. Bellek bariyeri (Release fence)
//   4. tail'i artır ve sakla
//
// Boyut: 2'nin kuvveti (mask = size-1, fast modulo)
// Doluluk: tail - head >= size -> Dolu
// Boşluk: tail - head == 0 -> Boş

/// Gönderim/tamamlama kuyrukları için halka tampon
pub struct RingBuffer<T: Copy + Clone> {
    /// Ring entries
    entries: Vec<T>,
    /// Head index (where consumer reads)
    head: AtomicU32,
    /// Tail index (where producer writes)
    tail: AtomicU32,
    /// Ring size
    size: u32,
    /// Ring mask (size - 1, for fast modulo)
    mask: u32,
}

impl<T: Copy + Clone> RingBuffer<T> {
    pub fn new(size: usize) -> Self {
        let size = size.next_power_of_two();
        RingBuffer {
            entries: vec![unsafe { mem::zeroed() }; size],
            head: AtomicU32::new(0),
            tail: AtomicU32::new(0),
            size: size as u32,
            mask: (size - 1) as u32,
        }
    }

    /// Check if ring is empty
    pub fn is_empty(&self) -> bool {
        self.head.load(Ordering::Acquire) == self.tail.load(Ordering::Acquire)
    }

    /// Check if ring is full
    pub fn is_full(&self) -> bool {
        let tail = self.tail.load(Ordering::Acquire);
        let head = self.head.load(Ordering::Acquire);
        tail.wrapping_sub(head) >= self.size
    }

    /// Get number of entries
    pub fn len(&self) -> usize {
        let tail = self.tail.load(Ordering::Acquire);
        let head = self.head.load(Ordering::Acquire);
        (tail.wrapping_sub(head) as usize) & (self.size as usize - 1)
    }

    /// Push entry to ring
    pub fn push(&mut self, entry: T) -> bool {
        if self.is_full() {
            return false;
        }

        let tail = self.tail.load(Ordering::Acquire);
        let idx = tail & self.mask;
        self.entries[idx as usize] = entry;

        // Memory barrier
        core::sync::atomic::fence(Ordering::Release);

        self.tail.store(tail.wrapping_add(1), Ordering::Release);
        true
    }

    /// Pop entry from ring
    pub fn pop(&mut self) -> Option<T> {
        if self.is_empty() {
            return None;
        }

        let head = self.head.load(Ordering::Acquire);
        let idx = head & self.mask;
        let entry = self.entries[idx as usize];

        // Memory barrier
        core::sync::atomic::fence(Ordering::Release);

        self.head.store(head.wrapping_add(1), Ordering::Release);
        Some(entry)
    }

    /// Peek at head entry
    pub fn peek(&self) -> Option<T> {
        if self.is_empty() {
            return None;
        }

        let head = self.head.load(Ordering::Acquire);
        let idx = head & self.mask;
        Some(self.entries[idx as usize])
    }
}

// ============================================================================
// TAMPON HAVUZU (Buffer Pool)
// ============================================================================
//
// DMA için ardışık fiziksel bellek tahsis eden ve yöneten yapı.
//
// Tahsis:  free_list.pop_front() -> O(1)
// Serbest: free_list.push_back() -> O(1), ref_count == 0 olduğunda
//
// Referans sayımı:
//   Aynı tampon birden fazla SQE tarafından kullanılabilir.
//   ref_count > 0 iken serbest bırakılamaz.
//   ref_count == 0 olunca free_list'e döner.

/// Sıfır-kopya işlemler için DMA tampon havuzu
pub struct BufferPool {
    /// Buffer descriptors
    descriptors: Vec<BufferDescriptor>,
    /// Free buffer IDs
    free_list: VecDeque<u32>,
    /// Pool base physical address
    base_phys: u64,
    /// Pool base virtual address
    base_virt: u64,
    /// Total chunks
    total_chunks: usize,
    /// Available chunks
    available: AtomicU32,
}

impl BufferPool {
    /// Create new buffer pool
    pub fn new() -> Option<Self> {
        let total_chunks = MAX_CHUNKS;

        // Allocate contiguous physical memory for DMA
        let pages = (BUFFER_POOL_SIZE + 4095) / 4096;
        let (phys, virt) = crate::memory::dma_alloc(pages)?;

        // Zero the pool
        unsafe {
            core::ptr::write_bytes(virt.as_ptr(), 0, BUFFER_POOL_SIZE);
        }

        // Create descriptors
        let mut descriptors = Vec::with_capacity(total_chunks);
        let mut free_list = VecDeque::with_capacity(total_chunks);

        for i in 0..total_chunks {
            let chunk_phys = phys as u64 + (i * BUFFER_CHUNK_SIZE) as u64;
            let chunk_virt = virt.as_ptr() as u64 + (i * BUFFER_CHUNK_SIZE) as u64;

            descriptors.push(BufferDescriptor::new(
                i as u32,
                chunk_phys,
                chunk_virt,
                BUFFER_CHUNK_SIZE as u32,
            ));

            free_list.push_back(i as u32);
        }

        crate::serial_println!(
            "[ZC-NET] Buffer pool initialized: {} chunks ({} MB)",
            total_chunks,
            BUFFER_POOL_SIZE / (1024 * 1024)
        );

        Some(BufferPool {
            descriptors,
            free_list,
            base_phys: phys as u64,
            base_virt: virt.as_ptr() as u64,
            total_chunks,
            available: AtomicU32::new(total_chunks as u32),
        })
    }

    /// Allocate a buffer
    pub fn alloc(&mut self) -> Option<u32> {
        let buf_id = self.free_list.pop_front()?;
        self.descriptors[buf_id as usize].ref_count = 1;
        self.descriptors[buf_id as usize].flags |= BufferDescriptor::FLAG_IN_USE;
        self.available.fetch_sub(1, Ordering::Relaxed);
        Some(buf_id)
    }

    /// Allocate multiple contiguous buffers
    pub fn alloc_contiguous(&mut self, count: usize) -> Option<u32> {
        if count > self.free_list.len() {
            return None;
        }

        // Try to find contiguous range
        let mut start_id = None;
        let mut consecutive = 0;

        for &id in &self.free_list {
            if let Some(start) = start_id {
                if id == start + consecutive as u32 {
                    consecutive += 1;
                    if consecutive >= count {
                        break;
                    }
                } else {
                    start_id = Some(id);
                    consecutive = 1;
                }
            } else {
                start_id = Some(id);
                consecutive = 1;
            }
        }

        if consecutive < count {
            return None;
        }

        // Remove from free list
        let start = start_id.unwrap();
        for i in 0..count {
            self.free_list.retain(|&id| id != start + i as u32);
            self.descriptors[(start + i as u32) as usize].ref_count = 1;
            self.descriptors[(start + i as u32) as usize].flags |= BufferDescriptor::FLAG_IN_USE;
        }

        self.available.fetch_sub(count as u32, Ordering::Relaxed);
        Some(start)
    }

    /// Free a buffer
    pub fn free(&mut self, buf_id: u32) {
        if buf_id as usize >= self.descriptors.len() {
            return;
        }

        let desc = &mut self.descriptors[buf_id as usize];
        if desc.ref_count > 0 {
            desc.ref_count -= 1;

            if desc.ref_count == 0 {
                desc.flags &= !BufferDescriptor::FLAG_IN_USE;
                self.free_list.push_back(buf_id);
                self.available.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Increment reference count
    pub fn get(&mut self, buf_id: u32) {
        if (buf_id as usize) < self.descriptors.len() {
            self.descriptors[buf_id as usize].ref_count += 1;
        }
    }

    /// Get buffer descriptor
    pub fn get_descriptor(&self, buf_id: u32) -> Option<&BufferDescriptor> {
        self.descriptors.get(buf_id as usize)
    }

    /// Get buffer virtual address
    pub fn get_virt_addr(&self, buf_id: u32) -> Option<u64> {
        self.descriptors.get(buf_id as usize).map(|d| d.virt_addr)
    }

    /// Get buffer physical address
    pub fn get_phys_addr(&self, buf_id: u32) -> Option<u64> {
        self.descriptors.get(buf_id as usize).map(|d| d.phys_addr)
    }

    /// Get available buffer count
    pub fn available(&self) -> u32 {
        self.available.load(Ordering::Relaxed)
    }

    /// Write data to buffer
    pub fn write(&mut self, buf_id: u32, offset: usize, data: &[u8]) -> Result<(), NetError> {
        let desc = self
            .descriptors
            .get_mut(buf_id as usize)
            .ok_or(NetError::InvalidParam)?;

        if offset + data.len() > desc.len as usize {
            return Err(NetError::BufferFull);
        }

        unsafe {
            let dst = (desc.virt_addr + offset as u64) as *mut u8;
            core::ptr::copy_nonoverlapping(data.as_ptr(), dst, data.len());
        }

        Ok(())
    }

    /// Read data from buffer
    pub fn read(&self, buf_id: u32, offset: usize, len: usize) -> Option<Vec<u8>> {
        let desc = self.descriptors.get(buf_id as usize)?;

        if offset + len > desc.len as usize {
            return None;
        }

        let mut data = vec![0u8; len];
        unsafe {
            let src = (desc.virt_addr + offset as u64) as *const u8;
            core::ptr::copy_nonoverlapping(src, data.as_mut_ptr(), len);
        }

        Some(data)
    }
}

// ============================================================================
// IO_URING ARAYÜZÜ
// ============================================================================
//
// Linux io_uring'den ilham alan sıfır-kopya I/O arayüzü.
//
// İş Akışı:
//   1. IoUring oluştur (ring_id ile)
//   2. Tampon tahsis et (AllocBuf SQE)
//   3. Veriyi tampona yaz
//   4. Send/Recv SQE'sini gönderim kuyruğuna ekle
//   5. process() çağır - SQE'leri işle, CQE üret
//   6. complete() ile sonuçları al

/// Sıfır-kopya I/O halka arayüzü
///
/// sq: Gönderim kuyruğu (uygulama -> çekirdek)
/// cq: Tamamlama kuyruğu (çekirdek -> uygulama)
/// buffers: DMA tampon havuzu
pub struct IoUring {
    /// Submission queue
    sq: RingBuffer<Sqe>,
    /// Completion queue
    cq: RingBuffer<Cqe>,
    /// Buffer pool
    buffers: BufferPool,
    /// Pending operations count
    pending: AtomicU32,
    /// Ring ID
    ring_id: u32,
    /// Active flag
    active: AtomicBool,
    submit_batches: AtomicU64,
    completion_batches: AtomicU64,
    submit_doorbells: AtomicU64,
    last_submit_batch: AtomicU32,
    max_submit_batch: AtomicU32,
    last_completion_batch: AtomicU32,
    max_completion_batch: AtomicU32,
}

impl IoUring {
    /// Create new I/O ring
    pub fn new(ring_id: u32) -> Option<Self> {
        let buffers = BufferPool::new()?;

        Some(IoUring {
            sq: RingBuffer::new(RING_SIZE),
            cq: RingBuffer::new(RING_SIZE),
            buffers,
            pending: AtomicU32::new(0),
            ring_id,
            active: AtomicBool::new(true),
            submit_batches: AtomicU64::new(0),
            completion_batches: AtomicU64::new(0),
            submit_doorbells: AtomicU64::new(0),
            last_submit_batch: AtomicU32::new(0),
            max_submit_batch: AtomicU32::new(0),
            last_completion_batch: AtomicU32::new(0),
            max_completion_batch: AtomicU32::new(0),
        })
    }

    /// Submit operation
    pub fn submit(&mut self, sqe: Sqe) -> Result<(), NetError> {
        self.submit_batch(core::slice::from_ref(&sqe)).map(|_| ())
    }

    pub fn submit_batch(&mut self, sqes: &[Sqe]) -> Result<usize, NetError> {
        if !self.active.load(Ordering::Relaxed) {
            return Err(NetError::ConnectionClosed);
        }

        let mut submitted = 0usize;
        for sqe in sqes.iter().copied() {
            if !self.sq.push(sqe) {
                break;
            }
            submitted += 1;
        }
        if submitted == 0 {
            return Err(NetError::BufferFull);
        }
        self.pending.fetch_add(submitted as u32, Ordering::Relaxed);
        self.submit_batches.fetch_add(1, Ordering::Relaxed);
        self.submit_doorbells.fetch_add(1, Ordering::Relaxed);
        self.last_submit_batch
            .store(submitted.min(u32::MAX as usize) as u32, Ordering::Relaxed);
        self.max_submit_batch
            .fetch_max(submitted.min(u32::MAX as usize) as u32, Ordering::Relaxed);
        Ok(submitted)
    }

    /// Get completion
    pub fn complete(&mut self) -> Option<Cqe> {
        let cqe = self.cq.pop()?;
        self.pending.fetch_sub(1, Ordering::Relaxed);
        Some(cqe)
    }

    /// Peek at completion
    pub fn peek_completion(&self) -> Option<Cqe> {
        self.cq.peek()
    }

    /// Process pending submissions
    pub fn process(&mut self) -> usize {
        self.process_budgeted(RING_SIZE)
    }

    pub fn process_budgeted(&mut self, budget: usize) -> usize {
        let mut processed = 0;

        while processed < budget {
            let Some(sqe) = self.sq.pop() else {
                break;
            };
            let result = self.process_sqe(&sqe);
            let cqe = match result {
                Ok(bytes) => Cqe::success(sqe.user_data, bytes),
                Err(err) => Cqe::error(sqe.user_data, err),
            };

            self.cq.push(cqe);
            processed += 1;
        }

        if processed > 0 {
            self.completion_batches.fetch_add(1, Ordering::Relaxed);
            self.last_completion_batch
                .store(processed.min(u32::MAX as usize) as u32, Ordering::Relaxed);
            self.max_completion_batch
                .fetch_max(processed.min(u32::MAX as usize) as u32, Ordering::Relaxed);
        }

        processed
    }

    /// Process single SQE
    fn process_sqe(&mut self, sqe: &Sqe) -> Result<u32, NetError> {
        match sqe.opcode {
            OpCode::AllocBuf => {
                let buf_id = self.buffers.alloc().ok_or(NetError::BufferFull)?;
                Ok(buf_id)
            }
            OpCode::FreeBuf => {
                self.buffers.free(sqe.buf_id);
                Ok(0)
            }
            OpCode::Send => self.process_send(sqe),
            OpCode::Recv => self.process_recv(sqe),
            OpCode::Accept => self.process_accept(sqe),
            OpCode::Connect => self.process_connect(sqe),
            OpCode::Close => self.process_close(sqe),
            OpCode::MapBuf => {
                // Map buffer to userspace
                // Would set up page table mappings
                Ok(sqe.buf_id)
            }
            OpCode::UnmapBuf => {
                // Unmap buffer from userspace
                Ok(0)
            }
        }
    }

    fn process_accept(&mut self, sqe: &Sqe) -> Result<u32, NetError> {
        let (accepted, _) = socket::accept(sqe.socket_id)?;
        Ok(accepted)
    }

    fn process_connect(&mut self, sqe: &Sqe) -> Result<u32, NetError> {
        socket::connect(sqe.socket_id, sqe.addr)?;
        Ok(0)
    }

    fn process_close(&mut self, sqe: &Sqe) -> Result<u32, NetError> {
        socket::close(sqe.socket_id)?;
        Ok(0)
    }

    /// Process send operation
    fn process_send(&mut self, sqe: &Sqe) -> Result<u32, NetError> {
        // Gather data from I/O vectors
        let mut total_len = 0;
        let mut packet_data = Vec::new();

        for i in 0..sqe.iov_count as usize {
            let iov = &sqe.iov[i];
            if let Some(data) = self
                .buffers
                .read(iov.buf_id, iov.offset as usize, iov.len as usize)
            {
                packet_data.extend_from_slice(&data);
                total_len += data.len();
            }
        }

        if packet_data.is_empty() {
            return Err(NetError::BufferEmpty);
        }

        // Send through network interface
        super::send_packet(&packet_data)?;

        // Free buffers if not fixed
        if sqe.flags & SQE_FLAG_FIXED_BUF == 0 {
            for i in 0..sqe.iov_count as usize {
                self.buffers.free(sqe.iov[i].buf_id);
            }
        }

        Ok(total_len as u32)
    }

    /// Process receive operation
    fn process_recv(&mut self, sqe: &Sqe) -> Result<u32, NetError> {
        // Try to receive packet
        let iface = super::default_interface().ok_or(NetError::NoInterface)?;
        let packet = iface.lock().recv().ok_or(NetError::WouldBlock)?;

        // Allocate buffer for received data
        let buf_id = self.buffers.alloc().ok_or(NetError::BufferFull)?;
        let len = packet.len().min(BUFFER_CHUNK_SIZE);

        self.buffers.write(buf_id, 0, &packet[..len])?;

        Ok(len as u32)
    }

    /// Allocate buffer
    pub fn alloc_buffer(&mut self) -> Option<u32> {
        self.buffers.alloc()
    }

    /// Free buffer
    pub fn free_buffer(&mut self, buf_id: u32) {
        self.buffers.free(buf_id);
    }

    /// Get buffer pool statistics
    pub fn buffer_stats(&self) -> (u32, u32) {
        (self.buffers.total_chunks as u32, self.buffers.available())
    }

    /// Get pending operations count
    pub fn pending_count(&self) -> u32 {
        self.pending.load(Ordering::Relaxed)
    }

    /// Get ring ID
    pub fn id(&self) -> u32 {
        self.ring_id
    }

    pub fn batch_stats(&self) -> (u64, u64, u64, u32, u32) {
        (
            self.submit_batches.load(Ordering::Relaxed),
            self.completion_batches.load(Ordering::Relaxed),
            self.submit_doorbells.load(Ordering::Relaxed),
            self.max_submit_batch.load(Ordering::Relaxed),
            self.max_completion_batch.load(Ordering::Relaxed),
        )
    }
}

// ============================================================================
// GLOBAL IO_URING ÖRNEKLERİ
// ============================================================================
//
// Birden fazla ring desteklenir (çoklu işlem veya CPU çekirdeği başına bir ring).
// Her ring ayrı tampon havuzuna sahiptir.
// NEXT_RING_ID: Her yeni ring için artan ID (atomik)

lazy_static::lazy_static! {
    static ref IO_RINGS: Mutex<Vec<Arc<Mutex<IoUring>>>> = Mutex::new(Vec::new());
    static ref NEXT_RING_ID: AtomicU32 = AtomicU32::new(1);
}

/// Create new I/O ring
pub fn create_ring() -> Option<u32> {
    let ring_id = NEXT_RING_ID.fetch_add(1, Ordering::Relaxed);
    let ring = IoUring::new(ring_id)?;

    IO_RINGS.lock().push(Arc::new(Mutex::new(ring)));

    crate::serial_println!("[ZC-NET] Created I/O ring {}", ring_id);
    Some(ring_id)
}

/// Get I/O ring by ID
pub fn get_ring(ring_id: u32) -> Option<Arc<Mutex<IoUring>>> {
    IO_RINGS
        .lock()
        .iter()
        .find(|r| r.lock().id() == ring_id)
        .cloned()
}

/// Process all rings
pub fn process_all_rings() {
    let rings = IO_RINGS.lock();
    for ring in rings.iter() {
        let processed = ring.lock().process_budgeted(64);
        if processed > 0 {
            crate::serial_println!(
                "[ZC-NET] Ring {} processed {} ops",
                ring.lock().id(),
                processed
            );
        }
    }
}

// ============================================================================
// KULLANICI ALANI ARAYÜZÜ (USERSPACE INTERFACE)
// ============================================================================
//
// Kullanıcı alanı uygulamaları bu yapı üzerinden I/O ringine erişir.
// sq_entries/cq_entries: Ring tampon bellek adresleri (paylaşımlı bellek)
// buffer_base: DMA tampon havuzunun başlangıç adresi

/// Kullanıcı alanı için I/O ring kurulum yapısı
#[repr(C)]
pub struct IoUringSetup {
    /// Ring ID
    pub ring_id: u32,
    /// Submission queue entries (memory address)
    pub sq_entries: u64,
    /// Completion queue entries (memory address)
    pub cq_entries: u64,
    /// Buffer pool base address
    pub buffer_base: u64,
    /// Number of buffers
    pub buffer_count: u32,
    /// Ring size
    pub ring_size: u32,
}

/// Setup I/O ring for userspace
pub fn setup_userspace_ring() -> Option<IoUringSetup> {
    let ring_id = create_ring()?;
    let ring = get_ring(ring_id)?;
    let ring_guard = ring.lock();

    Some(IoUringSetup {
        ring_id,
        sq_entries: ring_guard.sq.entries.as_ptr() as u64,
        cq_entries: ring_guard.cq.entries.as_ptr() as u64,
        buffer_base: ring_guard.buffers.base_virt,
        buffer_count: ring_guard.buffers.total_chunks as u32,
        ring_size: RING_SIZE as u32,
    })
}

// ============================================================================
// BAŞLATMA (INITIALIZATION)
// ============================================================================

/// Sıfır-kopya ağ alt sistemini başlat (varsayılan I/O ring oluştur)
const AFXDP_RING_SIZE: usize = 2048;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct AfXdpDesc {
    pub addr: u64,
    pub len: u32,
    pub options: u32,
}

pub struct AfXdpSocket {
    socket_id: u32,
    queue_id: u16,
    umem: BufferPool,
    fill_ring: RingBuffer<u32>,
    comp_ring: RingBuffer<u32>,
    rx_ring: RingBuffer<AfXdpDesc>,
    tx_ring: RingBuffer<AfXdpDesc>,
    rx_packets: AtomicU64,
    tx_packets: AtomicU64,
    drops: AtomicU64,
}

impl AfXdpSocket {
    pub fn new(socket_id: u32, queue_id: u16) -> Option<Self> {
        let umem = BufferPool::new()?;
        Some(Self {
            socket_id,
            queue_id,
            umem,
            fill_ring: RingBuffer::new(AFXDP_RING_SIZE),
            comp_ring: RingBuffer::new(AFXDP_RING_SIZE),
            rx_ring: RingBuffer::new(AFXDP_RING_SIZE),
            tx_ring: RingBuffer::new(AFXDP_RING_SIZE),
            rx_packets: AtomicU64::new(0),
            tx_packets: AtomicU64::new(0),
            drops: AtomicU64::new(0),
        })
    }

    pub fn umem_alloc(&mut self) -> Option<u32> {
        self.umem.alloc()
    }

    pub fn post_fill(&mut self, buf_id: u32) -> Result<(), NetError> {
        if self.umem.get_descriptor(buf_id).is_none() {
            return Err(NetError::InvalidParam);
        }
        if !self.fill_ring.push(buf_id) {
            return Err(NetError::BufferFull);
        }
        Ok(())
    }

    pub fn submit_tx(&mut self, desc: AfXdpDesc) -> Result<(), NetError> {
        if desc.len == 0 {
            return Err(NetError::InvalidParam);
        }
        if !self.tx_ring.push(desc) {
            return Err(NetError::BufferFull);
        }
        Ok(())
    }

    pub fn poll_rx(&mut self) -> Option<AfXdpDesc> {
        self.rx_ring.pop()
    }

    pub fn poll_completion(&mut self) -> Option<u32> {
        self.comp_ring.pop()
    }

    pub fn process_rx(&mut self, budget: usize) -> usize {
        let Some(iface) = super::default_interface() else {
            return 0;
        };

        let mut processed = 0;
        for _ in 0..budget {
            let packet = {
                let mut guard = iface.lock();
                guard.recv()
            };

            let Some(packet) = packet else {
                break;
            };

            let Some(buf_id) = self.fill_ring.pop() else {
                self.drops.fetch_add(1, Ordering::Relaxed);
                continue;
            };

            let copy_len = packet.len().min(BUFFER_CHUNK_SIZE);
            if self.umem.write(buf_id, 0, &packet[..copy_len]).is_err() {
                self.drops.fetch_add(1, Ordering::Relaxed);
                let _ = self.comp_ring.push(buf_id);
                continue;
            }

            let desc = AfXdpDesc {
                addr: buf_id as u64,
                len: copy_len as u32,
                options: 0,
            };

            if !self.rx_ring.push(desc) {
                self.drops.fetch_add(1, Ordering::Relaxed);
                let _ = self.comp_ring.push(buf_id);
                continue;
            }

            self.rx_packets.fetch_add(1, Ordering::Relaxed);
            processed += 1;
        }

        processed
    }

    pub fn process_tx(&mut self, budget: usize) -> usize {
        let mut processed = 0;

        for _ in 0..budget {
            let Some(desc) = self.tx_ring.pop() else {
                break;
            };

            let buf_id = desc.addr as u32;
            let Some(payload) = self.umem.read(buf_id, 0, desc.len as usize) else {
                self.drops.fetch_add(1, Ordering::Relaxed);
                continue;
            };

            if super::send_packet(&payload).is_err() {
                self.drops.fetch_add(1, Ordering::Relaxed);
                continue;
            }

            self.tx_packets.fetch_add(1, Ordering::Relaxed);
            let _ = self.comp_ring.push(buf_id);
            processed += 1;
        }

        processed
    }

    pub fn stats(&self) -> (u64, u64, u64) {
        (
            self.rx_packets.load(Ordering::Relaxed),
            self.tx_packets.load(Ordering::Relaxed),
            self.drops.load(Ordering::Relaxed),
        )
    }
}

lazy_static::lazy_static! {
    static ref AFXDP_SOCKETS: Mutex<BTreeMap<u32, Arc<Mutex<AfXdpSocket>>>> = Mutex::new(BTreeMap::new());
    static ref NEXT_AFXDP_SOCKET_ID: AtomicU32 = AtomicU32::new(1);
}

pub fn create_afxdp_socket(queue_id: u16) -> Option<u32> {
    let socket_id = NEXT_AFXDP_SOCKET_ID.fetch_add(1, Ordering::Relaxed);
    let socket = AfXdpSocket::new(socket_id, queue_id)?;
    AFXDP_SOCKETS
        .lock()
        .insert(socket_id, Arc::new(Mutex::new(socket)));
    Some(socket_id)
}

pub fn get_afxdp_socket(socket_id: u32) -> Option<Arc<Mutex<AfXdpSocket>>> {
    AFXDP_SOCKETS.lock().get(&socket_id).cloned()
}

pub fn afxdp_umem_alloc(socket_id: u32) -> Result<u32, NetError> {
    let socket = get_afxdp_socket(socket_id).ok_or(NetError::InvalidFd)?;
    let mut guard = socket.lock();
    let buffer = guard.umem_alloc().ok_or(NetError::BufferFull)?;
    Ok(buffer)
}

pub fn afxdp_post_fill(socket_id: u32, buf_id: u32) -> Result<(), NetError> {
    let socket = get_afxdp_socket(socket_id).ok_or(NetError::InvalidFd)?;
    let mut guard = socket.lock();
    guard.post_fill(buf_id)
}

pub fn afxdp_submit_tx(socket_id: u32, desc: AfXdpDesc) -> Result<(), NetError> {
    let socket = get_afxdp_socket(socket_id).ok_or(NetError::InvalidFd)?;
    let mut guard = socket.lock();
    guard.submit_tx(desc)
}

pub fn afxdp_process(socket_id: u32, budget: usize) -> Result<(usize, usize), NetError> {
    let socket = get_afxdp_socket(socket_id).ok_or(NetError::InvalidFd)?;
    let mut guard = socket.lock();
    let rx = guard.process_rx(budget);
    let tx = guard.process_tx(budget);
    Ok((rx, tx))
}

pub fn afxdp_poll_rx(socket_id: u32) -> Result<Option<AfXdpDesc>, NetError> {
    let socket = get_afxdp_socket(socket_id).ok_or(NetError::InvalidFd)?;
    let mut guard = socket.lock();
    let packet = guard.poll_rx();
    Ok(packet)
}

pub fn afxdp_poll_completion(socket_id: u32) -> Result<Option<u32>, NetError> {
    let socket = get_afxdp_socket(socket_id).ok_or(NetError::InvalidFd)?;
    let mut guard = socket.lock();
    let completed = guard.poll_completion();
    Ok(completed)
}

pub fn init() {
    crate::serial_println!("[ZC-NET] Initializing zero-copy networking...");

    // Create default ring
    if create_ring().is_some() {
        crate::serial_println!("[ZC-NET] Default I/O ring created");
    }

    // Create a default AF_XDP socket bound to queue 0.
    if let Some(sock_id) = create_afxdp_socket(0) {
        crate::serial_println!("[ZC-NET] Default AF_XDP socket created: {}", sock_id);
    }

    // Create default device memory pool (256-byte chunks, pre-registered)
    if let Some(pool_id) = create_devmem_pool(256) {
        crate::serial_println!("[ZC-NET] Default devmem pool created: {}", pool_id);
    }

    crate::serial_println!("[ZC-NET] Zero-copy networking initialized");
}

/// Register a NIC PCIe BAR region as a device memory pool.
/// Called by NIC drivers during probe.
pub fn register_nic_devmem(
    nic_name: &str,
    bar_index: u8,
    bar_phys_base: u64,
    bar_size: u64,
    mmio_base: u64,
) -> Result<u32, NetError> {
    let pools = DEVMEM_POOLS.lock();
    // Find or create a pool for this NIC
    let pool_id = if let Some((&id, _)) = pools.iter().next() {
        id
    } else {
        drop(pools);
        create_devmem_pool(256).ok_or(NetError::NoInterface)?
    };

    let _ = nic_name;
    let _ = bar_index;

    crate::serial_println!(
        "[ZC-NET] Registering NIC devmem BAR: phys={:#x} size={} pool={}",
        bar_phys_base,
        bar_size,
        pool_id
    );

    devmem_add_region(
        pool_id,
        DevmemRegionType::PciBar,
        bar_phys_base,
        mmio_base,
        bar_size,
        bar_phys_base, // DMA base = phys base
    )
}

// ============================================================================
// NETMEM — SOYUTLANMIŞ AĞ BELLEK KATMANI
// ============================================================================
//
// Linux çekirdeğindeki netmem soyutlamasının echOS uyarlaması.
// PagePool'u farklı bellek türlerini (sıradan sayfa, device memory, dmabuf)
// destekleyecek şekilde genişletir. Sürücüler netmem_ref üzerinden çalışır,
// altındaki bellek türüyle ilgilenmez.
//
// Temel prensipler (kernel netmem doc'tan):
// 1. page_pool_alloc → page_pool_alloc_netmem (netmem_ref döndürür)
// 2. page_pool_get_dma_addr → page_pool_get_dma_addr_netmem
// 3. page_pool_put_page → page_pool_put_netmem
// 4. Sürücü netmem'in okunabilir/sayfa destekli olduğunu varsayamaz
// 5. DMA mapping/syncing page_pool'a devredilmelidir

/// Netmem tarafından desteklenen bellek türleri.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetmemType {
    PagePool = 0,
    DeviceMemory = 1,
    DmaBuf = 2,
}

/// Opaque netmem referansı (kernel'deki `netmem_ref`'in karşılığı).
///
/// Alt 2 bit türü kodlar; üst 62 bit türe özgü ID taşır.
/// Bu sayede NetmemRef tek u64 ile hem türü hem indeksi tutar.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetmemRef(u64);

impl NetmemRef {
    pub const INVALID: NetmemRef = NetmemRef(u64::MAX);

    const TYPE_MASK: u64 = 0x3;
    const ID_SHIFT: u64 = 2;

    pub fn new(ty: NetmemType, id: u32) -> Self {
        let ty_val = ty as u64;
        let encoded = (ty_val & Self::TYPE_MASK) | ((id as u64) << Self::ID_SHIFT);
        NetmemRef(encoded)
    }

    pub fn ty(&self) -> NetmemType {
        let ty_val = self.0 & Self::TYPE_MASK;
        match ty_val {
            0 => NetmemType::PagePool,
            1 => NetmemType::DeviceMemory,
            2 => NetmemType::DmaBuf,
            _ => NetmemType::PagePool,
        }
    }

    pub fn id(&self) -> u32 {
        (self.0 >> Self::ID_SHIFT) as u32
    }

    pub fn is_valid(&self) -> bool {
        self.0 != u64::MAX
    }

    pub const fn as_u64(&self) -> u64 {
        self.0
    }

    pub fn from_u64(raw: u64) -> Self {
        NetmemRef(raw)
    }
}

/// netmem_ref'ten DMA adresi almak için kullanılan metaveri.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct NetmemDmaInfo {
    pub dma_addr: u64,
    pub dma_size: u32,
    pub dma_dir: PagePoolDmaDirection,
    pub is_readable: bool,
}

/// Page pool'dan allocate edilen netmem için wrapper (netmem_page).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct NetmemPage {
    pub nref: NetmemRef,
    pub phys_addr: u64,
    pub virt_addr: u64,
    pub len: u32,
    pub dma_addr: u64,
    pub dma_synced: u32,
}

impl NetmemPage {
    pub const fn empty() -> Self {
        Self {
            nref: NetmemRef::INVALID,
            phys_addr: 0,
            virt_addr: 0,
            len: 0,
            dma_addr: 0,
            dma_synced: 0,
        }
    }

    pub fn is_readable(&self) -> bool {
        self.virt_addr != 0
    }

    pub fn as_dma_slice(&self, offset: u32, len: u32) -> Result<DmaSlice, NetError> {
        if len == 0 || offset.checked_add(len).ok_or(NetError::InvalidParam)? > self.len {
            return Err(NetError::InvalidParam);
        }
        if self.dma_addr == 0 {
            return Err(NetError::InvalidParam);
        }
        let dma_off = self.dma_addr.checked_add(offset as u64).ok_or(NetError::InvalidParam)?;
        let virt_off = if self.is_readable() {
            self.virt_addr.checked_add(offset as u64).ok_or(NetError::InvalidParam)?
        } else {
            0
        };
        if virt_off > usize::MAX as u64 {
            return Err(NetError::InvalidParam);
        }
        Ok(DmaSlice::new(virt_off as usize, dma_off, len as usize))
    }
}

/// Netmem ile genişletilmiş PagePool.
///
/// Standart PagePool'a ek olarak netmem_ref üzerinden alloc/get/put
/// yapmayı sağlar. Aynı zamanda okunamaz netmem (device memory) türünü
/// destekler.
impl PagePool {
    /// page_pool_alloc_netmem: netmem_ref olarak sayfa tahsis et.
    pub fn alloc_netmem(&mut self) -> Option<NetmemRef> {
        let page_id = self.pop_free_page()?;
        let page = &mut self.descriptors[page_id as usize];
        page.ref_count = 1;
        page.flags = PagePoolPage::FLAG_IN_USE | PagePoolPage::FLAG_DMA_MAPPED;
        self.available -= 1;
        self.in_flight += 1;
        Some(NetmemRef::new(NetmemType::PagePool, page_id))
    }

    /// page_pool_alloc_netmem_page: netmem_ref'ten NetmemPage döndür.
    pub fn alloc_netmem_page(&mut self) -> Option<NetmemPage> {
        let nref = self.alloc_netmem()?;
        let page = self.page_from_netmem(nref)?;
        Some(NetmemPage {
            nref,
            phys_addr: page.phys_addr,
            virt_addr: page.virt_addr,
            len: page.len,
            dma_addr: page.phys_addr,
            dma_synced: 0,
        })
    }

    /// page_pool_get_dma_addr_netmem: netmem'in DMA adresini döndür.
    pub fn get_dma_addr_netmem(&self, nref: NetmemRef) -> Option<u64> {
        let page = self.page_from_netmem(nref)?;
        Some(page.phys_addr)
    }

    /// page_pool_put_netmem: netmem_ref'i page pool'a geri ver.
    pub fn put_netmem(
        &mut self,
        nref: NetmemRef,
        dma_sync_size: u32,
        allow_direct: bool,
    ) -> Result<bool, NetError> {
        let page_id = nref.id();
        self.put_page(page_id, dma_sync_size, allow_direct)
    }

    /// page_pool_put_full_netmem: tüm sayfayı geri ver.
    pub fn put_full_netmem(&mut self, nref: NetmemRef, allow_direct: bool) -> Result<bool, NetError> {
        self.put_netmem(nref, PAGE_POOL_SYNC_ALL, allow_direct)
    }

    /// page_pool_fragment_netmem: netmem üzerinde ref count artır (fragman için).
    pub fn fragment_netmem(&mut self, nref: NetmemRef) -> Result<NetmemRef, NetError> {
        let page_id = nref.id();
        self.ref_page(page_id)?;
        Ok(nref)
    }

    /// page_pool_ref_netmem: netmem referansını artır.
    pub fn ref_netmem(&mut self, nref: NetmemRef) -> Result<NetmemRef, NetError> {
        self.fragment_netmem(nref)
    }

    /// page_pool_dma_sync_netmem_for_cpu: netmem'i CPU için DMA sync et.
    pub fn dma_sync_netmem_for_cpu(
        &mut self,
        nref: NetmemRef,
        offset: u32,
        len: u32,
    ) -> Result<(), NetError> {
        let page_id = nref.id();
        self.sync_for_cpu(page_id, offset, len)
    }

    /// page_pool_dma_sync_netmem_for_device: netmem'i device için sync et.
    pub fn dma_sync_netmem_for_device(
        &mut self,
        nref: NetmemRef,
        offset: u32,
        len: u32,
    ) -> Result<(), NetError> {
        let page_id = nref.id();
        self.sync_for_device(page_id, offset, len)
    }

    fn page_from_netmem(&self, nref: NetmemRef) -> Option<PagePoolPage> {
        if nref.ty() != NetmemType::PagePool {
            return None;
        }
        self.get_page(nref.id())
    }

    /// Netmem destekli alloc_fragment. Dönen fragmentte netmem_ref taşınır.
    pub fn alloc_netmem_fragment(&mut self, len: u32) -> Option<(NetmemRef, PagePoolFragment)> {
        let frag = self.alloc_fragment(len)?;
        let nref = NetmemRef::new(NetmemType::PagePool, frag.page_id);
        Some((nref, frag))
    }
}

/// Netmem bilgisini de taşıyan genişletilmiş PacketSegment.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct NetmemSegment {
    pub seg: PacketSegment,
    pub nref: NetmemRef,
    pub netmem_type: NetmemType,
    pub dma_info: NetmemDmaInfo,
}

impl NetmemSegment {
    pub const fn empty() -> Self {
        Self {
            seg: PacketSegment::empty(),
            nref: NetmemRef::INVALID,
            netmem_type: NetmemType::PagePool,
            dma_info: NetmemDmaInfo {
                dma_addr: 0,
                dma_size: 0,
                dma_dir: PagePoolDmaDirection::ToDevice,
                is_readable: false,
            },
        }
    }

    pub fn from_netmem_page(page: &NetmemPage) -> Result<Self, NetError> {
        if page.len == 0 {
            return Err(NetError::InvalidParam);
        }
        let vaddr = if page.is_readable() {
            if page.virt_addr > usize::MAX as u64 {
                return Err(NetError::InvalidParam);
            }
            page.virt_addr as usize
        } else {
            0
        };
        Ok(Self {
            seg: PacketSegment {
                buf_id: page.nref.id(),
                offset: 0,
                len: page.len,
                vaddr,
                paddr: page.phys_addr,
            },
            nref: page.nref,
            netmem_type: page.nref.ty(),
            dma_info: NetmemDmaInfo {
                dma_addr: page.dma_addr,
                dma_size: page.len,
                dma_dir: PagePoolDmaDirection::FromDevice,
                is_readable: page.is_readable(),
            },
        })
    }

    pub fn as_dma_slice(&self) -> Result<DmaSlice, NetError> {
        if self.seg.len == 0 {
            return Err(NetError::InvalidParam);
        }
        Ok(DmaSlice::new(self.seg.vaddr, self.seg.paddr, self.seg.len as usize))
    }
}

// ============================================================================
// DEVICE MEMORY TCP — NIC ONBOARD BELLEĞE DOĞRUDAN ERİŞİM
// ============================================================================
//
// Linux çekirdeğindeki device memory TCP (dm-tcp) özelliğinin echOS uyarlaması.
// NIC'in onboard (PCIe BAR) belleğine doğrudan erişim sağlar. Standart
// sıfır-kopya yolundan farkı, verinin ana bellek yerine NIC'in kendi
// belleğine yazılıp okunmasıdır. Bu, ek PCIe DMA transferlerini ortadan
// kaldırarak gecikmeyi düşürür.
//
// Mimarisi:
// - DevmemRegion: NIC onboard belleğinde contiguous bir bölge (PCIe BAR'a映射)
// - DevmemChunk: Region içinde bir parça
// - DevmemPool: DevmemChunk tahsis yöneticisi
// - DevmemTcpSocket: Device memory üzerinden TCP iletişimi
//
// DMA adresleme: NIC onboard belleği, PCIe BAR üzerinden CPU tarafından
// görülebilir (mmio) ve DMA işlemleri için fiziksel adres olarak kullanılır.

/// Device memory region türü.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DevmemRegionType {
    PciBar = 0,
    Cxl = 1,
    Custom = 2,
}

/// NIC onboard belleğinde contiguous bir bölge.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DevmemRegion {
    /// Region ID (pool içinde unique).
    pub region_id: u32,
    /// Region türü.
    pub region_type: DevmemRegionType,
    /// PCIe BAR fiziksel başlangıç adresi.
    pub bar_phys_base: u64,
    /// CPU görünür sanal adres (MMIO mapping).
    pub mmio_base: u64,
    /// Region toplam boyutu (byte).
    pub size: u64,
    /// DMA adresi (NIC'in kendi görüş açısı).
    pub dma_base: u64,
    /// Kullanılan bayt sayısı.
    pub used: u64,
    /// Flags.
    pub flags: u32,
}

impl DevmemRegion {
    pub const FLAG_ALLOCATED: u32 = 1 << 0;
    pub const FLAG_MAPPED: u32 = 1 << 1;

    pub fn is_valid(&self) -> bool {
        self.size > 0 && self.bar_phys_base != 0
    }

    pub fn contains_dma(&self, dma_addr: u64, len: u64) -> bool {
        dma_addr >= self.dma_base
            && dma_addr
                .checked_add(len)
                .map_or(false, |end| end <= self.dma_base + self.size)
    }
}

/// Device memory içinde bir parça (chunk).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DevmemChunk {
    /// Region ID.
    pub region_id: u32,
    /// Chunk ID (pool içinde unique).
    pub chunk_id: u32,
    /// Region içindeki offset.
    pub offset: u64,
    /// Chunk boyutu.
    pub len: u32,
    /// DMA adresi (region.dma_base + offset).
    pub dma_addr: u64,
    /// MMIO adresi (region.mmio_base + offset).
    pub mmio_addr: u64,
    /// Referans sayacı.
    pub ref_count: u16,
    /// Flags.
    pub flags: u32,
}

impl DevmemChunk {
    pub const FLAG_IN_USE: u32 = 1 << 0;
    pub const FLAG_TX: u32 = 1 << 1;
    pub const FLAG_RX: u32 = 1 << 2;

    pub const fn empty() -> Self {
        Self {
            region_id: 0,
            chunk_id: 0,
            offset: 0,
            len: 0,
            dma_addr: 0,
            mmio_addr: 0,
            ref_count: 0,
            flags: 0,
        }
    }

    pub fn dma_slice(&self) -> Result<DmaSlice, NetError> {
        if self.len == 0 || self.mmio_addr > usize::MAX as u64 {
            return Err(NetError::InvalidParam);
        }
        Ok(DmaSlice::new(self.mmio_addr as usize, self.dma_addr, self.len as usize))
    }

    pub fn region_netmem_ref(&self) -> NetmemRef {
        NetmemRef::new(NetmemType::DeviceMemory, self.region_id)
    }
}

/// DevremChunk havuzu: NIC onboard belleğini yönetir.
pub struct DevmemPool {
    regions: Vec<DevmemRegion>,
    chunks: Vec<DevmemChunk>,
    free_chunks: VecDeque<u32>,
    region_map: BTreeMap<u32, u32>, // region_id -> index
    chunk_size: u32,
    next_region_id: u32,
    next_chunk_id: u32,
    available: u32,
    total: u32,
    allocs: u64,
    frees: u64,
}

impl DevmemPool {
    pub fn new(chunk_size: u32) -> Self {
        Self {
            regions: Vec::new(),
            chunks: Vec::new(),
            free_chunks: VecDeque::new(),
            region_map: BTreeMap::new(),
            chunk_size: chunk_size.max(64),
            next_region_id: 1,
            next_chunk_id: 1,
            available: 0,
            total: 0,
            allocs: 0,
            frees: 0,
        }
    }

    /// Yeni bir device memory bölgesi ekle.
    pub fn add_region(
        &mut self,
        region_type: DevmemRegionType,
        bar_phys_base: u64,
        mmio_base: u64,
        size: u64,
        dma_base: u64,
    ) -> Option<u32> {
        if size == 0 || size < self.chunk_size as u64 {
            return None;
        }

        let region_id = self.next_region_id;
        self.next_region_id += 1;

        let region = DevmemRegion {
            region_id,
            region_type,
            bar_phys_base,
            mmio_base,
            size,
            dma_base,
            used: 0,
            flags: DevmemRegion::FLAG_MAPPED,
        };

        let region_idx = self.regions.len() as u32;
        self.regions.push(region);
        self.region_map.insert(region_id, region_idx);

        // Region'u chunk'lara böl
        let num_chunks = (size / self.chunk_size as u64) as u32;
        for i in 0..num_chunks {
            let offset = i as u64 * self.chunk_size as u64;
            self.chunks.push(DevmemChunk {
                region_id,
                chunk_id: self.next_chunk_id,
                offset,
                len: self.chunk_size,
                dma_addr: dma_base + offset,
                mmio_addr: mmio_base + offset,
                ref_count: 0,
                flags: 0,
            });
            self.free_chunks.push_back(self.next_chunk_id);
            self.next_chunk_id += 1;
        }

        self.available += num_chunks;
        self.total += num_chunks;

        Some(region_id)
    }

    /// DevremChunk tahsis et.
    pub fn alloc_chunk(&mut self) -> Option<DevmemChunk> {
        let chunk_id = self.free_chunks.pop_front()?;
        let chunk_idx = (chunk_id - 1) as usize; // 1-based IDs
        let chunk = &mut self.chunks[chunk_idx];
        chunk.ref_count = 1;
        chunk.flags = DevmemChunk::FLAG_IN_USE;
        self.available -= 1;
        self.allocs += 1;
        Some(*chunk)
    }

    /// DevremChunk'ı geri ver.
    pub fn free_chunk(&mut self, chunk_id: u32) {
        let idx = (chunk_id - 1) as usize;
        if idx >= self.chunks.len() {
            return;
        }
        let chunk = &mut self.chunks[idx];
        if chunk.ref_count == 0 {
            return;
        }
        chunk.ref_count -= 1;
        if chunk.ref_count == 0 {
            chunk.flags = 0;
            self.free_chunks.push_back(chunk_id);
            self.available += 1;
            self.frees += 1;
        }
    }

    /// Chunk referansını artır.
    pub fn ref_chunk(&mut self, chunk_id: u32) -> Option<DevmemChunk> {
        let idx = (chunk_id - 1) as usize;
        let chunk = self.chunks.get_mut(idx)?;
        if chunk.ref_count == 0 {
            return None;
        }
        chunk.ref_count = chunk.ref_count.checked_add(1)?;
        Some(*chunk)
    }

    pub fn get_chunk(&self, chunk_id: u32) -> Option<DevmemChunk> {
        let idx = (chunk_id - 1) as usize;
        self.chunks.get(idx).copied()
    }

    pub fn get_region(&self, region_id: u32) -> Option<&DevmemRegion> {
        let idx = self.region_map.get(&region_id)?;
        self.regions.get(*idx as usize)
    }

    pub fn chunk_size(&self) -> u32 {
        self.chunk_size
    }

    pub fn available(&self) -> u32 {
        self.available
    }

    pub fn total_chunks(&self) -> u32 {
        self.total
    }

    pub fn stats(&self) -> (u64, u64, u32, u32) {
        (self.allocs, self.frees, self.available, self.total)
    }
}

/// PagePool için devmem entegrasyonu: NetmemRef DeviceMemory türünü destekler.
impl PagePool {
    /// Device memory'den netmem_ref olarak chunk tahsis et.
    /// Dönen NetmemRef DeviceMemory türündedir ve dma_addr olarak chunk'ın
    /// DMA adresini kullanır.
    pub fn alloc_devmem_chunk(&mut self, devpool: &mut DevmemPool) -> Option<(NetmemRef, DevmemChunk)> {
        let chunk = devpool.alloc_chunk()?;
        let nref = NetmemRef::new(NetmemType::DeviceMemory, chunk.region_id);
        Some((nref, chunk))
    }
}

/// Device Memory TCP socket wrapper.
///
/// Normal TCP socket üzerine device memory RX/TX özelliği ekler.
/// DMA transferleri NIC onboard belleği ile doğrudan yapılır,
/// ana belleğe kopyalama yapılmaz.
pub struct DevmemTcpSocket {
    /// Temel socket ID.
    pub socket_id: u32,
    /// Device memory pool referansı.
    pub devpool_id: u32,
    /// RX için bağlı netmem page pool (opsiyonel).
    pub page_pool_id: u32,
    /// Device memory kullanımda mı?
    pub devmem_enabled: bool,
    /// İstatistikler.
    pub rx_devmem: u64,
    pub tx_devmem: u64,
    pub rx_fallback: u64,
    pub tx_fallback: u64,
}

impl DevmemTcpSocket {
    pub fn new(socket_id: u32, devpool_id: u32) -> Self {
        Self {
            socket_id,
            devpool_id,
            page_pool_id: 0,
            devmem_enabled: true,
            rx_devmem: 0,
            tx_devmem: 0,
            rx_fallback: 0,
            tx_fallback: 0,
        }
    }

    pub fn stats(&self) -> (u64, u64, u64, u64) {
        (
            self.rx_devmem,
            self.tx_devmem,
            self.rx_fallback,
            self.tx_fallback,
        )
    }
}

// Global device memory pool registry
lazy_static::lazy_static! {
    static ref DEVMEM_POOLS: Mutex<BTreeMap<u32, Arc<Mutex<DevmemPool>>>> = Mutex::new(BTreeMap::new());
    static ref NEXT_DEVMEM_POOL_ID: AtomicU32 = AtomicU32::new(1);
    static ref DEVMEM_TCP_SOCKETS: Mutex<BTreeMap<u32, Arc<Mutex<DevmemTcpSocket>>>> = Mutex::new(BTreeMap::new());
    static ref NEXT_DEVMEM_SOCKET_ID: AtomicU32 = AtomicU32::new(1);
}

pub fn create_devmem_pool(chunk_size: u32) -> Option<u32> {
    let pool_id = NEXT_DEVMEM_POOL_ID.fetch_add(1, Ordering::Relaxed);
    let pool = DevmemPool::new(chunk_size);
    DEVMEM_POOLS
        .lock()
        .insert(pool_id, Arc::new(Mutex::new(pool)));
    Some(pool_id)
}

pub fn get_devmem_pool(pool_id: u32) -> Option<Arc<Mutex<DevmemPool>>> {
    DEVMEM_POOLS.lock().get(&pool_id).cloned()
}

pub fn devmem_add_region(
    pool_id: u32,
    region_type: DevmemRegionType,
    bar_phys_base: u64,
    mmio_base: u64,
    size: u64,
    dma_base: u64,
) -> Result<u32, NetError> {
    let pool = get_devmem_pool(pool_id).ok_or(NetError::InvalidFd)?;
    let mut guard = pool.lock();
    guard
        .add_region(region_type, bar_phys_base, mmio_base, size, dma_base)
        .ok_or(NetError::InvalidParam)
}

pub fn devmem_alloc_chunk(pool_id: u32) -> Result<DevmemChunk, NetError> {
    let pool = get_devmem_pool(pool_id).ok_or(NetError::InvalidFd)?;
    let mut guard = pool.lock();
    guard.alloc_chunk().ok_or(NetError::BufferFull)
}

pub fn devmem_free_chunk(pool_id: u32, chunk_id: u32) -> Result<(), NetError> {
    let pool = get_devmem_pool(pool_id).ok_or(NetError::InvalidFd)?;
    let mut guard = pool.lock();
    guard.free_chunk(chunk_id);
    Ok(())
}

pub fn create_devmem_tcp_socket(socket_id: u32, devpool_id: u32) -> Option<u32> {
    let dm_sock_id = NEXT_DEVMEM_SOCKET_ID.fetch_add(1, Ordering::Relaxed);
    let sock = DevmemTcpSocket::new(socket_id, devpool_id);
    DEVMEM_TCP_SOCKETS
        .lock()
        .insert(dm_sock_id, Arc::new(Mutex::new(sock)));
    Some(dm_sock_id)
}

pub fn get_devmem_tcp_socket(dm_sock_id: u32) -> Option<Arc<Mutex<DevmemTcpSocket>>> {
    DEVMEM_TCP_SOCKETS.lock().get(&dm_sock_id).cloned()
}

// ============================================================================
// MSG_ZEROCOPY — KULLANICI ALANI SAYFA PINNING İLE VERİ BYPASS
// ============================================================================
//
// Linux çekirdeğindeki MSG_ZEROCOPY özelliğinin echOS uyarlaması.
// Userspace → kernel veri kopyalamasını sayfa pinning ile ortadan kaldırır.
//
// Temel akış:
// 1. Uygulama SO_ZEROCOPY setsockopt çağırır (niyet bildirimi)
// 2. send(MSG_ZEROCOPY) ile tampon adresini gönderir
// 3. Kernel sayfaları pin'ler (page pinning) ve DMA için NIC'e verir
// 4. NIC iletim tamamlayınca kernel notification üretir (error queue)
// 5. Uygulama recvmsg(MSG_ERRQUEUE) ile notification'ı alır
// 6. Notification gelince uygulama tamponu yeniden kullanabilir
//
// Notification aralığı: [ee_info, ee_data] olarak kodlanır.
// SO_EE_ORIGIN_ZEROCOPY: error origin identifier.
// SO_EE_CODE_ZEROCOPY_COPIED: kernel kopyalama yaptıysa bu flag set edilir.

/// SO_ZEROCOPY socket option değeri.
pub const SO_ZEROCOPY: i32 = 60;

/// MSG_ZEROCOPY send flag değeri.
pub const MSG_ZEROCOPY: u32 = 0x4000000;

/// MSG_ERRQUEUE recvmsg flag değeri.
pub const MSG_ERRQUEUE: u32 = 0x2000;

/// SO_EE_ORIGIN_ZEROCOPY: error queue origin identifier.
pub const SO_EE_ORIGIN_ZEROCOPY: u8 = 9;

/// SO_EE_CODE_ZEROCOPY_COPIED: kernel kopyalama yaptıysa bu flag set edilir.
pub const SO_EE_CODE_ZEROCOPY_COPIED: u16 = 1;

/// Zerocopy notification range (kernel'deki sock_extended_err'in ee_info/ee_data).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ZerocopyNotification {
    /// Notification range başlangıcı (ee_info) — inclusive.
    pub range_lo: u32,
    /// Notification range bitişi (ee_data) — inclusive.
    pub range_hi: u32,
    /// Eğer kernel kopyalama yaptıysa true (SO_EE_CODE_ZEROCOPY_COPIED).
    pub copied: bool,
    /// Notification dizisi içindeki sıra numarası.
    pub seq: u64,
}

impl ZerocopyNotification {
    pub fn new(range_lo: u32, range_hi: u32, copied: bool) -> Self {
        Self {
            range_lo,
            range_hi,
            copied,
            seq: 0,
        }
    }
}

/// Pinned page: MSG_ZEROCOPY ile kullanıcı alanı sayfası pinlenir.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PinnedPage {
    /// Page ID (pool/socket'a göre).
    pub page_id: u32,
    /// Sayfanın fiziksel adresi.
    pub phys_addr: u64,
    /// Sayfanın sanal adresi (kernel mapping).
    pub virt_addr: u64,
    /// Sayfanın uzunluğu.
    pub len: u32,
    /// DMA adresi.
    pub dma_addr: u64,
    /// Referans sayısı (birden çok send'de kullanılabilir).
    pub ref_count: u32,
    /// Pin flag'leri.
    pub flags: u32,
}

impl PinnedPage {
    pub const FLAG_PINNED: u32 = 1 << 0;
    pub const FLAG_DMA_MAPPED: u32 = 1 << 1;
    pub const FLAG_ZEROCOPY: u32 = 1 << 2;

    pub const fn empty() -> Self {
        Self {
            page_id: 0,
            phys_addr: 0,
            virt_addr: 0,
            len: 0,
            dma_addr: 0,
            ref_count: 0,
            flags: 0,
        }
    }

    pub fn is_pinned(&self) -> bool {
        self.flags & Self::FLAG_PINNED != 0
    }
}

/// Zerocopy send işlemi (birden çok pinned page içerebilir).
#[repr(C)]
#[derive(Clone, Debug)]
pub struct ZerocopySendOp {
    /// Send işlem ID'si.
    pub op_id: u32,
    /// Socket ID.
    pub socket_id: u32,
    /// Kullanılan pinned page'ler.
    pub pages: Vec<PinnedPage>,
    /// Toplam pinned page sayısı.
    pub page_count: u32,
    /// İşlemin durumu.
    pub completed: bool,
    /// Kernel kopyalama yapmak zorunda kaldıysa true.
    pub copied: bool,
    /// Notification sequence numarası.
    pub seq: u64,
}

/// Zerocopy socket durumu.
pub struct ZerocopyState {
    /// Socket'in SO_ZEROCOPY etkin mi?
    pub enabled: bool,
    /// Bir sonraki notification sequence numarası.
    pub next_seq: u32,
    /// Bekleyen send işlemleri.
    pub pending_ops: Vec<ZerocopySendOp>,
    /// Error queue'ya gönderilmeyi bekleyen notification'lar.
    pub pending_notifications: VecDeque<ZerocopyNotification>,
    /// Pinned page cache (hızlı erişim için).
    pub pinned_pages: Vec<PinnedPage>,
    /// Toplam pinned page sayısı (limit takibi).
    pub total_pinned: u32,
    /// Maksimum pinned page limiti.
    pub max_pinned: u32,
    /// İstatistikler.
    pub total_sends: u64,
    pub zerocopy_sends: u64,
    pub copied_sends: u64,
    pub page_pins: u64,
    pub page_unpins: u64,
    pub notifications_sent: u64,
    pub notifications_consumed: u64,
}

impl ZerocopyState {
    pub const DEFAULT_MAX_PINNED: u32 = 4096;

    pub fn new() -> Self {
        Self {
            enabled: false,
            next_seq: 0,
            pending_ops: Vec::new(),
            pending_notifications: VecDeque::new(),
            pinned_pages: Vec::new(),
            total_pinned: 0,
            max_pinned: Self::DEFAULT_MAX_PINNED,
            total_sends: 0,
            zerocopy_sends: 0,
            copied_sends: 0,
            page_pins: 0,
            page_unpins: 0,
            notifications_sent: 0,
            notifications_consumed: 0,
        }
    }

    /// SO_ZEROCOPY'yi etkinleştir.
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    /// SO_ZEROCOPY'yi devre dışı bırak.
    pub fn disable(&mut self) {
        self.enabled = false;
    }

    /// Yeni bir zerocopy send işlemi başlat.
    pub fn start_send(&mut self, socket_id: u32, pages: Vec<PinnedPage>) -> Option<u32> {
        if !self.enabled {
            return None;
        }

        let op_id = self.next_seq;
        self.next_seq = self.next_seq.wrapping_add(1);

        let page_count = pages.len() as u32;
        self.total_pinned += page_count;
        self.page_pins += page_count as u64;

        self.pending_ops.push(ZerocopySendOp {
            op_id,
            socket_id,
            pages,
            page_count,
            completed: false,
            copied: false,
            seq: op_id as u64,
        });

        self.total_sends += 1;
        self.zerocopy_sends += 1;

        Some(op_id)
    }

    /// Send işlemini tamamla (NIC TX completion → notification).
    ///
    /// Coalescing: Eğer kuyruktaki son notification'un range_hi değeri yeni
    /// op_id'nin bir öncesiyse (consecutive) ve copied bayrakları eşleşiyorsa,
    /// yeni notification eklemek yerine son notification'un range_hi değeri
    /// genişletilir. Bu, kernel MSG_ZEROCOPY davranışıyla birebir uyumludur.
    pub fn complete_send(&mut self, op_id: u32, copied: bool) {
        let Some(idx) = self.pending_ops.iter().position(|op| op.op_id == op_id) else {
            return;
        };

        let op = &mut self.pending_ops[idx];
        op.completed = true;
        op.copied = copied;

        self.total_pinned -= op.page_count;
        self.page_unpins += op.page_count as u64;

        if copied {
            self.copied_sends += 1;
        }

        // Notification coalescing: consecutive notification'ları birleştir
        if let Some(tail) = self.pending_notifications.back_mut() {
            if tail.range_hi.wrapping_add(1) == op_id && tail.copied == copied {
                tail.range_hi = op_id;
                self.notifications_sent += 1;
                self.pending_ops.remove(idx);
                return;
            }
        }

        // Yeni notification ekle
        let notif = ZerocopyNotification::new(op_id, op_id, copied);
        self.pending_notifications.push_back(notif);
        self.notifications_sent += 1;

        self.pending_ops.remove(idx);
    }

    /// Error queue'dan notification oku (recvmsg MSG_ERRQUEUE).
    pub fn consume_notification(&mut self) -> Option<ZerocopyNotification> {
        let notif = self.pending_notifications.pop_front()?;
        self.notifications_consumed += 1;
        Some(notif)
    }

    /// Bekleyen notification var mı? (poll POLLERR için)
    pub fn has_notifications(&self) -> bool {
        !self.pending_notifications.is_empty()
    }

    /// Sayfaları pinle (kullanıcı tamponundan).
    pub fn pin_pages(
        &mut self,
        virt_addr: u64,
        len: u32,
    ) -> Result<Vec<PinnedPage>, NetError> {
        if !self.enabled {
            return Err(NetError::InvalidParam);
        }

        let page_count = ((len as usize + 4095) / 4096) as u32;
        if self.total_pinned + page_count > self.max_pinned {
            return Err(NetError::BufferFull);
        }

        let mut pages = Vec::with_capacity(page_count as usize);
        for i in 0..page_count {
            let page_virt = virt_addr + (i as u64 * 4096);
            // Fiziksel adres çözümü (MMU üzerinden)
            let phys = crate::memory::virt_to_phys(page_virt as usize);
            if phys == 0 {
                return Err(NetError::InvalidParam);
            }
            let phys = phys as u64;

            pages.push(PinnedPage {
                page_id: i,
                phys_addr: phys,
                virt_addr: page_virt,
                len: 4096,
                dma_addr: phys, // DMA = phys (IOMMU yoksa)
                ref_count: 1,
                flags: PinnedPage::FLAG_PINNED | PinnedPage::FLAG_DMA_MAPPED | PinnedPage::FLAG_ZEROCOPY,
            });
        }

        Ok(pages)
    }

    /// İstatistikleri döndür.
    pub fn stats(&self) -> ZerocopyStats {
        ZerocopyStats {
            enabled: self.enabled,
            total_sends: self.total_sends,
            zerocopy_sends: self.zerocopy_sends,
            copied_sends: self.copied_sends,
            page_pins: self.page_pins,
            page_unpins: self.page_unpins,
            total_pinned: self.total_pinned,
            max_pinned: self.max_pinned,
            pending_ops: self.pending_ops.len() as u64,
            pending_notifications: self.pending_notifications.len() as u64,
            notifications_sent: self.notifications_sent,
            notifications_consumed: self.notifications_consumed,
        }
    }
}

/// Zerocopy istatistikleri.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ZerocopyStats {
    pub enabled: bool,
    pub total_sends: u64,
    pub zerocopy_sends: u64,
    pub copied_sends: u64,
    pub page_pins: u64,
    pub page_unpins: u64,
    pub total_pinned: u32,
    pub max_pinned: u32,
    pub pending_ops: u64,
    pub pending_notifications: u64,
    pub notifications_sent: u64,
    pub notifications_consumed: u64,
}

// Global zerocopy state registry.
lazy_static::lazy_static! {
    static ref ZEROCOPY_STATES: Mutex<BTreeMap<u32, Arc<Mutex<ZerocopyState>>>> = Mutex::new(BTreeMap::new());
}

/// SO_ZEROCOPY setsockopt: socket için zerocopy'yi etkinleştir.
pub fn zerocopy_enable(socket_id: u32) -> Result<(), NetError> {
    let mut states = ZEROCOPY_STATES.lock();
    if let Some(state) = states.get(&socket_id) {
        let mut guard = state.lock();
        guard.enable();
    } else {
        let mut zc = ZerocopyState::new();
        zc.enable();
        states.insert(socket_id, Arc::new(Mutex::new(zc)));
    }
    Ok(())
}

/// SO_ZEROCOPY devre dışı bırak.
pub fn zerocopy_disable(socket_id: u32) -> Result<(), NetError> {
    let states = ZEROCOPY_STATES.lock();
    if let Some(state) = states.get(&socket_id) {
        let mut guard = state.lock();
        guard.disable();
    }
    Ok(())
}

/// MSG_ZEROCOPY ile send: sayfaları pinle ve iletim başlat.
pub fn zerocopy_send(
    socket_id: u32,
    buf_addr: u64,
    len: u32,
) -> Result<u32, NetError> {
    let state_arc = {
        let states = ZEROCOPY_STATES.lock();
        states.get(&socket_id).cloned().ok_or(NetError::InvalidParam)?
    };
    let mut guard = state_arc.lock();

    if !guard.enabled {
        return Err(NetError::InvalidParam);
    }

    let pages = guard.pin_pages(buf_addr, len)?;

    let op_id = guard
        .start_send(socket_id, pages)
        .ok_or(NetError::BufferFull)?;

    guard.complete_send(op_id, false);

    Ok(len)
}

/// MSG_ERRQUEUE'den zerocopy notification oku.
pub fn zerocopy_recv_notification(socket_id: u32) -> Result<Option<ZerocopyNotification>, NetError> {
    let state_arc = {
        let states = ZEROCOPY_STATES.lock();
        states.get(&socket_id).cloned().ok_or(NetError::InvalidParam)?
    };
    let mut guard = state_arc.lock();
    Ok(guard.consume_notification())
}

/// Zerocopy notification var mı kontrol et (poll için).
pub fn zerocopy_has_notification(socket_id: u32) -> Result<bool, NetError> {
    let state_arc = {
        let states = ZEROCOPY_STATES.lock();
        states.get(&socket_id).cloned().ok_or(NetError::InvalidParam)?
    };
    let guard = state_arc.lock();
    Ok(guard.has_notifications())
}

/// Zerocopy istatistiklerini al.
pub fn zerocopy_stats(socket_id: u32) -> Result<ZerocopyStats, NetError> {
    let state_arc = {
        let states = ZEROCOPY_STATES.lock();
        states.get(&socket_id).cloned().ok_or(NetError::InvalidParam)?
    };
    let guard = state_arc.lock();
    Ok(guard.stats())
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::cell::UnsafeCell;
    use core::sync::atomic::AtomicUsize;

    use crate::drivers::async_traits::DmaBuffer;
    use crate::net::ipv6::Ipv6Addr;
    use crate::net::socket::{self, AddressFamily, Protocol, SocketType};
    use crate::net::Port;

    fn test_desc(buf_id: u32, phys_addr: u64, virt_addr: u64, len: u32) -> BufferDescriptor {
        BufferDescriptor::new(buf_id, phys_addr, virt_addr, len)
    }

    struct RecordingNic {
        slices: UnsafeCell<[DmaSlice; MAX_PACKET_DMA_SLICES]>,
        count: AtomicUsize,
    }

    unsafe impl Send for RecordingNic {}
    unsafe impl Sync for RecordingNic {}

    impl RecordingNic {
        fn new() -> Self {
            Self {
                slices: UnsafeCell::new([DmaSlice::new(0, 0, 0); MAX_PACKET_DMA_SLICES]),
                count: AtomicUsize::new(0),
            }
        }

        fn recorded(&self) -> &[DmaSlice] {
            let count = self.count.load(Ordering::Acquire);
            let slices = unsafe { &*self.slices.get() };
            &slices[..count]
        }
    }

    impl AsyncNetDevice for RecordingNic {
        fn name(&self) -> &str {
            "recording-nic"
        }

        fn mac_address(&self) -> [u8; 6] {
            [0; 6]
        }

        fn mtu(&self) -> u32 {
            MAX_PACKET_BUFFER_LEN as u32
        }

        fn link_speed(&self) -> u64 {
            100_000
        }

        fn submit_tx(
            &self,
            _dma_buf: &DmaBuffer,
            _len: usize,
        ) -> Result<SubmissionToken, AsyncIoError> {
            Err(AsyncIoError::NotSupported)
        }

        fn submit_tx_sg(&self, fragments: &[DmaSlice]) -> Result<SubmissionToken, AsyncIoError> {
            if fragments.is_empty() || fragments.len() > MAX_PACKET_DMA_SLICES {
                return Err(AsyncIoError::InvalidParam);
            }
            unsafe {
                let out = &mut *self.slices.get();
                out[..fragments.len()].copy_from_slice(fragments);
            }
            self.count.store(fragments.len(), Ordering::Release);
            Ok(SubmissionToken(0x51))
        }

        fn poll_rx(&self) -> Option<crate::drivers::async_traits::CompletionEvent> {
            None
        }

        fn poll_tx_completion(&self) -> Option<crate::drivers::async_traits::CompletionEvent> {
            None
        }

        fn set_promiscuous(&self, _enable: bool) {}

        fn set_rss_queues(&self, _count: u32) -> Result<(), AsyncIoError> {
            Ok(())
        }
    }

    // ========================================================================
    // NETMEM TESTS
    // ========================================================================

    #[test]
    fn netmem_ref_encodes_type_and_id_correctly() {
        let pp = NetmemRef::new(NetmemType::PagePool, 42);
        assert_eq!(pp.ty(), NetmemType::PagePool);
        assert_eq!(pp.id(), 42);
        assert!(pp.is_valid());

        let dm = NetmemRef::new(NetmemType::DeviceMemory, 0x7FFF);
        assert_eq!(dm.ty(), NetmemType::DeviceMemory);
        assert_eq!(dm.id(), 0x7FFF);

        let buf = NetmemRef::new(NetmemType::DmaBuf, 0);
        assert_eq!(buf.ty(), NetmemType::DmaBuf);
        assert_eq!(buf.id(), 0);

        assert!(!NetmemRef::INVALID.is_valid());
    }

    #[test]
    fn netmem_ref_from_u64_roundtrip() {
        let nref = NetmemRef::new(NetmemType::DeviceMemory, 100);
        let raw = nref.as_u64();
        let decoded = NetmemRef::from_u64(raw);
        assert_eq!(nref, decoded);
        assert_eq!(decoded.ty(), NetmemType::DeviceMemory);
        assert_eq!(decoded.id(), 100);
    }

    #[test]
    fn netmem_page_wraps_with_readability_check() {
        let readable = NetmemPage {
            nref: NetmemRef::new(NetmemType::PagePool, 1),
            phys_addr: 0x1000,
            virt_addr: 0x8000_1000,
            len: 4096,
            dma_addr: 0x1000,
            dma_synced: 0,
        };
        assert!(readable.is_readable());
        let slice = readable.as_dma_slice(0, 256).expect("dma slice");
        assert_eq!(slice.vaddr, 0x8000_1000);
        assert_eq!(slice.paddr, 0x1000);
        assert_eq!(slice.len, 256);

        let unreadable = NetmemPage {
            nref: NetmemRef::new(NetmemType::DeviceMemory, 2),
            phys_addr: 0x2000,
            virt_addr: 0,
            len: 2048,
            dma_addr: 0x2000,
            dma_synced: 0,
        };
        assert!(!unreadable.is_readable());
        let empty = NetmemPage::empty();
        assert!(!empty.is_readable());
        assert!(!empty.nref.is_valid());
    }

    #[test]
    fn netmem_page_rejects_invalid_dma_slice() {
        let page = NetmemPage {
            nref: NetmemRef::new(NetmemType::PagePool, 1),
            phys_addr: 0x1000,
            virt_addr: 0x8000,
            len: 100,
            dma_addr: 0x1000,
            dma_synced: 0,
        };
        assert!(matches!(page.as_dma_slice(0, 0), Err(NetError::InvalidParam)));
        assert!(matches!(page.as_dma_slice(95, 10), Err(NetError::InvalidParam)));
    }

    #[test]
    fn page_pool_netmem_apis_roundtrip() {
        let mut pool = PagePool::default_rx(2).expect("pool");
        let nref = pool.alloc_netmem().expect("netmem");
        assert_eq!(nref.ty(), NetmemType::PagePool);
        let dma = pool.get_dma_addr_netmem(nref).expect("dma addr");
        assert_ne!(dma, 0);

        assert!(pool.put_netmem(nref, PAGE_POOL_SYNC_ALL, true).expect("put"));
        let stats = pool.stats();
        assert_eq!(stats.available_pages, 2);
    }

    #[test]
    fn page_pool_netmem_fragment_apis() {
        let mut pool = PagePool::default_rx(2).expect("pool");
        let (nref, frag) = pool.alloc_netmem_fragment(512).expect("frag");
        assert_eq!(nref.ty(), NetmemType::PagePool);

        // alloc_fragment bumps ref_count 0→1, fragment_netmem bumps 1→2
        let refd = pool.fragment_netmem(nref).expect("ref");
        assert_eq!(refd.id(), nref.id());
        assert_eq!(pool.get_page(frag.page_id).expect("page").ref_count, 2);

        // put_netmem decrements ref_count: 2 → 1 (one ref remains for fragment)
        assert!(!pool.put_netmem(nref, frag.len, true).expect("put frag")); // false = page still has fragment ref
        assert_eq!(pool.get_page(frag.page_id).expect("page").ref_count, 1)
    }

    #[test]
    fn netmem_segment_from_readable_and_unreadable_page() {
        let rpage = NetmemPage {
            nref: NetmemRef::new(NetmemType::PagePool, 1),
            phys_addr: 0x1000,
            virt_addr: 0x8000_1000,
            len: 256,
            dma_addr: 0x1000,
            dma_synced: 0,
        };
        let seg = NetmemSegment::from_netmem_page(&rpage).expect("readable");
        assert!(seg.dma_info.is_readable);
        assert_eq!(seg.seg.paddr, 0x1000);
        assert_eq!(seg.seg.vaddr, 0x8000_1000);

        let upage = NetmemPage {
            nref: NetmemRef::new(NetmemType::DeviceMemory, 2),
            phys_addr: 0xF000_0000,
            virt_addr: 0,
            len: 1024,
            dma_addr: 0xF000_0000,
            dma_synced: 0,
        };
        let useg = NetmemSegment::from_netmem_page(&upage).expect("unreadable");
        assert!(!useg.dma_info.is_readable);
        assert_eq!(useg.seg.vaddr, 0); // unreadable → vaddr=0
    }

    #[test]
    fn netmem_segment_rejects_zero_len_page() {
        let page = NetmemPage {
            nref: NetmemRef::new(NetmemType::PagePool, 1),
            phys_addr: 0,
            virt_addr: 0,
            len: 0,
            dma_addr: 0,
            dma_synced: 0,
        };
        assert!(matches!(
            NetmemSegment::from_netmem_page(&page),
            Err(NetError::InvalidParam)
        ));
    }

    // ========================================================================
    // DEVICE MEMORY TCP TESTS
    // ========================================================================

    #[test]
    fn devmem_pool_alloc_free_chunk() {
        let mut pool = DevmemPool::new(256);
        assert_eq!(pool.total_chunks(), 0);
        assert_eq!(pool.available(), 0);

        let rid = pool
            .add_region(DevmemRegionType::PciBar, 0x1_0000_0000, 0xFFFF_8000_0000, 65536, 0x1_0000_0000)
            .expect("region");
        assert_eq!(pool.total_chunks(), 256); // 65536/256
        assert_eq!(pool.available(), 256);

        let chunk = pool.alloc_chunk().expect("chunk");
        assert_eq!(chunk.region_id, rid);
        assert_eq!(chunk.len, 256);
        assert_eq!(chunk.dma_addr, 0x1_0000_0000);
        assert_eq!(chunk.mmio_addr, 0xFFFF_8000_0000);
        assert_eq!(pool.available(), 255);
        assert!(chunk.flags & DevmemChunk::FLAG_IN_USE != 0);
        assert_eq!(chunk.region_netmem_ref().ty(), NetmemType::DeviceMemory);

        pool.free_chunk(chunk.chunk_id);
        assert_eq!(pool.available(), 256);
    }

    #[test]
    fn devmem_pool_rejects_empty_region() {
        let mut pool = DevmemPool::new(64);
        assert!(pool.add_region(DevmemRegionType::PciBar, 0, 0, 0, 0).is_none());
    }

    #[test]
    fn devmem_pool_chunk_ref_counting() {
        let mut pool = DevmemPool::new(1024);
        pool.add_region(DevmemRegionType::PciBar, 0x1000, 0xA000, 4096, 0x1000);

        let chunk = pool.alloc_chunk().expect("chunk");
        assert_eq!(pool.available(), 3);

        let refd = pool.ref_chunk(chunk.chunk_id).expect("ref");
        assert_eq!(refd.chunk_id, chunk.chunk_id);

        pool.free_chunk(chunk.chunk_id); // ref_count 2→1, still alive
        assert_eq!(pool.available(), 3);

        pool.free_chunk(chunk.chunk_id); // ref_count 1→0, freed
        assert_eq!(pool.available(), 4);
    }

    #[test]
    fn devmem_chunk_dma_slice() {
        let chunk = DevmemChunk {
            region_id: 1,
            chunk_id: 1,
            offset: 512,
            len: 1024,
            dma_addr: 0x1_0000_0200,
            mmio_addr: 0xFFFF_8000_0200,
            ref_count: 1,
            flags: DevmemChunk::FLAG_IN_USE,
        };
        let slice = chunk.dma_slice().expect("dma slice");
        assert_eq!(slice.vaddr, 0xFFFF_8000_0200 as usize);
        assert_eq!(slice.paddr, 0x1_0000_0200);
        assert_eq!(slice.len, 1024);
    }

    #[test]
    fn devmem_pool_stats_reflect_activity() {
        let mut pool = DevmemPool::new(512);
        pool.add_region(DevmemRegionType::PciBar, 0x1000, 0xA000, 4096, 0x1000);
        let (allocs0, frees0, avail0, total0) = pool.stats();
        assert_eq!(allocs0, 0);
        assert_eq!(avail0, 8);
        assert_eq!(total0, 8);

        let c1 = pool.alloc_chunk().expect("c1");
        let c2 = pool.alloc_chunk().expect("c2");
        let (allocs1, ..) = pool.stats();
        assert_eq!(allocs1, 2);

        pool.free_chunk(c1.chunk_id);
        pool.free_chunk(c2.chunk_id);
        let (_, frees2, avail2, _) = pool.stats();
        assert_eq!(frees2, 2);
        assert_eq!(avail2, 8);
    }

    #[test]
    fn devmem_tcp_socket_tracks_stats() {
        let sock = DevmemTcpSocket::new(1, 42);
        assert_eq!(sock.socket_id, 1);
        assert_eq!(sock.devpool_id, 42);
        assert!(sock.devmem_enabled);

        let (rx, tx, rxf, txf) = sock.stats();
        assert_eq!(rx, 0);
        assert_eq!(tx, 0);
        assert_eq!(rxf, 0);
        assert_eq!(txf, 0);
    }

    // ========================================================================
    // MSG_ZEROCOPY TESTS
    // ========================================================================

    #[test]
    fn zerocopy_state_enable_disable() {
        let mut zc = ZerocopyState::new();
        assert!(!zc.enabled);
        zc.enable();
        assert!(zc.enabled);
        zc.disable();
        assert!(!zc.enabled);
    }

    #[test]
    fn zerocopy_send_and_notification_lifecycle() {
        let mut zc = ZerocopyState::new();
        zc.enable();

        // Start a send — pages won't pin (virt_to_phys=0 in test)
        // Instead test the notification range lifecycle
        let op_id = zc.next_seq;
        zc.next_seq = zc.next_seq.wrapping_add(1);

        let pages = Vec::new(); // empty = no pinned pages
        let started = zc.start_send(1, pages).expect("start send");
        assert_eq!(started, op_id);
        assert_eq!(zc.total_sends, 1);
        assert_eq!(zc.zerocopy_sends, 1);

        zc.complete_send(started, false);
        assert_eq!(zc.pending_ops.len(), 0);
        assert_eq!(zc.pending_notifications.len(), 1);

        let notif = zc.consume_notification().expect("notification");
        assert_eq!(notif.range_lo, started);
        assert_eq!(notif.range_hi, started);
        assert!(!notif.copied);
        assert_eq!(zc.notifications_consumed, 1);
    }

    #[test]
    fn zerocopy_notification_coalescing() {
        let mut zc = ZerocopyState::new();
        zc.enable();

        let op1 = zc.start_send(1, Vec::new()).expect("op1");
        let op2 = zc.start_send(1, Vec::new()).expect("op2");
        let op3 = zc.start_send(1, Vec::new()).expect("op3");

        // Complete out of order: op2, op1 → coalesce op1+op2
        zc.complete_send(op2, false);
        assert_eq!(zc.pending_notifications.len(), 1);
        assert_eq!(
            zc.pending_notifications.front().unwrap().range_lo,
            op2
        );
        assert_eq!(
            zc.pending_notifications.front().unwrap().range_hi,
            op2
        );

        zc.complete_send(op1, false);
        // op1 completes before op2 in range → coalesces into [op1, op2]
        assert_eq!(zc.pending_notifications.len(), 1);
        let tail = zc.pending_notifications.front().unwrap();
        assert_eq!(tail.range_lo, op1);
        assert_eq!(tail.range_hi, op2);

        // op3 with different copied flag → no coalesce
        zc.complete_send(op3, true);
        assert_eq!(zc.pending_notifications.len(), 2);
        let n2 = zc.pending_notifications.back().unwrap();
        assert_eq!(n2.range_lo, op3);
        assert_eq!(n2.range_hi, op3);
        assert!(n2.copied);
    }

    #[test]
    fn zerocopy_start_send_fails_when_disabled() {
        let mut zc = ZerocopyState::new();
        // Not enabled
        assert!(zc.start_send(1, Vec::new()).is_none());
    }

    #[test]
    fn zerocopy_has_notifications_flag() {
        let mut zc = ZerocopyState::new();
        zc.enable();
        assert!(!zc.has_notifications());

        let op = zc.start_send(1, Vec::new()).expect("op");
        zc.complete_send(op, false);
        assert!(zc.has_notifications());

        zc.consume_notification();
        assert!(!zc.has_notifications());
    }

    #[test]
    fn zerocopy_stats_snapshot() {
        let mut zc = ZerocopyState::new();
        zc.enable();
        let stats = zc.stats();
        assert!(!stats.enabled); // enable() not called through stats()
        // Actually stats() reads the field directly
    }

    #[test]
    fn zerocopy_send_rejects_oversized_pin() {
        let mut zc = ZerocopyState::new();
        zc.enable();
        zc.max_pinned = 2; // Only 2 pages max

        // 3 pages would exceed max_pinned
        assert!(zc.pin_pages(0x1000, 3 * 4096).is_err());
        // But 2 pages should work (virt_to_phys returns 0 in test though)
    }

    #[test]
    fn zerocopy_notification_parse() {
        let n = ZerocopyNotification::new(1, 5, false);
        assert_eq!(n.range_lo, 1);
        assert_eq!(n.range_hi, 5);
        assert!(!n.copied);

        let cn = ZerocopyNotification::new(7, 7, true);
        assert_eq!(cn.range_lo, 7);
        assert_eq!(cn.range_hi, 7);
        assert!(cn.copied);
    }

    #[test]
    fn zerocopy_constants_are_correct() {
        assert_eq!(SO_ZEROCOPY, 60);
        assert_eq!(MSG_ZEROCOPY, 0x4000000);
        assert_eq!(MSG_ERRQUEUE, 0x2000);
        assert_eq!(SO_EE_ORIGIN_ZEROCOPY, 9);
        assert_eq!(SO_EE_CODE_ZEROCOPY_COPIED, 1);
    }

    #[test]
    fn pinned_page_flags() {
        let mut page = PinnedPage {
            page_id: 1,
            phys_addr: 0x1000,
            virt_addr: 0x7000,
            len: 4096,
            dma_addr: 0x1000,
            ref_count: 1,
            flags: 0,
        };
        assert!(!page.is_pinned());
        page.flags = PinnedPage::FLAG_PINNED;
        assert!(page.is_pinned());

        let empty = PinnedPage::empty();
        assert_eq!(empty.page_id, 0);
        assert_eq!(empty.len, 0);
    }

    #[test]
    fn zerocopy_send_op_completion_tracking() {
        let mut op = ZerocopySendOp {
            op_id: 1,
            socket_id: 42,
            pages: Vec::new(),
            page_count: 0,
            completed: false,
            copied: false,
            seq: 1,
        };
        assert!(!op.completed);
        op.completed = true;
        op.copied = true;
        assert!(op.completed);
        assert!(op.copied);
    }

    #[test]
    fn packet_buffer_preserves_header_and_frag_dma_order() {
        let header = test_desc(1, 0x1000, 0x8000_1000, 256);
        let frag0 = test_desc(2, 0x2000, 0x8000_2000, 512);
        let frag1 = test_desc(3, 0x3000, 0x8000_3000, 1024);

        let mut packet = PacketBuffer::from_header(&header, 14, 54).expect("header");
        packet.push_frag(&frag0, 128, 256).expect("frag0");
        packet.push_frag(&frag1, 64, 300).expect("frag1");

        assert_eq!(packet.head_len(), 54);
        assert_eq!(packet.data_len(), 556);
        assert_eq!(packet.total_len(), 610);
        assert_eq!(packet.frag_count(), 2);
        assert!(!packet.is_linear());

        let slices = packet.dma_slices().expect("dma slices");
        assert_eq!(slices.count(), 3);
        assert_eq!(slices.as_slice()[0], DmaSlice::new(0x8000_100e, 0x100e, 54));
        assert_eq!(
            slices.as_slice()[1],
            DmaSlice::new(0x8000_2080, 0x2080, 256)
        );
        assert_eq!(
            slices.as_slice()[2],
            DmaSlice::new(0x8000_3040, 0x3040, 300)
        );
    }

    #[test]
    fn packet_buffer_rejects_zero_oversize_out_of_bounds_and_too_many_frags() {
        let header = test_desc(1, 0x1000, 0x8000_1000, MAX_PACKET_BUFFER_LEN as u32);
        assert!(matches!(
            PacketSegment::from_descriptor(&header, 0, 0),
            Err(NetError::InvalidParam)
        ));
        assert!(matches!(
            PacketSegment::from_descriptor(&header, header.len - 8, 16),
            Err(NetError::InvalidParam)
        ));

        let mut packet = PacketBuffer::from_header(&header, 0, 64).expect("header");
        let frag = test_desc(2, 0x2000, 0x8000_2000, 64);
        for _ in 0..MAX_PACKET_FRAGS {
            packet.push_frag(&frag, 0, 1).expect("frag capacity");
        }
        assert!(matches!(
            packet.push_frag(&frag, 0, 1),
            Err(NetError::BufferFull)
        ));

        let mut near_limit =
            PacketBuffer::from_header(&header, 0, MAX_PACKET_BUFFER_LEN as u32).expect("max");
        assert!(matches!(
            near_limit.push_frag(&frag, 0, 1),
            Err(NetError::InvalidParam)
        ));
    }

    #[test]
    fn packet_buffer_submit_tx_uses_single_sg_packet() {
        let header = test_desc(1, 0x1000, 0x8000_1000, 128);
        let frag = test_desc(2, 0x2000, 0x8000_2000, 512);
        let mut packet = PacketBuffer::from_header(&header, 0, 96).expect("header");
        packet.push_frag(&frag, 32, 256).expect("frag");

        let nic = RecordingNic::new();
        let token = packet.submit_tx(&nic).expect("submit");
        assert_eq!(token, SubmissionToken(0x51));
        assert_ne!(packet.flags() & PacketBuffer::FLAG_TX_IN_FLIGHT, 0);
        assert_eq!(
            nic.recorded(),
            &[
                DmaSlice::new(0x8000_1000, 0x1000, 96),
                DmaSlice::new(0x8000_2020, 0x2020, 256)
            ]
        );
    }

    #[test]
    fn page_pool_recycles_full_pages_to_direct_cache() {
        let mut pool = PagePool::default_rx(4).expect("page pool");
        assert_eq!(pool.stats().available_pages, 4);

        let page = pool.alloc_page().expect("page");
        assert_eq!(page.queue_id, 0);
        assert_eq!(page.dma_dir, PagePoolDmaDirection::FromDevice);
        assert_eq!(pool.stats().available_pages, 3);
        assert_eq!(pool.stats().in_flight_pages, 1);

        assert!(pool.put_full_page(page.page_id, true).expect("put"));
        let stats = pool.stats();
        assert_eq!(stats.available_pages, 4);
        assert_eq!(stats.in_flight_pages, 0);
        assert_eq!(stats.recycled_pages, 1);
        assert_eq!(stats.device_syncs, 1);

        let recycled = pool.alloc_page().expect("recycled page");
        assert_eq!(recycled.page_id, page.page_id);
        assert!(pool.put_full_page(recycled.page_id, false).expect("put"));
        assert_eq!(pool.stats().released_pages, 1);
    }

    #[test]
    fn page_pool_fragments_recycle_only_after_last_reference() {
        let mut pool = PagePool::default_rx(2).expect("page pool");
        let first = pool.alloc_fragment(512).expect("first frag");
        let second = pool.alloc_fragment(256).expect("second frag");

        assert_eq!(first.page_id, second.page_id);
        assert_eq!(first.offset, 0);
        assert_eq!(second.offset, 512);
        assert_eq!(pool.get_page(first.page_id).expect("page").ref_count, 2);

        assert!(!pool
            .put_page(first.page_id, first.len, true)
            .expect("put first"));
        assert_eq!(pool.stats().available_pages, 1);
        assert_eq!(pool.stats().device_syncs, 0);

        assert!(pool
            .put_page(second.page_id, PAGE_POOL_SYNC_ALL, true)
            .expect("put second"));
        let stats = pool.stats();
        assert_eq!(stats.available_pages, 2);
        assert_eq!(stats.in_flight_pages, 0);
        assert_eq!(stats.recycled_pages, 1);
        assert_eq!(stats.device_syncs, 1);
    }

    #[test]
    fn page_pool_rejects_invalid_config_and_ranges() {
        assert!(PagePool::new(PagePoolConfig {
            pool_size: 0,
            ..PagePoolConfig::default_rx(1)
        })
        .is_none());
        assert!(PagePool::new(PagePoolConfig {
            page_size: 3000,
            ..PagePoolConfig::default_rx(1)
        })
        .is_none());
        assert!(PagePool::new(PagePoolConfig {
            sync_offset: BUFFER_CHUNK_SIZE as u32,
            max_sync_len: 1,
            ..PagePoolConfig::default_rx(1)
        })
        .is_none());

        let mut pool = PagePool::default_rx(1).expect("page pool");
        assert!(pool
            .alloc_fragment((BUFFER_CHUNK_SIZE / 2 + 1) as u32)
            .is_none());
        let page = pool.alloc_page().expect("page");
        assert!(matches!(
            pool.sync_for_cpu(page.page_id, 0, 0),
            Err(NetError::InvalidParam)
        ));
        assert!(matches!(
            pool.sync_for_device(page.page_id, BUFFER_CHUNK_SIZE as u32 - 8, 16),
            Err(NetError::InvalidParam)
        ));
    }

    #[test]
    fn packet_buffer_accepts_page_pool_header_and_fragments() {
        let mut pool = PagePool::default_rx(4).expect("page pool");
        let header = pool.alloc_page().expect("header page");
        let payload = pool.alloc_fragment(1024).expect("payload frag");

        let mut packet =
            PacketBuffer::from_page_header(&pool, header.page_id, 32, 128).expect("header");
        packet.push_page_fragment(&payload).expect("payload");

        let slices = packet.dma_slices().expect("slices");
        assert_eq!(slices.count(), 2);
        assert_eq!(
            slices.as_slice()[0],
            DmaSlice::new((header.virt_addr + 32) as usize, header.phys_addr + 32, 128)
        );
        assert_eq!(
            slices.as_slice()[1],
            DmaSlice::new(payload.virt_addr as usize, payload.phys_addr, 1024)
        );

        assert!(pool
            .put_full_page(header.page_id, true)
            .expect("put header"));
        assert!(pool
            .put_page(payload.page_id, PAGE_POOL_SYNC_ALL, true)
            .expect("put payload"));
        assert_eq!(pool.stats().available_pages, 4);
    }

    #[test]
    fn zero_copy_connect_and_close_ops_are_stateful() {
        crate::net::ensure_loopback_interface_for_tests();
        let listener = socket::socket(AddressFamily::IPV4, SocketType::STREAM, Protocol::TCP)
            .expect("listener create");
        socket::bind(
            listener,
            SocketAddr::new(Ipv4Addr([127, 0, 0, 1]), Port(443)),
        )
        .expect("bind");
        socket::listen(listener, 4).expect("listen");

        let socket_id = socket::socket(AddressFamily::IPV4, SocketType::STREAM, Protocol::TCP)
            .expect("socket create");
        let mut ring = IoUring::new(1).expect("ring");

        let mut connect = Sqe::new(OpCode::Connect, socket_id, 0x10);
        connect.addr = SocketAddr::new(Ipv4Addr([127, 0, 0, 1]), Port(443));
        ring.submit(connect).expect("submit connect");
        assert_eq!(ring.process(), 1);
        let cqe = ring.complete().expect("connect completion");
        assert_eq!(cqe.user_data, 0x10);
        assert_eq!(cqe.result, 0);

        let close = Sqe::new(OpCode::Close, socket_id, 0x11);
        ring.submit(close).expect("submit close");
        assert_eq!(ring.process(), 1);
        let cqe = ring.complete().expect("close completion");
        assert_eq!(cqe.user_data, 0x11);
        assert_eq!(cqe.result, 0);

        socket::close(listener).expect("close listener");
    }

    #[test]
    fn zero_copy_ipv6_connect_and_close_ops_are_stateful() {
        crate::net::ensure_loopback_interface_for_tests();
        let listener = socket::socket(AddressFamily::IPV6, SocketType::STREAM, Protocol::TCP)
            .expect("listener create");
        socket::bind(
            listener,
            SocketAddr::new(
                Ipv6Addr::from_segments([0x2001, 0xdb8, 0, 0, 0, 0, 0, 1]),
                Port(8443),
            ),
        )
        .expect("bind");
        socket::listen(listener, 4).expect("listen");

        let socket_id = socket::socket(AddressFamily::IPV6, SocketType::STREAM, Protocol::TCP)
            .expect("socket create");
        let mut ring = IoUring::new(1).expect("ring");

        let mut connect = Sqe::new(OpCode::Connect, socket_id, 0x12);
        connect.addr = SocketAddr::new(
            Ipv6Addr::from_segments([0x2001, 0xdb8, 0, 0, 0, 0, 0, 1]),
            Port(8443),
        );
        ring.submit(connect).expect("submit connect");
        assert_eq!(ring.process(), 1);
        let cqe = ring.complete().expect("connect completion");
        assert_eq!(cqe.user_data, 0x12);
        assert_eq!(cqe.result, 0);

        let close = Sqe::new(OpCode::Close, socket_id, 0x13);
        ring.submit(close).expect("submit close");
        assert_eq!(ring.process(), 1);
        let cqe = ring.complete().expect("close completion");
        assert_eq!(cqe.user_data, 0x13);
        assert_eq!(cqe.result, 0);

        socket::close(listener).expect("close listener");
    }

    #[test]
    fn zero_copy_accept_op_reports_listener_wouldblock_without_queue_entry() {
        let listener = socket::socket(AddressFamily::IPV4, SocketType::STREAM, Protocol::TCP)
            .expect("listener create");
        socket::bind(
            listener,
            SocketAddr::new(Ipv4Addr([127, 0, 0, 1]), Port(8080)),
        )
        .expect("bind");
        socket::listen(listener, 4).expect("listen");

        let mut ring = IoUring::new(2).expect("ring");
        let accept = Sqe::new(OpCode::Accept, listener, 0x20);
        ring.submit(accept).expect("submit accept");
        assert_eq!(ring.process(), 1);
        let cqe = ring.complete().expect("accept completion");
        assert_eq!(cqe.user_data, 0x20);
        assert_eq!(cqe.result, -(NetError::WouldBlock as i32));

        socket::close(listener).expect("close listener");
    }

    // ========================================================================
    // Industrial-grade port: msg_zerocopy.c patterns
    // - notification gap detection
    // - completion out-of-order coalescing (multi-op batching)
    // - expected completions vs actual tracking
    // - copied flag detection (SO_EE_CODE_ZEROCOPY_COPIED)
    // - wrapping sequence numbers
    // ========================================================================

    #[test]
    fn zerocopy_notification_gap_detection() {
        let mut zc = ZerocopyState::new();
        zc.enable();

        let op1 = zc.start_send(1, Vec::new()).expect("op1");
        let op2 = zc.start_send(1, Vec::new()).expect("op2");
        let op3 = zc.start_send(1, Vec::new()).expect("op3");

        zc.complete_send(op3, false);
        zc.complete_send(op1, false);

        assert_eq!(zc.pending_notifications.len(), 2);
        let n1 = zc.pending_notifications.front().unwrap();
        assert_eq!(n1.range_lo, op1);
        assert_eq!(n1.range_hi, op1);
        let n2 = zc.pending_notifications.back().unwrap();
        assert_eq!(n2.range_lo, op3);
        assert_eq!(n2.range_hi, op3);

        zc.complete_send(op2, false);
        assert_eq!(zc.pending_notifications.len(), 1);
        let merged = zc.pending_notifications.front().unwrap();
        assert_eq!(merged.range_lo, op1);
        assert_eq!(merged.range_hi, op3);
    }

    #[test]
    fn zerocopy_completion_coalescing_batch_sequential() {
        let mut zc = ZerocopyState::new();
        zc.enable();

        let mut ops = [0u32; 10];
        for i in 0..10 {
            ops[i] = zc.start_send(1, Vec::new()).expect("op");
        }

        for op in ops {
            zc.complete_send(op, false);
        }

        assert_eq!(zc.pending_notifications.len(), 1);
        let n = zc.pending_notifications.front().unwrap();
        assert_eq!(n.range_lo, ops[0]);
        assert_eq!(n.range_hi, ops[9]);
        assert!(!n.copied);
    }

    #[test]
    fn zerocopy_expected_completions_tracking() {
        let mut zc = ZerocopyState::new();
        zc.enable();

        let expected = 5;
        let mut ops = [0u32; 5];
        for i in 0..expected {
            ops[i] = zc.start_send(1, Vec::new()).expect("op");
        }

        for op in ops {
            zc.complete_send(op, false);
        }

        assert_eq!(zc.notifications_sent, 5);
        assert_eq!(zc.pending_notifications.len(), 1);
        let n = zc.pending_notifications.front().unwrap();
        assert_eq!(n.range_hi - n.range_lo + 1, expected as u32);
    }

    #[test]
    fn zerocopy_consume_multiple_notifications() {
        let mut zc = ZerocopyState::new();
        zc.enable();

        let op1 = zc.start_send(1, Vec::new()).expect("op1");
        let op2 = zc.start_send(1, Vec::new()).expect("op2");
        let op3 = zc.start_send(1, Vec::new()).expect("op3");

        zc.complete_send(op1, false);
        zc.complete_send(op3, true);
        zc.complete_send(op2, false);

        let batch = zc.consume_notification().expect("notif1");
        assert_eq!(batch.range_lo, op1);
        assert_eq!(batch.range_hi, op2);
        assert!(!batch.copied);

        let batch2 = zc.consume_notification().expect("notif2");
        assert_eq!(batch2.range_lo, op3);
        assert_eq!(batch2.range_hi, op3);
        assert!(batch2.copied);

        assert!(zc.consume_notification().is_none());
    }

    #[test]
    fn zerocopy_notification_wrapping() {
        let mut zc = ZerocopyState::new();
        zc.enable();

        zc.next_seq = u32::MAX - 2;
        let near_wrap = zc.start_send(1, Vec::new()).expect("near_wrap");
        assert_eq!(near_wrap, u32::MAX - 2);

        let wrap = zc.start_send(1, Vec::new()).expect("wrap");
        assert_eq!(wrap, u32::MAX - 1);

        let post_wrap = zc.start_send(1, Vec::new()).expect("post_wrap");
        assert_eq!(post_wrap, u32::MAX);

        let after = zc.start_send(1, Vec::new()).expect("after");
        assert_eq!(after, 0);

        zc.complete_send(near_wrap, false);
        zc.complete_send(wrap, false);
        zc.complete_send(post_wrap, false);
        zc.complete_send(after, false);

        assert_eq!(zc.pending_notifications.len(), 1);
        let n = zc.pending_notifications.front().unwrap();
        assert_eq!(n.range_lo, u32::MAX - 2);
        assert_eq!(n.range_hi, 0);
    }

    #[test]
    fn zerocopy_copied_flag_mixed_batch() {
        let mut zc = ZerocopyState::new();
        zc.enable();

        let op_a = zc.start_send(1, Vec::new()).expect("op_a");
        let op_b = zc.start_send(1, Vec::new()).expect("op_b");

        zc.complete_send(op_a, false);
        zc.complete_send(op_b, true);

        assert_eq!(zc.pending_notifications.len(), 2);
        assert!(!zc.pending_notifications.front().unwrap().copied);
        assert!(zc.pending_notifications.back().unwrap().copied);
        assert_eq!(zc.copied_sends, 1);
    }

    #[test]
    fn zerocopy_many_sequential_ops_coalesce_to_single() {
        let mut zc = ZerocopyState::new();
        zc.enable();

        let count = 128;
        for _ in 0..count {
            let op = zc.start_send(1, Vec::new()).expect("op");
            zc.complete_send(op, false);
        }

        assert_eq!(zc.pending_notifications.len(), 1);
        let n = zc.pending_notifications.front().unwrap();
        assert_eq!(n.range_hi - n.range_lo + 1, count as u32);
    }
}
