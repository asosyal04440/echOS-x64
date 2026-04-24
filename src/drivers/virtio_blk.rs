//! # echOS VirtIO Blok Aygıt Sürücüsü
//!
//! VirtIO Blok, QEMU/KVM sanallaştırma ortamında sanal disk erişimi için kullanılan
//! standart bir VirtIO aygıt tipidir. Bu sürücü, sektör okuma/yazma işlemlerini
//! DMA tamponları ve non-blocking virtqueue arayüzü üzerinden gerçekleştirir.
//!
//! ## VirtIO Blok Mimarisi
//!
//! ```text
//!   KONUK OS (echOS)                    ANA MAKİNE (QEMU)
//!  +--------------------+               +------------------+
//!  | read_sector(lba)   |               | Disk Dosyası     |
//!  |        |           |               | (.img / .qcow2)  |
//!  | [DMA Tamponu]      |               |                  |
//!  |  BlkReq  (16B)     |  VirtIO PCI   | VirtIO Blk       |
//!  |  Veri    (512B)    |<=============>| Sunucusu         |
//!  |  BlkResp (1B)      |   Virtqueue   |                  |
//!  +--------------------+               +------------------+
//!
//!  BLK_DMA_DOMAIN: IOMMU domain = DMA adresleme yalıtımı
//! ```
//!
//! ## Sektör Okuma Akışı (Non-Blocking)
//!
//! ```text
//!  1. DMA tamponu ayır (1 sayfa = 4096 byte):
//!
//!     +-----------+----------+------------+
//!     | BlkReq    | BlkResp  | Veri (512B)|
//!     | (16 byte) | (1 byte) | ...        |
//!     +-----------+----------+------------+
//!
//!  2. read_blocks_nb(lba, req, buf, resp)
//!       --> virtqueue descriptor zinciri oluşturur
//!       --> token (u16) döner: tamamlanma tanımlayıcısı
//!
//!  3. peek_used() == Some(token)? -- spin loop
//!       --> yaklaşık 5_000_000 iterasyon sonra timeout!
//!
//!  4. complete_read_blocks(token, req, buf, resp)
//!       --> veriyi doğrular, tampon içeriğini döner
//!
//!  5. buffer[lba_offset..] = dma_buf  (kopyala)
//!
//!  6. DMA tamponunu serbest bırak
//! ```
//!
//! ## DMA Domain Yalıtımı (IOMMU)
//!
//! ```text
//!  IOMMU kullanılarak her PCI aygıtı kendi DMA domain'ine atanır.
//!  Bu, farklı aygıtların birbirinin DMA bölgelerine erişmesini engeller.
//!
//!  CPU smp::current_dma_domain() ile aktif domain'i takip eder.
//!  DMA işlemi sırasında: domain geçici olarak BLK_DMA_DOMAIN'e set edilir,
//!  işlem sonrasında: eski domain geri yüklenir.
//!
//!  with_blk_domain(|| { ... })  -- bu geçiş sargısını sağlar
//! ```
//!
//! ## Hizalama ve Bellek Düzeni
//!
//! ```text
//!  DMA tamponu (tek sayfa = 4096 byte) içindeki yapı yerleşimi:
//!
//!  Adres          | İçerik
//!  ---------------+-------------------------------------------
//!  base + 0x00    | BlkReq   (hizalama: align_of::<BlkReq>())
//!  base + 0x??    | BlkResp  (hizalama: align_of::<BlkResp>())
//!  base + 0x??    | Veri tamponu (512 byte)
//!
//!  Hizalama formülü:
//!  offset = (offset + align - 1) & !(align - 1)
//! ```

use core::sync::atomic::{AtomicU32, Ordering};
use log::{error, info};
use spin::Mutex;
use virtio_drivers::device::blk::{BlkReq, BlkResp, VirtIOBlk};
use virtio_drivers::transport::pci::PciTransport;
use virtio_drivers::{BufferDirection, Hal};

use super::virtio_hal::VirtioHal;

/// Global VirtIO blok aygıt nesnesi.
/// `Option` kullanılır çünkü başlatma başarısız olabilir.
/// `Mutex<Option<...>>` ile spin kilit koruması sağlanır.
static BLK_DEV: Mutex<Option<VirtIOBlk<VirtioHal, PciTransport>>> = Mutex::new(None);

/// Blok aygıtının IOMMU DMA alan kimliği.
/// `with_blk_domain` sargısı tarafından kullanılır.
static BLK_DMA_DOMAIN: AtomicU32 = AtomicU32::new(0);

