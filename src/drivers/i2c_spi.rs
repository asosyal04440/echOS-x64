//! # I2C ve SPI Alt Sistemi
//!
//! Gömülü donanımlarda çok yaygın kullanılan iki seri bus protokolünün sürücüsü.
//!
//! ## I2C (Inter-Integrated Circuit) Nedir?
//!
//! Philips (NXP) tarafından geliştirilen 2 kablolu seri veri yolu.
//! Yalnızca **SDA** (veri) ve **SCL** (saat) hatları kullanılır.
//!
//! ```
//!   Master (CPU)
//!     |         |
//!    SDA       SCL
//!     |         |
//!   +-----------+---+---+---+
//!   |           |   |   |   |
//!  Slave0     Slave1 Slave2 ... (7-bit adresle seçilir)
//! ```
//!
//! Hız modları:
//!   Standard   ->  100 kHz  (eski sensörler)
//!   Fast       ->  400 kHz  (LCD, IMU)
//!   Fast+      ->    1 MHz
//!   High Speed ->  3.4 MHz  (yüksek hızlı bellek)
//!
//! ## SPI (Serial Peripheral Interface) Nedir?
//!
//! 4 kablolu tam-çift yönlü seri bus:
//!
//! ```
//!   Master (CPU)
//!     |    |    |    |
//!    MOSI MISO SCLK  CS0  CS1  CS2
//!                     |    |    |
//!                  Flash  ADC  LCD
//! ```
//!
//!   MOSI = Master Out Slave In
//!   MISO = Master In Slave Out
//!   SCLK = Serial Clock
//!   CS   = Chip Select (her cihaz için ayrı)
//!
//! SPI Modları (CPOL x CPHA kombinasyonu):
//!   Mod 0: CPOL=0, CPHA=0  ->  en yaygın
//!   Mod 1: CPOL=0, CPHA=1
//!   Mod 2: CPOL=1, CPHA=0
//!   Mod 3: CPOL=1, CPHA=1

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use spin::Mutex;

// ============================================================================
// I2C SABİTLERİ (I2C CONSTANTS)
// ============================================================================

/// I2C standart mod: 100 kHz (eski ve basit sensörler)
pub const I2C_STANDARD_MODE: u32 = 100_000;
/// I2C hızlı mod: 400 kHz (modern sensörler ve ekranlar)
pub const I2C_FAST_MODE: u32 = 400_000;
/// I2C hızlı mod plus: 1 MHz
pub const I2C_FAST_MODE_PLUS: u32 = 1_000_000;
/// I2C yüksek hız modu: 3.4 MHz (yoğun veri transferi)
pub const I2C_HIGH_SPEED_MODE: u32 = 3_400_000;

// I2C ioctl komut sabitleri (Linux i2c-dev.h ile uyumlu)
pub const I2C_RETRIES: u32 = 0x0701;  // Kaç kere NACK sonrası tekrar dene
pub const I2C_TIMEOUT: u32 = 0x0702;  // Zaman aşımı (10ms biriminde)
pub const I2C_SLAVE: u32 = 0x0703;    // Hedef slave adresini ayarla
pub const I2C_SLAVE_FORCE: u32 = 0x0706; // Kullanımdaki adresi zorla ayarla
pub const I2C_TENBIT: u32 = 0x0704;   // 10-bit adresleme etkinleştir
pub const I2C_RDWR: u32 = 0x0707;     // Birleşik okuma/yazma işlemi
pub const I2C_PEC: u32 = 0x0708;      // SMBus paket hata denetimi (PEC)
pub const I2C_SMBUS: u32 = 0x0720;    // SMBus transfer

// ============================================================================
// I2C MESAJ YAPISI (I2C MESSAGE)
// ============================================================================

// I2C transferi bir veya birden fazla mesajdan oluşur.
// Birleşik işlemde (combined transaction) STOP biti araya girmez:
//
//   [START | addr+W | reg | REPEATED-START | addr+R | data | STOP]
//
// Bu pattern SMBus okuma için standarttır.

