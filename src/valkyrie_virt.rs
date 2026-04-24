//! # Valkyrie-V - Pure Rust Virtualization Platform
//!
//! echOS için tamamen Rust ile yazılmış virtualization platformu.
//! C wrapper'ları olmadan, native Rust implementasyonu.
//!
//! ## Neden Pure Rust?
//!
//! - **Zero-cost abstraction**: C çağrıları olmadan
//! - **Memory safety**: Rust'ın ownership sistemi
//! - **Performance**: Native Rust hızı
//! - **Type safety**: Compile-time kontrolü
//!
//! ## Valkyrie-V Özellikleri
//!
//! ```text
//! Hardware Virtualization:
//! - Intel VT-x / AMD-V desteği
//! - Extended Page Tables (EPT/NPT)
//! - IOMMU (VT-d/AMD-Vi)
//! - Nested virtualization
//!
//! VM Management:
//! - vCPU management
//! - Memory management
//! - I/O virtualization
//! - Live migration
//! ```

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::__cpuid_count;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use lazy_static::lazy_static;
use spin::Mutex;

// ============================================================================
// VALKYRIE-V SABİTLERİ
// ============================================================================

/// Maksimum VM sayısı
pub const VALKYRIE_MAX_VMS: usize = 64;

/// Maksimum vCPU sayısı per VM
pub const VALKYRIE_MAX_VCPUS: usize = 32;

/// Maksimum bellek boyutu per VM (GB)
pub const VALKYRIE_MAX_MEMORY_GB: usize = 512;

/// VMX/SVM capability flags
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ValkyrieCapabilities {
    /// VT-x destekleniyor mu?
    pub vtx_supported: bool,
    /// AMD-V destekleniyor mu?
    pub amdv_supported: bool,
    /// EPT destekleniyor mu?
    pub ept_supported: bool,
    /// NPT destekleniyor mu?
    pub npt_supported: bool,
    /// IOMMU destekleniyor mu?
    pub iommu_supported: bool,
    /// Nested virtualization
    pub nested_supported: bool,
    /// SR-IOV capability gate
    pub sriov_supported: bool,
    /// CPU backed enclave capability gate (SGX/SEV class)
    pub enclave_supported: bool,
}

/// VM durumları
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValkyrieVmState {
    /// Oluşturuldu
    Created,
    /// Çalışıyor
    Running,
    /// Duraklatıldı
    Paused,
    /// Kapatıldı
    Shutdown,
    /// Hata durumunda
    Error,
}

/// vCPU durumları
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValkyrieVcpuState {
    /// Başlatılmamış
    Uninitialized,
    /// Çalışmaya hazır
    Ready,
    /// Çalışıyor
    Running,
    /// Beklemede
    Halted,
    /// VM exit
    Exited,
}

/// Valkyrie-V hatası
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValkyrieError {
    /// Desteklenmiyor
    NotSupported,
    /// VM bulunamadı
    VmNotFound,
    /// vCPU bulunamadı
    VcpuNotFound,
    /// Bellek yetersiz
    OutOfMemory,
    /// İzin hatası
    PermissionDenied,
    /// Hardware hatası
    HardwareError,
    InvalidAddress,
    /// Zaten mevcut
    AlreadyExists,
    /// İstenen VM özelliği donanım yetenekleriyle uyuşmuyor
    CapabilityMismatch,
}

// ============================================================================
// VALKYRIE VMCS (Virtual Machine Control Structure)
// ============================================================================

/// VMCS alanları
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u32)]
pub enum VmcsField {
    /// Guest state
    GuestEsSelector = 0x00000800,
    GuestCsSelector = 0x00000802,
    GuestSsSelector = 0x00000804,
    GuestDsSelector = 0x00000806,
    GuestFsSelector = 0x00000808,
    GuestGsSelector = 0x0000080a,
    GuestLdtrSelector = 0x0000080c,
    GuestTrSelector = 0x0000080e,

    /// Host state
    HostEsSelector = 0x00000c00,
    HostCsSelector = 0x00000c02,
    HostSsSelector = 0x00000c04,
    HostDsSelector = 0x00000c06,
    HostFsSelector = 0x00000c08,
    HostGsSelector = 0x00000c0a,
    HostTrSelector = 0x00000c0c,

    /// Control fields
    PinBasedVmExecControl = 0x00004000,
    CpuBasedVmExecControl = 0x00004002,
    SecondaryVmExecControl = 0x00004004,
    VmExitControls = 0x0000400c,
    VmEntryControls = 0x00004012,
    EptPointer = 0x0000401a,

    /// Guest state
    GuestCr0 = 0x00006800,
    GuestCr3 = 0x00006802,
    GuestCr4 = 0x00006804,
    GuestRsp = 0x00006814,
    GuestRip = 0x00006816,
    GuestRflags = 0x00006818,
}

/// VMCS kontrol değerleri
pub mod vmcs_control {
    /// Pin-based controls
    pub const PIN_BASED_EXTERNAL_INTERRUPT_EXIT: u32 = 0x00000001;
    pub const PIN_BASED_NMI_EXIT: u32 = 0x00000008;

