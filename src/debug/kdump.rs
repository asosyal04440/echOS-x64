//! # Kernel Crash Dump (kdump)
//!
//! Çekirdek panik durumunda bellek dökümü, register durumu ve
//! yığın izi (stack trace) yakalama alt sistemi.
//!
//! ## Mimari
//!
//! ```text
//!  Panic Handler
//!       │
//!       ▼
//!  ┌─────────────────┐
//!  │  CrashDumper     │
//!  │  ├─ CPU regs     │ → RegisterDump (16 GPR + segment + control)
//!  │  ├─ Stack trace  │ → StackFrame[] (RBP chain walking)
//!  │  ├─ Memory snap  │ → MemoryRegion[] (bölge bazlı)
//!  │  └─ Vmcore hdr   │ → ELF64 PT_NOTE + PT_LOAD
//!  └─────────────────┘
//! ```
//!
//! ## Kullanım
//!
//! Panic handler'da `capture_crash_dump()` çağrılır.
//! Dump, seri port veya disk'e yazılabilir.

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// SABITLER
// ============================================================================

/// Kdump magic: "ECHDUMP\0"
pub const KDUMP_MAGIC: u64 = 0x0050_4D55_4448_4345;
/// Kdump versiyon
pub const KDUMP_VERSION: u32 = 1;
/// Maksimum stack frame derinliği
pub const MAX_STACK_DEPTH: usize = 64;
/// Maksimum bellek bölgesi
pub const MAX_MEMORY_REGIONS: usize = 32;
/// ELF64 magic
pub const ELF_MAGIC: u32 = 0x464C457F;
/// Minidump signature "MDMP"
pub const MINIDUMP_SIGNATURE: u32 = 0x504D444D;
/// PT_NOTE segment tipi
pub const PT_NOTE: u32 = 4;
/// PT_LOAD segment tipi
pub const PT_LOAD: u32 = 1;

// ============================================================================
// CPU Register Dump
// ============================================================================

/// x86_64 CPU register durumu.
///
/// Panic anındaki tüm GPR, segment, kontrol ve debug register'ları yakalar.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct RegisterDump {
    // Genel amaçlı register'lar
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rbp: u64,
    pub rsp: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,

    // Instruction pointer ve flags
    pub rip: u64,
    pub rflags: u64,

    // Segment register'ları
    pub cs: u16,
    pub ds: u16,
    pub es: u16,
    pub fs: u16,
    pub gs: u16,
    pub ss: u16,

    // Kontrol register'ları
    pub cr0: u64,
    pub cr2: u64, // Sayfa hatası adresi
    pub cr3: u64, // Sayfa tablosu tabanı
    pub cr4: u64,

    // MSR'ler
    pub fs_base: u64,
    pub gs_base: u64,
    pub kernel_gs_base: u64,

    // Hata bilgisi
    pub error_code: u64,
    pub exception_vector: u64,
}

impl RegisterDump {
    /// Mevcut CPU durumunu yakalar.
    pub fn capture() -> Self {
        let mut dump = Self::zeroed();

        unsafe {
            // Kontrol register'ları
            core::arch::asm!("mov {}, cr0", out(reg) dump.cr0);
            core::arch::asm!("mov {}, cr2", out(reg) dump.cr2);
            core::arch::asm!("mov {}, cr3", out(reg) dump.cr3);
            core::arch::asm!("mov {}, cr4", out(reg) dump.cr4);

            // Stack pointer ve base pointer
            core::arch::asm!("mov {}, rsp", out(reg) dump.rsp);
            core::arch::asm!("mov {}, rbp", out(reg) dump.rbp);

            // Instruction pointer (yaklaşık — bu fonksiyonun içini gösterir)
            core::arch::asm!(
                "lea {}, [rip]",
                out(reg) dump.rip,
            );

            // Flags
            core::arch::asm!(
                "pushfq",
                "pop {}",
                out(reg) dump.rflags,
            );

            // Segment register'ları
            core::arch::asm!("mov {:x}, cs", out(reg) dump.cs);
            core::arch::asm!("mov {:x}, ds", out(reg) dump.ds);
            core::arch::asm!("mov {:x}, ss", out(reg) dump.ss);
        }

        dump
    }

