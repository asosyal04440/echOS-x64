//! # echOS PCI Veri Yolu Sürücüsü
//!
//! Bu modül, PCI (Peripheral Component Interconnect) veri yolundaki donanım
//! aygıtlarını taramak, konfigürasyon alanlarını okuyup yazmak ve MSI/MSI-X
//! kesme mekanizmalarını yapılandırmak için gerekli altyapıyı sağlar.
//!
//! ## PCI Veri Yolu Mimarisi
//!
//! ```text
//!        CPU
//!         |
//!  [Kuzey Köprüsü / PCIe Kök Kompleksi (Root Complex)]
//!         |
//!    [PCIe Bus 0]
//!    |    |    |
//!  Dev0 Dev1 [PCIe Köprü]  <-- Bus numarasını artırır
//!              |      |
//!            Dev0    Dev1
//!
//!  Her aygıt, Bus:Device.Function (BB:DD.F) üçlüsü ile tanımlanır.
//!  Örnek: 00:1f.3 => Bus 0, Cihaz 31, Fonksiyon 3 (Intel HDA Ses)
//!  Bus: 0-255  |  Device: 0-31  |  Function: 0-7
//! ```
//!
//! ## PCI Konfigürasyon Alanı Düzeni (256 byte, Legacy; 4096 byte, ECAM)
//!
//! ```text
//!  Offset | Boyut | Alan
//!  -------+-------+-------------------------------
//!   0x00  |   2   | Vendor ID  (örn: 0x8086 Intel)
//!   0x02  |   2   | Device ID
//!   0x04  |   2   | Command Register (I/O, MMIO, Bus Master bitleri)
//!   0x06  |   2   | Status Register  (bit4=Yetenekler mevcut)
//!   0x08  |   1   | Revision ID
//!   0x09  |   1   | Prog IF  (programlama arayüzü)
//!   0x0A  |   1   | Subclass (alt sınıf kodu)
//!   0x0B  |   1   | Class Code (ana sınıf: 0x01=Depolama, 0x02=Ağ ...)
//!   0x0E  |   1   | Header Type (bit7=Çok fonksiyonlu aygıt)
//!   0x10  | 4x6   | BAR0-BAR5 (Taban Adres Yazmaçları)
//!   0x34  |   1   | Capabilities Pointer (yetenek listesi başlangıcı)
//! ```
//!
//! ## PCI Erişim Yöntemleri: Legacy (PIO) ve ECAM
//!
//! ```text
//!  Legacy Port I/O:                ECAM (Memory-Mapped):
//!  +--------------------------+    +-----------------------------+
//!  | Port 0xCF8 = Adres       |    | MMIO Taban (MCFG tablosunda)|
//!  | Port 0xCFC = Veri        |    |   + (Bus    << 20)          |
//!  |                          |    |   + (Device << 15)          |
//!  | Adres formatı:           |    |   + (Func   << 12)          |
//!  | Bit31=1 (enable)         |    |   + Offset                  |
//!  | Bit23-16 = Bus           |    |                             |
//!  | Bit15-11 = Device        |    | Cihaz başına 4096 byte      |
//!  | Bit10-8  = Function      |    | Doğrudan bellek erişimi     |
//!  | Bit7-2   = Offset        |    +-----------------------------+
//!  | Cihaz başına 256 byte    |
//!  +--------------------------+
//! ```
//!
//! ## BAR (Base Address Register) Yapısı
//!
//! ```text
//!  MMIO BAR (Bit0=0):           I/O Space BAR (Bit0=1):
//!  +-------------------+        +-------------------+
//!  | Bit31-4 = Taban   |        | Bit31-2 = Taban   |
//!  | Bit3 = Prefetch   |        | Bit1 = Rezerve    |
//!  | Bit2-1 = Tip      |        | Bit0 = 1 (I/O)    |
//!  |   00 = 32-bit     |        +-------------------+
//!  |   10 = 64-bit     |
//!  | Bit0 = 0 (MMIO)   |
//!  +-------------------+
//!
//!  Boyut tespiti: 0xFFFFFFFF yaz -> geri oku -> maskeyi tersle -> +1
//! ```
//!
//! ## MSI ve MSI-X Kesme Mekanizması
//!
//! ```text
//!  Geleneksel (INTx):          MSI/MSI-X:
//!
//!    Cihaz                       Cihaz
//!      |                           |
//!    IRQ hattı (paylaşımlı)      Bellek yazma işlemi
//!      |                           |
//!   [PIC / APIC]               0xFEE00000 + (APIC_ID << 12)
//!      |                           |
//!     CPU                        [LAPIC]
//!                                  |
//!                                 CPU
//!
//!  MSI:   Tek vektör, konfigürasyon alanında adres+veri çifti.
//!  MSI-X: Çoklu vektör, MMIO tablosunda her vektör için ayrı giriş.
//!          Her tablo girişi 16 byte: [Adres-Lo | Adres-Hi | Veri | Kontrol]
//! ```

use alloc::vec::Vec;
use core::ptr::{read_volatile, write_volatile};
use spin::Mutex;
use x86_64::instructions::port::Port;

/// PCI aygıtını tanımlayan yapı.
/// Bus:Device.Function üçlüsü ve sınıf/vendor bilgilerini içerir.
#[derive(Debug, Clone)]
pub struct PciDevice {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class_code: u8,
    pub subclass: u8,
    pub prog_if: u8,
    pub header_type: u8,
}

/// MMIO BAR (Bellek Eşlemeli I/O Taban Adres Yazmacı) bilgisi.
/// 64-bit BAR'lar iki ardışık BAR kaydını kullanır.
#[derive(Debug, Clone)]
pub struct PciBar {
    pub base: u64,
    pub size: u64,
    pub is_64: bool,
}

/// I/O Space BAR (Port I/O Taban Adres Yazmacı) bilgisi.
/// x86 in/out komutları ile erişilen eski tip port alanı.
#[derive(Debug, Clone)]
pub struct PciIoBar {
    pub base: u32,
    pub size: u32,
}

/// Örtüşen MMIO bölgelerinin çift rezervasyonunu önlemek için
/// kayıtlı tüm MMIO bölgelerini tutan thread-safe liste.
static PCI_MMIO_RESERVATIONS: Mutex<Vec<(u64, u64)>> = Mutex::new(Vec::new());

/// Verilen MMIO bölgesini (taban, boyut) rezervasyon listesine ekler.
/// Örtüşen bir bölge varsa `false` döner; başarılı rezervasyonda `true`.
fn reserve_mmio_region(base: u64, size: u64) -> bool {
    if size == 0 {
        return false;
    }
    let end = base.saturating_add(size.saturating_sub(1));
    let mut regions = PCI_MMIO_RESERVATIONS.lock();
    for (r_base, r_size) in regions.iter() {
        let r_end = r_base.saturating_add(r_size.saturating_sub(1));
        if base <= r_end && end >= *r_base {
            return false;
        }
    }
    regions.push((base, size));
    true
}

/// Linux glue katmanındaki PciDev yapısından sürücüye özel veriyi alır.
/// Null pointer kontrolü yaparak güvenli referans döner.
fn linux_pci_priv(
    dev: *mut crate::linux_glue::PciDev,
) -> Option<&'static crate::linux_glue::LinuxPciPriv> {
    if dev.is_null() {
        return None;
    }
    let ptr = unsafe { (*dev).driver_data as *const crate::linux_glue::LinuxPciPriv };
    if ptr.is_null() {
        return None;
    }
    Some(unsafe { &*ptr })
}

/// PCI Command Register'ın bit 0 (I/O Space), bit 1 (Memory Space) ve
/// bit 2 (Bus Master) bitlerini etkinleştirir.
/// Bus Master, cihazın DMA yapabilmesi için zorunludur.
pub unsafe fn enable_bus_master(dev: *mut crate::linux_glue::PciDev) -> i32 {
    let priv_data = match linux_pci_priv(dev) {
        Some(data) => data,
        None => return -1,
    };
    let mut command = read_config_dword(priv_data.bus, priv_data.device, priv_data.function, 0x04);
    command |= (1 << 0) | (1 << 1) | (1 << 2);
    write_config_dword(
        priv_data.bus,
        priv_data.device,
        priv_data.function,
        0x04,
        command,
    );
    0
}

