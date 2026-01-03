//! # echOS Task Yapısı
//! 
//! Bu modül, işletim sistemindeki task (görev) yapısını tanımlar.
//! Her task kendi stack'ine, context'ine ve önceliğine sahiptir.

use alloc::vec::Vec;
use alloc::vec;
use core::sync::atomic::{AtomicUsize, Ordering};
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
}

// ============================================================================
// FPU/SSE STATE
// ============================================================================

/// x86_64 FXSAVE/FXRSTOR için 512 byte'lık alan.
/// SSE, AVX ve FPU register'larını saklar.
#[repr(C, align(16))]
#[derive(Debug, Clone)]
pub struct FxSaveArea {
    pub data: [u8; 512],
}

// ============================================================================
// TASK CONTEXT
// ============================================================================

/// Task'ın CPU durumu (register'lar).
/// Context switch sırasında kaydedilir ve geri yüklenir.
#[repr(C)]
#[derive(Debug, Clone)]
pub struct TaskContext {
    // Callee-saved register'lar (ABI gereği korunmalı)
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub rbx: u64,
    pub rbp: u64,      // Base pointer
    pub rsp: u64,      // Stack pointer
    pub rflags: u64,   // CPU flags
    pub rip: u64,      // Instruction pointer (dönüş adresi)
    pub padding: u64,  // 16-byte alignment için
    pub fx_state: FxSaveArea,  // SSE/FPU durumu
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
            padding: 0,
            fx_state: FxSaveArea { data: [0; 512] },
        }
    }
}

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

// ============================================================================
// TASK
// ============================================================================

/// Bir işletim sistemi task'ı (thread/process).
pub struct Task {
    /// Benzersiz task kimliği
    pub id: TaskId,
    /// Mevcut durum (Ready, Running, vb.)
    pub state: TaskState,
    /// Öncelik seviyesi
    pub priority: Priority,
    /// CPU register durumu
    pub context: TaskContext,
    /// Debug için okunabilir isim
    pub name: &'static str,
    /// Çalışma modu (Ring 0 veya Ring 3)
    pub mode: ExecutionMode,
    /// Sayfa tablosu (Ring 3 için gerekli)
    pub page_table: Option<PhysFrame>,
    /// Bekleme süresi (aging için)
    pub wait_ticks: u32,
    /// Task'ın özel stack alanı
    stack: Vec<u8>,
}

impl Task {
    /// Normal öncelikle yeni task oluşturur.
    pub fn new(entry_point: fn() -> !) -> Self {
        Self::with_priority(entry_point, Priority::Normal, "unnamed")
    }
    
    /// Belirtilen öncelikle yeni task oluşturur.
    /// 
    /// # Parametreler
    /// - `entry_point`: Task'ın başlangıç fonksiyonu (sonsuza kadar çalışır)
    /// - `priority`: Öncelik seviyesi
    /// - `name`: Debug için isim
    pub fn with_priority(entry_point: fn() -> !, priority: Priority, name: &'static str) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        
        // 64KB stack ayır
        let mut stack = vec![0u8; 65536];
        let stack_top = stack.as_mut_ptr() as u64 + 65536;
        
        // 16-byte alignment (ABI gereksinimi)
        let stack_top = stack_top & !0xF;
        
        Self {
            id,
            state: TaskState::Ready,
            priority,
            context: TaskContext::new(entry_point as u64, stack_top),
            name,
            mode: ExecutionMode::NativeRust,
            page_table: None,
            wait_ticks: 0,
            stack,
        }
    }
    
    /// CPU boşta olduğunda çalışan özel idle task'ı oluşturur.
    /// Bu task sadece HLT instruction'ı çalıştırır.
    pub fn idle() -> Self {
        fn idle_task() -> ! {
            loop {
                x86_64::instructions::hlt();
            }
        }
        
        Self::with_priority(idle_task as fn() -> !, Priority::Idle, "idle")
    }
}
