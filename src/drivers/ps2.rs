//! # echOS PS/2 Klavye Kontrolcüsü (8042)
//!
//! PS/2 Keyboard Controller için sürücü implementasyonu.
//! ExitBootServices sonrası klavyeyi başlatır ve yapılandırır.

use core::cell::UnsafeCell;
use x86_64::instructions::port::{Port, PortReadOnly, PortWriteOnly};

const DATA_PORT: u16 = 0x60;
const STATUS_PORT: u16 = 0x64;
const COMMAND_PORT: u16 = 0x64;

/// 8042 PS/2 Controller komutları
mod commands {
    pub const READ_CONFIG: u8 = 0x20;
    pub const WRITE_CONFIG: u8 = 0x60;
    pub const DISABLE_PORT1: u8 = 0xAD;
    pub const ENABLE_PORT1: u8 = 0xAE;
    pub const DISABLE_PORT2: u8 = 0xA7;
    #[allow(dead_code)]
    pub const ENABLE_PORT2: u8 = 0xA8;
    pub const SELF_TEST: u8 = 0xAA;
    pub const TEST_PORT1: u8 = 0xAB;
}

/// Status register bitleri
mod status {
    pub const OUTPUT_FULL: u8 = 0x01;
    pub const INPUT_FULL: u8 = 0x02;
}

/// PS/2 Controller arayüzü
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

    /// Output buffer dolana kadar bekle (veri okunabilir olana kadar)
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

    /// Input buffer boşalana kadar bekle (komut gönderilebilir olana kadar)
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
    fn send_command(&mut self, cmd: u8) {
        self.wait_input();
        unsafe {
            self.command.write(cmd);
        }
    }

    /// Kontrolcüden veri okur.
    fn read_data(&mut self) -> u8 {
        self.wait_output();
        unsafe { self.data.read() }
    }

    /// Kontrolcüye veri yazar.
    fn write_data(&mut self, data: u8) {
        self.wait_input();
        unsafe {
            self.data.write(data);
        }
    }

    /// Buffer'ı temizler (kalan verileri okuyup atar).
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
    /// Self-test yapar, portu test eder ve interruptları açar.
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

        // Başlangıçta interruptları kapat (bit 0 = 0), saat sinyalini aç (bit 4 = 0)
        config &= !0x01; // Port 1 interrupt disable
        config &= !0x10; // Port 1 clock enable

        self.send_command(commands::WRITE_CONFIG);
        self.write_data(config);

        // Controller Self-Test
        self.send_command(commands::SELF_TEST);
        let result = self.read_data();
        if result != 0x55 {
            crate::serial_println!("PS/2: Self-test BAŞARISIZ! (0x{:02X})", result);
            return false;
        }
        crate::serial_println!("PS/2: Self-test başarılı");

        // Config'i geri yükle
        self.send_command(commands::WRITE_CONFIG);
        self.write_data(config);

        // Port 1 Test
        self.send_command(commands::TEST_PORT1);
        let result = self.read_data();
        if result != 0x00 {
            crate::serial_println!("PS/2: Port 1 testi BAŞARISIZ! (0x{:02X})", result);
            return false;
        }

        // Port 1'i etkinleştir
        self.send_command(commands::ENABLE_PORT1);

        // Taramayı başlat (0xF4)
        crate::serial_println!("PS/2: Tarama etkinleştiriliyor (0xF4)...");
        self.write_data(0xF4);

        // ACK (0xFA) bekle
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

        // Interruptları aç! (bit 0 = 1)
        self.send_command(commands::READ_CONFIG);
        config = self.read_data();
        config |= 0x01;
        self.send_command(commands::WRITE_CONFIG);
        self.write_data(config);
        crate::serial_println!("PS/2: Interruptlar etkinleştirildi!");

        true
    }
}

/// Global PS/2 kontrolcü nesnesi
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

/// PS/2 klavyeyi başlatır.
pub fn init() -> bool {
    unsafe { (&mut *PS2.get()).init() }
}

/// Klavyeyi poll eder (Interruptlar çalışmadığında fallback olarak).
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

    // Veri varsa oku
    if status & status::OUTPUT_FULL != 0 {
        let scancode = unsafe { Port::<u8>::new(DATA_PORT).read() };

        // İşle
        if let Ok(Some(key_event)) = POLL_KEYBOARD.lock().add_byte(scancode) {
            if let Some(key) = POLL_KEYBOARD.lock().process_keyevent(key_event) {
                crate::keyboard::push_key(key);
            }
        }
    }
}