/// Aygıtın kaynaklarını (BAR bölgelerini) MMIO rezervasyon listesine ekler.
/// Başarısız veya çakışan rezervasyonda -1 döner.
pub unsafe fn request_regions(dev: *mut crate::linux_glue::PciDev) -> i32 {
    if dev.is_null() {
        return -1;
    }
    let resources = unsafe { &(*dev).resource };
    let mut has_region = false;
    for res in resources {
        if res.start != 0 && res.end >= res.start {
            let size = res.end.saturating_sub(res.start).saturating_add(1);
            if (res.flags & 0x0000_0200) != 0 && !reserve_mmio_region(res.start, size) {
                return -1;
            }
            has_region = true;
        }
    }
    if has_region {
        0
    } else {
        -1
    }
}

/// Tüm PCI veri yolunu tarar. ACPI MCFG tablosu varsa ECAM, yoksa
/// geleneksel Port I/O yöntemi kullanılır.
pub fn scan() -> Vec<PciDevice> {
    #[cfg(any(test, target_os = "windows"))]
    {
        return Vec::new();
    }
    #[cfg(not(any(test, target_os = "windows")))]
    {
        let entries = crate::cpu::acpi::get_mcfg_entries();
        if !entries.is_empty() {
            scan_ecam(&entries)
        } else {
            scan_legacy()
        }
    }
}

/// Belirli bir bus numarasına karşılık gelen ECAM MMIO taban adresini döner.
/// MCFG tablosunda bulunamazsa varsayılan olarak 0xE000_0000 kullanılır.
pub fn ecam_base_for_bus(bus: u8) -> Option<u64> {
    let entries = crate::cpu::acpi::get_mcfg_entries();
    for entry in entries {
        if bus >= entry.start_bus && bus <= entry.end_bus {
            return Some(entry.base_address);
        }
    }
    Some(0xE000_0000)
}

/// Bulunan tüm PCI aygıtlarını ve yeteneklerini (MSI/MSI-X/PCIe) seri porta yazdırır.
/// Hata ayıklama amacıyla kullanılır.
pub fn debug_print() {
    let devices = scan();
    for dev in devices {
        crate::serial_println!(
            "PCI {:02x}:{:02x}.{} vendor={:04x} device={:04x} class={:02x} subclass={:02x} prog_if={:02x}",
            dev.bus,
            dev.device,
            dev.function,
            dev.vendor_id,
            dev.device_id,
            dev.class_code,
            dev.subclass,
            dev.prog_if
        );
        let caps = read_capabilities(dev.bus, dev.device, dev.function);
        if caps.has_msi || caps.has_msix || caps.has_pcie {
            crate::serial_println!(
                "  CAPS MSI={} MSI-X={} PCIe={} msi_off=0x{:02x} msix_off=0x{:02x} pcie_off=0x{:02x}",
                caps.has_msi as u8,
                caps.has_msix as u8,
                caps.has_pcie as u8,
                caps.msi_offset,
                caps.msix_offset,
                caps.pcie_offset
            );
        }
    }
}

/// PCI aygıtı için uygun sürücüyü başlatmaya çalışır.
/// IDE denetleyicileri atlanır; VirtIO blok aygıtları için komut bitleri açılır.
pub fn init_driver(dev: &PciDevice) -> bool {
    if dev.class_code == 0x01 && dev.subclass == 0x01 {
        crate::serial_println!(
            "Skipping IDE controller {:02x}:{:02x}.{}",
            dev.bus,
            dev.device,
            dev.function
        );
        return false;
    }
    if dev.vendor_id == 0x1AF4 && (dev.device_id == 0x1001 || dev.device_id == 0x1042) {
        let mut command = read_config_dword(dev.bus, dev.device, dev.function, 0x04);
        command |= (1 << 0) | (1 << 1) | (1 << 2);
        write_config_dword(dev.bus, dev.device, dev.function, 0x04, command);
        crate::serial_println!(
            "PCI CMD ENABLED for Dev {:x}:{:x}",
            dev.vendor_id,
            dev.device_id
        );
        crate::serial_println!("VIRTIO BLK: init transport main tarafından sağlanacak");
        return false;
    }
    false
}

/// Bir PCI aygıtının desteklediği yetenek tiplerini ve config-space ofsetlerini özetler.
/// MSI, MSI-X ve PCIe yeteneği varlığını bayrakla raporlar.
#[derive(Debug, Clone, Copy)]
pub struct PciCapabilityInfo {
    pub has_msi: bool,
    pub has_msix: bool,
    pub has_pcie: bool,
    pub msi_offset: u8,
    pub msix_offset: u8,
    pub pcie_offset: u8,
}

/// MSI (Message Signaled Interrupt) yetenek yapısı.
/// Adres, veri ve kontrol alanlarını içerir.
/// 64-bit destekte üst 32-bit adres ayrıca saklanır.
#[derive(Debug, Clone, Copy)]
pub struct MsiCapability {
    pub control: u16,
    pub address: u64,
    pub data: u16,
}

/// MSI-X yetenek yapısı.
/// Tablo ve PBA (Bekleyen Bit Dizisi) bilgilerini içerir.
///
/// ```text
///  MSI-X Tablo Girişi (16 byte):
///  +------------+------------+--------+---------+
///  | Adres-Lo   | Adres-Hi   | Veri   | Kontrol |
///  | (4 byte)   | (4 byte)   |(4 byte)|(4 byte) |
///  +------------+------------+--------+---------+
///      ^--- Bu adreste LAPIC'e mesaj gönderilir
/// ```
#[derive(Debug, Clone, Copy)]
pub struct MsixCapability {
    pub control: u16,
    pub table_bar: u8,
    pub table_offset: u32,
    pub pba_bar: u8,
    pub pba_offset: u32,
}

/// Konfigürasyon alanından 16-bit (word) değer okur.
/// İçeride `read_config_dword` çağrısı yapar, uygun biti maskeler.
pub fn read_config_word(bus: u8, device: u8, function: u8, offset: u16) -> u16 {
    let value = read_config_dword(bus, device, function, offset);
    let shift = ((offset & 2) * 8) as u32;
    ((value >> shift) & 0xFFFF) as u16
}

/// Konfigürasyon alanından 8-bit (byte) değer okur.
/// Dword içindeki byte konumunu offset ile hesaplar.
fn read_config_byte(bus: u8, device: u8, function: u8, offset: u16) -> u8 {
    let value = read_config_dword(bus, device, function, offset);
    let shift = ((offset & 3) * 8) as u32;
    ((value >> shift) & 0xFF) as u8
}

/// Konfigürasyon alanına 16-bit (word) değer yazar.
/// Önce mevcut dword okunur, hedef word güncellenerek geri yazılır (RMW).
pub fn write_config_word(bus: u8, device: u8, function: u8, offset: u16, value: u16) {
    let aligned = offset & 0xFFFC;
    let current = read_config_dword(bus, device, function, aligned);
    let shift = ((offset & 2) * 8) as u32;
    let mask = 0xFFFFu32 << shift;
    let new_value = (current & !mask) | ((value as u32) << shift);
    write_config_dword(bus, device, function, aligned, new_value);
}

const PCI_CAP_PTR_MIN: u8 = 0x40;
const PCI_CAP_PTR_MAX: u8 = 0xFC;
const PCI_EXT_CAP_START: u16 = 0x100;
const PCI_EXT_CAP_LAST_DWORD: u16 = 0xFFC;
const PCI_EXT_CAP_MAX_STEPS: usize = 256;

#[inline]
fn sanitize_cap_ptr(raw: u8) -> u8 {
    raw & 0xFC
}

#[inline]
fn valid_cap_ptr(ptr: u8) -> bool {
    ptr >= PCI_CAP_PTR_MIN && ptr <= PCI_CAP_PTR_MAX && (ptr & 0x3) == 0
}

#[inline]
fn decode_ext_cap_next(header: u32) -> u16 {
    (((header >> 20) & 0xFFF) as u16) & !0x3
}

