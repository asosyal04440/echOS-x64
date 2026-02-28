//! # CPU Microcode Güncelleme Desteği
//!
//! Intel ve AMD CPU'lar için önyükleme/çalışma zamanı microcode yükleme.
//!
//! Microcode, CPU'nun iç işlem mantığını düzelten yazılım yamalarıdır.
//! BIOS/UEFI tarafından yüklenebileceği gibi işletim sistemi de her boot'ta
//! en güncel microcode'u MSR üzerinden CPU'ya yazarak güvenlik açıklarını
//! ve hatalı davranışları düzeltebilir.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

// ============================================================================
// MICROCODE SABİTLERİ
// ============================================================================

/// Intel microcode MSR adresleri — microcode yazma ve revizyon okuma için
pub const MSR_IA32_UCODE_WRITE: u32 = 0x79;
pub const MSR_IA32_UCODE_REV: u32 = 0x8B;
pub const MSR_IA32_BIOS_SIGN_ID: u32 = 0x8B;
pub const MSR_IA32_UCODE_API_VERSION: u32 = 0x8C;

/// AMD microcode MSR adresi — yama yükleyici için
pub const MSR_AMD_PATCH_LOADER: u32 = 0xC0010020;

/// Tek bir microcode güncelleme dosyasının izin verilen maksimum boyutu
pub const MICROCODE_MAX_SIZE: usize = 2 * 1024 * 1024; // 2MB

// ============================================================================
// INTEL MICROCODE BAŞLIĞI
// ============================================================================

/// Intel microcode güncelleme başlığı (header)
/// Intel SDM Vol.3A §9.11.1 — microcode güncelleme dosyasının sabit başlık alanları
#[repr(C, packed)]
pub struct IntelMicrocodeHeader {
    /// Başlık sürüm damgası (her zaman 1 olmalıdır)
    pub header_version: u32,
    /// Yama revizyon numarası — daha yüksek = daha yeni
    pub update_revision: u32,
    /// Oluşturulma tarihi (BCD formatı: 0xYYYYMMDD)
    pub date: u32,
    /// Genişletilmiş imza tablosunun boyutu (bayt)
    pub ext_sig_table_size: u32,
    /// Genişletilmiş imza tablosunun checksum değeri
    pub ext_sig_checksum: u32,
    /// Ayrılmış alanlar
    pub reserved: [u32; 3],
    /// İşlemci aile/model/stepping değeri (CPUID.EAX'tan okunur)
    pub processor_signature: u32,
    /// Güncelleme verisi + başlığın checksum'ı (toplamı 0 olmalı)
    pub checksum: u32,
    /// Yükleyici sürümü
    pub loader_revision: u32,
    /// Platform kimliği bit maskesi (hangi platform varyantları desteklenir)
    pub processor_flags: u32,
    /// Veri boyutu (bayt, 4'e bölünmüş)
    pub data_size: u32,
    /// Toplam boyut (bayt, 4'e bölünmüş) — genişletilmiş imzalar dahil
    pub total_size: u32,
    // Arkasından: data[datasize], genişletilmiş imzalar gelir
}

/// Intel genişletilmiş imza — aynı yama birden fazla CPU modeline uygulanabilir
#[repr(C)]
pub struct IntelExtSignature {
    pub processor_signature: u32,
    pub processor_flags: u32,
    pub checksum: u32,
}

// ============================================================================
// AMD MICROCODE BAŞLIĞI
// ============================================================================

/// AMD microcode güncelleme başlığı
/// AMD işlemcileri için WRMSR(0xC0010020) ile yama yükleme başlık formatı
#[repr(C, packed)]
pub struct AmdMicrocodeHeader {
    /// Veri boyutu (bayt)
    pub data_size: u32,
    /// Yama seviyesi (revizyon numarası)
    pub patch_id: u32,
    /// Ayrılmış
    pub reserved1: [u8; 4],
    /// Çip 1 kimlik numarası
    pub chip1_id: u16,
    /// Çip 2 kimlik numarası
    pub chip2_id: u16,
    /// İşlemci revizyon kimliği
    pub proc_rev_id: u16,
    /// Çip 1 revizyon kimliği
    pub chip1_rev_id: u16,
    /// Çip 2 revizyon kimliği
    pub chip2_rev_id: u16,
    /// Kuzey Köprüsü (North Bridge) kimliği
    pub nb_id: u16,
    /// Güney Köprüsü (South Bridge) kimliği
    pub sb_id: u16,
    /// BIOS revizyon numarası
    pub bios_rev: u32,
    /// Ayrılmış
    pub reserved2: [u32; 3],
    /// Eşleştirme kaydı
    pub match_reg: u32,
    /// Yama veri bloğu kimliği
    pub patch_data_id: u32,
    /// Yama bloğu uzunluğu
    pub patch_block_len: u8,
    /// Başlangıç bloğu uzunluğu
    pub init_block_len: u8,
    /// Bloğun yükleneceği taban adresi
    pub block_load_base: u16,
    /// Blok sayısı
    pub num_blocks: u8,
    /// Başlık sürümü
    pub header_version: u8,
    /// Ayrılmış
    pub reserved3: [u8; 6],
    /// Yamanın gerçek ikili verisi
    pub patch_data_block: [u8; 896],
}

