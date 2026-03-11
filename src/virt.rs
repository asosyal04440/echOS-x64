//! # echOS Sanallaştırma (Virtualization) Desteği
//!
//! Intel VMX ve AMD SVM donanım destekli sanallaştırma altyapısı.
//! EPT (Extended Page Tables) ve NPT (Nested Page Tables) ile bellek izolasyonu.
//!
//! ## Sanallaştırma Nasıl Çalışır?
//! ```ascii
//! [Konuk İşletim Sistemi (Guest OS)]
//!         |  VM-GIRIŞ (VMLAUNCH/VMRESUME)
//!         v
//! [Sanal Makine Kontrol Yapısı (VMCS)]
//!      |       |
//!  [Konuk   [Ana Makine
//!   Durumu]  Durumu]
//!         |  VM-ÇIKIŞ (VM-EXIT)
//!         v
//! [Hipervizör (Hypervisor / VMM)]
//! ```
//!
//! - Intel VMX: `VMXON`/`VMXOFF` ile VMX kipi, `VMLAUNCH`/`VMRESUME` ile konuk çalışır.
//! - AMD SVM: `VMRUN` ile konuk çalışır, `VMEXIT` ile hipervizöre döner.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::mem;
use spin::Mutex;

// ============================================================================
// SANALLAŞTIRMA SABİTLERİ
// ============================================================================

/// Sanallaştırma bilgisi için CPUID yaprağı (leaf).
///
/// `CPUID 0x40000000` çalıştırıldığında hipervizör kimliği döner.
const CPUID_VIRT_LEAF: u32 = 0x40000000;

/// Intel VMX MSR kayıtları.
///
/// Bu kayıtlar VMX özelliklerini ve yeteneklerini sorgular.
const IA32_FEATURE_CONTROL: u32 = 0x3A; // VMX kilit ve etkinleştirme denetimi
const IA32_VMX_BASIC: u32 = 0x480; // VMCS boyutu ve VMCS düzeltme kimliği
const IA32_VMX_PINBASED_CTLS: u32 = 0x481; // Pin tabanlı VM çalışma denetimleri
const IA32_VMX_PROCBASED_CTLS: u32 = 0x482; // İşlemci tabanlı VM çalışma denetimleri
const IA32_VMX_EXIT_CTLS: u32 = 0x483; // VM çıkış denetimleri
const IA32_VMX_ENTRY_CTLS: u32 = 0x484; // VM giriş denetimleri
const IA32_VMX_MISC: u32 = 0x485; // VMX çeşitli özellikleri
const IA32_VMX_CR0_FIXED0: u32 = 0x486; // CR0 sabit 0 bitleri
const IA32_VMX_CR0_FIXED1: u32 = 0x487; // CR0 sabit 1 bitleri
const IA32_VMX_CR4_FIXED0: u32 = 0x488; // CR4 sabit 0 bitleri
const IA32_VMX_CR4_FIXED1: u32 = 0x489; // CR4 sabit 1 bitleri
const IA32_VMX_VMCS_ENUM: u32 = 0x48A; // VMCS alan numaralandırması
const IA32_VMX_PROCBASED_CTLS2: u32 = 0x48B; // İkincil işlemci tabanlı denetimler
const IA32_VMX_EPT_VPID_CAP: u32 = 0x48C; // EPT ve VPID yetenekleri

/// AMD SVM MSR kayıtları.
///
/// AMD sistemlerde Güvenli Sanal Makine (SVM) özelliklerini denetler.
const MSR_VM_CR: u32 = 0xC0010114; // VM denetim kaydı (SVM etkinleştirme)
const MSR_VM_HSAVE_PA: u32 = 0xC0010117; // Ev sahibi durumu kayıt adresi
const MSR_VM_LOCK: u32 = 0xC0010115; // VM kilitleme kaydı
const MSR_VM_ASID: u32 = 0xC0010116; // Adres Alanı Tanımlayıcısı

/// VMCS alan kodlamaları (Intel belirtimi).
///
/// VMCS alanları 16-bit kodlamalara göre gruplanır:
/// - 0x0000xxxx: Denetim alanları
/// - 0x0800xxxx: Konuk alanları
/// - 0x0C00xxxx: Ana makine alanları
const VMCS_CTRL_PIN_BASED: u32 = 0x00004000; // Pin tabanlı denetimler
const VMCS_CTRL_PROC_BASED: u32 = 0x00004002; // İşlemci tabanlı birincil denetimler
const VMCS_CTRL_PROC_BASED_2: u32 = 0x0000401E; // İşlemci tabanlı ikincil denetimler
const VMCS_CTRL_EXIT: u32 = 0x0000400C; // VM çıkış denetimleri
const VMCS_CTRL_ENTRY: u32 = 0x00004012; // VM giriş denetimleri
const VMCS_CTRL_EXEC: u32 = 0x0000401C; // VM yürütme denetimleri

/// Konuk segment seçicileri (Guest Segment Selectors).
const VMCS_GUEST_ES_SEL: u32 = 0x00000800;
const VMCS_GUEST_CS_SEL: u32 = 0x00000802;
const VMCS_GUEST_SS_SEL: u32 = 0x00000804;
const VMCS_GUEST_DS_SEL: u32 = 0x00000806;
const VMCS_GUEST_FS_SEL: u32 = 0x00000808;
const VMCS_GUEST_GS_SEL: u32 = 0x0000080A;
const VMCS_GUEST_LDTR_SEL: u32 = 0x0000080C;
const VMCS_GUEST_TR_SEL: u32 = 0x0000080E;

/// Konuk kontrol kaydedici kodlamaları (Guest Control Registers).
const VMCS_GUEST_CR0: u32 = 0x00000820; // Konuk CR0 (Koruma Modu, Sayfalama)
const VMCS_GUEST_CR3: u32 = 0x00000822; // Konuk CR3 (Sayfa Tablosu Tabanı)
const VMCS_GUEST_CR4: u32 = 0x00000824; // Konuk CR4 (PAE, VMX vb. ek özellikler)
const VMCS_GUEST_ES_BASE: u32 = 0x00000806;
const VMCS_GUEST_CS_BASE: u32 = 0x00000808;
const VMCS_GUEST_SS_BASE: u32 = 0x0000080A;
const VMCS_GUEST_DS_BASE: u32 = 0x0000080C;
const VMCS_GUEST_FS_BASE: u32 = 0x0000080E;
const VMCS_GUEST_GS_BASE: u32 = 0x00000810;
const VMCS_GUEST_LDTR_BASE: u32 = 0x00000812;
const VMCS_GUEST_TR_BASE: u32 = 0x00000814;
const VMCS_GUEST_GDTR_BASE: u32 = 0x00000816;
const VMCS_GUEST_IDTR_BASE: u32 = 0x00000818;

/// Konuk yığın işaretçisi, talimat işaretçisi ve bayraklar.
const VMCS_GUEST_RSP: u32 = 0x0000081C; // Konuk yığın tepe adresi
const VMCS_GUEST_RIP: u32 = 0x0000081E; // Konuk talimat işaretçisi (giriş noktası)
const VMCS_GUEST_RFLAGS: u32 = 0x00000820; // Konuk işlemci bayrakları

/// Konuk segment sınırları (Guest Segment Limits).
const VMCS_GUEST_ES_LIMIT: u32 = 0x00000800;
const VMCS_GUEST_CS_LIMIT: u32 = 0x00000802;
const VMCS_GUEST_SS_LIMIT: u32 = 0x00000804;
const VMCS_GUEST_DS_LIMIT: u32 = 0x00000806;
const VMCS_GUEST_FS_LIMIT: u32 = 0x00000808;
const VMCS_GUEST_GS_LIMIT: u32 = 0x0000080A;
const VMCS_GUEST_LDTR_LIMIT: u32 = 0x0000080C;
const VMCS_GUEST_TR_LIMIT: u32 = 0x0000080E;
const VMCS_GUEST_GDTR_LIMIT: u32 = 0x00000810;
const VMCS_GUEST_IDTR_LIMIT: u32 = 0x00000812;

