//! # echOS PE/COFF Yükleyici
//!
//! Windows Portable Executable (PE) formatındaki ikili dosyaları yükler.
//! PE32+ (64-bit) çalıştırılabilir dosyaları ve DLL'leri destekler.
//!
//! ## PE Dosya Yapısı
//! Bir PE dosyası şu bölümlerden oluşur:
//! - DOS Header (MZ başlığı) — Geriye dönük uyumluluk için 16-bit DOS stub
//! - PE Signature ("PE\0\0") — PE dosya imzası
//! - File Header (COFF başlığı) — Makine türü, bölüm sayısı, özellikler
//! - Optional Header — Giriş noktası, image base, bölüm hizalamaları, veri dizinleri
//! - Section Headers — .text (kod), .data, .rdata, .bss vb.
//! - Sections — Gerçek ikili veri

use alloc::vec::Vec;
use alloc::vec;
use alloc::string::String;
use alloc::string::ToString;
use alloc::collections::BTreeMap;
use alloc::boxed::Box;
use alloc::sync::Arc;
use spin::Mutex;
use core::mem::size_of;

// ============================================================================
// PE SABİTLERİ
// ============================================================================

/// DOS başlık sihirli sayısı ("MZ") — Mark Zbikowski'nin baş harflerinden
const DOS_MAGIC: u16 = 0x5A4D;

/// PE imzası ("PE\0\0") — tüm PE dosyalarının tanımlayıcısı
const PE_SIGNATURE: u32 = 0x00004550;

/// PE32+ (64-bit) isteğe bağlı başlık sihiri
const PE32_PLUS_MAGIC: u16 = 0x20B;

/// PE32 (32-bit) isteğe bağlı başlık sihiri
const PE32_MAGIC: u16 = 0x10B;

// Görüntü özellikleri (IMAGE_FILE_CHARACTERISTICS)
const IMAGE_FILE_EXECUTABLE_IMAGE: u16 = 0x0002; // Çalıştırılabilir dosya
const IMAGE_FILE_LARGE_ADDRESS_AWARE: u16 = 0x0020; // 2GB üzeri adres kullanabilir
const IMAGE_FILE_32BIT_MACHINE: u16 = 0x0100; // 32-bit makine
const IMAGE_FILE_DLL: u16 = 0x2000;           // DLL (dinamik bağlantı kütüphanesi)

// Bölüm özellikleri (IMAGE_SCN_*)
const IMAGE_SCN_CNT_CODE: u32 = 0x00000020;              // Kod içerir
const IMAGE_SCN_CNT_INITIALIZED_DATA: u32 = 0x00000040;  // İlklendirilmiş veri
const IMAGE_SCN_CNT_UNINITIALIZED_DATA: u32 = 0x00000080; // İlklendirilmemiş veri (BSS)
const IMAGE_SCN_MEM_EXECUTE: u32 = 0x20000000;           // Çalıştırılabilir bellek
const IMAGE_SCN_MEM_READ: u32 = 0x40000000;              // Okunabilir bellek
const IMAGE_SCN_MEM_WRITE: u32 = 0x80000000;             // Yazılabilir bellek

// Veri dizini giriş indeksleri (IMAGE_DIRECTORY_ENTRY_*)
const IMAGE_DIRECTORY_ENTRY_EXPORT: usize = 0;   // Dışa aktarma tablosu
const IMAGE_DIRECTORY_ENTRY_IMPORT: usize = 1;   // İçe aktarma tablosu
const IMAGE_DIRECTORY_ENTRY_RESOURCE: usize = 2; // Kaynak tablosu
const IMAGE_DIRECTORY_ENTRY_EXCEPTION: usize = 3; // İstisna tablosu
const IMAGE_DIRECTORY_ENTRY_SECURITY: usize = 4; // Güvenlik sertifikaları
const IMAGE_DIRECTORY_ENTRY_BASERELOC: usize = 5; // Yer değiştirme tablosu
const IMAGE_DIRECTORY_ENTRY_DEBUG: usize = 6;    // Hata ayıklama bilgisi
const IMAGE_DIRECTORY_ENTRY_TLS: usize = 9;      // Thread yerel depolama
const IMAGE_DIRECTORY_ENTRY_IAT: usize = 12;     // İçe aktarma adresi tablosu

