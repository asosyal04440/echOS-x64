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

use crate::drivers::gesture::GestureRecognizer;
use crate::drivers::input::{push_event, InputEvent, MousePacket};
use core::sync::atomic::{AtomicI32, AtomicU64, AtomicU8, Ordering};
use spin::Mutex;
use x86_64::instructions::port::Port;

// ============================================================================
// PS/2 PORT SABİTLERİ (PS/2 PORT CONSTANTS)
// ============================================================================

// i8042 PS/2 denetleyicisi iki I/O portuna sahiptir:
//   0x60: Veri portu (okuma: keyboard/mouse verisi; yazma: komut argümanı)
//   0x64: Durum/Komut portu (okuma: durum; yazma: komut)

const DATA_PORT: u16 = 0x60; // PS/2 veri portu (input/output)
const STATUS_PORT: u16 = 0x64; // PS/2 durum portu (okuma için)
const COMMAND_PORT: u16 = 0x64; // PS/2 komut portu (yazma için; status ile aynı adres)

// ============================================================================
// GLOBAL MOUSE DURUMU (GLOBAL MOUSE STATE)
// ============================================================================

// Atomic state: thread-safe mouse position and button state
// IRQ handler updates these, compositor reads them without unsafe blocks

#[repr(C, align(64))]
struct MousePublicationState {
    sequence: AtomicU64,
    x: AtomicI32,
    y: AtomicI32,
    buttons: AtomicU8,
}

impl MousePublicationState {
    const fn new(x: i32, y: i32, buttons: u8) -> Self {
        Self {
            sequence: AtomicU64::new(0),
            x: AtomicI32::new(x),
            y: AtomicI32::new(y),
            buttons: AtomicU8::new(buttons),
        }
    }