/// Konuk segment erişim hakları (Guest Segment Access Rights).
const VMCS_GUEST_ES_AR: u32 = 0x00000814;
const VMCS_GUEST_CS_AR: u32 = 0x00000816;
const VMCS_GUEST_SS_AR: u32 = 0x00000818;
const VMCS_GUEST_DS_AR: u32 = 0x0000081A;
const VMCS_GUEST_FS_AR: u32 = 0x0000081C;
const VMCS_GUEST_GS_AR: u32 = 0x0000081E;
const VMCS_GUEST_LDTR_AR: u32 = 0x00000820;
const VMCS_GUEST_TR_AR: u32 = 0x00000822;

/// Konuk etkinlik durumu ve diğer alanlar.
const VMCS_GUEST_ACTIVITY: u32 = 0x00000826; // Konuk etkinlik durumu (aktif, HLT vb.)
const VMCS_GUEST_INT_STATE: u32 = 0x00000824; // Konuk kesme durumu
const VMCS_GUEST_SMBASE: u32 = 0x00000828; // SM modu taban adresi

/// Ana makine (Host) segment seçicileri.
const VMCS_HOST_ES_SEL: u32 = 0x00000C00;
const VMCS_HOST_CS_SEL: u32 = 0x00000C02;
const VMCS_HOST_SS_SEL: u32 = 0x00000C04;
const VMCS_HOST_DS_SEL: u32 = 0x00000C06;
const VMCS_HOST_FS_SEL: u32 = 0x00000C08;
const VMCS_HOST_GS_SEL: u32 = 0x00000C0A;
const VMCS_HOST_TR_SEL: u32 = 0x00000C0C;

/// Ana makine kontrol kaydedici ve diğer alanlar.
const VMCS_HOST_CR0: u32 = 0x00000C00;
const VMCS_HOST_CR3: u32 = 0x00000C02;
const VMCS_HOST_CR4: u32 = 0x00000C04;
const VMCS_HOST_FS_BASE: u32 = 0x00000C06;
const VMCS_HOST_GS_BASE: u32 = 0x00000C08;
const VMCS_HOST_TR_BASE: u32 = 0x00000C0A;
const VMCS_HOST_GDTR_BASE: u32 = 0x00000C0C;
const VMCS_HOST_IDTR_BASE: u32 = 0x00000C0E;
const VMCS_HOST_RSP: u32 = 0x00000C10; // VM-EXIT sonrası ana makine yığını
const VMCS_HOST_RIP: u32 = 0x00000C12; // VM-EXIT sonrası ana makine giriş noktası

/// EPT işaretçisi ve VPID alanları.
const VMCS_EPTP: u32 = 0x0000201A; // Genişletilmiş Sayfa Tablosu İşaretçisi (EPTP)
const VMCS_VPID: u32 = 0x00002000; // Sanal İşlemci Kimliği (VPID)

/// VMX talimat hata kodları.
///
/// VMWRITE, VMREAD, VMLAUNCH, VMRESUME başarısız olduğunda bu kodlar döner.
const VMXERR_VMCLEAR_INVALID_ADDR: u32 = 2; // Geçersiz VMCLEAR adresi
const VMXERR_VMLAUNCH_NON_CLEAR: u32 = 4; // VMLAUNCH için temizlenmemiş VMCS
const VMXERR_VMRESUME_NON_LAUNCHED: u32 = 5; // VMRESUME için başlatılmamış VMCS
const VMXERR_VMRESUME_VMCLEAR: u32 = 6; // VMRESUME için temizlenmiş VMCS
const VMXERR_INVALID_VMCS_FIELD: u32 = 7; // Geçersiz VMCS alan kimliği
const VMXERR_INVALID_HOST_STATE: u32 = 8; // Geçersiz ana makine durumu
const VMXERR_INVALID_GUEST_STATE: u32 = 11; // Geçersiz konuk durumu

/// EPT bellek türleri (Extended Page Table Memory Types).
///
/// CPU'nun önbellek davranışını belirler.
const EPT_MEM_TYPE_UC: u64 = 0x00; // Önbelleksiz (Uncacheable)
const EPT_MEM_TYPE_WC: u64 = 0x01; // Birleştirilerek Yazılır (Write Combining)
const EPT_MEM_TYPE_WT: u64 = 0x04; // Yazma Geçirgen (Write Through)
const EPT_MEM_TYPE_WP: u64 = 0x05; // Yazmaya Korumalı (Write Protected)
const EPT_MEM_TYPE_WB: u64 = 0x06; // Geri Yazma (Write Back) - en verimli

/// EPT sayfa izinleri (EPT Permissions).
///
/// Her EPT girdisinin erişim izinlerini bit maskeleri ile belirler.
const EPT_READ: u64 = 0x01; // Okuma izni
const EPT_WRITE: u64 = 0x02; // Yazma izni
const EPT_EXECUTE: u64 = 0x04; // Çalıştırma izni (supervisor modu)
const EPT_EXECUTE_USER: u64 = 0x08; // Kullanıcı modu çalıştırma izni

/// Sayfa boyutları.
///
/// EPT 4K, 2M ve 1G sayfa boyutlarını destekler.
const PAGE_SIZE_4K: u64 = 4096; // 4 KiB: standart
const PAGE_SIZE_2M: u64 = 2 * 1024 * 1024; // 2 MiB: büyük sayfa
const PAGE_SIZE_1G: u64 = 1024 * 1024 * 1024; // 1 GiB: çok büyük sayfa

// ============================================================================
// SANALLAŞTIRMA HATASI
// ============================================================================

/// Sanallaştırma işlemleri için hata türleri.
///
/// Her tür belirli bir başarısızlık senaryosunu temsil eder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VirtError {
    /// Donanım sanallaştırma desteği yok
    NotSupported,
    /// BIOS/UEFI'de sanallaştırma devre dışı bırakılmış
    DisabledInBIOS,
    /// VMXON talimatı başarısız
    VmxOnFailed,
    /// VMCS başlatma başarısız
    VmcsInitFailed,
    /// VMLAUNCH başarısız
    VmlaunchFailed,
    /// Geçersiz durum geçişi
    InvalidState,
    /// EPT eşleme hatası
    EptError,
    /// Bellek tahsis hatası
    MemoryError,
    /// Bilinmeyen hata
    Unknown,
}

// ============================================================================
// CPU ÜRETİCİSİ
// ============================================================================

/// CPU üreticisi tanımlayıcısı.
///
/// VMX (Intel) ve SVM (AMD) için farklı başlatma yolları kullanılır.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpuVendor {
    Intel,
    Amd,
    Unknown,
}

impl CpuVendor {
    /// CPUID talimatı ile CPU üreticisini tespit eder.
    ///
    /// Gerçek uygulamada `cpuid` talimatı çalıştırılmalıdır.
    /// CPUID yaprağı 0 satıcı dizisini döner ("GenuineIntel" veya "AuthenticAMD").
    pub fn detect() -> Self {
        // CPUID yaprağı 0, satıcı dizesini döner
        // Gerçek uygulamada cpuid talimatı kullanılmalı
        CpuVendor::Intel // Şimdilik varsayılan
    }
}

// ============================================================================
// VMX DESTEĞİ (INTEL)
// ============================================================================

/// Intel VMX yetenekleri ve konfigürasyonu.
///
/// `VmxCapabilities::detect()` MSR'lardan VMX özelliklerini okur.
#[derive(Clone, Debug)]
pub struct VmxCapabilities {
    pub supported: bool,
    pub enabled: bool,
    pub locked: bool,
    pub vmxon_region_size: u32,
    pub vmcs_revision: u32,
    pub use_msr_bitmaps: bool,
    pub use_io_bitmaps: bool,
    pub use_tpr_shadow: bool,
    pub use_ept: bool,
    pub use_vpid: bool,
    pub ept_capabilities: EptCapabilities,
}

