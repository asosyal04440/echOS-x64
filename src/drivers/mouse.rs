//! # echOS PS/2 Mouse Sürücüsü
//!
//! OSDev Wiki implementasyonuna dayalı PS/2 Mouse sürücüsü.
//! Mouse başlatma, paket işleme ve pozisyon takibi.
//! Kaynak: https://wiki.osdev.org/Mouse_Input

use spin::Mutex;
use x86_64::instructions::port::Port;

// PS/2 Controller Portları
const DATA_PORT: u16 = 0x60;
const STATUS_PORT: u16 = 0x64;
const COMMAND_PORT: u16 = 0x64;

// Global Mouse Durumu
pub static mut MOUSE_X: i32 = 640;
pub static mut MOUSE_Y: i32 = 400;
pub static mut MOUSE_BUTTONS: MouseButtons = MouseButtons {
    left: false,
    right: false,
    middle: false,
};

// Paket buffer (3 byte'lık döngü)
static MOUSE_CYCLE: Mutex<u8> = Mutex::new(0);
static MOUSE_PACKET: Mutex<[u8; 3]> = Mutex::new([0; 3]);

// Ekran sınırları (Mouse clamp için)
const SCREEN_WIDTH: i32 = 1280;
const SCREEN_HEIGHT: i32 = 800;

/// Mouse buton durumları
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MouseButtons {
    pub left: bool,
    pub right: bool,
    pub middle: bool,
}

// =============================================================================
// YARDIMCI FONKSİYONLAR
// =============================================================================

/// Controller yazmaya hazır olana kadar bekle (Status bit 1).
fn wait_write() {
    let mut status_port = Port::<u8>::new(STATUS_PORT);
    let mut timeout = 100_000;
    while timeout > 0 {
        let status = unsafe { status_port.read() };
        if (status & 0x02) == 0 {
            return;
        }
        timeout -= 1;
        core::hint::spin_loop();
    }
}

/// Controller'dan veri gelene kadar bekle (Status bit 0).
fn wait_read() {
    let mut status_port = Port::<u8>::new(STATUS_PORT);
    let mut timeout = 100_000;
    while timeout > 0 {
        let status = unsafe { status_port.read() };
        if (status & 0x01) != 0 {
            return;
        }
        timeout -= 1;
        core::hint::spin_loop();
    }
}

/// Mouse'a komut gönderir (0xD4 prefix ile).
fn mouse_write(cmd: u8) {
    let mut command_port = Port::<u8>::new(COMMAND_PORT);
    let mut data_port = Port::<u8>::new(DATA_PORT);

    // Controller'a "sıradaki byte mouse'a gidecek" de
    wait_write();
    unsafe {
        command_port.write(0xD4);
    }

    // Komutu gönder
    wait_write();
    unsafe {
        data_port.write(cmd);
    }
}

/// Data portundan byte okur.
fn mouse_read() -> u8 {
    let mut data_port = Port::<u8>::new(DATA_PORT);
    wait_read();
    unsafe { data_port.read() }
}

// =============================================================================
// BAŞLATMA (INIT)
// =============================================================================

/// PS/2 Mouse'u başlatır.
pub fn init() -> bool {
    crate::serial_println!("Mouse: Başlatılıyor...");

    let mut command_port = Port::<u8>::new(COMMAND_PORT);
    let mut data_port = Port::<u8>::new(DATA_PORT);

    // Adım 1: Auxiliary Device (Mouse) Etkinleştir
    wait_write();
    unsafe {
        command_port.write(0xA8);
    }

    // Adım 2: Compaq Status Byte Oku
    wait_write();
    unsafe {
        command_port.write(0x20);
    }
    wait_read();
    let mut status_byte = unsafe { data_port.read() };

    // Adım 3: Status Byte Güncelle
    // - Bit 1 AÇ (Enable IRQ12)
    // - Bit 5 KAPAT (Enable Mouse Clock)
    status_byte |= 0x02;
    status_byte &= 0xDF;

    // Adım 4: Yeni Status Byte'ı Yaz
    wait_write();
    unsafe {
        command_port.write(0x60);
    }
    wait_write();
    unsafe {
        data_port.write(status_byte);
    }

    // Adım 5: Varsayılan Ayarları Yükle (0xF6)
    mouse_write(0xF6);
    let _ack = mouse_read();

    // Adım 6: Paket Akışını Başlat (0xF4)
    mouse_write(0xF4);
    let ack = mouse_read();

    if ack == 0xFA {
        crate::serial_println!("Mouse: Başarıyla başlatıldı!");
        true
    } else {
        crate::serial_println!("Mouse: Başlatılamadı (ACK=0x{:02X})", ack);
        false
    }
}