// ============================================================================
// PE HATA TİPİ
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeError {
    InvalidDosHeader,      // Geçersiz DOS başlığı (MZ sihiri yanlış)
    InvalidPeSignature,    // Geçersiz PE imzası
    InvalidMachine,        // Desteklenmeyen makine mimarisi
    InvalidOptionalHeader, // Geçersiz isteğe bağlı başlık
    InvalidSection,        // Geçersiz bölüm başlığı
    NotPe64,               // 64-bit PE değil
    NotExecutable,         // Çalıştırılabilir değil
    ImportNotFound,        // İçe aktarılan işlev bulunamadı
    RelocationFailed,      // Yer değiştirme başarısız
    MemoryAllocation,      // Bellek tahsisi hatası
    EntryNotFound,         // Giriş noktası bulunamadı
    DllNotFound,           // DLL bulunamadı
    SymbolNotFound,        // Sembol bulunamadı
    InvalidExport,         // Geçersiz dışa aktarma girişi
}

// ============================================================================
// DOS BAŞLIĞI
// ============================================================================

/// DOS Başlığı (64 bayt) — Her PE dosyasının başında bulunur
/// e_lfanew alanı gerçek PE başlığına olan ofseti içerir
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct ImageDosHeader {
    pub e_magic: u16,           // 0x00: Sihirli sayı (MZ = 0x5A4D)
    pub e_cblp: u16,            // 0x02: Son sayfadaki bayt sayısı
    pub e_cp: u16,              // 0x04: Dosyadaki sayfa sayısı
    pub e_crlc: u16,            // 0x06: Yer değiştirme sayısı
    pub e_cparhdr: u16,         // 0x08: Paragraf cinsinden başlık boyutu
    pub e_minalloc: u16,        // 0x0A: Minimum ekstra paragraf
    pub e_maxalloc: u16,        // 0x0C: Maksimum ekstra paragraf
    pub e_ss: u16,              // 0x0E: Başlangıç SS (stack segment) değeri
    pub e_sp: u16,              // 0x10: Başlangıç SP (stack pointer) değeri
    pub e_csum: u16,            // 0x12: Sağlama toplamı
    pub e_ip: u16,              // 0x14: Başlangıç IP (instruction pointer) değeri
    pub e_cs: u16,              // 0x16: Başlangıç CS (code segment) değeri
    pub e_lfarlc: u16,          // 0x18: Yer değiştirme tablosunun dosya adresi
    pub e_ovno: u16,            // 0x1A: Katman (overlay) numarası
    pub e_res: [u16; 4],        // 0x1C: Ayrılmış
    pub e_oemid: u16,           // 0x24: OEM tanımlayıcısı
    pub e_oeminfo: u16,         // 0x26: OEM bilgisi
    pub e_res2: [u16; 10],      // 0x28: Ayrılmış
    pub e_lfanew: u32,          // 0x3C: Yeni EXE başlığının dosya adresi (PE'ye işaret eder)
}

// ============================================================================
// PE DOSYA BAŞLIĞI
// ============================================================================

/// PE Dosya Başlığı (20 bayt) — COFF formatından miras
/// PE imzasından hemen sonra gelir
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct ImageFileHeader {
    pub machine: u16,           // 0x00: Makine türü (AMD64 = 0x8664)
    pub number_of_sections: u16, // 0x02: Bölüm sayısı
    pub time_date_stamp: u32,   // 0x04: Derleme zaman damgası (Unix epoch)
    pub pointer_to_symbol_table: u32, // 0x08: Sembol tablosu işaretçisi (genelde sıfır)
    pub number_of_symbols: u32, // 0x0C: Sembol sayısı
    pub size_of_optional_header: u16, // 0x10: İsteğe bağlı başlık boyutu
    pub characteristics: u16,   // 0x12: Dosya özellikleri bayrakları
}

/// Makine türleri — hangi CPU mimarisi için derlendiğini belirtir
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MachineType {
    Unknown = 0x0000, // Bilinmeyen mimari
    I386 = 0x014C,    // 32-bit x86
    AMD64 = 0x8664,   // 64-bit x86-64 (amd64/x86_64)
    ARM = 0x01C0,     // 32-bit ARM
    ARM64 = 0xAA64,   // 64-bit ARM (AArch64)
}

impl MachineType {
    pub fn from_u16(value: u16) -> Self {
        match value {
            0x014C => MachineType::I386,
            0x8664 => MachineType::AMD64,
            0x01C0 => MachineType::ARM,
            0xAA64 => MachineType::ARM64,
            _ => MachineType::Unknown,
        }
    }
}

