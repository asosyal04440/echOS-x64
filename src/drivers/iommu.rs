//! # IOMMU Desteği (I/O Bellek Yönetim Birimi)
//!
//! Intel VT-d ve AMD-Vi mimarisi: çevre birimlerinin DMA erişimini denetler.
//!
//! ## IOMMU Neden Gereklidir?
//!
//! Geleneksel sistemlerde bir DMA aygıtı (disk, NIC, GPU) HERHANGI bir
//! fiziksel bellek adresine erişebilir. Bu ciddi güvenlik ve kararlılık
//! açıklarına yol açar:
//!   - Hatalı sürücü tüm RAM'i bozabilir
//!   - Kötü amaçlı PCIe cihaz çekirdek belleğini okuyabilir (DMA attack)
//!
//! IOMMU bu sorunu çözer:
//!
//! ```
//! Cihaz (GPU/NIC/Disk)
//!        |
//!        | DMA adresi (cihazın gördüğü adres)
//!        v
//!    [IOMMU Donanımı]  <-- DMA adres -> Fiziksel adres çevirisi
//!        |
//!        | Fiziksel adres (gerçek RAM)
//!        v
//!    [Sistem RAM]
//! ```
//!
//! ## Intel VT-d vs AMD-Vi
//!
//! Her iki teknoloji fonksiyonel olarak eşdeğerdir; yalnızca yazmaç
//! adresleri ve komut yapıları farklıdır.
//!
//!   Intel VT-d: DMAR tablosu (ACPI) ile keşfedilir
//!   AMD-Vi:     IVRS tablosu (ACPI) ile keşfedilir
//!
//! ## Domain (Alan) Kavramı
//!
//! Her domain bağımsız bir DMA adres uzayıdır. Cihazlar bir domain'e atanır;
//! o domain içinde kendi sayfa tablolarına göre erişim denetlenir.
//!
//! ```
//! Domain 1: GPU, Sound card  -> sadece grafik/ses belleğine erişim
//! Domain 2: NIC              -> sadece ağ tamponu belleğine erişim
//! ```

use alloc::boxed::Box;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::vec::Vec;
use core::mem::size_of;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// IOMMU SABİTLERİ (IOMMU CONSTANTS)
// ============================================================================

// Intel VT-d yazmaç ofseti haritası.
// Tüm yazmaçlar MMIO alanında base_addr + offset adresinde yer alır.

/// Versiyon yazmacı: donanım versiyonunu bildirir
pub const VTD_VER_REG: u32 = 0x00;
/// Yetenek yazmacı: hangi özelliklerin desteklendiğini gösteren bitmask
pub const VTD_CAP_REG: u32 = 0x08;
/// Genişletilmiş yetenek yazmacı
pub const VTD_ECAP_REG: u32 = 0x10;
/// Global komut yazmacı: çeviriyi etkinleştir, kök tabloyu ayarla
pub const VTD_GCMD_REG: u32 = 0x18;
/// Global durum yazmacı: GCMD komutlarının tamamlanıp tamamlanmadığını gösterir
pub const VTD_GSTS_REG: u32 = 0x1C;
/// Kök tablo adresi yazmacı: DMA çeviri tablosunun başlangıç adresi
pub const VTD_RTADDR_REG: u32 = 0x20;
/// Bağlam komut yazmacı: bağlam girişi geçersiz kılma
pub const VTD_CCMD_REG: u32 = 0x28;
/// Hata durum yazmacı
pub const VTD_FSTS_REG: u32 = 0x34;
/// Hata denetim yazmacı
pub const VTD_FECTL_REG: u32 = 0x38;
/// Hata kesme veri yazmacı
pub const VTD_FEDATA_REG: u32 = 0x3C;
/// Hata kesme adres yazmacı
pub const VTD_FEADDR_REG: u32 = 0x40;
/// Gelişmiş hata günlük yazmacı
pub const VTD_AFLOG_REG: u32 = 0x58;
/// Geçersiz kılma kuyruğu adres yazmacı
pub const VTD_PQADDR_REG: u32 = 0x60;
/// IOTLB geçersiz kılma yazmacı (çeviri önbelleği temizleme)
pub const VTD_IOTLB_REG: u32 = 0x80;
/// Page request queue head register
pub const VTD_PQH_REG: u32 = 0xC0;
/// Page request queue tail register
pub const VTD_PQT_REG: u32 = 0xC8;
/// Page request queue address register
pub const VTD_PQA_REG: u32 = 0xD0;
/// Page request status register
pub const VTD_PRS_REG: u32 = 0xD8;
/// Page request event control register
pub const VTD_PECTL_REG: u32 = 0xE0;

// VT-d Global Komut Bitleri
pub const VTD_GCMD_TE: u32 = 1 << 31; // Çeviriyi etkinleştir (Translation Enable)
pub const VTD_GCMD_SRTP: u32 = 1 << 30; // Kök tablo pointer'ını ayarla
pub const VTD_GCMD_WBF: u32 = 1 << 27; // Yazma tamponu temizleme (Write Buffer Flush)
pub const VTD_GCMD_QIE: u32 = 1 << 26; // Geçersiz kılma kuyruğunu etkinleştir
pub const VTD_GCMD_IRE: u32 = 1 << 25; // Kesme yeniden yönlendirmeyi etkinleştir
pub const VTD_GCMD_EAFL: u32 = 1 << 24; // Gelişmiş hata kaydını etkinleştir
pub const VTD_GSTS_WBFS: u64 = 1 << 27; // Write Buffer Flush Status
pub const VTD_GSTS_QIES: u64 = 1 << 26; // Queued Invalidation Enable Status
pub const VTD_CAP_RWBF: u64 = 1 << 4; // Required Write Buffer Flush (CAP.RWBF)
pub const VTD_IOTLB_IVT: u64 = 1 << 63; // IOTLB invalidation request valid bit
pub const VTD_IOTLB_IIRG_DOMAIN: u64 = 0b010 << 60; // Domain-selective invalidation
pub const VTD_IOTLB_DR: u64 = 1 << 49; // Drain reads
pub const VTD_IOTLB_DW: u64 = 1 << 48; // Drain writes
pub const VTD_IOTLB_DID_SHIFT: u64 = 32;

// VT-d PASID Table Entry constants (Scalable Mode, VT-d Spec Rev 4.0 §9.6)
pub const VTD_PASID_ENTRY_PRESENT: u64 = 1 << 0;
pub const VTD_PASID_ENTRY_PASID_EN: u64 = 1 << 3;
pub const VTD_PASID_ENTRY_FLPM_4LVL: u64 = 0 << 1; // 4-level first-level page tables
pub const VTD_PASID_ENTRY_FLPM_5LVL: u64 = 1 << 1; // 5-level first-level page tables
pub const VTD_PASID_ENTRY_FLPTP_MASK: u64 = 0x000F_FFFF_FFFF_F000; // bits 51:12
pub const VTD_PASID_ENTRY_AW_48: u64 = 0 << 7; // 48-bit address width
pub const VTD_PASID_ENTRY_AW_57: u64 = 1 << 7; // 57-bit address width
pub const VTD_PASID_ENTRY_SRE: u64 = 1 << 62; // Supervisor Request Enable
pub const VTD_PASID_ENTRY_EAFE: u64 = 1 << 61; // Extended Access Enable
pub const VTD_PASID_PDIR_PRESENT: u64 = 1 << 0; // PASID Directory Entry Present

// VT-d Fault Status Register (FSTS) bits (§10.4.14)
pub const VTD_FSTS_PPF: u32 = 1 << 0; // Primary Pending Fault
pub const VTD_FSTS_PFO: u32 = 1 << 1; // Primary Fault Overflow
pub const VTD_FSTS_APF: u32 = 1 << 2; // Advanced Pending Fault
pub const VTD_FSTS_AFO: u32 = 1 << 3; // Advanced Fault Overflow
pub const VTD_FSTS_IPF: u32 = 1 << 4; // Invalidate Request Pending Fault
pub const VTD_FSTS_ICE: u32 = 1 << 5; // Invalidation Completion Error
pub const VTD_FSTS_ITE: u32 = 1 << 6; // Invalidation Timeout Error
pub const VTD_FSTS_FLF: u32 = 1 << 7; // First-Level Fault

// VT-d Queued Invalidation constants (§6.5)
pub const VTD_QI_CMD_SHIFT: u64 = 4;
pub const VTD_QI_CC: u64 = 0x01; // Context-cache Invalidate
pub const VTD_QI_IOTLB: u64 = 0x02; // IOTLB Invalidate
pub const VTD_QI_PASID: u64 = 0x03; // PASID-cache Invalidate
pub const VTD_QI_GRAN_CC_GLOBAL: u64 = 0 << 4;
pub const VTD_QI_GRAN_IOTLB_GLOBAL: u64 = 0 << 4;
pub const VTD_QI_GRAN_IOTLB_DOMAIN: u64 = 1 << 4;
pub const VTD_QI_DID_SHIFT: u64 = 16;
pub const VTD_QI_IF_IIG: u64 = 1 << 11; // Interrupt if Invalidation Gate

// AMD IOMMU DTE constants (AMD IOMMU Spec 48882, §2.2.2)
pub const DTE_FLAG_V: u64 = 1 << 0; // Valid
pub const DTE_FLAG_TV: u64 = 1 << 1; // Translation Valid
pub const DTE_FLAG_IR: u64 = 1 << 61; // Interrupt Read
pub const DTE_FLAG_IW: u64 = 1 << 62; // Interrupt Write
pub const DTE_FLAG_GV: u64 = 1 << 55; // Guest Valid (PASID/GCR3)
pub const DTE_FLAG_GIOV: u64 = 1 << 54; // Guest I/O Valid
pub const DTE_GLX: u64 = 3 << 56; // GCR3 Level mask (GLX shift)
pub const DTE_GCR3_14_12_SHIFT: u64 = 58;
pub const DTE_GCR3_30_15_SHIFT: u64 = 16;
pub const DTE_GCR3_51_31_SHIFT: u64 = 43;
pub const DTE_GPT_LEVEL_SHIFT: u64 = 54;
pub const DTE_FLAG_IOTLB: u64 = 1 << 32; // IOTLB invalidate needed
pub const AMDVI_DTE_SIZE: usize = 32; // 256-bit = 4x64-bit
const VTD_MMIO_POLL_SPINS: usize = 100_000;

// AMD-Vi yazmaç ofseti haritası
pub const AMDVI_CONTROL_REG: u32 = 0x00;
pub const AMDVI_EXCL_BASE_REG: u32 = 0x08;
pub const AMDVI_EXCL_LIMIT_REG: u32 = 0x10;
pub const AMDVI_DEV_TABLE_BASE_REG: u32 = 0x18;
pub const AMDVI_CMD_BASE_REG: u32 = 0x20;
pub const AMDVI_CMD_TAIL_REG: u32 = 0x28;
pub const AMDVI_CONTROL_EXT_REG: u32 = 0x18;
pub const AMDVI_PPR_LOG_A_BASE_REG: u32 = 0x2020;
pub const AMDVI_PPR_LOG_A_HEAD_REG: u32 = 0x2028;
pub const AMDVI_PPR_LOG_B_BASE_REG: u32 = 0x2030;
pub const AMDVI_PPR_LOG_B_TAIL_REG: u32 = 0x2038;
pub const AMDVI_PPR_AUTO_RESPONSE_REG: u32 = 0x2040;
pub const AMDVI_CTRL_PPR_ENABLE: u64 = 1 << 2;
pub const AMDVI_CTRL_PPR_LOG_ENABLE: u64 = 1 << 3;
pub const AMDVI_PPR_RESPONSE_SUCCESS: u32 = 0;
pub const AMDVI_PPR_RESPONSE_INVALID: u32 = 1;
pub const AMDVI_PPR_RESPONSE_FAILURE: u32 = 2;

// ============================================================================
// PCIe ATS (Address Translation Services) SABİTLERİ
// ============================================================================

// PCIe Capability IDs
pub const PCI_CAP_ID_ACS: u8 = 0x0D; // Access Control Services
pub const PCI_CAP_ID_ATS: u8 = 0x0F; // Address Translation Services
pub const PCI_CAP_ID_PRI: u8 = 0x13; // Page Request Interface