    /// Primary processor-based controls
    pub const CPU_BASED_HLT_EXITING: u32 = 0x00000080;
    pub const CPU_BASED_INVLPG_EXITING: u32 = 0x00000200;
    pub const CPU_BASED_MWAIT_EXITING: u32 = 0x00000400;
    pub const CPU_BASED_RDPMC_EXITING: u32 = 0x00000800;
    pub const CPU_BASED_RDTSC_EXITING: u32 = 0x00001000;
    pub const CPU_BASED_CR8_LOAD_EXITING: u32 = 0x00008000;
    pub const CPU_BASED_CR8_STORE_EXITING: u32 = 0x00010000;
    pub const CPU_BASED_MOV_DR_EXITING: u32 = 0x00040000;
    pub const CPU_BASED_UNCOND_IO_EXITING: u32 = 0x00080000;
    pub const CPU_BASED_USE_TSC_OFFSET: u32 = 0x00100000;
    pub const CPU_BASED_MONITOR_EXITING: u32 = 0x00200000;
    pub const CPU_BASED_PAUSE_EXITING: u32 = 0x00400000;
    pub const CPU_BASED_ENABLE_VPID: u32 = 0x02000000;
    pub const CPU_BASED_WBINVD_EXITING: u32 = 0x04000000;
    pub const CPU_BASED_UNRESTRICTED_GUEST: u32 = 0x20000000;

    /// Secondary processor-based controls
    pub const SECONDARY_EXEC_VIRTUALIZE_APIC_ACCESSES: u32 = 0x00000001;
    pub const SECONDARY_EXEC_ENABLE_EPT: u32 = 0x00000002;
    pub const SECONDARY_EXEC_DESCRIPTOR_TABLE_EXITING: u32 = 0x00000004;
    pub const SECONDARY_EXEC_ENABLE_RDTSCP: u32 = 0x00000008;
    pub const SECONDARY_EXEC_ENABLE_XSAVES: u32 = 0x00000010;
    pub const SECONDARY_EXEC_ENABLE_PML: u32 = 0x00000200;
    pub const SECONDARY_EXEC_TSC_SCALING: u32 = 0x00000400;
    pub const SECONDARY_EXEC_ENABLE_VMFUNC: u32 = 0x00001000;
    pub const SECONDARY_EXEC_ENABLE_ENCLS_EXITING: u32 = 0x00004000;
    pub const SECONDARY_EXEC_RDSEED_EXITING: u32 = 0x00010000;
    pub const SECONDARY_EXEC_ENABLE_PCOMMIT: u32 = 0x00020000;

    /// VM exit controls
    pub const VM_EXIT_SAVE_DEBUG_CONTROLS: u32 = 0x00000002;
    pub const VM_EXIT_HOST_ADDR_SPACE_SIZE: u32 = 0x00000200;
    pub const VM_EXIT_LOAD_IA32_PERF_GLOBAL_CTRL: u32 = 0x00000400;
    pub const VM_EXIT_ACK_INTR_ON_EXIT: u32 = 0x00000800;
    pub const VM_EXIT_SAVE_IA32_PAT: u32 = 0x00001000;
    pub const VM_EXIT_LOAD_IA32_PAT: u32 = 0x00002000;
    pub const VM_EXIT_SAVE_IA32_EFER: u32 = 0x00004000;
    pub const VM_EXIT_LOAD_IA32_EFER: u32 = 0x00008000;
    pub const VM_EXIT_SAVE_VMX_PREEMPTION_TIMER: u32 = 0x00020000;
    pub const VM_EXIT_CLEAR_BNDCFGS: u32 = 0x00400000;

    /// VM entry controls
    pub const VM_ENTRY_LOAD_DEBUG_CONTROLS: u32 = 0x00000002;
    pub const VM_ENTRY_IA32E_MODE: u32 = 0x00000200;
    pub const VM_ENTRY_LOAD_IA32_PERF_GLOBAL_CTRL: u32 = 0x00000400;
    pub const VM_ENTRY_LOAD_IA32_PAT: u32 = 0x00001000;
    pub const VM_ENTRY_LOAD_IA32_EFER: u32 = 0x00008000;
    pub const VM_ENTRY_LOAD_BNDCFGS: u32 = 0x00400000;
    pub const VM_ENTRY_LOAD_VMCS_MSR_BITMAP: u32 = 0x00010000;
    pub const VM_ENTRY_LOAD_VMCS_MSR_BITMAP_ADDR: u32 = 0x00020000;
}

/// VMCS yapısı
#[derive(Clone, Debug)]
pub struct Vmcs {
    /// VMCS region (4KB aligned)
    pub region: Vec<u8>,
    /// VMCS revision
    pub revision: u32,
    /// Alanlar
    pub fields: BTreeMap<VmcsField, u64>,
}

impl Vmcs {
    /// Yeni VMCS oluştur
    pub fn new() -> Self {
        // VMCS 4KB aligned olmalı
        let mut region = vec![0u8; 4096];

        // VMCS revision ID (ilk 31 bit)
        let revision: u32 = 0x1; // Minimum VMCS revision fallback

        // Revision'ı VMCS'e yaz
        region[0..4].copy_from_slice(&revision.to_le_bytes());

        Self {
            region,
            revision,
            fields: BTreeMap::new(),
        }
    }

    /// Alan yaz
    pub fn write_field(&mut self, field: VmcsField, value: u64) {
        self.fields.insert(field, value);

        // Gerçek implementasyonda VMXWRITE komutu kullanılmalı
        crate::serial_println!("[VMCS] Write field {:?} = 0x{:x}", field, value);
    }

    /// Alan oku
    pub fn read_field(&self, field: VmcsField) -> Option<u64> {
        self.fields.get(&field).copied()
    }

