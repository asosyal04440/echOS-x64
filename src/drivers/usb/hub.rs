//! # echOS USB Hub Sürücüsü
//!
//! USB hub numaralandırma ve port yönetimi.
//! Hem kök hub'ları (xHCI denetleyici içindeki sanal hub) hem de
//! harici USB hub cihazlarını destekler.
//!
//! ## Hub Topolojisi
//!
//! ```
//!  xHCI Denetleyici
//!  ┌──────────────────────────────────────────────────┐
//!  │  Kök Hub (sanal, xHCI içinde)                    │
//!  │  ├── Port 0 ──► Flash bellek (MassStorage)       │
//!  │  ├── Port 1 ──► Harici Hub (USB 2.0, TT destekli)│
//!  │  │              ├── Port 1 ──► Klavye (FS)        │
//!  │  │              ├── Port 2 ──► Fare (LS)          │
//!  │  │              └── Port 3 ──► Boş               │
//!  │  └── Port 2 ──► Webcam (High Speed)              │
//!  └──────────────────────────────────────────────────┘
//! ```
//!
//! ## Kök Hub vs Harici Hub
//!
//! **Kök Hub** (`is_root=true`):
//! - xHCI denetleyicinin içine entegre edilmiş sanal hub
//! - Port durumu PORTSC MMIO register'ından okunur
//! - USB sınıfı request gerektirmez
//! - `create_root_hub()` ile oluşturulur
//!
//! **Harici Hub** (`is_root=false`):
//! - Bağımsız USB hub cihazı (class=0x09)
//! - Hub tanımlayıcısı `GET_DESCRIPTOR(Hub)` ile alınır
//! - Port durumu `GET_STATUS` kontrol isteğiyle okunur
//! - `new()` ile UsbDevice üzerinden oluşturulur
//!
//! ## TT (Transaction Translator)
//!
//! Hızlı (High Speed, 480 Mbps) bir hub'a bağlı yavaş (Full/Low Speed)
//! cihazlarla iletişim kurmak için TT ara birim kullanılır.
//! TT, HS paketlerini FS/LS formatına dönüştürür:
//! ```
//! xHCI (HS) ──► Hub TT ──► FS klavye (12 Mbps)
//!                      └──► LS fare (1.5 Mbps)
//! ```
//! `USB_PROTOCOL_HUB_TT_SINGLE` = Tüm portlar için tek TT
//! `USB_PROTOCOL_HUB_TT_MULTI`  = Her port için ayrı TT
//!
//! ## Port Durumu Okuma Akışı
//!
//! ```
//! poll() → get_port_status() → GET_STATUS → PortState
//!                ↓
//!         PortStatus  PortChange
//!         (bağlı?)    (değişim?)
//!              ↓           ↓
//!         Hız tespiti  clear_connection_change()
//!              ↓
//!         reset_port() → SET_FEATURE(PORT_RESET)
//!              ↓
//!         Cihaz numaralandırma (address 0 → yeni adres)
//! ```
//!
//! ## Hub Tanımlayıcı Formatı (9+ byte)
//!
//! ```
//! Byte 0: bDescLength      - toplam tanımlayıcı boyutu
//! Byte 1: bDescriptorType  - 0x29 (Hub)
//! Byte 2: bNbrPorts        - downstream port sayısı
//! Byte 3-4: wHubCharacteristics - hub özellikleri bit alanı
//!   bit1-0: Loglevel güç anahtarlama (00=ganged, 01=bireysel)
//!   bit2:   Bileşik cihaz mı?
//!   bit4-3: Aşırı akım koruması
//!   bit6-5: TT düşünme süresi
//!   bit7:   Port göstergesi desteği
//! Byte 5: bPwrOn2PwrGood   - güç açma gecikmesi (2ms birimi)
//! Byte 6: bHubContrCurrent - hub kontrol akımı (mA)
//! Byte 7+: DeviceRemovable, PortPwrCtrlMask (değişken uzunluk)
//! ```

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::mem;
use spin::Mutex;

use super::{
    UsbClass, UsbDevice, UsbDeviceAddress, UsbEndpoint, UsbError, UsbSetupPacket, UsbSpeed,
    XhciController,
};

// ============================================================================
// HUB SABİTLERİ
// USB Hub sınıf, alt sınıf ve protokol kodları
// ============================================================================

/// USB Hub sınıf kodu (USB sınıfı 0x09)
pub const USB_CLASS_HUB: u8 = 0x09;

/// Hub alt sınıfı (çok hızlı hub'lar için 0x00)
pub const USB_SUBCLASS_HUB: u8 = 0x00;

/// Hub protokolü: yalnızca tam hız (Full Speed, TT yoktur)
pub const USB_PROTOCOL_HUB_FS: u8 = 0x00;

/// Hub protokolü: tek TT (Transaction Translator)
/// Tüm downstream portlar tek bir TT devresini paylaşır
pub const USB_PROTOCOL_HUB_TT_SINGLE: u8 = 0x01;

