//! # echOS Task Yapısı
//!
//! Bu modül, işletim sistemindeki task (görev) yapısını tanımlar.
//! Her task kendi stack'ine, context'ine ve önceliğine sahiptir.

use crate::allocator::stack::KernelStack;
use crate::memory::AddressSpace;
use crate::task::signal::SignalHandlers;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicUsize, Ordering};
use spin::Mutex;
use x86_64::structures::paging::PhysFrame;

/// Task'ların benzersiz kimlik numarası
pub type TaskId = usize;

/// Sonraki task ID için atomik sayaç
static NEXT_ID: AtomicUsize = AtomicUsize::new(1);

// ============================================================================
// PRIORITY (ÖNCELİK)
// ============================================================================

/// Task öncelik seviyeleri.
/// Düşük sayısal değer = yüksek öncelik.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    /// En yüksek öncelik - sistem kritik task'ları
    High = 0,
    /// Normal öncelik - standart task'lar
    Normal = 1,
    /// Düşük öncelik - arka plan işleri
    Low = 2,
    /// En düşük öncelik - sadece CPU boşta olduğunda
    Idle = 3,
}

impl Default for Priority {
    fn default() -> Self {
        Priority::Normal
    }
}

pub fn weight_for_priority(priority: Priority) -> u32 {
    match priority {
        Priority::High => 1536,
        Priority::Normal => 1024,
        Priority::Low => 768,
        Priority::Idle => 256,
    }
}

// ============================================================================
// TASK STATE (DURUM)
// ============================================================================

/// Task'ın mevcut durumu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    /// Çalışmaya hazır, CPU bekliyor
    Ready,
    /// Şu anda CPU'da çalışıyor
    Running,
    /// I/O veya kaynak bekliyor
    Blocked,
    /// Belirli bir tick'e kadar uyuyor
    Sleeping { wake_tick: usize },
    /// Sonlandırıldı, temizlenecek
    Terminated,
    /// Durduruldu (SIGSTOP veya Ctrl+Z)
    Stopped,
    /// Zombi - sonlandı ama parent wait() yapmadı
    Zombie,
}

// ============================================================================
// FPU/SSE/AVX DURUMU — Silicon-Assisted Eager FPU
// ============================================================================

/// XSAVE alan boyutu üst sınırı (AVX-512 dahil).
/// Boot sırasında gerçek boyut `crate::cpu::xsave_area_size()` ile sorgulanır.
pub const XSAVE_MAX_SIZE: usize = 2688;

/// x86_64 XSAVE/XSAVEOPT/XSAVEC ve fallback FXSAVE için durum alanı.
/// Align 64: XSAVE 64-byte alignment gerektirir (FXSAVE için 16 yeterli ama ileriye dönük).
#[repr(C, align(64))]
#[derive(Clone)]
pub struct XSaveArea {
    pub data: [u8; XSAVE_MAX_SIZE],
}

impl core::fmt::Debug for XSaveArea {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("XSaveArea")
            .field("size", &XSAVE_MAX_SIZE)
            .finish()
    }
}

/// Geriye dönük uyumluluk alias — eski kodda FxSaveArea kullanan yerleri kırmamak için
pub type FxSaveArea = XSaveArea;

// ============================================================================
// TASK FLAGS — AOT (Ahead-Of-Time) FPU Hinting
// ============================================================================

/// Task davranış bayrakları — context switch optimizasyonları için.
/// Bitwise flags olarak tasarlanmıştır.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskFlags(u32);

impl TaskFlags {
    /// Boş bayrak kümesi
    pub const NONE: Self = Self(0);
    /// FPU/SSE/AVX kullanmayan kernel task — xsaveopt/xrstor atlanır
    pub const NO_FPU: Self = Self(1 << 0);
    /// FPU durumu henüz başlatılmadı (ilk xrstor'u atla)
    pub const FPU_PRISTINE: Self = Self(1 << 1);
    /// Real-time task — preemption kısıtlı
    pub const REALTIME: Self = Self(1 << 2);
    /// Kernel thread (user-space yok)
    pub const KERNEL_THREAD: Self = Self(1 << 3);