// ============================================================================
// MICROCODE YÖNETİCİSİ
// ============================================================================

/// Microcode durum bilgisi — mevcut revizyon ve yüklenen yama bilgisini tutar
#[derive(Clone, Debug)]
pub struct MicrocodeInfo {
    pub vendor: CpuVendor,
    pub current_revision: u32,
    pub processor_signature: u32,
    pub processor_flags: u32,
    pub loaded_patch: Option<u32>,
}

/// CPU üretici bilgisi — microcode formatı üreticiye göre farklılık gösterir
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpuVendor {
    Intel,
    Amd,
    Unknown,
}

/// Microcode yöneticisi — CPU'nun microcode revizyonunu izler ve günceller
pub struct MicrocodeManager {
    /// Şu anki microcode revizyon numarası (MSR'dan okunur)
    current_revision: AtomicU32,
    /// İşlemci imzası: CPUID.1.EAX (aile/model/stepping)
    processor_signature: AtomicU32,
    /// Platform bayrakları (hangi kart konfigürasyonu)
    processor_flags: AtomicU32,
    /// CPU üretici kimliği
    vendor: CpuVendor,
    /// Toplam güncelleme sayısı
    update_count: AtomicU32,
    /// Son güncellenme zamanı (TSC)
    last_update: AtomicU64,
}

impl MicrocodeManager {
    pub const fn new() -> Self {
        Self {
            current_revision: AtomicU32::new(0),
            processor_signature: AtomicU32::new(0),
            processor_flags: AtomicU32::new(0),
            vendor: CpuVendor::Unknown,
            update_count: AtomicU32::new(0),
            last_update: AtomicU64::new(0),
        }
    }

    /// CPU bilgisini algıla ve microcode yöneticisini başlat
    pub fn init(&mut self) {
        // CPUID ile CPU üretici string'ini oku (EBX:EDX:ECX sırası)
        let vendor_str = self.get_vendor_string();
        self.vendor = if vendor_str.starts_with("GenuineIntel") {
            CpuVendor::Intel
        } else if vendor_str.starts_with("AuthenticAMD") {
            CpuVendor::Amd
        } else {
            CpuVendor::Unknown
        };
        
        // CPUID leaf 1 EAX: işlemci aile/model/stepping imzası
        let cpuid = unsafe { core::arch::x86_64::__cpuid(1) };
        self.processor_signature.store(cpuid.eax, Ordering::SeqCst);
        
        // MSR'dan şu anki microcode revizyonunu oku
        self.read_current_revision();
        
        crate::serial_println!(
            "[MICROCODE] Vendor: {:?}, Signature: {:#x}, Revision: {}",
            self.vendor,
            self.processor_signature.load(Ordering::SeqCst),
            self.current_revision.load(Ordering::SeqCst)
        );
    }

    /// CPUID leaf 0'dan CPU üretici string'ini oku ("GenuineIntel" / "AuthenticAMD")
    fn get_vendor_string(&self) -> [u8; 13] {
        let mut vendor = [0u8; 13];
        unsafe {
            let cpuid = core::arch::x86_64::__cpuid(0);
            let ebx = cpuid.ebx.to_le_bytes();
            let ecx = cpuid.ecx.to_le_bytes();
            let edx = cpuid.edx.to_le_bytes();
            vendor[0..4].copy_from_slice(&ebx);
            vendor[4..8].copy_from_slice(&edx);
            vendor[8..12].copy_from_slice(&ecx);
        }
        vendor
    }

