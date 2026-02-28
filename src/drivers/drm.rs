//! # DRM/KMS - Doğrudan Render Yöneticisi (Direct Rendering Manager)
//!
//! GPU ve ekran alt sistemi yönetimi. Linux DRM/KMS mimarisini uygular.
//!
//! ## DRM/KMS Kavramları
//!
//! Modern Linux'ta ekran çıkışı şu hiyerarşiyle yönetilir:
//!
//! ```
//! [GPU/DrmDevice]
//!       |
//!       +--[CRTC]          <- Ekran denetleyicisi; hangi FB'yi hangi modda çıkarır
//!       |     |
//!       |     +--[Plane]   <- Framebuffer katmanı (birden fazla çakışık katman olabilir)
//!       |
//!       +--[Encoder]       <- Dijital/Analog sinyal dönüştürücü (TMDS, LVDS, VGA DAC...)
//!             |
//!             +--[Connector] <- Fiziksel bağlantı noktası (HDMI, DisplayPort, VGA, eDP)
//!                   |
//!               [Monitör]
//! ```
//!
//! ## GEM (Graphics Execution Manager)
//!
//! GPU bellek nesneleri GEM handle'larıyla yönetilir:
//!
//! ```
//! gem_create(size) -> handle
//!       |
//!       v
//! gem_get(handle) -> Arc<GemObject>   <- vaddr/paddr ile CPU taraflı haritalama
//!       |
//!       v
//! gem_close(handle)                   <- nesneyi yok et, belleği geri ver
//! ```
//!
//! ## DRM ioctl Akışı
//!
//! Kullanıcı alanı (Mesa/libdrm) aşağıdaki sırayla ekran açar:
//!   1. DRM_IOCTL_MODE_GETRESOURCES  -> CRTC/Connector/Encoder listesi
//!   2. DRM_IOCTL_MODE_GETCONNECTOR  -> monitör bilgisi, desteklenen çözünürlükler
//!   3. DRM_IOCTL_MODE_CREATE_DUMB   -> GEM objesi yarat (CPU taraflı FB)
//!   4. DRM_IOCTL_MODE_ADDFB         -> GEM'i framebuffer olarak kaydet
//!   5. DRM_IOCTL_MODE_SETCRTC       -> CRTC'yi aç, FB'yi ekrana bağla

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// DRM IOCTL SABİTLERİ (DRM CONSTANTS)
// ============================================================================

// DRM ioctl numaraları Linux ABI'siyle uyumludur.
// Üst 16 bit yön+boyut bilgisi taşır (Linux _IOC makrosu); alt 16 bit komut.
// 0x64 = 'd' = DRM magic byte

/// Temel DRM ioctl komutları (versiyon, GEM, CAP sorguları)
pub const DRM_IOCTL_VERSION: u32 = 0x8000_6400;
pub const DRM_IOCTL_GET_MAGIC: u32 = 0x8000_6401;
pub const DRM_IOCTL_IRQ_BUSID: u32 = 0xC008_6402;
pub const DRM_IOCTL_GET_MAP: u32 = 0xC028_6403;
pub const DRM_IOCTL_GET_CLIENT: u32 = 0xC028_6404;
pub const DRM_IOCTL_GET_STATS: u32 = 0xC008_6405;
pub const DRM_IOCTL_SET_VERSION: u32 = 0xC024_6406;
pub const DRM_IOCTL_MODESET_CTL: u32 = 0x4008_6407;
pub const DRM_IOCTL_GEM_CLOSE: u32 = 0x4008_6408;
pub const DRM_IOCTL_GEM_FLINK: u32 = 0xC008_6409;
pub const DRM_IOCTL_GEM_OPEN: u32 = 0xC010_640A;
pub const DRM_IOCTL_GET_CAP: u32 = 0xC010_640B;
pub const DRM_IOCTL_SET_CLIENT_CAP: u32 = 0x4010_640C;
pub const DRM_IOCTL_PRIME_HANDLE_TO_FD: u32 = 0xC00C_642E;
pub const DRM_IOCTL_PRIME_FD_TO_HANDLE: u32 = 0xC00C_642F;

