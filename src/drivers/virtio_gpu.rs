//! # VirtIO GPU Sürücüsü
//!
//! Bu modül, VirtIO GPU protokolü üzerinden donanım hızlandırmalı
//! 3D grafik (VirGL) desteği sağlar.
//!
//! ## VirtIO GPU Protokol Katmanları
//!
//! ```
//!  ┌───────────────────────────────────────────────────────┐
//!  │  Uygulama: hardware_clear_amber(), drm_submit_3d()    │
//!  ├───────────────────────────────────────────────────────┤
//!  │  VirGL Komut Kodlayıcı (VirglEncoder)                 │
//!  │  create_surface → set_framebuffer → clear            │
//!  ├───────────────────────────────────────────────────────┤
//!  │  VirtIO GPU Kontrol Komutları (CTX_CREATE, SUBMIT_3D) │
//!  ├───────────────────────────────────────────────────────┤
//!  │  VirtIO Kuyruk (Virtqueue) - TRB halkaları            │
//!  ├───────────────────────────────────────────────────────┤
//!  │  PCI/PCIe MMIO Yapılandırması                         │
//!  └───────────────────────────────────────────────────────┘
//! ```
//!
//! ## VirtIO PCI Yetenekleri (Capabilities)
//!
//! VirtIO cihazı, PCI yapılandırma uzayındaki yetenek listesi (CAP list)
//! ile kendini tanıtır. Her capability şunları içerir:
//! - COMMON_CFG: Kuyruk yönetimi, özellik müzakeresi
//! - NOTIFY_CFG: Kapı zili (doorbell) register'ları
//! - ISR_CFG: Interrupt durum register'ı
//! - DEVICE_CFG: GPU'ya özgü yapılandırma
//!
//! ## Virtqueue Mekanizması
//!
//! ```
//!  Descriptor Table (tanımlayıcılar)
//!  ┌────────┬─────────────────────────────┐
//!  │ desc[0]│ addr=istek_buf, len=N, OUT  │ ← sürücü yazar
//!  │ desc[1]│ addr=cevap_buf, len=M, IN   │ ← cihaz yazar
//!  └────────┴─────────────────────────────┘
//!
//!  Available Ring (sürücü → cihaz)
//!  avail.ring[idx % size] = 0  (desc zinciri başı)
//!  avail.idx++  → cihaza bildir
//!
//!  Used Ring (cihaz → sürücü)
//!  cihaz used.idx'i artırınca transfer tamamdır
//! ```
//!
//! ## VirGL 3D Komut Akışı
//!
//! ```
//! 1. PCI yeteneklerini oku
//! 2. Özellikleri müzakere et (VirGL desteği?)
//! 3. Kontrol kuyruğunu kur
//! 4. CTX_CREATE → GPU bağlamı oluştur
//! 5. RESOURCE_CREATE_3D → framebuffer kaynağı
//! 6. VirGL komutları gönder (SUBMIT_3D)
//! 7. SET_SCANOUT + RESOURCE_FLUSH → ekrana bas
//! ```

use alloc::vec::Vec;
use core::ptr::{read_volatile, write_volatile};
use spin::Mutex;

use crate::linux_glue::PciDev;

// ---------------------------------------------------------------------------
// VirtIO PCI Yetenek Sabitleri
// CAP_ID = 0x09: Bu ID'ye sahip PCI capability'ler VirtIO'ya aittir
// ---------------------------------------------------------------------------

const VIRTIO_PCI_CAP_ID: u8 = 0x09;
const VIRTIO_PCI_CAP_COMMON_CFG: u8 = 1; // Ortak yapılandırma
const VIRTIO_PCI_CAP_NOTIFY_CFG: u8 = 2; // Bildirim (doorbell)
const VIRTIO_PCI_CAP_ISR_CFG: u8 = 3; // Interrupt durum
const VIRTIO_PCI_CAP_DEVICE_CFG: u8 = 4; // Aygıta özgü yapılandırma

// ---------------------------------------------------------------------------
// VirtIO Durum Biti Sabitleri
// Cihaz başlatma sırası: ACKNOWLEDGE → DRIVER → FEATURES_OK → DRIVER_OK
// Bu sıra VirtIO spesifikasyonunda zorunludur.
// ---------------------------------------------------------------------------

const VIRTIO_STATUS_ACKNOWLEDGE: u8 = 1; // Sürücü cihazı tanıdı
const VIRTIO_STATUS_DRIVER: u8 = 2; // Sürücü cihazla konuşabilir
const VIRTIO_STATUS_DRIVER_OK: u8 = 4; // Sürücü hazır
const VIRTIO_STATUS_FEATURES_OK: u8 = 8; // Özellik müzakeresi tamamlandı

// ---------------------------------------------------------------------------
// VirtIO GPU Özellik Bitleri
// VirGL: 3D donanım hızlandırma desteği (bit 0)
// ---------------------------------------------------------------------------

const VIRTIO_GPU_F_VIRGL: u64 = 1 << 0;

// ---------------------------------------------------------------------------
// VirtIO GPU Komut Kodları
// 0x01xx: 2D komutlar  0x02xx: 3D (VirGL) komutlar
// 0x11xx: Başarı yanıtları
// ---------------------------------------------------------------------------

const VIRTIO_GPU_CMD_GET_DISPLAY_INFO: u32 = 0x0100; // Ekran bilgisi al
const VIRTIO_GPU_CMD_GET_CAPSET: u32 = 0x0108; // VirGL yetenek seti al
const VIRTIO_GPU_CMD_SET_SCANOUT: u32 = 0x0103; // Tarama kaynağını ayarla
const VIRTIO_GPU_CMD_RESOURCE_FLUSH: u32 = 0x0104; // Kaynağı ekrana yansıt
const VIRTIO_GPU_CMD_CTX_CREATE: u32 = 0x0200; // VirGL bağlamı oluştur
const VIRTIO_GPU_CMD_RESOURCE_CREATE_3D: u32 = 0x0202; // 3D kaynak oluştur
const VIRTIO_GPU_CMD_SUBMIT_3D: u32 = 0x0205; // VirGL komut paketi gönder
const VIRTIO_GPU_RESP_OK_NODATA: u32 = 0x1100; // Başarı, veri yok
const VIRTIO_GPU_RESP_OK_CAPSET: u32 = 0x1107; // Yetenek seti yanıtı
const VIRTIO_GPU_RESP_OK_DISPLAY_INFO: u32 = 0x1101; // Ekran bilgisi yanıtı
const VIRTIO_GPU_FLAG_FENCE: u32 = 1; // Çit (senkronizasyon işareti)

const VIRTIO_GPU_MAX_SCANOUTS: usize = 16; // Maksimum ekran çıkışı sayısı