#[inline]
fn valid_ext_cap_offset(offset: u16) -> bool {
    offset >= PCI_EXT_CAP_START && offset <= PCI_EXT_CAP_LAST_DWORD && (offset & 0x3) == 0
}

#[cfg(test)]
pub(crate) fn phase5_kickoff_contract_green() -> bool {
    let cap_ptr_contract = sanitize_cap_ptr(0x47) == 0x44
        && valid_cap_ptr(0x40)
        && !valid_cap_ptr(0x3C);
    let ext_cap_contract = decode_ext_cap_next((0x123u32 << 20) | 0x0001) == 0x120
        && valid_ext_cap_offset(0x100)
        && !valid_ext_cap_offset(0x0FC);
    let msix_window_contract = msix_table_window_fits(0x1000, 0x0F00, 0, 16)
        && !msix_table_window_fits(0x1000, 0x0FF0, 2, 2)
        && !msix_table_window_fits(0x1000, 0, 0, 0);
    cap_ptr_contract && ext_cap_contract && msix_window_contract
}

/// PCI Yetenekler listesini yürütür (walk).
/// Status register bit4=1 ise liste mevcuttur; 0x34 ofsetindeki işaretçiden başlar.
/// Her yetenek: [ID (1 byte) | Sonraki (1 byte) | ...] formatındadır.
/// Sonsuz döngüyü önlemek için 64 adımda guard sınırı uygulanır.
fn walk_capabilities<F>(bus: u8, device: u8, function: u8, mut f: F)
where
    F: FnMut(u8, u8),
{
    let status = read_config_word(bus, device, function, 0x06);
    if (status & (1 << 4)) == 0 {
        return;
    }
    let mut ptr = sanitize_cap_ptr(read_config_byte(bus, device, function, 0x34));
    let mut guard = 0u8;
    let mut seen = [false; 64];
    while valid_cap_ptr(ptr) && guard < 64 {
        let slot = (ptr >> 2) as usize;
        if seen[slot] {
            break;
        }
        seen[slot] = true;
        let cap_id = read_config_byte(bus, device, function, ptr as u16);
        if cap_id == 0 || cap_id == 0xFF {
            break;
        }
        let next = sanitize_cap_ptr(read_config_byte(bus, device, function, ptr as u16 + 1));
        f(cap_id, ptr);
        if next == 0 {
            break;
        }
        ptr = next;
        guard = guard.wrapping_add(1);
    }
}

/// PCI aygıtının yetenek listesini tarayarak MSI (0x05),
/// MSI-X (0x11) ve PCIe (0x10) yeteneklerini tespit eder.
/// Her yeteneğin config-space ofseti de kaydedilir.
pub fn read_capabilities(bus: u8, device: u8, function: u8) -> PciCapabilityInfo {
    let mut info = PciCapabilityInfo {
        has_msi: false,
        has_msix: false,
        has_pcie: false,
        msi_offset: 0,
        msix_offset: 0,
        pcie_offset: 0,
    };
    walk_capabilities(bus, device, function, |cap_id, ptr| match cap_id {
        0x05 => {
            if !info.has_msi {
                info.has_msi = true;
                info.msi_offset = ptr;
            }
        }
        0x11 => {
            if !info.has_msix {
                info.has_msix = true;
                info.msix_offset = ptr;
            }
        }
        0x10 => {
            if !info.has_pcie {
                info.has_pcie = true;
                info.pcie_offset = ptr;
            }
        }
        _ => {}
    });
    info
}

/// MSI yetenek yapısını konfigürasyon alanından okur.
/// 64-bit mod destekleniyorsa (control bit7=1) adresin üst 32 biti de alınır.
pub fn read_msi(bus: u8, device: u8, function: u8) -> Option<MsiCapability> {
    let caps = read_capabilities(bus, device, function);
    if !caps.has_msi || caps.msi_offset == 0 {
        return None;
    }
    let offset = caps.msi_offset as u16;
    let control = read_config_word(bus, device, function, offset + 2);
    let is_64 = (control & (1 << 7)) != 0;
    let addr_low = read_config_dword(bus, device, function, offset + 4) as u64;
    let address = if is_64 {
        let addr_high = read_config_dword(bus, device, function, offset + 8) as u64;
        (addr_high << 32) | addr_low
    } else {
        addr_low
    };
    let data_offset = if is_64 { offset + 12 } else { offset + 8 };
    let data = read_config_word(bus, device, function, data_offset);
    Some(MsiCapability {
        control,
        address,
        data,
    })
}

/// MSI-X yetenek yapısını konfigürasyon alanından okur.
/// Tablo ve PBA alanlarının hangi BAR'da ve hangi ofsette olduğunu döner.
pub fn read_msix(bus: u8, device: u8, function: u8) -> Option<MsixCapability> {
    let caps = read_capabilities(bus, device, function);
    if !caps.has_msix || caps.msix_offset == 0 {
        return None;
    }
    let offset = caps.msix_offset as u16;
    let control = read_config_word(bus, device, function, offset + 2);
    let table = read_config_dword(bus, device, function, offset + 4);
    let pba = read_config_dword(bus, device, function, offset + 8);
    let table_bar = (table & 0x7) as u8;
    let table_offset = table & !0x7;
    let pba_bar = (pba & 0x7) as u8;
    let pba_offset = pba & !0x7;
    Some(MsixCapability {
        control,
        table_bar,
        table_offset,
        pba_bar,
        pba_offset,
    })
}

/// MSI mesaj adresini hesaplar: 0xFEE00000 | (APIC_ID << 12).
/// x2APIC modunda tüm 32-bit APIC kimliği kullanılır; xAPIC'te sadece 8-bit.
fn msi_message_address(apic_id: u32) -> u64 {
    let mode = crate::apic::lapic::mode();
    let dest = match mode {
        crate::apic::lapic::ApicMode::X2Apic => apic_id as u64,
        _ => (apic_id as u64) & 0xFF,
    };
    0xFEE0_0000u64 | (dest << 12)
}

/// MSI mesaj verisini oluşturur: interrupt vektör numarasını 16-bit olarak döner.
fn msi_message_data(vector: u8) -> u16 {
    vector as u16
}

/// Tek vektör için MSI konfigürasyonu yapar.
/// Konfigürasyon alanına adres ve veri yazılır; ardından MSI Enable biti (bit0=1) etkinleştirilir.
/// Command register'da Interrupt Disable (bit10=0) yapılmaz -- bitler uygun ayarlanır.
pub fn configure_msi(bus: u8, device: u8, function: u8, vector: u8, apic_id: u32) -> bool {
    let caps = read_capabilities(bus, device, function);
    if !caps.has_msi || caps.msi_offset == 0 {
        return false;
    }
    let offset = caps.msi_offset as u16;
    let control = read_config_word(bus, device, function, offset + 2);
    let is_64 = (control & (1 << 7)) != 0;
    let address = msi_message_address(apic_id);
    let data = msi_message_data(vector);
    write_config_dword(bus, device, function, offset + 4, address as u32);
    if is_64 {
        write_config_dword(bus, device, function, offset + 8, (address >> 32) as u32);
    }
    let data_offset = if is_64 { offset + 12 } else { offset + 8 };
    write_config_word(bus, device, function, data_offset, data);
    let mut new_control = control & !0x70;
    new_control |= 1;
    write_config_word(bus, device, function, offset + 2, new_control);
    let mut command = read_config_word(bus, device, function, 0x04);
    command |= 1 << 10;
    write_config_word(bus, device, function, 0x04, command);
    true
}