/// KMS (Kernel Mode Setting) ioctl komutları - ekran ayarları
pub const DRM_IOCTL_MODE_GETRESOURCES: u32 = 0xC040_64A0;
pub const DRM_IOCTL_MODE_GETCONNECTOR: u32 = 0xC1A0_64A1;
pub const DRM_IOCTL_MODE_GETENCODER: u32 = 0xC0A0_64A2;
pub const DRM_IOCTL_MODE_GETCRTC: u32 = 0xC0C0_64A3;
pub const DRM_IOCTL_MODE_SETCRTC: u32 = 0xC0C0_64A4;
pub const DRM_IOCTL_MODE_GETPLANE: u32 = 0xC0B0_64A5;
pub const DRM_IOCTL_MODE_SETPLANE: u32 = 0xC0B0_64A6;
pub const DRM_IOCTL_MODE_CURSOR: u32 = 0xC080_64A7;
pub const DRM_IOCTL_MODE_GETFB: u32 = 0xC080_64A8;
pub const DRM_IOCTL_MODE_ADDFB: u32 = 0xC080_64A9;
pub const DRM_IOCTL_MODE_RMFB: u32 = 0x4008_64AA;
pub const DRM_IOCTL_MODE_PAGE_FLIP: u32 = 0xC018_64B0;
pub const DRM_IOCTL_MODE_DIRTYFB: u32 = 0xC018_64B1;
pub const DRM_IOCTL_MODE_CREATE_DUMB: u32 = 0xC0C0_64B2;
pub const DRM_IOCTL_MODE_MAP_DUMB: u32 = 0xC010_64B3;
pub const DRM_IOCTL_MODE_DESTROY_DUMB: u32 = 0xC008_64B4;

// ============================================================================
// DRM VERSİYON BİLGİSİ (DRM VERSION)
// ============================================================================

// DRM_IOCTL_VERSION ioctl'une yanıt olarak kullanıcı alanına doldurulur.
// name/date/desc alanları kullanıcı alanı pointer'larıdır (u64 olarak saklanır).

#[repr(C)]
pub struct DrmVersion {
    pub version_major: i32,
    pub version_minor: i32,
    pub version_patchlevel: i32,
    pub name_len: usize,
    pub name: u64,
    pub date_len: usize,
    pub date: u64,
    pub desc_len: usize,
    pub desc: u64,
}

// ============================================================================
// DRM CİHAZI (DRM DEVICE)
// ============================================================================

// Her GPU bir DrmDevice örneğiyle temsil edilir.
// Birden fazla GPU desteklemek için DrmManager birden fazla DrmDevice tutar.
//
//   DrmDevice [card0]
//     |-- framebuffers: BTreeMap<fb_id, DrmFramebuffer>
//     |-- gem_objects:  BTreeMap<handle, GemObject>    <- GPU bellek nesneleri
//     |-- crtcs:        Vec<DrmCrtc>                   <- Ekran denetleyicileri
//     |-- encoders:     Vec<DrmEncoder>                <- Sinyal dönüştürücüler
//     |-- connectors:   Vec<DrmConnector>              <- Fiziksek çıkışlar
//     +-- planes:       Vec<DrmPlane>                  <- Görüntü katmanları

