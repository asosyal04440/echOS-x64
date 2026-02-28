//! # echOS PCI Kökü (VirtIO için)
//!
//! Bu modül, `virtio_drivers` kütüphanesinin gerektirdiği `PciRoot` nesnesini
//! Port I/O (PIO) modunda oluşturur ve PCI aygıtlarını etkinleştirmek için
//! yardımcı fonksiyonlar sağlar.
//!
//! ## PCI Konfigürasyonuna Erişim: PIO Modu
//!
//! ```text
//!  x86 PCI Port I/O Adresleme Şeması:
//!
//!  +--------+       Port 0xCF8 (Adres Kaydı)       +-----------+
//!  |        |-------------------------------------->|           |
//!  |  CPU   |                                       | PCI Kök   |
//!  |        |       Port 0xCFC (Veri Kaydı)         | Kontrolcü |
//!  |        |<------------------------------------->|           |
//!  +--------+                                       +-----------+
//!
//!  Adres Kaydı Formatı (32-bit):
//!  +-----+--------+-----------+----------+----------+------+
//!  | 31  | 30-24  |  23-16    |  15-11   |  10-8    | 7-2  |
//!  +-----+--------+-----------+----------+----------+------+
//!  |  1  | Rezerv |   Bus     |  Device  | Function | Ofst |
//!  +-----+--------+-----------+----------+----------+------+
//!   ^ Etkinleştirme biti
//! ```
//!
//! ## Command Register (Ofset 0x04) Bitleri
//!
//! ```text
//!  Bit | Ad                | Acıklama
//!  ----+-------------------+--------------------------------------------
//!   0  | I/O Space         | Cihazın I/O portlarına erişimini etkinleştirir
//!   1  | Memory Space      | Cihazın MMIO bölgesine erişimini etkinleştirir
//!   2  | Bus Master        | Cihazın DMA (bellek ana işlemcisi) yapmasını sağlar
//!  10  | Interrupt Disable | Legacy INTx kesmelerini devre dışı bırakır
//! ```
//!
//! ## BAR Boyutu Tespit Algoritması
//!
//! ```text
//!  1. Özgün BAR değerini oku ve sakla     --> original
//!  2. BAR yazmacına 0xFFFFFFFF yaz
//!  3. Geri oku                             --> size_response
//!  4. Özgün değeri yeniden yaz (geri yükle)
//!  5. Taban adresi: original & 0xFFFFFFF0 (MMIO) veya original & 0xFFFFFFFC (I/O)
//!  6. Boyut:        !(size_response & maske) + 1
//! ```

use virtio_drivers::transport::pci::bus::{Cam, PciRoot};

/// PIO modunda PciRoot nesnesi oluşturur.
///
/// Bu nesne, VirtIO sürücüsünün PCI konfigürasyon alanını Port I/O üzerinden
/// okumasını ve yazmasını sağlar. MMIO taban adresi yerine null pointer
/// kullanılır; gerçek erişim 0xCF8/0xCFC portları üzerinden gerçekleşir.
pub fn create_pci_root() -> PciRoot {
    // GÜVENLİ: PIO modunda MMIO tabanı kullanılmaz, x86 port I/O devreye girer
    // PciRoot ile Cam::Pio seçildiğinde config erişimi port I/O üzerinden yapılır
    unsafe { PciRoot::new(core::ptr::null_mut(), Cam::Pio) }
}

/// Belirtilen PCI aygıtı için Bus Master ve Bellek Alanı erişimini etkinleştirir.
///
/// VirtIO aygıtları çalışabilmek için Command Register'ın şu bitlerini açık olmasını gerektirir:
///   - Bit 0 (I/O Space)     : Port I/O erişimi
///   - Bit 1 (Memory Space)  : MMIO erişimi
///   - Bit 2 (Bus Master)    : DMA (Direct Memory Access) yetkisi
pub fn enable_device(bus: u8, device: u8, function: u8) {
    // Mevcut Command Register değerini oku
    let command = super::pci::read_config_dword(bus, device, function, 0x04);

    // Bus Master, Memory Space ve I/O Space bitlerini etkinleştir
    let new_command = command | (1 << 0) | (1 << 1) | (1 << 2);

    super::pci::write_config_dword(bus, device, function, 0x04, new_command);
}

/// Verilen BAR (Taban Adres Yazmacı) için taban adresi ve boyutu döner.
///
/// Dönüş değeri: `(taban: u64, boyut: u64)`
///
/// I/O, 32-bit MMIO ve 64-bit MMIO olmak üç BAR biçimini destekler.
/// 64-bit BAR'larda bir sonraki BAR yazmacı üst 32-bit için kullanılır.
pub fn get_bar(bus: u8, device: u8, function: u8, bar_index: u8) -> (u64, u64) {
    let bar_offset = 0x10 + (bar_index as u16 * 4);

    // Özgün BAR değerini oku
    let original = super::pci::read_config_dword(bus, device, function, bar_offset);

    // Boyutu öğrenmek için tüm bitleri 1 yap
    super::pci::write_config_dword(bus, device, function, bar_offset, 0xFFFFFFFF);
    let size_response = super::pci::read_config_dword(bus, device, function, bar_offset);

    // Özgün değeri geri yükle
    super::pci::write_config_dword(bus, device, function, bar_offset, original);

    // Taban ve boyutları hesapla: bit0=1 ise I/O, bit2-1=10 ise 64-bit MMIO
    let is_io = (original & 1) != 0;
    let is_64bit = !is_io && ((original >> 1) & 3) == 2;

    let base = if is_io {
        // I/O BAR: alt 2 bit durum bitleri, geri kalanı taban
        (original as u64) & 0xFFFFFFFC
    } else if is_64bit {
        // 64-bit MMIO BAR: bir sonraki BAR üst 32-biti içerir
        let upper = if bar_index < 5 {
            super::pci::read_config_dword(bus, device, function, bar_offset + 4)
        } else {
            0
        };
        ((original as u64) & 0xFFFFFFF0) | ((upper as u64) << 32)
    } else {
        // 32-bit MMIO BAR: alt 4 bit durum bitleri
        (original as u64) & 0xFFFFFFF0
    };

    let size = if is_io {
        // I/O boyutu: maskeyi tersle ve +1 (2'nin tümleyeni)
        let mask = !(size_response as u64 & 0xFFFFFFFC);
        mask + 1
    } else if is_64bit {
        // 64-bit MMIO boyutu: her iki BAR'ı birleştirerek hesapla
        let upper_size = if bar_index < 5 {
            let next_offset = bar_offset + 4;
            super::pci::write_config_dword(bus, device, function, next_offset, 0xFFFFFFFF);
            let upper_resp = super::pci::read_config_dword(bus, device, function, next_offset);
            super::pci::write_config_dword(bus, device, function, next_offset,
                super::pci::read_config_dword(bus, device, function, bar_offset + 4));
            upper_resp
        } else {
            0
        };
        let mask = !(((size_response as u64) & 0xFFFFFFF0) | ((upper_size as u64) << 32));
        mask + 1
    } else {
        // 32-bit MMIO boyutu
        let mask = !((size_response as u64) & 0xFFFFFFF0);
        mask + 1
    };

    (base, size)
}