/// Hub protokolü: çoklu TT
/// Her downstream port için ayrı TT devresi; daha yüksek bant genişliği
pub const USB_PROTOCOL_HUB_TT_MULTI: u8 = 0x02;

/// Hub sınıf isteği: Durum al (hub veya port)
pub const HUB_GET_STATUS: u8 = 0x00;
/// Hub sınıf isteği: Özellik bitini temizle (port değişim bayraklarını sıfırla)
pub const HUB_CLEAR_FEATURE: u8 = 0x01;
/// Hub sınıf isteği: Özellik bitini set et (port gücü aç, port sıfırla vb.)
pub const HUB_SET_FEATURE: u8 = 0x03;
/// Hub sınıf isteği: Hub/port tanımlayıcısı al
pub const HUB_GET_DESCRIPTOR: u8 = 0x06;
/// Hub sınıf isteği: Hub/port tanımlayıcısı yaz
pub const HUB_SET_DESCRIPTOR: u8 = 0x07;
/// Hub sınıf isteği: TT tamponunu temizle (hata sonrası kurtarma)
pub const HUB_CLEAR_TT_BUFFER: u8 = 0x08;
/// Hub sınıf isteği: TT'yi sıfırla
pub const HUB_RESET_TT: u8 = 0x09;
/// Hub sınıf isteği: TT durumunu al
pub const HUB_GET_TT_STATE: u8 = 0x0A;
/// Hub sınıf isteği: TT'yi durdur
pub const HUB_STOP_TT: u8 = 0x0B;

// ============================================================================
// HUB ÖZELLİK SEÇİCİLERİ (Feature Selectors)
// SET_FEATURE ve CLEAR_FEATURE isteklerinde wValue alanına yazılır
// ============================================================================

/// Hub özelliği: Yerel güç durumu değişimi
pub const HUB_C_HUB_LOCAL_POWER: u8 = 0x00;
/// Hub özelliği: Aşırı akım değişimi
pub const HUB_C_HUB_OVER_CURRENT: u8 = 0x01;

// Port özellikleri (wIndex = port numarası)
/// Port özelliği: Cihaz bağlantısı (bağlı/bağlantısız)
pub const HUB_PORT_CONNECTION: u8 = 0x00;
/// Port özelliği: Port etkin/devre dışı
pub const HUB_PORT_ENABLE: u8 = 0x01;
/// Port özelliği: Port askıda (suspend)
pub const HUB_PORT_SUSPEND: u8 = 0x02;
/// Port özelliği: Aşırı akım
pub const HUB_PORT_OVER_CURRENT: u8 = 0x03;
/// Port özelliği: Port sıfırlama (reset)
pub const HUB_PORT_RESET: u8 = 0x04;
/// Port özelliği: Port gücü (açık/kapalı)
pub const HUB_PORT_POWER: u8 = 0x08;
/// Port özelliği: Düşük hızlı cihaz
pub const HUB_PORT_LOW_SPEED: u8 = 0x09;
/// Port özelliği: Yüksek hızlı cihaz
pub const HUB_PORT_HIGH_SPEED: u8 = 0x0A;

// Port değişim bildirimleri — CLEAR_FEATURE ile temizlenmesi gerekir
/// Port değişim özelliği: Bağlantı durumu değişti
pub const HUB_C_PORT_CONNECTION: u8 = 0x10;
/// Port değişim özelliği: Etkin/devre dışı durumu değişti
pub const HUB_C_PORT_ENABLE: u8 = 0x11;
/// Port değişim özelliği: Askı durumu değişti
pub const HUB_C_PORT_SUSPEND: u8 = 0x12;
/// Port değişim özelliği: Aşırı akım değişti
pub const HUB_C_PORT_OVER_CURRENT: u8 = 0x13;
/// Port değişim özelliği: Sıfırlama tamamlandı
pub const HUB_C_PORT_RESET: u8 = 0x14;
/// Port özelliği: Test modu
pub const HUB_PORT_TEST: u8 = 0x15;
/// Port özelliği: Gösterge rengi
pub const HUB_PORT_INDICATOR: u8 = 0x16;

// ============================================================================
// HUB TANIMLAYICISI
// USB hub tanımlayıcı yapısı (USB spesifikasyonu 11.23.2.1)
// ============================================================================