// VirGL Yetenek Seti kimlikleri
const VIRTIO_GPU_CAPSET_VIRGL: u32 = 1; // VirGL v1
const VIRTIO_GPU_CAPSET_VIRGL2: u32 = 2; // VirGL v2 (tercih edilir)

// B8G8R8A8_UNORM: Mavi-Yeşil-Kırmızı-Alfa, normalize edilmemiş
const VIRTIO_GPU_FORMAT_B8G8R8A8_UNORM: u32 = 67;

// ---------------------------------------------------------------------------
// VirGL Komut Kodları
// VirGL komutları 32-bit kelimeler halinde kodlanır.
// Her komut başlığı: opcode | (payload_len << 16)
// ---------------------------------------------------------------------------

const VIRGL_CCMD_CREATE_OBJECT: u32 = 1; // GPU nesnesi oluştur
const VIRGL_CCMD_SET_FRAMEBUFFER_STATE: u32 = 2; // Framebuffer'ı ayarla
const VIRGL_CCMD_CLEAR: u32 = 3; // Renk tamponu temizle
const VIRGL_OBJECT_SURFACE: u32 = 1; // Nesne türü: yüzey
const VIRGL_CLEAR_COLOR: u32 = 1; // Renk tamponu temizleme maskesi

/// VirtIO PCI Ortak Yapılandırma Register'ları
///
/// `#[repr(C)]`: C bellek düzeni güvencesi - MMIO erişimi için şarttır.
/// Bu yapı doğrudan donanım registerlarına overlay edilir; padding olmaz.
///
/// ```
/// MMIO adresi + 0x00 → device_feature_select
/// MMIO adresi + 0x04 → device_feature
/// ...
/// MMIO adresi + 0x68 → queue_used (64-bit)
/// ```
#[repr(C)]
struct VirtioPciCommonCfg {
    device_feature_select: u32, // Hangi 32-bitlik özellik grubunu seçer (0 veya 1)
    device_feature: u32,        // Seçilen gruptaki cihaz özellikleri
    driver_feature_select: u32, // Sürücü özellik grubu seçimi
    driver_feature: u32,        // Sürücünün istediği özellikler
    msix_config: u16,           // MSI-X yapılandırması
    num_queues: u16,            // Bu cihazın kuyruk sayısı
    device_status: u8,          // Cihaz durum byte'ı (yukarıdaki sabitler)
    config_generation: u8,      // Yapılandırma değişim sayacı
    queue_select: u16,          // Hangi kuyruğu yapılandıracağımızı seçer
    queue_size: u16,            // Seçili kuyruğun maksimum boyutu
    queue_msix_vector: u16,     // Kuyruk MSI-X vektörü
    queue_enable: u16,          // 1 = kuyruk aktif
    queue_notify_off: u16,      // Bildirim register ofseti
    queue_desc: u64,            // Tanımlayıcı tablosu fiziksel adresi
    queue_avail: u64,           // Kullanılabilir halka fiziksel adresi
    queue_used: u64,            // Kullanılmış halka fiziksel adresi
}

/// VirtIO GPU Kontrol Başlığı
///
/// Tüm GPU komutları bu başlıkla başlar. `type_` komut kodunu,
/// `fence_id` senkronizasyon çit numarasını belirtir.
#[repr(C)]
struct VirtioGpuCtrlHdr {
    type_: u32,    // Komut/yanıt kodu (yukarıdaki sabitler)
    flags: u32,    // Çit bayrağı ve diğerleri
    fence_id: u64, // Senkronizasyon için benzersiz ID
    ctx_id: u32,   // VirGL bağlam kimliği
    padding: u32,  // 8-byte hizalama için dolgu
}

/// GPU Dikdörtgen Alanı (koordinat + boyut)
#[repr(C)]
#[derive(Clone, Copy)]
struct VirtioGpuRect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

/// Tek ekran çıkışı bilgisi
#[repr(C)]
#[derive(Clone, Copy)]
struct VirtioGpuDisplayOne {
    r: VirtioGpuRect, // Ekranın dikdörtgen alanı
    enabled: u32,     // 0 = devre dışı, 1 = aktif
    flags: u32,
}

/// GET_DISPLAY_INFO yanıtı: tüm ekranların bilgisi
#[repr(C)]
struct VirtioGpuRespDisplayInfo {
    hdr: VirtioGpuCtrlHdr,
    pmodes: [VirtioGpuDisplayOne; VIRTIO_GPU_MAX_SCANOUTS],
}

/// GET_CAPSET isteği: belirli bir yetenek setini ister
#[repr(C)]
struct VirtioGpuGetCapset {
    hdr: VirtioGpuCtrlHdr,
    capset_id: u32,      // İstenen yetenek seti (VirGL veya VirGL2)
    capset_version: u32, // İstenen sürüm (0 = en son)
}

/// GET_CAPSET yanıtı: yetenek seti meta verisi
#[repr(C)]
struct VirtioGpuRespCapset {
    hdr: VirtioGpuCtrlHdr,
    capset_id: u32,
    capset_version: u32,
    size: u32, // Yetenek verisinin byte boyutu
    padding: u32,
}

/// CTX_CREATE komutu: VirGL 3D bağlamı oluşturur
#[repr(C)]
struct VirtioGpuCtxCreate {
    hdr: VirtioGpuCtrlHdr,
    ctx_id: u32, // Yeni bağlamın kimliği
    nlen: u32,   // Bağlam adı uzunluğu (0 = adsız)
}

/// RESOURCE_CREATE_3D komutu: GPU belleğinde bir kaynak (texture/buffer) oluşturur
#[repr(C)]
struct VirtioGpuResourceCreate3d {
    hdr: VirtioGpuCtrlHdr,
    resource_id: u32, // Kaynağa atanan kimlik
    format: u32,      // Piksel formatı (B8G8R8A8 vb.)
    width: u32,
    height: u32,
    depth: u32,      // 2D için 1
    array_size: u32, // Dizi dokusu için, normal için 1
    last_level: u32, // Mipmap seviyesi (0 = mip yok)
    nr_samples: u32, // Çoklu örnekleme sayısı
    flags: u32,
}

/// SET_SCANOUT komutu: bir kaynağı ekran çıkışına bağlar
#[repr(C)]
struct VirtioGpuSetScanout {
    hdr: VirtioGpuCtrlHdr,
    r: VirtioGpuRect, // Görüntülenecek kaynak bölgesi
    scanout_id: u32,  // Hedef ekran çıkışı (0 = birincil)
    resource_id: u32, // Bağlanacak kaynak
}

/// RESOURCE_FLUSH komutu: GPU belleğini ekrana yansıtır
#[repr(C)]
struct VirtioGpuResourceFlush {
    hdr: VirtioGpuCtrlHdr,
    r: VirtioGpuRect, // Yenilecek bölge
    resource_id: u32,
    padding: u32,
}