/// VirtIO blok sektörünün sabit boyutu (byte).
/// VirtIO spec'e göre tek sektör daima 512 byte'tır.
const SECTOR_SIZE: usize = 512;
const AUDIT_VIRTQ_DESC_F_NEXT: u16 = 1;
const AUDIT_VIRTQ_DESC_F_INDIRECT: u16 = 4;

#[derive(Clone, Copy, Debug)]
struct VirtqDescAudit {
    len: u32,
    flags: u16,
    next: u16,
}

fn audit_virtq_descriptor_chain(descs: &[VirtqDescAudit], head: u16) -> bool {
    if descs.is_empty() || descs.len() > 64 {
        return false;
    }
    let mut seen = 0u64;
    let mut current = head as usize;
    for _ in 0..descs.len() {
        if current >= descs.len() {
            return false;
        }
        let bit = 1u64 << current;
        if seen & bit != 0 {
            return false;
        }
        seen |= bit;
        let desc = descs[current];
        if desc.len == 0 || desc.flags & AUDIT_VIRTQ_DESC_F_INDIRECT != 0 {
            return false;
        }
        if desc.flags & AUDIT_VIRTQ_DESC_F_NEXT == 0 {
            return true;
        }
        current = desc.next as usize;
    }
    false
}

fn audit_used_ring_delta(previous: u16, current: u16, queue_size: u16) -> bool {
    queue_size != 0 && current.wrapping_sub(previous) <= queue_size
}

/// VirtIO blok sürücüsünü başlatır.
///
/// Adımlar:
/// 1. PCI Transport'tan Bus/Device/Function alınır.
/// 2. IOMMU üzerinden DMA domain kaydedilir.
/// 3. Aktif DMA domain geçici olarak blok domain'e ayarlanır.
/// 4. VirtIOBlk sürücüsü oluşturulur (virtqueue kurulumu).
/// 5. Eski DMA domain geri yüklenir.
/// 6. Kapasite bilgisi seri porta yazılır.
pub fn init(transport: PciTransport) -> bool {
    crate::serial_println!("VIRTIO BLK: baslatma basliyor");

    // IOMMU: blok aygıtı için DMA domain oluştur ve kaydet
    let df = transport.device_function();
    let domain = crate::memory::iommu_register_device(df.bus, df.device, df.function);
    BLK_DMA_DOMAIN.store(domain, Ordering::Release);

    // Mevcut domain'i sakla, blok domain'e geç
    let prev_domain = crate::cpu::smp::current_dma_domain();
    crate::cpu::smp::set_current_dma_domain(domain);

    // VirtIO blok sürücüsünü başlat (virtqueue kurulumu + özellik müzakeresi)
    let driver = match VirtIOBlk::<VirtioHal, _>::new(transport) {
        Ok(value) => value,
        Err(err) => {
            // Hata durumunda eski domain'e geri dön
            crate::cpu::smp::set_current_dma_domain(prev_domain);
            crate::serial_println!("VIRTIO BLK: baslatma hatasi: {:?}", err);
            error!("VIRTIO BLK: baslatma hatasi: {:?}", err);
            return false;
        }
    };

    // Eski domain'e geri dön
    crate::cpu::smp::set_current_dma_domain(prev_domain);

    // Toplam disk kapasitesini (sektör cinsinden) al ve raporla
    let capacity = driver.capacity();
    *BLK_DEV.lock() = Some(driver);

    crate::serial_println!("VIRTIO BLK: baslatildi, kapasite={} sektor", capacity);
    info!("VIRTIO BLK: kapasite={} sektor", capacity);
    true
}

/// Blok aygıtının DMA domain'ini geçici olarak etkinleştirir; kapatır F bitince geri yükler.
///
/// IOMMU yalıtımı için DMA işlemleri doğru domain'de yapılmalıdır.
/// Bu sargı, kaydetme ve geri yükleme işlemini otomatik olarak yönetir.
fn with_blk_domain<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    // Mevcut domain'i sakla
    let prev_domain = crate::cpu::smp::current_dma_domain();

    // Blok aygıtının domain'ine geç
    let domain = BLK_DMA_DOMAIN.load(Ordering::Acquire);
    crate::cpu::smp::set_current_dma_domain(domain);

    // İşlemi gerçekleştir
    let result = f();

    // Önceki domain'e geri dön
    crate::cpu::smp::set_current_dma_domain(prev_domain);
    result
}