/// Birden fazla vektör için MSI çoklu mesaj konfigürasyonu yapar.
/// Vektörler ardışık ve 2'nin kuvveti sayıda olmalıdır (MME alanı bunu gerektirir).
/// Control register'ın bit6-4 alanına istenen log2(vektör sayısı) yazılır.
pub fn configure_msi_multi(
    bus: u8,
    device: u8,
    function: u8,
    vectors: &[u8],
    apic_id: u32,
) -> bool {
    if vectors.is_empty() {
        return false;
    }
    let caps = read_capabilities(bus, device, function);
    if !caps.has_msi || caps.msi_offset == 0 {
        return false;
    }
    let base_vector = vectors[0];
    for (idx, &vector) in vectors.iter().enumerate() {
        if vector != base_vector.wrapping_add(idx as u8) {
            return false;
        }
    }
    let count = vectors.len() as u16;
    if count == 0 || (count & (count - 1)) != 0 {
        return false;
    }
    let offset = caps.msi_offset as u16;
    let control = read_config_word(bus, device, function, offset + 2);
    let is_64 = (control & (1 << 7)) != 0;
    let max_log = ((control >> 1) & 0x7) as u16;
    let req_log = count.trailing_zeros() as u16;
    if req_log > max_log {
        return false;
    }
    let address = msi_message_address(apic_id);
    let data = msi_message_data(base_vector);
    write_config_dword(bus, device, function, offset + 4, address as u32);
    if is_64 {
        write_config_dword(bus, device, function, offset + 8, (address >> 32) as u32);
    }
    let data_offset = if is_64 { offset + 12 } else { offset + 8 };
    write_config_word(bus, device, function, data_offset, data);
    let mut new_control = control & !0x70;
    new_control |= (req_log << 4) & 0x70;
    new_control |= 1;
    write_config_word(bus, device, function, offset + 2, new_control);
    let mut command = read_config_word(bus, device, function, 0x04);
    command |= 1 << 10;
    write_config_word(bus, device, function, 0x04, command);
    true
}

/// Tek bir MSI-X tablo girişini yapılandırır.
/// Tablo, ilgili BAR'da MMIO olarak eşlenir; giriş adresi ve vektör verisi yazılır.
/// MSI-X Enable (bit15=1) ve Function Mask (bit14=0) bitleri güncellenir.
pub fn configure_msix(
    bus: u8,
    device: u8,
    function: u8,
    table_index: u16,
    vector: u8,
    apic_id: u32,
) -> bool {
    let caps = read_capabilities(bus, device, function);
    if !caps.has_msix || caps.msix_offset == 0 {
        return false;
    }
    let offset = caps.msix_offset as u16;
    let control = read_config_word(bus, device, function, offset + 2);
    let table_size = (control & 0x07FF) as u16 + 1;
    if table_index >= table_size {
        return false;
    }
    let msix = match read_msix(bus, device, function) {
        Some(msix) => msix,
        None => return false,
    };
    let bar = match read_bar_mmio(bus, device, function, msix.table_bar) {
        Some(bar) => bar,
        None => return false,
    };
    if !msix_table_window_fits(bar.size, msix.table_offset, table_index, 1) {
        return false;
    }
    // Programlama sırasında function-level mask kullanılır.
    write_config_word(bus, device, function, offset + 2, control | (1 << 14));
    let mapped = crate::memory::map_mmio(bar.base, bar.size as usize);
    let base = if mapped.is_null() {
        crate::memory::active_physical_offset() + bar.base
    } else {
        mapped as u64
    };
    let entry_addr = base + msix.table_offset as u64 + (table_index as u64) * 16;
    let address = msi_message_address(apic_id);
    let data = msi_message_data(vector) as u32;
    unsafe {
        write_volatile((entry_addr + 12) as *mut u32, 1);
        write_volatile(entry_addr as *mut u32, address as u32);
        write_volatile((entry_addr + 4) as *mut u32, (address >> 32) as u32);
        write_volatile((entry_addr + 8) as *mut u32, data);
        write_volatile((entry_addr + 12) as *mut u32, 0);
    }
    let mut new_control = control | (1 << 15);
    new_control &= !(1 << 14);
    write_config_word(bus, device, function, offset + 2, new_control);
    let mut command = read_config_word(bus, device, function, 0x04);
    command |= 1 << 10;
    write_config_word(bus, device, function, 0x04, command);
    true
}

/// Birden fazla MSI-X tablo girişini toplu olarak yapılandırır.
/// `table_base` başlangıç indeksinden itibaren `vectors.len()` kadar giriş ayarlanır.
pub fn configure_msix_table(
    bus: u8,
    device: u8,
    function: u8,
    table_base: u16,
    vectors: &[u8],
    apic_id: u32,
) -> bool {
    if vectors.is_empty() {
        return false;
    }
    let caps = read_capabilities(bus, device, function);
    if !caps.has_msix || caps.msix_offset == 0 {
        return false;
    }
    let offset = caps.msix_offset as u16;
    let control = read_config_word(bus, device, function, offset + 2);
    let table_size = (control & 0x07FF) as u16 + 1;
    let entries_u16 = vectors.len() as u16;
    let Some(table_end) = table_base.checked_add(entries_u16) else {
        return false;
    };
    if table_end > table_size {
        return false;
    }
    let msix = match read_msix(bus, device, function) {
        Some(msix) => msix,
        None => return false,
    };
    let bar = match read_bar_mmio(bus, device, function, msix.table_bar) {
        Some(bar) => bar,
        None => return false,
    };
    if !msix_table_window_fits(bar.size, msix.table_offset, table_base, vectors.len()) {
        return false;
    }
    write_config_word(bus, device, function, offset + 2, control | (1 << 14));
    let mapped = crate::memory::map_mmio(bar.base, bar.size as usize);
    let base = if mapped.is_null() {
        crate::memory::active_physical_offset() + bar.base
    } else {
        mapped as u64
    };
    let address = msi_message_address(apic_id);
    for (idx, &vector) in vectors.iter().enumerate() {
        let entry = table_base as u64 + idx as u64;
        let entry_addr = base + msix.table_offset as u64 + entry * 16;
        let data = msi_message_data(vector) as u32;
        unsafe {
            write_volatile((entry_addr + 12) as *mut u32, 1);
            write_volatile(entry_addr as *mut u32, address as u32);
            write_volatile((entry_addr + 4) as *mut u32, (address >> 32) as u32);
            write_volatile((entry_addr + 8) as *mut u32, data);
            write_volatile((entry_addr + 12) as *mut u32, 0);
        }
    }
    let mut new_control = control | (1 << 15);
    new_control &= !(1 << 14);
    write_config_word(bus, device, function, offset + 2, new_control);
    let mut command = read_config_word(bus, device, function, 0x04);
    command |= 1 << 10;
    write_config_word(bus, device, function, 0x04, command);
    true
}

/// Tek vektör için politikaya göre kesme konfigürasyonu yapar.
/// Politika MSI-X öncelikliyse önce MSI-X, başarısızsa MSI denenir; tersi de geçerli.
/// LegacyOnly politikasında her zaman false döner.
pub fn configure_pci_interrupt(
    bus: u8,
    device: u8,
    function: u8,
    vector: u8,
    apic_id: u32,
) -> bool {
    let policy = crate::interrupts::pci_interrupt_policy();
    let target_apic = crate::interrupts::resolve_msi_target(vector, apic_id);
    match policy {
        crate::interrupts::PciInterruptPolicy::MsiXPreferred => {
            if configure_msix(bus, device, function, 0, vector, target_apic) {
                return true;
            }
            configure_msi(bus, device, function, vector, target_apic)
        }
        crate::interrupts::PciInterruptPolicy::MsiPreferred => {
            if configure_msi(bus, device, function, vector, target_apic) {
                return true;
            }
            configure_msix(bus, device, function, 0, vector, target_apic)
        }
        crate::interrupts::PciInterruptPolicy::LegacyOnly => false,
    }
}