// ============================================================================
// PE İSTEĞE BAĞLI BAŞLIĞI (PE32+)
// ============================================================================

/// PE32+ İsteğe Bağlı Başlık (240 bayt) — 64-bit PE dosyaları için
/// Giriş noktası, image base, bölüm hizalamaları ve 16 veri dizinini içerir
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct ImageOptionalHeader64 {
    pub magic: u16,                     // 0x00: Sihir (PE32+ için 0x20B)
    pub major_linker_version: u8,       // 0x02: Bağlayıcı ana sürümü
    pub minor_linker_version: u8,       // 0x03: Bağlayıcı alt sürümü
    pub size_of_code: u32,              // 0x04: Kod bölümünün boyutu
    pub size_of_initialized_data: u32,  // 0x08: İlklendirilmiş verinin boyutu
    pub size_of_uninitialized_data: u32, // 0x0C: İlklendirilmemiş verinin boyutu
    pub address_of_entry_point: u32,    // 0x10: Giriş noktasının RVA'sı (Relative Virtual Address)
    pub base_of_code: u32,              // 0x14: Kod bölümünün RVA'sı
    pub image_base: u64,                // 0x18: Tercih edilen yükleme adresi (64-bit)
    pub section_alignment: u32,         // 0x20: Bellekteki bölüm hizalaması (genelde 4096)
    pub file_alignment: u32,            // 0x24: Dosyadaki veri hizalaması (genelde 512)
    pub major_operating_system_version: u16, // 0x28: Minimum işletim sistemi ana sürümü
    pub minor_operating_system_version: u16, // 0x2A: Minimum işletim sistemi alt sürümü
    pub major_image_version: u16,       // 0x2C: Görüntü ana sürümü
    pub minor_image_version: u16,       // 0x2E: Görüntü alt sürümü
    pub major_subsystem_version: u16,   // 0x30: Alt sistem ana sürümü
    pub minor_subsystem_version: u16,   // 0x32: Alt sistem alt sürümü
    pub win32_version_value: u32,       // 0x34: Win32 sürüm değeri (rezerve, sıfır olmalı)
    pub size_of_image: u32,             // 0x38: Belleğe yüklenen görüntünün toplam boyutu
    pub size_of_headers: u32,           // 0x3C: Tüm başlıkların toplam boyutu
    pub check_sum: u32,                 // 0x40: Dosya sağlama toplamı
    pub subsystem: u16,                 // 0x44: Alt sistem türü (GUI, konsol vb.)
    pub dll_characteristics: u16,       // 0x46: DLL özellikleri (ASLR, NX vb.)
    pub size_of_stack_reserve: u64,     // 0x48: Yığın için ayrılan sanal bellek
    pub size_of_stack_commit: u64,      // 0x50: Yığın için taahhüt edilen fiziksel bellek
    pub size_of_heap_reserve: u64,      // 0x58: Heap için ayrılan sanal bellek
    pub size_of_heap_commit: u64,       // 0x60: Heap için taahhüt edilen fiziksel bellek
    pub loader_flags: u32,              // 0x68: Yükleyici bayrakları (rezerve)
    pub number_of_rva_and_sizes: u32,   // 0x6C: Veri dizini giriş sayısı (genelde 16)
    // Veri dizinleri buradan sonra gelir (16 giriş × 8 bayt = 128 bayt)
}

/// Veri Dizini Girişi — RVA (göreli sanal adres) ve boyut çifti
/// Her veri dizini (import, export, reloc...) bu yapıyla tanımlanır
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ImageDataDirectory {
    pub virtual_address: u32, // Dizinin başlangıç RVA'sı
    pub size: u32,             // Dizinin bayt cinsinden boyutu
}

// ============================================================================
// BÖLÜM BAŞLIĞI
// ============================================================================

