//! # PCI BAR Okuyucu
//!
//! ## PCI Konfigürasyon Alanı Nedir?
//!
//! Her PCI/PCIe cihazı, 256 byte'lık bir "konfigürasyon başlığı" (config header)
//! öne sürer. Bu başlık cihaz hakkında temel bilgileri içerir:
//!   - Vendor ID / Device ID (orn: NVIDIA = 0x10DE)
//!   - Cihaz sınıfı (VGA Controller, USB, NVMe vs.)
//!   - **Base Address Register (BAR)** dizisi — cihazın bellek/IO penceresi
//!
//! ## BAR'lar Ne İşe Yarır?
//!
//! Bir GPU, register'larını ve VRAM penceresini fiziksel bellek adres uzayına
//! "BAR" aracılığıyla map'ler. CPU o adrese yazarsa GPU register yazılır.
//! Passthrough için bu adreslerin *tam olarak* bilinmesi gerekir —
//! işte bu modülün amacı budur.
//!
//! ## CF8/CFC Port I/O Protokolü
//!
//! PCI konfig alanlarına iki özel x86 I/O portu üzerinden erişilir:
//!
//! ```text
//!   Port 0xCF8 (CONFIG_ADDRESS): Erişmek istediğin cihazı söyle
//!     bit 31:    = 1 (enable)
//!     bit 23-16: bus numarası
//!     bit 15-11: device numarası
//!     bit 10-8:  function numarası
//!     bit 7-2:   register offset
//!
//!   Port 0xCFC (CONFIG_DATA): Cihazın konfig değerini oku/yaz
//! ```
//!
//! ## BAR Boyutu Nasıl Saptanir?
//!
//! PCI spesifikasyonuna göre "write-all-ones" trick'i kullanılır:
//! 1. BAR’a `0xFFFFFFFF` yaz.
//! 2. BAR’ı oku: dönen değer sonunda "0" olan bit sayısı = boyut log2.
//!    Örnek: dönüş `0xFF000000` ise boyut = 16 MiB.
//! 3. Orijinal değeri geri yaz (cihazı bozma).

#![allow(dead_code)]

use alloc::vec::Vec;

// ============================================================================
// PCI SABITLER
// ============================================================================
//
// 0xCF8: CONFIG_ADDRESS — hangi cihazı, hangi register'a erişmek istediğimizi söyleriz.
// 0xCFC: CONFIG_DATA    — gerçek veriyi buradan okur/yazarız.
// Bu iki port PCI 2.1 spesifikasyonundan beri sabit — tüm x86 PC'lerde aynı.

const PCI_CFG_ADDR_PORT: u16 = 0x0CF8;
const PCI_CFG_DATA_PORT: u16 = 0x0CFC;

/// BAR dizisinin PCI końfig header'daki offset'i.
/// Tip-0 header (endpoint device): BAR0=0x10, BAR1=0x14, ..., BAR5=0x24
const BAR_OFFSET_BASE:   u8 = 0x10; // Baş BAR (BAR0)
const BAR_COUNT:         usize = 6;  // Toplam 6 BAR ø 32-bit slot

// BAR bit sınıflandırması:
// bit 0 = 1  → I/O port (eski PCI: VGA vs.)
// bit 0 = 0  → MMIO (modürn PCI: GPU, NVMe vs.)
// bit [2:1] = 00 → 32-bit MMIO
// bit [2:1] = 10 → 64-bit MMIO (iki konsekütif BAR slot kullanır!)
const BAR_TYPE_IO:       u32 = 0x01;
const BAR_TYPE_MEM_MASK: u32 = 0x06;
const BAR_TYPE_MEM_64:   u32 = 0x04;

// ============================================================================
// BAR DESCRIPTOR
// ============================================================================