/// SUBMIT_3D komutu başlığı: VirGL komut paketi paketi gönderir
///
/// Gerçek VirGL komutları bu başlığın hemen arkasına eklenir:
/// [VirtioGpuCmdSubmit3d | virgl_cmds...]
#[repr(C)]
struct VirtioGpuCmdSubmit3d {
    hdr: VirtioGpuCtrlHdr,
    size: u32, // VirGL komutlarının toplam byte boyutu
    padding: u32,
}

/// VirGL Komut Kodlayıcı
///
/// VirGL komutları 32-bit kelimeler dizisi olarak gönderilir.
/// Her komutun başlığı: `opcode | (payload_word_count << 16)`
///
/// Örnek:
/// ```
/// CREATE_OBJECT (opcode=1) + 8 payload kelimesi:
/// başlık = 1 | (8 << 16) = 0x00080001
/// ```
struct VirglEncoder {
    words: Vec<u32>,
}

impl VirglEncoder {
    fn new() -> Self {
        Self { words: Vec::new() }
    }

    /// Bir VirGL komutu ekler.
    ///
    /// `opcode`: komut kodu
    /// `payload`: komutun argüman kelimeleri
    fn push_cmd(&mut self, opcode: u32, payload: &[u32]) {
        let header = opcode | ((payload.len() as u32) << 16);
        self.words.push(header);
        self.words.extend_from_slice(payload);
    }

    /// Kelime dizisini little-endian byte dizisine dönüştürür
    fn into_bytes(self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.words.len() * 4);
        for word in self.words {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes
    }
}

/// Virtqueue Tanımlayıcısı (Descriptor)
///
/// Her tanımlayıcı bir bellek tamponunu tanımlar.
/// Bayrak bitleri:
/// - bit 0 (NEXT): next alanı geçerli, zincir devam ediyor
/// - bit 1 (WRITE): tampon cihaz tarafından yazılacak (IN tamponu)
#[repr(C)]
struct VirtqDesc {
    addr: u64,  // Tampon fiziksel adresi
    len: u32,   // Tampon boyutu (byte)
    flags: u16, // NEXT, WRITE bayrakları
    next: u16,  // Zincirdeki sonraki tanımlayıcı indeksi
}

/// Virtqueue Kullanılabilir Halkası
///
/// Sürücü bu halkaya yeni tanımlayıcı zincirleri ekler.
/// `idx` monoton artan sayaçtır; cihaz son gördüğü idx'e kadar işler.
#[repr(C)]
struct VirtqAvail {
    flags: u16,      // Bildirim bastırma bayrağı
    idx: u16,        // Sürücünün eklediği sonraki giriş indeksi
    ring: [u16; 8],  // Tanımlayıcı zinciri başlangıç indeksleri
    used_event: u16, // Kesme optimizasyonu için kullanılan indeks
}

/// Kullanılmış Halka Girdisi
#[repr(C)]
struct VirtqUsedElem {
    id: u32,  // Kullanılan descriptor zinciri başlangıç indeksi
    len: u32, // Cihazın toplam yazdığı byte sayısı
}

/// Virtqueue Kullanılmış Halkası
///
/// Cihaz işlemi bitirince bu halkaya sonucu ekler.
/// `idx` monoton artar; sürücü yeni girdileri burada bulur.
#[repr(C)]
struct VirtqUsed {
    flags: u16, // Bildirim bastırma bayrağı
    idx: u16,   // Cihazın eklediği sonraki giriş indeksi
    ring: [VirtqUsedElem; 8],
    avail_event: u16, // Kesme optimizasyonu
}

/// Tam Virtqueue: desc + avail + used halkaları + meta veriler
struct VirtQueue {
    desc: *mut VirtqDesc,   // Tanımlayıcı tablosu pointer'ı
    avail: *mut VirtqAvail, // Kullanılabilir halka pointer'ı
    used: *mut VirtqUsed,   // Kullanılmış halka pointer'ı
    size: u16,              // Kuyruk kapasitesi
    last_used: u16,         // Son işlenen kullanılmış indeks
    notify_off: u16,        // Bildirim register ofseti (çarpan uygulanmadan önce)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VirtioGpuDoorbellSnapshot {
    pub queue_index: u16,
    pub queue_notify_off: u16,
    pub notify_off_multiplier: u32,
    pub notify_mmio_offset: u32,
    pub avail_idx: u16,
    pub used_idx: u16,
}

/// VirtIO GPU Cihaz Durumu
///
/// Tüm GPU durumu tek global yapıda tutulur.
/// `unsafe impl Send`: raw pointer içerdiği için Rust bunu otomatik yapmaz,
/// ancak yalnızca kilitleme altında erişileceği garanti edilir.
struct VirtioGpuDevice {
    common: *mut VirtioPciCommonCfg, // Ortak VirtIO register'ları
    notify: *mut u8,                 // Bildirim (doorbell) tabanı
    notify_off_multiplier: u32,      // Bildirim ofseti çarpanı
    isr: *mut u8,                    // Interrupt durumu
    device_cfg: *mut u8,             // GPU'ya özgü yapılandırma
    features: u64,                   // Müzakere edilen özellikler
    virgl: bool,                     // VirGL destekleniyor mu
    capset_id: u32,                  // Aktif VirGL yetenek seti ID'si
    capset_version: u32,             // Yetenek seti sürümü
    capset_size: u32,                // Yetenek verisi boyutu
    capset_data: *mut u8,            // Yetenek verisi tamponu
    ctx_id: u32,                     // VirGL bağlam kimliği
    resource_id: u32,                // Sonraki kaynak kimliği
    surface_handle: u32,             // VirGL yüzey tanıtıcısı
    fence_counter: u64,              // Senkronizasyon çit sayacı
    ctrl_queue: VirtQueue,           // Kontrol kuyruğu
}

unsafe impl Send for VirtioGpuDevice {}

/// Global GPU cihaz örneği (Mutex ile korunur)
static GPU_DEVICE: Mutex<Option<VirtioGpuDevice>> = Mutex::new(None);