/// Bölüm Başlığı (40 bayt) — Her bölümü (.text, .data, .rdata vb.) tanımlar
/// Bölümün sanal adresini, ham veri ofsetini ve özelliklerini içerir
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct ImageSectionHeader {
    pub name: [u8; 8],           // 0x00: Bölüm adı (null sonlanmalı, 8 karakter max)
    pub virtual_size: u32,       // 0x08: Bellekteki sanal boyut
    pub virtual_address: u32,    // 0x0C: Sanal adres (RVA)
    pub size_of_raw_data: u32,   // 0x10: Dosyadaki ham veri boyutu
    pub pointer_to_raw_data: u32, // 0x14: Dosyada ham verinin başlangıcı
    pub pointer_to_relocations: u32, // 0x18: Yer değiştirme girişlerinin işaretçisi
    pub pointer_to_linenumbers: u32, // 0x1C: Satır numarası bilgileri işaretçisi
    pub number_of_relocations: u16, // 0x20: Yer değiştirme sayısı
    pub number_of_linenumbers: u16, // 0x22: Satır numarası sayısı
    pub characteristics: u32,    // 0x24: Bölüm özellikleri (okuma/yazma/çalıştırma)
}

impl ImageSectionHeader {
    pub fn name_as_string(&self) -> String {
        let mut name = String::new();
        for &b in &self.name {
            if b == 0 {
                break;
            }
            name.push(b as char);
        }
        name
    }
}

// ============================================================================
// İÇE AKTARMA TABLOSU
// ============================================================================

/// İçe Aktarma Dizini Girişi — Her DLL için bir tane
/// DLL adını ve içe aktarılan işlevlerin listesini tanımlar
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct ImageImportDescriptor {
    pub original_first_thunk: u32,  // 0x00: Orijinal ilk dönüştürücü (RVA) — isim/ordinal listesi
    pub time_date_stamp: u32,       // 0x04: Bağlanma zaman damgası
    pub forwarder_chain: u32,       // 0x08: İletici (forwarder) zinciri
    pub name: u32,                  // 0x0C: DLL adı RVA'sı
    pub first_thunk: u32,           // 0x10: İlk dönüştürücü (RVA) — IAT girişleri
}

/// İçe Aktarma Arama (64-bit) — IAT/INT girişi
/// En yüksek bit ordinal mı yoksa isimle mi içe aktarıldığını belirtir
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct ImageThunkData64 {
    pub ordinal_or_address: u64, // Bit 63=1: ordinal, Bit 63=0: isim RVA'sı
}

impl ImageThunkData64 {
    pub fn is_ordinal(&self) -> bool {
        (self.ordinal_or_address & (1 << 63)) != 0
    }

    pub fn ordinal(&self) -> u16 {
        (self.ordinal_or_address & 0xFFFF) as u16
    }

    pub fn hint_name_rva(&self) -> u32 {
        (self.ordinal_or_address & 0x7FFFFFFF) as u32
    }
}

/// İçe aktarma İpucu/İsim girişi — işlev adı ve hint numarası
#[repr(C, packed)]
pub struct ImageImportHintName {
    pub hint: u16,
    // Arkasından null ile sonlanan işlev adı gelir
}

// ============================================================================
// DIŞA AKTARMA TABLOSU
// ============================================================================

/// Dışa Aktarma Dizini Tablosu — DLL'nin dışarıya sunduğu işlevleri tanımlar
/// İşlev adresleri, isimleri ve ordinal numaraları içerir
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct ImageExportDirectory {
    pub characteristics: u32,       // 0x00: Özellikler (rezerve)
    pub time_date_stamp: u32,      // 0x04: Derleme zaman damgası
    pub major_version: u16,        // 0x08: Ana sürüm
    pub minor_version: u16,        // 0x0A: Alt sürüm
    pub name: u32,                 // 0x0C: DLL adı RVA'sı
    pub base: u32,                 // 0x10: İlk ordinal numarası
    pub number_of_functions: u32,  // 0x14: İşlev sayısı (AddressOfFunctions boyutu)
    pub number_of_names: u32,      // 0x18: İsimle dışa aktarılan işlev sayısı
    pub address_of_functions: u32, // 0x1C: İşlev adres dizisi RVA'sı
    pub address_of_names: u32,     // 0x20: İsim işaretçi dizisi RVA'sı
    pub address_of_name_ordinals: u32, // 0x24: Ordinal dizisi RVA'sı
}

// ============================================================================
// TEMEL YER DEĞİŞTİRME (Base Relocation)
// ============================================================================

/// Temel Yer Değiştirme Bloğu — görüntü farklı adrese yüklenirse düzeltme
/// Her blok bir sayfa (4KB) için yer değiştirme girişlerini gruplar
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct ImageBaseRelocation {
    pub virtual_address: u32,  // 0x00: Sayfa RVA'sı
    pub size_of_block: u32,    // 0x04: Bu bloğun toplam boyutu (başlık dahil)
}