#[derive(Clone, Debug)]
pub struct I2cMsg {
    /// Hedef slave'in 7-bit (veya 10-bit) I2C adresi
    pub addr: u16,
    /// Mesaj bayrakları (I2C_M_RD, I2C_M_TEN vb.)
    pub flags: u16,
    /// Veri tamponu (yazma için gönderilecek, okuma için doldurulacak)
    pub buf: Vec<u8>,
    /// Tampon uzunluğu
    pub len: u16,
}

// I2C mesaj bayrakları (Linux i2c.h ile uyumlu)
/// Okuma yönü: master slave'den veri alır
pub const I2C_M_RD: u16 = 0x0001;
/// 10-bit adres kullan (7-bit yerine)
pub const I2C_M_TEN: u16 = 0x0010;
/// DMA güvenli tampon kullan
pub const I2C_M_DMA_SAFE: u16 = 0x0020;
/// Yanıt uzunluğunu slave'den al (SMBus block read)
pub const I2C_M_RECV_LEN: u16 = 0x0400;
/// Okuma ACK'sini atla
pub const I2C_M_NO_RD_ACK: u16 = 0x0800;
/// NACK'ı yoksay (devam et)
pub const I2C_M_IGNORE_NAK: u16 = 0x1000;
/// Adres yönünü tersine çevir
pub const I2C_M_REV_DIR_ADDR: u16 = 0x2000;
/// Transfer sonunda STOP biti gönder
pub const I2C_M_STOP: u16 = 0x8000;

// ============================================================================
// I2C ADAPTÖR (I2C ADAPTER)
// ============================================================================

// I2C bus denetleyicisini temsil eder.
// Linux'ta /dev/i2c-0, /dev/i2c-1 vb. olarak görünür.
//
// Fiziksel altyapı:
//
//   CPU SoC içindeki I2C denetleyicisi
//       |
//   I2cAdapter (nr=0, name="i2c-smbus0")
//       |
//       +-- 0x50: EEPROM (I2cClient)
//       +-- 0x68: RTC    (I2cClient)
//       +-- 0x3C: OLED   (I2cClient)

pub struct I2cAdapter {
    /// Bus numarası (0'dan başlar)
    pub nr: u32,
    /// İnsan okunabilir adaptör adı
    pub name: String,
    /// Bus frekansı (Hz cinsinden)
    pub frequency: AtomicU32,
    /// 10-bit adresleme desteği
    pub ten_bit: AtomicBool,
    /// MMIO taban adresi (0 = henüz atanmadı)
    pub base_addr: u64,
    /// Bu bus üzerindeki kayıtlı client'lar
    pub clients: Mutex<Vec<Arc<I2cClient>>>,
    /// Bus seviyesi kilit (eş zamanlı transfer önlenir)
    pub lock: Mutex<()>,
}

impl I2cAdapter {
    pub fn new(nr: u32, name: &str) -> Self {
        Self {
            nr,
            name: String::from(name),
            frequency: AtomicU32::new(I2C_STANDARD_MODE),
            ten_bit: AtomicBool::new(false),
            base_addr: 0,
            clients: Mutex::new(Vec::new()),
            lock: Mutex::new(()),
        }
    }

    /// Bir veya birden fazla I2C mesajını gönderir.
    /// Lock alınarak eş zamanlı erişim önlenir.
    pub fn transfer(&self, msgs: &mut [I2cMsg]) -> Result<u32, I2cError> {
        let _lock = self.lock.lock();

        for msg in msgs.iter() {
            self.do_transfer(msg)?;
        }

        Ok(msgs.len() as u32)
    }