impl VmxCapabilities {
    /// VMX yeteneklerini tespit eder.
    ///
    /// Gerçek uygulamada şu adımlar uygulanmalıdır:
    /// 1. `CPUID.1:ECX.VMX[5]` bitini kontrol et → VMX destekli mi?
    /// 2. `IA32_FEATURE_CONTROL` MSR'ını oku → VMX etkin ve kilitli mi?
    /// 3. `IA32_VMX_BASIC` MSR'ını oku → VMCS boyutunu ve düzeltme kimliğini al.
    pub fn detect() -> Self {
        // CPUID.1:ECX.VMX[5] bitini kontrol et
        // IA32_FEATURE_CONTROL MSR'ını oku
        // IA32_VMX_BASIC MSR'ını oku

        VmxCapabilities {
            supported: true,
            enabled: true,
            locked: true,
            vmxon_region_size: 4096,
            vmcs_revision: 1,
            use_msr_bitmaps: true,
            use_io_bitmaps: true,
            use_tpr_shadow: true,
            use_ept: true,
            use_vpid: true,
            ept_capabilities: EptCapabilities::detect(),
        }
    }
}

// ============================================================================
// SVM DESTEĞİ (AMD)
// ============================================================================

/// AMD SVM (Secure Virtual Machine) yetenekleri ve konfigürasyonu.
#[derive(Clone, Debug)]
pub struct SvmCapabilities {
    pub supported: bool,
    pub enabled: bool,
    pub nested_paging: bool,
    pub asid_count: u32,
    pub npt_size: u32,
}

impl SvmCapabilities {
    /// AMD SVM yeteneklerini tespit eder.
    ///
    /// Gerçek uygulamada şu adımlar uygulanmalıdır:
    /// 1. `CPUID 0x8000000A` çalıştır → SVM özelliklerini kontrol et.
    /// 2. `VM_CR` MSR'ını oku → SVM etkin ve kilitlenme durumu.
    pub fn detect() -> Self {
        // CPUID 0x8000000A ile SVM özelliklerini kontrol et
        // VM_CR MSR'ını oku

        SvmCapabilities {
            supported: false, // Intel test sistemi için varsayılan
            enabled: false,
            nested_paging: false,
            asid_count: 0,
            npt_size: 0,
        }
    }
}

// ============================================================================
// EPT (GENİŞLETİLMİŞ SAYFA TABLOLARI)
// ============================================================================

/// Intel EPT (Extended Page Tables) yetenekleri.
///
/// EPT, konuk fiziksel adresleri (GPA) ana makine fiziksel adreslerine (HPA) çevirir.
/// Böylece her konuk kendi bellek alanında izole çalışır.
#[derive(Clone, Debug)]
pub struct EptCapabilities {
    pub supported: bool,
    pub page_walk_4: bool,    // 4 seviyeli sayfa gezintisi destekli mi?
    pub page_walk_5: bool,    // 5 seviyeli sayfa gezintisi destekli mi?
    pub pml4_1g_pages: bool,  // 1 GiB büyük sayfa desteği
    pub pml4_2m_pages: bool,  // 2 MiB büyük sayfa desteği
    pub invept: bool,         // INVEPT talimatı destekli mi?
    pub invept_single: bool,  // Tek bağlam INVEPT
    pub invept_global: bool,  // Global INVEPT
    pub invept_context: bool, // Tüm bağlam INVEPT
    pub memory_types: u8,     // Desteklenen EPT bellek türleri bit maskesi
}

impl EptCapabilities {
    /// EPT yeteneklerini tespit eder.
    ///
    /// Gerçek uygulamada `IA32_VMX_EPT_VPID_CAP` MSR'ını oku.
    pub fn detect() -> Self {
        // IA32_VMX_EPT_VPID_CAP MSR'ını oku
        EptCapabilities {
            supported: true,
            page_walk_4: true,
            page_walk_5: false,
            pml4_1g_pages: true,
            pml4_2m_pages: true,
            invept: true,
            invept_single: true,
            invept_global: true,
            invept_context: false,
            memory_types: 0x3F,
        }
    }
}

/// EPT sayfa tablosu girdisi (PML4/PDPT/PD/PT düzeyi).
///
/// Her girdi bir fiziksel adresi, izinleri ve bellek türünü kodlar.
#[derive(Clone, Copy, Debug)]
pub struct EptEntry {
    pub value: u64,
}

impl EptEntry {
    /// Boş (mevcut olmayan) bir EPT girdisi oluşturur.
    pub fn new() -> Self {
        EptEntry { value: 0 }
    }

    /// Fiziksel adres, izinler ve bellek türünden EPT girdisi oluşturur.
    pub fn from_addr(addr: u64, perms: u64, mem_type: u64) -> Self {
        EptEntry {
            value: (addr & 0x000FFFFF_FFFFF000) | (perms & 0xF) | ((mem_type & 0x7) << 3) | 0x40, // Mevcut + Yaz + Çalıştır
        }
    }

    /// Girdideki fiziksel adresi düzeltilmiş maske ile döner.
    pub fn get_addr(&self) -> u64 {
        self.value & 0x000FFFFF_FFFFF000
    }

    /// Girdi mevcut (present) mu?
    pub fn is_present(&self) -> bool {
        (self.value & 0x40) != 0
    }

    /// Girdinin mevcutluk bitini ayarlar veya temizler.
    pub fn set_present(&mut self, present: bool) {
        if present {
            self.value |= 0x40;
        } else {
            self.value &= !0x40;
        }
    }

    /// Büyük sayfa (large page) girdi mi? (2 MiB veya 1 GiB)
    pub fn is_large(&self) -> bool {
        (self.value & 0x80) != 0
    }

    /// Büyük sayfa bitini ayarlar veya temizler.
    pub fn set_large(&mut self, large: bool) {
        if large {
            self.value |= 0x80;
        } else {
            self.value &= !0x80;
        }
    }

    /// Girdi izin bitlerini döner (Okuma/Yazma/Çalıştırma).
    pub fn get_permissions(&self) -> u64 {
        self.value & 0xF
    }

    /// Girdi izinlerini günceller.
    pub fn set_permissions(&mut self, perms: u64) {
        self.value = (self.value & !0xF) | (perms & 0xF);
    }
}

/// EPT Sayfa Tablosu yapısı (4 seviyeli: PML4 -> PDPT -> PD -> PT).
///
/// Her tablo 512 girdi içerir. Konuk fiziksel adresi dört indeks ile ayrıştırılır:
/// ```ascii
/// GPA: [PML4 idx 9 bit][PDPT idx 9 bit][PD idx 9 bit][PT idx 9 bit][Offset 12 bit]
/// ```
#[derive(Clone, Debug)]
pub struct EptPageTable {
    pub pml4: Vec<EptEntry>, // 4. seviye: PML4 tablosu
    pub pdpt: Vec<EptEntry>, // 3. seviye: PDPT tablosu
    pub pd: Vec<EptEntry>,   // 2. seviye: PD tablosu
    pub pt: Vec<EptEntry>,   // 1. seviye: PT tablosu
    pub pml4_phys: u64,      // PML4 tablosunun fiziksel adresi (EPTP için)
}

impl EptPageTable {
    /// Boş bir EPT sayfa tablosu oluşturur.
    ///
    /// Her tablo 512 geçersiz girdi ile başlatılır.
    pub fn new() -> Self {
        EptPageTable {
            pml4: vec![EptEntry::new(); 512],
            pdpt: vec![EptEntry::new(); 512],
            pd: vec![EptEntry::new(); 512],
            pt: vec![EptEntry::new(); 512],
            pml4_phys: 0,
        }
    }

