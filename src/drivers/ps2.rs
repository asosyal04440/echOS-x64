//! # echOS PS/2 Klavye Kontrolcüsü (8042)
//!
//! PS/2 Klavye Kontrolcüsü (Intel 8042) için sürücü implementasyonu.
//! ExitBootServices sonrası klavyeyi başlatır ve yapılandırır.
//!
//! ## PS/2 Kontrolcü Mimarisi
//!
//! ```text
//!  +--------+    Port 0x60 (Veri R/W)     +------------------+
//!  |        |<--------------------------->|                  |
//!  |  CPU   |    Port 0x64 (Durum - RO)   |   Intel 8042     |
//!  |        |<--------------------------- |  PS/2 Kontrolcü  |
//!  |        |    Port 0x64 (Komut - WO)   |                  |
//!  |        |--------------------------->| Port 1 | Port 2  |
//!  +--------+                             +----+--------+----+
//!                                              |        |
//!                                         +---+--+  +--+---+
//!                                         | KBD  |  | Mouse|
//!                                         | PS/2 |  | PS/2 |
//!                                         +------+  +------+
//!
//!  Port 0x60: Veri portu (hem okuma hem yazma)
//!  Port 0x64: Okuma = Durum yazmacı, Yazma = Komut yazmacı
//! ```
//!
//! ## Durum Yazmacı (Port 0x64) Bitleri
//!
//! ```text
//!  Bit | Ad            | Açıklama
//!  ----+---------------+------------------------------------------
//!   0  | Output Full   | 1 = Okunmayı bekleyen veri var (port 0x60)
//!   1  | Input Full    | 1 = Kontrolcü meşgul, komut/veri yazmayı bekle
//!   2  | System Flag   | POST sonrası 1 olur
//!   3  | Command/Data  | 0=0x60'a veri, 1=0x60'a komut gönderildi
//!   5  | Timeout Error | İletişim zaman aşımı
//!   6  | Parity Error  | Eşlik biti hatası
//! ```
//!
//! ## Başlatma Sırası (Init Sequence)
//!
//! ```text
//!  1. Buffer'ı temizle (flush)
//!  2. Port 1 ve Port 2'yi devre dışı bırak (0xAD, 0xA7)
//!  3. Konfigürasyon byte'ını oku (komut 0x20, port 0x60'tan oku)
//!  4. Konfigürasyon: IRQ kapat, saat etkinleştir
//!  5. Kontrolcü öz-testi (komut 0xAA) -> 0x55 beklenir
//!  6. Port 1 testi (komut 0xAB) -> 0x00 beklenir
//!  7. Port 1'i etkinleştir (0xAE)
//!  8. Klavyeye tarama etkinleştir komutu gönder (0xF4)
//!  9. ACK (0xFA) bekle
//! 10. IRQ'yu etkinleştir (konfigürasyon byte bit0=1)
//! ```
//!
//! ## Scan Code Set 1 - Örnek Tuş Kodları
//!
//! ```text
//!  Tuş        | Make (Basıldı) | Break (Bırakıldı)
//!  -----------+----------------+-------------------
//!  A          | 0x1E           | 0x9E (= 0x1E | 0x80)
//!  Enter      | 0x1C           | 0x9C
//!  Space      | 0x39           | 0xB9
//!  Left Shift | 0x2A           | 0xAA
//!  Escape     | 0x01           | 0x81
//!
//!  Break kodu = Make kodu | 0x80
//! ```

use core::cell::UnsafeCell;
use x86_64::instructions::port::{Port, PortReadOnly, PortWriteOnly};

/// Veri portu: klavye scan kodları bu porttan okunur, komut verileri buraya yazılır.
const DATA_PORT: u16 = 0x60;
/// Durum portu: kontrolcünün hazır olup olmadığını buradan kontrol edebiliriz.
const STATUS_PORT: u16 = 0x64;
/// Komut portu: 8042 kontrolcüsüne komutlar buraya yazılır.
const COMMAND_PORT: u16 = 0x64;

/// 8042 PS/2 Kontrolcüsüne gönderilen komut sabitleri.
/// Her komut, komut portuna (0x64) yazılır.
mod commands {
    /// Konfigürasyon byte'ını oku (yanıtı port 0x60'tan oku)
    pub const READ_CONFIG: u8 = 0x20;
    /// Konfigürasyon byte'ını yaz (ardından veriyi port 0x60'a yaz)
    pub const WRITE_CONFIG: u8 = 0x60;
    /// Port 1 (Klavye) devre dışı bırak
    pub const DISABLE_PORT1: u8 = 0xAD;
    /// Port 1 (Klavye) etkinleştir
    pub const ENABLE_PORT1: u8 = 0xAE;
    /// Port 2 (Fare) devre dışı bırak
    pub const DISABLE_PORT2: u8 = 0xA7;
    #[allow(dead_code)]
    /// Port 2 (Fare) etkinleştir
    pub const ENABLE_PORT2: u8 = 0xA8;
    /// Kontrolcü öz-testi: başarı yanıtı 0x55'tir
    pub const SELF_TEST: u8 = 0xAA;
    /// Port 1 (Klavye) arayüz testi: başarı yanıtı 0x00'dır
    pub const TEST_PORT1: u8 = 0xAB;
}