    #[inline(always)]
    pub const fn contains(self, flag: Self) -> bool {
        (self.0 & flag.0) == flag.0
    }

    #[inline(always)]
    pub const fn insert(self, flag: Self) -> Self {
        Self(self.0 | flag.0)
    }

    #[inline(always)]
    pub const fn remove(self, flag: Self) -> Self {
        Self(self.0 & !flag.0)
    }

    /// Raw u32 değerini döndürür (asm'e geçirmek için)
    #[inline(always)]
    pub const fn bits(self) -> u32 {
        self.0
    }
}

// ============================================================================
// TASK CONTEXT
// ============================================================================

/// Task'ın CPU durumu (register'lar).
/// Context switch sırasında kaydedilir ve geri yüklenir.
///
/// Bellek düzeni (offset):
///   0x00: r15, 0x08: r14, 0x10: r13, 0x18: r12
///   0x20: rbx, 0x28: rbp, 0x30: rsp, 0x38: rflags
///   0x40: rip
///   0x48..0x80: _pad (7×u64 = 56 byte, 64-byte alignment sağlar)
///   0x80 (128): fx_state — XSaveArea (64-byte aligned, XSAVE/FXSAVE için)
#[repr(C)]
#[derive(Debug, Clone)]
pub struct TaskContext {
    // Callee-saved register'lar (ABI gereği korunmalı)
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub rbx: u64,
    pub rbp: u64,            // Taban işaretçisi (base pointer)
    pub rsp: u64,            // Yığın işaretçisi (stack pointer)
    pub rflags: u64,         // İşlemci bayrakları (CPU flags)
    pub rip: u64,            // Komut işaretçisi (instruction pointer / dönüş adresi)
    pub _pad: [u64; 7],      // 56 byte pad → fx_state offset = 128 (0x80), 64-byte aligned
    pub fx_state: XSaveArea, // SSE/AVX/FPU durumu (XSAVE formatı)
}

impl TaskContext {
    /// Yeni bir task context oluşturur.
    ///
    /// # Parametreler
    /// - `entry_point`: Task'ın başlangıç adresi
    /// - `stack_top`: Task stack'inin tepesi
    pub fn new(entry_point: u64, stack_top: u64) -> Self {
        Self {
            r15: 0,
            r14: 0,
            r13: 0,
            r12: 0,
            rbx: 0,
            rbp: stack_top,
            rsp: stack_top,
            rflags: 0x202, // Interrupt'lar aktif
            rip: entry_point,
            _pad: [0u64; 7],
            fx_state: XSaveArea {
                data: [0; XSAVE_MAX_SIZE],
            },
        }
    }
}

// Derleme zamanı kontrol: fx_state offseti 128 (0x80) olmalı — XSAVE 64-byte alignment gerektirir
const _: () = assert!(core::mem::offset_of!(TaskContext, fx_state) == 128);

// ============================================================================
// EXECUTION MODE
// ============================================================================