    /// 4 KiB büyüklüğünde bir sayfayı eşler.
    ///
    /// Konuk fiziksel adresi (GPA) -> Ana makine fiziksel adresi (HPA) eşlemesi oluşturur.
    /// Ara tablo düzeyleri gerektiğinde otomatik olarak bağlanır.
    pub fn map_4k(&mut self, gpa: u64, hpa: u64, perms: u64, mem_type: u64) {
        // GPA'yı tablo seviyesi indekslerine ayır
        let pml4_idx = ((gpa >> 39) & 0x1FF) as usize;
        let pdpt_idx = ((gpa >> 30) & 0x1FF) as usize;
        let pd_idx = ((gpa >> 21) & 0x1FF) as usize;
        let pt_idx = ((gpa >> 12) & 0x1FF) as usize;

        // PT seviyesinde HPA girdisini oluştur
        self.pt[pt_idx] = EptEntry::from_addr(hpa, perms, mem_type);
        self.pt[pt_idx].set_present(true);

        // Üst seviyeleri bağla (yoksa oluştur)
        if !self.pml4[pml4_idx].is_present() {
            self.pml4[pml4_idx] = EptEntry::from_addr(
                self.pdpt.as_ptr() as u64,
                EPT_READ | EPT_WRITE | EPT_EXECUTE,
                EPT_MEM_TYPE_WB,
            );
            self.pml4[pml4_idx].set_present(true);
        }

        if !self.pdpt[pdpt_idx].is_present() {
            self.pdpt[pdpt_idx] = EptEntry::from_addr(
                self.pd.as_ptr() as u64,
                EPT_READ | EPT_WRITE | EPT_EXECUTE,
                EPT_MEM_TYPE_WB,
            );
            self.pdpt[pdpt_idx].set_present(true);
        }

        if !self.pd[pd_idx].is_present() {
            self.pd[pd_idx] = EptEntry::from_addr(
                self.pt.as_ptr() as u64,
                EPT_READ | EPT_WRITE | EPT_EXECUTE,
                EPT_MEM_TYPE_WB,
            );
            self.pd[pd_idx].set_present(true);
        }
    }

    /// 2 MiB büyüklüğünde büyük bir sayfayı eşler.
    ///
    /// PD seviyesinde büyük sayfa girdisi oluşturur; PT tablosuna gerek yoktur.
    /// 2 MiB hizalı GPA ve HPA gerektirir.
    pub fn map_2m(&mut self, gpa: u64, hpa: u64, perms: u64, mem_type: u64) {
        let pml4_idx = ((gpa >> 39) & 0x1FF) as usize;
        let pdpt_idx = ((gpa >> 30) & 0x1FF) as usize;
        let pd_idx = ((gpa >> 21) & 0x1FF) as usize;

        // PD seviyesinde büyük sayfa girdisi oluştur
        self.pd[pd_idx] = EptEntry::from_addr(hpa, perms, mem_type);
        self.pd[pd_idx].set_present(true);
        self.pd[pd_idx].set_large(true); // Büyük sayfa bayrağı

        // Üst seviyeleri bağla
        if !self.pml4[pml4_idx].is_present() {
            self.pml4[pml4_idx] = EptEntry::from_addr(
                self.pdpt.as_ptr() as u64,
                EPT_READ | EPT_WRITE | EPT_EXECUTE,
                EPT_MEM_TYPE_WB,
            );
            self.pml4[pml4_idx].set_present(true);
        }

        if !self.pdpt[pdpt_idx].is_present() {
            self.pdpt[pdpt_idx] = EptEntry::from_addr(
                self.pd.as_ptr() as u64,
                EPT_READ | EPT_WRITE | EPT_EXECUTE,
                EPT_MEM_TYPE_WB,
            );
            self.pdpt[pdpt_idx].set_present(true);
        }
    }

    /// VMCS'ye yazılacak EPTP (EPT Pointer) değerini hesaplar.
    ///
    /// EPTP formatı (Intel SDM Vol. 3C):
    /// - Bit 2:0 - Bellek türü (0=UC, 6=WB)
    /// - Bit 5:3 - Sayfa gezinisi uzunluğu eksi 1 (3=PML4 = 4 seviye)
    /// - Bit 51:12 - PML4 fiziksel adresi
    /// - Bit 6 - Kirli/Erişilen bayrak güncellemelerini etkinleştir
    pub fn get_eptp(&self) -> u64 {
        // EPTP formatı:
        // Bit 2:0 - Bellek türü (0=UC, 6=WB)
        // Bit 5:3 - Sayfa gezinisi uzunluğu eksi 1 (3=PML4, 4=PML5)
        // Bit 51:12 - PML4 fiziksel adresi
        // Bit 6 - Kirli bayrak erişimi/güncellemelerini etkinleştir
        let mem_type = EPT_MEM_TYPE_WB;
        let walk_length = 3; // 4 seviyeli sayfalama
        (mem_type) | (walk_length << 3) | (self.pml4_phys & 0x000FFFFF_FFFFF000) | (1 << 6)
    }
}

impl Default for EptPageTable {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// VMCS (SANAL MAKİNE DENETİM YAPISI)
// ============================================================================

/// Intel VMCS (Virtual Machine Control Structure).
///
/// VMCS, bir konuk sanal makinenin tüm durumunu ve giriş/çıkış denetimlerini tutar.
/// Her mantıksal işlemci için bir VMCS bulunur.
#[derive(Clone, Debug)]
pub struct Vmcs {
    pub revision_id: u32,     // VMCS düzeltme kimliği (IA32_VMX_BASIC'den alınır)
    pub abort_indicator: u32, // VMX iptal göstergesi
    pub data: Vec<u64>,       // VMCS veri alanı (gerçekte 4 KiB boyut)
    pub initialized: bool,    // Konuk durumu başlatıldı mı?
    pub launched: bool,       // VMLAUNCH çağrıldı mı?
}

impl Vmcs {
    /// Belirtilen düzeltme kimliğiyle yeni bir VMCS oluşturur.
    ///
    /// `data` alanı VMCS boyutunu simüle eder (4 KiB = 512 adet u64).
    pub fn new(revision: u32) -> Self {
        Vmcs {
            revision_id: revision,
            abort_indicator: 0,
            data: vec![0; 2048 / 8], // VMCS boyutu 4 KB'dır
            initialized: false,
            launched: false,
        }
    }

    /// VMCS alanına değer yazar.
    ///
    /// Gerçek uygulamada `VMWRITE` talimatı kullanılmalıdır.
    pub fn write(&mut self, field: u32, value: u64) -> Result<(), VirtError> {
        // Gerçek uygulamada VMWRITE talimatı kullanılmalı
        let offset = Self::field_to_offset(field);
        if offset < self.data.len() {
            self.data[offset] = value;
            Ok(())
        } else {
            Err(VirtError::VmcsInitFailed)
        }
    }

    /// VMCS alanından değer okur.
    ///
    /// Gerçek uygulamada `VMREAD` talimatı kullanılmalıdır.
    pub fn read(&self, field: u32) -> Result<u64, VirtError> {
        let offset = Self::field_to_offset(field);
        if offset < self.data.len() {
            Ok(self.data[offset])
        } else {
            Err(VirtError::VmcsInitFailed)
        }
    }

    /// VMCS alan kodlamasını veri dizisi ofsetine dönüştürür.
    fn field_to_offset(field: u32) -> usize {
        // VMCS alan kodlaması ofsete dönüşüm
        ((field & 0x7FF) as usize) * 2
    }