/// Verilen LBA (Logical Block Address) adresinden başlayarak sektör okur.
///
/// `buffer` boş olmamalı ve `SECTOR_SIZE` (512) katı büyüklükte olmalıdır.
/// Her sektör için ayrı bir non-blocking VirtIO isteği gönderilir.
///
/// # Parametreler
/// - `lba`: Okumaya başlanacak mantıksal blok adresi
/// - `buffer`: Verinin yazılacağı bellek alanı (sektör katı büyüklükte)
///
/// # DMA Tampon Düzeni
/// Tek sayfalık (4096 byte) DMA tamponu:
/// `[BlkReq (hizalı)] [BlkResp (hizalı)] [Sektör Verisi (512B)]`
pub fn read_sector(lba: u64, buffer: &mut [u8]) -> Result<(), &'static str> {
    with_blk_domain(|| {
        crate::serial_println!("VIRTIO BLK: okuma lba={} boyut={}", lba, buffer.len());

        // Tampon boyutu kontrolü: boş veya 512'nin katı olmalı
        if buffer.is_empty() || buffer.len() % SECTOR_SIZE != 0 {
            crate::serial_println!("VIRTIO BLK: gecersiz tampon boyutu");
            return Err("Invalid buffer size");
        }

        let mut guard = BLK_DEV.lock();
        let Some(device) = guard.as_mut() else {
            crate::serial_println!("VIRTIO BLK: aygit baslatilmamis");
            return Err("Device not initialized");
        };

        let sectors = buffer.len() / SECTOR_SIZE;

        // DMA tamponunu ayır (1 sayfa = BlkReq + BlkResp + Veri için yeterli)
        let (paddr, vaddr) = <VirtioHal as Hal>::dma_alloc(1, BufferDirection::Both);
        if paddr == 0 {
            crate::serial_println!("VIRTIO BLK: DMA tahsis hatasi");
            return Err("Disk Error");
        }

        let base = vaddr.as_ptr() as usize;

        // DMA tamponu içinde hizalanmış yapı adresleri hesapla
        let mut offset = 0usize;

        // BlkReq hizalaması
        offset =
            (offset + core::mem::align_of::<BlkReq>() - 1) & !(core::mem::align_of::<BlkReq>() - 1);
        let req_ptr = (base + offset) as *mut BlkReq;
        offset += core::mem::size_of::<BlkReq>();

        // BlkResp hizalaması
        offset = (offset + core::mem::align_of::<BlkResp>() - 1)
            & !(core::mem::align_of::<BlkResp>() - 1);
        let resp_ptr = (base + offset) as *mut BlkResp;
        offset += core::mem::size_of::<BlkResp>();

        // Bir sektör için yer kaldığından emin ol (sayfa sınırı kontrolü)
        if offset + SECTOR_SIZE > crate::memory::PAGE_SIZE {
            unsafe { <VirtioHal as Hal>::dma_dealloc(paddr, vaddr, 1) };
            crate::serial_println!("VIRTIO BLK: DMA tamponu cok kucuk");
            return Err("Disk Error");
        }

        // Veri tamponu slice'ı oluştur (DMA tamponundaki ham bellek alanı)
        let dma_buf =
            unsafe { core::slice::from_raw_parts_mut((base + offset) as *mut u8, SECTOR_SIZE) };

        // Her sektör için ayrı non-blocking VirtIO isteği gönder
        for i in 0..sectors {
            // İstek ve yanıt yapılarını temizle
            unsafe {
                core::ptr::write(req_ptr, BlkReq::default());
                core::ptr::write(resp_ptr, BlkResp::default());
            }

            // Non-blocking okuma isteği gönder; token = tamamlanma tanımlayıcısı
            let token = match unsafe {
                device.read_blocks_nb(
                    lba as usize + i,
                    unsafe { &mut *req_ptr },
                    dma_buf,
                    unsafe { &mut *resp_ptr },
                )
            } {
                Ok(value) => value,
                Err(_) => {
                    unsafe { <VirtioHal as Hal>::dma_dealloc(paddr, vaddr, 1) };
                    crate::serial_println!("VIRTIO BLK: okuma hatasi lba={}", lba + i as u64);
                    return Err("Disk Error");
                }
            };

            // Token kullanıldı bildirimini bekle (spin loop, max 5_000_000 döngü)
            // peek_used(): completed (used ring) kuyruğunu kontrol eder
            let mut spins: u32 = 0;
            while device.peek_used() != Some(token) {
                if spins > 5_000_000 {
                    // Zaman aşımı: DMA tamponu serbest bırak ve hata döndür
                    unsafe { <VirtioHal as Hal>::dma_dealloc(paddr, vaddr, 1) };
                    crate::serial_println!("VIRTIO BLK: zaman asimi lba={}", lba + i as u64);
                    return Err("Timeout");
                }
                spins = spins.wrapping_add(1);
                core::hint::spin_loop();
            }

            // İsteği tamamla: yanıt ve veri doğrulanır
            unsafe {
                device
                    .complete_read_blocks(token, &*req_ptr, dma_buf, &mut *resp_ptr)
                    .map_err(|_| {
                        unsafe { <VirtioHal as Hal>::dma_dealloc(paddr, vaddr, 1) };
                        crate::serial_println!("VIRTIO BLK: okuma hatasi lba={}", lba + i as u64);
                        "Disk Error"
                    })?;
            }

            // DMA tamponundaki sektör verisini hedef belleğe kopyala
            let start = i * SECTOR_SIZE;
            let end = start + SECTOR_SIZE;
            buffer[start..end].copy_from_slice(dma_buf);
        }

        // DMA tamponunu serbest bırak
        unsafe { <VirtioHal as Hal>::dma_dealloc(paddr, vaddr, 1) };

        crate::serial_println!(
            "VIRTIO BLK: okuma tamamlandi lba={} sektor={}",
            lba,
            sectors
        );
        Ok(())
    })
}