/// Task'ın çalışma modu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionMode {
    /// Ring 0 (kernel mode) - Rust ile yazılmış güvenli task'lar
    NativeRust,
    /// Ring 3 (user mode) - İzole bellek alanında çalışan legacy task'lar
    LegacyRing3,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RseqState {
    pub registered: bool,
    pub area_ptr: u64,
    pub area_len: u32,
    pub signature: u32,
    pub flags: u32,
    pub cpu_id_start: u32,
    pub cpu_id: u32,
    pub numa_node: u32,
    pub abort_count: u64,
    pub event_counter: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Win32ThreadState {
    pub teb_base: u64,
    pub peb_base: u64,
    pub process_parameters_base: u64,
    pub user_stack_top: u64,
    pub entry_rip: u64,
    pub initial_rcx: u64,
    pub heap_seed: u64,
    pub owner_pid: u64,
    pub thread_handle: u64,
    pub gs_base_shadow: u64,
    pub bootstrap_flags: u32,
}

// ============================================================================
// TASK
// ============================================================================

/// Task'ın "Sıcak" verileri - Scheduler tarafından sık erişilenler
#[derive(Debug, Clone)]
pub struct TaskHotData {
    pub id: TaskId,
    pub state: TaskState,
    pub priority: Priority,
    pub vruntime: u64,
    pub weight: u32,
    pub last_start: u64,
    pub affinity: u32,
    pub last_cpu: u32,
    pub kernel_stack_top: u64,
    /// AOT davranış bayrakları — context switch optimizasyonları
    pub flags: TaskFlags,
}

/// Task'ın "Soğuk" verileri - Nadiren erişilen veya sadece context switch'te gerekenler
#[derive(Clone)]
pub struct TaskColdData {
    pub name: &'static str,
    pub mode: ExecutionMode,
    pub page_table: Option<PhysFrame>,
    pub address_space: Option<Arc<Mutex<AddressSpace>>>,
    pub user_entry: Option<u64>,
    pub user_stack_top: Option<u64>,
    pub exit_code: Option<i32>,
    pub wait_ticks: u32,
    pub exec_runtime: u64,
    pub ptrace_flags: u32,
    pub tracer_pid: Option<TaskId>,
    pub seccomp_mode: u32,
    pub stack: KernelStack,
    /// Background task mı (job control için)
    pub is_background: bool,
    /// POSIX sinyal yöneticisi (Arc ile paylaşılır — Clone uyumluluğu için)
    pub signals: Arc<SignalHandlers>,
    /// PCID (Process Context Identifier) — TLB flush optimization.
    /// 0 = kernel PCID, 1..4095 = user task PCID'leri.
    pub pcid: u16,
    /// Üst süreç PID'si — fork()/clone() ile oluşturulmuş ise set edilir
    pub parent_pid: Option<TaskId>,
    /// Alt süreç PID listesi — fork ile oluşturulan çocuklar
    pub children: Vec<TaskId>,
    pub rseq: RseqState,
    pub win32: Option<Win32ThreadState>,
}

/// Bir işletim sistemi task'ı (thread/process).
/// Hot/Cold splitting uygulanmıştır.
#[derive(Clone)]
pub struct Task {
    /// Sık erişilen veriler (Cache-friendly)
    pub hot: TaskHotData,
    /// CPU register durumu (Context switch'te kritik)
    pub context: TaskContext,
    /// Nadiren erişilen veriler (Pointer arkasında da olabilir ama şimdilik struct içinde)
    pub cold: TaskColdData,
}

impl Deref for Task {
    type Target = TaskHotData;

    fn deref(&self) -> &Self::Target {
        &self.hot
    }
}

impl DerefMut for Task {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.hot
    }
}

// Geriye dönük uyumluluk için Deref trait'i kullanılabilir veya
// doğrudan erişim metodları yazılabilir. Şimdilik doğrudan erişim.

impl Task {
    // Geriye dönük uyumluluk için yardımcı erişici metodlar.
    // Mevcut kodu refactor etmeden uyumluluğu koruyan doğrudan erişici metodlar.

    pub fn id(&self) -> TaskId {
        self.hot.id
    }
    pub fn state(&self) -> TaskState {
        self.hot.state
    }
    // ... diğerleri için refactor gerekecek.
}

impl Task {
    /// Normal öncelikle yeni task oluşturur.
    pub fn new(entry_point: fn() -> !) -> Self {
        Self::with_priority(entry_point, Priority::Normal, "unnamed")
    }

    /// Belirtilen öncelikle yeni task oluşturur.
    pub fn with_priority(entry_point: fn() -> !, priority: Priority, name: &'static str) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        Self::with_priority_and_id(entry_point, priority, name, id)
    }