/// Birden fazla vektör için politikaya göre kesme konfigürasyonu yapar.
/// MSI-X tercihinde her vektör ayrı tablo girişine yazılır;
/// MSI tercihinde çoklu MSI (MME modunda) denenir.
pub fn configure_pci_interrupts(
    bus: u8,
    device: u8,
    function: u8,
    vectors: &[u8],
    apic_id: u32,
) -> bool {
    if vectors.is_empty() {
        return false;
    }
    let policy = crate::interrupts::pci_interrupt_policy();
    match policy {
        crate::interrupts::PciInterruptPolicy::MsiXPreferred => {
            let mut ok = true;
            for (idx, &vector) in vectors.iter().enumerate() {
                let target_apic = crate::interrupts::resolve_msi_target(vector, apic_id);
                if !configure_msix(bus, device, function, idx as u16, vector, target_apic) {
                    ok = false;
                    break;
                }
            }
            if ok {
                return true;
            }
            let target_apic = crate::interrupts::resolve_msi_target(vectors[0], apic_id);
            configure_msi_multi(bus, device, function, vectors, target_apic)
        }
        crate::interrupts::PciInterruptPolicy::MsiPreferred => {
            let target_apic = crate::interrupts::resolve_msi_target(vectors[0], apic_id);
            if configure_msi_multi(bus, device, function, vectors, target_apic) {
                return true;
            }
            let mut ok = true;
            for (idx, &vector) in vectors.iter().enumerate() {
                let target_apic = crate::interrupts::resolve_msi_target(vector, apic_id);
                if !configure_msix(bus, device, function, idx as u16, vector, target_apic) {
                    ok = false;
                    break;
                }
            }
            ok
        }
        crate::interrupts::PciInterruptPolicy::LegacyOnly => false,
    }
}

/// PCI config alanını cihazın erişim yöntemine göre okur.
/// ACPI MCFG tablosunda eşleşen segment varsa ECAM, aksi halde Legacy PIO kullanılır.
pub fn read_config_dword(bus: u8, device: u8, function: u8, offset: u16) -> u32 {
    #[cfg(any(test, target_os = "windows"))]
    {
        let _ = (bus, device, function, offset);
        return 0xFFFF_FFFF;
    }
    #[cfg(not(any(test, target_os = "windows")))]
    {
        let aligned = offset & 0xFFFC;
        let entries = crate::cpu::acpi::get_mcfg_entries();
        for entry in entries {
            if bus >= entry.start_bus && bus <= entry.end_bus {
                return read_ecam_dword(entry.base_address, bus, device, function, aligned);
            }
        }
        read_legacy_dword(bus, device, function, aligned as u8)
    }
}

/// PCI config alanına cihazın erişim yöntemine göre yazar.
/// ECAM modunda doğrudan bellek yazımı; Legacy modunda 0xCF8/0xCFC port yazımı kullanılır.
pub fn write_config_dword(bus: u8, device: u8, function: u8, offset: u16, value: u32) {
    #[cfg(any(test, target_os = "windows"))]
    {
        let _ = (bus, device, function, offset, value);
        return;
    }
    #[cfg(not(any(test, target_os = "windows")))]
    {
        let aligned = offset & 0xFFFC;
        let entries = crate::cpu::acpi::get_mcfg_entries();
        for entry in entries {
            if bus >= entry.start_bus && bus <= entry.end_bus {
                let address = entry.base_address
                    + ((bus as u64) << 20)
                    + ((device as u64) << 15)
                    + ((function as u64) << 12)
                    + ((aligned as u64) & 0xFFC);
                unsafe {
                    write_volatile(address as *mut u32, value);
                }
                return;
            }
        }
        let address: u32 = 0x8000_0000
            | ((bus as u32) << 16)
            | ((device as u32) << 11)
            | ((function as u32) << 8)
            | ((aligned as u32) & 0xFC);
        unsafe {
            let mut addr_port = Port::<u32>::new(0xCF8);
            let mut data_port = Port::<u32>::new(0xCFC);
            addr_port.write(address);
            data_port.write(value);
        }
    }
}

/// PCI BAR'ını okuyup MMIO taban ve boyut bilgisini çıkarır.
///
/// Boyut tespiti:
///   1. Özgün değeri sakla.
///   2. BAR'a 0xFFFFFFFF yaz.
///   3. Geri oku -> maskelenmiş değer.
///   4. Maskeyi tersle, +1 yap: bu boyuttur.
///   5. Özgün değeri yeniden yaz.
pub fn read_bar_mmio(bus: u8, device: u8, function: u8, bar_index: u8) -> Option<PciBar> {
    if bar_index >= 6 {
        return None;
    }
    let offset = 0x10u16 + (bar_index as u16) * 4;
    let original = read_config_dword(bus, device, function, offset);
    if original == 0 || (original & 0x1) != 0 {
        return None;
    }
    let is_64 = (original & 0x6) == 0x4;
    write_config_dword(bus, device, function, offset, 0xFFFF_FFFF);
    let mask_low = read_config_dword(bus, device, function, offset);
    write_config_dword(bus, device, function, offset, original);
    let masked_low = mask_low & 0xFFFF_FFF0;
    if masked_low == 0 {
        return None;
    }
    let mut base = (original & 0xFFFF_FFF0) as u64;
    let mut size = (!masked_low + 1) as u64;
    if is_64 {
        if bar_index == 5 {
            // 64-bit BAR iki register tüketir; BAR5 üst dword için alan yok.
            return None;
        }
        let offset_high = offset + 4;
        let original_high = read_config_dword(bus, device, function, offset_high);
        write_config_dword(bus, device, function, offset_high, 0xFFFF_FFFF);
        let mask_high = read_config_dword(bus, device, function, offset_high);
        write_config_dword(bus, device, function, offset_high, original_high);
        let mask_full = ((mask_high as u64) << 32) | masked_low as u64;
        if mask_full == 0 {
            return None;
        }
        base |= (original_high as u64) << 32;
        size = (!mask_full + 1) & 0xFFFF_FFFF_FFFF_FFF0;
    } else {
        size &= 0xFFFF_FFF0;
    }
    Some(PciBar { base, size, is_64 })
}

fn msix_table_window_fits(bar_size: u64, table_offset: u32, table_base: u16, entry_count: usize) -> bool {
    if entry_count == 0 {
        return false;
    }
    let Some(byte_len) = (entry_count as u64).checked_mul(16) else {
        return false;
    };
    let Some(start) = (table_offset as u64).checked_add((table_base as u64).saturating_mul(16)) else {
        return false;
    };
    let Some(end) = start.checked_add(byte_len) else {
        return false;
    };
    end <= bar_size
}

/// I/O Space BAR'ını okuyarak port taban adresini ve boyutunu döner.
/// BAR bit0=1 olmalıdır; aksi hâlde MMIO BAR'dır ve None döner.
pub fn read_bar_io(bus: u8, device: u8, function: u8, bar_index: u8) -> Option<PciIoBar> {
    if bar_index >= 6 {
        return None;
    }
    let offset = 0x10u16 + (bar_index as u16) * 4;
    let original = read_config_dword(bus, device, function, offset);
    if original == 0 || (original & 0x1) == 0 {
        return None;
    }
    write_config_dword(bus, device, function, offset, 0xFFFF_FFFF);
    let mask = read_config_dword(bus, device, function, offset);
    write_config_dword(bus, device, function, offset, original);
    let masked = mask & 0xFFFF_FFFC;
    if masked == 0 {
        return None;
    }
    let base = original & 0xFFFF_FFFC;
    let size = (!masked).wrapping_add(1) & 0xFFFF_FFFC;
    Some(PciIoBar { base, size })
}

/// PCI köprü sınıfı kodları: 0x06=Bridge, 0x04=PCI-to-PCI Bridge.
/// Köprü tespit edildiğinde ikincil bus numarası okunarak özyinelemeli tarama yapılır.
const PCI_CLASS_BRIDGE: u8 = 0x06;
const PCI_SUBCLASS_PCI_BRIDGE: u8 = 0x04;

/// Legacy (Port I/O) modunda bus 0'dan başlayarak tüm veri yolunu tarar.
/// Köprüler tespit edildiğinde ikincil bus'lar da özyinelemeli olarak taranır.
fn scan_legacy() -> Vec<PciDevice> {
    let mut devices = Vec::new();
    // Legacy PCI için köprüleri takip ederek tarama yap
    let mut visited = [false; 256];
    scan_bus_legacy_recursive(0, &mut visited, &mut devices);
    devices
}