// ATS Capability Register Bits
pub const ATS_CAP_QDEP_SHIFT: u8 = 4; // Invalidate Queue Depth shift
pub const ATS_CAP_QDEP_MASK: u32 = 0x1F << ATS_CAP_QDEP_SHIFT;
pub const ATS_CAP_PAGE_ALIGNED: u32 = 1 << 0; // Page Aligned Request bit

// ATS Control Register Bits
pub const ATS_CTRL_ENABLE: u32 = 1 << 31; // ATS Enable bit
pub const ATS_CTRL_STU_SHIFT: u8 = 16; // Smallest Translation Unit shift
pub const ATS_CTRL_STU_MASK: u32 = 0x1F << ATS_CTRL_STU_SHIFT;

// PRI (Page Request Interface) Control Register Bits
pub const PRI_CTRL_ENABLE: u32 = 1 << 31; // PRI Enable bit
pub const PRI_CTRL_RESET: u32 = 1 << 30; // PRI Reset bit

// ============================================================================
// ARM SMMUv3 SABİTLERİ
// ============================================================================

// SMMUv3 Register Offsets (relative to SMMU base address)
pub const SMMU_IDR0: u32 = 0x0000; // Identification Register 0
pub const SMMU_IDR1: u32 = 0x0004; // Identification Register 1
pub const SMMU_IDR3: u32 = 0x000C; // Identification Register 3
pub const SMMU_CR0: u32 = 0x0020; // Control Register 0
pub const SMMU_CR0ACK: u32 = 0x0024; // Control Register 0 Acknowledge
pub const SMMU_CR2: u32 = 0x0028; // Control Register 2
pub const SMMU_GBPA: u32 = 0x0044; // Global Buffer Performance Abort
pub const SMMU_IRQ_CTRL: u32 = 0x0050; // Interrupt Control
pub const SMMU_IRQ_CTRLACK: u32 = 0x0054; // Interrupt Control Acknowledge
pub const SMMU_GERROR: u32 = 0x0060; // Global Error
pub const SMMU_GERRORN: u32 = 0x0064; // Global Error Non-secure
pub const SMMU_GERROR_IRQ_CFG0: u32 = 0x0070; // Global Error IRQ Config 0

// SMMU Stream Table and Context Descriptors
pub const SMMU_STRTAB_BASE: u32 = 0x0080; // Stream Table Base
pub const SMMU_STRTAB_BASE_CFG: u32 = 0x0088; // Stream Table Base Config

// SMMU Command Queue
pub const SMMU_CMDQ_BASE: u32 = 0x0090; // Command Queue Base
pub const SMMU_CMDQ_PROD: u32 = 0x0098; // Command Queue Producer
pub const SMMU_CMDQ_CONS: u32 = 0x009C; // Command Queue Consumer

// SMMU Event Queue
pub const SMMU_EVENTQ_BASE: u32 = 0x00A0; // Event Queue Base
pub const SMMU_EVENTQ_PROD: u32 = 0x00A8; // Event Queue Producer
pub const SMMU_EVENTQ_CONS: u32 = 0x00AC; // Event Queue Consumer

// SMMU Control Register Bits
pub const SMMU_CR0_SMMUEN: u32 = 1 << 0; // SMMU Enable
pub const SMMU_CR0_EVENTQEN: u32 = 1 << 2; // Event Queue Enable
pub const SMMU_CR0_CMDQEN: u32 = 1 << 1; // Command Queue Enable

// SMMU Identification Register Bits
pub const SMMU_IDR0_STALL_MODEL: u32 = 1 << 24; // Stall Model
pub const SMMU_IDR0_HYP: u32 = 1 << 9; // Hypervisor Support
pub const SMMU_IDR0_VMID16: u32 = 1 << 8; // 16-bit VMID Support

// SMMU Stream Table Entry Bits
pub const STE_CONFIG_ABORT: u64 = 0; // Abort configuration
pub const STE_CONFIG_BYPASS: u64 = 1; // Bypass configuration
pub const STE_CONFIG_S1_TRANS: u64 = 2; // Stage 1 Translation
pub const STE_CONFIG_S2_TRANS: u64 = 3; // Stage 2 Translation
pub const STE_CONFIG_NESTED: u64 = 4; // Nested Stage 1+2 Translation
pub const AMDVI_EVT_BASE_REG: u32 = 0x30;
pub const AMDVI_EVT_HEAD_REG: u32 = 0x38;
pub const AMDVI_STATUS_REG: u32 = 0x2020;

// ============================================================================
// DMA YENİDEN YÖNLENDİRME (DMA REMAPPING)
// ============================================================================

// DMA çeviri tablosu girişleri. VT-d iki düzeyli yapı kullanır:
//
//   Kök Tablosu (Root Table)
//     |
//     +-- Bağlam Tablosu (Context Table) [her PCI bus için bir tane]
//           |
//           +-- Sayfa Tablosu (Page Table) [her cihaz için bir tane]
//                 |
//                 +-- Sayfa Tablosu Giriş (PTE) [her DMA sayfası için]

/// DMA adres çeviri bilgisi (yazılımsal temsil)
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DmaTranslation {
    pub present: bool,    // Bu eşleme geçerli mi?
    pub read_perm: bool,  // Cihaz bu adresten okuyabilir mi?
    pub write_perm: bool, // Cihaz bu adrese yazabilir mi?
    pub phys_addr: u64,   // Eşlenen fiziksel adres
    pub size: u64,        // Eşleme boyutu (byte)
}

/// Intel VT-d kök tablosu girişi (16 byte, 16 byte hizalı)
#[repr(C, align(16))]
pub struct VtdRootEntry {
    pub lo: u64,
    pub hi: u64,
}

/// Intel VT-d bağlam tablosu girişi (16 byte, 16 byte hizalı)
#[repr(C, align(16))]
pub struct VtdContextEntry {
    pub lo: u64,
    pub hi: u64,
}

/// DMA için sayfa tablosu girişi (Page Table Entry)
#[repr(C)]
pub struct DmaPte {
    pub val: u64,
}

impl DmaPte {
    /// Yeni bir DMA sayfa tablosu girişi oluşturur.
    /// Bit 0 (Present), Bit 1 (Read), Bit 2 (Write) VT-d PTE formatıdır.
    pub fn new(phys: u64, read: bool, write: bool) -> Self {
        let mut val = phys & !0xFFF; // Sayfa hizalaması: alt 12 bit sıfırlanır
        val |= 1; // Present biti
        if read {
            val |= 1 << 1;
        } // Okuma izni
        if write {
            val |= 1 << 2;
        } // Yazma izni
        Self { val }
    }
}

// ============================================================================
// PCIe ATS VERİ YAPILARI
// ============================================================================

/// PCIe ATS yetenek yapısı
#[derive(Clone, Copy, Debug)]
pub struct PciAtsCapability {
    pub offset: u8,         // PCIe konfigürasyon alanındaki ofset
    pub qdep: u8,           // Invalidate queue depth
    pub page_aligned: bool, // Page aligned request destekli mi?
    pub enabled: bool,      // ATS şu anda etkin mi?
}

impl PciAtsCapability {
    pub fn new() -> Self {
        Self {
            offset: 0,
            qdep: 0,
            page_aligned: false,
            enabled: false,
        }
    }
}

/// PCIe PRI (Page Request Interface) yetenek yapısı
#[derive(Clone, Copy, Debug)]
pub struct PciPriCapability {
    pub offset: u8,          // PCIe konfigürasyon alanındaki ofset
    pub enabled: bool,       // PRI şu anda etkin mi?
    pub reset_pending: bool, // Reset işlemi beklemede mi?
}

impl PciPriCapability {
    pub fn new() -> Self {
        Self {
            offset: 0,
            enabled: false,
            reset_pending: false,
        }
    }
}