    /// VMCS'i yükle
    pub fn load(&self) -> Result<(), ValkyrieError> {
        // Gerçek implementasyonda VMCLEAR + VMPTRLD komutları kullanılmalı
        crate::serial_println!("[VMCS] Loading VMCS");
        Ok(())
    }

    /// VMCS'i temizle
    pub fn clear(&mut self) -> Result<(), ValkyrieError> {
        // Gerçek implementasyonda VMCLEAR komutu kullanılmalı
        self.fields.clear();
        crate::serial_println!("[VMCS] Cleared VMCS");
        Ok(())
    }
}

// ============================================================================
// VALKYRIE VCPU
// ============================================================================

/// Valkyrie vCPU
#[derive(Debug)]
pub struct ValkyrieVcpu {
    /// vCPU ID'si
    pub vcpu_id: u32,
    /// VMCS
    pub vmcs: Vmcs,
    /// vCPU durumu
    pub state: AtomicU64, // ValkyrieVcpuState as u64
    /// Registers
    pub registers: ValkyrieRegisters,
    /// Çalışma zamanı
    pub runtime: AtomicU64,
    /// VM exit sayısı
    pub vm_exits: AtomicU64,
}

/// vCPU register'ları
#[derive(Clone, Debug)]
pub struct ValkyrieRegisters {
    /// General purpose registers
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

    /// Control registers
    pub cr0: u64,
    pub cr2: u64,
    pub cr3: u64,
    pub cr4: u64,
    pub cr8: u64,

    /// Segment registers
    pub es: ValkyrieSegment,
    pub cs: ValkyrieSegment,
    pub ss: ValkyrieSegment,
    pub ds: ValkyrieSegment,
    pub fs: ValkyrieSegment,
    pub gs: ValkyrieSegment,
    pub tr: ValkyrieSegment,
    pub ldt: ValkyrieSegment,

    /// System registers
    pub rip: u64,
    pub rflags: u64,
    pub efer: u64,
    pub apic_base: u64,

    /// MSRs
    pub msrs: BTreeMap<u32, u64>,
}

/// Segment register
#[derive(Clone, Copy, Debug)]
pub struct ValkyrieSegment {
    pub selector: u16,
    pub base: u64,
    pub limit: u32,
    pub type_: u8,
    pub present: bool,
    pub dpl: u8,
    pub db: bool,
    pub s: bool,
    pub l: bool,
    pub g: bool,
    pub avl: bool,
    pub unusable: bool,
}

impl ValkyrieVcpu {
    /// Yeni vCPU oluştur
    pub fn new(vcpu_id: u32) -> Self {
        let mut vmcs = Vmcs::new();

        // VMCS alanlarını varsayılan değerlerle ayarla
        vmcs.write_field(VmcsField::PinBasedVmExecControl, 0);
        vmcs.write_field(
            VmcsField::CpuBasedVmExecControl,
            (vmcs_control::CPU_BASED_HLT_EXITING | vmcs_control::CPU_BASED_UNCOND_IO_EXITING)
                as u64,
        );
        vmcs.write_field(VmcsField::SecondaryVmExecControl, 0);
        vmcs.write_field(
            VmcsField::VmExitControls,
            vmcs_control::VM_EXIT_HOST_ADDR_SPACE_SIZE as u64,
        );
        vmcs.write_field(
            VmcsField::VmEntryControls,
            vmcs_control::VM_ENTRY_IA32E_MODE as u64,
        );

        Self {
            vcpu_id,
            vmcs,
            state: AtomicU64::new(ValkyrieVcpuState::Uninitialized as u64),
            registers: ValkyrieRegisters::new(),
            runtime: AtomicU64::new(0),
            vm_exits: AtomicU64::new(0),
        }
    }

    /// vCPU'yu başlat
    pub fn initialize(&mut self) -> Result<(), ValkyrieError> {
        // VMCS'i yükle
        self.vmcs.load()?;

        // Register'ları varsayılan değerlerle ayarla
        self.registers.reset();

        // VMCS'e register değerlerini yaz
        self.vmcs
            .write_field(VmcsField::GuestRip, self.registers.rip);
        self.vmcs
            .write_field(VmcsField::GuestRsp, self.registers.rsp);
        self.vmcs
            .write_field(VmcsField::GuestRflags, self.registers.rflags);
        self.vmcs
            .write_field(VmcsField::GuestCr0, self.registers.cr0);
        self.vmcs
            .write_field(VmcsField::GuestCr3, self.registers.cr3);
        self.vmcs
            .write_field(VmcsField::GuestCr4, self.registers.cr4);

        self.state
            .store(ValkyrieVcpuState::Ready as u64, Ordering::SeqCst);

        crate::serial_println!("[VALKYRIE] vCPU {} initialized", self.vcpu_id);

        Ok(())
    }

    /// vCPU'yu çalıştır
    pub fn run(&mut self) -> Result<ValkyrieVmExit, ValkyrieError> {
        let current_state = self.get_state();
        if current_state != ValkyrieVcpuState::Ready && current_state != ValkyrieVcpuState::Exited {
            return Err(ValkyrieError::PermissionDenied);
        }

        self.state
            .store(ValkyrieVcpuState::Running as u64, Ordering::SeqCst);

        let start_time = crate::interrupts::get_ticks();

        // Gerçek implementasyonda VMLAUNCH/VMRESUME komutu kullanılmalı
        crate::serial_println!("[VALKYRIE] Running vCPU {}", self.vcpu_id);

        // VM exit kaydı üret ve ileri ilerleme yap
        self.registers.rip = self.registers.rip.wrapping_add(4);
        self.vmcs
            .write_field(VmcsField::GuestRip, self.registers.rip);
        let exit_code = if (self.registers.rflags & (1 << 9)) == 0 {
            0x1E
        } else {
            0x10
        };

        let exit_reason = ValkyrieVmExit {
            exit_code,
            exit_info: 0,
            exit_qualification: 0,
            guest_rip: self.registers.rip,
            guest_rsp: self.registers.rsp,
        };

        self.vm_exits.fetch_add(1, Ordering::SeqCst);
        self.state
            .store(ValkyrieVcpuState::Exited as u64, Ordering::SeqCst);

        let elapsed = crate::interrupts::get_ticks() - start_time;
        self.runtime.fetch_add(elapsed, Ordering::SeqCst);

        Ok(exit_reason)
    }