    /// Konuk işlemci durumunu başlatır.
    ///
    /// Segment seçicileri, taban adresleri, sınırlar, erişim hakları,
    /// kontrol kaydediciler ve başlangıç RIP/RSP/RFLAGS değerleri ayarlanır.
    pub fn setup_guest_state(&mut self, entry_point: u64, stack: u64) -> Result<(), VirtError> {
        // Konuk segment seçicileri
        self.write(VMCS_GUEST_CS_SEL, 0x08)?; // Çekirdek kod segmenti
        self.write(VMCS_GUEST_DS_SEL, 0x10)?; // Çekirdek veri segmenti
        self.write(VMCS_GUEST_SS_SEL, 0x10)?;
        self.write(VMCS_GUEST_ES_SEL, 0x10)?;
        self.write(VMCS_GUEST_FS_SEL, 0x10)?;
        self.write(VMCS_GUEST_GS_SEL, 0x10)?;

        // Konuk segment taban adresleri (düz bellek modeli için sıfır)
        self.write(VMCS_GUEST_CS_BASE, 0)?;
        self.write(VMCS_GUEST_DS_BASE, 0)?;
        self.write(VMCS_GUEST_SS_BASE, 0)?;
        self.write(VMCS_GUEST_ES_BASE, 0)?;
        self.write(VMCS_GUEST_FS_BASE, 0)?;
        self.write(VMCS_GUEST_GS_BASE, 0)?;

        // Konuk segment sınırları (4 GiB granülerlik)
        self.write(VMCS_GUEST_CS_LIMIT, 0xFFFFF)?;
        self.write(VMCS_GUEST_DS_LIMIT, 0xFFFFF)?;
        self.write(VMCS_GUEST_SS_LIMIT, 0xFFFFF)?;
        self.write(VMCS_GUEST_ES_LIMIT, 0xFFFFF)?;
        self.write(VMCS_GUEST_FS_LIMIT, 0xFFFFF)?;
        self.write(VMCS_GUEST_GS_LIMIT, 0xFFFFF)?;

        // Konuk segment erişim hakları
        // 0xA09B = Mevcut, DPL=0, Kod, Çalıştırılabilir, Okunabilir, Erişildi
        self.write(VMCS_GUEST_CS_AR, 0xA09B)?;
        // 0xC093 = Mevcut, DPL=0, Veri, Yazılabilir, Erişildi
        self.write(VMCS_GUEST_DS_AR, 0xC093)?;
        self.write(VMCS_GUEST_SS_AR, 0xC093)?;
        self.write(VMCS_GUEST_ES_AR, 0xC093)?;
        self.write(VMCS_GUEST_FS_AR, 0xC093)?;
        self.write(VMCS_GUEST_GS_AR, 0xC093)?;

        // Konuk kontrol kaydediciler
        self.write(VMCS_GUEST_CR0, 0x80000001)?; // PE (Koruma Modu) + PG (Sayfalama)
        self.write(VMCS_GUEST_CR3, 0)?;
        self.write(VMCS_GUEST_CR4, 0x00000620)?; // PAE + VMXE (VMX Etkinleştirme)

        // Konuk RIP (giriş noktası), RSP (yığın), RFLAGS
        self.write(VMCS_GUEST_RIP, entry_point)?;
        self.write(VMCS_GUEST_RSP, stack)?;
        self.write(VMCS_GUEST_RFLAGS, 0x02)?; // Daima 1 olan ayrılmış bit

        self.initialized = true;
        Ok(())
    }

    /// Ana makine işlemci durumunu başlatır.
    ///
    /// VM-EXIT sonrasında hipervizörün kaldığı yerden devam edeceği durum.
    pub fn setup_host_state(&mut self, host_rsp: u64, host_rip: u64) -> Result<(), VirtError> {
        // Ana makine segment seçicileri
        self.write(VMCS_HOST_CS_SEL, 0x08)?;
        self.write(VMCS_HOST_DS_SEL, 0x10)?;
        self.write(VMCS_HOST_SS_SEL, 0x10)?;
        self.write(VMCS_HOST_ES_SEL, 0x10)?;
        self.write(VMCS_HOST_FS_SEL, 0x10)?;
        self.write(VMCS_HOST_GS_SEL, 0x10)?;
        self.write(VMCS_HOST_TR_SEL, 0x28)?;

        // Ana makine kontrol kaydediciler
        self.write(VMCS_HOST_CR0, 0x80000001)?;
        self.write(VMCS_HOST_CR3, 0)?;
        self.write(VMCS_HOST_CR4, 0x00000620)?;

        // Ana makine RSP ve RIP (VM-EXIT giriş noktası)
        self.write(VMCS_HOST_RSP, host_rsp)?;
        self.write(VMCS_HOST_RIP, host_rip)?;

        Ok(())
    }

    /// VM giriş/çıkış denetimlerini ve EPT işaretçisini yapılandırır.
    pub fn setup_controls(&mut self, eptp: u64) -> Result<(), VirtError> {
        // Pin tabanlı denetimler: harici kesme çıkışı etkin
        self.write(VMCS_CTRL_PIN_BASED, 0x00000001)?; // Harici kesme çıkışı

        // Birincil işlemci tabanlı denetimler
        let proc_ctrl = 0x00000000;
        self.write(VMCS_CTRL_PROC_BASED, proc_ctrl)?;

        // İkincil işlemci tabanlı denetimler: EPT etkin
        let proc_ctrl2 = 0x00000002; // EPT'yi etkinleştir
        self.write(VMCS_CTRL_PROC_BASED_2, proc_ctrl2)?;

        // EPTP: EPT sayfa tablosunun kök adresi
        self.write(VMCS_EPTP, eptp)?;

        // Çıkış denetimleri
        self.write(VMCS_CTRL_EXIT, 0x00000000)?;

        // Giriş denetimleri
        self.write(VMCS_CTRL_ENTRY, 0x00000000)?;

        Ok(())
    }
}

// ============================================================================
// SANAL MAKİNE
// ============================================================================

/// Tek bir sanal makine örneğini temsil eder.
///
/// VMCS, EPT tablosu, konuk belleği ve çalışma durumunu içerir.
#[derive(Clone, Debug)]
pub struct VirtualMachine {
    pub id: u32,
    pub name: String,
    pub vmcs: Vmcs,
    pub ept: EptPageTable,
    pub state: VmState,
    pub exit_reason: u32,
    pub exit_qualification: u64,
    pub guest_memory: Vec<u8>,
    pub guest_memory_size: usize,
}

/// Sanal makine çalışma durumu.
///
/// Durum geçişleri:
/// Created -> Running -> Paused -> Running ...
/// Running -> Halted
/// Running -> Error
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VmState {
    Created, // Oluşturuldu, henüz başlatılmadı
    Running, // Çalışıyor
    Halted,  // Durduruldu
    Paused,  // Duraklatıldı (VM-EXIT nedeniyle)
    Error,   // Hata durumunda
}

impl VirtualMachine {
    /// Yeni bir sanal makine oluşturur.
    ///
    /// Belirtilen boyutta konuk belleği tahsis eder ve tüm durum sıfırlanmış olarak başlatılır.
    pub fn new(id: u32, name: &str, memory_size: usize, vmcs_revision: u32) -> Self {
        VirtualMachine {
            id,
            name: name.to_string(),
            vmcs: Vmcs::new(vmcs_revision),
            ept: EptPageTable::new(),
            state: VmState::Created,
            exit_reason: 0,
            exit_qualification: 0,
            guest_memory: vec![0; memory_size],
            guest_memory_size: memory_size,
        }
    }

    /// Sanal makineyi başlatır: EPT, VMCS konuk/ana makine durumu yapılandırılır.
    pub fn init(&mut self, entry_point: u64, stack_top: u64) -> Result<(), VirtError> {
        // EPT'yi kur: konuk fiziksel bellekten ana makineye eşleme
        // Her 4 KiB'lık bloku konuk fiziksel adres -> ana makine fiziksel adres olarak eşle
        for i in 0..self.guest_memory_size / 4096 {
            let gpa = (i * 4096) as u64;
            let hpa = self.guest_memory.as_ptr() as u64 + gpa;
            self.ept.map_4k(
                gpa,
                hpa,
                EPT_READ | EPT_WRITE | EPT_EXECUTE,
                EPT_MEM_TYPE_WB,
            );
        }

        // VMCS'yi yapılandır
        let eptp = self.ept.get_eptp();
        self.vmcs.setup_controls(eptp)?;
        self.vmcs.setup_guest_state(entry_point, stack_top)?;
        self.vmcs.setup_host_state(0, 0)?; // Ana makine durumu VM girişinde ayarlanır

        self.state = VmState::Created;
        Ok(())
    }