/// VirtIO GPU'yu PCI cihazından başlatır.
///
/// # Başlatma Adımları
/// 1. PCI capability listesini tara, MMIO adreslerini bul
/// 2. VirtIO özelliklerini müzakere et (VirGL kontrolü)
/// 3. Kontrol kuyruğunu kur (descriptor + avail + used halkaları)
/// 4. VirGL yetenek setini sorgula
/// 5. GPU bağlamı oluştur
/// 6. İlk 3D kaynağı oluştur
/// 7. DRIVER_OK durumunu bildir
pub unsafe fn init_from_pci(dev: *mut PciDev) -> bool {
    if dev.is_null() {
        return false;
    }
    let (common, notify, notify_mul, isr, device_cfg) = match read_pci_caps(dev) {
        Some(data) => data,
        None => return false,
    };
    let mut gpu = VirtioGpuDevice {
        common,
        notify,
        notify_off_multiplier: notify_mul,
        isr,
        device_cfg,
        features: 0,
        virgl: false,
        capset_id: 0,
        capset_version: 0,
        capset_size: 0,
        capset_data: core::ptr::null_mut(),
        ctx_id: 1,
        resource_id: 1,
        surface_handle: 1,
        fence_counter: 1,
        ctrl_queue: unsafe { core::mem::zeroed() },
    };
    if !negotiate_features(&mut gpu) {
        return false;
    }
    if !setup_ctrl_queue(&mut gpu) {
        return false;
    }
    let capset = query_capset(&mut gpu);
    if capset == 0 {
        return false;
    }
    gpu.capset_id = capset;
    if !create_context(&mut gpu) {
        return false;
    }
    if !create_resource_3d(&mut gpu, 64, 64) {
        return false;
    }
    let status = read_volatile(&(*gpu.common).device_status);
    write_volatile(
        &mut (*gpu.common).device_status,
        status | VIRTIO_STATUS_DRIVER_OK,
    );
    *GPU_DEVICE.lock() = Some(gpu);
    true
}

fn queue_doorbell_snapshot(gpu: &VirtioGpuDevice, queue_index: u16) -> VirtioGpuDoorbellSnapshot {
    let q = &gpu.ctrl_queue;
    let avail_idx = unsafe { core::ptr::read_volatile(core::ptr::addr_of!((*q.avail).idx)) };
    let used_idx = unsafe { core::ptr::read_volatile(core::ptr::addr_of!((*q.used).idx)) };
    VirtioGpuDoorbellSnapshot {
        queue_index,
        queue_notify_off: q.notify_off,
        notify_off_multiplier: gpu.notify_off_multiplier,
        notify_mmio_offset: q.notify_off as u32 * gpu.notify_off_multiplier,
        avail_idx,
        used_idx,
    }
}

fn ring_queue_notify(gpu: &VirtioGpuDevice, snapshot: VirtioGpuDoorbellSnapshot) {
    let notify_ptr = unsafe { gpu.notify.add(snapshot.notify_mmio_offset as usize) as *mut u16 };
    unsafe { write_volatile(notify_ptr, snapshot.queue_index) };
}

pub fn gpu_doorbell_snapshot() -> Option<VirtioGpuDoorbellSnapshot> {
    let guard = GPU_DEVICE.lock();
    let gpu = guard.as_ref()?;
    Some(queue_doorbell_snapshot(gpu, 0))
}

/// PCI capability listesini okur, VirtIO MMIO adreslerini çıkarır.
///
/// PCI CAP listesi sözdizimi:
/// ```
/// config[0x34] → ilk CAP ofseti
/// config[cap_ptr + 0] = cap_id   (0x09 = VirtIO)
/// config[cap_ptr + 1] = next_ptr (zincir devam)
/// config[cap_ptr + 3] = cfg_type (COMMON/NOTIFY/ISR/DEVICE)
/// config[cap_ptr + 4] = bar      (hangi BAR'dan)
/// config[cap_ptr + 8] = offset   (BAR içi ofset)
/// ```
unsafe fn read_pci_caps(
    dev: *mut PciDev,
) -> Option<(*mut VirtioPciCommonCfg, *mut u8, u32, *mut u8, *mut u8)> {
    let (bus, device, function) = unsafe {
        let priv_ptr = (*dev).driver_data as *const crate::linux_glue::LinuxPciPriv;
        if priv_ptr.is_null() {
            return None;
        }
        ((*priv_ptr).bus, (*priv_ptr).device, (*priv_ptr).function)
    };
    // PCI Status register bit 4: Capabilities listesi var mı?
    let status = read_config_u16(bus, device, function, 0x06);
    if (status & 0x10) == 0 {
        return None;
    }
    let mut cap_ptr = read_config_u8(bus, device, function, 0x34);
    let mut common = None;
    let mut notify = None;
    let mut notify_mul = 0u32;
    let mut isr = None;
    let mut device_cfg = None;
    while cap_ptr != 0 {
        let cap_id = read_config_u8(bus, device, function, cap_ptr);
        let next = read_config_u8(bus, device, function, cap_ptr + 1);
        if cap_id == VIRTIO_PCI_CAP_ID {
            let cfg_type = read_config_u8(bus, device, function, cap_ptr + 3);
            let bar = read_config_u8(bus, device, function, cap_ptr + 4);
            let offset = read_config_u32(bus, device, function, cap_ptr + 8);
            let length = read_config_u32(bus, device, function, cap_ptr + 12);
            let bar_base = get_bar_base(dev, bar)?;
            let base = (bar_base + offset as u64) as usize;
            match cfg_type {
                VIRTIO_PCI_CAP_COMMON_CFG => {
                    common = Some(base as *mut VirtioPciCommonCfg);
                }
                VIRTIO_PCI_CAP_NOTIFY_CFG => {
                    notify = Some(base as *mut u8);
                    // notify_off_multiplier: her kuyruğun bildirim register'ı
                    // adres = notify_base + queue_notify_off * notify_off_multiplier
                    notify_mul = read_config_u32(bus, device, function, cap_ptr + 16);
                }
                VIRTIO_PCI_CAP_ISR_CFG => {
                    if length > 0 {
                        isr = Some(base as *mut u8);
                    }
                }
                VIRTIO_PCI_CAP_DEVICE_CFG => {
                    device_cfg = Some(base as *mut u8);
                }
                _ => {}
            }
        }
        cap_ptr = next;
    }
    Some((common?, notify?, notify_mul, isr?, device_cfg?))
}

/// VirtIO özelliklerini müzakere eder.
///
/// # Protokol
/// 1. Cihazı sıfırla (device_status = 0)
/// 2. ACKNOWLEDGE + DRIVER bitlerini set et
/// 3. Cihazın sunduğu özellikleri oku (64-bit = 2 × 32-bit)
/// 4. VirGL özelliğini seç (diğerlerini reddet)
/// 5. Seçilen özellikleri cihaza yaz
/// 6. FEATURES_OK bitini set et
/// 7. Cihazın FEATURES_OK'u koruduğunu doğrula
unsafe fn negotiate_features(gpu: &mut VirtioGpuDevice) -> bool {
    let common = &mut *gpu.common;
    write_volatile(&mut common.device_status, 0);
    write_volatile(
        &mut common.device_status,
        VIRTIO_STATUS_ACKNOWLEDGE | VIRTIO_STATUS_DRIVER,
    );
    // 64-bit özellikler iki 32-bit register çiftiyle okunur
    write_volatile(&mut common.device_feature_select, 0);
    let low = read_volatile(&common.device_feature) as u64;
    write_volatile(&mut common.device_feature_select, 1);
    let high = read_volatile(&common.device_feature) as u64;
    let features = low | (high << 32);
    gpu.features = features;
    let enabled = features & VIRTIO_GPU_F_VIRGL;
    gpu.virgl = enabled != 0;
    write_volatile(&mut common.driver_feature_select, 0);
    write_volatile(&mut common.driver_feature, (enabled & 0xFFFF_FFFF) as u32);
    write_volatile(&mut common.driver_feature_select, 1);
    write_volatile(
        &mut common.driver_feature,
        ((enabled >> 32) & 0xFFFF_FFFF) as u32,
    );
    let status = read_volatile(&common.device_status);
    write_volatile(
        &mut common.device_status,
        status | VIRTIO_STATUS_FEATURES_OK,
    );
    // Cihaz FEATURES_OK'u silerse müzakere başarısız demektir
    let status = read_volatile(&common.device_status);
    (status & VIRTIO_STATUS_FEATURES_OK) != 0 && gpu.virgl
}