    /// Tek bir I2C mesajı donanıma gönderir (donanıma özgü kısım)
    fn do_transfer(&self, msg: &I2cMsg) -> Result<(), I2cError> {
        // I2C MMIO yazmacı üzerinden transfer gerçekleştir
        let base = self.base_addr;
        if base == 0 {
            return Err(I2cError::BusError);
        }

        unsafe {
            // 1. Hedef adres yazmacına slave adresini yaz
            let tar_reg = base as *mut u32;
            core::ptr::write_volatile(tar_reg, msg.addr as u32);

            // 2. Kontrol yazmacı: master modu, hız, 7-bit adres
            let ctrl_reg = (base + 0x04) as *mut u32;
            let ctrl_val = 0x65; // Master, 7-bit, fast mode
            core::ptr::write_volatile(ctrl_reg, ctrl_val);

            // 3. Enable yazmacı
            let enable_reg = (base + 0x6C) as *mut u32;
            core::ptr::write_volatile(enable_reg, 1);

            // 4. Veri gönder/al
            let data_cmd_reg = (base + 0x10) as *mut u32;
            if msg.flags & 1 != 0 {
                // Okuma: READ komutu gönder
                for _ in 0..msg.len {
                    core::ptr::write_volatile(data_cmd_reg, 0x100); // READ cmd
                }
            } else {
                // Yazma
                for i in 0..msg.len as usize {
                    if i < msg.buf.len() {
                        core::ptr::write_volatile(data_cmd_reg, msg.buf[i] as u32);
                    }
                }
            }

            crate::serial_println!("[I2C] Transfer: addr={:#x} len={} {}",
                msg.addr, msg.len, if msg.flags & 1 != 0 { "READ" } else { "WRITE" });
        }

        Ok(())
    }

    /// SMBus tek byte okuma: slave'den 1 byte alır
    pub fn smbus_read_byte(&self, addr: u16) -> Result<u8, I2cError> {
        let mut msg = I2cMsg {
            addr,
            flags: I2C_M_RD,
            buf: vec![0],
            len: 1,
        };
        self.transfer(&mut [msg])?;
        Ok(msg.buf[0])
    }

    /// SMBus tek byte yazma: slave'e 1 byte gönderir
    pub fn smbus_write_byte(&self, addr: u16, value: u8) -> Result<(), I2cError> {
        let msg = I2cMsg {
            addr,
            flags: 0,
            buf: vec![value],
            len: 1,
        };
        self.transfer(&mut [msg.clone()])?;
        Ok(())
    }

    /// SMBus register okuma: önce reg adresini yazar, sonra 1 byte okur
    pub fn smbus_read_byte_data(&self, addr: u16, reg: u8) -> Result<u8, I2cError> {
        let mut msgs = [
            I2cMsg { addr, flags: 0, buf: vec![reg], len: 1 },
            I2cMsg { addr, flags: I2C_M_RD, buf: vec![0], len: 1 },
        ];
        self.transfer(&mut msgs)?;
        Ok(msgs[1].buf[0])
    }

    /// SMBus register yazma: [reg, value] çiftini tek mesajda gönderir
    pub fn smbus_write_byte_data(&self, addr: u16, reg: u8, value: u8) -> Result<(), I2cError> {
        let msg = I2cMsg {
            addr,
            flags: 0,
            buf: vec![reg, value],
            len: 2,
        };
        self.transfer(&mut [msg])?;
        Ok(())
    }

    /// Bus'a yeni bir slave client kaydeder
    pub fn add_client(&self, client: Arc<I2cClient>) {
        self.clients.lock().push(client);
    }
}

// ============================================================================
// I2C CLIENT (I2C İSTEMCİSİ)
// ============================================================================

// Bus üzerindeki tek bir I2C slave cihazını temsil eder.
// Örnek: 0x68 adresindeki MPU-6050 IMU sensörü

pub struct I2cClient {
    /// Slave cihaz adı (örn. "mpu6050", "ds3231")
    pub name: String,
    /// 7-bit slave adresi (0x08 - 0x77 arası geçerli)
    pub addr: u16,
    /// Bağlı olduğu I2C adaptörü
    pub adapter: Arc<I2cAdapter>,
    /// Sürücüye özgü özel veri (örn. kalibrasyon değerleri)
    pub driver_data: Mutex<u64>,
}

impl I2cClient {
    pub fn new(name: &str, addr: u16, adapter: Arc<I2cAdapter>) -> Self {
        Self {
            name: String::from(name),
            addr,
            adapter,
            driver_data: Mutex::new(0),
        }
    }
}

// ============================================================================
// SPI SABİTLERİ (SPI CONSTANTS)
// ============================================================================

// SPI modları CPOL (saat polaritesi) ve CPHA (saat fazı) kombinasyonudur:
//
//   CPOL=0: Boşta düşük  | CPOL=1: Boşta yüksek
//   CPHA=0: İlk kenarda örnek al | CPHA=1: İkinci kenarda örnek al
//
//   Mod 0 (CPOL=0, CPHA=0): En yaygın; SD kart, SPI flash, birçok sensör
//   Mod 3 (CPOL=1, CPHA=1): Bazı ADC'ler ve DAC'lar