    /// Sanal makineyi başlatır (VMLAUNCH simügülasyonu).
    ///
    /// Gerçek uygulamada `VMLAUNCH` talimatı çalıştırılmalıdır.
    pub fn start(&mut self) -> Result<(), VirtError> {
        if self.state != VmState::Created && self.state != VmState::Halted {
            return Err(VirtError::InvalidState);
        }

        // Gerçek uygulamada VMLAUNCH talimatı kullanılmalı
        self.state = VmState::Running;
        self.vmcs.launched = true;

        Ok(())
    }

    /// Duraklatılmış sanal makineyi sürdürür (VMRESUME simülasyonu).
    ///
    /// Gerçek uygulamada `VMRESUME` talimatı çalıştırılmalıdır.
    pub fn resume(&mut self) -> Result<(), VirtError> {
        if self.state != VmState::Paused {
            return Err(VirtError::InvalidState);
        }

        // Gerçek uygulamada VMRESUME talimatı kullanılmalı
        self.state = VmState::Running;

        Ok(())
    }

    /// Çalışan sanal makineyi duraklatır.
    pub fn pause(&mut self) -> Result<(), VirtError> {
        if self.state != VmState::Running {
            return Err(VirtError::InvalidState);
        }

        self.state = VmState::Paused;
        Ok(())
    }

    /// Sanal makineyi durdurur.
    pub fn stop(&mut self) -> Result<(), VirtError> {
        self.state = VmState::Halted;
        Ok(())
    }

    /// VM-EXIT nedenini işler.
    ///
    /// Çıkış nedenine göre farklı eylemler alınır:
    /// - 0: Harici kesme → duraklatıldı
    /// - 1: Üçlü hata → hata durumu
    /// - Diğer: Genel VM çıkışı → duraklatıldı
    pub fn handle_exit(&mut self) -> Result<(), VirtError> {
        // Çıkış nedenini ve nitelemesini oku
        // self.exit_reason = self.vmcs.read(VM_EXIT_REASON)? as u32;
        // self.exit_qualification = self.vmcs.read(VM_EXIT_QUALIFICATION)?;

        match self.exit_reason {
            0 => {
                // Harici kesme: konuk duraklatıldı
                self.state = VmState::Paused;
            }
            1 => {
                // Üçlü hata: kurtarılamaz hata durumu
                self.state = VmState::Error;
                return Err(VirtError::InvalidState);
            }
            _ => {
                // Diğer çıkış nedenleri: hipervizör ele alacak
                self.state = VmState::Paused;
            }
        }

        Ok(())
    }

    /// Konuk belleğine salt okunur referans döner.
    pub fn get_memory(&self) -> &[u8] {
        &self.guest_memory
    }

    /// Konuk belleğine değiştirilebilir referans döner.
    pub fn get_memory_mut(&mut self) -> &mut [u8] {
        &mut self.guest_memory
    }
}

// ============================================================================
// SANAL MAKİNE YÖNETİCİSİ (VMM)
// ============================================================================

/// Sanal Makine Yöneticisi (Virtual Machine Manager / Hypervisor).
///
/// Tüm sanal makineleri, CPU özelliklerini ve sanallaştırma altyapısını yönetir.
#[derive(Clone, Debug)]
pub struct Vmm {
    pub vendor: CpuVendor,                  // CPU üreticisi (Intel/AMD)
    pub vmx_caps: Option<VmxCapabilities>,  // Intel VMX yetenekleri
    pub svm_caps: Option<SvmCapabilities>,  // AMD SVM yetenekleri
    pub vms: BTreeMap<u32, VirtualMachine>, // Sanal makine koleksiyonu (kimlik -> VM)
    pub next_vm_id: u32,                    // Sonraki sanal makine kimliği
    pub initialized: bool,                  // VMM başlatıldı mı?
}

impl Vmm {
    /// Yeni bir VMM örneği oluşturur.
    pub fn new() -> Self {
        Vmm {
            vendor: CpuVendor::detect(),
            vmx_caps: None,
            svm_caps: None,
            vms: BTreeMap::new(),
            next_vm_id: 1,
            initialized: false,
        }
    }

    /// VMM'yi başlatır: CPU üreticisine göre VMX veya SVM yapılandırılır.
    pub fn init(&mut self) -> Result<(), VirtError> {
        crate::serial_println!("[VMM] Initializing virtualization...");

        match self.vendor {
            CpuVendor::Intel => {
                let caps = VmxCapabilities::detect();
                if !caps.supported {
                    return Err(VirtError::NotSupported);
                }
                if !caps.enabled {
                    return Err(VirtError::DisabledInBIOS);
                }
                self.vmx_caps = Some(caps);
                crate::serial_println!("[VMM] Intel VMX detected and enabled");
            }
            CpuVendor::Amd => {
                let caps = SvmCapabilities::detect();
                if !caps.supported {
                    return Err(VirtError::NotSupported);
                }
                if !caps.enabled {
                    return Err(VirtError::DisabledInBIOS);
                }
                self.svm_caps = Some(caps);
                crate::serial_println!("[VMM] AMD SVM detected and enabled");
            }
            CpuVendor::Unknown => {
                return Err(VirtError::NotSupported);
            }
        }

        self.initialized = true;
        Ok(())
    }

    /// Yeni bir sanal makine oluşturur.
    ///
    /// `name`: sanal makine adı, `memory_size`: konuk bellek boyutu (bayt).
    /// Döner değer yeni VM'nin kimlik numarasıdır.
    pub fn create_vm(&mut self, name: &str, memory_size: usize) -> Result<u32, VirtError> {
        if !self.initialized {
            return Err(VirtError::NotSupported);
        }

        let id = self.next_vm_id;
        self.next_vm_id += 1;

        let vmcs_revision = self.vmx_caps.as_ref().map(|c| c.vmcs_revision).unwrap_or(1);
        let vm = VirtualMachine::new(id, name, memory_size, vmcs_revision);

        self.vms.insert(id, vm);

        crate::serial_println!(
            "[VMM] Created VM {} ({}) with {} MB memory",
            id,
            name,
            memory_size / (1024 * 1024)
        );

        Ok(id)
    }

    /// Verilen kimliğe sahip sanal makineye salt okunur referans döner.
    pub fn get_vm(&self, id: u32) -> Option<&VirtualMachine> {
        self.vms.get(&id)
    }

    /// Verilen kimliğe sahip sanal makineye değiştirilebilir referans döner.
    pub fn get_vm_mut(&mut self, id: u32) -> Option<&mut VirtualMachine> {
        self.vms.get_mut(&id)
    }

    /// Sanal makineyi yok eder ve koleksiyondan kaldırır.
    pub fn destroy_vm(&mut self, id: u32) -> bool {
        self.vms.remove(&id).is_some()
    }