/// Yer değiştirme türleri (her girişin üst 4 bitinde saklanır)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelocationType {
    Absolute = 0,  // Doldurma; hiçbir işlem yapılmaz
    High = 1,      // Üst 16-bit yer değiştirme
    Low = 2,       // Alt 16-bit yer değiştirme
    HighLow = 3,   // Adresin tamamı (32-bit)
    Dir64 = 10,    // 64-bit mutlak adres (PE32+ için)
}

// ============================================================================
// PE GÖRÜNTÜSÜ (Yüklenmiş Temsil)
// ============================================================================

/// Yüklenmiş PE Görüntüsü — ayrıştırılmış ve belleğe hazırlanmış PE dosyası
#[derive(Clone, Debug)]
pub struct PeImage {
    /// Görüntünün yüklendiği temel adres
    pub image_base: u64,
    /// Giriş noktasının mutlak adresi
    pub entry_point: u64,
    /// Görüntünün bayt cinsinden boyutu
    pub image_size: u32,
    /// Yüklenmiş bölümler (.text, .data vb.)
    pub sections: Vec<PeSection>,
    /// İçe aktarma girişleri (DLL bağımlılıkları)
    pub imports: Vec<ImportEntry>,
    /// Dışa aktarma tablosu (isim → adres eşleşmesi)
    pub exports: BTreeMap<String, u64>,
    /// DLL mi yoksa EXE mi
    pub is_dll: bool,
    /// Hedef makine mimarisi
    pub machine: MachineType,
}

/// Yüklenmiş bölüm — ham veriyi ve erişim özelliklerini içerir
#[derive(Clone, Debug)]
pub struct PeSection {
    pub name: String,            // Bölüm adı (.text, .data vb.)
    pub virtual_address: u32,   // Bellekteki RVA
    pub virtual_size: u32,      // Bellekteki boyut
    pub raw_data: Vec<u8>,      // Ham ikili veri
    pub characteristics: u32,   // Ham özellik bayrakları
    pub is_code: bool,           // Kod bölümü mü?
    pub is_data: bool,           // Veri bölümü mü?
    pub is_readable: bool,       // Okunabilir mi?
    pub is_writable: bool,       // Yazılabilir mi?
    pub is_executable: bool,     // Çalıştırılabilir mi?
}

/// İçe aktarma girişi — tek bir DLL'den içe aktarılan işlevler
#[derive(Clone, Debug)]
pub struct ImportEntry {
    pub dll_name: String,            // Kaynak DLL adı
    pub functions: Vec<ImportFunction>, // İçe aktarılan işlevler
}

/// İçe aktarılan işlev — ad, ordinal ve çözünürlük bilgisi
#[derive(Clone, Debug)]
pub struct ImportFunction {
    pub name: String,                    // İşlev adı
    pub ordinal: Option<u16>,           // Ordinal numarası (varsa)
    pub thunk_address: u64,             // IAT girişinin adresi
    pub resolved_address: Option<u64>,  // Çözümlenmiş gerçek adres
}

// ============================================================================
// PE YÜKLEYİCİSİ
// ============================================================================

pub struct PeLoader {
    /// Yüklenmiş DLL'lerin önbelleği — aynı DLL birden fazla kez yüklenmez
    loaded_dlls: BTreeMap<String, Arc<Mutex<PeImage>>>,
}

impl PeLoader {
    pub fn new() -> Self {
        PeLoader {
            loaded_dlls: BTreeMap::new(),
        }
    }