    fn begin_write(&self) -> u64 {
        loop {
            let sequence = self.sequence.load(Ordering::Relaxed);
            if (sequence & 1) != 0 {
                core::hint::spin_loop();
                continue;
            }
            if self
                .sequence
                .compare_exchange_weak(sequence, sequence + 1, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
            {
                return sequence + 2;
            }
            core::hint::spin_loop();
        }
    }

    fn finish_write(&self, published_sequence: u64) {
        self.sequence.store(published_sequence, Ordering::Release);
    }
}

static MOUSE_PUBLICATION: MousePublicationState = MousePublicationState::new(640, 400, 0);

// Paket buffer (3 byte'lık döngü): IRQ başına 1 byte gelir, 3 byte = 1 tam paket
static MOUSE_CYCLE: Mutex<u8> = Mutex::new(0); // Hangi byte bekleniyor: 0, 1, 2
static MOUSE_PACKET: Mutex<[u8; 3]> = Mutex::new([0; 3]); // Toplanan paket byte'ları
static GESTURE_RECOGNIZER: Mutex<GestureRecognizer> = Mutex::new(GestureRecognizer::new());

// Ekran sınırları (Mouse clamp için) — runtime'da güncellenir.
// Ekranın dışına çıkmayı önlemek için mouse koordinatları bu sınırlara kırpılır.
pub static mut SCREEN_WIDTH: i32 = 1280;
pub static mut SCREEN_HEIGHT: i32 = 800;

// ============================================================================
// MOUSE BUTON YAPISI (MOUSE BUTTONS)
// ============================================================================

/// Mouse buton durumları: sol, sağ, orta tuşların basılı olup olmadığı
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MouseButtons {
    pub left: bool,   // Sol tuş (bit 0 flags byte)
    pub right: bool,  // Sağ tuş (bit 1 flags byte)
    pub middle: bool, // Orta tuş / scroll wheel tıklaması (bit 2 flags byte)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MouseSnapshot {
    pub x: i32,
    pub y: i32,
    pub buttons: MouseButtons,
}

const fn encode_mouse_buttons(buttons: MouseButtons) -> u8 {
    (buttons.left as u8) | ((buttons.right as u8) << 1) | ((buttons.middle as u8) << 2)
}

const fn decode_mouse_buttons(bits: u8) -> MouseButtons {
    MouseButtons {
        left: (bits & 0x01) != 0,
        right: (bits & 0x02) != 0,
        middle: (bits & 0x04) != 0,
    }
}

fn publish_snapshot(x: i32, y: i32, buttons: MouseButtons) {
    let published_sequence = MOUSE_PUBLICATION.begin_write();
    MOUSE_PUBLICATION.x.store(x, Ordering::Relaxed);
    MOUSE_PUBLICATION.y.store(y, Ordering::Relaxed);
    MOUSE_PUBLICATION
        .buttons
        .store(encode_mouse_buttons(buttons), Ordering::Relaxed);
    MOUSE_PUBLICATION.finish_write(published_sequence);
}

pub fn publish_state(x: i32, y: i32, buttons: MouseButtons) {
    publish_snapshot(x, y, buttons);
}

pub fn snapshot() -> MouseSnapshot {
    loop {
        let sequence = MOUSE_PUBLICATION.sequence.load(Ordering::Acquire);
        if (sequence & 1) != 0 {
            core::hint::spin_loop();
            continue;
        }
        let x = MOUSE_PUBLICATION.x.load(Ordering::Relaxed);
        let y = MOUSE_PUBLICATION.y.load(Ordering::Relaxed);
        let buttons = decode_mouse_buttons(MOUSE_PUBLICATION.buttons.load(Ordering::Relaxed));
        let end_sequence = MOUSE_PUBLICATION.sequence.load(Ordering::Acquire);
        if sequence == end_sequence {
            return MouseSnapshot { x, y, buttons };
        }
        core::hint::spin_loop();
    }
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
    status_byte |= 0x02; // IRQ12 etkinleştir
    status_byte &= 0xDF; // Mouse clock etkinleştir (bit 5 = 0)

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

            // Pointer Acceleration Curve (Phase 8.3)
            // cursor_speed = raw_delta * (1.0 + raw_delta.abs() * ACCEL_FACTOR)
            let raw_dx = dx as f32;
            let raw_dy = dy as f32;
            const ACCEL_FACTOR: f32 = 0.08; // İvme faktörü
            const BASE_SENSITIVITY: f32 = 1.5; // Temel hassasiyet

            // İvme uygula
            let accel_dx = raw_dx * (1.0 + raw_dx.abs() * ACCEL_FACTOR);
            let accel_dy = raw_dy * (1.0 + raw_dy.abs() * ACCEL_FACTOR);

            // Hassasiyet uygula ve integer'a çevir
            dx = (accel_dx * BASE_SENSITIVITY) as i32;
            dy = (accel_dy * BASE_SENSITIVITY) as i32;

            // Gesture Engine Feed (Phase 8.2)
            x86_64::instructions::interrupts::without_interrupts(|| {
                if let Some(gesture) = GESTURE_RECOGNIZER.lock().feed(dx, dy) {
                    push_event(InputEvent::Gesture(gesture));
                }
            });

            let buttons = flags & 0x07;

            // Global pozisyonu güncelle (atomik işlemler, thread-safe)
            let screen_w = unsafe { SCREEN_WIDTH };
            let screen_h = unsafe { SCREEN_HEIGHT };
            let previous = snapshot();
            let new_x = (previous.x + dx).clamp(0, screen_w - 1);
            let new_y = (previous.y - dy).clamp(0, screen_h - 1);
            publish_snapshot(
                new_x,
                new_y,
                MouseButtons {
                    left: (flags & 0x01) != 0,
                    right: (flags & 0x02) != 0,
                    middle: (flags & 0x04) != 0,
                },
            );
            // Y ekseni: PS/2 protokolünde Y yukari=pozitif, ekranda yukari=negatif

            // Buton durumlarını güncelle (flags byte'ının alt 3 biti)

            // EchInput pointer route'u yalnızca Mouse paket event'leri üzerinden çalışır.
            // Cursor state'i güncelledikten sonra tam paketi kuyruğa bas.
            push_event(InputEvent::Mouse(MousePacket::Standard {
                buttons,
                x: dx,
                y: dy,
            }));
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
        // Mevcut koordinatları yeni sınırlara kırp (atomik işlemler)
        let current = snapshot();
        let x = current.x.clamp(0, SCREEN_WIDTH - 1);
        let y = current.y.clamp(0, SCREEN_HEIGHT - 1);
        publish_snapshot(x, y, current.buttons);
    }
}