/// Kontrol kuyruğunu (queue 0) kurar.
///
/// # Virtqueue Bellek Düzeni
///
/// ```
/// [Descriptor Table        ] ← 4KB hizalı, boyut = 16 * queue_size byte
/// [Available Ring          ] ← descriptor tablosunun hemen ardından
/// [padding to 4KB boundary ]
/// [Used Ring               ] ← bir sonraki 4KB sınırına hizalı
/// ```
unsafe fn setup_ctrl_queue(gpu: &mut VirtioGpuDevice) -> bool {
    let common = &mut *gpu.common;
    // Kuyruk 0'ı seç
    write_volatile(&mut common.queue_select, 0);
    let size = read_volatile(&common.queue_size);
    if size == 0 {
        return false;
    }
    // Maksimum 8 girdi kullan (basit implementasyon için yeterli)
    let queue_size = 8u16.min(size);
    let (desc, avail, used) = match allocate_queue(queue_size) {
        Some(queue) => queue,
        None => return false,
    };
    // Fiziksel adresler cihaza bildirilir
    write_volatile(&mut common.queue_size, queue_size);
    write_volatile(&mut common.queue_desc, desc as u64);
    write_volatile(&mut common.queue_avail, avail as u64);
    write_volatile(&mut common.queue_used, used as u64);
    write_volatile(&mut common.queue_enable, 1);
    let notify_off = read_volatile(&common.queue_notify_off);
    gpu.ctrl_queue = VirtQueue {
        desc,
        avail,
        used,
        size: queue_size,
        last_used: 0,
        notify_off,
    };
    true
}

/// Virtqueue için bellek tahsis eder.
///
/// Tüm kuyruk belleği tek büyük blok olarak tahsis edilir,
/// parçalar uygun ofsetlerde konumlandırılır.
unsafe fn allocate_queue(size: u16) -> Option<(*mut VirtqDesc, *mut VirtqAvail, *mut VirtqUsed)> {
    let desc_bytes = core::mem::size_of::<VirtqDesc>() * size as usize;
    let avail_bytes = core::mem::size_of::<u16>() * 3 + core::mem::size_of::<u16>() * size as usize;
    let used_bytes =
        core::mem::size_of::<VirtqUsedElem>() * size as usize + core::mem::size_of::<u16>() * 3;
    let total = align_up(desc_bytes + avail_bytes + used_bytes, 4096);
    let ptr = crate::allocator::heap_alloc(total) as *mut u8;
    if ptr.is_null() {
        return None;
    }
    core::ptr::write_bytes(ptr, 0, total);
    let desc = ptr as *mut VirtqDesc;
    let avail = ptr.add(desc_bytes) as *mut VirtqAvail;
    // Used ring 4KB sınırına hizalanır (VirtIO spec gereksinimi)
    let used = ptr.add(align_up(desc_bytes + avail_bytes, 4096)) as *mut VirtqUsed;
    Some((desc, avail, used))
}

/// VirGL yetenek setini sorgular.
///
/// Önce VirGL2'yi dene (daha yeni), başarısız olursa VirGL1.
/// İkisi de yoksa GPU 3D desteği yok demektir → 0 döner.
unsafe fn query_capset(gpu: &mut VirtioGpuDevice) -> u32 {
    if fetch_capset(gpu, VIRTIO_GPU_CAPSET_VIRGL2) {
        return VIRTIO_GPU_CAPSET_VIRGL2;
    }
    if fetch_capset(gpu, VIRTIO_GPU_CAPSET_VIRGL) {
        return VIRTIO_GPU_CAPSET_VIRGL;
    }
    0
}

/// VirGL GPU bağlamı oluşturur.
///
/// Bağlam (context), sunucuda bir OpenGL/Gallium3D durum makinesi örneğidir.
/// Tüm 3D komutlar bağlam kimliği (ctx_id) ile etiketlenir.
unsafe fn create_context(gpu: &mut VirtioGpuDevice) -> bool {
    let req = VirtioGpuCtxCreate {
        hdr: VirtioGpuCtrlHdr {
            type_: VIRTIO_GPU_CMD_CTX_CREATE,
            flags: 0,
            fence_id: 0,
            ctx_id: 0,
            padding: 0,
        },
        ctx_id: gpu.ctx_id,
        nlen: 0,
    };
    let mut resp = VirtioGpuCtrlHdr {
        type_: 0,
        flags: 0,
        fence_id: 0,
        ctx_id: 0,
        padding: 0,
    };
    submit_ctrl(
        gpu,
        &req as *const _ as *const u8,
        core::mem::size_of::<VirtioGpuCtxCreate>(),
        &mut resp as *mut _ as *mut u8,
        core::mem::size_of::<VirtioGpuCtrlHdr>(),
    ) && resp.type_ == VIRTIO_GPU_RESP_OK_NODATA
}

/// GPU'da 3D kaynak (texture/buffer) oluşturur.
///
/// `format`: B8G8R8A8_UNORM → her piksel 4 byte (BGRA sırası)
/// `depth=1, array_size=1`: basit 2D texture
unsafe fn create_resource_3d(gpu: &mut VirtioGpuDevice, width: u32, height: u32) -> bool {
    let req = VirtioGpuResourceCreate3d {
        hdr: VirtioGpuCtrlHdr {
            type_: VIRTIO_GPU_CMD_RESOURCE_CREATE_3D,
            flags: 0,
            fence_id: 0,
            ctx_id: gpu.ctx_id,
            padding: 0,
        },
        resource_id: gpu.resource_id,
        format: VIRTIO_GPU_FORMAT_B8G8R8A8_UNORM,
        width,
        height,
        depth: 1,
        array_size: 1,
        last_level: 0,
        nr_samples: 1,
        flags: 0,
    };
    let mut resp = VirtioGpuCtrlHdr {
        type_: 0,
        flags: 0,
        fence_id: 0,
        ctx_id: 0,
        padding: 0,
    };
    submit_ctrl(
        gpu,
        &req as *const _ as *const u8,
        core::mem::size_of::<VirtioGpuResourceCreate3d>(),
        &mut resp as *mut _ as *mut u8,
        core::mem::size_of::<VirtioGpuCtrlHdr>(),
    ) && resp.type_ == VIRTIO_GPU_RESP_OK_NODATA
}