pub struct DrmDevice {
    /// Sistemdeki benzersiz cihaz kimliği
    pub id: u64,
    /// Cihaz adı (örn. "card0", "card1")
    pub name: String,
    /// Sürücü adı (kullanıcı alanına raporlanır)
    pub driver_name: String,
    /// Sürücü versiyonu (major, minor, patch)
    pub driver_version: (u32, u32, u32),
    /// Sürücü yetenekleri (DRM_CAP_* sabitleriyle sorgulanır)
    pub caps: Mutex<BTreeMap<u64, u64>>,
    /// Kayıtlı framebuffer'lar (fb_id -> nesne)
    pub framebuffers: Mutex<BTreeMap<u32, Arc<DrmFramebuffer>>>,
    /// GEM bellek nesneleri (handle -> nesne)
    pub gem_objects: Mutex<BTreeMap<u32, Arc<GemObject>>>,
    /// Bir sonraki GEM handle değeri (her zaman artarak gider)
    next_gem_handle: AtomicU32,
    /// Bir sonraki framebuffer ID değeri
    next_fb_id: AtomicU32,
    /// KMS modunun etkin olup olmadığı
    pub modeset_enabled: AtomicBool,
    /// Bu GPU'nun CRTC listesi (her biri bağımsız ekrana çıkış yapabilir)
    pub crtcs: Mutex<Vec<Arc<DrmCrtc>>>,
    /// Encoder listesi
    pub encoders: Mutex<Vec<Arc<DrmEncoder>>>,
    /// Connector listesi (HDMI, DP, VGA...)
    pub connectors: Mutex<Vec<Arc<DrmConnector>>>,
    /// Plane listesi (her CRTC'nin bir veya daha fazla plane'i var)
    pub planes: Mutex<Vec<Arc<DrmPlane>>>,
}

impl DrmDevice {
    pub fn new(id: u64, name: &str) -> Self {
        Self {
            id,
            name: String::from(name),
            driver_name: String::from("echos-drm"),
            driver_version: (1, 0, 0),
            caps: Mutex::new(BTreeMap::new()),
            framebuffers: Mutex::new(BTreeMap::new()),
            gem_objects: Mutex::new(BTreeMap::new()),
            next_gem_handle: AtomicU32::new(1),
            next_fb_id: AtomicU32::new(1),
            modeset_enabled: AtomicBool::new(true),
            crtcs: Mutex::new(Vec::new()),
            encoders: Mutex::new(Vec::new()),
            connectors: Mutex::new(Vec::new()),
            planes: Mutex::new(Vec::new()),
        }
    }

    /// Yeni GEM bellek nesnesi oluşturur.
    /// Kullanıcı alanı DRM_IOCTL_MODE_CREATE_DUMB veya drmPrimeFdToHandle ile çağırır.
    pub fn gem_create(&self, size: u64) -> Arc<GemObject> {
        let handle = self.next_gem_handle.fetch_add(1, Ordering::SeqCst);
        let obj = Arc::new(GemObject::new(handle, size));
        self.gem_objects.lock().insert(handle, obj.clone());
        obj
    }

    /// Handle ile GEM nesnesini getirir
    pub fn gem_get(&self, handle: u32) -> Option<Arc<GemObject>> {
        self.gem_objects.lock().get(&handle).cloned()
    }

    /// GEM nesnesini kapatır; referans düşerse bellek serbest bırakılır
    pub fn gem_close(&self, handle: u32) {
        self.gem_objects.lock().remove(&handle);
    }

    /// Framebuffer oluşturur; GPU çıkışı için piksel tamponu.
    /// handles[0..3]: renk/derinlik/stencil için GEM handle'lar (ARGB için 1 yeterli).
    pub fn fb_create(&self, width: u32, height: u32, format: u32, handles: [u32; 4]) -> u32 {
        let fb_id = self.next_fb_id.fetch_add(1, Ordering::SeqCst);
        let fb = Arc::new(DrmFramebuffer::new(fb_id, width, height, format, handles));
        self.framebuffers.lock().insert(fb_id, fb);
        fb_id
    }

    /// Framebuffer ID ile framebuffer nesnesini getirir
    pub fn fb_get(&self, fb_id: u32) -> Option<Arc<DrmFramebuffer>> {
        self.framebuffers.lock().get(&fb_id).cloned()
    }

    /// Framebuffer'ı siler; CRTC'nin bu FB'yi kullanmadığından emin ol
    pub fn fb_remove(&self, fb_id: u32) {
        self.framebuffers.lock().remove(&fb_id);
    }

    /// Sürücü yeteneğini sorgular (DRM_CAP_DUMB_BUFFER, DRM_CAP_VBLANK_HIGH_CRTC...)
    pub fn get_cap(&self, cap: u64) -> u64 {
        self.caps.lock().get(&cap).copied().unwrap_or(0)
    }