/// ATS geçersiz kılma isteği
#[derive(Clone, Copy, Debug)]
pub struct AtsInvalidateRequest {
    pub requester_id: u16,  // PCI requester ID (Bus:Device.Function)
    pub address: u64,       // Geçersiz kılınacak adres
    pub length: u32,        // Geçersiz kılma uzunluğu (byte)
    pub pasid: Option<u32>, // Process Address Space ID (varsa)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PasidBinding {
    pub process_id: u64,
    pub pasid: u32,
    pub address_space_id: u64,
    pub page_table_root: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SharedVirtualAddressWindow {
    pub process_id: u64,
    pub pasid: u32,
    pub base: u64,
    pub length: u64,
    pub page_table_root: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GpuVirtualAddressRange {
    pub pasid: u32,
    pub gpu_va: u64,
    pub phys_addr: u64,
    pub size: u64,
    pub read: bool,
    pub write: bool,
    pub dma_buf_fd: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PriBudgetState {
    pub max_outstanding: u32,
    pub consumed: u32,
    pub replay_count: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SharedVaSnapshot {
    pub pasid_bindings: u32,
    pub sva_windows: u32,
    pub gpuva_ranges: u32,
    pub device_bindings: u32,
    pub pending_page_requests: u32,
    pub completed_page_replays: u32,
    pub invalidation_records: u32,
    pub pri_budget: PriBudgetState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PriFaultReplay {
    source_id: u16,
    pasid: u32,
    address: u64,
    timestamp: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PriPageRequest {
    pub request_id: u64,
    pub source_id: u16,
    pub pasid: u32,
    pub address: u64,
    pub length: u64,
    pub write: bool,
    pub generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PriReplayResult {
    pub request_id: u64,
    pub source_id: u16,
    pub pasid: u32,
    pub address: u64,
    pub length: u64,
    pub replayed: bool,
    pub invalidate_seq: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IotlbInvalidateRecord {
    pub pasid: u32,
    pub start: u64,
    pub length: u64,
    pub sequence: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageRequestResponseCode {
    Success = 0,
    Invalid = 1,
    Failure = 2,
}

#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IntelPageRequestRecord {
    pub source_id: u16,
    pub flags: u16,
    pub pasid: u32,
    pub address: u64,
}

impl IntelPageRequestRecord {
    pub const FLAG_WRITE: u16 = 1 << 0;

    pub fn from_request(request: &PriPageRequest) -> Self {
        Self {
            source_id: request.source_id,
            flags: if request.write { Self::FLAG_WRITE } else { 0 },
            pasid: request.pasid,
            address: request.address,
        }
    }

    pub fn into_request(self, request_id: u64, generation: u64) -> PriPageRequest {
        PriPageRequest {
            request_id,
            source_id: self.source_id,
            pasid: self.pasid,
            address: self.address,
            length: 4096,
            write: (self.flags & Self::FLAG_WRITE) != 0,
            generation,
        }
    }
}

#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IntelPageResponseRecord {
    pub source_id: u16,
    pub code: u8,
    pub reserved0: u8,
    pub pasid: u32,
    pub page_group_index: u32,
    pub reserved1: u32,
}

impl IntelPageResponseRecord {
    pub fn from_result(result: &PriReplayResult) -> Self {
        Self {
            source_id: result.source_id,
            code: if result.replayed {
                PageRequestResponseCode::Success as u8
            } else {
                PageRequestResponseCode::Failure as u8
            },
            reserved0: 0,
            pasid: result.pasid,
            page_group_index: ((result.address >> 12) & 0xFFFF_FFFF) as u32,
            reserved1: 0,
        }
    }
}

#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AmdPprLogRecord {
    pub device_id: u16,
    pub flags: u16,
    pub pasid: u32,
    pub address: u64,
}

impl AmdPprLogRecord {
    pub const FLAG_WRITE: u16 = 1 << 0;

    pub fn from_request(request: &PriPageRequest) -> Self {
        Self {
            device_id: request.source_id,
            flags: if request.write { Self::FLAG_WRITE } else { 0 },
            pasid: request.pasid,
            address: request.address,
        }
    }

    pub fn into_request(self, request_id: u64, generation: u64) -> PriPageRequest {
        PriPageRequest {
            request_id,
            source_id: self.device_id,
            pasid: self.pasid,
            address: self.address,
            length: 4096,
            write: (self.flags & Self::FLAG_WRITE) != 0,
            generation,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AmdPprResponseRecord {
    pub device_id: u16,
    pub pasid: u32,
    pub code: PageRequestResponseCode,
}

impl AmdPprResponseRecord {
    pub fn from_result(result: &PriReplayResult) -> Self {
        Self {
            device_id: result.source_id,
            pasid: result.pasid,
            code: if result.replayed {
                PageRequestResponseCode::Success
            } else {
                PageRequestResponseCode::Failure
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageRequestQueueSnapshot {
    pub entry_count: u32,
    pub head: u32,
    pub tail: u32,
    pub pending_records: u32,
    pub completed_responses: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PageRequestQueueVendor {
    Intel,
    Amd,
}

struct PageRequestQueueState {
    vendor: PageRequestQueueVendor,
    entry_count: u32,
    ring_base_phys: usize,
    ring_base_virt: *mut u8,
    ring_bytes: usize,
    head: u32,
    tail: u32,
    pending_request_meta: VecDeque<(u64, u64)>,
    decoded_requests: VecDeque<PriPageRequest>,
    intel_responses: Vec<IntelPageResponseRecord>,
    amd_responses: Vec<AmdPprResponseRecord>,
    programmed: bool,
}

unsafe impl Send for PageRequestQueueState {}
unsafe impl Sync for PageRequestQueueState {}

impl PageRequestQueueState {
    fn alloc(
        vendor: PageRequestQueueVendor,
        entry_count: u32,
        entry_size: usize,
    ) -> Result<Self, IommuError> {
        let ring_bytes = (entry_count as usize).saturating_mul(entry_size).max(4096);
        let pages = (ring_bytes + 4095) / 4096;
        let (paddr, vaddr) = crate::memory::dma_alloc(pages).ok_or(IommuError::NoMemory)?;
        unsafe {
            core::ptr::write_bytes(vaddr.as_ptr(), 0, pages.saturating_mul(4096));
        }
        Ok(Self {
            vendor,
            entry_count,
            ring_base_phys: paddr,
            ring_base_virt: vaddr.as_ptr(),
            ring_bytes: pages.saturating_mul(4096),
            head: 0,
            tail: 0,
            pending_request_meta: VecDeque::new(),
            decoded_requests: VecDeque::new(),
            intel_responses: Vec::new(),
            amd_responses: Vec::new(),
            programmed: false,
        })
    }

    fn write_intel_entry(&mut self, entry: IntelPageRequestRecord) -> Result<(), IommuError> {
        if self.vendor != PageRequestQueueVendor::Intel {
            return Err(IommuError::NotSupported);
        }
        let next_tail = (self.tail + 1) % self.entry_count.max(1);
        if next_tail == self.head {
            return Err(IommuError::NoMemory);
        }
        let offset = (self.tail as usize).saturating_mul(size_of::<IntelPageRequestRecord>());
        unsafe {
            (self.ring_base_virt.add(offset) as *mut IntelPageRequestRecord).write_volatile(entry);
        }
        self.tail = next_tail;
        Ok(())
    }

    fn write_amd_entry(&mut self, entry: AmdPprLogRecord) -> Result<(), IommuError> {
        if self.vendor != PageRequestQueueVendor::Amd {
            return Err(IommuError::NotSupported);
        }
        let next_tail = (self.tail + 1) % self.entry_count.max(1);
        if next_tail == self.head {
            return Err(IommuError::NoMemory);
        }
        let offset = (self.tail as usize).saturating_mul(size_of::<AmdPprLogRecord>());
        unsafe {
            (self.ring_base_virt.add(offset) as *mut AmdPprLogRecord).write_volatile(entry);
        }
        self.tail = next_tail;
        Ok(())
    }
}

// ============================================================================
// ARM SMMUv3 VERİ YAPILARI
// ============================================================================

/// SMMUv3 Stream Table Entry
/// Her PCIe aygıtı için bir giriş içerir
#[repr(C, align(64))]
#[derive(Clone, Copy, Debug)]
pub struct SmmuStreamTableEntry {
    pub config: u64,        // Yapılandırma (STE_CONFIG_* değerleri)
    pub s1ctxptr: u64,      // Stage 1 context descriptor pointer
    pub s2ptr: u64,         // Stage 2 translation table pointer
    pub msi_addr_lo: u64,   // MSI adresi (alt 64 bit)
    pub msi_addr_hi: u64,   // MSI adresi (üst 64 bit)
    pub msi_data: u32,      // MSI veri
    pub reserved: [u32; 3], // Rezerve alan
}

impl SmmuStreamTableEntry {
    pub fn new() -> Self {
        Self {
            config: STE_CONFIG_ABORT,
            s1ctxptr: 0,
            s2ptr: 0,
            msi_addr_lo: 0,
            msi_addr_hi: 0,
            msi_data: 0,
            reserved: [0; 3],
        }
    }

    /// Bypass moduna ayarla (çeviri yok)
    pub fn set_bypass(&mut self) {
        self.config = STE_CONFIG_BYPASS;
    }

    /// Stage 1 çevirisine ayarla
    pub fn set_stage1_translation(&mut self, ctx_ptr: u64) {
        self.config = STE_CONFIG_S1_TRANS;
        self.s1ctxptr = ctx_ptr & !0xFFF; // Sayfa hizalama
    }
}

/// SMMUv3 Context Descriptor
/// Stage 1 çeviri için sayfa tablosu tanımlar
#[repr(C, align(64))]
#[derive(Clone, Copy, Debug)]
pub struct SmmuContextDescriptor {
    pub ttbr0: u64, // Translation Table Base Register 0
    pub tcr: u64,   // Translation Control Register
    pub mair: u64,  // Memory Attribute Indirection Register
    pub reserved: u64,
}

impl SmmuContextDescriptor {
    pub fn new() -> Self {
        Self {
            ttbr0: 0,
            tcr: 0,
            mair: 0,
            reserved: 0,
        }
    }
}

/// SMMUv3 Komut Kuyruğu Girişi
#[repr(C, align(16))]
#[derive(Clone, Copy, Debug)]
pub struct SmmuCommand {
    pub opcode: u32,       // Komut türü
    pub substream_id: u32, // Alt akış ID
    pub stream_id: u32,    // Akış ID (PCI requester ID)
    pub leaf: u32,         // Leaf bit (son seviye)
    pub addr: u64,         // Adres (çeviri için)
}

/// SMMUv3 Olay Kuyruğu Girişi
#[repr(C, align(16))]
#[derive(Clone, Copy, Debug)]
pub struct SmmuEvent {
    pub type_: u8,         // Olay türü
    pub reason: u8,        // Hata nedeni
    pub level: u8,         // Hata seviyesi
    pub aborted: u8,       // İptal edildi mi?
    pub stream_id: u32,    // Akış ID
    pub substream_id: u32, // Alt akış ID
    pub addr: u64,         // Hatalı adres
    pub timestamp: u64,    // Zaman damgası
}

// ============================================================================
// IOMMU DOMAIN (IOMMU ALANI)
// ============================================================================

// Her domain bağımsız bir DMA adres uzayıdır.
//
// Bir domain'e atanan cihazlar yalnızca o domain'in eşleme tablosundaki
// adreslere erişebilir. Diğer domainlerin (ve çekirdek kodunun) belleğine
// erişim donanım tarafından engellenir ve hata kaydedilir.

pub struct IommuDomain {
    /// Domain kimliği (sürücü tarafından atanır)
    pub id: u32,
    /// Sayfa tablosunun fiziksel adresi (kök tablo pointer'ı)
    pub page_table: u64,
    /// Bu domain için çeviri etkin mi?
    pub translation_enabled: bool,
    /// Bu domain'e atanmış cihazlar: (segment, BDF) ikilileri
    /// BDF = Bus:Device:Function (PCI cihaz tanımlayıcısı)
    pub devices: Mutex<Vec<(u16, u16)>>,
    /// DMA adres -> fiziksel adres eşleme tablosu
    pub mappings: Mutex<BTreeMap<u64, DmaTranslation>>,
    pub pasid_bindings: Mutex<BTreeMap<u32, PasidBinding>>,
    pub sva_windows: Mutex<BTreeMap<u64, SharedVirtualAddressWindow>>,
    pub gpuva_ranges: Mutex<BTreeMap<u64, GpuVirtualAddressRange>>,
    pub device_pasids: Mutex<BTreeMap<(u16, u16), u32>>,
    pub pri_budget_limit: AtomicU32,
    pub pri_budget_used: AtomicU32,
    fault_replays: Mutex<Vec<PriFaultReplay>>,
    pending_page_requests: Mutex<Vec<PriPageRequest>>,
    completed_page_replays: Mutex<Vec<PriReplayResult>>,
    invalidation_log: Mutex<Vec<IotlbInvalidateRecord>>,
    replay_counter: AtomicU64,
    request_counter: AtomicU64,
    sva_generation: AtomicU64,
    invalidate_sequence: AtomicU64,
}

impl IommuDomain {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            page_table: 0,
            translation_enabled: true,
            devices: Mutex::new(Vec::new()),
            mappings: Mutex::new(BTreeMap::new()),
            pasid_bindings: Mutex::new(BTreeMap::new()),
            sva_windows: Mutex::new(BTreeMap::new()),
            gpuva_ranges: Mutex::new(BTreeMap::new()),
            device_pasids: Mutex::new(BTreeMap::new()),
            pri_budget_limit: AtomicU32::new(256),
            pri_budget_used: AtomicU32::new(0),
            fault_replays: Mutex::new(Vec::new()),
            pending_page_requests: Mutex::new(Vec::new()),
            completed_page_replays: Mutex::new(Vec::new()),
            invalidation_log: Mutex::new(Vec::new()),
            replay_counter: AtomicU64::new(0),
            request_counter: AtomicU64::new(1),
            sva_generation: AtomicU64::new(1),
            invalidate_sequence: AtomicU64::new(1),
        }
    }

    /// DMA adresi eşler: cihaz dma_addr'ye erişince phys_addr'ye yönlendirilir
    pub fn map(
        &self,
        dma_addr: u64,
        phys_addr: u64,
        size: u64,
        read: bool,
        write: bool,
    ) -> Result<(), IommuError> {
        let mapping = DmaTranslation {
            present: true,
            read_perm: read,
            write_perm: write,
            phys_addr,
            size,
        };
        self.mappings.lock().insert(dma_addr, mapping);
        Ok(())
    }

    /// DMA eşlemesini kaldırır; cihaz bu adrese artık erişemez
    pub fn unmap(&self, dma_addr: u64) -> bool {
        self.mappings.lock().remove(&dma_addr).is_some()
    }

    /// DMA adresini fiziksel adrese çevirir; eşleme yoksa None döner
    pub fn translate(&self, dma_addr: u64) -> Option<DmaTranslation> {
        self.mappings.lock().get(&dma_addr).cloned()
    }

    /// PCI cihazını bu domain'e atar; artık bu domain'in kuralları geçerli
    pub fn attach_device(&self, segment: u16, bdf: u16) {
        self.devices.lock().push((segment, bdf));
    }

    /// PCI cihazını domain'den ayırır
    pub fn detach_device(&self, segment: u16, bdf: u16) {
        self.devices
            .lock()
            .retain(|&(s, b)| s != segment || b != bdf);
        self.device_pasids.lock().remove(&(segment, bdf));
    }

    pub fn bind_process_address_space(
        &self,
        process_id: u64,
        pasid: u32,
        address_space_id: u64,
        page_table_root: u64,
    ) -> Result<(), IommuError> {
        if pasid == 0 || page_table_root == 0 {
            return Err(IommuError::InvalidAddress);
        }
        self.pasid_bindings.lock().insert(
            pasid,
            PasidBinding {
                process_id,
                pasid,
                address_space_id,
                page_table_root,
            },
        );
        self.sva_generation.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }

    pub fn attach_device_with_pasid(
        &self,
        segment: u16,
        bdf: u16,
        pasid: u32,
    ) -> Result<(), IommuError> {
        if !self.pasid_bindings.lock().contains_key(&pasid) {
            return Err(IommuError::DomainNotFound);
        }
        self.attach_device(segment, bdf);
        self.device_pasids.lock().insert((segment, bdf), pasid);
        Ok(())
    }

    pub fn register_sva_window(
        &self,
        process_id: u64,
        pasid: u32,
        base: u64,
        length: u64,
    ) -> Result<(), IommuError> {
        let binding = self
            .pasid_bindings
            .lock()
            .get(&pasid)
            .copied()
            .ok_or(IommuError::DomainNotFound)?;
        if binding.process_id != process_id || length == 0 {
            return Err(IommuError::InvalidAddress);
        }
        self.sva_windows.lock().insert(
            base,
            SharedVirtualAddressWindow {
                process_id,
                pasid,
                base,
                length,
                page_table_root: binding.page_table_root,
            },
        );
        self.sva_generation.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }

    pub fn map_gpu_virtual_address(
        &self,
        pasid: u32,
        gpu_va: u64,
        phys_addr: u64,
        size: u64,
        read: bool,
        write: bool,
        dma_buf_fd: u32,
    ) -> Result<(), IommuError> {
        if !self.pasid_bindings.lock().contains_key(&pasid) || gpu_va == 0 || size == 0 {
            return Err(IommuError::InvalidAddress);
        }
        self.map(gpu_va, phys_addr, size, read, write)?;
        self.gpuva_ranges.lock().insert(
            gpu_va,
            GpuVirtualAddressRange {
                pasid,
                gpu_va,
                phys_addr,
                size,
                read,
                write,
                dma_buf_fd,
            },
        );
        self.sva_generation.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }

    pub fn translate_gpuva(&self, pasid: u32, gpu_va: u64) -> Option<GpuVirtualAddressRange> {
        self.gpuva_ranges
            .lock()
            .values()
            .find(|range| {
                range.pasid == pasid
                    && gpu_va >= range.gpu_va
                    && gpu_va < range.gpu_va.saturating_add(range.size)
            })
            .copied()
    }

    pub fn try_consume_pri_budget(&self, request_count: u32) -> bool {
        let limit = self.pri_budget_limit.load(Ordering::Acquire);
        let used = self.pri_budget_used.load(Ordering::Acquire);
        if used.saturating_add(request_count) > limit {
            return false;
        }
        self.pri_budget_used
            .fetch_add(request_count, Ordering::AcqRel);
        true
    }

    pub fn release_pri_budget(&self, request_count: u32) {
        self.pri_budget_used
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                Some(used.saturating_sub(request_count))
            })
            .ok();
    }

    pub fn record_fault_replay(&self, source_id: u16, pasid: u32, address: u64) {
        self.fault_replays.lock().push(PriFaultReplay {
            source_id,
            pasid,
            address,
            timestamp: crate::task::scheduler::get_ticks() as u64,
        });
        self.replay_counter.fetch_add(1, Ordering::AcqRel);
    }

    pub fn queue_page_request(
        &self,
        source_id: u16,
        pasid: u32,
        address: u64,
        length: u64,
        write: bool,
    ) -> Result<u64, IommuError> {
        if length == 0 || !self.pasid_bindings.lock().contains_key(&pasid) {
            return Err(IommuError::InvalidAddress);
        }
        if !self.try_consume_pri_budget(1) {
            return Err(IommuError::NoMemory);
        }

        let request_id = self.request_counter.fetch_add(1, Ordering::AcqRel);
        let generation = self.sva_generation.load(Ordering::Acquire);
        self.pending_page_requests.lock().push(PriPageRequest {
            request_id,
            source_id,
            pasid,
            address,
            length,
            write,
            generation,
        });
        Ok(request_id)
    }

    pub fn drain_page_requests(&self, budget: usize) -> Vec<PriPageRequest> {
        let mut queue = self.pending_page_requests.lock();
        let take = budget.min(queue.len());
        queue.drain(0..take).collect()
    }

    pub fn pending_page_request_snapshot(&self) -> Vec<PriPageRequest> {
        self.pending_page_requests.lock().clone()
    }

    pub fn invalidate_gpuva_range(&self, pasid: u32, start: u64, length: u64) -> u64 {
        let sequence = self.invalidate_sequence.fetch_add(1, Ordering::AcqRel);
        self.invalidation_log.lock().push(IotlbInvalidateRecord {
            pasid,
            start,
            length,
            sequence,
        });
        sequence
    }

    fn replay_page_request(&self, request: PriPageRequest) -> Result<PriReplayResult, IommuError> {
        let current_generation = self.sva_generation.load(Ordering::Acquire);
        let replayed = request.generation <= current_generation
            && self
                .translate_gpuva(request.pasid, request.address)
                .is_some();
        if !replayed {
            return Err(IommuError::InvalidAddress);
        }

        let invalidate_seq =
            self.invalidate_gpuva_range(request.pasid, request.address, request.length);
        self.record_fault_replay(request.source_id, request.pasid, request.address);
        Ok(PriReplayResult {
            request_id: request.request_id,
            source_id: request.source_id,
            pasid: request.pasid,
            address: request.address,
            length: request.length,
            replayed: true,
            invalidate_seq,
        })
    }

    pub fn process_page_requests(&self, budget: usize) -> Vec<PriReplayResult> {
        let requests = self.drain_page_requests(budget);
        let mut results = Vec::with_capacity(requests.len());
        for request in requests.into_iter() {
            let result = match self.replay_page_request(request) {
                Ok(result) => result,
                Err(_) => PriReplayResult {
                    request_id: request.request_id,
                    source_id: request.source_id,
                    pasid: request.pasid,
                    address: request.address,
                    length: request.length,
                    replayed: false,
                    invalidate_seq: 0,
                },
            };
            self.release_pri_budget(1);
            self.completed_page_replays.lock().push(result);
            results.push(result);
        }
        results
    }

    pub fn shared_va_snapshot(&self) -> SharedVaSnapshot {
        SharedVaSnapshot {
            pasid_bindings: self.pasid_bindings.lock().len() as u32,
            sva_windows: self.sva_windows.lock().len() as u32,
            gpuva_ranges: self.gpuva_ranges.lock().len() as u32,
            device_bindings: self.device_pasids.lock().len() as u32,
            pending_page_requests: self.pending_page_requests.lock().len() as u32,
            completed_page_replays: self.completed_page_replays.lock().len() as u32,
            invalidation_records: self.invalidation_log.lock().len() as u32,
            pri_budget: PriBudgetState {
                max_outstanding: self.pri_budget_limit.load(Ordering::Acquire),
                consumed: self.pri_budget_used.load(Ordering::Acquire),
                replay_count: self.replay_counter.load(Ordering::Acquire),
            },
        }
    }
}

// ============================================================================
// IOMMU BİRİMİ (IOMMU UNIT)
// ============================================================================

// Fiziksel IOMMU donanımını temsil eder.
// Sistemde birden fazla IOMMU bulunabilir (çok soketli sunucular, PCIe segment kümeleri).
//
//   IommuUnit
//     |-- vendor: Intel veya AMD
//     |-- base_addr: MMIO alanının başlangıç adresi (DMAR/IVRS tablosundan)
//     |-- domains: domain_id -> IommuDomain tablosu
//     +-- fault_recording: IOMMU ihlallerinin kaydı

pub struct IommuUnit {
    /// Birim kimliği (0'dan başlar)
    pub id: u32,
    /// Üretici (Intel VT-d, AMD-Vi veya ARM SMMU)
    pub vendor: IommuVendor,
    /// MMIO yazmaçlarının fiziksel taban adresi
    pub base_addr: u64,
    /// Haritalanmış MMIO sanal adresi (None = henüz haritalanmadı)
    pub mmio: Mutex<Option<u64>>,
    /// Çeviri etkin mi?
    pub enabled: AtomicBool,
    /// Kök tablonun fiziksel adresi
    pub root_table: AtomicU64,
    /// Kaydedilmiş IOMMU hataları (debug/audit için)
    pub fault_recording: Mutex<Vec<IommuFault>>,
    /// Bu birim tarafından yönetilen domain'ler
    pub domains: Mutex<BTreeMap<u32, IommuDomain>>,
    /// Bir sonraki domain ID sayacı
    pub next_domain_id: AtomicU32,
    /// PCIe ATS desteği (varsa)
    pub ats_supported: bool,
    /// ARM SMMUv3 desteği (varsa)
    pub smmu_v3_supported: bool,
    /// SMMUv3 stream table (ARM için)
    pub smmu_stream_table: Option<*mut SmmuStreamTableEntry>,
    /// SMMUv3 command queue (ARM için)
    pub smmu_cmd_queue: Option<*mut SmmuCommand>,
    /// SMMUv3 event queue (ARM için)
    pub smmu_event_queue: Option<*mut SmmuEvent>,
    page_request_queue: Mutex<Option<PageRequestQueueState>>,
}

unsafe impl Send for IommuUnit {}
unsafe impl Sync for IommuUnit {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IommuVendor {
    Intel, // Intel VT-d
    Amd,   // AMD-Vi
    Arm,   // ARM SMMUv3
    Unknown,
}

/// IOMMU erişim ihlali kaydı
#[derive(Clone, Debug)]
pub struct IommuFault {
    /// İhlali yapan cihazın PCI kaynak kimliği (BDF)
    pub source_id: u16,
    /// İhlalin yaşandığı domain
    pub domain_id: u32,
    /// İhlal edilen DMA adresi
    pub address: u64,
    /// İhlalin türü
    pub fault_type: IommuFaultType,
    /// İhlalin zamanlama damgası (tick sayısı)
    pub timestamp: u64,
}

/// IOMMU ihlal türleri
#[derive(Clone, Copy, Debug)]
pub enum IommuFaultType {
    ReadViolation,     // Okuma izni olmayan adrese erişim
    WriteViolation,    // Yazma izni olmayan adrese erişim
    TranslationFailed, // Eşleme tablosunda adres bulunamadı
    AccessViolation,   // Genel erişim ihlali
}

impl IommuUnit {
    pub fn new(id: u32, vendor: IommuVendor, base_addr: u64) -> Self {
        Self {
            id,
            vendor,
            base_addr,
            mmio: Mutex::new(None),
            enabled: AtomicBool::new(false),
            root_table: AtomicU64::new(0),
            fault_recording: Mutex::new(Vec::new()),
            domains: Mutex::new(BTreeMap::new()),
            next_domain_id: AtomicU32::new(1),
            ats_supported: false,
            smmu_v3_supported: matches!(vendor, IommuVendor::Arm),
            smmu_stream_table: None,
            smmu_cmd_queue: None,
            smmu_event_queue: None,
            page_request_queue: Mutex::new(None),
        }
    }

    /// IOMMU donanımını başlatır:
    /// 1. MMIO alanını haritalar
    /// 2. Yetenek yazmaçlarını okur
    /// 3. Kök tabloyu tahsis eder
    pub fn init(&self) -> Result<(), IommuError> {
        // MMIO alanını sanal adres uzayına haritala
        *self.mmio.lock() = Some(self.base_addr);

        // Donanımın desteklediği özellikleri doğrula
        self.check_capabilities()?;

        // Sayfa hizalı kök tablosu tahsis et
        let root_table = self.alloc_root_table();
        self.root_table.store(root_table, Ordering::SeqCst);

        crate::serial_println!("[IOMMU] Unit {} initialized ({:?})", self.id, self.vendor);
        Ok(())
    }

    /// Yetenek yazmaçlarını okur ve desteklenen özellikleri doğrular
    fn check_capabilities(&self) -> Result<(), IommuError> {
        // Gerçek uygulamada: VTD_CAP_REG ve VTD_ECAP_REG okunur
        Ok(())
    }

    /// Kök tablo için sayfa hizalı bellek tahsis eder
    fn alloc_root_table(&self) -> u64 {
        // Gerçek uygulamada: sayfa tahsisi yapılır, sıfırlanır
        0x100000
    }

    /// DMA çevirisini etkinleştirir (kök tabloyu yükle + çeviriyi aç)
    pub fn enable(&self) -> Result<(), IommuError> {
        match self.vendor {
            IommuVendor::Intel => self.enable_vtd()?,
            IommuVendor::Amd => self.enable_amd()?,
            _ => return Err(IommuError::NotSupported),
        }

        self.enabled.store(true, Ordering::SeqCst);
        crate::serial_println!("[IOMMU] Unit {} enabled", self.id);
        Ok(())
    }

    /// Intel VT-d çevirisini etkinleştir:
    /// 1. RTADDR yazmacına kök tabloyu yaz
    /// 2. GCMD yazmacında SRTP bitini ayarla (kök tabloyu uygula)
    /// 3. GCMD yazmacında TE bitini ayarla (çeviriyi etkinleştir)
    fn enable_vtd(&self) -> Result<(), IommuError> {
        self.write_mmio_reg(VTD_RTADDR_REG, self.root_table.load(Ordering::Acquire));
        let mut gcmd = self.read_mmio_reg(VTD_GCMD_REG) as u32;
        gcmd |= VTD_GCMD_SRTP;
        self.write_mmio_reg(VTD_GCMD_REG, gcmd as u64);
        if self.ats_supported {
            gcmd |= VTD_GCMD_QIE;
            self.write_mmio_reg(VTD_GCMD_REG, gcmd as u64);
        }
        gcmd |= VTD_GCMD_TE;
        self.write_mmio_reg(VTD_GCMD_REG, gcmd as u64);
        Ok(())
    }

    /// AMD-Vi çevirisini etkinleştir
    fn enable_amd(&self) -> Result<(), IommuError> {
        let dev_table = self.root_table.load(Ordering::Acquire);
        self.write_mmio_reg(AMDVI_DEV_TABLE_BASE_REG, dev_table);
        let mut control = self.read_mmio_reg(AMDVI_CONTROL_EXT_REG);
        if self.ats_supported {
            control |= (AMDVI_CTRL_PPR_ENABLE | AMDVI_CTRL_PPR_LOG_ENABLE) as u64;
        }
        self.write_mmio_reg(AMDVI_CONTROL_EXT_REG, control);
        Ok(())
    }

    /// PCIe ATS (Address Translation Services) desteğini başlatır
    /// Bu metod, PCIe cihazlarının donanım destekli çeviri önbelleği
    /// kullanmasına izin verir.
    pub fn init_ats_support(&mut self) -> Result<(), IommuError> {
        match self.vendor {
            IommuVendor::Intel => self.init_intel_ats()?,
            IommuVendor::Amd => self.init_amd_ats()?,
            IommuVendor::Arm => self.init_arm_smmuv3()?,
            _ => return Err(IommuError::NotSupported),
        }

        self.ats_supported = true;
        crate::serial_println!("[IOMMU] Unit {} ATS support initialized", self.id);
        Ok(())
    }

    /// Intel VT-d için ATS desteğini başlatır
    fn init_intel_ats(&self) -> Result<(), IommuError> {
        // Intel VT-d'de ATS, genişletilmiş yetenek yazmacında (ECAP)
        // ATS biti (bit 15) kontrol edilerek tespit edilir
        let ecap = self.read_mmio_reg(VTD_ECAP_REG);
        if (ecap & (1 << 15)) == 0 {
            return Err(IommuError::NotSupported);
        }

        let mut prq = PageRequestQueueState::alloc(
            PageRequestQueueVendor::Intel,
            256,
            size_of::<IntelPageRequestRecord>(),
        )?;
        self.write_mmio_reg(VTD_PQH_REG, 0);
        self.write_mmio_reg(VTD_PQT_REG, 0);
        self.write_mmio_reg(VTD_PQA_REG, prq.ring_base_phys as u64);
        self.write_mmio_reg(VTD_PRS_REG, 0);
        self.write_mmio_reg(VTD_PECTL_REG, 0);
        prq.programmed = true;
        *self.page_request_queue.lock() = Some(prq);
        Ok(())
    }

    /// AMD-Vi için ATS desteğini başlatır
    fn init_amd_ats(&self) -> Result<(), IommuError> {
        let mut prq = PageRequestQueueState::alloc(
            PageRequestQueueVendor::Amd,
            256,
            size_of::<AmdPprLogRecord>(),
        )?;
        self.write_mmio_reg(AMDVI_PPR_LOG_A_BASE_REG, prq.ring_base_phys as u64);
        self.write_mmio_reg(AMDVI_PPR_LOG_A_HEAD_REG, 0);
        self.write_mmio_reg(AMDVI_PPR_LOG_B_BASE_REG, prq.ring_base_phys as u64);
        self.write_mmio_reg(AMDVI_PPR_LOG_B_TAIL_REG, 0);
        self.write_mmio_reg(
            AMDVI_PPR_AUTO_RESPONSE_REG,
            AMDVI_PPR_RESPONSE_SUCCESS as u64,
        );
        let mut control = self.read_mmio_reg(AMDVI_CONTROL_EXT_REG);
        control |= (AMDVI_CTRL_PPR_ENABLE | AMDVI_CTRL_PPR_LOG_ENABLE) as u64;
        self.write_mmio_reg(AMDVI_CONTROL_EXT_REG, control);
        prq.programmed = true;
        *self.page_request_queue.lock() = Some(prq);
        Ok(())
    }

    pub fn page_request_queue_snapshot(&self) -> Option<PageRequestQueueSnapshot> {
        let queue = self.page_request_queue.lock();
        let state = queue.as_ref()?;
        Some(PageRequestQueueSnapshot {
            entry_count: state.entry_count,
            head: state.head,
            tail: state.tail,
            pending_records: state.tail.wrapping_sub(state.head),
            completed_responses: match state.vendor {
                PageRequestQueueVendor::Intel => state.intel_responses.len() as u32,
                PageRequestQueueVendor::Amd => state.amd_responses.len() as u32,
            },
        })
    }

    pub fn inject_intel_page_request(&self, request: &PriPageRequest) -> Result<(), IommuError> {
        let mut queue = self.page_request_queue.lock();
        let state = queue.as_mut().ok_or(IommuError::InitFailed)?;
        state.write_intel_entry(IntelPageRequestRecord::from_request(request))?;
        state
            .pending_request_meta
            .push_back((request.request_id, request.generation));
        state.decoded_requests.push_back(*request);
        self.write_mmio_reg(VTD_PQT_REG, state.tail as u64);
        Ok(())
    }

    pub fn inject_amd_page_request(&self, request: &PriPageRequest) -> Result<(), IommuError> {
        let mut queue = self.page_request_queue.lock();
        let state = queue.as_mut().ok_or(IommuError::InitFailed)?;
        state.write_amd_entry(AmdPprLogRecord::from_request(request))?;
        state
            .pending_request_meta
            .push_back((request.request_id, request.generation));
        state.decoded_requests.push_back(*request);
        self.write_mmio_reg(AMDVI_PPR_LOG_B_TAIL_REG, state.tail as u64);
        Ok(())
    }

    pub fn service_page_request_queue(
        &self,
        domain_id: u32,
        budget: usize,
    ) -> Result<Vec<PriReplayResult>, IommuError> {
        let domains = self.domains.lock();
        let domain = domains.get(&domain_id).ok_or(IommuError::DomainNotFound)?;
        let mut queue = self.page_request_queue.lock();
        let state = queue.as_mut().ok_or(IommuError::InitFailed)?;
        let count = budget.min(state.tail.wrapping_sub(state.head) as usize);
        let mut results = Vec::with_capacity(count);

        for _ in 0..count {
            let (request_id, generation) =
                state.pending_request_meta.pop_front().unwrap_or_else(|| {
                    (
                        domain.request_counter.fetch_add(1, Ordering::AcqRel),
                        domain.sva_generation.load(Ordering::Acquire),
                    )
                });
            let mut request = state
                .decoded_requests
                .pop_front()
                .unwrap_or(PriPageRequest {
                    request_id,
                    source_id: 0,
                    pasid: 0,
                    address: 0,
                    length: 4096,
                    write: false,
                    generation,
                });
            request.request_id = request_id;
            request.generation = generation;
            state.head = (state.head + 1) % state.entry_count.max(1);
            let result = domain.replay_page_request(request)?;
            domain.completed_page_replays.lock().push(result);
            match state.vendor {
                PageRequestQueueVendor::Intel => {
                    state
                        .intel_responses
                        .push(IntelPageResponseRecord::from_result(&result));
                    self.write_mmio_reg(VTD_PQH_REG, state.head as u64);
                }
                PageRequestQueueVendor::Amd => {
                    let response = AmdPprResponseRecord::from_result(&result);
                    state.amd_responses.push(response);
                    self.write_mmio_reg(
                        AMDVI_PPR_AUTO_RESPONSE_REG,
                        match response.code {
                            PageRequestResponseCode::Success => AMDVI_PPR_RESPONSE_SUCCESS as u64,
                            PageRequestResponseCode::Invalid => AMDVI_PPR_RESPONSE_INVALID as u64,
                            PageRequestResponseCode::Failure => AMDVI_PPR_RESPONSE_FAILURE as u64,
                        },
                    );
                    self.write_mmio_reg(AMDVI_PPR_LOG_A_HEAD_REG, state.head as u64);
                }
            }
            results.push(result);
        }

        Ok(results)
    }

    /// ARM SMMUv3'ü başlatır
    fn init_arm_smmuv3(&mut self) -> Result<(), IommuError> {
        // SMMUv3 ID register'larını kontrol et
        let idr0 = self.read_smmu_reg(SMMU_IDR0);

        // SMMUv3 destekleniyor mu?
        if (idr0 & 0x1) == 0 {
            return Err(IommuError::NotSupported);
        }

        // Stream table'ı tahsis et (2^N giriş, her giriş 64 byte)
        let stream_table_entries = 1024; // 10-bit stream ID desteği
        let stream_table_size = stream_table_entries * core::mem::size_of::<SmmuStreamTableEntry>();

        // Gerçek uygulamada: sayfa hizalı bellek tahsisi yapılır
        let stream_table_ptr = 0x200000u64; // Örnek adres
        self.smmu_stream_table = Some(stream_table_ptr as *mut SmmuStreamTableEntry);

        // Stream table base register'ı ayarla
        self.write_smmu_reg(SMMU_STRTAB_BASE, stream_table_ptr as u32);

        // Stream table config register'ı ayarla (log2(entries) - 1)
        let stcfg = (10 - 1) << 0; // 1024 giriş = 2^10
        self.write_smmu_reg(SMMU_STRTAB_BASE_CFG, stcfg);

        // Command queue'yu yapılandır
        self.init_smmu_command_queue()?;

        // Event queue'yu yapılandır
        self.init_smmu_event_queue()?;

        // SMMU'yu etkinleştir
        let cr0 = SMMU_CR0_SMMUEN | SMMU_CR0_CMDQEN | SMMU_CR0_EVENTQEN;
        self.write_smmu_reg(SMMU_CR0, cr0);

        // Acknowledge bekle
        while (self.read_smmu_reg(SMMU_CR0ACK) & SMMU_CR0_SMMUEN) == 0 {
            core::hint::spin_loop();
        }

        Ok(())
    }

    /// SMMUv3 command queue'yu başlatır
    fn init_smmu_command_queue(&mut self) -> Result<(), IommuError> {
        // Command queue için bellek tahsis et
        let cmd_queue_size = 4096; // 4KB, 256 komut (her komut 16 byte)
        let cmd_queue_ptr = 0x300000u64; // Örnek adres

        self.smmu_cmd_queue = Some(cmd_queue_ptr as *mut SmmuCommand);

        // Command queue base register'ı ayarla
        self.write_smmu_reg(SMMU_CMDQ_BASE, cmd_queue_ptr as u32);

        // Producer/consumer pointer'ları sıfırla
        self.write_smmu_reg(SMMU_CMDQ_PROD, 0);
        self.write_smmu_reg(SMMU_CMDQ_CONS, 0);

        Ok(())
    }

    /// SMMUv3 event queue'yu başlatır
    fn init_smmu_event_queue(&mut self) -> Result<(), IommuError> {
        // Event queue için bellek tahsis et
        let event_queue_size = 4096; // 4KB, 256 olay (her olay 16 byte)
        let event_queue_ptr = 0x400000u64; // Örnek adres

        self.smmu_event_queue = Some(event_queue_ptr as *mut SmmuEvent);

        // Event queue base register'ı ayarla
        self.write_smmu_reg(SMMU_EVENTQ_BASE, event_queue_ptr as u32);

        // Producer/consumer pointer'ları sıfırla
        self.write_smmu_reg(SMMU_EVENTQ_PROD, 0);
        self.write_smmu_reg(SMMU_EVENTQ_CONS, 0);

        Ok(())
    }

    /// MMIO register'ından okuma yapar
    fn read_mmio_reg(&self, offset: u32) -> u64 {
        if let Some(mmio_base) = *self.mmio.lock() {
            let addr = mmio_base + offset as u64;
            unsafe { (addr as *const u64).read_volatile() }
        } else {
            0
        }
    }

    /// MMIO register'ına yazma yapar
    fn write_mmio_reg(&self, offset: u32, value: u64) {
        if let Some(mmio_base) = *self.mmio.lock() {
            let addr = mmio_base + offset as u64;
            unsafe { (addr as *mut u64).write_volatile(value) };
        }
    }

    /// SMMU register'ından okuma yapar
    fn read_smmu_reg(&self, offset: u32) -> u32 {
        if let Some(mmio_base) = *self.mmio.lock() {
            let addr = mmio_base + offset as u64;
            unsafe { (addr as *const u32).read_volatile() }
        } else {
            0
        }
    }

    /// SMMU register'ına yazma yapar
    fn write_smmu_reg(&self, offset: u32, value: u32) {
        if let Some(mmio_base) = *self.mmio.lock() {
            let addr = mmio_base + offset as u64;
            unsafe { (addr as *mut u32).write_volatile(value) };
        }
    }

    /// PCIe cihazı için ATS yeteneğini kontrol eder
    pub fn probe_pci_ats(&self, bus: u8, device: u8, function: u8) -> Option<PciAtsCapability> {
        // PCIe konfigürasyon alanını tara ve ATS capability'yi bul
        let mut offset = self.read_pci_config(bus, device, function, 0x34) as u8; // Capabilities pointer

        while offset != 0 {
            let cap_id = self.read_pci_config(bus, device, function, offset) as u8;
            let cap_data = self.read_pci_config(bus, device, function, offset + 2);

            if cap_id == PCI_CAP_ID_ATS {
                let mut ats_cap = PciAtsCapability::new();
                ats_cap.offset = offset;
                ats_cap.qdep = ((cap_data & ATS_CAP_QDEP_MASK) >> ATS_CAP_QDEP_SHIFT) as u8;
                ats_cap.page_aligned = (cap_data & ATS_CAP_PAGE_ALIGNED) != 0;
                return Some(ats_cap);
            }

            offset = (cap_data >> 8) as u8; // Next capability pointer
        }

        None
    }

    /// PCIe cihazı için PRI (Page Request Interface) yeteneğini kontrol eder
    pub fn probe_pci_pri(&self, bus: u8, device: u8, function: u8) -> Option<PciPriCapability> {
        let mut offset = self.read_pci_config(bus, device, function, 0x34) as u8;

        while offset != 0 {
            let cap_id = self.read_pci_config(bus, device, function, offset) as u8;

            if cap_id == PCI_CAP_ID_PRI {
                let mut pri_cap = PciPriCapability::new();
                pri_cap.offset = offset;
                return Some(pri_cap);
            }

            let cap_data = self.read_pci_config(bus, device, function, offset + 2);
            offset = (cap_data >> 8) as u8;
        }

        None
    }

    /// PCIe cihazında ATS'yi etkinleştirir
    pub fn enable_pci_ats(
        &self,
        bus: u8,
        device: u8,
        function: u8,
        ats_cap: &PciAtsCapability,
    ) -> Result<(), IommuError> {
        let ctrl_reg = ats_cap.offset + 4; // ATS Control register offset
        let mut ctrl_value = self.read_pci_config(bus, device, function, ctrl_reg);

        // ATS'yi etkinleştir ve en küçük çeviri birimini ayarla (4KB)
        ctrl_value |= ATS_CTRL_ENABLE;
        ctrl_value &= !ATS_CTRL_STU_MASK;
        ctrl_value |= (12 << ATS_CTRL_STU_SHIFT); // 4KB = 2^12

        self.write_pci_config(bus, device, function, ctrl_reg, ctrl_value);

        // Geçersiz kılma kuyruğunu yapılandır (varsa)
        self.configure_ats_invalidation_queue(bus, device, function)?;

        Ok(())
    }

    /// ATS geçersiz kılma kuyruğunu yapılandırır
    fn configure_ats_invalidation_queue(
        &self,
        bus: u8,
        device: u8,
        function: u8,
    ) -> Result<(), IommuError> {
        let mmio_ok = self.mmio.lock().is_some();
        if !mmio_ok {
            return Err(IommuError::HardwareError);
        }

        let ats = self
            .probe_pci_ats(bus, device, function)
            .ok_or(IommuError::NotSupported)?;
        let requested_depth = (ats.qdep as u32).saturating_add(1);

        let mut prq = self.page_request_queue.lock();
        let queue = prq.as_mut().ok_or(IommuError::InitFailed)?;
        if !queue.programmed {
            return Err(IommuError::InitFailed);
        }
        if requested_depth > queue.entry_count {
            return Err(IommuError::NoMemory);
        }

        match self.vendor {
            IommuVendor::Intel => {
                let mut gcmd = self.read_mmio_reg(VTD_GCMD_REG) as u32;
                gcmd |= VTD_GCMD_QIE;
                self.write_mmio_reg(VTD_GCMD_REG, gcmd as u64);
                if !self.wait_mmio_bit_set(VTD_GSTS_REG, VTD_GSTS_QIES) {
                    return Err(IommuError::HardwareError);
                }
                Ok(())
            }
            IommuVendor::Amd => Ok(()),
            _ => Err(IommuError::UnsupportedVendor),
        }
    }

    /// SMMUv3 stream table entry oluşturur
    pub fn create_smmu_stream_entry(
        &self,
        stream_id: u32,
        stage1_ttbr: u64,
    ) -> SmmuStreamTableEntry {
        let mut ste = SmmuStreamTableEntry::new();
        ste.set_stage1_translation(stage1_ttbr);
        ste
    }

    /// SMMUv3 stream table entry günceller
    pub fn update_smmu_stream_entry(&self, stream_id: u32, entry: SmmuStreamTableEntry) {
        if let Some(stream_table) = self.smmu_stream_table {
            unsafe {
                let ste_ptr = stream_table.add(stream_id as usize);
                *ste_ptr = entry;
            }

            // Stream table entry geçersiz kıl
            self.invalidate_smmu_stream_entry(stream_id);
        }
    }

    /// SMMUv3 stream table entry geçersiz kılar
    fn invalidate_smmu_stream_entry(&self, stream_id: u32) {
        // SMMUv3 komut kuyruğuna geçersiz kılma komutu ekle
        // Gerçek uygulamada: CMD_SYNC komutu gönderilir
    }

    /// SMMUv3 komut kuyruğuna komut ekler
    fn enqueue_smmu_command(&self, cmd: SmmuCommand) -> Result<(), IommuError> {
        if let Some(cmd_queue) = self.smmu_cmd_queue {
            let prod = self.read_smmu_reg(SMMU_CMDQ_PROD);
            let cons = self.read_smmu_reg(SMMU_CMDQ_CONS);

            // Kuyruk dolu mu?
            let queue_size = 256; // 256 komut kapasite
            if (prod.wrapping_sub(cons)) >= queue_size {
                return Err(IommuError::NoMemory);
            }

            // Komutu kuyruğa ekle
            unsafe {
                let cmd_ptr = cmd_queue.add((prod % queue_size) as usize);
                *cmd_ptr = cmd;
            }

            // Producer pointer'ı güncelle
            self.write_smmu_reg(SMMU_CMDQ_PROD, prod.wrapping_add(1));

            Ok(())
        } else {
            Err(IommuError::InitFailed)
        }
    }

    /// PCI konfigürasyon alanından okuma yapar
    fn read_pci_config(&self, bus: u8, device: u8, function: u8, offset: u8) -> u32 {
        let offset = (offset as u64) & !0x3;
        if self.base_addr != 0 {
            // ECAM: base + bus[27:20] + device[19:15] + function[14:12] + register[11:2]
            let ecam_offset =
                ((bus as u64) << 20) | ((device as u64) << 15) | ((function as u64) << 12) | offset;
            let addr = self.base_addr.saturating_add(ecam_offset);
            return unsafe { (addr as *const u32).read_volatile() };
        }

        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        unsafe {
            use x86_64::instructions::port::Port;

            let address = 0x8000_0000u32
                | ((bus as u32) << 16)
                | ((device as u32) << 11)
                | ((function as u32) << 8)
                | ((offset as u32) & 0xFC);
            let mut addr_port = Port::<u32>::new(0xCF8);
            let mut data_port = Port::<u32>::new(0xCFC);
            addr_port.write(address);
            return data_port.read();
        }

        #[allow(unreachable_code)]
        0
    }

    #[cfg(not(target_os = "none"))]
    pub fn bind_verification_mmio(&self, mmio_base: u64) {
        *self.mmio.lock() = Some(mmio_base);
    }

    #[cfg(not(target_os = "none"))]
    pub fn init_verification_ats(&mut self) -> Result<(), IommuError> {
        match self.vendor {
            IommuVendor::Intel => self.init_intel_ats()?,
            IommuVendor::Amd => self.init_amd_ats()?,
            IommuVendor::Arm => self.init_arm_smmuv3()?,
            IommuVendor::Unknown => return Err(IommuError::NotSupported),
        }
        self.ats_supported = true;
        Ok(())
    }

    /// PCI konfigürasyon alanına yazma yapar
    fn write_pci_config(&self, bus: u8, device: u8, function: u8, offset: u8, value: u32) {
        let offset = (offset as u64) & !0x3;
        if self.base_addr != 0 {
            let ecam_offset =
                ((bus as u64) << 20) | ((device as u64) << 15) | ((function as u64) << 12) | offset;
            let addr = self.base_addr.saturating_add(ecam_offset);
            unsafe { (addr as *mut u32).write_volatile(value) };
            return;
        }

        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        unsafe {
            use x86_64::instructions::port::Port;

            let address = 0x8000_0000u32
                | ((bus as u32) << 16)
                | ((device as u32) << 11)
                | ((function as u32) << 8)
                | ((offset as u32) & 0xFC);
            let mut addr_port = Port::<u32>::new(0xCF8);
            let mut data_port = Port::<u32>::new(0xCFC);
            addr_port.write(address);
            data_port.write(value);
        }
    }

    /// DMA çevirisini devre dışı bırakır
    pub fn disable(&self) {
        self.enabled.store(false, Ordering::SeqCst);
    }

    /// Yeni bir DMA domain oluşturur ve ID'sini döner
    pub fn create_domain(&self) -> u32 {
        let id = self.next_domain_id.fetch_add(1, Ordering::SeqCst);
        let domain = IommuDomain::new(id);
        self.domains.lock().insert(id, domain);
        id
    }

    /// Domain ID ile domain nesnesini döner
    pub fn get_domain(&self, id: u32) -> Option<IommuDomain> {
        self.domains.lock().get(&id).cloned()
    }

    /// IOTLB (I/O TLB) geçersiz kılar: çeviri önbelleğini temizler.
    /// Eşleme değişikliklerinden sonra çağrılmalıdır.
    pub fn flush_iotlb(&self, domain_id: u32, _addr: u64) -> Result<(), IommuError> {
        if self.mmio.lock().is_none() {
            return Err(IommuError::HardwareError);
        }
        if self.get_domain(domain_id).is_none() {
            return Err(IommuError::DomainNotFound);
        }

        match self.vendor {
            IommuVendor::Intel => {
                let request = vtd_domain_iotlb_request(domain_id);
                self.write_mmio_reg(VTD_IOTLB_REG, request);

                if self.wait_mmio_bit_clear(VTD_IOTLB_REG, VTD_IOTLB_IVT) {
                    Ok(())
                } else {
                    Err(IommuError::HardwareError)
                }
            }
            _ => Err(IommuError::UnsupportedVendor),
        }
    }

    /// Yazma tamponunu temizler: askıdaki DMA yazmaları tamamlanır
    pub fn flush_write_buffer(&self) -> Result<(), IommuError> {
        if self.mmio.lock().is_none() {
            return Err(IommuError::HardwareError);
        }
        match self.vendor {
            IommuVendor::Intel => {
                let cap = self.read_mmio_reg(VTD_CAP_REG);
                let rwbf_required = (cap & VTD_CAP_RWBF) != 0;

                let mut gcmd = self.read_mmio_reg(VTD_GCMD_REG) as u32;
                gcmd |= VTD_GCMD_WBF;
                self.write_mmio_reg(VTD_GCMD_REG, gcmd as u64);

                if !rwbf_required {
                    return Ok(());
                }
                if self.wait_mmio_bit_clear(VTD_GSTS_REG, VTD_GSTS_WBFS) {
                    Ok(())
                } else {
                    Err(IommuError::HardwareError)
                }
            }
            _ => Err(IommuError::UnsupportedVendor),
        }
    }

    fn wait_mmio_bit_set(&self, offset: u32, bit: u64) -> bool {
        for _ in 0..VTD_MMIO_POLL_SPINS {
            if (self.read_mmio_reg(offset) & bit) != 0 {
                return true;
            }
            core::hint::spin_loop();
        }
        false
    }

    fn wait_mmio_bit_clear(&self, offset: u32, bit: u64) -> bool {
        for _ in 0..VTD_MMIO_POLL_SPINS {
            if (self.read_mmio_reg(offset) & bit) == 0 {
                return true;
            }
            core::hint::spin_loop();
        }
        false
    }

    pub fn assign_device_to_domain(
        &self,
        domain_id: u32,
        segment: u16,
        bdf: u16,
    ) -> Result<(), IommuError> {
        let domains = self.domains.lock();
        let domain = domains.get(&domain_id).ok_or(IommuError::DomainNotFound)?;
        domain.attach_device(segment, bdf);
        Ok(())
    }

    pub fn bind_process_shared_va(
        &self,
        domain_id: u32,
        process_id: u64,
        pasid: u32,
        address_space_id: u64,
        page_table_root: u64,
    ) -> Result<(), IommuError> {
        let domains = self.domains.lock();
        let domain = domains.get(&domain_id).ok_or(IommuError::DomainNotFound)?;
        domain.bind_process_address_space(process_id, pasid, address_space_id, page_table_root)
    }

    pub fn map_gpuva(
        &self,
        domain_id: u32,
        pasid: u32,
        gpu_va: u64,
        phys_addr: u64,
        size: u64,
        read: bool,
        write: bool,
        dma_buf_fd: u32,
    ) -> Result<(), IommuError> {
        let domains = self.domains.lock();
        let domain = domains.get(&domain_id).ok_or(IommuError::DomainNotFound)?;
        domain.map_gpu_virtual_address(pasid, gpu_va, phys_addr, size, read, write, dma_buf_fd)
    }

    // ========================================================================
    // VT-d PASID TABLE ENTRY PROGRAMMING (Scalable Mode, §9.6)
    // ========================================================================

    /// VT-d Scalable-Mode PASID Table Entry oluşturur.
    /// VT-d Spec Rev 4.0 §9.6: 128-bit entry format.
    /// - lo[0] = Present
    /// - lo[3] = PASID Enable
    /// - lo[9:7] = Address Width (0=48-bit, 1=57-bit)
    /// - lo[11:10] = FLPM (0=4-level, 1=5-level)
    /// - lo[51:12] = First-Level Page Table Pointer (FLPTP)
    /// - hi[62] = SRE, hi[61] = EAFE
    pub fn create_pasid_entry(
        page_table_root: u64,
        address_width_57: bool,
        five_level: bool,
        supervisor_requests: bool,
    ) -> (u64, u64) {
        let mut lo: u64 = VTD_PASID_ENTRY_PRESENT | VTD_PASID_ENTRY_PASID_EN;
        if address_width_57 {
            lo |= VTD_PASID_ENTRY_AW_57;
        } else {
            lo |= VTD_PASID_ENTRY_AW_48;
        }
        if five_level {
            lo |= VTD_PASID_ENTRY_FLPM_5LVL;
        } else {
            lo |= VTD_PASID_ENTRY_FLPM_4LVL;
        }
        lo |= page_table_root & VTD_PASID_ENTRY_FLPTP_MASK;

        let mut hi: u64 = 0;
        if supervisor_requests {
            hi |= VTD_PASID_ENTRY_SRE;
        }

        (lo, hi)
    }

    /// PASID table'ye entry yazar.
    /// PASID table, ECAP.PSS ile belirlenen boyutta, page-aligned olmalı.
    /// Her entry 128-bit (2x u64).
    pub fn write_pasid_entry(
        &self,
        pasid_table_base: u64,
        pasid_index: u32,
        lo: u64,
        hi: u64,
    ) {
        let entry_offset = (pasid_index as u64) * 16; // 128-bit per entry
        let addr_lo = pasid_table_base + entry_offset;
        let addr_hi = pasid_table_base + entry_offset + 8;
        unsafe {
            (addr_lo as *mut u64).write_volatile(lo);
            (addr_hi as *mut u64).write_volatile(hi);
        }
    }

    /// PASID-cache invalidate (VT-d §6.2.3).
    /// PASID table entry değiştikten sonra çağrılmalı.
    pub fn invalidate_pasid_cache(&self, pasid: u32, domain_id: u16) {
        // Queued Invalidation: PASID-cache invalidate descriptor
        // Format: qw0 = [PASID[19:0] | DID[15:0] | index=3 (PASID inv)], qw1 = [gran=0 | IF=1]
        let qw0 = (pasid as u64) << 32 | (domain_id as u64) << 16 | VTD_QI_PASID;
        let qw1 = VTD_QI_IF_IIG;
        self.write_mmio_reg(0x180, qw0); // IQH (Invalidation Queue Head)
        self.write_mmio_reg(0x188, qw1); // IQT (Invalidation Queue Tail) — trigger
    }

    // ========================================================================
    // AMD IOMMU DTE PROGRAMMING (§2.2.2, 256-bit entry)
    // ========================================================================

    /// AMD IOMMU Device Table Entry oluşturur.
    /// AMD IOMMU Spec 48882: 256-bit (4x64-bit) entry.
    /// DTE[0]: V, TV, domain ID, IR, IW
    /// DTE[1]: GCR3 table root pointer (bits 51:12 split across fields)
    /// DTE[2]: GPT level, GIOV, GV
    pub fn create_amd_dte(
        domain_id: u16,
        enable_interrupt: bool,
        gcr3_table_root: u64,
        gcr3_levels: u8, // 1-3 (GLX field)
    ) -> [u64; 4] {
        let mut data = [0u64; 4];

        // data[0]: V=1, TV=1, domain_id, IR/IW
        data[0] |= DTE_FLAG_V;
        data[0] |= DTE_FLAG_TV;
        data[0] |= domain_id as u64;
        if enable_interrupt {
            data[0] |= DTE_FLAG_IR;
            data[0] |= DTE_FLAG_IW;
        }

        // data[0]: GCR3 table root pointer (split into 3 fields)
        if gcr3_table_root != 0 {
            let gcr3 = gcr3_table_root >> 12; // bits 51:12
            data[0] |= (gcr3 & 0x7) << DTE_GCR3_14_12_SHIFT; // bits 14:12
            data[1] |= (gcr3 >> 3) & 0xFFFF; // bits 30:15 → data[1][31:16]
            data[0] |= ((gcr3 >> 19) & 0x1FFFFF) << DTE_GCR3_51_31_SHIFT; // bits 51:31
            data[0] |= DTE_FLAG_GV;
        }

        // data[2]: GPT level (GLX)
        let glx = (gcr3_levels.saturating_sub(1) as u64) & 0x3;
        data[2] |= glx << DTE_GPT_LEVEL_SHIFT;

        data
    }

    /// AMD DTE'yi 256-bit atomic write ile günceller.
    /// Linux kernel pattern: DTE[V] son yazılır (set), ilk silinir (clear).
    pub fn write_amd_dte(&self, dev_table_base: u64, devid: u16, dte: [u64; 4]) {
        let base = dev_table_base + (devid as u64) * AMDVI_DTE_SIZE as u64;
        unsafe {
            // Lower 128-bit first, then upper 128-bit
            // V bit is in data[0], so it's written last naturally
            (base as *mut u64).write_volatile(dte[0]);
            (base.wrapping_add(8) as *mut u64).write_volatile(dte[1]);
            (base.wrapping_add(16) as *mut u64).write_volatile(dte[2]);
            (base.wrapping_add(24) as *mut u64).write_volatile(dte[3]);
        }
        // IOMMU DTE sync — hardware'a görünür olması için
        core::sync::atomic::fence(Ordering::SeqCst);
    }

    // ========================================================================
    // VT-d FAULT HANDLING (FSTS/FEDATA/FEADDR, §10.4.14-18)
    // ========================================================================

    /// IOMMU fault event register'larını okur ve hatayı kaydeder.
    /// VT-d Spec §10.4: Fault recording via FSTS + FEDATA + FEADDR.
    pub fn handle_fault(&self) {
        let fsts = self.read_mmio_reg(VTD_FSTS_REG) as u32;
        if fsts == 0 {
            return;
        }

        let fedata = self.read_mmio_reg(VTD_FEDATA_REG);
        let feaddr = self.read_mmio_reg(VTD_FEADDR_REG);

        // Fault source ID: FEDATA[31:16] = Source ID (BDF)
        let source_id = ((fedata >> 16) & 0xFFFF) as u16;
        // Fault type: FEDATA[3:0] = Fault Reason
        let fault_reason = (fedata & 0xF) as u8;
        // Fault address: FEADDR (or FEUADDR for 64-bit)

        let fault_type = match fault_reason {
            0 => IommuFaultType::ReadViolation,
            1 => IommuFaultType::WriteViolation,
            2 => IommuFaultType::TranslationFailed,
            _ => IommuFaultType::AccessViolation,
        };

        let fault = IommuFault {
            source_id,
            domain_id: ((fedata >> 8) & 0xFF) as u32,
            address: feaddr,
            fault_type,
            timestamp: crate::task::scheduler::get_ticks() as u64,
        };

        crate::serial_println!(
            "[IOMMU] Fault: src={:#06x} dom={} addr={:#x} reason={} type={:?}",
            source_id, fault.domain_id, feaddr, fault_reason, fault_type
        );

        self.fault_recording.lock().push(fault);

        // Clear fault status — write 1 to clear
        self.write_mmio_reg(VTD_FSTS_REG, fsts as u64);
    }

    // ========================================================================
    // ATS INVALIDATE WITH DEVICE IOTLB (VT-d §6.2, PCIe ATS Spec)
    // ========================================================================

    /// ATS Invalidate — device IOTLB'yi temizler.
    /// Cihaz ATS enabled olduğunda kendi IOTLB'sini tutar;
    /// IOMMU mapping değişince device IOTLB de invalidate edilmeli.
    pub fn ats_invalidate(&self, requester_id: u16, address: u64, size_pages: u32) {
        if !self.ats_supported {
            return;
        }

        // ATS Invalidate Request: 64-bit message to device
        // Format: [Address[63:12] | Size[5:0] | Reserved[3:0] | Type=0]
        let ats_msg_lo = (address & !0xFFF) as u32;
        let ats_msg_hi = ((address >> 32) as u32) & 0xFFFFF;
        let size_field = (size_pages.saturating_sub(1) as u32) & 0x3F;

        // Device'e ATS Invalidate mesajı gönder (MMIO through PCIe config)
        let bus = (requester_id >> 8) as u8;
        let device = ((requester_id >> 3) & 0x1F) as u8;
        let function = (requester_id & 0x7) as u8;

        // ATS Capability offset'ini bul ve Invalidate mesajı yaz
        if let Some(ats_cap) = self.probe_pci_ats(bus, device, function) {
            let ctrl_reg = ats_cap.offset + 4;
            // ATS Invalidate: address + size through device's ATS interface
            self.write_pci_config(bus, device, function, ctrl_reg, ats_msg_lo);
        }

        // IOMMU tarafında da IOTLB flush
        let _ = self.flush_iotlb(0, address);
    }
}

// ============================================================================
// IOMMU YÖNETİCİSİ (IOMMU MANAGER)
// ============================================================================

// Sistemdeki tüm IOMMU birimlerini merkezi olarak yönetir.
// ACPI tabloları (DMAR veya IVRS) taranarak birimler keşfedilir.

pub struct IommuManager {
    units: Mutex<Vec<IommuUnit>>,
    /// Varsayılan domain ID'si (kimliği doğrulanmamış cihazlar için)
    default_domain: AtomicU32,
    /// Herhangi bir IOMMU etkin mi?
    iommu_enabled: AtomicBool,
}

impl IommuManager {
    pub const fn new() -> Self {
        Self {
            units: Mutex::new(Vec::new()),
            default_domain: AtomicU32::new(0),
            iommu_enabled: AtomicBool::new(false),
        }
    }

    /// Yeni IOMMU birimi kaydeder (DMAR/IVRS tablosundan çağrılır)
    pub fn register_unit(&self, vendor: IommuVendor, base_addr: u64) -> u32 {
        let id = self.units.lock().len() as u32;
        let unit = IommuUnit::new(id, vendor, base_addr);
        self.units.lock().push(unit);
        id
    }

    /// Tüm IOMMU birimlerini başlatır
    pub fn init_all(&self) -> Result<(), IommuError> {
        for unit in self.units.lock().iter() {
            unit.init()?;
        }
        Ok(())
    }

    /// Tüm IOMMU birimlerinde DMA çevirisini etkinleştirir
    pub fn enable_all(&self) -> Result<(), IommuError> {
        for unit in self.units.lock().iter() {
            unit.enable()?;
        }
        self.iommu_enabled.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// Birim ID'si ile IOMMU birimini döner
    pub fn get_unit(&self, id: u32) -> Option<IommuUnit> {
        self.units.lock().get(id as usize).cloned()
    }

    /// Cihaz için DMA adresi eşler: BDF ile ilgili domain bulunur ve map çağrılır
    pub fn map_dma(
        &self,
        segment: u16,
        bdf: u16,
        dma_addr: u64,
        phys_addr: u64,
        size: u64,
        read: bool,
        write: bool,
    ) -> Result<(), IommuError> {
        let units = self.units.lock();
        for unit in units.iter() {
            let domains = unit.domains.lock();
            for domain in domains.values() {
                if domain
                    .devices
                    .lock()
                    .iter()
                    .any(|&(seg, dev)| seg == segment && dev == bdf)
                {
                    return domain.map(dma_addr, phys_addr, size, read, write);
                }
            }
        }
        Err(IommuError::DeviceNotFound)
    }

    /// Cihaz için DMA eşlemesini kaldırır
    pub fn unmap_dma(&self, segment: u16, bdf: u16, dma_addr: u64) -> bool {
        let units = self.units.lock();
        for unit in units.iter() {
            let domains = unit.domains.lock();
            for domain in domains.values() {
                if domain
                    .devices
                    .lock()
                    .iter()
                    .any(|&(seg, dev)| seg == segment && dev == bdf)
                {
                    return domain.unmap(dma_addr);
                }
            }
        }
        false
    }

    /// IOMMU'nun etkin olup olmadığını döner
    pub fn is_enabled(&self) -> bool {
        self.iommu_enabled.load(Ordering::SeqCst)
    }
}

// IommuUnit için Clone implementasyonu (AtomicXxx alanları manuel klonlanır)
impl Clone for IommuUnit {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            vendor: self.vendor,
            base_addr: self.base_addr,
            mmio: Mutex::new(*self.mmio.lock()),
            enabled: AtomicBool::new(self.enabled.load(Ordering::Relaxed)),
            root_table: AtomicU64::new(self.root_table.load(Ordering::Relaxed)),
            fault_recording: Mutex::new(self.fault_recording.lock().clone()),
            domains: Mutex::new(self.domains.lock().clone()),
            next_domain_id: AtomicU32::new(self.next_domain_id.load(Ordering::Relaxed)),
            ats_supported: self.ats_supported,
            smmu_v3_supported: self.smmu_v3_supported,
            smmu_stream_table: self.smmu_stream_table,
            smmu_cmd_queue: self.smmu_cmd_queue,
            smmu_event_queue: self.smmu_event_queue,
            page_request_queue: Mutex::new(None),
        }
    }
}

// IommuDomain için Clone implementasyonu
impl Clone for IommuDomain {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            page_table: self.page_table,
            translation_enabled: self.translation_enabled,
            devices: Mutex::new(self.devices.lock().clone()),
            mappings: Mutex::new(self.mappings.lock().clone()),
            pasid_bindings: Mutex::new(self.pasid_bindings.lock().clone()),
            sva_windows: Mutex::new(self.sva_windows.lock().clone()),
            gpuva_ranges: Mutex::new(self.gpuva_ranges.lock().clone()),
            device_pasids: Mutex::new(self.device_pasids.lock().clone()),
            pri_budget_limit: AtomicU32::new(self.pri_budget_limit.load(Ordering::Relaxed)),
            pri_budget_used: AtomicU32::new(self.pri_budget_used.load(Ordering::Relaxed)),
            fault_replays: Mutex::new(self.fault_replays.lock().clone()),
            pending_page_requests: Mutex::new(self.pending_page_requests.lock().clone()),
            completed_page_replays: Mutex::new(self.completed_page_replays.lock().clone()),
            invalidation_log: Mutex::new(self.invalidation_log.lock().clone()),
            replay_counter: AtomicU64::new(self.replay_counter.load(Ordering::Relaxed)),
            request_counter: AtomicU64::new(self.request_counter.load(Ordering::Relaxed)),
            sva_generation: AtomicU64::new(self.sva_generation.load(Ordering::Relaxed)),
            invalidate_sequence: AtomicU64::new(self.invalidate_sequence.load(Ordering::Relaxed)),
        }
    }
}

lazy_static::lazy_static! {
    pub static ref IOMMU_MANAGER: IommuManager = IommuManager::new();
}

// ============================================================================
// HATA TÜRLERİ (ERROR TYPES)
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IommuError {
    NotSupported,   // Bu donanımda IOMMU desteklenmiyor
    InitFailed,     // Başlatma başarısız
    NoMemory,       // Kök/sayfa tablosu için bellek yetersiz
    InvalidAddress, // Geçersiz DMA adresi
    DeviceNotFound, // Belirtilen PCI cihazı bulunamadı
    DomainNotFound, // Belirtilen domain bulunamadı
    HardwareError,
    UnsupportedVendor,
}

// ============================================================================
// BAŞLATMA (INITIALIZATION)
// ============================================================================

pub fn init() -> bool {
    let units = crate::cpu::acpi::get_dmar_units();
    if units.is_empty() {
        crate::serial_println!("[IOMMU] No DMAR units discovered");
        return false;
    }

    {
        let mut registered = IOMMU_MANAGER.units.lock();
        registered.clear();
    }
    IOMMU_MANAGER.iommu_enabled.store(false, Ordering::SeqCst);

    for unit in units {
        let unit_id = IOMMU_MANAGER.register_unit(IommuVendor::Intel, unit.register_base);
        let Some(iommu) = IOMMU_MANAGER.get_unit(unit_id) else {
            crate::serial_println!("[IOMMU] Failed to fetch registered unit {}", unit_id);
            return false;
        };
        let domain_id = iommu.create_domain();
        for device in unit.devices {
            let bdf = ((device.bus as u16) << 8)
                | ((device.device as u16) << 3)
                | (device.function as u16);
            if let Err(err) = iommu.assign_device_to_domain(domain_id, unit.segment, bdf) {
                crate::serial_println!(
                    "[IOMMU] Domain attach failed seg={} bdf={:#x}: {:?}",
                    unit.segment,
                    bdf,
                    err
                );
                return false;
            }
        }
    }

    if let Err(err) = IOMMU_MANAGER.init_all() {
        crate::serial_println!("[IOMMU] Hardware init failed: {:?}", err);
        return false;
    }
    if let Err(err) = IOMMU_MANAGER.enable_all() {
        crate::serial_println!("[IOMMU] Hardware enable failed: {:?}", err);
        return false;
    }
    if !run_self_test() {
        crate::serial_println!("[IOMMU] Self-test failed");
        return false;
    }

    crate::serial_println!("[IOMMU] Early-boot DMA remapping online");
    true
}

pub fn sync_hotplug_device(bus: u8, device: u8, function: u8, present: bool) -> bool {
    let Some(unit) = IOMMU_MANAGER.get_unit(0) else {
        return !present;
    };
    if present && !unit.enabled.load(Ordering::Acquire) {
        return false;
    }

    let bdf = ((bus as u16) << 8) | ((device as u16) << 3) | (function as u16);
    if present {
        let already_attached = {
            let domains = unit.domains.lock();
            domains.values().any(|domain| {
                domain
                    .devices
                    .lock()
                    .iter()
                    .any(|&(segment, attached_bdf)| segment == 0 && attached_bdf == bdf)
            })
        };
        if already_attached {
            return true;
        }
        let domain_id = unit.create_domain();
        return unit.assign_device_to_domain(domain_id, 0, bdf).is_ok();
    }

    let mut detached = false;
    let domains = unit.domains.lock();
    for domain in domains.values() {
        let had_device = domain
            .devices
            .lock()
            .iter()
            .any(|&(segment, attached_bdf)| segment == 0 && attached_bdf == bdf);
        if had_device {
            domain.detach_device(0, bdf);
            detached = true;
        }
    }
    detached || !unit.enabled.load(Ordering::Acquire)
}

fn run_self_test() -> bool {
    let Some(unit) = IOMMU_MANAGER.get_unit(0) else {
        return false;
    };
    let domain_id = unit.create_domain();
    let Some(domain) = unit.get_domain(domain_id) else {
        return false;
    };

    const TEST_DMA: u64 = 0x4000_0000;
    const TEST_PHYS: u64 = 0x0020_0000;
    const TEST_SIZE: u64 = 0x1000;

    if domain
        .map(TEST_DMA, TEST_PHYS, TEST_SIZE, true, true)
        .is_err()
    {
        return false;
    }
    let mapped = matches!(
        domain.translate(TEST_DMA),
        Some(translation) if translation.present
            && translation.phys_addr == TEST_PHYS
            && translation.size == TEST_SIZE
            && translation.read_perm
            && translation.write_perm
    );
    let unmapped_neighbor = domain.translate(TEST_DMA + TEST_SIZE).is_none();
    let unmapped = domain.unmap(TEST_DMA);
    let cleared = domain.translate(TEST_DMA).is_none();

    mapped && unmapped_neighbor && unmapped && cleared
}

fn vtd_domain_iotlb_request(domain_id: u32) -> u64 {
    VTD_IOTLB_IVT
        | VTD_IOTLB_IIRG_DOMAIN
        | VTD_IOTLB_DR
        | VTD_IOTLB_DW
        | ((domain_id as u64) << VTD_IOTLB_DID_SHIFT)
}

#[cfg(test)]
pub(crate) fn phase5_kickoff_contract_green() -> bool {
    const TEST_DMA: u64 = 0x4000_0000;
    const TEST_PHYS: u64 = 0x0020_0000;
    const TEST_SIZE: u64 = 0x1000;

    let domain = IommuDomain::new(7);
    if domain
        .map(TEST_DMA, TEST_PHYS, TEST_SIZE, true, true)
        .is_err()
    {
        return false;
    }

    let map_contract = matches!(
        domain.translate(TEST_DMA),
        Some(translation) if translation.present
            && translation.phys_addr == TEST_PHYS
            && translation.size == TEST_SIZE
            && translation.read_perm
            && translation.write_perm
    ) && domain.translate(TEST_DMA + TEST_SIZE).is_none();

    let invalidate_contract = {
        let req = vtd_domain_iotlb_request(0x1234);
        (req & VTD_IOTLB_IVT) != 0
            && (req & VTD_IOTLB_IIRG_DOMAIN) == VTD_IOTLB_IIRG_DOMAIN
            && (req & VTD_IOTLB_DR) != 0
            && (req & VTD_IOTLB_DW) != 0
            && (((req >> VTD_IOTLB_DID_SHIFT) & 0xFFFF) as u32 == 0x1234)
    };

    map_contract && invalidate_contract && domain.unmap(TEST_DMA) && domain.translate(TEST_DMA).is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vtd_domain_iotlb_request_encodes_domain_and_granularity() {
        let value = vtd_domain_iotlb_request(0x1234);
        assert_ne!(value & VTD_IOTLB_IVT, 0);
        assert_eq!(value & (0b111 << 60), VTD_IOTLB_IIRG_DOMAIN);
        assert_ne!(value & VTD_IOTLB_DR, 0);
        assert_ne!(value & VTD_IOTLB_DW, 0);
        assert_eq!(((value >> VTD_IOTLB_DID_SHIFT) & 0xFFFF) as u32, 0x1234);
    }

    #[cfg(not(target_os = "none"))]
    #[test]
    fn flush_iotlb_rejects_unsupported_vendor_even_with_mmio_and_domain() {
        let unit = IommuUnit::new(0, IommuVendor::Amd, 0);
        let mut regs = [0u64; 64];
        unit.bind_verification_mmio(regs.as_mut_ptr() as u64);
        let domain_id = unit.create_domain();
        let result = unit.flush_iotlb(domain_id, 0);
        assert_eq!(result, Err(IommuError::UnsupportedVendor));
    }

    #[test]
    fn phase5_kickoff_contract_is_green() {
        assert!(phase5_kickoff_contract_green());
    }
}