    /// Tüm sanal makinelerin (kimlik, ad, durum) listesini döner.
    pub fn list_vms(&self) -> Vec<(u32, String, VmState)> {
        self.vms
            .iter()
            .map(|(id, vm)| (*id, vm.name.clone(), vm.state))
            .collect()
    }
}

impl Default for Vmm {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// GLOBAL VMM ÖRNEĞİ
// ============================================================================

/// Küresel VMM (hipervizör) örneği.
///
/// `lazy_static` ile yalnızca ilk erişimde oluşturulur.
/// `Mutex<Vmm>` ile çok işlemcili güvenli erişim sağlanır.
lazy_static::lazy_static! {
    static ref VMM_INSTANCE: Mutex<Vmm> = Mutex::new(Vmm::new());
}

/// Sanallaştırma sistemini başlatır.
///
/// CPU üreticisini tespit eder ve ilgili donanım sanallaştırma özelliklerini yapılandırır.
pub fn init() -> Result<(), VirtError> {
    VMM_INSTANCE.lock().init()
}

/// Küresel VMM'nin klonunu döner.
///
/// Durum sorgulaması için kullanılır; döner değer anlık görüntüdür.
pub fn get_vmm() -> Vmm {
    VMM_INSTANCE.lock().clone()
}

/// Yeni bir sanal makine oluşturur.
pub fn create_vm(name: &str, memory_size: usize) -> Result<u32, VirtError> {
    VMM_INSTANCE.lock().create_vm(name, memory_size)
}

/// Belirtilen kimliğe sahip sanal makineyi döner (klon olarak).
pub fn get_vm(id: u32) -> Option<VirtualMachine> {
    VMM_INSTANCE.lock().get_vm(id).cloned()
}

/// Belirtilen kimliğe sahip sanal makineyi yok eder.
pub fn destroy_vm(id: u32) -> bool {
    VMM_INSTANCE.lock().destroy_vm(id)
}

/// Tüm sanal makinelerin listesini döner.
pub fn list_vms() -> Vec<(u32, String, VmState)> {
    VMM_INSTANCE.lock().list_vms()
}

// ============================================================================
// KVM Geliştirmeleri — VM Exit Handler, VPID, EPT, vCPU
// ============================================================================

/// VM-Exit nedenleri (Intel SDM Vol 3C, Appendix C)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmExitReason {
    /// Harici kesme
    ExternalInterrupt = 1,
    /// Triple fault
    TripleFault = 2,
    /// INIT sinyali
    InitSignal = 3,
    /// CPUID talimatı
    Cpuid = 10,
    /// HLT talimatı
    Hlt = 12,
    /// INVLPG
    Invlpg = 14,
    /// RDTSC
    Rdtsc = 16,
    /// VMCALL (hipervizör çağrısı)
    Vmcall = 18,
    /// CR erişimi
    CrAccess = 28,
    /// I/O portu erişimi
    IoInstruction = 30,
    /// MSR okuma
    RdMsr = 31,
    /// MSR yazma
    WrMsr = 32,
    /// EPT ihlali
    EptViolation = 48,
    /// EPT yanlış yapılandırma
    EptMisconfiguration = 49,
    /// XSETBV
    Xsetbv = 55,
    /// Bilinmeyen
    Unknown = 0xFFFF,
}

impl VmExitReason {
    pub fn from_u32(val: u32) -> Self {
        match val {
            1 => Self::ExternalInterrupt,
            2 => Self::TripleFault,
            3 => Self::InitSignal,
            10 => Self::Cpuid,
            12 => Self::Hlt,
            14 => Self::Invlpg,
            16 => Self::Rdtsc,
            18 => Self::Vmcall,
            28 => Self::CrAccess,
            30 => Self::IoInstruction,
            31 => Self::RdMsr,
            32 => Self::WrMsr,
            48 => Self::EptViolation,
            49 => Self::EptMisconfiguration,
            55 => Self::Xsetbv,
            _ => Self::Unknown,
        }
    }
}

/// VM-Exit bilgisi
#[derive(Debug, Clone)]
pub struct VmExitInfo {
    /// Exit nedeni
    pub reason: VmExitReason,
    /// Exit qualification (neden detayı)
    pub qualification: u64,
    /// Guest RIP (exit anında)
    pub guest_rip: u64,
    /// Talimat uzunluğu
    pub instruction_length: u32,
    /// Guest fiziksel adres (EPT violation'da)
    pub guest_physical_addr: u64,
}

/// vCPU yapısı — sanal işlemci
#[derive(Debug, Clone)]
pub struct VCpu {
    /// vCPU ID
    pub id: u32,
    /// VMCS bölge adresi (4KB-aligned physical)
    pub vmcs_region: u64,
    /// VPID (Virtual Processor ID) — TLB tag'i
    pub vpid: u16,
    /// vCPU durumu
    pub state: VCpuState,
    /// Toplam VM-Exit sayısı
    pub exit_count: u64,
    /// Son exit nedeni
    pub last_exit: Option<VmExitReason>,
    /// Guest register durumu
    pub regs: VCpuRegisters,
}

/// vCPU durumları
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VCpuState {
    /// Oluşturuldu ama başlatılmadı
    Created,
    /// Çalışıyor (VMRESUME/VMLAUNCH sonrası)
    Running,
    /// Durduruldu
    Stopped,
    /// Init bekleniyor (SIPI bekleniyor)
    WaitingForSipi,
}

/// vCPU register dosyası
#[derive(Debug, Clone, Default)]
pub struct VCpuRegisters {
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rsp: u64,
    pub rbp: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rip: u64,
    pub rflags: u64,
    pub cr0: u64,
    pub cr3: u64,
    pub cr4: u64,
}

impl VCpu {
    /// Yeni vCPU oluşturur.
    pub fn new(id: u32, vpid: u16) -> Self {
        Self {
            id,
            vmcs_region: 0,
            vpid,
            state: VCpuState::Created,
            exit_count: 0,
            last_exit: None,
            regs: VCpuRegisters::default(),
        }
    }