/// Canli mouse clamp sinirlarini dondurur.
/// Output geometry drift tespiti yapan ust servisler bu gercegi kullanir.
pub fn get_bounds() -> (i32, i32) {
    unsafe { (SCREEN_WIDTH, SCREEN_HEIGHT) }
}

// =============================================================================
// PUBLIC API
// =============================================================================

/// Mouse pozisyonunu döndürür.
/// Döner: (x, y) piksel koordinatları; (0,0) sol-üst köşe
pub fn get_position() -> (i32, i32) {
    let snapshot = snapshot();
    (snapshot.x, snapshot.y)
}

/// Mouse buton durumlarını döndürür.
/// IRQ12 tabanlı güncellemeden sonra çağrılmalıdır.
pub fn get_buttons() -> MouseButtons {
    snapshot().buttons
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

/// QEMU/virt ortamlarında AUX (bit5) güvenilir olduğunda yalnız fare verisini tüketir.
/// Klavye scancode'larını çalmamak için output-buffer dolu olsa bile AUX biti yoksa okumaz.
pub fn poll_aux_only() -> bool {
    let mut status_port = Port::<u8>::new(STATUS_PORT);
    let mut data_port = Port::<u8>::new(DATA_PORT);

    let status = unsafe { status_port.read() };
    if (status & 0x01) == 0 || (status & 0x20) == 0 {
        return false;
    }

    let packet_byte = unsafe { data_port.read() };
    handle_packet(packet_byte);
    true
}

/// AUX-bounded polling burst. QEMU/UEFI GUI path'inde IRQ12 kaçsa bile
/// fare hareketini toparlamak için kullanılır.
pub fn poll_aux_burst(max_bytes: usize) -> usize {
    let mut count = 0usize;
    while count < max_bytes {
        if !poll_aux_only() {
            break;
        }
        count += 1;
    }
    count
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_bounds_updates_pointer_clamp_for_runtime_framebuffer_size() {
        publish_state(1817, 877, MouseButtons::default());

        set_bounds(1920, 1080);
        let (x, y) = get_position();
        let (screen_width, screen_height) = unsafe { (SCREEN_WIDTH, SCREEN_HEIGHT) };

        assert_eq!((x, y), (1817, 877));
        assert_eq!(screen_width, 1920);
        assert_eq!(screen_height, 1080);
    }

    #[test]
    fn set_bounds_clamps_legacy_pointer_inside_new_screen_rect() {
        publish_state(4096, 4096, MouseButtons::default());

        set_bounds(1920, 1080);
        let (x, y) = get_position();

        assert_eq!((x, y), (1919, 1079));
    }

    #[test]
    fn get_bounds_reports_latest_runtime_screen_limits() {
        set_bounds(1600, 900);

        assert_eq!(get_bounds(), (1600, 900));
    }

    #[test]
    fn publish_state_roundtrips_coherent_snapshot() {
        publish_state(
            321,
            654,
            MouseButtons {
                left: true,
                right: false,
                middle: true,
            },
        );

        let snapshot = snapshot();
        assert_eq!(snapshot.x, 321);
        assert_eq!(snapshot.y, 654);
        assert_eq!(
            snapshot.buttons,
            MouseButtons {
                left: true,
                right: false,
                middle: true,
            }
        );
    }
}