    /// Ham baytları PE olarak yükle — DOS başlığından bölüm verilerine kadar tümünü ayrıştırır
    pub fn load(&mut self, data: &[u8]) -> Result<PeImage, PeError> {
        // DOS başlığını ayrıştır — MZ sihirini doğrula
        if data.len() < size_of::<ImageDosHeader>() {
            return Err(PeError::InvalidDosHeader);
        }

        let dos_header = unsafe { &*(data.as_ptr() as *const ImageDosHeader) };

        if dos_header.e_magic != DOS_MAGIC {
            return Err(PeError::InvalidDosHeader);
        }

        // e_lfanew'dan PE başlığının ofsetini al
        let pe_offset = dos_header.e_lfanew as usize;
        if pe_offset + 4 > data.len() {
            return Err(PeError::InvalidPeSignature);
        }

        // PE imzasını doğrula ("PE\0\0")
        let pe_sig = read_u32(&data[pe_offset..]);
        if pe_sig != PE_SIGNATURE {
            return Err(PeError::InvalidPeSignature);
        }

        // Dosya başlığını ayrıştır — PE imzasından 4 bayt sonra gelir
        let file_header_offset = pe_offset + 4;
        if file_header_offset + size_of::<ImageFileHeader>() > data.len() {
            return Err(PeError::InvalidPeSignature);
        }

        let file_header = unsafe {
            &*(data.as_ptr().add(file_header_offset) as *const ImageFileHeader)
        };

        // Makine türünü kontrol et — sadece AMD64 (x86-64) desteklenir
        let machine = MachineType::from_u16(file_header.machine);
        if machine != MachineType::AMD64 {
            return Err(PeError::NotPe64);
        }

        // DLL mi kontrol et
        let is_dll = (file_header.characteristics & IMAGE_FILE_DLL) != 0;

        // İsteğe bağlı başlığı ayrıştır — dosya başlığından hemen sonra gelir
        let optional_offset = file_header_offset + size_of::<ImageFileHeader>();
        if optional_offset + size_of::<ImageOptionalHeader64>() > data.len() {
            return Err(PeError::InvalidOptionalHeader);
        }

        let optional_header = unsafe {
            &*(data.as_ptr().add(optional_offset) as *const ImageOptionalHeader64)
        };

        // Sihiri doğrula — PE32+ olmalı (0x20B)
        if optional_header.magic != PE32_PLUS_MAGIC {
            return Err(PeError::NotPe64);
        }

        // Bölümleri ayrıştır — isteğe bağlı başlık boyutu kadar ilerle
        let section_offset = optional_offset + file_header.size_of_optional_header as usize;
        let num_sections = file_header.number_of_sections as usize;
        let mut sections = Vec::with_capacity(num_sections);

        for i in 0..num_sections {
            let sec_offset = section_offset + i * size_of::<ImageSectionHeader>();
            if sec_offset + size_of::<ImageSectionHeader>() > data.len() {
                return Err(PeError::InvalidSection);
            }

            let sec_header = unsafe {
                &*(data.as_ptr().add(sec_offset) as *const ImageSectionHeader)
            };

            // Bölüm ham verisini kopyala — dosya ofsetinden raw_size kadar
            let raw_size = sec_header.size_of_raw_data as usize;
            let raw_offset = sec_header.pointer_to_raw_data as usize;
            let raw_data = if raw_offset + raw_size <= data.len() {
                data[raw_offset..raw_offset + raw_size].to_vec()
            } else {
                vec![0u8; raw_size]
            };

            let section = PeSection {
                name: sec_header.name_as_string(),
                virtual_address: sec_header.virtual_address,
                virtual_size: sec_header.virtual_size,
                raw_data,
                characteristics: sec_header.characteristics,
                is_code: (sec_header.characteristics & IMAGE_SCN_CNT_CODE) != 0,
                is_data: (sec_header.characteristics & IMAGE_SCN_CNT_INITIALIZED_DATA) != 0,
                is_readable: (sec_header.characteristics & IMAGE_SCN_MEM_READ) != 0,
                is_writable: (sec_header.characteristics & IMAGE_SCN_MEM_WRITE) != 0,
                is_executable: (sec_header.characteristics & IMAGE_SCN_MEM_EXECUTE) != 0,
            };

            sections.push(section);
        }

        // İçe aktarma tablosunu ayrıştır (basitleştirilmiş)
        let imports = self.parse_imports(data, optional_offset, optional_header)?;

        // Dışa aktarma tablosunu ayrıştır (basitleştirilmiş)
        let exports = self.parse_exports(data, optional_offset, optional_header)?;

        let image = PeImage {
            image_base: optional_header.image_base,
            entry_point: optional_header.image_base + optional_header.address_of_entry_point as u64,
            image_size: optional_header.size_of_image,
            sections,
            imports,
            exports,
            is_dll,
            machine,
        };

        Ok(image)
    }