    /// MSR_IA32_BIOS_SIGN_ID (0x8B) üzerinden şu anki microcode revizyonunu oku
    /// Intel SDM: önce 0 yazılır, CPUID çalıştırılır, ardından yüksek 32-bit revizyon içerir
    fn read_current_revision(&self) {
        // MSR'a 0 yazarak CPU'nun revizyon alanını yenilemesini tetikle
        unsafe {
            crate::cpu::msr::write(MSR_IA32_BIOS_SIGN_ID, 0);
            // CPUID güncellemeyi tetikler
            let _ = core::arch::x86_64::__cpuid(1);
            // Yüksek 32-bit: revizyon numarası
            let rev = crate::cpu::msr::read(MSR_IA32_BIOS_SIGN_ID) >> 32;
            self.current_revision.store(rev as u32, Ordering::SeqCst);
        }
    }

    /// Intel microcode yamasını yükle (MSR_IA32_UCODE_WRITE üzerinden)
    pub fn load_intel_microcode(&self, data: &[u8]) -> Result<u32, MicrocodeError> {
        if data.len() < core::mem::size_of::<IntelMicrocodeHeader>() {
            return Err(MicrocodeError::InvalidFormat);
        }
        
        let header = unsafe { &*(data.as_ptr() as *const IntelMicrocodeHeader) };
        
        // Başlık sürümü 1 olmalıdır — farklıysa geçersiz format
        if header.header_version != 1 {
            return Err(MicrocodeError::InvalidVersion);
        }
        
        // İşlemci imzasını doğrula — yanlış işlemci için yama yüklenemez
        let sig = self.processor_signature.load(Ordering::SeqCst);
        if header.processor_signature != sig {
            // Genişletilmiş imza tablosunu da kontrol et (aynı yama birden fazla modeli destekleyebilir)
            if header.ext_sig_table_size > 0 {
                let ext_offset = header.total_size as usize * 4;
                let num_ext = header.ext_sig_table_size as usize / 
                    core::mem::size_of::<IntelExtSignature>();
                
                for i in 0..num_ext {
                    let ext = unsafe {
                        &*(data.as_ptr().add(ext_offset + i * 
                            core::mem::size_of::<IntelExtSignature>()) 
                            as *const IntelExtSignature)
                    };
                    if ext.processor_signature == sig {
                        break;
                    }
                    if i == num_ext - 1 {
                        return Err(MicrocodeError::SignatureMismatch);
                    }
                }
            } else {
                return Err(MicrocodeError::SignatureMismatch);
            }
        }
        
        // Mevcut revizyondan eski bir yama yüklemeyi reddet
        if header.update_revision <= self.current_revision.load(Ordering::SeqCst) {
            return Err(MicrocodeError::OlderRevision);
        }
        
        // Checksum doğrula — bütün 32-bit kelimelerin toplamı 0 olmalı
        if !self.verify_intel_checksum(data, header) {
            return Err(MicrocodeError::ChecksumFailed);
        }
        
        // Microcode verisini MSR aracılığıyla CPU'ya yükle
        unsafe {
            // Başlık atlanır, veri bölümünün adresi MSR'a yazılır
            let data_offset = core::mem::size_of::<IntelMicrocodeHeader>();
            let data_ptr = data.as_ptr().add(data_offset) as u64;
            crate::cpu::msr::write(MSR_IA32_UCODE_WRITE, data_ptr);
        }
        
        // Güncelleme sonrası yeni revizyonu MSR'dan tekrar oku ve doğrula
        self.read_current_revision();
        let new_rev = self.current_revision.load(Ordering::SeqCst);
        
        if new_rev != header.update_revision {
            return Err(MicrocodeError::LoadFailed);
        }
        
        self.update_count.fetch_add(1, Ordering::SeqCst);
        crate::serial_println!("[MICROCODE] Updated to revision {}", new_rev);
        
        Ok(new_rev)
    }

    /// Intel microcode checksum doğrulaması
    /// Tüm 32-bit sözcüklerin toplanması 0 vermelidir — Intel SDM §9.11.3
    fn verify_intel_checksum(&self, data: &[u8], header: &IntelMicrocodeHeader) -> bool {
        let total_size = if header.data_size == 0 {
            1024 // Varsayılan boyut (eski format)
        } else {
            header.total_size as usize * 4
        };
        
        if data.len() < total_size {
            return false;
        }
        
        // 32-bit kelimelerin toplamı tam olarak 0 olmalı
        let mut sum: u32 = 0;
        for i in (0..total_size).step_by(4) {
            let word = unsafe {
                *(data.as_ptr().add(i) as *const u32)
            };
            sum = sum.wrapping_add(word);
        }
        
        sum == 0
    }