/// USB hub tanımlayıcısı.
///
/// ## Bellek Düzeni
///
/// `#[repr(C, packed)]`: C ABI düzeni, hizalama dolgusu yok.
/// Bu gereklidir çünkü tanımlayıcı, USB kablosi üzerinden tam olarak
/// bu bellek düzeniyle alınır ve doğrudan cast yapılabilmelidir.
///
/// ## Power-Good Gecikmesi
///
/// `b_pwr_on_2_pwr_good * 2ms` = portlara güç verilmesinden sonra
/// cihazların stabil hale gelmesi için beklenmesi gereken süre.
/// Örnek: `b_pwr_on_2_pwr_good=50` → 100ms gecikme.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct HubDescriptor {
    /// Tanımlayıcı boyutu (basit hub için 9 byte)
    pub b_desc_length: u8,
    /// Tanımlayıcı tipi: 0x29 (Hub)
    pub b_descriptor_type: u8,
    /// Downstream port sayısı (1..255)
    pub b_nbr_ports: u8,
    /// Hub özellikleri bit alanı (wHubCharacteristics)
    /// bit0-1: güç anahtarlama modu
    /// bit2: bileşik cihaz
    /// bit3-4: aşırı akım koruması
    /// bit5-6: TT düşünme süresi (00=8, 01=16, 10=24, 11=32 FS bit süre)
    /// bit7: port gösterge desteği
    pub w_hub_characteristics: u16,
    /// Güç açıktan stabil hale gelme süresi (2ms birimi)
    pub b_pwr_on_2_pwr_good: u8,
    /// Hub kontrol devresi akımı (mA)
    pub b_hub_control_current: u8,
    /// Çıkarılabilir cihaz bit haritası (değişken uzunluk, basit için 2 byte)
    /// Her bit: ilgili port için cihaz çıkarılabilir mi?
    pub device_removable: [u8; 2],
    /// Port güç kontrol maskesi (değişken uzunluk)
    pub port_pwr_ctrl_mask: [u8; 2],
}

impl HubDescriptor {
    /// Ham baytlardan hub tanımlayıcısını ayrıştırır.
    ///
    /// En az 9 byte gereklidir.
    /// `device_removable` alanı port sayısına göre hesaplanan bitmapten okunur:
    /// `ceil(port_count / 8)` byte.
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 9 {
            return None;
        }

        let mut desc: HubDescriptor = unsafe { mem::zeroed() };
        desc.b_desc_length = data[0];
        desc.b_descriptor_type = data[1];
        desc.b_nbr_ports = data[2];
        desc.w_hub_characteristics = u16::from_le_bytes([data[3], data[4]]);
        desc.b_pwr_on_2_pwr_good = data[5];
        desc.b_hub_control_current = data[6];

        // Değişken uzunluklu alan: port sayısından hesaplanır
        let rem_len = ((desc.b_nbr_ports + 7) / 8) as usize;
        if data.len() >= 7 + rem_len {
            desc.device_removable[0] = data[7];
            if rem_len > 1 && data.len() >= 8 + rem_len {
                desc.device_removable[1] = data[8];
            }
        }

        Some(desc)
    }

    /// Downstream port sayısını döndürür.
    pub fn port_count(&self) -> u8 {
        self.b_nbr_ports
    }

    /// Hub'ın güç anahtarlaması var mı?
    /// `w_hub_characteristics` bit0=0 → her port ayrı anahtarlanabilir.
    pub fn is_power_switched(&self) -> bool {
        (self.w_hub_characteristics & 0x01) == 0
    }

    /// Tüm portlar aynı anda güçlendirilir mi? (Ganged power)
    /// `w_hub_characteristics` bit1=0 → gang power (ekonomik)
    pub fn is_ganged_power(&self) -> bool {
        (self.w_hub_characteristics & 0x02) == 0
    }

    /// Hub aşırı akım korumasına sahip mi?
    pub fn has_over_current(&self) -> bool {
        (self.w_hub_characteristics & 0x08) != 0
    }

    /// Her port için ayrı aşırı akım koruması var mı?
    pub fn has_individual_oc(&self) -> bool {
        (self.w_hub_characteristics & 0x10) != 0
    }

    /// TT düşünme süresi (FS bit süresi cinsinden).
    ///
    /// TT, HS paketini FS/LS formatına dönüştürürken bu kadar
    /// zaman dilimine (FS bit süresi) ihtiyaç duyar.
    /// Değer 0..3: 00=8, 01=16, 10=24, 11=32 FS bit süresi.
    pub fn tt_think_time(&self) -> u8 {
        ((self.w_hub_characteristics >> 5) & 0x03) as u8
    }

    /// Belirtilen port'un cihazı çıkarılabilir mi?
    ///
    /// `device_removable[port/8]` bit `port%8` → 1 ise çıkarılabilir.
    pub fn is_port_removable(&self, port: u8) -> bool {
        let byte_idx = port as usize / 8;
        let bit_idx = port as usize % 8;

        if byte_idx < self.device_removable.len() {
            (self.device_removable[byte_idx] & (1 << bit_idx)) != 0
        } else {
            false
        }
    }
}

// ============================================================================
// PORT DURUMU
// wPortStatus ve wPortChange register bit alanları
// ============================================================================