    /// İçe aktarma tablosunu ayrıştır
    /// Her DLL bağımlılığı ve içe aktarılan işlevlerin listesini oluşturur
    fn parse_imports(
        &self,
        data: &[u8],
        optional_offset: usize,
        optional_header: &ImageOptionalHeader64,
    ) -> Result<Vec<ImportEntry>, PeError> {
        let mut imports = Vec::new();

        // İçe aktarma dizinini bul — isteğe bağlı başlık sonrası 112. baytta
        let import_dir_offset = optional_offset + 112; // İsteğe bağlı başlık alanlarından sonra
        if import_dir_offset + size_of::<ImageDataDirectory>() > data.len() {
            return Ok(imports);
        }

        let import_dir = unsafe {
            &*(data.as_ptr().add(import_dir_offset) as *const ImageDataDirectory)
        };

        if import_dir.virtual_address == 0 {
            return Ok(imports);
        }

        // İçe aktarma dizinini bölümlerde ara
        let import_rva = import_dir.virtual_address;

        // İçe aktarma tanımlayıcılarını ayrıştır (basitleştirilmiş)
        // Gerçek uygulamada tüm tanımlayıcılar üzerinde yinelenirdi

        Ok(imports)
    }

    /// Dışa aktarma tablosunu ayrıştır
    /// DLL'nin dışarıya sunduğu işlevlerin isim→adres eşleşmesini oluşturur
    fn parse_exports(
        &self,
        data: &[u8],
        optional_offset: usize,
        optional_header: &ImageOptionalHeader64,
    ) -> Result<BTreeMap<String, u64>, PeError> {
        let mut exports = BTreeMap::new();

        // Dışa aktarma dizinini bul — isteğe bağlı başlık sonrası ilk veri dizini (ofset 96)
        let export_dir_offset = optional_offset + 96; // İlk veri dizini (dışa aktarma)
        if export_dir_offset + size_of::<ImageDataDirectory>() > data.len() {
            return Ok(exports);
        }

        let export_dir = unsafe {
            &*(data.as_ptr().add(export_dir_offset) as *const ImageDataDirectory)
        };

        if export_dir.virtual_address == 0 {
            return Ok(exports);
        }

        // Dışa aktarma girişlerini ayrıştır (basitleştirilmiş)

        Ok(exports)
    }

    /// Yüklenmiş DLL'yi al veya yükle
    pub fn get_dll(&mut self, name: &str) -> Option<Arc<Mutex<PeImage>>> {
        self.loaded_dlls.get(name).cloned()
    }

    /// DLL'yi önbelleğe kaydet
    pub fn register_dll(&mut self, name: String, image: PeImage) {
        self.loaded_dlls.insert(name, Arc::new(Mutex::new(image)));
    }

    /// İçe aktarılan işlevi çözümle — önce yüklenmiş DLL'lerde, sonra Win32 emülasyonunda ara
    pub fn resolve_import(&mut self, dll_name: &str, func_name: &str) -> Option<u64> {
        // Yüklenmiş DLL'lerde ara
        if let Some(dll) = self.loaded_dlls.get(dll_name) {
            let dll = dll.lock();
            return dll.exports.get(func_name).copied();
        }

        // Win32 API emülasyonunda ara (echOS'un kendi Win32 katmanı)
        crate::win32::get_proc_address(dll_name, func_name)
    }
}

impl Default for PeLoader {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// YARDIMCI FONKSİYONLAR
// ============================================================================

/// Little-endian 16-bit okuma
fn read_u16(data: &[u8]) -> u16 {
    u16::from_le_bytes([data[0], data[1]])
}

/// Little-endian 32-bit okuma
fn read_u32(data: &[u8]) -> u32 {
    u32::from_le_bytes([data[0], data[1], data[2], data[3]])
}

/// Little-endian 64-bit okuma
fn read_u64(data: &[u8]) -> u64 {
    u64::from_le_bytes([
        data[0], data[1], data[2], data[3],
        data[4], data[5], data[6], data[7],
    ])
}

// ============================================================================
// GLOBAL YÜKLEYİCİ
// ============================================================================

/// Spin mutex korumalı global PE yükleyici örneği
static PE_LOADER: Mutex<PeLoader> = Mutex::new(PeLoader {
    loaded_dlls: BTreeMap::new(),
});

/// PE çalıştırılabilir dosyasını yükle
pub fn load_pe(data: &[u8]) -> Result<PeImage, PeError> {
    PE_LOADER.lock().load(data)
}

/// Yüklenmiş DLL'yi al
pub fn get_dll(name: &str) -> Option<Arc<Mutex<PeImage>>> {
    PE_LOADER.lock().get_dll(name)
}

/// İçe aktarılan işlevi çözümle
pub fn resolve_import(dll_name: &str, func_name: &str) -> Option<u64> {
    PE_LOADER.lock().resolve_import(dll_name, func_name)
}