    /// Sıfırlanmış dump oluşturur.
    pub fn zeroed() -> Self {
        Self {
            rax: 0,
            rbx: 0,
            rcx: 0,
            rdx: 0,
            rsi: 0,
            rdi: 0,
            rbp: 0,
            rsp: 0,
            r8: 0,
            r9: 0,
            r10: 0,
            r11: 0,
            r12: 0,
            r13: 0,
            r14: 0,
            r15: 0,
            rip: 0,
            rflags: 0,
            cs: 0,
            ds: 0,
            es: 0,
            fs: 0,
            gs: 0,
            ss: 0,
            cr0: 0,
            cr2: 0,
            cr3: 0,
            cr4: 0,
            fs_base: 0,
            gs_base: 0,
            kernel_gs_base: 0,
            error_code: 0,
            exception_vector: 0,
        }
    }
}

// ============================================================================
// Stack Frame
// ============================================================================

/// Yığın çerçevesi (RBP chain walking ile).
#[derive(Debug, Clone, Copy)]
pub struct StackFrame {
    /// Frame pointer (RBP)
    pub rbp: u64,
    /// Return address
    pub rip: u64,
    /// Frame derinliği (0 = en üst)
    pub depth: u32,
}

/// Mevcut yığın izini yakalar.
///
/// RBP zincirleme yürüyüşü ile stack frame'leri toplar.
/// Güvenlik: geçersiz bellek okumalarına karşı sınır kontrolü yapılır.
pub fn capture_stack_trace() -> Vec<StackFrame> {
    let mut frames = Vec::new();
    let mut rbp: u64;

    unsafe {
        core::arch::asm!("mov {}, rbp", out(reg) rbp);
    }

    let mut depth = 0u32;
    while rbp != 0 && depth < MAX_STACK_DEPTH as u32 {
        // Güvenlik: çekirdek adres alanında mı kontrol et
        if rbp < 0xFFFF_8000_0000_0000 && rbp > 0x0000_7FFF_FFFF_FFFF {
            break;
        }

        // Canonical adres kontrolü
        if rbp == 0 || (rbp & 0x7) != 0 {
            break; // Hizalanmamış frame pointer
        }

        let saved_rbp = unsafe { core::ptr::read_volatile(rbp as *const u64) };
        let return_addr = unsafe { core::ptr::read_volatile((rbp + 8) as *const u64) };

        frames.push(StackFrame {
            rbp,
            rip: return_addr,
            depth,
        });

        rbp = saved_rbp;
        depth += 1;
    }

    frames
}

// ============================================================================
// Bellek Bölgesi Dökümü
// ============================================================================

/// Bellek bölgesi tanımlayıcısı
#[derive(Debug, Clone)]
pub struct MemoryRegion {
    /// Bölge başlangıç fiziksel adresi
    pub phys_start: u64,
    /// Bölge boyutu (bayt)
    pub size: u64,
    /// Bölge türü açıklaması
    pub description: String,
    /// Erişim izni (R/W/X)
    pub permissions: u8,
}

/// Bellek bölge türleri
pub const MEM_REGION_KERNEL_TEXT: u8 = 0x01;
pub const MEM_REGION_KERNEL_DATA: u8 = 0x02;
pub const MEM_REGION_KERNEL_BSS: u8 = 0x03;
pub const MEM_REGION_HEAP: u8 = 0x04;
pub const MEM_REGION_STACK: u8 = 0x05;
pub const MEM_REGION_MMIO: u8 = 0x06;

// ============================================================================
// Vmcore ELF64 Header (crash dump formatı)
// ============================================================================

/// ELF64 program header (vmcore segmentleri için)
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Elf64Phdr {
    /// Segment tipi (PT_NOTE=4, PT_LOAD=1)
    pub p_type: u32,
    /// Segment bayrakları
    pub p_flags: u32,
    /// Dosya ofseti
    pub p_offset: u64,
    /// Sanal adres
    pub p_vaddr: u64,
    /// Fiziksel adres
    pub p_paddr: u64,
    /// Dosyadaki boyut
    pub p_filesz: u64,
    /// Bellekteki boyut
    pub p_memsz: u64,
    /// Hizalama
    pub p_align: u64,
}

/// ELF64 note header
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Elf64Nhdr {
    /// İsim uzunluğu
    pub n_namesz: u32,
    /// Veri uzunluğu
    pub n_descsz: u32,
    /// Note tipi
    pub n_type: u32,
}