/// One decoded PCI BAR.
#[derive(Debug, Clone)]
pub struct BarInfo {
    pub bar_idx:  u8,
    pub phys_base: u64,
    pub size:      u64,
    pub is_mmio:   bool,
    pub is_64bit:  bool,
    pub is_prefetchable: bool,
}

// ============================================================================
// PCI CONFIG SPACE ACCESS (CF8/CFC port I/O)
// ============================================================================

/// CF8 adresi oluştur.
///
/// x86 PCI spec'e göre:
/// - Bit 31 mutlaka 1 olmalı ("çözümlemeyi etkinleştir")
/// - Bus : 8 bit → 0–255 arası PCI bus
/// - Device: 5 bit → 0–31 arası cihaz
/// - Function: 3 bit → 0–7 arası fonksiyon (multi-function kart)
/// - Register: alt 2 bit sıfırlanır (çünkü 4-byte hizalı erişim)
#[inline]
fn make_pci_addr(bus: u8, device: u8, function: u8, register: u8) -> u32 {
    0x8000_0000
        | ((bus      as u32) << 16)
        | ((device   as u32) << 11)
        | ((function as u32) << 8)
        | ((register as u32) & 0xFC)
}

/// PCI konfig alanından 32-bit DWORD oku.
///
/// ## Protokol (2 adım):
/// 1. 0xCF8'e hedef adres yaz (“bu cihaza / bu register'a gidiyorum”)
/// 2. 0xCFC'den veriyi oku (cihaz cevabı)
#[inline]
unsafe fn pci_read32(bus: u8, device: u8, function: u8, reg: u8) -> u32 {
    let addr = make_pci_addr(bus, device, function, reg);
    crate::cpu::outl(PCI_CFG_ADDR_PORT, addr);
    crate::cpu::inl(PCI_CFG_DATA_PORT)
}

/// PCI konfig alanına 32-bit DWORD yaz.
///
/// ## Ne zaman kullanılır?
/// BAR boyutu ölçmek için 0xFFFFFFFF yazılır, sonra orijinal değer
/// geri yüklenir. BAR adresi firmware tarafından atandığından
/// rastgele değiştirilmemeli — sadece size-probe için gereklidir.
#[inline]
unsafe fn pci_write32(bus: u8, device: u8, function: u8, reg: u8, val: u32) {
    let addr = make_pci_addr(bus, device, function, reg);
    crate::cpu::outl(PCI_CFG_ADDR_PORT, addr);
    crate::cpu::outl(PCI_CFG_DATA_PORT, val);
}

// ============================================================================
// BAR ENUMERATION
// ============================================================================