/// Belirli bir bus numarasını Legacy modda tarar.
/// 0-31 arası cihaz numarası, 0-7 arası fonksiyon sayısı kontrol edilir.
fn scan_bus_legacy_recursive(bus: u8, visited: &mut [bool; 256], devices: &mut Vec<PciDevice>) {
    // Aynı bus'ın tekrar taranmasını engelle
    if visited[bus as usize] {
        return;
    }
    visited[bus as usize] = true;
    for device in 0u8..32 {
        let vendor_id = read_legacy_word(bus, device, 0, 0x00);
        if vendor_id == 0xFFFF {
            continue;
        }
        let header_type = read_legacy_byte(bus, device, 0, 0x0E);
        let function_count = if header_type & 0x80 != 0 { 8 } else { 1 };
        for function in 0u8..function_count {
            let vendor_id = read_legacy_word(bus, device, function, 0x00);
            if vendor_id == 0xFFFF {
                continue;
            }
            let dev = read_device_legacy(bus, device, function);
            crate::serial_println!(
                "PCI SCAN {:02x}:{:02x}.{} vendor={:04x} device={:04x} class={:02x} subclass={:02x} prog_if={:02x}",
                dev.bus,
                dev.device,
                dev.function,
                dev.vendor_id,
                dev.device_id,
                dev.class_code,
                dev.subclass,
                dev.prog_if
            );
            if dev.class_code == 0x01 {
                crate::serial_println!(
                    "PCI STORAGE FOUND: Bus/Dev/Fn {:02x}:{:02x}.{} ID={:04x}:{:04x}",
                    dev.bus,
                    dev.device,
                    dev.function,
                    dev.vendor_id,
                    dev.device_id
                );
            }
            let is_bridge =
                dev.class_code == PCI_CLASS_BRIDGE && dev.subclass == PCI_SUBCLASS_PCI_BRIDGE;
            devices.push(dev);
            if is_bridge {
                let secondary = read_legacy_byte(bus, device, function, 0x19);
                if secondary != 0 && secondary != 0xFF {
                    scan_bus_legacy_recursive(secondary, visited, devices);
                }
            }
        }
    }
}

/// Legacy PIO yöntemiyle bir cihazın tüm tanımlayıcı alanlarını okur ve
/// `PciDevice` yapısı oluşturur.
fn read_device_legacy(bus: u8, device: u8, function: u8) -> PciDevice {
    let vendor_id = read_legacy_word(bus, device, function, 0x00);
    let device_id = read_legacy_word(bus, device, function, 0x02);
    let prog_if = read_legacy_byte(bus, device, function, 0x09);
    let subclass = read_legacy_byte(bus, device, function, 0x0A);
    let class_code = read_legacy_byte(bus, device, function, 0x0B);
    let header_type = read_legacy_byte(bus, device, function, 0x0E);
    PciDevice {
        bus,
        device,
        function,
        vendor_id,
        device_id,
        class_code,
        subclass,
        prog_if,
        header_type,
    }
}

/// Legacy PIO ile 16-bit konfigürasyon verisi okur.
fn read_legacy_word(bus: u8, device: u8, function: u8, offset: u8) -> u16 {
    let value = read_legacy_dword(bus, device, function, offset);
    let shift = ((offset & 2) * 8) as u32;
    ((value >> shift) & 0xFFFF) as u16
}

/// Legacy PIO ile 8-bit konfigürasyon verisi okur.
fn read_legacy_byte(bus: u8, device: u8, function: u8, offset: u8) -> u8 {
    let value = read_legacy_dword(bus, device, function, offset);
    let shift = ((offset & 3) * 8) as u32;
    ((value >> shift) & 0xFF) as u8
}

/// Legacy PIO mekanizmasıyla 32-bit konfigürasyon dword'ü okur.
/// Adres portu 0xCF8'e yazılır; veri portu 0xCFC'den okunur.
fn read_legacy_dword(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    let address: u32 = 0x8000_0000
        | ((bus as u32) << 16)
        | ((device as u32) << 11)
        | ((function as u32) << 8)
        | ((offset as u32) & 0xFC);
    unsafe {
        let mut addr_port = Port::<u32>::new(0xCF8);
        let mut data_port = Port::<u32>::new(0xCFC);
        addr_port.write(address);
        data_port.read()
    }
}

/// ACPI MCFG tablosundaki tüm segmentler için ECAM taraması yapar.
/// Her segment kendi bus aralığını kapsar; örtüşen taramayı önlemek için
/// `visited` dizisi per-segment tutulur.
fn scan_ecam(entries: &[crate::cpu::acpi::PciEcamInfo]) -> Vec<PciDevice> {
    let mut devices = Vec::new();
    for entry in entries {
        // Her segment için bus taramasını ayrı tut
        let mut visited = [false; 256];
        for bus in entry.start_bus..=entry.end_bus {
            if !visited[bus as usize] {
                scan_bus_ecam_recursive(
                    entry.base_address,
                    entry.start_bus,
                    entry.end_bus,
                    bus,
                    &mut visited,
                    &mut devices,
                );
            }
        }
    }
    devices
}

/// ECAM yöntemiyle belirtilen bus'ı tarar ve köprü bulunursa
/// ikincil bus'ı özyinelemeli olarak da tarar.
fn scan_bus_ecam_recursive(
    base: u64,
    start_bus: u8,
    end_bus: u8,
    bus: u8,
    visited: &mut [bool; 256],
    devices: &mut Vec<PciDevice>,
) {
    // ECAM aralığı dışına taşmayı engelle
    if bus < start_bus || bus > end_bus {
        return;
    }
    // Aynı bus'ın tekrar taranmasını engelle
    if visited[bus as usize] {
        return;
    }
    visited[bus as usize] = true;
    for device in 0u8..32 {
        let vendor_id = read_ecam_word(base, bus, device, 0, 0x00);
        if vendor_id == 0xFFFF {
            continue;
        }
        let header_type = read_ecam_byte(base, bus, device, 0, 0x0E);
        let function_count = if header_type & 0x80 != 0 { 8 } else { 1 };
        for function in 0u8..function_count {
            let vendor_id = read_ecam_word(base, bus, device, function, 0x00);
            if vendor_id == 0xFFFF {
                continue;
            }
            let dev = read_device_ecam(base, bus, device, function);
            crate::serial_println!(
                "PCI SCAN {:02x}:{:02x}.{} vendor={:04x} device={:04x} class={:02x} subclass={:02x} prog_if={:02x}",
                dev.bus,
                dev.device,
                dev.function,
                dev.vendor_id,
                dev.device_id,
                dev.class_code,
                dev.subclass,
                dev.prog_if
            );
            if dev.class_code == 0x01 {
                crate::serial_println!(
                    "PCI STORAGE FOUND: Bus/Dev/Fn {:02x}:{:02x}.{} ID={:04x}:{:04x}",
                    dev.bus,
                    dev.device,
                    dev.function,
                    dev.vendor_id,
                    dev.device_id
                );
            }
            let is_bridge =
                dev.class_code == PCI_CLASS_BRIDGE && dev.subclass == PCI_SUBCLASS_PCI_BRIDGE;
            devices.push(dev);
            if is_bridge {
                let secondary = read_ecam_byte(base, bus, device, function, 0x19);
                if secondary != 0 && secondary != 0xFF {
                    scan_bus_ecam_recursive(base, start_bus, end_bus, secondary, visited, devices);
                }
            }
        }
    }
}

/// ECAM yöntemiyle bir cihazın tüm tanımlayıcı alanlarını okur ve
/// `PciDevice` yapısı oluşturur.
fn read_device_ecam(base: u64, bus: u8, device: u8, function: u8) -> PciDevice {
    let vendor_id = read_ecam_word(base, bus, device, function, 0x00);
    let device_id = read_ecam_word(base, bus, device, function, 0x02);
    let prog_if = read_ecam_byte(base, bus, device, function, 0x09);
    let subclass = read_ecam_byte(base, bus, device, function, 0x0A);
    let class_code = read_ecam_byte(base, bus, device, function, 0x0B);
    let header_type = read_ecam_byte(base, bus, device, function, 0x0E);
    PciDevice {
        bus,
        device,
        function,
        vendor_id,
        device_id,
        class_code,
        subclass,
        prog_if,
        header_type,
    }
}

/// ECAM MMIO üzerinden 16-bit konfigürasyon verisi okur.
fn read_ecam_word(base: u64, bus: u8, device: u8, function: u8, offset: u16) -> u16 {
    let value = read_ecam_dword(base, bus, device, function, offset);
    let shift = ((offset & 2) * 8) as u32;
    ((value >> shift) & 0xFFFF) as u16
}