/// USB port durum kaydı (wPortStatus).
///
/// ## Bit Alanları
///
/// ```
/// bit0:  CCS  - Geçerli Bağlantı Durumu (Current Connect Status)
/// bit1:  PED  - Port Etkin/Devre Dışı (Port Enabled/Disabled)
/// bit2:  PES  - Port Askıda (Port Enabled Suspend)
/// bit3:  POC  - Aşırı Akım Aktif (Port Over-Current)
/// bit4:  PRS  - Port Sıfırlanıyor (Port Reset)
/// bit8:  PSP  - Port Güçlü (Port Powered)
/// bit9:  PLSC - Düşük Hız Cihaz Bağlı
/// bit10: PHSC - Yüksek Hız Cihaz Bağlı
/// bit11: PTC  - Port Test Modunda
/// bit12: PIC  - Port Göstergesi Aktif
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct PortStatus {
    /// Ham durum değeri (wPortStatus)
    pub raw: u16,
}

impl PortStatus {
    /// Cihaz bağlı mı? (CCS bit0)
    pub fn is_connected(&self) -> bool {
        (self.raw & (1 << 0)) != 0
    }

    /// Port etkin mi? (PED bit1)
    /// Port sıfırlanana kadar etkin olmaz.
    pub fn is_enabled(&self) -> bool {
        (self.raw & (1 << 1)) != 0
    }

    /// Port askıda mı? (PES bit2)
    pub fn is_suspended(&self) -> bool {
        (self.raw & (1 << 2)) != 0
    }

    /// Aşırı akım aktif mi? (POC bit3)
    pub fn is_over_current(&self) -> bool {
        (self.raw & (1 << 3)) != 0
    }

    /// Port sıfırlanıyor mu? (PRS bit4)
    pub fn is_reset(&self) -> bool {
        (self.raw & (1 << 4)) != 0
    }

    /// Port güçlü mü? (PSP bit8)
    pub fn is_powered(&self) -> bool {
        (self.raw & (1 << 8)) != 0
    }

    /// Bağlı cihaz düşük hızlı mı? (1.5 Mbps)
    pub fn is_low_speed(&self) -> bool {
        (self.raw & (1 << 9)) != 0
    }

    /// Bağlı cihaz yüksek hızlı mı? (480 Mbps)
    pub fn is_high_speed(&self) -> bool {
        (self.raw & (1 << 10)) != 0
    }

    /// Port test modunda mı?
    pub fn is_test_mode(&self) -> bool {
        (self.raw & (1 << 11)) != 0
    }

    /// Port göstergesi var mı?
    pub fn has_indicator(&self) -> bool {
        (self.raw & (1 << 12)) != 0
    }

    /// Bağlı cihazın hızını belirler.
    ///
    /// HS biti set ise → High (480 Mbps)
    /// LS biti set ise → Low (1.5 Mbps)
    /// Bağlı ama bilgi yoksa → Full (12 Mbps, varsayılan)
    pub fn speed(&self) -> UsbSpeed {
        if self.is_high_speed() {
            UsbSpeed::High
        } else if self.is_low_speed() {
            UsbSpeed::Low
        } else if self.is_connected() {
            UsbSpeed::Full
        } else {
            UsbSpeed::Unknown
        }
    }
}

/// USB port değişim kaydı (wPortChange).
///
/// Her bit, ilgili port durum bitinin bir önceki okumadan bu yana
/// değişip değişmediğini gösterir.
/// Değişim bitleri `CLEAR_FEATURE` isteğiyle temizlenmelidir;
/// temizlenmezse hub kesme bildirimleri devam eder.
#[derive(Clone, Copy, Debug, Default)]
pub struct PortChange {
    /// Ham değişim değeri (wPortChange)
    pub raw: u16,
}

impl PortChange {
    /// Bağlantı durumu değişti mi? (C_PORT_CONNECTION)
    pub fn connection_changed(&self) -> bool {
        (self.raw & (1 << 0)) != 0
    }

    /// Etkin/devre dışı durumu değişti mi? (C_PORT_ENABLE)
    pub fn enable_changed(&self) -> bool {
        (self.raw & (1 << 1)) != 0
    }

    /// Askı durumu değişti mi? (C_PORT_SUSPEND)
    pub fn suspend_changed(&self) -> bool {
        (self.raw & (1 << 2)) != 0
    }

    /// Aşırı akım değişti mi? (C_PORT_OVER_CURRENT)
    pub fn over_current_changed(&self) -> bool {
        (self.raw & (1 << 3)) != 0
    }

    /// Sıfırlama tamamlandı mı? (C_PORT_RESET)
    pub fn reset_changed(&self) -> bool {
        (self.raw & (1 << 4)) != 0
    }

    /// Tüm değişim bitlerini sıfırla.
    pub fn clear_all(&mut self) {
        self.raw = 0;
    }
}

/// Port durum ve değişim bilgisini birleştiren yapı.
///
/// `GET_STATUS` isteğine yanıt olarak cihaz 4 byte gönderir:
/// - Byte 0-1: wPortStatus (PortStatus)
/// - Byte 2-3: wPortChange (PortChange)
#[derive(Clone, Copy, Debug)]
pub struct PortState {
    pub status: PortStatus,
    pub change: PortChange,
}