/// Durum yazmacı bayrakları.
/// Bu bitler Port 0x64'ten okunarak kontrolcünün durumu anlaşılır.
mod status {
    /// Bit 0: Çıkış tamponu dolu (1 = veri okunabilir)
    pub const OUTPUT_FULL: u8 = 0x01;
    /// Bit 1: Giriş tamponu dolu (1 = kontrolcü meşgul, yazmayı bekle)
    pub const INPUT_FULL: u8 = 0x02;
}

/// PS/2 Kontrolcü arayüzü.
/// Üç ayrı port erişimcisi içerir: veri (R/W), durum (RO) ve komut (WO).
pub struct Ps2Controller {
    data: Port<u8>,
    status: PortReadOnly<u8>,
    command: PortWriteOnly<u8>,
}

impl Ps2Controller {
    pub const fn new() -> Self {
        Self {
            data: Port::new(DATA_PORT),
            status: PortReadOnly::new(STATUS_PORT),
            command: PortWriteOnly::new(COMMAND_PORT),
        }
    }

    /// Çıkış tamponu dolana kadar bekler (veri okunabilir hale gelene dek).
    /// Durum yazmacının Output Full biti (bit0) 1 olana kadar spin-loop uygular.
    /// En fazla 100000 döngüde zaman aşımına uğrar.
    fn wait_output(&mut self) {
        let mut timeout = 100000;
        while timeout > 0 {
            if unsafe { self.status.read() } & status::OUTPUT_FULL != 0 {
                return;
            }
            timeout -= 1;
            core::hint::spin_loop();
        }
    }

    /// Giriş tamponu boşalana kadar bekler (komut gönderilebilir hale gelene dek).
    /// Durum yazmacının Input Full biti (bit1) 0 olana kadar spin-loop uygular.
    fn wait_input(&mut self) {
        let mut timeout = 100000;
        while timeout > 0 {
            if unsafe { self.status.read() } & status::INPUT_FULL == 0 {
                return;
            }
            timeout -= 1;
            core::hint::spin_loop();
        }
    }

    /// Kontrolcüye komut gönderir.
    /// Önce giriş tamponunun boşalmasını bekler, ardından komut portuna yazar.
    fn send_command(&mut self, cmd: u8) {
        self.wait_input();
        unsafe {
            self.command.write(cmd);
        }
    }

    /// Kontrolcüden veri okur.
    /// Önce çıkış tamponunun dolmasını bekler, ardından veri portundan okur.
    fn read_data(&mut self) -> u8 {
        self.wait_output();
        unsafe { self.data.read() }
    }

    /// Kontrolcüye veri yazar.
    /// Önce giriş tamponunun boşalmasını bekler, ardından veri portuna yazar.
    fn write_data(&mut self, data: u8) {
        self.wait_input();
        unsafe {
            self.data.write(data);
        }
    }

    /// Çıkış tamponunu temizler.
    /// Bekleyen, işlenmemiş baytları okuyup atar. Başlatma öncesi temiz durum sağlar.
    fn flush(&mut self) {
        for _ in 0..100 {
            if unsafe { self.status.read() } & status::OUTPUT_FULL != 0 {
                unsafe {
                    self.data.read();
                }
            } else {
                break;
            }
        }
    }