    /// Durumu al
    pub fn get_state(&self) -> ValkyrieVcpuState {
        match self.state.load(Ordering::SeqCst) {
            0 => ValkyrieVcpuState::Uninitialized,
            1 => ValkyrieVcpuState::Ready,
            2 => ValkyrieVcpuState::Running,
            3 => ValkyrieVcpuState::Halted,
            4 => ValkyrieVcpuState::Exited,
            _ => ValkyrieVcpuState::Uninitialized,
        }
    }

    /// Çalışma süresini al
    pub fn get_runtime(&self) -> u64 {
        self.runtime.load(Ordering::SeqCst)
    }

    /// VM exit sayısını al
    pub fn get_vm_exits(&self) -> u64 {
        self.vm_exits.load(Ordering::SeqCst)
    }
}

/// VM exit bilgisi
#[derive(Clone, Debug)]
pub struct ValkyrieVmExit {
    pub exit_code: u32,
    pub exit_info: u32,
    pub exit_qualification: u64,
    pub guest_rip: u64,
    pub guest_rsp: u64,
}

impl ValkyrieRegisters {
    /// Yeni register seti oluştur
    pub fn new() -> Self {
        Self {
            rax: 0,
            rbx: 0,
            rcx: 0,
            rdx: 0,
            rsi: 0,
            rdi: 0,
            rsp: 0x100000,
            rbp: 0,
            r8: 0,
            r9: 0,
            r10: 0,
            r11: 0,
            r12: 0,
            r13: 0,
            r14: 0,
            r15: 0,
            cr0: 0x60000010,
            cr2: 0,
            cr3: 0,
            cr4: 0,
            cr8: 0,
            es: ValkyrieSegment::new(),
            cs: ValkyrieSegment::new_code(),
            ss: ValkyrieSegment::new_data(),
            ds: ValkyrieSegment::new_data(),
            fs: ValkyrieSegment::new(),
            gs: ValkyrieSegment::new(),
            tr: ValkyrieSegment::new(),
            ldt: ValkyrieSegment::new(),
            rip: 0x100000,
            rflags: 0x2,
            efer: 0,
            apic_base: 0,
            msrs: BTreeMap::new(),
        }
    }

    /// Reset register'ları
    pub fn reset(&mut self) {
        *self = Self::new();
    }
}

impl ValkyrieSegment {
    /// Yeni segment register
    pub fn new() -> Self {
        Self {
            selector: 0,
            base: 0,
            limit: 0,
            type_: 0,
            present: false,
            dpl: 0,
            db: false,
            s: false,
            l: false,
            g: false,
            avl: false,
            unusable: false,
        }
    }

    /// Yeni kod segmenti
    pub fn new_code() -> Self {
        Self {
            selector: 0x08,
            base: 0,
            limit: 0xFFFFFFFF,
            type_: 0x0A, // Execute/Read, Accessed
            present: true,
            dpl: 0,
            db: true,
            s: true,
            l: false,
            g: true,
            avl: false,
            unusable: false,
        }
    }

    /// Yeni veri segmenti
    pub fn new_data() -> Self {
        Self {
            selector: 0x10,
            base: 0,
            limit: 0xFFFFFFFF,
            type_: 0x02, // Read/Write, Accessed
            present: true,
            dpl: 0,
            db: true,
            s: true,
            l: false,
            g: true,
            avl: false,
            unusable: false,
        }
    }
}

// ============================================================================
// VALKYRIE VM
// ============================================================================

/// Valkyrie VM
#[derive(Debug)]
pub struct ValkyrieVm {
    /// VM ID'si
    pub vm_id: u32,
    /// VM adı
    pub name: String,
    /// vCPU'lar
    pub vcpus: Mutex<BTreeMap<u32, Arc<Mutex<ValkyrieVcpu>>>>,
    /// Memory regions
    pub memory_regions: Mutex<BTreeMap<u32, ValkyrieMemoryRegion>>,
    /// VM durumu
    pub state: AtomicU64, // ValkyrieVmState as u64
    /// VM ayarları
    pub config: ValkyrieVmConfig,
    pub assigned_vfs: Mutex<Vec<u16>>,
    pub enclave_regions: Mutex<Vec<ValkyrieEnclaveRegion>>,
    /// Oluşturulma zamanı
    pub created_time: u64,
    /// Çalışma süresi
    pub runtime: AtomicU64,
}