    /// Sürücü yeteneği tanımlar
    pub fn set_cap(&self, cap: u64, value: u64) {
        self.caps.lock().insert(cap, value);
    }

    /// CRTC ekler (başlatma sırasında çağrılır)
    pub fn add_crtc(&self, crtc: Arc<DrmCrtc>) {
        self.crtcs.lock().push(crtc);
    }

    /// Connector ekler (HDMI, DP, VGA bağlantı noktaları)
    pub fn add_connector(&self, connector: Arc<DrmConnector>) {
        self.connectors.lock().push(connector);
    }

    /// Encoder ekler (TMDS, LVDS, DAC dönüştürücüler)
    pub fn add_encoder(&self, encoder: Arc<DrmEncoder>) {
        self.encoders.lock().push(encoder);
    }

    /// Plane ekler (görüntü katmanı: birincil, kaplama, imleç)
    pub fn add_plane(&self, plane: Arc<DrmPlane>) {
        self.planes.lock().push(plane);
    }
}

// ============================================================================
// GEM NESNESİ (GEM OBJECT)
// ============================================================================

// GEM (Graphics Execution Manager) GPU bellek nesnelerini temsil eder.
// Linux'ta TTM (Translation Table Manager) veya GEM bu işi yapar.
//
// Nesne haritalaması:
//
//   GemObject
//     |-- paddr: Option<u64>  <- Fiziksel adres (DMA için)
//     +-- vaddr: Option<u64>  <- Sanal adres   (CPU tarafı yazma/okuma için)
//
// PRIME/DMA-BUF ile farklı süreçler arasında handle paylaşımı:
//   gem_flink  -> global isim oluştur
//   gem_open   -> başka süreç bu isimle nesneye erişir

pub struct GemObject {
    pub handle: u32,
    pub size: u64,
    pub vaddr: Mutex<Option<u64>>,
    pub paddr: Mutex<Option<u64>>,
    pub ref_count: AtomicU32,
    pub dma_buf: Mutex<Option<u32>>,
}

impl GemObject {
    pub fn new(handle: u32, size: u64) -> Self {
        Self {
            handle,
            size,
            vaddr: Mutex::new(None),
            paddr: Mutex::new(None),
            ref_count: AtomicU32::new(1),
            dma_buf: Mutex::new(None),
        }
    }

    /// Nesneyi CPU adres uzayına haritalar ve sanal adresi döner.
    /// Gerçek uygulamada sayfa tablosu girişi oluşturur.
    pub fn map(&self) -> u64 {
        let mut vaddr = self.vaddr.lock();
        if vaddr.is_none() {
            // Fiziksel bellek tahsis et ve sanal adrese haritala
            *vaddr = Some(0xFFFF_8000_0000_0000);
        }
        vaddr.unwrap()
    }

    /// CPU haritalamasını kaldırır; sayfa tablosu girişi temizlenir
    pub fn unmap(&self) {
        *self.vaddr.lock() = None;
    }
}

// ============================================================================
// FRAMEBUFFER (DRM FRAMEBUFFER)
// ============================================================================

// Ekrana gönderilebilecek piksel tampon nesnesi.
// Birden fazla GEM handle ile çok düzlemli (planar) formatlar desteklenir:
//   handles[0]: Y düzlemi (luma)
//   handles[1]: UV düzlemi (chroma, NV12 vb.)
//
// Piksel formatı FourCC kodu ile belirlenir:
//   DRM_FORMAT_XRGB8888 = 0x34325258 (en yaygın)
//   DRM_FORMAT_NV12     = 0x3231564E (video)

pub struct DrmFramebuffer {
    pub id: u32,
    pub width: u32,
    pub height: u32,
    pub format: u32,
    pub handles: [u32; 4],
    pub pitches: [u32; 4],  // bytes per row for each plane
    pub offsets: [u32; 4],  // offset within GEM for each plane
    pub modifier: u64,       // tiling/compression modifier (AMD, Intel tiling)
    pub ref_count: AtomicU32,
}