    /// VM-Exit işler ve uygun eylemi belirler.
    pub fn handle_exit(&mut self, info: &VmExitInfo) -> VmExitAction {
        self.exit_count += 1;
        self.last_exit = Some(info.reason);

        match info.reason {
            VmExitReason::Cpuid => {
                // CPUID emülasyonu: leaf EAX, subleaf ECX'ten
                let _leaf = self.regs.rax as u32;
                let _subleaf = self.regs.rcx as u32;
                // Emülasyon sonrası RIP'i ilerlet
                VmExitAction::AdvanceRip(info.instruction_length)
            }
            VmExitReason::Hlt => {
                // HLT — vCPU'yu durdur, kesme bekle
                VmExitAction::Halt
            }
            VmExitReason::Vmcall => {
                // Hipervizör çağrısı (paravirtualization)
                let _call_nr = self.regs.rax;
                VmExitAction::AdvanceRip(info.instruction_length)
            }
            VmExitReason::IoInstruction => {
                let port = (info.qualification >> 16) as u16;
                let _size = (info.qualification & 0x7) as u8 + 1;
                let is_in = (info.qualification & 0x8) != 0;
                VmExitAction::EmulateIo { port, is_in }
            }
            VmExitReason::RdMsr => {
                let _msr = self.regs.rcx as u32;
                VmExitAction::AdvanceRip(info.instruction_length)
            }
            VmExitReason::WrMsr => {
                let _msr = self.regs.rcx as u32;
                let _value = ((self.regs.rdx as u64) << 32) | (self.regs.rax as u64 & 0xFFFF_FFFF);
                VmExitAction::AdvanceRip(info.instruction_length)
            }
            VmExitReason::EptViolation => {
                // EPT ihlali — sayfa hatası gibi; bellek eşleme gerekli
                VmExitAction::HandleEptViolation {
                    guest_phys: info.guest_physical_addr,
                }
            }
            VmExitReason::TripleFault => {
                crate::serial_println!("[KVM] vCPU {} triple fault!", self.id);
                VmExitAction::Shutdown
            }
            VmExitReason::ExternalInterrupt => {
                // Harici kesme — host'a yönlendir
                VmExitAction::InjectInterrupt
            }
            _ => {
                crate::serial_println!("[KVM] vCPU {} unhandled exit: {:?}", self.id, info.reason);
                VmExitAction::Shutdown
            }
        }
    }
}

/// VM-Exit sonrası alınacak eylem
#[derive(Debug, Clone, Copy)]
pub enum VmExitAction {
    /// RIP'i belirtilen uzunluk kadar ilerlet ve VMRESUME
    AdvanceRip(u32),
    /// vCPU'yu durdur (HLT)
    Halt,
    /// I/O emülasyonu
    EmulateIo { port: u16, is_in: bool },
    /// EPT ihlalini çöz
    HandleEptViolation { guest_phys: u64 },
    /// Harici kesmeyi host'a yönlendir
    InjectInterrupt,
    /// VM'yi kapat
    Shutdown,
}

/// VPID yöneticisi — benzersiz VPID atar.
static NEXT_VPID: core::sync::atomic::AtomicU16 = core::sync::atomic::AtomicU16::new(1);

/// Yeni VPID tahsis eder.
pub fn allocate_vpid() -> u16 {
    let vpid = NEXT_VPID.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    if vpid == 0 {
        NEXT_VPID.fetch_add(1, core::sync::atomic::Ordering::Relaxed)
    } else {
        vpid
    }
}

/// INVVPID talimatı — VPID tabanlı TLB temizleme.
///
/// Belirli bir VPID'nin TLB girişlerini geçersiz kılar.
pub fn invvpid_single_context(vpid: u16) {
    if vpid == 0 {
        return;
    }
    // INVVPID type=1 (single-context invalidation)
    // Descriptor: [VPID:16][Rsvd:48][LinearAddr:64]
    let descriptor: [u64; 2] = [vpid as u64, 0];
    unsafe {
        core::arch::asm!(
            "invvpid {0}, [{1}]",
            in(reg) 1u64, // type = single-context
            in(reg) &descriptor as *const _ as u64,
            options(nostack, preserves_flags)
        );
    }
}

// ============================================================================
// NESTED PAGING (EPT/NPT) IMPLEMENTATION
// ============================================================================

use alloc::sync::Arc;

/// VM Exit Reasons
pub const EXIT_REASON_EPT_VIOLATION: u32 = 48;
pub const EXIT_REASON_EPT_MISCONFIG: u32 = 49;

/// Nested Paging Hatası
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NestedPagingError {
    InvalidGuestPhysicalAddress,
    InvalidHostPhysicalAddress,
    PageTableFull,
    InvalidPageSize,
    AccessViolation,
    Misconfiguration,
    UnsupportedFeature,
    OutOfMemory,
}

/// EPT/NPT Sanal Makine için Sayfa Tablosu
#[derive(Debug)]
pub struct VmNestedPageTable {
    pub vm_id: u32,
    pub ept_pointer: u64,
    pub page_mappings: Mutex<BTreeMap<u64, u64>>,
    pub large_page_mappings: Mutex<BTreeMap<u64, u64>>,
}

impl VmNestedPageTable {
    pub fn new(vm_id: u32) -> Self {
        Self {
            vm_id,
            ept_pointer: 0,
            page_mappings: Mutex::new(BTreeMap::new()),
            large_page_mappings: Mutex::new(BTreeMap::new()),
        }
    }

    /// 4KB sayfa eşlemesi oluşturur
    pub fn map_page_4k(
        &self,
        guest_phys_addr: u64,
        host_phys_addr: u64,
        read: bool,
        write: bool,
        execute: bool,
        mem_type: u64,
    ) -> Result<(), NestedPagingError> {
        if guest_phys_addr & 0xFFF != 0 || host_phys_addr & 0xFFF != 0 {
            return Err(NestedPagingError::InvalidGuestPhysicalAddress);
        }

        // Gerçek uygulamada: EPT tabloları oluşturulur ve yapılandırılır
        self.page_mappings
            .lock()
            .insert(guest_phys_addr, host_phys_addr);

        crate::serial_println!(
            "[EPT] VM {} mapped GPA 0x{:x} -> HPA 0x{:x}",
            self.vm_id,
            guest_phys_addr,
            host_phys_addr
        );

        Ok(())
    }

    /// 2MB büyük sayfa eşlemesi oluşturur
    pub fn map_page_2m(
        &self,
        guest_phys_addr: u64,
        host_phys_addr: u64,
        read: bool,
        write: bool,
        execute: bool,
        mem_type: u64,
    ) -> Result<(), NestedPagingError> {
        if guest_phys_addr & 0x1FFFFF != 0 || host_phys_addr & 0x1FFFFF != 0 {
            return Err(NestedPagingError::InvalidGuestPhysicalAddress);
        }

        self.large_page_mappings
            .lock()
            .insert(guest_phys_addr, host_phys_addr);

        crate::serial_println!(
            "[EPT] VM {} mapped 2MB GPA 0x{:x} -> HPA 0x{:x}",
            self.vm_id,
            guest_phys_addr,
            host_phys_addr
        );

        Ok(())
    }

    /// EPT ihlali işleme
    pub fn handle_ept_violation(
        &self,
        guest_linear_addr: u64,
        guest_phys_addr: u64,
        qualification: u64,
    ) -> Result<(), NestedPagingError> {
        let read_violation = (qualification & 1) != 0;
        let write_violation = (qualification & 2) != 0;
        let exec_violation = (qualification & 4) != 0;

        crate::serial_println!(
            "[EPT] Violation in VM {}: GPA=0x{:x}, GLA=0x{:x}, R={} W={} X={}",
            self.vm_id,
            guest_phys_addr,
            guest_linear_addr,
            read_violation,
            write_violation,
            exec_violation
        );

        // İhlali çöz - gerçek uygulamada: doğru eşleme yapılır
        if read_violation || write_violation || exec_violation {
            self.map_page_4k(
                guest_phys_addr & !0xFFF,
                guest_phys_addr & !0xFFF,
                true,
                false,
                true,
                EPT_MEM_TYPE_WB,
            )?;
        }

        Ok(())
    }

    /// EPT pointer'ı döndürür
    pub fn get_ept_pointer(&self) -> u64 {
        self.ept_pointer
    }
}

static NESTED_PAGING_MANAGER: spin::Once<Mutex<BTreeMap<u32, Arc<VmNestedPageTable>>>> =
    spin::Once::new();

/// Nested paging sistemini başlatır
pub fn init_nested_paging() -> Result<(), NestedPagingError> {
    NESTED_PAGING_MANAGER.call_once(|| Mutex::new(BTreeMap::new()));
    crate::serial_println!("[EPT/NPT] Nested Paging Manager initialized");
    Ok(())
}

/// Sanal makine için nested page table oluşturur
pub fn create_vm_page_table(vm_id: u32) -> Result<Arc<VmNestedPageTable>, NestedPagingError> {
    let manager = NESTED_PAGING_MANAGER.get().unwrap();
    let page_table = Arc::new(VmNestedPageTable::new(vm_id));
    manager.lock().insert(vm_id, page_table.clone());

    crate::serial_println!("[EPT/NPT] Created page table for VM {}", vm_id);
    Ok(page_table)
}

/// Sanal makinenin page table'ını alır
pub fn get_vm_page_table(vm_id: u32) -> Option<Arc<VmNestedPageTable>> {
    let manager = NESTED_PAGING_MANAGER.get()?;
    manager.lock().get(&vm_id).cloned()
}

/// EPT ihlalini işler
pub fn handle_vmx_exit(
    exit_reason: u32,
    guest_linear_addr: u64,
    guest_phys_addr: u64,
    qualification: u64,
    vm_id: u32,
) -> Result<bool, NestedPagingError> {
    match exit_reason {
        EXIT_REASON_EPT_VIOLATION => {
            if let Some(page_table) = get_vm_page_table(vm_id) {
                page_table.handle_ept_violation(
                    guest_linear_addr,
                    guest_phys_addr,
                    qualification,
                )?;
                Ok(true)
            } else {
                Err(NestedPagingError::InvalidGuestPhysicalAddress)
            }
        }
        EXIT_REASON_EPT_MISCONFIG => {
            crate::serial_println!("[EPT] Misconfiguration in VM {}", vm_id);
            Err(NestedPagingError::Misconfiguration)
        }
        _ => Ok(false),
    }
}