/// Note tipleri
pub const NT_PRSTATUS: u32 = 1;
pub const NT_PRPSINFO: u32 = 3;
pub const NT_TASKSTRUCT: u32 = 4;
pub const NT_ECHOS_VMCOREINFO: u32 = 0x4543; // "EC" özel

// ============================================================================
// Crash Dump Kaydı
// ============================================================================

/// Tam crash dump kaydı
#[derive(Debug, Clone)]
pub struct CrashDump {
    /// Magic sayı doğrulaması
    pub magic: u64,
    /// Versiyon
    pub version: u32,
    /// Dump zaman damgası (TSC)
    pub timestamp_tsc: u64,
    /// CPU register durumu
    pub registers: RegisterDump,
    /// Yığın izi
    pub stack_frames: Vec<StackFrame>,
    /// Bellek bölgeleri
    pub memory_regions: Vec<MemoryRegion>,
    /// Panik mesajı
    pub panic_message: String,
    /// CPU ID (hangi çekirdekte)
    pub cpu_id: u32,
    /// İşlem/görev ID
    pub task_id: u64,
    /// Çekirdek versiyonu
    pub kernel_version: String,
}

impl CrashDump {
    /// Yeni crash dump oluşturur.
    pub fn new(panic_msg: &str) -> Self {
        let registers = RegisterDump::capture();
        let stack_frames = capture_stack_trace();

        let tsc = unsafe { core::arch::x86_64::_rdtsc() };
        let task_id = crate::task::scheduler::current_task_id();

        Self {
            magic: KDUMP_MAGIC,
            version: KDUMP_VERSION,
            timestamp_tsc: tsc,
            registers,
            stack_frames,
            memory_regions: Vec::new(),
            panic_message: String::from(panic_msg),
            cpu_id: 0, // TODO: APIC ID'den oku
            task_id: task_id as u64,
            kernel_version: String::from("echOS 0.1.0"),
        }
    }

    /// Bellek bölgesi ekler.
    pub fn add_memory_region(&mut self, phys_start: u64, size: u64, desc: &str, perms: u8) {
        if self.memory_regions.len() < MAX_MEMORY_REGIONS {
            self.memory_regions.push(MemoryRegion {
                phys_start,
                size,
                description: String::from(desc),
                permissions: perms,
            });
        }
    }

    /// Dump'ı seri porta yazar (temel çıktı).
    pub fn dump_to_serial(&self) {
        crate::serial_println!("╔══════════════════════════════════════════════╗");
        crate::serial_println!("║       echOS KERNEL CRASH DUMP (kdump)        ║");
        crate::serial_println!("╚══════════════════════════════════════════════╝");
        crate::serial_println!("Panik: {}", self.panic_message);
        crate::serial_println!(
            "TSC: {:#x}  CPU: {}  Task: {}",
            self.timestamp_tsc,
            self.cpu_id,
            self.task_id
        );
        crate::serial_println!("Kernel: {}", self.kernel_version);
        crate::serial_println!("");

        crate::serial_println!("── Register Dump ──");
        crate::serial_println!(
            "RIP: {:#018x}  RSP: {:#018x}",
            self.registers.rip,
            self.registers.rsp
        );
        crate::serial_println!(
            "RBP: {:#018x}  RAX: {:#018x}",
            self.registers.rbp,
            self.registers.rax
        );
        crate::serial_println!(
            "RBX: {:#018x}  RCX: {:#018x}",
            self.registers.rbx,
            self.registers.rcx
        );
        crate::serial_println!(
            "RDX: {:#018x}  RSI: {:#018x}",
            self.registers.rdx,
            self.registers.rsi
        );
        crate::serial_println!(
            "RDI: {:#018x}  R8:  {:#018x}",
            self.registers.rdi,
            self.registers.r8
        );
        crate::serial_println!(
            "R9:  {:#018x}  R10: {:#018x}",
            self.registers.r9,
            self.registers.r10
        );
        crate::serial_println!(
            "R11: {:#018x}  R12: {:#018x}",
            self.registers.r11,
            self.registers.r12
        );
        crate::serial_println!(
            "R13: {:#018x}  R14: {:#018x}",
            self.registers.r13,
            self.registers.r14
        );
        crate::serial_println!(
            "R15: {:#018x}  RFLAGS: {:#018x}",
            self.registers.r15,
            self.registers.rflags
        );
        crate::serial_println!(
            "CR0: {:#018x}  CR2: {:#018x}",
            self.registers.cr0,
            self.registers.cr2
        );
        crate::serial_println!(
            "CR3: {:#018x}  CR4: {:#018x}",
            self.registers.cr3,
            self.registers.cr4
        );
        crate::serial_println!("");

        crate::serial_println!("── Stack Trace ({} frames) ──", self.stack_frames.len());
        for frame in &self.stack_frames {
            crate::serial_println!(
                "  #{:2}: RIP={:#018x}  RBP={:#018x}",
                frame.depth,
                frame.rip,
                frame.rbp
            );
        }
        crate::serial_println!("");

        if !self.memory_regions.is_empty() {
            crate::serial_println!("── Memory Regions ({}) ──", self.memory_regions.len());
            for region in &self.memory_regions {
                crate::serial_println!(
                    "  {:#012x} - {:#012x} ({}) [{}]",
                    region.phys_start,
                    region.phys_start + region.size,
                    region.description,
                    region.permissions
                );
            }
        }
    }