/// Verilen LBA adresinden başlayarak sektör yazar.
///
/// `buffer` boş olmamalı ve `SECTOR_SIZE` (512) katı büyüklükte olmalıdır.
/// Her sektör için ayrı blocking `write_blocks` isteği gönderilir.
///
/// # Parametreler
/// - `lba`: Yazılacak mantıksal blok adresi
/// - `buffer`: Yazılacak veri (sektör katı büyüklükte)
///
/// # Okumadan Fark
/// Yazma işlemi blocking `write_blocks` kullanır (non-blocking değil).
/// Bu, birden fazla VirtIO descriptor aşaması gerektirmez; senkron I/O yeterlidir.
pub fn write_sector(lba: u64, buffer: &[u8]) -> Result<(), &'static str> {
    with_blk_domain(|| {
        crate::serial_println!("VIRTIO BLK: yazma lba={} boyut={}", lba, buffer.len());

        // Tampon boyutu kontrolü
        if buffer.is_empty() || buffer.len() % SECTOR_SIZE != 0 {
            crate::serial_println!("VIRTIO BLK: gecersiz tampon boyutu");
            return Err("Invalid buffer size");
        }

        let mut guard = BLK_DEV.lock();
        let Some(device) = guard.as_mut() else {
            crate::serial_println!("VIRTIO BLK: aygit baslatilmamis");
            return Err("Device not initialized");
        };

        let sectors = buffer.len() / SECTOR_SIZE;

        // Her sektörü ayrı ayrı doğrudan yaz (blocking)
        for i in 0..sectors {
            let start = i * SECTOR_SIZE;
            let end = start + SECTOR_SIZE;

            device
                .write_blocks(lba as usize + i, &buffer[start..end])
                .map_err(|_| {
                    crate::serial_println!("VIRTIO BLK: yazma hatasi lba={}", lba + i as u64);
                    "Disk Error"
                })?;
        }

        crate::serial_println!(
            "VIRTIO BLK: yazma tamamlandi lba={} sektor={}",
            lba,
            sectors
        );
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtio_audit_rejects_circular_descriptor_chain() {
        let descs = [
            VirtqDescAudit {
                len: 16,
                flags: AUDIT_VIRTQ_DESC_F_NEXT,
                next: 1,
            },
            VirtqDescAudit {
                len: 512,
                flags: AUDIT_VIRTQ_DESC_F_NEXT,
                next: 0,
            },
        ];

        assert!(!audit_virtq_descriptor_chain(&descs, 0));
    }

    #[test]
    fn virtio_audit_accepts_three_descriptor_blk_chain() {
        let descs = [
            VirtqDescAudit {
                len: 16,
                flags: AUDIT_VIRTQ_DESC_F_NEXT,
                next: 1,
            },
            VirtqDescAudit {
                len: SECTOR_SIZE as u32,
                flags: AUDIT_VIRTQ_DESC_F_NEXT,
                next: 2,
            },
            VirtqDescAudit {
                len: 1,
                flags: 0,
                next: 0,
            },
        ];

        assert!(audit_virtq_descriptor_chain(&descs, 0));
    }

    #[test]
    fn virtio_audit_used_ring_delta_accepts_wrap_and_rejects_jump() {
        assert!(audit_used_ring_delta(u16::MAX, 0, 8));
        assert!(!audit_used_ring_delta(0, 9, 8));
    }
}