impl PortState {
    pub fn new() -> Self {
        PortState {
            status: PortStatus::default(),
            change: PortChange::default(),
        }
    }

    /// 4 byte'lık ham GET_STATUS verisinden ayrıştırır.
    ///
    /// `u16::from_le_bytes`: USB bus little-endian formatındadır.
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 4 {
            return None;
        }

        Some(PortState {
            status: PortStatus {
                raw: u16::from_le_bytes([data[0], data[1]]),
            },
            change: PortChange {
                raw: u16::from_le_bytes([data[2], data[3]]),
            },
        })
    }
}

// ============================================================================
// USB HUB CİHAZI
// Hem kök (root) hem de harici hub'ları temsil eder
// ============================================================================

/// USB hub cihazı.
///
/// ## Kök Hub Farkı
///
/// `is_root=true` olan hub'larda:
/// - `device` alanı boş bir `UsbDevice` içerir (kullanılmaz)
/// - Port durumu xHCI PORTSC register'ından okunur
/// - Kontrol talepleri (SET_FEATURE, GET_STATUS) gönderilmez
///
/// `is_root=false` olan harici hub'larda:
/// - `device` gerçek USB hub cihazına bağlıdır
/// - Tüm işlemler USB kontrol aktarımıyla yapılır
pub struct UsbHub {
    /// Hub USB cihaz referansı (harici hub için geçerli)
    device: Arc<Mutex<UsbDevice>>,
    /// Hub tanımlayıcısı (port sayısı, TT, güç bilgileri)
    descriptor: HubDescriptor,
    /// Her portun mevcut durumu (güncellenen anlık bilgi)
    ports: Vec<PortState>,
    /// Hub USB adresi (0..127)
    address: UsbDeviceAddress,
    /// Hub topoloji derinliği (kök=0, birinci seviye=1, ...)
    tier: u8,
    /// Kök hub mu? (xHCI içindeki sanal hub)
    is_root: bool,
    /// TT'den gelen FS/LS cihazlar için TT port numarası
    tt_port: Option<u8>,
    /// Hub adı (örn. "root-hub-0", "hub-5")
    name: String,
}

impl UsbHub {
    /// USB cihaz referansından harici hub oluşturur.
    ///
    /// Adımlar:
    /// 1. `GET_DESCRIPTOR(Hub)` isteğiyle 64 byte al
    /// 2. `HubDescriptor::parse()` ile tanımlayıcıyı ayrıştır
    /// 3. `b_nbr_ports` kadar `PortState` başlat
    pub fn new(device: Arc<Mutex<UsbDevice>>, tier: u8) -> Result<Self, UsbError> {
        let address = device.lock().address;

        // Hub tanımlayıcısını al
        let desc_data = Self::get_hub_descriptor(&device)?;
        let descriptor = HubDescriptor::parse(&desc_data).ok_or(UsbError::DescriptorError)?;

        let port_count = descriptor.port_count();
        let mut ports = Vec::with_capacity(port_count as usize);
        for _ in 0..port_count {
            ports.push(PortState::new());
        }

        let name = format!("hub-{}", address);

        crate::serial_println!(
            "[USB-HUB] Found hub at address {}: {} ports",
            address,
            port_count
        );

        Ok(UsbHub {
            device,
            descriptor,
            ports,
            address,
            tier,
            is_root: false,
            tt_port: None,
            name,
        })
    }

    /// xHCI için sanal kök hub oluşturur.
    ///
    /// `controller_idx`: xHCI denetleyici numarası (birden fazla xHCI olabilir)
    /// `port_count`: xHCI'nin HCSPARAMS1'den okunan maksimum port sayısı
    ///
    /// Kök hub tanımlayıcısı sabit değerlerle oluşturulur:
    /// - `w_hub_characteristics=0x0009`: Ganged power, bireysel OC
    /// - `b_pwr_on_2_pwr_good=50`: 100ms güç stabilizasyon gecikmesi
    pub fn create_root_hub(controller_idx: usize, port_count: u8) -> Self {
        let mut ports = Vec::with_capacity(port_count as usize);
        for _ in 0..port_count {
            ports.push(PortState::new());
        }

        let name = format!("root-hub-{}", controller_idx);

        UsbHub {
            device: Arc::new(Mutex::new(UsbDevice::default())),
            descriptor: HubDescriptor {
                b_desc_length: 9,
                b_descriptor_type: 0x29,
                b_nbr_ports: port_count,
                w_hub_characteristics: 0x0009, // Ganged power, individual OC
                b_pwr_on_2_pwr_good: 50,       // 100ms
                b_hub_control_current: 0,
                device_removable: [0; 2],
                port_pwr_ctrl_mask: [0; 2],
            },
            ports,
            address: 0,
            tier: 0,
            is_root: true,
            tt_port: None,
            name,
        }
    }