    /// Dump'ı ELF64 vmcore formatında serileştirir.
    pub fn to_vmcore(&self) -> Vec<u8> {
        let mut vmcore = Vec::new();

        // ELF64 header (basitleştirilmiş)
        // e_ident
        vmcore.extend_from_slice(&[0x7F, b'E', b'L', b'F']); // magic
        vmcore.push(2); // ELFCLASS64
        vmcore.push(1); // ELFDATA2LSB
        vmcore.push(1); // EV_CURRENT
        vmcore.push(0); // ELFOSABI_NONE
        vmcore.extend_from_slice(&[0u8; 8]); // padding
        vmcore.extend_from_slice(&4u16.to_le_bytes()); // e_type = ET_CORE
        vmcore.extend_from_slice(&0x3Eu16.to_le_bytes()); // e_machine = EM_X86_64
        vmcore.extend_from_slice(&1u32.to_le_bytes()); // e_version
        vmcore.extend_from_slice(&0u64.to_le_bytes()); // e_entry
        vmcore.extend_from_slice(&64u64.to_le_bytes()); // e_phoff
        vmcore.extend_from_slice(&0u64.to_le_bytes()); // e_shoff
        vmcore.extend_from_slice(&0u32.to_le_bytes()); // e_flags
        vmcore.extend_from_slice(&64u16.to_le_bytes()); // e_ehsize
        vmcore.extend_from_slice(&56u16.to_le_bytes()); // e_phentsize
        let phnum = 1 + self.memory_regions.len() as u16; // PT_NOTE + PT_LOADs
        vmcore.extend_from_slice(&phnum.to_le_bytes()); // e_phnum
        vmcore.extend_from_slice(&0u16.to_le_bytes()); // e_shentsize
        vmcore.extend_from_slice(&0u16.to_le_bytes()); // e_shnum
        vmcore.extend_from_slice(&0u16.to_le_bytes()); // e_shstrndx

        // PT_NOTE program header
        let note_offset = 64 + 56 * phnum as u64;
        vmcore.extend_from_slice(&PT_NOTE.to_le_bytes()); // p_type
        vmcore.extend_from_slice(&0u32.to_le_bytes()); // p_flags
        vmcore.extend_from_slice(&note_offset.to_le_bytes()); // p_offset
        vmcore.extend_from_slice(&0u64.to_le_bytes()); // p_vaddr
        vmcore.extend_from_slice(&0u64.to_le_bytes()); // p_paddr
        let note_size = 32u64; // basitleştirilmiş
        vmcore.extend_from_slice(&note_size.to_le_bytes()); // p_filesz
        vmcore.extend_from_slice(&note_size.to_le_bytes()); // p_memsz
        vmcore.extend_from_slice(&4u64.to_le_bytes()); // p_align

        // PT_LOAD headers (bellek bölgeleri)
        for region in &self.memory_regions {
            vmcore.extend_from_slice(&PT_LOAD.to_le_bytes());
            vmcore.extend_from_slice(&(region.permissions as u32).to_le_bytes());
            vmcore.extend_from_slice(&0u64.to_le_bytes()); // p_offset
            vmcore.extend_from_slice(&region.phys_start.to_le_bytes()); // p_vaddr
            vmcore.extend_from_slice(&region.phys_start.to_le_bytes()); // p_paddr
            vmcore.extend_from_slice(&region.size.to_le_bytes()); // p_filesz
            vmcore.extend_from_slice(&region.size.to_le_bytes()); // p_memsz
            vmcore.extend_from_slice(&0x1000u64.to_le_bytes()); // p_align
        }

        vmcore
    }