/// Bir PCI cihazının 6 BAR’nın tamamını okur, boş olmayanları döndürür.
///
/// ## Önemli: Cihazı Önce Durdur!
///
/// BAR'a 0xFFFF_FFFF yazdığımızda cihaz aktif DMA yapıyorsa
/// adresi bozulup bellek silebilir. Bu yüzden GPU passthrough sırasında
/// önce cihazın bus master biti kapatılır (CMD register bit 2 = 0).
///
/// ## 64-bit BAR Mantığı:
/// Bir GPU'nun VRAM window'u genellikle 64-bit'tir (4 GB+ VRAM).
/// Bu durumda iki ardışık BAR slotu kullanılır:
///   BAR[i]   = düşük 32 bit (type=0x04)
///   BAR[i+1] = yüksek 32 bit
/// Bu nedenle döngüde `i += 2` ile iki slotu birden atlarsınız.
pub fn read_bars(bus: u8, device: u8, function: u8) -> Result<Vec<BarInfo>, &'static str> {
    let mut bars = Vec::new();
    let mut i    = 0usize;

    while i < BAR_COUNT {
        let reg    = BAR_OFFSET_BASE + (i as u8) * 4;
        let bar_lo  = unsafe { pci_read32(bus, device, function, reg) };

        if bar_lo == 0 {
            i += 1;
            continue;
        }

        let is_io   = (bar_lo & BAR_TYPE_IO) != 0;
        let is_64   = !is_io && (bar_lo & BAR_TYPE_MEM_MASK) == BAR_TYPE_MEM_64;
        let is_pref = !is_io && (bar_lo & 0x08) != 0;

        // Probe BAR size: write all-ones, read back masked size bits
        unsafe { pci_write32(bus, device, function, reg, 0xFFFF_FFFF); }
        let size_lo = unsafe { pci_read32(bus, device, function, reg) };
        // Restore original
        unsafe { pci_write32(bus, device, function, reg, bar_lo); }

        let (phys_base, size_bytes) = if is_64 && i + 1 < BAR_COUNT {
            // 64-bit BAR: combines BAR[i] (low 32 bits) + BAR[i+1] (high 32 bits)
            let reg_hi  = BAR_OFFSET_BASE + (i as u8 + 1) * 4;
            let bar_hi  = unsafe { pci_read32(bus, device, function, reg_hi) };

            unsafe { pci_write32(bus, device, function, reg,    0xFFFF_FFFF); }
            unsafe { pci_write32(bus, device, function, reg_hi, 0xFFFF_FFFF); }
            let sz_lo = unsafe { pci_read32(bus, device, function, reg) };
            let sz_hi = unsafe { pci_read32(bus, device, function, reg_hi) };
            // Restore
            unsafe { pci_write32(bus, device, function, reg,    bar_lo); }
            unsafe { pci_write32(bus, device, function, reg_hi, bar_hi); }

            let phys = ((bar_hi as u64) << 32) | ((bar_lo & !0xF) as u64);
            let raw_sz = ((sz_hi as u64) << 32) | ((sz_lo & !0xF) as u64);
            let sz = if raw_sz == 0 { 0 } else { !(raw_sz) + 1 };

            i += 2; // consume two BAR slots
            (phys, sz)
        } else if is_io {
            let phys = (bar_lo & !0x3) as u64;
            let raw_sz = (size_lo & !0x3) as u64;
            let sz = if raw_sz == 0 { 0 } else { !raw_sz + 1 };
            i += 1;
            (phys, sz & 0xFFFF)
        } else {
            // 32-bit MMIO BAR
            let phys = (bar_lo & !0xF) as u64;
            let raw_sz = (size_lo & !0xF) as u64;
            let sz = if raw_sz == 0 { 0 } else { !(raw_sz) + 1 };
            i += 1;
            (phys, sz)
        };

        if size_bytes == 0 || phys_base == 0 { continue; }

        bars.push(BarInfo {
            bar_idx:         (i - 1) as u8,
            phys_base,
            size: size_bytes,
            is_mmio:          !is_io,
            is_64bit:         is_64,
            is_prefetchable:  is_pref,
        });
    }

    Ok(bars)
}

/// Read PCI Vendor + Device ID for a device.
pub fn read_vendor_device(bus: u8, device: u8, function: u8) -> (u16, u16) {
    let val = unsafe { pci_read32(bus, device, function, 0x00) };
    let vendor = (val & 0xFFFF) as u16;
    let devid  = (val >> 16) as u16;
    (vendor, devid)
}

/// Check if a device exists (vendor != 0xFFFF).
pub fn device_present(bus: u8, device: u8, function: u8) -> bool {
    let (vendor, _) = read_vendor_device(bus, device, function);
    vendor != 0xFFFF
}

/// Scan PCI bus `bus` and return all present (bus, device, function, vendor, device_id) tuples.
pub fn scan_bus(bus: u8) -> Vec<(u8, u8, u8, u16, u16)> {
    let mut found = Vec::new();
    for dev in 0u8..32 {
        for fun in 0u8..8 {
            let (vendor, devid) = read_vendor_device(bus, dev, fun);
            if vendor != 0xFFFF {
                found.push((bus, dev, fun, vendor, devid));
            }
        }
    }
    found
}