    /// Hub tanımlayıcısını USB kontrol aktarımıyla alır.
    ///
    /// Setup paketi:
    /// - `request_type=0xA0`: Class, Interface → Host (Device'tan alınır)
    /// - `value=0x2900`: Tanımlayıcı tipi (0x29=Hub) << 8 | index
    /// - `length=64`: Maksimum hub tanımlayıcı boyutu
    fn get_hub_descriptor(device: &Arc<Mutex<UsbDevice>>) -> Result<Vec<u8>, UsbError> {
        let mut dev = device.lock();

        // GET_DESCRIPTOR setup paketi (Hub sınıfına özgü)
        let setup = UsbSetupPacket {
            request_type: 0xA0, // Sınıf, Arabirim → Host
            request: HUB_GET_DESCRIPTOR,
            value: 0x2900, // Tanımlayıcı tipi (Hub) << 8 | index
            index: 0,
            length: 64,
        };

        // Alım tamponu
        let mut buffer = vec![0u8; 64];

        // Kontrol aktarımı gönder
        dev.control_transfer(setup, Some(&mut buffer))?;

        Ok(buffer)
    }

    /// Downstream port sayısını döndürür.
    pub fn port_count(&self) -> u8 {
        self.descriptor.port_count()
    }

    /// Hub topoloji derinliğini döndürür (kök=0).
    pub fn tier(&self) -> u8 {
        self.tier
    }

    /// Kök hub mu?
    pub fn is_root_hub(&self) -> bool {
        self.is_root
    }

    /// Hub adını döndürür.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Tüm portlara güç verir ve güç stabilizasyon süresini bekler.
    ///
    /// Her port için `SET_FEATURE(PORT_POWER)` gönderilir.
    /// Ardından `b_pwr_on_2_pwr_good * 2ms` kadar beklenir.
    /// Bu bekleme olmadan port durumu okunursa cihazlar görünmeyebilir.
    pub fn power_on_ports(&mut self) -> Result<(), UsbError> {
        for port in 1..=self.port_count() {
            self.set_port_feature(port, HUB_PORT_POWER)?;
        }

        // Power-good gecikmesini bekle
        let delay_ms = self.descriptor.b_pwr_on_2_pwr_good as u64 * 2;
        crate::task::scheduler::sleep(delay_ms as usize);

        crate::serial_println!("[USB-HUB] Ports powered on ({}ms delay)", delay_ms);
        Ok(())
    }

    /// Belirtilen portun durum ve değişim bilgilerini alır.
    ///
    /// Port sıralaması 1'den başlar (0 geçersiz).
    /// Kök hub için xHCI PORTSC register kullanılır.
    /// Harici hub için `GET_STATUS` kontrol isteği gönderilir.
    pub fn get_port_status(&mut self, port: u8) -> Result<PortState, UsbError> {
        if port == 0 || port > self.port_count() {
            return Err(UsbError::InvalidPort);
        }

        if self.is_root {
            // Kök hub: xHCI PORTSC register'dan durum okunmuş olmalı
            return Ok(self.ports[(port - 1) as usize]);
        }

        let mut dev = self.device.lock();

        // GET_STATUS setup paketi (Other yönü, büyük harfle)
        let setup = UsbSetupPacket {
            request_type: 0xA3, // Sınıf, Other → Host
            request: HUB_GET_STATUS,
            value: 0,
            index: port as u16, // Port numarası
            length: 4,          // 4 byte: wPortStatus + wPortChange
        };

        let mut buffer = [0u8; 4];
        dev.control_transfer(setup, Some(&mut buffer))?;

        let state = PortState::parse(&buffer).ok_or(UsbError::TransferError)?;
        // Yerel önbelleği güncelle
        self.ports[(port - 1) as usize] = state;

        Ok(state)
    }

    /// Belirtilen portta bir özellik bitini set eder.
    ///
    /// `SET_FEATURE` komutu yalnızca harici hub'lara gönderilir.
    /// Kök hub port özellikleri xHCI PORTSC yazmacıyla kontrol edilir.
    pub fn set_port_feature(&mut self, port: u8, feature: u8) -> Result<(), UsbError> {
        if port == 0 || port > self.port_count() {
            return Err(UsbError::InvalidPort);
        }

        if self.is_root {
            // Kök hub: xHCI PORTSC doğrudan yönetilir
            return Ok(());
        }

        let mut dev = self.device.lock();

        // SET_FEATURE setup paketi
        let setup = UsbSetupPacket {
            request_type: 0x23, // Sınıf, Other → Device
            request: HUB_SET_FEATURE,
            value: feature as u16, // Özellik seçici
            index: port as u16,    // Port numarası
            length: 0,
        };

        dev.control_transfer(setup, None)?;

        Ok(())
    }