/// Aktif ekranın çözünürlüğünü sorgular.
///
/// `pmodes[0]`: birincil ekranın bilgileri
/// `enabled != 0` ve boyutlar sıfır değilse geçerlidir.
unsafe fn get_display_info(gpu: &mut VirtioGpuDevice) -> Option<(u32, u32)> {
    let req = VirtioGpuCtrlHdr {
        type_: VIRTIO_GPU_CMD_GET_DISPLAY_INFO,
        flags: 0,
        fence_id: 0,
        ctx_id: 0,
        padding: 0,
    };
    let mut resp = VirtioGpuRespDisplayInfo {
        hdr: VirtioGpuCtrlHdr {
            type_: 0,
            flags: 0,
            fence_id: 0,
            ctx_id: 0,
            padding: 0,
        },
        pmodes: [VirtioGpuDisplayOne {
            r: VirtioGpuRect {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            },
            enabled: 0,
            flags: 0,
        }; VIRTIO_GPU_MAX_SCANOUTS],
    };
    if !submit_ctrl(
        gpu,
        &req as *const _ as *const u8,
        core::mem::size_of::<VirtioGpuCtrlHdr>(),
        &mut resp as *mut _ as *mut u8,
        core::mem::size_of::<VirtioGpuRespDisplayInfo>(),
    ) {
        return None;
    }
    if resp.hdr.type_ != VIRTIO_GPU_RESP_OK_DISPLAY_INFO {
        return None;
    }
    let first = &resp.pmodes[0];
    if first.enabled == 0 || first.r.width == 0 || first.r.height == 0 {
        return None;
    }
    Some((first.r.width, first.r.height))
}

/// Belirtilen VirGL yetenek setini GPU'dan alır.
///
/// # Adımlar
/// 1. Yetenek setinin boyutunu ve sürümünü sorgula
/// 2. Yeterli bellek tahsis et
/// 3. Gerçek yetenek verisini al
/// 4. Başarılı ise meta veriyi gpu yapısına kaydet
unsafe fn fetch_capset(gpu: &mut VirtioGpuDevice, capset_id: u32) -> bool {
    let Some((size, version)) = send_get_capset_info(gpu, capset_id) else {
        return false;
    };
    if size == 0 {
        return false;
    }
    let total = align_up(
        size as usize + core::mem::size_of::<VirtioGpuRespCapset>(),
        8,
    );
    let buffer = crate::allocator::heap_alloc(total) as *mut u8;
    if buffer.is_null() {
        return false;
    }
    core::ptr::write_bytes(buffer, 0, total);
    let req = VirtioGpuGetCapset {
        hdr: VirtioGpuCtrlHdr {
            type_: VIRTIO_GPU_CMD_GET_CAPSET,
            flags: 0,
            fence_id: 0,
            ctx_id: 0,
            padding: 0,
        },
        capset_id,
        capset_version: version,
    };
    if !submit_ctrl(
        gpu,
        &req as *const _ as *const u8,
        core::mem::size_of::<VirtioGpuGetCapset>(),
        buffer,
        total,
    ) {
        return false;
    }
    let resp = &*(buffer as *const VirtioGpuRespCapset);
    if resp.hdr.type_ != VIRTIO_GPU_RESP_OK_CAPSET || resp.size == 0 {
        return false;
    }
    gpu.capset_version = resp.capset_version;
    gpu.capset_size = resp.size;
    // Yetenek verisi yanıt başlığının hemen arkasında başlar
    gpu.capset_data = unsafe { buffer.add(core::mem::size_of::<VirtioGpuRespCapset>()) };
    true
}

/// Yetenek seti boyut ve sürüm bilgisini sorgular (verinin kendisini değil).
unsafe fn send_get_capset_info(gpu: &mut VirtioGpuDevice, capset_id: u32) -> Option<(u32, u32)> {
    let req = VirtioGpuGetCapset {
        hdr: VirtioGpuCtrlHdr {
            type_: VIRTIO_GPU_CMD_GET_CAPSET,
            flags: 0,
            fence_id: 0,
            ctx_id: 0,
            padding: 0,
        },
        capset_id,
        capset_version: 0, // 0 = en son sürümü sor
    };
    let mut resp = VirtioGpuRespCapset {
        hdr: VirtioGpuCtrlHdr {
            type_: 0,
            flags: 0,
            fence_id: 0,
            ctx_id: 0,
            padding: 0,
        },
        capset_id: 0,
        capset_version: 0,
        size: 0,
        padding: 0,
    };
    submit_ctrl(
        gpu,
        &req as *const _ as *const u8,
        core::mem::size_of::<VirtioGpuGetCapset>(),
        &mut resp as *mut _ as *mut u8,
        core::mem::size_of::<VirtioGpuRespCapset>(),
    )
    .then_some(())
    .filter(|_| resp.hdr.type_ == VIRTIO_GPU_RESP_OK_CAPSET)
    .map(|_| (resp.size, resp.capset_version))
}

/// VirGL 3D komut paketini GPU'ya gönderir.
///
/// # Çit (Fence) Mekanizması
///
/// Her SUBMIT_3D komutu benzersiz bir `fence_id` taşır.
/// Yanıttaki fence_id eşleşirse GPU komutu gerçekten işledi demektir.
/// Bu, sürücünün GPU ile senkronize olmasını sağlar.
unsafe fn submit_3d_command(gpu: &mut VirtioGpuDevice, data: *const u8, len: usize) -> bool {
    if data.is_null() || len == 0 {
        return false;
    }
    let total = core::mem::size_of::<VirtioGpuCmdSubmit3d>() + len;
    let buffer = crate::allocator::heap_alloc(align_up(total, 8)) as *mut u8;
    if buffer.is_null() {
        return false;
    }
    let fence_id = gpu.fence_counter;
    gpu.fence_counter = gpu.fence_counter.wrapping_add(1);
    let header = VirtioGpuCmdSubmit3d {
        hdr: VirtioGpuCtrlHdr {
            type_: VIRTIO_GPU_CMD_SUBMIT_3D,
            flags: VIRTIO_GPU_FLAG_FENCE,
            fence_id,
            ctx_id: gpu.ctx_id,
            padding: 0,
        },
        size: len as u32,
        padding: 0,
    };
    core::ptr::copy_nonoverlapping(
        &header as *const _ as *const u8,
        buffer,
        core::mem::size_of::<VirtioGpuCmdSubmit3d>(),
    );
    core::ptr::copy_nonoverlapping(
        data,
        buffer.add(core::mem::size_of::<VirtioGpuCmdSubmit3d>()),
        len,
    );
    let mut resp = VirtioGpuCtrlHdr {
        type_: 0,
        flags: 0,
        fence_id: 0,
        ctx_id: 0,
        padding: 0,
    };
    submit_ctrl(
        gpu,
        buffer,
        total,
        &mut resp as *mut _ as *mut u8,
        core::mem::size_of::<VirtioGpuCtrlHdr>(),
    ) && resp.type_ == VIRTIO_GPU_RESP_OK_NODATA
        && resp.fence_id == fence_id // Çit eşleşmesi: GPU'nun komutu işlediğini doğrular
}