/// ECAM MMIO üzerinden 8-bit konfigürasyon verisi okur.
fn read_ecam_byte(base: u64, bus: u8, device: u8, function: u8, offset: u16) -> u8 {
    let value = read_ecam_dword(base, bus, device, function, offset);
    let shift = ((offset & 3) * 8) as u32;
    ((value >> shift) & 0xFF) as u8
}

/// ECAM MMIO üzerinden 32-bit konfigürasyon dword'ü okur.
/// Adres formülü: taban + (Bus<<20) + (Dev<<15) + (Fn<<12) + (Offset & 0xFFC)
fn read_ecam_dword(base: u64, bus: u8, device: u8, function: u8, offset: u16) -> u32 {
    let address = base
        + ((bus as u64) << 20)
        + ((device as u64) << 15)
        + ((function as u64) << 12)
        + ((offset as u64) & 0xFFC);
    unsafe { read_volatile(address as *const u32) }
}

// ============================================================================
// PCIe AER (Advanced Error Reporting)
// ============================================================================

/// PCIe AER Extended Capability ID
const PCI_EXT_CAP_ID_AER: u16 = 0x0001;

/// AER Uncorrectable Error Status Register offset (relative to AER capability)
const AER_UNCORR_STATUS: u16 = 0x04;
/// AER Uncorrectable Error Mask Register offset
const AER_UNCORR_MASK: u16 = 0x08;
/// AER Uncorrectable Error Severity Register offset
const AER_UNCORR_SEVERITY: u16 = 0x0C;
/// AER Correctable Error Status Register offset
const AER_CORR_STATUS: u16 = 0x10;
/// AER Correctable Error Mask Register offset
const AER_CORR_MASK: u16 = 0x14;
/// AER Advanced Error Capabilities and Control Register offset
const AER_CAP_CONTROL: u16 = 0x18;
/// AER Header Log [0-3] offsets
const AER_HEADER_LOG: u16 = 0x1C;
/// AER TLP Prefix Log offsets
const AER_TLP_PREFIX_LOG: u16 = 0x38;

/// AER Uncorrectable Error bits
pub const AER_UNCORR_TRAINING_ERROR: u32 = 1 << 0;
pub const AER_UNCORR_DLLP_ERROR: u32 = 1 << 4;
pub const AER_UNCORR_SURPRISE_DOWN: u32 = 1 << 5;
pub const AER_UNCORR_POISONED_TLP: u32 = 1 << 12;
pub const AER_UNCORR_FC_PROTOCOL: u32 = 1 << 13;
pub const AER_UNCORR_COMPLETION_TIMEOUT: u32 = 1 << 14;
pub const AER_UNCORR_COMPLETER_ABORT: u32 = 1 << 15;
pub const AER_UNCORR_UNEXPECTED_COMPLETION: u32 = 1 << 16;
pub const AER_UNCORR_RECEIVER_OVERFLOW: u32 = 1 << 17;
pub const AER_UNCORR_MALFORMED_TLP: u32 = 1 << 18;
pub const AER_UNCORR_ECRC_ERROR: u32 = 1 << 19;
pub const AER_UNCORR_UNSUPPORTED_REQUEST: u32 = 1 << 20;
pub const AER_UNCORR_ACS_VIOLATION: u32 = 1 << 21;
pub const AER_UNCORR_INTERNAL_ERROR: u32 = 1 << 22;
pub const AER_UNCORR_MC_BLOCKED_TLP: u32 = 1 << 23;
pub const AER_UNCORR_ATOMIC_EGRESS_BLOCKED: u32 = 1 << 24;
pub const AER_UNCORR_TLP_PREFIX_BLOCKED: u32 = 1 << 25;

/// AER Correctable Error bits
pub const AER_CORR_RECEIVER_ERROR: u32 = 1 << 0;
pub const AER_CORR_REPLAY_ROLLOVER: u32 = 1 << 8;
pub const AER_CORR_REPLAY_TIMER_TIMEOUT: u32 = 1 << 12;
pub const AER_CORR_ADVISORY_NONFATAL: u32 = 1 << 13;
pub const AER_CORR_INTERNAL_ERROR: u32 = 1 << 14;
pub const AER_CORR_HEADER_LOG_OVFLOW: u32 = 1 << 15;

/// AER error information structure
#[derive(Debug, Clone)]
pub struct AerErrorInfo {
    /// Device identification
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    /// Uncorrectable error status
    pub uncorr_status: u32,
    /// Correctable error status
    pub corr_status: u32,
    /// Error severity (from uncorr_severity register interpretation)
    pub is_fatal: bool,
    /// Header log (first TLP header that caused error)
    pub header_log: [u32; 4],
}

impl AerErrorInfo {
    /// Check if there are any uncorrectable errors
    pub fn has_uncorrectable(&self) -> bool {
        self.uncorr_status != 0
    }

    /// Check if there are any correctable errors
    pub fn has_correctable(&self) -> bool {
        self.corr_status != 0
    }

    /// Get human-readable error description
    pub fn describe_uncorr_error(&self) -> alloc::string::String {
        let mut desc = alloc::string::String::new();
        if self.uncorr_status & AER_UNCORR_TRAINING_ERROR != 0 {
            desc.push_str("Training Error, ");
        }
        if self.uncorr_status & AER_UNCORR_DLLP_ERROR != 0 {
            desc.push_str("DLLP Error, ");
        }
        if self.uncorr_status & AER_UNCORR_SURPRISE_DOWN != 0 {
            desc.push_str("Surprise Down, ");
        }
        if self.uncorr_status & AER_UNCORR_POISONED_TLP != 0 {
            desc.push_str("Poisoned TLP, ");
        }
        if self.uncorr_status & AER_UNCORR_FC_PROTOCOL != 0 {
            desc.push_str("FC Protocol Error, ");
        }
        if self.uncorr_status & AER_UNCORR_COMPLETION_TIMEOUT != 0 {
            desc.push_str("Completion Timeout, ");
        }
        if self.uncorr_status & AER_UNCORR_COMPLETER_ABORT != 0 {
            desc.push_str("Completer Abort, ");
        }
        if self.uncorr_status & AER_UNCORR_UNEXPECTED_COMPLETION != 0 {
            desc.push_str("Unexpected Completion, ");
        }
        if self.uncorr_status & AER_UNCORR_MALFORMED_TLP != 0 {
            desc.push_str("Malformed TLP, ");
        }
        if self.uncorr_status & AER_UNCORR_ECRC_ERROR != 0 {
            desc.push_str("ECRC Error, ");
        }
        if self.uncorr_status & AER_UNCORR_UNSUPPORTED_REQUEST != 0 {
            desc.push_str("Unsupported Request, ");
        }
        if self.uncorr_status & AER_UNCORR_INTERNAL_ERROR != 0 {
            desc.push_str("Internal Error, ");
        }
        desc
    }

    /// Get human-readable correctable error description
    pub fn describe_corr_error(&self) -> alloc::string::String {
        let mut desc = alloc::string::String::new();
        if self.corr_status & AER_CORR_RECEIVER_ERROR != 0 {
            desc.push_str("Receiver Error, ");
        }
        if self.corr_status & AER_CORR_REPLAY_ROLLOVER != 0 {
            desc.push_str("Replay Rollover, ");
        }
        if self.corr_status & AER_CORR_REPLAY_TIMER_TIMEOUT != 0 {
            desc.push_str("Replay Timer Timeout, ");
        }
        if self.corr_status & AER_CORR_ADVISORY_NONFATAL != 0 {
            desc.push_str("Advisory Non-Fatal, ");
        }
        if self.corr_status & AER_CORR_INTERNAL_ERROR != 0 {
            desc.push_str("Internal Error, ");
        }
        desc
    }
}