// =============================================================================
// INTERRUPT HANDLER & PAKET İŞLEME
// =============================================================================

/// IRQ12 handler tarafından çağrılır.
/// Mouse'dan gelen raw byte'ı işler.
pub fn handle_packet(packet_byte: u8) {
    let mut cycle = MOUSE_CYCLE.lock();
    let mut packet = MOUSE_PACKET.lock();

    match *cycle {
        0 => {
            // Byte 0: Flagler
            // Bit 3 her zaman 1 olmalı (alignment check)
            if (packet_byte & 0x08) == 0 {
                return; // Senkronizasyon bozuk, yoksay
            }
            packet[0] = packet_byte;
            *cycle = 1;
        }
        1 => {
            // Byte 1: X Hareketi
            packet[1] = packet_byte;
            *cycle = 2;
        }
        2 => {
            // Byte 2: Y Hareketi - Paket Tamamlandı!
            packet[2] = packet_byte;
            *cycle = 0;

            // Paketi işle
            let flags = packet[0];
            let mut dx = packet[1] as i32;
            let mut dy = packet[2] as i32;

            // Sign extension (negatif değerler için)
            if (flags & 0x10) != 0 {
                dx -= 256;
            } // X sign bit
            if (flags & 0x20) != 0 {
                dy -= 256;
            } // Y sign bit

            // Global pozisyonu güncelle
            unsafe {
                MOUSE_X = (MOUSE_X + dx).clamp(0, SCREEN_WIDTH - 1);
                MOUSE_Y = (MOUSE_Y - dy).clamp(0, SCREEN_HEIGHT - 1); // Y ekseni ters olabilir

                MOUSE_BUTTONS.left = (flags & 0x01) != 0;
                MOUSE_BUTTONS.right = (flags & 0x02) != 0;
                MOUSE_BUTTONS.middle = (flags & 0x04) != 0;
            }
        }
        _ => {
            *cycle = 0;
        }
    }
}

// =============================================================================
// PUBLIC API
// =============================================================================

/// Mouse pozisyonunu döndürür.
pub fn get_position() -> (i32, i32) {
    unsafe { (MOUSE_X, MOUSE_Y) }
}

/// Mouse buton durumlarını döndürür.
pub fn get_buttons() -> MouseButtons {
    unsafe { MOUSE_BUTTONS }
}

/// Polling ile mouse verisi okur (Interruptsız mod için).
pub fn poll() -> bool {
    let mut status_port = Port::<u8>::new(STATUS_PORT);
    let mut data_port = Port::<u8>::new(DATA_PORT);

    let status = unsafe { status_port.read() };
    if (status & 0x01) != 0 {
        let packet_byte = unsafe { data_port.read() };
        handle_packet(packet_byte);
        return true;
    }
    false
}

/// ExitBootServices sonrası yeniden başlatma.
pub fn reinit_streaming() {
    crate::serial_println!("Mouse: Re-init (post-ExitBS)...");

    let mut command_port = Port::<u8>::new(COMMAND_PORT);
    let mut data_port = Port::<u8>::new(DATA_PORT);

    // Aux tekrar etkinleştir
    wait_write();
    unsafe {
        command_port.write(0xA8);
    }

    // Status byte düzelt
    wait_write();
    unsafe {
        command_port.write(0x20);
    }
    wait_read();
    let mut status_byte = unsafe { data_port.read() };

    status_byte |= 0x02; // IRQ12 enable
    status_byte &= 0xDF; // Clock enable

    wait_write();
    unsafe {
        command_port.write(0x60);
    }
    wait_write();
    unsafe {
        data_port.write(status_byte);
    }

    // Streaming tekrar aç
    wait_write();
    unsafe {
        command_port.write(0xD4);
    }
    wait_write();
    unsafe {
        data_port.write(0xF4);
    }

    wait_read();
    let _ack = unsafe { data_port.read() };
}