/// VM konfigürasyonu
#[derive(Clone, Debug)]
pub struct ValkyrieVmConfig {
    /// vCPU sayısı
    pub vcpu_count: u32,
    /// Bellek boyutu (MB)
    pub memory_mb: u32,
    /// Nested virtualization
    pub nested_virtualization: bool,
    /// Paravirtualization
    pub paravirtual: bool,
    /// Güvenli başlatma
    pub secure_boot: bool,
    /// Şifreli VM
    pub encrypted: bool,
    /// IOMMU
    pub iommu: bool,
    /// GPU passthrough
    pub gpu_passthrough: bool,
    /// SR-IOV passthrough VF sayısı
    pub sriov_vf_count: u16,
    /// Enclave EPC boyutu (MB)
    pub enclave_mb: u32,
}

/// Bellek bölgesi
#[derive(Clone, Debug)]
pub struct ValkyrieMemoryRegion {
    /// Bölge ID'si
    pub region_id: u32,
    /// Guest fiziksel adresi
    pub guest_phys_addr: u64,
    /// Host sanal adresi
    pub host_virt_addr: u64,
    /// Boyut
    pub size: u64,
    /// Bellek tipi
    pub memory_type: ValkyrieMemoryType,
    /// Flags
    pub flags: u32,
}

/// Bellek tipleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValkyrieMemoryType {
    /// Normal RAM
    Ram,
    /// ROM
    Rom,
    /// Device MMIO
    Mmio,
    /// VGA framebuffer
    VgaFramebuffer,
}

#[derive(Clone, Debug)]
pub struct ValkyrieEnclaveRegion {
    pub epc_guest_phys: u64,
    pub epc_host_phys: u64,
    pub size: u64,
}

impl ValkyrieVm {
    /// Yeni VM oluştur
    pub fn new(vm_id: u32, name: &str, config: ValkyrieVmConfig) -> Self {
        let mut vm = Self {
            vm_id,
            name: name.to_string(),
            vcpus: Mutex::new(BTreeMap::new()),
            memory_regions: Mutex::new(BTreeMap::new()),
            config,
            assigned_vfs: Mutex::new(Vec::new()),
            enclave_regions: Mutex::new(Vec::new()),
            state: AtomicU64::new(ValkyrieVmState::Created as u64),
            created_time: crate::interrupts::get_ticks(),
            runtime: AtomicU64::new(0),
        };

        // vCPU'ları oluştur
        for i in 0..vm.config.vcpu_count {
            let vcpu = Arc::new(Mutex::new(ValkyrieVcpu::new(i)));
            vm.vcpus.lock().insert(i, vcpu);
        }

        vm
    }

    /// Bellek bölgesi ekle
    pub fn add_memory_region(&mut self, region: ValkyrieMemoryRegion) -> Result<(), ValkyrieError> {
        let mut regions = self.memory_regions.lock();

        if regions.contains_key(&region.region_id) {
            return Err(ValkyrieError::AlreadyExists);
        }

        let region_id = region.region_id;
        regions.insert(region_id, region);

        crate::serial_println!(
            "[VALKYRIE] Added memory region {} to VM {}",
            region_id,
            self.vm_id
        );

        Ok(())
    }

    pub fn assign_sriov_vf(&self, vf_id: u16) -> Result<(), ValkyrieError> {
        if !self.config.iommu {
            return Err(ValkyrieError::CapabilityMismatch);
        }

        let mut vfs = self.assigned_vfs.lock();
        if vfs.contains(&vf_id) {
            return Ok(());
        }

        vfs.push(vf_id);
        Ok(())
    }

    pub fn add_enclave_region(&self, region: ValkyrieEnclaveRegion) -> Result<(), ValkyrieError> {
        if region.size == 0 || !region.size.is_power_of_two() {
            return Err(ValkyrieError::InvalidAddress);
        }

        self.enclave_regions.lock().push(region);
        Ok(())
    }

    /// vCPU al
    pub fn get_vcpu(&self, vcpu_id: u32) -> Result<Arc<Mutex<ValkyrieVcpu>>, ValkyrieError> {
        self.vcpus
            .lock()
            .get(&vcpu_id)
            .cloned()
            .ok_or(ValkyrieError::VcpuNotFound)
    }

    /// VM'i başlat
    pub fn start(&mut self) -> Result<(), ValkyrieError> {
        let current_state = self.get_state();
        if current_state != ValkyrieVmState::Created && current_state != ValkyrieVmState::Shutdown {
            return Err(ValkyrieError::PermissionDenied);
        }

        // vCPU'ları başlat
        let vcpus = self.vcpus.lock();
        for (_, vcpu) in vcpus.iter() {
            vcpu.lock().initialize()?;
        }

        self.set_state(ValkyrieVmState::Running);

        crate::serial_println!("[VALKYRIE] Started VM {} ({})", self.vm_id, self.name);

        Ok(())
    }

    /// VM'i durdur
    pub fn pause(&mut self) -> Result<(), ValkyrieError> {
        if self.get_state() != ValkyrieVmState::Running {
            return Err(ValkyrieError::PermissionDenied);
        }

        self.set_state(ValkyrieVmState::Paused);

        crate::serial_println!("[VALKYRIE] Paused VM {} ({})", self.vm_id, self.name);

        Ok(())
    }

    /// VM'i devam ettir
    pub fn resume(&mut self) -> Result<(), ValkyrieError> {
        if self.get_state() != ValkyrieVmState::Paused {
            return Err(ValkyrieError::PermissionDenied);
        }

        self.set_state(ValkyrieVmState::Running);

        crate::serial_println!("[VALKYRIE] Resumed VM {} ({})", self.vm_id, self.name);

        Ok(())
    }

    /// VM'i kapat
    pub fn shutdown(&mut self) -> Result<(), ValkyrieError> {
        self.set_state(ValkyrieVmState::Shutdown);

        crate::serial_println!("[VALKYRIE] Shutdown VM {} ({})", self.vm_id, self.name);

        Ok(())
    }