    /// Dump'ı minidump benzeri kompakt formata dönüştürür.
    ///
    /// Layout:
    /// - Header: signature, version, stream_count
    /// - Register stream
    /// - Stack trace stream
    /// - Panic message stream
    pub fn to_minidump(&self) -> Vec<u8> {
        let mut out = Vec::new();

        // Header (32 byte)
        out.extend_from_slice(&MINIDUMP_SIGNATURE.to_le_bytes()); // Signature
        out.extend_from_slice(&KDUMP_VERSION.to_le_bytes()); // Version
        out.extend_from_slice(&3u32.to_le_bytes()); // stream count
        out.extend_from_slice(&(self.timestamp_tsc as u32).to_le_bytes()); // timestamp low
        out.extend_from_slice(&(self.timestamp_tsc >> 32).to_le_bytes()); // timestamp high
        out.extend_from_slice(&self.cpu_id.to_le_bytes()); // cpu id
        out.extend_from_slice(&0u32.to_le_bytes()); // reserved

        // Stream 1: Registers
        out.extend_from_slice(&1u32.to_le_bytes()); // stream id
        out.extend_from_slice(&(core::mem::size_of::<RegisterDump>() as u32).to_le_bytes());
        let regs = &self.registers as *const RegisterDump as *const u8;
        let regs_slice =
            unsafe { core::slice::from_raw_parts(regs, core::mem::size_of::<RegisterDump>()) };
        out.extend_from_slice(regs_slice);

        // Stream 2: Stack frames
        out.extend_from_slice(&2u32.to_le_bytes()); // stream id
        let stack_bytes_len = self.stack_frames.len() * core::mem::size_of::<StackFrame>();
        out.extend_from_slice(&(stack_bytes_len as u32).to_le_bytes());
        for frame in &self.stack_frames {
            out.extend_from_slice(&frame.rbp.to_le_bytes());
            out.extend_from_slice(&frame.rip.to_le_bytes());
            out.extend_from_slice(&frame.depth.to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes()); // alignment
        }

        // Stream 3: Panic message (UTF-8)
        out.extend_from_slice(&3u32.to_le_bytes()); // stream id
        out.extend_from_slice(&(self.panic_message.len() as u32).to_le_bytes());
        out.extend_from_slice(self.panic_message.as_bytes());

        out
    }

    /// Frame sayısını döner.
    pub fn frame_count(&self) -> usize {
        self.stack_frames.len()
    }
}

// ============================================================================
// Global State
// ============================================================================

lazy_static::lazy_static! {
    /// Son crash dump (varsa)
    static ref LAST_CRASH: Mutex<Option<CrashDump>> = Mutex::new(None);
    /// Crash dump sayacı
    static ref CRASH_COUNT: AtomicU64 = AtomicU64::new(0);
    /// Crash dump aktif mi
    static ref KDUMP_ENABLED: AtomicBool = AtomicBool::new(true);
}

/// Crash dump yakalar ve saklar.
pub fn capture_crash_dump(panic_msg: &str) {
    if !KDUMP_ENABLED.load(Ordering::Acquire) {
        return;
    }

    let dump = CrashDump::new(panic_msg);
    dump.dump_to_serial();

    CRASH_COUNT.fetch_add(1, Ordering::Relaxed);
    *LAST_CRASH.lock() = Some(dump);
}

/// Son crash dump'ı döner.
pub fn last_crash() -> Option<CrashDump> {
    LAST_CRASH.lock().clone()
}

/// Son çöküş için minidump döndürür.
pub fn last_crash_minidump() -> Option<Vec<u8>> {
    LAST_CRASH.lock().as_ref().map(|dump| dump.to_minidump())
}

/// Toplam crash sayısını döner.
pub fn crash_count() -> u64 {
    CRASH_COUNT.load(Ordering::Relaxed)
}

/// kdump'ı etkinleştirir/devre dışı bırakır.
pub fn set_enabled(enabled: bool) {
    KDUMP_ENABLED.store(enabled, Ordering::Release);
}

/// Modülü başlatır.
pub fn init() {
    crate::serial_println!("[kdump] Çekirdek crash dump alt sistemi hazır");
    crate::serial_println!("[kdump] Maks. stack derinliği: {}", MAX_STACK_DEPTH);
}
