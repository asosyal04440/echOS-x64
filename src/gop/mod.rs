//! # echOS GOP Framebuffer
//!
//! UEFI Graphics Output Protocol (GOP) framebuffer sarmalayıcısı.
//! Doğrudan ekrana piksel çizim desteği.
//!
//! ## UEFI GOP Nedir?
//!
//! GOP, UEFI firmware'inin işletim sistemine sunduğu standart grafik arayüzüdür.
//! Önceki BIOS dönemindeki VGA/VESA arayüzlerinin yerini almıştır.
//! Framebuffer, fiziksel bellekte doğrudan piksel yazılabilen bir bellek alanıdır.
//!
//! ## Framebuffer Adres Alanı Diyagramı
//!
//! ```text
//!  Fiziksel Bellek (örnek: 4K ekran = 3840×2160)
//!  ┌─────────────────────────────────────────────┐
//!  │  0x0000_0000  Kullanılan bellek (kernel)    │
//!  │  ...                                        │
//!  │  0xFD00_0000  ← base_addr (GOP'tan gelir)  │ ← FRAMEBUFFER BAŞLANGICI
//!  │  ┌─────────────────────────────────────┐   │
//!  │  │ Satır 0: px[0]..px[3839] = 15360 B │   │  ← pixels_per_scan_line × 4 byte
//!  │  │ Satır 1: px[0]..px[3839] = 15360 B │   │
//!  │  │ ...                                 │   │
//!  │  │ Satır 2159: son satır               │   │
//!  │  └─────────────────────────────────────┘   │
//!  │  0xFD00_0000 + (width × height × 4)        │ ← FRAMEBUFFER SONU
//!  └─────────────────────────────────────────────┘
//!
//!  Piksel adresi hesabı:
//!  addr = base_addr + (y × pixels_per_scan_line + x) × 4
//! ```
//!
//! ## Piksel Formatı (BGRx / xRGB)
//!
//! ```text
//!  Her piksel 32-bit (4 byte) little-endian tam sayıdır:
//!
//!  Bit:  31..24   23..16   15..8    7..0
//!        ──────   ──────   ─────    ────
//!        Ayrık    Kırmızı  Yeşil    Mavi
//!        (X/α)      (R)     (G)      (B)
//!
//!  Örnek: Saf kırmızı = 0x00FF0000
//!         Saf yeşil   = 0x0000FF00
//!         Saf mavi    = 0x000000FF
//!         Beyaz       = 0x00FFFFFF
//!
//!  Not: UEFI GOP genellikle PixelBlueGreenRedReserved (BGR) formatını kullanır.
//!  Bu, R ve B kanallarının standart RGB'ye göre yer değiştirdiği anlamına gelir.
//! ```

/// Framebuffer yönetimi ve çizim fonksiyonları
pub mod framebuffer;
