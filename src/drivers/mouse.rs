//! # echOS PS/2 Mouse Sürücüsü
//!
//! OSDev Wiki implementasyonuna dayalı PS/2 Mouse sürücüsü.
//! Mouse başlatma, paket işleme ve pozisyon takibi.
//! Kaynak: https://wiki.osdev.org/Mouse_Input
//!
//! ## PS/2 Mouse Protokolü
//!
//! PS/2 mouse, her harekette 3 baytlık paket gönderir:
//!
//! ```
//! Byte 0 (Flags):
//!   Bit 7: Y overflow  (taşma)
//!   Bit 6: X overflow  (taşma)
//!   Bit 5: Y sign bit  (negatif hareket işareti)
//!   Bit 4: X sign bit  (negatif hareket işareti)
//!   Bit 3: Daima 1     (senkronizasyon işareti)
//!   Bit 2: Middle btn  (orta tuş)
//!   Bit 1: Right btn   (sağ tuş)
//!   Bit 0: Left btn    (sol tuş)
//!
//! Byte 1: X delta (yatay hareket, -256..+255)
//! Byte 2: Y delta (dikey hareket, -256..+255, ters çevrilmiş!)
//! ```
//!
//! ## PS/2 Denetleyici Haberleşmesi
//!
//! Mouse komutları i8042 denetleyicisi üzerinden geçer:
//!
//! ```
//!   CPU ---[0xD4]--> [0x64 command port] --> i8042
//!                                               |
//!                    [0x60 data port] <--> mouse komut + yanıt
//! ```
//!
//! ## Başlatma Akışı
//!
//! ```
//! 1. 0xA8 -> 0x64  (Auxiliary cihazı etkinleştir)
//! 2. 0x20 -> 0x64  (Status byte oku)
//! 3. Status byte güncelle: bit1=1 (IRQ12 etkin), bit5=0 (clock etkin)
//! 4. 0x60 -> 0x64, yeni_status -> 0x60  (Status byte yaz)
//! 5. 0xF6 -> mouse  (Varsayılan ayarlar)
//! 6. 0xF4 -> mouse  (Paket akışını başlat)
//! ```

use spin::Mutex;
use x86_64::instructions::port::Port;

// ============================================================================
// PS/2 PORT SABİTLERİ (PS/2 PORT CONSTANTS)
// ============================================================================

// i8042 PS/2 denetleyicisi iki I/O portuna sahiptir:
//   0x60: Veri portu (okuma: keyboard/mouse verisi; yazma: komut argümanı)
//   0x64: Durum/Komut portu (okuma: durum; yazma: komut)

const DATA_PORT: u16 = 0x60;    // PS/2 veri portu (input/output)
const STATUS_PORT: u16 = 0x64;  // PS/2 durum portu (okuma için)
const COMMAND_PORT: u16 = 0x64; // PS/2 komut portu (yazma için; status ile aynı adres)

// ============================================================================
// GLOBAL MOUSE DURUMU (GLOBAL MOUSE STATE)
// ============================================================================

// unsafe static kullanımı: tek çekirdekli giriş yolunda basitlik sağlar.
// Çok çekirdekli ortamda Mutex<MouseState> tercih edilir.

/// Mevcut mouse X koordinatı (piksel; 0=sol kenar)
pub static mut MOUSE_X: i32 = 640;
/// Mevcut mouse Y koordinatı (piksel; 0=üst kenar)
pub static mut MOUSE_Y: i32 = 400;
/// Mevcut mouse buton durumları
pub static mut MOUSE_BUTTONS: MouseButtons = MouseButtons {
    left: false,
    right: false,
    middle: false,
};

// Paket buffer (3 byte'lık döngü): IRQ başına 1 byte gelir, 3 byte = 1 tam paket
static MOUSE_CYCLE: Mutex<u8> = Mutex::new(0);     // Hangi byte bekleniyor: 0, 1, 2
static MOUSE_PACKET: Mutex<[u8; 3]> = Mutex::new([0; 3]); // Toplanan paket byte'ları

// Ekran sınırları (Mouse clamp için) — runtime'da güncellenir.
// Ekranın dışına çıkmayı önlemek için mouse koordinatları bu sınırlara kırpılır.
pub static mut SCREEN_WIDTH: i32 = 1280;
pub static mut SCREEN_HEIGHT: i32 = 800;

// ============================================================================
// MOUSE BUTON YAPISI (MOUSE BUTTONS)
// ============================================================================

/// Mouse buton durumları: sol, sağ, orta tuşların basılı olup olmadığı
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MouseButtons {
    pub left: bool,    // Sol tuş (bit 0 flags byte)
    pub right: bool,   // Sağ tuş (bit 1 flags byte)
    pub middle: bool,  // Orta tuş / scroll wheel tıklaması (bit 2 flags byte)
}