impl DrmFramebuffer {
    pub fn new(id: u32, width: u32, height: u32, format: u32, handles: [u32; 4]) -> Self {
        Self {
            id,
            width,
            height,
            format,
            handles,
            pitches: [width * 4, 0, 0, 0], // XRGB8888: 4 byte per pixel
            offsets: [0, 0, 0, 0],
            modifier: 0,
            ref_count: AtomicU32::new(1),
        }
    }
}

// ============================================================================
// CRTC (Cathode Ray Tube Controller)
// ============================================================================

// Modern GPU'larda gerçek bir CRT olmasa da isim tarihi nedenle kalıcıdır.
// CRTC, piksel verisini ekrana saat sinyaliyle gönderen devre bloğudur.
//
// CRTC ve modeline ilişki:
//
//   DrmCrtc
//     |-- mode:  DrmMode   <- Video modu: çözünürlük, tarama frekansı
//     |-- fb_id: u32       <- Şu an görüntülenen framebuffer
//     +-- active: bool     <- Ekran açık mı?
//
// DrmMode alanları VESA/CEA standart zamanlamasını tanımlar:
//   hdisplay x vdisplay @ vrefresh Hz
//   Örn: 1920 x 1080 @ 60 Hz  -> clock = 148500 kHz

pub struct DrmCrtc {
    pub id: u32,
    pub index: u32,
    /// Görüntünün framebuffer içindeki X ofseti (scissor/pan)
    pub x: u32,
    /// Görüntünün framebuffer içindeki Y ofseti
    pub y: u32,
    /// Şu an görüntülenen framebuffer ID'si (0 = yok)
    pub fb_id: AtomicU32,
    /// Aktif video modu (None = devre dışı)
    pub mode: Mutex<Option<DrmMode>>,
    /// Gamma tablosunun boyutu (genellikle 256 giriş)
    pub gamma_size: u32,
    /// CRTC'nin aktif olup olmadığı
    pub active: AtomicBool,
}

/// Video zamanlama modu; piksel saati ve tarama parametrelerini tanımlar.
/// DRM_IOCTL_MODE_GETCONNECTOR ile monitörden EDID bilgisi alınarak doldurulur.
#[derive(Clone, Debug)]
pub struct DrmMode {
    /// Piksel saati (kHz cinsinden, örn. 148500 = 1080p60)
    pub clock: u32,
    pub hdisplay: u16,    // Görünür yatay piksel sayısı
    pub hsync_start: u16, // Yatay senkron başlangıcı
    pub hsync_end: u16,   // Yatay senkron bitişi
    pub htotal: u16,      // Toplam yatay süre (görünür + blanking)
    pub hskew: u16,
    pub vdisplay: u16,    // Görünür dikey satır sayısı
    pub vsync_start: u16,
    pub vsync_end: u16,
    pub vtotal: u16,
    pub vscan: u16,
    /// Dikey yenileme hızı (Hz)
    pub vrefresh: u32,
    pub flags: u32,
    pub type_: u32,
    /// Mod adı (örn. "1920x1080") UTF-8, null-padded
    pub name: [u8; 32],
}

impl DrmCrtc {
    pub fn new(id: u32, index: u32) -> Self {
        Self {
            id,
            index,
            x: 0,
            y: 0,
            fb_id: AtomicU32::new(0),
            mode: Mutex::new(None),
            gamma_size: 256,
            active: AtomicBool::new(false),
        }
    }

    /// Video modunu ayarlar ve CRTC'yi aktif duruma getirir
    pub fn set_mode(&self, mode: DrmMode) {
        *self.mode.lock() = Some(mode);
        self.active.store(true, Ordering::SeqCst);
    }

    /// Görüntülenen framebuffer'ı değiştirir (page flip öncesi)
    pub fn set_fb(&self, fb_id: u32) {
        self.fb_id.store(fb_id, Ordering::SeqCst);
    }
}

// ============================================================================
// ENCODER (DRM ENCODER)
// ============================================================================