/// Bir kaynağı belirtilen ekrana bağlar.
///
/// `scanout_id=0`: birincil ekran (genellikle tek ekran)
/// `resource_id`: SET_SCANOUT sonrası bu kaynağın içeriği ekranda görünür
unsafe fn set_scanout(
    gpu: &mut VirtioGpuDevice,
    resource_id: u32,
    width: u32,
    height: u32,
) -> bool {
    let req = VirtioGpuSetScanout {
        hdr: VirtioGpuCtrlHdr {
            type_: VIRTIO_GPU_CMD_SET_SCANOUT,
            flags: 0,
            fence_id: 0,
            ctx_id: 0,
            padding: 0,
        },
        r: make_rect(width, height),
        scanout_id: 0,
        resource_id,
    };
    let mut resp = VirtioGpuCtrlHdr {
        type_: 0,
        flags: 0,
        fence_id: 0,
        ctx_id: 0,
        padding: 0,
    };
    submit_ctrl(
        gpu,
        &req as *const _ as *const u8,
        core::mem::size_of::<VirtioGpuSetScanout>(),
        &mut resp as *mut _ as *mut u8,
        core::mem::size_of::<VirtioGpuCtrlHdr>(),
    ) && resp.type_ == VIRTIO_GPU_RESP_OK_NODATA
}

/// GPU kaynağındaki değişiklikleri ekrana yansıtır (flush).
///
/// SET_SCANOUT bir kaynağı ekranla ilişkilendirir,
/// RESOURCE_FLUSH ise GPU önbelleğindeki içeriği gerçekten görüntüler.
unsafe fn resource_flush(
    gpu: &mut VirtioGpuDevice,
    resource_id: u32,
    width: u32,
    height: u32,
) -> bool {
    let req = VirtioGpuResourceFlush {
        hdr: VirtioGpuCtrlHdr {
            type_: VIRTIO_GPU_CMD_RESOURCE_FLUSH,
            flags: 0,
            fence_id: 0,
            ctx_id: 0,
            padding: 0,
        },
        r: make_rect(width, height),
        resource_id,
        padding: 0,
    };
    let mut resp = VirtioGpuCtrlHdr {
        type_: 0,
        flags: 0,
        fence_id: 0,
        ctx_id: 0,
        padding: 0,
    };
    submit_ctrl(
        gpu,
        &req as *const _ as *const u8,
        core::mem::size_of::<VirtioGpuResourceFlush>(),
        &mut resp as *mut _ as *mut u8,
        core::mem::size_of::<VirtioGpuCtrlHdr>(),
    ) && resp.type_ == VIRTIO_GPU_RESP_OK_NODATA
}

/// f32 değerini bit kalıbı olarak u32'ye dönüştürür.
/// VirGL komutlarında kayan nokta değerleri ham bit kalıpları olarak iletilir.
fn f32_bits(value: f32) -> u32 {
    value.to_bits()
}

/// (0, 0) orijinli dikdörtgen oluşturur.
fn make_rect(width: u32, height: u32) -> VirtioGpuRect {
    VirtioGpuRect {
        x: 0,
        y: 0,
        width,
        height,
    }
}

/// VirGL CREATE_OBJECT (yüzey) komutunu kodlayıcıya ekler.
///
/// Yüzey (surface), framebuffer'a bağlanabilecek bir renk tamponu nesnesidir.
fn push_create_surface(
    encoder: &mut VirglEncoder,
    handle: u32,
    resource_id: u32,
    width: u32,
    height: u32,
) {
    encoder.push_cmd(
        VIRGL_CCMD_CREATE_OBJECT,
        &[
            VIRGL_OBJECT_SURFACE,
            handle,
            resource_id,
            VIRTIO_GPU_FORMAT_B8G8R8A8_UNORM,
            width,
            height,
            0,
            0,
        ],
    );
}

/// VirGL SET_FRAMEBUFFER_STATE komutunu kodlayıcıya ekler.
///
/// `nr_cbufs=1`: bir renk tamponu
/// `zsurf=0`: derinlik tamponu yok
fn push_set_framebuffer(encoder: &mut VirglEncoder, surface_handle: u32) {
    encoder.push_cmd(VIRGL_CCMD_SET_FRAMEBUFFER_STATE, &[1, 0, surface_handle]);
}

/// VirGL CLEAR komutunu kodlayıcıya ekler.
///
/// `r, g, b, a`: float'tan bit kalıbına çevrilen renk bileşenleri
/// `1.0_f32`: derinlik tamponu temizleme değeri (en uzak)
fn push_clear(encoder: &mut VirglEncoder, r: f32, g: f32, b: f32, a: f32) {
    encoder.push_cmd(
        VIRGL_CCMD_CLEAR,
        &[
            VIRGL_CLEAR_COLOR,
            f32_bits(r),
            f32_bits(g),
            f32_bits(b),
            f32_bits(a),
            f32_bits(1.0),
            0,
        ],
    );
}

/// Amber (kehribar sarısı) rengiyle ekran temizleme komut paketini oluşturur.
///
/// RGB: (1.0, 0.749, 0.0) → #FFBF00 Amber rengi
/// echOS'un marka rengi olarak kullanılır.
fn build_clear_command(surface_handle: u32, resource_id: u32, width: u32, height: u32) -> Vec<u8> {
    let mut encoder = VirglEncoder::new();
    push_create_surface(&mut encoder, surface_handle, resource_id, width, height);
    push_set_framebuffer(&mut encoder, surface_handle);
    push_clear(&mut encoder, 1.0, 0.749, 0.0, 1.0);
    encoder.into_bytes()
}

/// Dışarıdan kullanım için: yeni bir 3D GPU kaynağı oluşturur.
///
/// Her çağrıda `resource_id` otomatik artırılır.
/// VirGL aktif değilse `None` döner.
pub fn drm_resource_create_3d(width: u32, height: u32) -> Option<u32> {
    let mut guard = GPU_DEVICE.lock();
    let Some(gpu) = guard.as_mut() else {
        return None;
    };
    if !gpu.virgl {
        return None;
    }
    gpu.resource_id = gpu.resource_id.wrapping_add(1);
    let resource_id = gpu.resource_id;
    if unsafe { !create_resource_3d(gpu, width, height) } {
        return None;
    }
    Some(resource_id)
}