/// SPI Mod 0: CPOL=0, CPHA=0 - düşük boşta, ilk kenarda örnek
pub const SPI_MODE_0: u32 = 0;
/// SPI Mod 1: CPOL=0, CPHA=1
pub const SPI_MODE_1: u32 = 1;
/// SPI Mod 2: CPOL=1, CPHA=0
pub const SPI_MODE_2: u32 = 2;
/// SPI Mod 3: CPOL=1, CPHA=1
pub const SPI_MODE_3: u32 = 3;

// SPI bayrak bitleri
pub const SPI_CPHA: u32 = 0x01;          // Saat fazı
pub const SPI_CPOL: u32 = 0x02;          // Saat polaritesi
pub const SPI_MODE_0_MASK: u32 = 0;
pub const SPI_MODE_1_MASK: u32 = SPI_CPHA;
pub const SPI_MODE_2_MASK: u32 = SPI_CPOL;
pub const SPI_MODE_3_MASK: u32 = SPI_CPHA | SPI_CPOL;
/// CS aktiflik seviyesi: normal düşük, bu bit ile yüksek
pub const SPI_CS_HIGH: u32 = 0x04;
/// En düşük anlamlı bit önce gönder (varsayılan: MSB önce)
pub const SPI_LSB_FIRST: u32 = 0x08;
/// 3 kablolu mod: MOSI ve MISO aynı hattı paylaşır
pub const SPI_3WIRE: u32 = 0x10;
/// Loopback testi modu
pub const SPI_LOOP: u32 = 0x20;
/// CS sinyali kullanma (CS-less mode)
pub const SPI_NO_CS: u32 = 0x40;
/// Hazırlık sinyali bekle
pub const SPI_READY: u32 = 0x80;

// ============================================================================
// SPI MESAJ VE TRANSFER YAPILARI (SPI MESSAGE)
// ============================================================================

// SPI transferi birden fazla segment (parça) içerebilir.
// Her segmentte CS değişmeden art arda byte'lar gönderilir.
//
// Örnek: Flash bellek okuma
//   Segment 1: [0x03, addr_hi, addr_lo] gönder  (komut)
//   Segment 2: N byte oku                        (veri)
//
//   cs_change=false: CS iki segment arasında alçak kalır

#[derive(Clone, Debug)]
pub struct SpiMessage {
    /// Transfer segmentlerinin listesi
    pub segments: Vec<SpiTransfer>,
}

#[derive(Clone, Debug)]
pub struct SpiTransfer {
    /// Gönderilecek veri (master -> slave)
    pub tx_buf: Vec<u8>,
    /// Alınacak veri (slave -> master)
    pub rx_buf: Vec<u8>,
    /// Transfer uzunluğu (byte)
    pub len: usize,
    /// Bu segment için özel hız (0 = denetleyici varsayılanı)
    pub speed_hz: u32,
    /// Segmentten sonra bekleme süresi (mikrosaniye)
    pub delay_usecs: u16,
    /// Kelime başına bit sayısı (genellikle 8)
    pub bits_per_word: u8,
    /// true = segment sonunda CS yükselt (cihazı deselect et)
    pub cs_change: bool,
}

// ============================================================================
// SPI DENETLEYİCİ (SPI CONTROLLER)
// ============================================================================

// SPI bus master denetleyicisini temsil eder.
// Linux'ta /dev/spidev0.0, /dev/spidev0.1 vb. olarak görünür.
// Bir denetleyicide birden fazla CS hattı -> birden fazla cihaz.

pub struct SpiController {
    /// Denetleyici numarası
    pub nr: u32,
    /// Denetleyici adı (örn. "spi0", "spi-bcm2835")
    pub name: String,
    /// Bus numarası (/dev/spidev{bus_num}.x)
    pub bus_num: u32,
    /// Maksimum desteklenen hız (Hz)
    pub max_speed_hz: u32,
    /// Kelime başına bit sayısı (atomik: çalışma zamanında değişebilir)
    pub bits_per_word: AtomicU32,
    /// Aktif SPI modu (CPOL/CPHA)
    pub mode: AtomicU32,
    /// MMIO taban adresi (0 = henüz atanmadı)
    pub base_addr: u64,
    /// Bu bus üzerindeki kayıtlı SPI cihazları
    pub devices: Mutex<Vec<Arc<SpiDevice>>>,
    /// Bus seviyesi kilit
    pub lock: Mutex<()>,
}