// =============================================================================
// YARDIMCI FONKSİYONLAR
// =============================================================================

/// Controller yazmaya hazır olana kadar bekle (Status bit 1).
/// Status Register bit 1 = Input buffer full; 0 olunca yazabiliriz.
fn wait_write() {
    let mut status_port = Port::<u8>::new(STATUS_PORT);
    let mut timeout = 100_000;
    while timeout > 0 {
        let status = unsafe { status_port.read() };
        if (status & 0x02) == 0 {
            return; // Input buffer boş -> yazabilir
        }
        timeout -= 1;
        core::hint::spin_loop(); // CPU'ya döngü içinde olduğunu bildir (enerji tasarrufu)
    }
}

/// Controller'dan veri gelene kadar bekle (Status bit 0).
/// Status Register bit 0 = Output buffer full; 1 olunca okuyabiliriz.
fn wait_read() {
    let mut status_port = Port::<u8>::new(STATUS_PORT);
    let mut timeout = 100_000;
    while timeout > 0 {
        let status = unsafe { status_port.read() };
        if (status & 0x01) != 0 {
            return; // Output buffer dolu -> okunabilir
        }
        timeout -= 1;
        core::hint::spin_loop();
    }
}

/// Mouse'a komut gönderir (0xD4 prefix ile).
/// 0xD4 komutu: "sıradaki data byte'ını PS/2 port 2'ye (mouse'a) ilet"
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
///
/// Başarılı ise true, başlatılamadıysa false döner.
/// init() sonrası IRQ12 aktif olur ve handle_packet() çağrılmaya başlar.
pub fn init() -> bool {
    crate::serial_println!("Mouse: Başlatılıyor...");

    let mut command_port = Port::<u8>::new(COMMAND_PORT);
    let mut data_port = Port::<u8>::new(DATA_PORT);

    // Adım 1: Auxiliary Device (Mouse) Etkinleştir
    // 0xA8 komutu: i8042'nin ikinci portunun (PS/2 mouse) clock'unu açar
    wait_write();
    unsafe {
        command_port.write(0xA8);
    }

    // Adım 2: Compaq Status Byte Oku
    // 0x20 komutu: Controller Command Byte'ı oku
    wait_write();
    unsafe {
        command_port.write(0x20);
    }
    wait_read();
    let mut status_byte = unsafe { data_port.read() };

    // Adım 3: Status Byte Güncelle
    // - Bit 1 AÇ (Enable IRQ12): mouse verisi gelince CPU'ya interrupt gönder
    // - Bit 5 KAPAT (Enable Mouse Clock): mouse'un saat sinyalini etkinleştir
    status_byte |= 0x02;  // IRQ12 etkinleştir
    status_byte &= 0xDF;  // Mouse clock etkinleştir (bit 5 = 0)

    // Adım 4: Yeni Status Byte'ı Yaz
    // 0x60 komutu: Controller Command Byte'ını yaz
    wait_write();
    unsafe {
        command_port.write(0x60);
    }
    wait_write();
    unsafe {
        data_port.write(status_byte);
    }

    // Adım 5: Varsayılan Ayarları Yükle (0xF6)
    // Mouse'u fabrika ayarlarına döndürür: 100 dpi, 1:1 ölçek, stream modu
    mouse_write(0xF6);
    let _ack = mouse_read(); // ACK bekliyoruz (0xFA)

    // Adım 6: Paket Akışını Başlat (0xF4)
    // Stream mode'da mouse her hareketinde otomatik paket gönderir
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
///
/// ## 3-Byte Paket Toplama Döngüsü
///
/// PS/2 mouse her paket için 3 ayrı IRQ12 üretir:
///   IRQ12 #1 -> cycle=0: flags byte (buton + overflow + sign bitleri)
///   IRQ12 #2 -> cycle=1: X delta byte
///   IRQ12 #3 -> cycle=2: Y delta byte -> paket tamamlandı, pozisyon güncelle
pub fn handle_packet(packet_byte: u8) {
    let mut cycle = MOUSE_CYCLE.lock();
    let mut packet = MOUSE_PACKET.lock();

    match *cycle {
        0 => {
            // Byte 0: Flagler
            // Bit 3 her zaman 1 olmalı (alignment check / senkronizasyon)
            // Bu bit 0 ise protokol kayması var demektir; paketi yoksay
            if (packet_byte & 0x08) == 0 {
                return; // Senkronizasyon bozuk, yoksay
            }
            packet[0] = packet_byte;
            *cycle = 1;
        }
        1 => {
            // Byte 1: X Hareketi (ham, işaretli 8-bit; sign extension gerekir)
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

            // Sign extension (negatif değerler için): 8-bit signed -> 32-bit signed
            // Flags byte'ındaki sign bitleri ile sign extension uygulanır
            if (flags & 0x10) != 0 {
                dx -= 256; // X negatif: 0..255 aralığındaki değeri -256..-1'e çevir
            }
            if (flags & 0x20) != 0 {
                dy -= 256; // Y negatif
            }

            // Global pozisyonu güncelle
            unsafe {
                MOUSE_X = (MOUSE_X + dx).clamp(0, SCREEN_WIDTH - 1);
                // Y ekseni: PS/2 protokolünde Y yukari=pozitif, ekranda yukari=negatif
                MOUSE_Y = (MOUSE_Y - dy).clamp(0, SCREEN_HEIGHT - 1);

                // Buton durumlarını güncelle (flags byte'ının alt 3 biti)
                MOUSE_BUTTONS.left = (flags & 0x01) != 0;   // Bit 0: sol tuş
                MOUSE_BUTTONS.right = (flags & 0x02) != 0;  // Bit 1: sağ tuş
                MOUSE_BUTTONS.middle = (flags & 0x04) != 0; // Bit 2: orta tuş
            }
        }
        _ => {
            // Geçersiz durum: sıfırla
            *cycle = 0;
        }
    }
}

/// Mouse clamp sınırlarını runtime ekran boyutuna göre ayarlar.
/// Ekran boyutu değiştiğinde (örn. VT100 pencere boyutu) çağrılır.
pub fn set_bounds(width: i32, height: i32) {
    unsafe {
        SCREEN_WIDTH = width.max(1);
        SCREEN_HEIGHT = height.max(1);
        // Mevcut koordinatları yeni sınırlara kırp
        MOUSE_X = MOUSE_X.clamp(0, SCREEN_WIDTH - 1);
        MOUSE_Y = MOUSE_Y.clamp(0, SCREEN_HEIGHT - 1);
    }
}

// =============================================================================
// PUBLIC API
// =============================================================================

/// Mouse pozisyonunu döndürür.
/// Döner: (x, y) piksel koordinatları; (0,0) sol-üst köşe
pub fn get_position() -> (i32, i32) {
    unsafe { (MOUSE_X, MOUSE_Y) }
}

/// Mouse buton durumlarını döndürür.
/// IRQ12 tabanlı güncellemeden sonra çağrılmalıdır.
pub fn get_buttons() -> MouseButtons {
    unsafe { MOUSE_BUTTONS }
}

/// Polling ile mouse verisi okur (Interrupt'sız mod için).
///
/// Output buffer doluysa byte'ı tüketir ve handle_packet'e iletir.
/// Döner: yeni byte işlendiyse true, buffer boşsa false.
pub fn poll() -> bool {
    let mut status_port = Port::<u8>::new(STATUS_PORT);
    let mut data_port = Port::<u8>::new(DATA_PORT);

    let status = unsafe { status_port.read() };
    // Bazı emülasyonlarda AUX bit'i (bit5) güvenilir raporlanmıyor.
    // Hareketin kaçmaması için output-buffer doluysa byte'ı tüketiyoruz.
    if (status & 0x01) != 0 {
        let packet_byte = unsafe { data_port.read() };
        handle_packet(packet_byte);
        return true;
    }
    false
}

/// Bir kare içinde sınırlı sayıda AUX byte'ı tüketir.
/// Yoğun mouse hareketi sırasında tampon dolmasını önler.
/// Döner: işlenen toplam byte sayısı.
pub fn poll_burst(max_bytes: usize) -> usize {
    let mut count = 0usize;
    while count < max_bytes {
        if !poll() {
            break;
        }
        count += 1;
    }
    count
}

/// ExitBootServices sonrası yeniden başlatma.
///
/// UEFI ExitBootServices() çağrısı PS/2 denetleyicisini sıfırlayabilir.
/// Bu fonksiyon, UEFI'den çekirdek moduna geçişten sonra mouse'u yeniden başlatır.
pub fn reinit_streaming() {
    crate::serial_println!("Mouse: Re-init (post-ExitBS)...");

    let mut command_port = Port::<u8>::new(COMMAND_PORT);
    let mut data_port = Port::<u8>::new(DATA_PORT);

    // Aux tekrar etkinleştir (UEFI tarafından devre dışı bırakılmış olabilir)
    wait_write();
    unsafe {
        command_port.write(0xA8);
    }

    // Status byte düzelt: IRQ12 ve clock'u yeniden aç
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

    // Streaming tekrar aç: 0xD4 prefix + 0xF4 (Enable Data Reporting)
    wait_write();
    unsafe {
        command_port.write(0xD4);
    }
    wait_write();
    unsafe {
        data_port.write(0xF4);
    }

    // ACK byte'ını oku ve yoksay
    wait_read();
    let _ack = unsafe { data_port.read() };
}