    /// AMD microcode yamasını yükle (WRMSR MSR_AMD_PATCH_LOADER üzerinden)
    pub fn load_amd_microcode(&self, data: &[u8]) -> Result<u32, MicrocodeError> {
        if data.len() < core::mem::size_of::<AmdMicrocodeHeader>() {
            return Err(MicrocodeError::InvalidFormat);
        }
        
        let header = unsafe { &*(data.as_ptr() as *const AmdMicrocodeHeader) };
        
        // AMD'de patch verisinin fiziksel adresi doğrudan MSR'a yazılır,
        // CPU bellekten okuyarak yamayı uygular
        unsafe {
            crate::cpu::msr::write(MSR_AMD_PATCH_LOADER, data.as_ptr() as u64);
        }
        
        // Güncellemenin geçerli olup olmadığını revizyon okuyarak doğrula
        self.read_current_revision();
        let new_rev = self.current_revision.load(Ordering::SeqCst);
        
        self.update_count.fetch_add(1, Ordering::SeqCst);
        crate::serial_println!("[MICROCODE] AMD patch loaded, revision {}", new_rev);
        
        Ok(new_rev)
    }

    /// Bir buffer'dan microcode yükle — üreticiye göre doğru yöntemi seç
    pub fn load(&self, data: &[u8]) -> Result<u32, MicrocodeError> {
        match self.vendor {
            CpuVendor::Intel => self.load_intel_microcode(data),
            CpuVendor::Amd => self.load_amd_microcode(data),
            CpuVendor::Unknown => Err(MicrocodeError::UnknownVendor),
        }
    }

    /// Şu anki microcode revizyon ve imza bilgisini al
    pub fn get_info(&self) -> MicrocodeInfo {
        MicrocodeInfo {
            vendor: self.vendor,
            current_revision: self.current_revision.load(Ordering::SeqCst),
            processor_signature: self.processor_signature.load(Ordering::SeqCst),
            processor_flags: self.processor_flags.load(Ordering::SeqCst),
            loaded_patch: None,
        }
    }
}

lazy_static::lazy_static! {
    /// Global microcode yöneticisi — tek örnek (singleton)
    static ref MICROCODE_MANAGER: spin::Mutex<MicrocodeManager> = 
        spin::Mutex::new(MicrocodeManager::new());
}

// ============================================================================
// HATA TİPİ
// ============================================================================

/// Microcode yükleme işlemi sırasında oluşabilecek hata türleri.
/// Her variant, başarısız olduğu aşamayı açıklar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicrocodeError {
    InvalidFormat,
    InvalidVersion,
    SignatureMismatch,
    ChecksumFailed,
    OlderRevision,
    LoadFailed,
    UnknownVendor,
    NotFound,
}

// ============================================================================
// BAŞLATMA
// ============================================================================

/// Microcode alt sistemini başlat — CPU türünü ve mevcut revizyonu tespit eder
pub fn init() {
    MICROCODE_MANAGER.lock().init();
    crate::serial_println!("[MICROCODE] Subsystem initialized");
}

/// Bir buffer'dan microcode yükle
pub fn load(data: &[u8]) -> Result<u32, MicrocodeError> {
    MICROCODE_MANAGER.lock().load(data)
}

/// Şu anki microcode revizyon numarasını döndür
pub fn get_revision() -> u32 {
    MICROCODE_MANAGER.lock().current_revision.load(Ordering::SeqCst)
}

/// Ayrıntılı microcode bilgisi döndür
pub fn get_info() -> MicrocodeInfo {
    MICROCODE_MANAGER.lock().get_info()
}

/// Verilen buffer'daki microcode mevcut revizyondan daha yeni mi kontrol et
pub fn check_update_available(data: &[u8]) -> bool {
    let manager = MICROCODE_MANAGER.lock();
    if data.len() < core::mem::size_of::<IntelMicrocodeHeader>() {
        return false;
    }
    
    match manager.vendor {
        CpuVendor::Intel => {
            let header = unsafe { &*(data.as_ptr() as *const IntelMicrocodeHeader) };
            header.update_revision > manager.current_revision.load(Ordering::SeqCst)
        }
        _ => false,
    }
}