impl SpiController {
    pub fn new(nr: u32, name: &str, bus_num: u32) -> Self {
        Self {
            nr,
            name: String::from(name),
            bus_num,
            max_speed_hz: 50_000_000, // 50 MHz varsayılan
            bits_per_word: AtomicU32::new(8),
            mode: AtomicU32::new(SPI_MODE_0),
            base_addr: 0,
            devices: Mutex::new(Vec::new()),
            lock: Mutex::new(()),
        }
    }

    /// SPI mesajını gönderir; her segment sırayla işlenir
    pub fn transfer(&self, msg: &SpiMessage) -> Result<u32, SpiError> {
        let _lock = self.lock.lock();

        for segment in &msg.segments {
            self.do_transfer(segment)?;
        }

        Ok(msg.segments.len() as u32)
    }

    /// Tek bir SPI segmentini donanıma gönderir (donanıma özgü kısım)
    fn do_transfer(&self, transfer: &SpiTransfer) -> Result<(), SpiError> {
        let base = self.base_addr;
        if base == 0 {
            return Err(SpiError::BusError);
        }

        unsafe {
            // 1. CS (Chip Select) aktif et
            let cs_reg = (base + 0x10) as *mut u32;
            core::ptr::write_volatile(cs_reg, 1);

            // 2. FIFO'ya TX verisini yaz
            let tx_fifo = (base + 0x08) as *mut u32;
            for i in 0..transfer.len as usize {
                if i < transfer.tx_buf.len() {
                    core::ptr::write_volatile(tx_fifo, transfer.tx_buf[i] as u32);
                }
            }

            // 3. Transfer başlat
            let ctrl_reg = base as *mut u32;
            core::ptr::write_volatile(ctrl_reg, 0x01); // Enable + transfer start

            // 4. Transfer tamamlanmasını bekle (status register poll)
            let status_reg = (base + 0x04) as *const u32;
            let mut timeout = 10000u32;
            while core::ptr::read_volatile(status_reg) & 0x01 == 0 {
                timeout -= 1;
                if timeout == 0 {
                    break;
                }
            }

            // 5. CS deaktif et
            core::ptr::write_volatile(cs_reg, 0);

            crate::serial_println!("[SPI] Transfer: len={} speed={}Hz",
                transfer.len, transfer.speed_hz);
        }

        Ok(())
    }

    /// Sadece yazma işlemi: tx_buf'u gönderir, rx verisi ihmal edilir
    pub fn write(&self, data: &[u8]) -> Result<(), SpiError> {
        let msg = SpiMessage {
            segments: vec![SpiTransfer {
                tx_buf: data.to_vec(),
                rx_buf: Vec::new(),
                len: data.len(),
                speed_hz: self.max_speed_hz,
                delay_usecs: 0,
                bits_per_word: 8,
                cs_change: false,
            }],
        };
        self.transfer(&msg)?;
        Ok(())
    }

    /// Sadece okuma işlemi: dummy byte gönderir, slave'den veri alır
    pub fn read(&self, len: usize) -> Result<Vec<u8>, SpiError> {
        let msg = SpiMessage {
            segments: vec![SpiTransfer {
                tx_buf: vec![0; len], // SPI tam çift yönlü; okurken dummy gönder
                rx_buf: vec![0; len],
                len,
                speed_hz: self.max_speed_hz,
                delay_usecs: 0,
                bits_per_word: 8,
                cs_change: false,
            }],
        };
        self.transfer(&msg)?;
        Ok(msg.segments[0].rx_buf.clone())
    }

    /// Bus'a yeni SPI cihazı ekler
    pub fn add_device(&self, device: Arc<SpiDevice>) {
        self.devices.lock().push(device);
    }
}

// ============================================================================
// SPI CİHAZI (SPI DEVICE)
// ============================================================================