    /// Belirtilen öncelik ve ID ile yeni task oluşturur.
    ///
    /// Stack düzeni (düşük adres → yüksek adres):
    /// ```text
    /// [Guard Page 4KB | Kullanılabilir Stack 64KB]
    ///  ^-- erişilemez     ^-- stack_bottom      ^-- stack_top
    /// ```
    /// Guard page stack overflow'da page fault (#PF) üretir.
    pub fn with_priority_and_id(
        entry_point: fn() -> !,
        priority: Priority,
        name: &'static str,
        id: TaskId,
    ) -> Self {
        // 16KB stack + 4KB guard page = 20KB toplam ayır (Daha fazla task için küçültüldü)
        const GUARD_PAGE_SIZE: usize = 4096;
        const STACK_SIZE: usize = 16384; // 64KB -> 16KB
        let mut stack = KernelStack::new(GUARD_PAGE_SIZE + STACK_SIZE)
            .expect("Failed to allocate kernel stack");

        // Guard page: stack'in en altındaki 4KB
        // Bu alanı "zehirle" — debug pattern ile doldur (0xCC = INT3 opcode)
        // Gerçek koruma: page table'da PRESENT bit'i kaldırılarak yapılır
        for byte in stack[..GUARD_PAGE_SIZE].iter_mut() {
            *byte = 0xCC;
        }

        // Stack top: guard page'den sonraki 64KB'ın tepesi, 16-byte hizalı
        let stack_bottom = stack.as_mut_ptr() as u64 + GUARD_PAGE_SIZE as u64;
        let mut stack_top = stack_bottom + STACK_SIZE as u64;
        stack_top &= !0xFu64;

        Self {
            hot: TaskHotData {
                id,
                state: TaskState::Ready,
                priority,
                vruntime: 0,
                weight: weight_for_priority(priority),
                last_start: 0,
                affinity: 0xFFFFFFFF, // Tüm CPU'larda çalışabilir
                last_cpu: 0,
                kernel_stack_top: stack_top as u64,
                flags: TaskFlags::FPU_PRISTINE, // İlk xrstor atlanır
            },
            context: TaskContext::new(entry_point as u64, stack_top),
            cold: TaskColdData {
                name,
                mode: ExecutionMode::NativeRust,
                page_table: None,
                address_space: None,
                user_entry: None,
                user_stack_top: None,
                exit_code: None,
                wait_ticks: 0,
                exec_runtime: 0,
                ptrace_flags: 0,
                tracer_pid: None,
                seccomp_mode: 0, // 0 = Devre dışı
                stack,
                is_background: false,
                signals: Arc::new(SignalHandlers::new()),
                pcid: 0, // 0 = kernel PCID (NativeRust tasks)
                parent_pid: None,
                children: Vec::new(),
                rseq: RseqState::default(),
                win32: None,
            },
        }
    }

    /// CPU boşta olduğunda çalışan özel idle task'ı oluşturur.
    pub fn idle() -> Self {
        Self::idle_with_cpu(0)
    }

    /// Belirli CPU için idle task oluşturur.
    pub fn idle_with_cpu(cpu_id: u32) -> Self {
        fn idle_task() -> ! {
            loop {
                x86_64::instructions::hlt();
            }
        }

        let mut task = Self::with_priority(idle_task, Priority::Idle, "idle");
        task.hot.affinity = 1 << cpu_id; // Sadece belirli CPU'da
        task.hot.last_cpu = cpu_id;
        // Idle task FPU kullanmaz — context switch'te xsaveopt/xrstor atlanır
        task.hot.flags = TaskFlags::NO_FPU.insert(TaskFlags::KERNEL_THREAD);
        task
    }
}

// Cold data için de yardımcı metodlar
impl Task {
    // Cold data erişici/değiştirici metodlar
    pub fn name(&self) -> &'static str {
        self.cold.name
    }
    pub fn mode(&self) -> ExecutionMode {
        self.cold.mode
    }

    // Diğer cold field'lara doğrudan `task.cold.xxx` ile erişilir.
}