/// Ham VirGL komut tamponu gönderir.
///
/// Düşük seviyeli API - üst katman sürücüler tarafından kullanılır.
pub unsafe fn drm_submit_3d_command(data: *const u8, len: usize) -> bool {
    let mut guard = GPU_DEVICE.lock();
    let Some(gpu) = guard.as_mut() else {
        return false;
    };
    if !gpu.virgl {
        return false;
    }
    unsafe { submit_3d_command(gpu, data, len) }
}

/// Ekranı amber (kehribar sarısı) renge boyar.
///
/// # Tam Akış
///
/// ```
/// 1. Ekran çözünürlüğünü sorgula (GET_DISPLAY_INFO)
/// 2. Yeni kaynak ID ve yüzey tanıtıcısı al
/// 3. 3D kaynak oluştur (RESOURCE_CREATE_3D)
/// 4. VirGL komutları: yüzey oluştur → framebuffer → temizle
/// 5. Kaynağı ekrana bağla (SET_SCANOUT)
/// 6. Ekrana yansıt (RESOURCE_FLUSH)
/// ```
pub fn hardware_clear_amber(width: u32, height: u32) -> bool {
    let mut guard = GPU_DEVICE.lock();
    let Some(gpu) = guard.as_mut() else {
        return false;
    };
    if !gpu.virgl {
        return false;
    }
    let (target_width, target_height) = unsafe { get_display_info(gpu) }.unwrap_or((width, height));
    gpu.resource_id = gpu.resource_id.wrapping_add(1);
    gpu.surface_handle = gpu.surface_handle.wrapping_add(1);
    let resource_id = gpu.resource_id;
    let surface_handle = gpu.surface_handle;
    if unsafe { !create_resource_3d(gpu, target_width, target_height) } {
        return false;
    }
    let payload = build_clear_command(surface_handle, resource_id, target_width, target_height);
    if unsafe { !submit_3d_command(gpu, payload.as_ptr(), payload.len()) } {
        return false;
    }
    if unsafe { !set_scanout(gpu, resource_id, target_width, target_height) } {
        return false;
    }
    unsafe { resource_flush(gpu, resource_id, target_width, target_height) }
}

/// Kontrol kuyruğuna bir istek gönderir ve yanıtı bekler (senkron).
///
/// # Mekanizma (Polling)
///
/// ```
/// 1. desc[0] → istek (OUT: sürücüden cihaza)
/// 2. desc[1] → yanıt tamponu (IN: cihazdan sürücüye)
///    desc[0].next = 1  (zincir)
///    desc[1].flags |= WRITE
///
/// 3. avail.ring[idx % size] = 0  (zincir başı)
/// 4. avail.idx++                 (atomik bellek engeli ile)
/// 5. notify_ptr'e yaz            (kapı zilini çal)
///
/// 6. used.idx == avail.idx olana kadar döngüde bekle (busy-wait)
/// ```
///
/// NOT: Üretim sisteminde kesme tabanlı (interrupt-driven) yaklaşım kullanılır.
unsafe fn submit_ctrl(
    gpu: &mut VirtioGpuDevice,
    req: *const u8,
    req_len: usize,
    resp: *mut u8,
    resp_len: usize,
) -> bool {
    let q = &mut gpu.ctrl_queue;
    let desc = &mut *q.desc;
    let desc1 = q.desc.add(1);
    // desc[0]: istek tamponu (cihaz okur)
    (*desc).addr = req as u64;
    (*desc).len = req_len as u32;
    (*desc).flags = 0x0002; // NEXT bayrağı: zincir devam ediyor
    (*desc).next = 1;
    // desc[1]: yanıt tamponu (cihaz yazar)
    (*desc1).addr = resp as u64;
    (*desc1).len = resp_len as u32;
    (*desc1).flags = 0x0002 | 0x0001; // WRITE | NEXT (WRITE = cihaz yazar)
    (*desc1).next = 0;

    let avail = &mut *q.avail;
    let idx = avail.idx;
    avail.ring[(idx as usize) % q.size as usize] = 0; // desc zinciri başlangıcı = 0
    avail.idx = idx.wrapping_add(1);
    // SeqCst bellek engeli: derleyici/CPU sıralamayı bozmasın
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);

    // Kapı zili: cihaza yeni iş var diye bildir
    let notify_offset = q.notify_off as u32 * gpu.notify_off_multiplier;
    let notify_ptr = gpu.notify.add(notify_offset as usize) as *mut u16;
    write_volatile(notify_ptr, 0);

    // Busy-wait: cihaz used.idx'i güncelleyene kadar döngüde bekle
    let target = avail.idx;
    let used_ptr = q.used;
    loop {
        let used_idx = unsafe { core::ptr::read_volatile(core::ptr::addr_of!((*used_ptr).idx)) };
        if used_idx == target {
            q.last_used = used_idx;
            break;
        }
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
    }
    true
}

/// PCI BAR (Base Address Register) tabanını döndürür.
///
/// BAR, MMIO veya I/O port alanının fiziksel adresini içerir.
/// `resources` dizisi, Linux sürücü altyapısının doldurduğu BAR listesidir.
unsafe fn get_bar_base(dev: *mut PciDev, index: u8) -> Option<u64> {
    if dev.is_null() {
        return None;
    }
    let resources = &(*dev).resource;
    let res = resources.get(index as usize)?;
    if res.start == 0 {
        return None;
    }
    Some(res.start)
}

/// Değeri yukarı doğru belirtilen hizaya (alignment) yuvarlar.
///
/// Örnek: align_up(5, 4) = 8  (0b0101 → 0b1000)
/// Formül: (value + align - 1) & !(align - 1)
fn align_up(value: usize, align: usize) -> usize {
    (value + align - 1) & !(align - 1)
}

/// PCI yapılandırma uzayından 1 byte okur.
///
/// PCI config space dword (32-bit) erişim granülarittedir;
/// 8-bit değer için shift hesabı gerekir.
fn read_config_u8(bus: u8, device: u8, function: u8, offset: u8) -> u8 {
    let value = crate::drivers::pci::read_config_dword(bus, device, function, offset as u16);
    let shift = (offset & 3) * 8;
    ((value >> shift) & 0xFF) as u8
}

/// PCI yapılandırma uzayından 2 byte okur.
fn read_config_u16(bus: u8, device: u8, function: u8, offset: u8) -> u16 {
    let value = crate::drivers::pci::read_config_dword(bus, device, function, offset as u16);
    let shift = (offset & 2) * 8;
    ((value >> shift) & 0xFFFF) as u16
}

/// PCI yapılandırma uzayından 4 byte okur (doğrudan dword).
fn read_config_u32(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    crate::drivers::pci::read_config_dword(bus, device, function, offset as u16)
}