/// Find PCIe Extended Capability offset
/// PCIe extended capabilities start at offset 0x100 and use a linked list
pub fn find_ext_capability(bus: u8, device: u8, function: u8, cap_id: u16) -> Option<u16> {
    let mut offset = PCI_EXT_CAP_START;
    let mut guard = 0usize;
    let mut seen = [false; PCI_EXT_CAP_MAX_STEPS];
    while valid_ext_cap_offset(offset) && guard < PCI_EXT_CAP_MAX_STEPS {
        let slot = (offset >> 2) as usize;
        if seen[slot] {
            break;
        }
        seen[slot] = true;
        let header = read_config_dword(bus, device, function, offset);
        if header == 0 || header == 0xFFFF_FFFF {
            return None;
        }
        let curr_cap_id = (header & 0xFFFF) as u16;
        if curr_cap_id == cap_id {
            return Some(offset);
        }
        offset = decode_ext_cap_next(header);
        if offset == 0 {
            break;
        }
        guard += 1;
    }
    None
}

/// Check if device supports AER (Advanced Error Reporting)
pub fn has_aer(bus: u8, device: u8, function: u8) -> bool {
    find_ext_capability(bus, device, function, PCI_EXT_CAP_ID_AER).is_some()
}

/// Read AER capability registers
pub fn read_aer_info(bus: u8, device: u8, function: u8) -> Option<AerErrorInfo> {
    let aer_offset = find_ext_capability(bus, device, function, PCI_EXT_CAP_ID_AER)?;

    let uncorr_status = read_config_dword(bus, device, function, aer_offset + AER_UNCORR_STATUS);
    let corr_status = read_config_dword(bus, device, function, aer_offset + AER_CORR_STATUS);
    let uncorr_severity =
        read_config_dword(bus, device, function, aer_offset + AER_UNCORR_SEVERITY);

    // Determine if error is fatal based on severity register
    let is_fatal = (uncorr_status & uncorr_severity) != 0;

    // Read header log for the first TLP that caused the error
    let header_log = [
        read_config_dword(bus, device, function, aer_offset + AER_HEADER_LOG),
        read_config_dword(bus, device, function, aer_offset + AER_HEADER_LOG + 4),
        read_config_dword(bus, device, function, aer_offset + AER_HEADER_LOG + 8),
        read_config_dword(bus, device, function, aer_offset + AER_HEADER_LOG + 12),
    ];

    Some(AerErrorInfo {
        bus,
        device,
        function,
        uncorr_status,
        corr_status,
        is_fatal,
        header_log,
    })
}

/// Clear AER error status registers
pub fn clear_aer_status(bus: u8, device: u8, function: u8) -> bool {
    let aer_offset = match find_ext_capability(bus, device, function, PCI_EXT_CAP_ID_AER) {
        Some(off) => off,
        None => return false,
    };

    // Clear uncorrectable errors by writing 1s to status bits
    let uncorr_status = read_config_dword(bus, device, function, aer_offset + AER_UNCORR_STATUS);
    write_config_dword(
        bus,
        device,
        function,
        aer_offset + AER_UNCORR_STATUS,
        uncorr_status,
    );

    // Clear correctable errors by writing 1s to status bits
    let corr_status = read_config_dword(bus, device, function, aer_offset + AER_CORR_STATUS);
    write_config_dword(
        bus,
        device,
        function,
        aer_offset + AER_CORR_STATUS,
        corr_status,
    );

    crate::serial_println!(
        "[AER] Cleared errors for {:02x}:{:02x}.{} (UC={:#x}, C={:#x})",
        bus,
        device,
        function,
        uncorr_status,
        corr_status
    );

    true
}

/// Enable AER for a device
pub fn enable_aer(bus: u8, device: u8, function: u8) -> bool {
    let aer_offset = match find_ext_capability(bus, device, function, PCI_EXT_CAP_ID_AER) {
        Some(off) => off,
        None => return false,
    };

    // Clear any existing errors first
    clear_aer_status(bus, device, function);

    // Enable ECRC checking and generation (optional but recommended)
    let cap_control = read_config_dword(bus, device, function, aer_offset + AER_CAP_CONTROL);
    let new_control = cap_control | (1 << 6) | (1 << 7); // Enable ECRC check and generation
    write_config_dword(
        bus,
        device,
        function,
        aer_offset + AER_CAP_CONTROL,
        new_control,
    );

    // Unmask correctable errors (enable reporting)
    let corr_mask = read_config_dword(bus, device, function, aer_offset + AER_CORR_MASK);
    // Only unmask common non-critical errors
    let new_corr_mask = corr_mask & !(AER_CORR_RECEIVER_ERROR | AER_CORR_REPLAY_ROLLOVER);
    write_config_dword(
        bus,
        device,
        function,
        aer_offset + AER_CORR_MASK,
        new_corr_mask,
    );

    crate::serial_println!("[AER] Enabled for {:02x}:{:02x}.{}", bus, device, function);

    true
}

/// Scan all devices for AER errors and report them
pub fn scan_aer_errors() -> Vec<AerErrorInfo> {
    let devices = scan();
    let mut errors = Vec::new();

    for dev in devices {
        if let Some(mut info) = read_aer_info(dev.bus, dev.device, dev.function) {
            if info.has_uncorrectable() || info.has_correctable() {
                crate::serial_println!(
                    "[AER] Error detected at {:02x}:{:02x}.{}: UC={:#x} C={:#x}",
                    dev.bus,
                    dev.device,
                    dev.function,
                    info.uncorr_status,
                    info.corr_status
                );
                if info.has_uncorrectable() {
                    crate::serial_println!("  Uncorrectable: {}", info.describe_uncorr_error());
                }
                if info.has_correctable() {
                    crate::serial_println!("  Correctable: {}", info.describe_corr_error());
                }
                errors.push(info);
            }
        }
    }

    errors
}

/// Initialize AER for all PCIe devices
pub fn init_aer() {
    let devices = scan();
    let mut aer_count = 0;

    for dev in devices {
        if has_aer(dev.bus, dev.device, dev.function) {
            enable_aer(dev.bus, dev.device, dev.function);
            aer_count += 1;
        }
    }

    crate::serial_println!("[PCI] AER initialized for {} devices", aer_count);
}

#[cfg(test)]
mod tests {
    use super::{
        decode_ext_cap_next, msix_table_window_fits, phase5_kickoff_contract_green,
        sanitize_cap_ptr, valid_cap_ptr, valid_ext_cap_offset,
    };

    #[test]
    fn msix_table_window_fits_accepts_exact_bar_boundary() {
        assert!(msix_table_window_fits(0x1000, 0x0F00, 0, 16));
    }

    #[test]
    fn msix_table_window_fits_rejects_overflowing_window() {
        assert!(!msix_table_window_fits(0x1000, 0x0FF0, 2, 2));
    }

    #[test]
    fn msix_table_window_fits_rejects_zero_entries() {
        assert!(!msix_table_window_fits(0x1000, 0, 0, 0));
    }

    #[test]
    fn sanitize_cap_ptr_masks_low_two_bits() {
        assert_eq!(sanitize_cap_ptr(0x47), 0x44);
        assert_eq!(sanitize_cap_ptr(0xFC), 0xFC);
        assert_eq!(sanitize_cap_ptr(0x03), 0x00);
    }

    #[test]
    fn valid_cap_ptr_checks_range_and_alignment() {
        assert!(!valid_cap_ptr(0x00));
        assert!(!valid_cap_ptr(0x3C));
        assert!(valid_cap_ptr(0x40));
        assert!(valid_cap_ptr(0xFC));
        assert!(!valid_cap_ptr(0xFD));
    }

    #[test]
    fn decode_ext_cap_next_extracts_and_aligns() {
        let header = (0x123u32 << 20) | 0x0001;
        assert_eq!(decode_ext_cap_next(header), 0x120);
        let end_header = 0x0000_0001;
        assert_eq!(decode_ext_cap_next(end_header), 0);
    }

    #[test]
    fn valid_ext_cap_offset_enforces_pcie_window() {
        assert!(!valid_ext_cap_offset(0x0FC));
        assert!(valid_ext_cap_offset(0x100));
        assert!(valid_ext_cap_offset(0xFFC));
        assert!(!valid_ext_cap_offset(0xFFE));
    }

    #[test]
    fn phase5_kickoff_contract_is_green() {
        assert!(phase5_kickoff_contract_green());
    }
}