// CRTC'den gelen dijital piksel verisini fiziksel sinyal standardına dönüştürür:
//   DRM_MODE_ENCODER_TMDS  -> HDMI / DVI (Transition Minimized Differential Signaling)
//   DRM_MODE_ENCODER_LVDS  -> Dahili panel (laptop)
//   DRM_MODE_ENCODER_DAC   -> VGA analog
//   DRM_MODE_ENCODER_DSI   -> Mobil MIPI DSI
//
// possible_crtcs bitmask'i hangi CRTC'lerin bu encoder'ı kullanabileceğini gösterir.

pub struct DrmEncoder {
    pub id: u32,
    /// Encoder türü (DRM_MODE_ENCODER_* sabitleri)
    pub encoder_type: u32,
    /// Şu an bağlı CRTC'nin ID'si
    pub crtc_id: AtomicU32,
    /// Bu encoder ile uyumlu CRTC'lerin bitmask'i
    pub possible_crtcs: u32,
    /// Klonlanabilir encoder bitmask'i (mirror mode için)
    pub possible_clones: u32,
}

impl DrmEncoder {
    pub fn new(id: u32, encoder_type: u32) -> Self {
        Self {
            id,
            encoder_type,
            crtc_id: AtomicU32::new(0),
            possible_crtcs: 0xFFFF,
            possible_clones: 0xFFFF,
        }
    }
}

// ============================================================================
// CONNECTOR (DRM CONNECTOR)
// ============================================================================

// Fiziksel çıkış bağlantı noktası: HDMI, DisplayPort, VGA, eDP, DVI-D...
// Connector durumu hotplug ile değişebilir:
//   Connected    -> monitör bağlı, EDID okundu
//   Disconnected -> monitör yok
//   Unknown      -> henüz algılanmadı
//
// Connector->Encoder->CRTC zinciri:
//
//   [HDMI-A-1 connector] --bağlı--> [TMDS encoder] --bağlı--> [CRTC 0]

pub struct DrmConnector {
    pub id: u32,
    /// Bağlayıcı türü (DRM_MODE_CONNECTOR_HDMIA, VGA, DP, eDP...)
    pub connector_type: u32,
    /// Aynı türdeki sıra numarası (örn. HDMI-A-1, HDMI-A-2)
    pub connector_type_id: u32,
    /// Bağlı encoder'ın ID'si
    pub encoder_id: AtomicU32,
    /// Bağlantı durumu (hotplug olaylarıyla güncellenir)
    pub connection: Mutex<DrmConnectorStatus>,
    /// Monitörün desteklediği video modları (EDID'den okunur)
    pub modes: Mutex<Vec<DrmMode>>,
    /// Monitörün fiziksel genişliği (mm)
    pub width_mm: u32,
    /// Monitörün fiziksel yüksekliği (mm)
    pub height_mm: u32,
    /// Alt piksel düzeni (renk kalitesi için)
    pub subpixel: u32,
}

/// Fiziksel bağlantı algılama durumu
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrmConnectorStatus {
    Unknown,
    Connected,
    Disconnected,
}

impl DrmConnector {
    pub fn new(id: u32, connector_type: u32) -> Self {
        Self {
            id,
            connector_type,
            connector_type_id: 1,
            encoder_id: AtomicU32::new(0),
            connection: Mutex::new(DrmConnectorStatus::Unknown),
            modes: Mutex::new(Vec::new()),
            width_mm: 0,
            height_mm: 0,
            subpixel: 0,
        }
    }

    /// Desteklenen yeni video modu ekler (EDID ayrıştırmasından çağrılır)
    pub fn add_mode(&self, mode: DrmMode) {
        self.modes.lock().push(mode);
    }

    /// Monitör bağlantı durumunu döner (hotplug polling için)
    pub fn detect(&self) -> DrmConnectorStatus {
        *self.connection.lock()
    }
}

// ============================================================================
// PLANE (DRM PLANE)
// ============================================================================