// Bir CS hattına bağlı tek bir SPI slave cihazı.
// Örnek: 0 numaralı CS'ye bağlı W25Q128 SPI flash

pub struct SpiDevice {
    /// Cihaz adı (örn. "w25q128", "mcp3204")
    pub name: String,
    /// Chip select numarası (0-N)
    pub chip_select: u8,
    /// Bağlı SPI denetleyicisi
    pub controller: Arc<SpiController>,
    /// Bu cihazın maksimum hızı (denetleyici maksimumunu aşamaz)
    pub max_speed_hz: u32,
    /// Cihazın gerektirdiği SPI modu
    pub mode: u32,
    /// Sürücüye özgü özel veri
    pub driver_data: Mutex<u64>,
}

impl SpiDevice {
    pub fn new(name: &str, cs: u8, controller: Arc<SpiController>) -> Self {
        Self {
            name: String::from(name),
            chip_select: cs,
            controller: controller.clone(),
            max_speed_hz: controller.max_speed_hz,
            mode: SPI_MODE_0,
            driver_data: Mutex::new(0),
        }
    }
}

// ============================================================================
// YÖNETİCİ YAPILARI (MANAGERS)
// ============================================================================

// İki ayrı yönetici: I2cManager ve SpiManager
// Her biri kendi bus türü için adaptör/denetleyici kayıtlarını tutar.

pub struct I2cManager {
    adapters: Mutex<BTreeMap<u32, Arc<I2cAdapter>>>,
    next_nr: AtomicU32,
}

impl I2cManager {
    pub const fn new() -> Self {
        Self {
            adapters: Mutex::new(BTreeMap::new()),
            next_nr: AtomicU32::new(0),
        }
    }

    pub fn register(&self, name: &str) -> Arc<I2cAdapter> {
        let nr = self.next_nr.fetch_add(1, Ordering::SeqCst);
        let adapter = Arc::new(I2cAdapter::new(nr, name));
        self.adapters.lock().insert(nr, adapter.clone());
        adapter
    }

    pub fn get(&self, nr: u32) -> Option<Arc<I2cAdapter>> {
        self.adapters.lock().get(&nr).cloned()
    }
}

pub struct SpiManager {
    controllers: Mutex<BTreeMap<u32, Arc<SpiController>>>,
    next_nr: AtomicU32,
}

impl SpiManager {
    pub const fn new() -> Self {
        Self {
            controllers: Mutex::new(BTreeMap::new()),
            next_nr: AtomicU32::new(0),
        }
    }

    pub fn register(&self, name: &str, bus_num: u32) -> Arc<SpiController> {
        let nr = self.next_nr.fetch_add(1, Ordering::SeqCst);
        let controller = Arc::new(SpiController::new(nr, name, bus_num));
        self.controllers.lock().insert(nr, controller.clone());
        controller
    }

    pub fn get(&self, nr: u32) -> Option<Arc<SpiController>> {
        self.controllers.lock().get(&nr).cloned()
    }
}

lazy_static::lazy_static! {
    pub static ref I2C_MANAGER: I2cManager = I2cManager::new();
    pub static ref SPI_MANAGER: SpiManager = SpiManager::new();
}

// ============================================================================
// HATA TÜRLERİ (ERROR TYPES)
// ============================================================================

// I2C hataları:
//   Nack            -> Slave adresi yanıtsız (cihaz yok veya meşgul)
//   Timeout         -> SCL hattı takılı kaldı (clock stretching timeout)
//   ArbitrationLost -> Çoklu master bus çakışması

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum I2cError {
    Nack,             // Slave yanıt vermedi
    Timeout,          // Zaman aşımı
    ArbitrationLost,  // Bus arbitrasyon kaybı
    BusError,         // Donanım hatası
    NoDevice,         // Cihaz bulunamadı
}

// SPI hataları:
//   Timeout  -> Transfer tamamlanmadı
//   BusError -> Donanım veya DMA hatası

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpiError {
    Timeout,   // Transfer zaman aşımı
    BusError,  // Bus/donanım hatası
    NoDevice,  // Cihaz bulunamadı
}

// ============================================================================
// BAŞLATMA (INITIALIZATION)
// ============================================================================

pub fn init() {
    crate::serial_println!("[I2C/SPI] Subsystem initialized");
}