    /// Belirtilen portta bir özellik/değişim bitini temizler.
    ///
    /// Özellikle `CLEAR_FEATURE(C_PORT_*)` ile değişim bayrakları
    /// temizlenmeli; aksi hâlde hub hub kesme endpoint'i sürekli bildirim gönderir.
    pub fn clear_port_feature(&mut self, port: u8, feature: u8) -> Result<(), UsbError> {
        if port == 0 || port > self.port_count() {
            return Err(UsbError::InvalidPort);
        }

        if self.is_root {
            return Ok(());
        }

        let mut dev = self.device.lock();

        // CLEAR_FEATURE setup paketi
        let setup = UsbSetupPacket {
            request_type: 0x23, // Sınıf, Other → Device
            request: HUB_CLEAR_FEATURE,
            value: feature as u16, // Özellik seçici
            index: port as u16,
            length: 0,
        };

        dev.control_transfer(setup, None)?;

        Ok(())
    }

    /// Portu sıfırlar ve sıfırlama tamamlandıktan sonra cihaz hızını döndürür.
    ///
    /// ## Sıfırlama Protokolü
    ///
    /// 1. `SET_FEATURE(PORT_RESET)` → sıfırlama başlar
    /// 2. 10ms periyodlarla `GET_STATUS` ile `C_PORT_RESET` bitini bekle
    /// 3. `C_PORT_RESET=1` ise → sıfırlama tamamlandı
    /// 4. `CLEAR_FEATURE(C_PORT_RESET)` → değişim bitini temizle
    /// 5. `PED=1` (Port Enabled) ise → cihaz hazır, hızı döndür
    ///
    /// Maksimum 500ms (50 * 10ms) beklenir; aşılırsa `Timeout` hatası.
    pub fn reset_port(&mut self, port: u8) -> Result<UsbSpeed, UsbError> {
        if port == 0 || port > self.port_count() {
            return Err(UsbError::InvalidPort);
        }

        crate::serial_println!("[USB-HUB] Resetting port {}", port);

        // Port sıfırlama işlemini başlat
        self.set_port_feature(port, HUB_PORT_RESET)?;

        // Sıfırlamanın tamamlanmasını bekle (en fazla 500ms)
        for _ in 0..50 {
            crate::task::scheduler::sleep(10);

            let state = self.get_port_status(port)?;

            if state.change.reset_changed() {
                // Sıfırlama değişim bitini temizle
                self.clear_port_feature(port, HUB_C_PORT_RESET)?;

                if state.status.is_enabled() {
                    let speed = state.status.speed();
                    crate::serial_println!(
                        "[USB-HUB] Port {} reset complete, speed={:?}",
                        port,
                        speed
                    );
                    return Ok(speed);
                }
            }
        }

        // 500ms geçti, cihaz yanıt vermedi
        Err(UsbError::Timeout)
    }

    /// Bağlantı değişim bitini temizler (C_PORT_CONNECTION).
    ///
    /// Cihaz bağlandığında veya çıkarıldığında bu bit set edilir.
    /// Temizlenmezse hub sürekli kesme bildirimi gönderir.
    pub fn clear_connection_change(&mut self, port: u8) -> Result<(), UsbError> {
        self.clear_port_feature(port, HUB_C_PORT_CONNECTION)
    }

    /// Portu devre dışı bırakır (PORT_ENABLE bitini temizle).
    pub fn disable_port(&mut self, port: u8) -> Result<(), UsbError> {
        self.clear_port_feature(port, HUB_PORT_ENABLE)
    }

    /// Portu askıya alır (PORT_SUSPEND set et).
    ///
    /// Cihaz enerji tasarrufu moduna girer (U2 state).
    pub fn suspend_port(&mut self, port: u8) -> Result<(), UsbError> {
        self.set_port_feature(port, HUB_PORT_SUSPEND)
    }

    /// Portun askı durumunu kaldırır (PORT_SUSPEND temizle → K-state).
    pub fn resume_port(&mut self, port: u8) -> Result<(), UsbError> {
        self.clear_port_feature(port, HUB_PORT_SUSPEND)
    }

    /// Tüm portları sorgular ve bağlantı değişimi olan portların listesini döndürür.
    ///
    /// Her port için `GET_STATUS` gönderilir.
    /// `connection_changed()` = true olan portlar döndürülür.
    /// Bu listedeki portlar için `clear_connection_change()` + `reset_port()` yapılmalıdır.
    pub fn poll(&mut self) -> Result<Vec<u8>, UsbError> {
        let mut changed_ports = Vec::new();

        for port in 1..=self.port_count() {
            let state = self.get_port_status(port)?;

            if state.change.connection_changed() {
                changed_ports.push(port);
            }
        }

        Ok(changed_ports)
    }

    /// Belirtilen porttaki cihazın çıkarılabilir olup olmadığını kontrol eder.
    pub fn is_device_removable(&self, port: u8) -> bool {
        self.descriptor.is_port_removable(port)
    }
}

// ============================================================================
// HUB YÖNETİCİSİ
// Tüm kayıtlı hub'ları merkezi olarak tutan sözlükler
// ============================================================================