// Ekrana gönderilecek görüntü katmanı. Üç tür plane vardır:
//   Primary   -> CRTC'nin ana framebuffer'ı
//   Overlay   -> Donanım video overlay (YUV video için)
//   Cursor    -> Donanım imleç katmanı (16x16 veya 64x64 piksel)
//
// Her plane bağımsız kaynak (src_x/y/w/h) ve hedef (crtc_x/y/w/h) dikdörtgenleri
// tanımlar; donanım ölçekleme desteğiyle farklı boyutlarda gösterilebilir.

pub struct DrmPlane {
    pub id: u32,
    /// Bu plane'in kullanabileceği CRTC'lerin bitmask'i
    pub possible_crtcs: u32,
    /// Desteklenen piksel formatlarının sayısı
    pub format_count: u32,
    /// FourCC piksel formatları (DRM_FORMAT_*)
    pub formats: Vec<u32>,
    /// Bağlı CRTC ID'si
    pub crtc_id: AtomicU32,
    /// Görüntülenen framebuffer ID'si
    pub fb_id: AtomicU32,
    /// Hedef CRTC alanı (ekran koordinatlarında)
    pub crtc_x: AtomicU32,
    pub crtc_y: AtomicU32,
    pub crtc_w: AtomicU32,
    pub crtc_h: AtomicU32,
    /// Kaynak framebuffer alanı (16.16 sabit nokta formatı)
    pub src_x: AtomicU32,
    pub src_y: AtomicU32,
    pub src_w: AtomicU32,
    pub src_h: AtomicU32,
}

impl DrmPlane {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            possible_crtcs: 0xFFFF,
            format_count: 0,
            formats: Vec::new(),
            crtc_id: AtomicU32::new(0),
            fb_id: AtomicU32::new(0),
            crtc_x: AtomicU32::new(0),
            crtc_y: AtomicU32::new(0),
            crtc_w: AtomicU32::new(0),
            crtc_h: AtomicU32::new(0),
            src_x: AtomicU32::new(0),
            src_y: AtomicU32::new(0),
            src_w: AtomicU32::new(0),
            src_h: AtomicU32::new(0),
        }
    }
}

// ============================================================================
// DRM YÖNETİCİSİ (DRM MANAGER)
// ============================================================================

// Sistemdeki tüm DRM cihazlarının (GPU'ların) kayıtlarını tutar.
// Linux'ta /dev/dri/card0, /dev/dri/card1, ... olarak görünür.

pub struct DrmManager {
    devices: Mutex<BTreeMap<u64, Arc<DrmDevice>>>,
    next_device_id: AtomicU64,
}

impl DrmManager {
    pub const fn new() -> Self {
        Self {
            devices: Mutex::new(BTreeMap::new()),
            next_device_id: AtomicU64::new(1),
        }
    }

    pub fn register_device(&self, name: &str) -> Arc<DrmDevice> {
        let id = self.next_device_id.fetch_add(1, Ordering::SeqCst);
        let device = Arc::new(DrmDevice::new(id, name));
        self.devices.lock().insert(id, device.clone());
        device
    }

    pub fn get_device(&self, id: u64) -> Option<Arc<DrmDevice>> {
        self.devices.lock().get(&id).cloned()
    }
}

lazy_static::lazy_static! {
    pub static ref DRM_MANAGER: DrmManager = DrmManager::new();
}

// ============================================================================
// BAŞLATMA (INITIALIZATION)
// ============================================================================

pub fn init() {
    // Birincil GPU cihazını (card0) sisteme kaydet
    let gpu = DRM_MANAGER.register_device("card0");

    // Varsayılan CRTC ekle (ekran denetleyicisi; tek ekran için yeterli)
    let crtc = Arc::new(DrmCrtc::new(0, 0));
    gpu.add_crtc(crtc);

    // Varsayılan connector ekle (VGA tipi; gerçekte EDID algılama yapılır)
    let connector = Arc::new(DrmConnector::new(0, 0)); // VGA
    gpu.add_connector(connector);

    crate::serial_println!("[DRM] DRM/KMS initialized");
}