    /// PS/2 klavye kontrolcüsünü başlatır.
    ///
    /// Adım adım:
    /// 1. Buffer temizleme
    /// 2. Portları devre dışı bırak
    /// 3. Konfigürasyon byte'ını oku ve IRQ'yu kapat
    /// 4. Kontrolcü öz-testi (0x55 beklenir)
    /// 5. Port 1 testi (0x00 beklenir)
    /// 6. Port 1'i etkinleştir ve taramayı başlat (0xF4)
    /// 7. ACK (0xFA) bekle
    /// 8. IRQ bit0'ı tekrar aç
    pub fn init(&mut self) -> bool {
        crate::serial_println!("PS/2: Kontrolcü başlatılıyor...");

        self.flush();

        // Cihazları devre dışı bırak
        self.send_command(commands::DISABLE_PORT1);
        self.send_command(commands::DISABLE_PORT2);

        self.flush();

        // Konfigürasyon byte'ını oku
        self.send_command(commands::READ_CONFIG);
        let mut config = self.read_data();
        crate::serial_println!("PS/2: Config byte = 0x{:02X}", config);

        // Başlangıçta IRQ'ları kapat (bit0=0), saat sinyalini etkinleştir (bit4=0)
        config &= !0x01; // Port 1 IRQ devre dışı
        config &= !0x10; // Port 1 saati etkinleştir

        self.send_command(commands::WRITE_CONFIG);
        self.write_data(config);

        // Kontrolcü Öz-Testi: başarı koşulu yanıtın 0x55 olması
        self.send_command(commands::SELF_TEST);
        let result = self.read_data();
        if result != 0x55 {
            crate::serial_println!("PS/2: Self-test BAŞARISIZ! (0x{:02X})", result);
            return false;
        }
        crate::serial_println!("PS/2: Self-test başarılı");

        // Öz-test sonrası konfigürasyon sıfırlanmış olabilir, yeniden yaz
        self.send_command(commands::WRITE_CONFIG);
        self.write_data(config);

        // Port 1 Arayüz Testi: başarı koşulu yanıtın 0x00 olması
        self.send_command(commands::TEST_PORT1);
        let result = self.read_data();
        if result != 0x00 {
            crate::serial_println!("PS/2: Port 1 testi BAŞARISIZ! (0x{:02X})", result);
            return false;
        }

        // Port 1'i etkinleştir
        self.send_command(commands::ENABLE_PORT1);

        // Klavyeye tarama etkinleştirme komutu gönder (Scan Enable = 0xF4)
        crate::serial_println!("PS/2: Tarama etkinleştiriliyor (0xF4)...");
        self.write_data(0xF4);

        // ACK (0xFA) bekle: klavye komutu aldığında bu yanıtı verir
        let mut ack_received = false;
        for _ in 0..10000 {
            if unsafe { self.status.read() } & status::OUTPUT_FULL != 0 {
                let response = unsafe { self.data.read() };
                if response == 0xFA {
                    ack_received = true;
                    break;
                }
            }
            core::hint::spin_loop();
        }

        if ack_received {
            crate::serial_println!("PS/2: Tarama başarıyla başlatıldı!");
        } else {
            crate::serial_println!("PS/2: UYARI - ACK alınamadı!");
        }

        // Konfigürasyon byte'ında IRQ'yu tekrar aç (bit0=1)
        self.send_command(commands::READ_CONFIG);
        config = self.read_data();
        config |= 0x01;
        self.send_command(commands::WRITE_CONFIG);
        self.write_data(config);
        crate::serial_println!("PS/2: IRQ'lar etkinleştirildi!");

        true
    }
}

/// Global PS/2 kontrolcü nesnesi.
/// `UnsafeCell` ile sarılmış iç değişkenlik (interior mutability) sağlanır.
/// `Sync` güvenli olmayan şekilde uygulanır; tek çekirdekli veya kesme korumalı bağlamlarda kullanılır.
struct Ps2Cell(UnsafeCell<Ps2Controller>);

unsafe impl Sync for Ps2Cell {}

impl Ps2Cell {
    const fn new() -> Self {
        Self(UnsafeCell::new(Ps2Controller::new()))
    }

    fn get(&self) -> *mut Ps2Controller {
        self.0.get()
    }
}

static PS2: Ps2Cell = Ps2Cell::new();

/// Global PS/2 kontrolcüsünü başlatır.
/// Sürücü başlatma sırasında çekirdek tarafından çağrılır.
pub fn init() -> bool {
    unsafe { (&mut *PS2.get()).init() }
}

/// Klavyeyi yoklama (polling) yöntemiyle okur.
///
/// IRQ/interrupt tabanlı işlem mümkün değilse bu fonksiyon yedek olarak kullanılır.
/// `pc_keyboard` kütüphanesi scan kodunu tuş olayına (KeyEvent) çevirir.
/// Tanınan tuşlar global klavye kuyruğuna (`push_key`) eklenir.
pub fn poll_keyboard() {
    use pc_keyboard::{layouts, HandleControl, Keyboard, ScancodeSet1};
    use spin::Mutex;

    lazy_static::lazy_static! {
        static ref POLL_KEYBOARD: Mutex<Keyboard<layouts::Us104Key, ScancodeSet1>> = {
            Mutex::new(Keyboard::new(
                ScancodeSet1::new(),
                layouts::Us104Key,
                HandleControl::Ignore,
            ))
        };
    }

    let status = unsafe { PortReadOnly::<u8>::new(STATUS_PORT).read() };

    // Çıkış tamponu doluysa scan kodu oku
    if status & status::OUTPUT_FULL != 0 {
        let scancode = unsafe { Port::<u8>::new(DATA_PORT).read() };

        // Scan kodu -> KeyEvent -> DecodedKey dönüşümü
        if let Ok(Some(key_event)) = POLL_KEYBOARD.lock().add_byte(scancode) {
            if let Some(key) = POLL_KEYBOARD.lock().process_keyevent(key_event) {
                crate::keyboard::push_key(key);
            }
        }
    }
}