/// Global hub kaydı ve kök hub'lar listesi.
///
/// `HUB_REGISTRY`: isme göre erişim (`BTreeMap<isim, Arc<Mutex<UsbHub>>>`)
/// `ROOT_HUBS`: kök hub'ların listesi (xHCI başına bir tane)
///
/// `Arc<Mutex<UsbHub>>`:
/// - `Arc`: birden fazla referans (yönetici + port tarayıcı)
/// - `Mutex`: tek seferlik port sorgulama erişimi
lazy_static::lazy_static! {
    static ref HUB_REGISTRY: Mutex<BTreeMap<String, Arc<Mutex<UsbHub>>>> = Mutex::new(BTreeMap::new());
    static ref ROOT_HUBS: Mutex<Vec<Arc<Mutex<UsbHub>>>> = Mutex::new(Vec::new());
}

/// Hub'ı sisteme kaydeder ve `Arc<Mutex<UsbHub>>` döndürür.
///
/// Kök hub'lar ayrıca `ROOT_HUBS` listesine de eklenir.
/// `Arc::clone()`: referans sayacını artırır; orijinal Arc geçerliliğini korur.
pub fn register_hub(hub: UsbHub) -> Arc<Mutex<UsbHub>> {
    let name = hub.name().to_string();
    let is_root = hub.is_root_hub();
    let hub = Arc::new(Mutex::new(hub));

    HUB_REGISTRY.lock().insert(name.clone(), hub.clone());

    if is_root {
        ROOT_HUBS.lock().push(hub.clone());
    }

    crate::serial_println!("[USB-HUB] Registered hub: {}", name);
    hub
}

/// İsme göre hub döndürür.
///
/// `cloned()`: Arc referans sayacını artırır; kilitlenme olmaz.
pub fn get_hub(name: &str) -> Option<Arc<Mutex<UsbHub>>> {
    HUB_REGISTRY.lock().get(name).cloned()
}

/// Tüm kök hub'ları döndürür.
pub fn get_root_hubs() -> Vec<Arc<Mutex<UsbHub>>> {
    ROOT_HUBS.lock().clone()
}

/// Tüm kayıtlı hub'ları sorgular ve değişim bildiren portları döndürür.
///
/// Dönen değer: `(hub_ismi, değişen_port_numaraları)` listesi.
/// HUB_REGISTRY kilidi, her hub'ın `poll()` çağrısı süresince tutulur.
pub fn poll_all_hubs() -> Vec<(String, Vec<u8>)> {
    let mut changes = Vec::new();

    let hubs = HUB_REGISTRY.lock();
    for (name, hub) in hubs.iter() {
        if let Ok(changed_ports) = hub.lock().poll() {
            if !changed_ports.is_empty() {
                changes.push((name.clone(), changed_ports));
            }
        }
    }

    changes
}

/// Hub sürücüsünü başlatır.
pub fn init() {
    crate::serial_println!("[USB-HUB] Hub driver initialized");
}

// ============================================================================
// HUB NUMARALANDIRMA
// Hub portlarında cihaz tespiti ve numaralandırma yardımcıları
// ============================================================================

/// Cihazın USB hub olup olmadığını kontrol eder.
///
/// `device.device_class == UsbClass::Hub` → sınıf kodu 0x09
pub fn is_hub_device(device: &UsbDevice) -> bool {
    device.device_class == UsbClass::Hub
}

/// Hub portlarını numaralandırır.
///
/// ## Akış
///
/// 1. Tüm portlara güç ver → `power_on_ports()`
/// 2. Her port için `GET_STATUS` ile bağlantı durumunu kontrol et
/// 3. Bağlı + değişim biti olan portlar için:
///    a. `CLEAR_FEATURE(C_PORT_CONNECTION)` → değişim bitini temizle
///    b. `SET_FEATURE(PORT_RESET)` + bekleme → port sıfırla
///    c. Hız bilgisini al
/// 4. Cihaz numaralandırma (address 0 → yeni USB adresi) `enumerate_device()` çağrısıyla sağlanır
///
/// `enumerate_device`: Çağıran tarafından sağlanan fonksiyon pointer'ı.
/// Her cihaz default state'te (adres 0) iken bu fonksiyon çağrılır.
pub fn enumerate_hub_ports(
    hub: &mut UsbHub,
    enumerate_device: fn(&mut UsbDevice, u8) -> Result<(), UsbError>,
) -> Result<(), UsbError> {
    // Portlara güç ver (bekleme dahil)
    hub.power_on_ports()?;

    // Her portu kontrol et
    for port in 1..=hub.port_count() {
        let state = hub.get_port_status(port)?;

        if state.status.is_connected() && state.change.connection_changed() {
            // Bağlantı değişim bitini temizle
            hub.clear_connection_change(port)?;

            // Portu sıfırla ve hız bilgisini al
            let speed = hub.reset_port(port)?;

            crate::serial_println!("[USB-HUB] Port {} has device, speed={:?}", port, speed);

            // Bu noktada cihaz Default state'te ve adres 0'da
            // Numaralandırma çağırıcı tarafından yapılır
        }
    }

    Ok(())
}