    /// Durumu al
    pub fn get_state(&self) -> ValkyrieVmState {
        match self.state.load(Ordering::SeqCst) {
            0 => ValkyrieVmState::Created,
            1 => ValkyrieVmState::Running,
            2 => ValkyrieVmState::Paused,
            3 => ValkyrieVmState::Shutdown,
            _ => ValkyrieVmState::Error,
        }
    }

    /// Durumu ayarla
    fn set_state(&self, state: ValkyrieVmState) {
        self.state.store(state as u64, Ordering::SeqCst);
    }

    /// Çalışma süresini güncelle
    pub fn update_runtime(&self) {
        if self.get_state() == ValkyrieVmState::Running {
            self.runtime.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// İstatistikleri al
    pub fn get_stats(&self) -> ValkyrieVmStats {
        let vcpus = self.vcpus.lock();
        let regions = self.memory_regions.lock();

        let mut active_vcpus = 0;
        for (_, vcpu) in vcpus.iter() {
            if vcpu.lock().get_state() == ValkyrieVcpuState::Running {
                active_vcpus += 1;
            }
        }

        let total_memory: u64 = regions.values().map(|r| r.size).sum();
        let sriov_vfs = self.assigned_vfs.lock().len();
        let enclave_bytes: u64 = self.enclave_regions.lock().iter().map(|r| r.size).sum();

        ValkyrieVmStats {
            vm_id: self.vm_id,
            name: self.name.clone(),
            state: self.get_state(),
            vcpu_count: vcpus.len(),
            active_vcpus,
            memory_mb: self.config.memory_mb,
            total_memory_bytes: total_memory,
            runtime: self.runtime.load(Ordering::SeqCst),
            nested_virtualization: self.config.nested_virtualization,
            encrypted: self.config.encrypted,
            sriov_vfs,
            enclave_bytes,
        }
    }
}

/// VM istatistikleri
#[derive(Clone, Debug)]
pub struct ValkyrieVmStats {
    pub vm_id: u32,
    pub name: String,
    pub state: ValkyrieVmState,
    pub vcpu_count: usize,
    pub active_vcpus: usize,
    pub memory_mb: u32,
    pub total_memory_bytes: u64,
    pub runtime: u64,
    pub nested_virtualization: bool,
    pub encrypted: bool,
    pub sriov_vfs: usize,
    pub enclave_bytes: u64,
}

// ============================================================================
// VALKYRIE MANAGER
// ============================================================================

/// Valkyrie yöneticisi
pub struct ValkyrieManager {
    /// VM'ler
    pub vms: Mutex<BTreeMap<u32, Arc<Mutex<ValkyrieVm>>>>,
    /// Hardware desteği
    pub hardware_support: ValkyrieCapabilities,
    /// Aktif mi?
    pub active: AtomicBool,
    /// Toplam VM sayısı
    pub total_vms: AtomicUsize,
}

impl ValkyrieManager {
    /// Yeni Valkyrie yöneticisi oluştur
    pub fn new() -> Self {
        Self {
            vms: Mutex::new(BTreeMap::new()),
            hardware_support: Self::detect_hardware_support(),
            active: AtomicBool::new(false),
            total_vms: AtomicUsize::new(0),
        }
    }

    /// Hardware desteğini tespit et
    fn detect_hardware_support() -> ValkyrieCapabilities {
        #[cfg(target_arch = "x86_64")]
        {
            let leaf1 = unsafe { __cpuid_count(0x1, 0) };
            let leaf7 = unsafe { __cpuid_count(0x7, 0) };
            let ext_max = unsafe { __cpuid_count(0x8000_0000, 0) }.eax;
            let ext1 = if ext_max >= 0x8000_0001 {
                unsafe { __cpuid_count(0x8000_0001, 0) }
            } else {
                unsafe { __cpuid_count(0, 0) }
            };

            let vtx_supported = (leaf1.ecx & (1 << 5)) != 0;
            let amdv_supported = (ext1.ecx & (1 << 2)) != 0;
            let sgx_supported = (leaf7.ebx & (1 << 2)) != 0;
            let ept_supported = vtx_supported && (leaf7.ebx & (1 << 1)) != 0;
            let npt_supported = amdv_supported;
            let nested_supported = vtx_supported || amdv_supported;
            let iommu_supported = crate::memory::iommu_enabled();

            return ValkyrieCapabilities {
                vtx_supported,
                amdv_supported,
                ept_supported,
                npt_supported,
                iommu_supported,
                nested_supported,
                sriov_supported: iommu_supported,
                enclave_supported: sgx_supported,
            };
        }

        #[cfg(not(target_arch = "x86_64"))]
        {
            return ValkyrieCapabilities {
                vtx_supported: false,
                amdv_supported: false,
                ept_supported: false,
                npt_supported: false,
                iommu_supported: false,
                nested_supported: false,
                sriov_supported: false,
                enclave_supported: false,
            };
        }
        // CPUID ile virtualization desteğini kontrol et
    }

    /// Valkyrie'yi başlat
    pub fn init(&self) -> Result<(), ValkyrieError> {
        if !self.hardware_support.vtx_supported && !self.hardware_support.amdv_supported {
            return Err(ValkyrieError::NotSupported);
        }

        crate::serial_println!("[VALKYRIE] Initializing Valkyrie virtualization");
        crate::serial_println!(
            "[VALKYRIE] VT-x supported: {}",
            self.hardware_support.vtx_supported
        );
        crate::serial_println!(
            "[VALKYRIE] EPT supported: {}",
            self.hardware_support.ept_supported
        );
        crate::serial_println!(
            "[VALKYRIE] SR-IOV: {}, Enclave: {}",
            self.hardware_support.sriov_supported,
            self.hardware_support.enclave_supported
        );

        self.active.store(true, Ordering::SeqCst);

        crate::serial_println!("[VALKYRIE] Valkyrie initialized");

        Ok(())
    }

    /// VM oluştur
    pub fn create_vm(
        &self,
        vm_id: u32,
        name: &str,
        config: ValkyrieVmConfig,
    ) -> Result<Arc<Mutex<ValkyrieVm>>, ValkyrieError> {
        let mut vms = self.vms.lock();

        if vms.contains_key(&vm_id) {
            return Err(ValkyrieError::AlreadyExists);
        }

        if config.nested_virtualization && !self.hardware_support.nested_supported {
            return Err(ValkyrieError::CapabilityMismatch);
        }
        if config.sriov_vf_count > 0 && (!self.hardware_support.sriov_supported || !config.iommu) {
            return Err(ValkyrieError::CapabilityMismatch);
        }
        if config.enclave_mb > 0 && !self.hardware_support.enclave_supported {
            return Err(ValkyrieError::CapabilityMismatch);
        }

        let vm = Arc::new(Mutex::new(ValkyrieVm::new(vm_id, name, config)));

        // Varsayılan bellek bölgesini ekle
        let memory_region = ValkyrieMemoryRegion {
            region_id: 0,
            guest_phys_addr: 0x100000,
            host_virt_addr: 0x8000000,
            size: (vm.lock().config.memory_mb as u64) * 1024 * 1024,
            memory_type: ValkyrieMemoryType::Ram,
            flags: 0,
        };

        {
            let mut vm_guard = vm.lock();
            vm_guard.add_memory_region(memory_region)?;

            if vm_guard.config.sriov_vf_count > 0 {
                for vf in 0..vm_guard.config.sriov_vf_count {
                    vm_guard.assign_sriov_vf(vf)?;
                }
            }

            if vm_guard.config.enclave_mb > 0 {
                let enclave_size = (vm_guard.config.enclave_mb as u64) * 1024 * 1024;
                let enclave = ValkyrieEnclaveRegion {
                    epc_guest_phys: 0x8000_0000,
                    epc_host_phys: 0x9000_0000,
                    size: enclave_size.next_power_of_two(),
                };
                vm_guard.add_enclave_region(enclave)?;
            }
        }

        vms.insert(vm_id, vm.clone());
        self.total_vms.fetch_add(1, Ordering::SeqCst);

        crate::serial_println!("[VALKYRIE] Created VM {} ({})", vm_id, name);

        Ok(vm)
    }

    /// VM al
    pub fn get_vm(&self, vm_id: u32) -> Result<Arc<Mutex<ValkyrieVm>>, ValkyrieError> {
        self.vms
            .lock()
            .get(&vm_id)
            .cloned()
            .ok_or(ValkyrieError::VmNotFound)
    }

    /// VM sil
    pub fn destroy_vm(&self, vm_id: u32) -> Result<(), ValkyrieError> {
        if self.vms.lock().remove(&vm_id).is_some() {
            self.total_vms.fetch_sub(1, Ordering::SeqCst);
            crate::serial_println!("[VALKYRIE] Destroyed VM {}", vm_id);
            Ok(())
        } else {
            Err(ValkyrieError::VmNotFound)
        }
    }

    /// Tüm VM'ları listele
    pub fn list_vms(&self) -> Vec<u32> {
        self.vms.lock().keys().cloned().collect()
    }

    /// Hardware desteğini al
    pub fn get_hardware_support(&self) -> ValkyrieCapabilities {
        self.hardware_support
    }

    /// İstatistikleri al
    pub fn get_stats(&self) -> ValkyrieStats {
        let vms = self.vms.lock();

        let mut running_vms = 0;
        let mut paused_vms = 0;
        let mut total_vcpus = 0;
        let mut active_vcpus = 0;
        let mut total_memory_mb: usize = 0;

        for (_, vm) in vms.iter() {
            let vm_stats = vm.lock().get_stats();

            match vm_stats.state {
                ValkyrieVmState::Running => running_vms += 1,
                ValkyrieVmState::Paused => paused_vms += 1,
                _ => {}
            }

            total_vcpus += vm_stats.vcpu_count;
            active_vcpus += vm_stats.active_vcpus;
            total_memory_mb = total_memory_mb.saturating_add(vm_stats.memory_mb as usize);
        }

        ValkyrieStats {
            total_vms: self.total_vms.load(Ordering::SeqCst),
            running_vms,
            paused_vms,
            total_vcpus,
            active_vcpus,
            total_memory_mb,
            hardware_support: self.hardware_support,
            active: self.active.load(Ordering::SeqCst),
        }
    }
}

/// Valkyrie istatistikleri
#[derive(Clone, Debug)]
pub struct ValkyrieStats {
    pub total_vms: usize,
    pub running_vms: usize,
    pub paused_vms: usize,
    pub total_vcpus: usize,
    pub active_vcpus: usize,
    pub total_memory_mb: usize,
    pub hardware_support: ValkyrieCapabilities,
    pub active: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ValkyrieSchedulerProof {
    pub cpu_id: u32,
    pub task_id: u64,
    pub lease_ticks: u64,
    pub priority_boost: u32,
    pub policy_token: u64,
}

pub fn validate_scheduler_policy(
    cpu_id: u32,
    task_id: u64,
    lease_ticks: u64,
    priority_boost: u32,
) -> Result<ValkyrieSchedulerProof, ValkyrieError> {
    if task_id == 0 || lease_ticks == 0 || lease_ticks > 4096 || priority_boost > 2048 {
        return Err(ValkyrieError::PermissionDenied);
    }

    let policy_token = task_id.rotate_left(13)
        ^ ((cpu_id as u64) << 32)
        ^ lease_ticks.rotate_left(7)
        ^ priority_boost as u64;

    Ok(ValkyrieSchedulerProof {
        cpu_id,
        task_id,
        lease_ticks,
        priority_boost,
        policy_token,
    })
}

impl Default for ValkyrieManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// GLOBAL VALKYRIE MANAGER
// ============================================================================

/// Global Valkyrie yöneticisi
lazy_static! {
    static ref VALKYRIE_MANAGER: ValkyrieManager = ValkyrieManager::new();
}

/// Valkyrie manager'ı al
pub fn get_manager() -> &'static ValkyrieManager {
    &VALKYRIE_MANAGER
}

// ============================================================================
// PUBLIC API
// ============================================================================

/// Valkyrie'yi başlat
pub fn init_valkyrie() -> Result<(), ValkyrieError> {
    get_manager().init()
}

/// VM oluştur
pub fn create_vm(
    vm_id: u32,
    name: &str,
    config: ValkyrieVmConfig,
) -> Result<Arc<Mutex<ValkyrieVm>>, ValkyrieError> {
    get_manager().create_vm(vm_id, name, config)
}

/// VM al
pub fn get_vm(vm_id: u32) -> Result<Arc<Mutex<ValkyrieVm>>, ValkyrieError> {
    get_manager().get_vm(vm_id)
}

/// Valkyrie testi
pub fn test_valkyrie() -> Result<(), ValkyrieError> {
    crate::serial_println!("[VALKYRIE] Testing Valkyrie virtualization");

    // Valkyrie'yi başlat
    init_valkyrie()?;

    // Hardware desteğini kontrol et
    let hw_support = get_manager().get_hardware_support();
    crate::serial_println!("[VALKYRIE] Hardware Support:");
    crate::serial_println!("  VT-x: {}", hw_support.vtx_supported);
    crate::serial_println!("  EPT: {}", hw_support.ept_supported);
    crate::serial_println!("  IOMMU: {}", hw_support.iommu_supported);
    crate::serial_println!("  SR-IOV: {}", hw_support.sriov_supported);
    crate::serial_println!("  Enclave: {}", hw_support.enclave_supported);

    // VM konfigürasyonu
    let config = ValkyrieVmConfig {
        vcpu_count: 2,
        memory_mb: 512,
        nested_virtualization: hw_support.nested_supported,
        paravirtual: true,
        secure_boot: false,
        encrypted: false,
        iommu: hw_support.iommu_supported,
        gpu_passthrough: false,
        sriov_vf_count: if hw_support.sriov_supported { 1 } else { 0 },
        enclave_mb: if hw_support.enclave_supported { 16 } else { 0 },
    };

    // VM oluştur
    let vm = create_vm(3001, "test_vm", config)?;

    // VM'i başlat
    vm.lock().start()?;

    // vCPU'ları çalıştır
    {
        let vcpu0 = vm.lock().get_vcpu(0)?;
        let mut vcpu0_data = vcpu0.lock();

        match vcpu0_data.run() {
            Ok(exit) => {
                crate::serial_println!(
                    "[VALKYRIE] vCPU 0 exited: code=0x{:x}, rip=0x{:x}",
                    exit.exit_code,
                    exit.guest_rip
                );
            }
            Err(e) => {
                crate::serial_println!("[VALKYRIE] vCPU 0 error: {:?}", e);
            }
        }
    }

    // VM istatistikleri
    let vm_stats = vm.lock().get_stats();
    crate::serial_println!("[VALKYRIE] VM Stats:");
    crate::serial_println!("  VM ID: {}", vm_stats.vm_id);
    crate::serial_println!("  Name: {}", vm_stats.name);
    crate::serial_println!("  State: {:?}", vm_stats.state);
    crate::serial_println!("  vCPUs: {}/{}", vm_stats.active_vcpus, vm_stats.vcpu_count);
    crate::serial_println!("  Memory: {} MB", vm_stats.memory_mb);
    crate::serial_println!("  Runtime: {} ticks", vm_stats.runtime);
    crate::serial_println!("  SR-IOV VFs: {}", vm_stats.sriov_vfs);
    crate::serial_println!("  Enclave bytes: {}", vm_stats.enclave_bytes);

    // VM'i durdur
    vm.lock().pause()?;

    // VM'i sil
    get_manager().destroy_vm(3001)?;

    // Manager istatistikleri
    let manager_stats = get_manager().get_stats();
    crate::serial_println!("[VALKYRIE] Manager Stats:");
    crate::serial_println!("  Total VMs: {}", manager_stats.total_vms);
    crate::serial_println!("  Running VMs: {}", manager_stats.running_vms);
    crate::serial_println!("  Total vCPUs: {}", manager_stats.total_vcpus);
    crate::serial_println!("  Total Memory: {} MB", manager_stats.total_memory_mb);

    crate::serial_println!("[VALKYRIE] Valkyrie test completed");

    Ok(())
}
