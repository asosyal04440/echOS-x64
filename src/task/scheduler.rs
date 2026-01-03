//! # echOS Görev Zamanlayıcı (Task Scheduler)
//! 
//! Bu modül, işletim sisteminin preemptive multitasking desteğini sağlar.
//! Priority-Based Aging algoritması kullanarak task'ları adil bir şekilde zamanlar.

#![allow(unused)]
#![allow(static_mut_refs)]

use super::task::{Task, TaskId, TaskState, TaskContext, Priority, ExecutionMode};
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};
use core::arch::global_asm;
use x86_64::registers::control::{Cr3, Cr3Flags};
use spin::Mutex;
use lazy_static::lazy_static;

// ============================================================================
// SCHEDULER YAPISI
// ============================================================================

/// Ana zamanlayıcı yapısı.
/// Çalışmaya hazır tüm task'ları bir kuyrukta tutar.
pub struct Scheduler {
    /// Task kuyruğu - VecDeque sayesinde hem baştan hem sondan erişim O(1).
    local_queue: VecDeque<Task>,
}

impl Scheduler {
    /// Yeni boş bir Scheduler oluşturur.
    pub fn new() -> Self {
        Self {
            local_queue: VecDeque::new(),
        }
    }

    /// Yeni bir task'ı kuyruğun SONUNA ekler.
    /// Yeni spawn edilen task'lar buraya gelir.
    pub fn spawn(&mut self, task: Task) {
        self.local_queue.push_back(task);
    }

    /// Yield eden task'ı kuyruğun BAŞINA ekler.
    /// Round-robin rotasyonu için kullanılır.
    pub fn requeue(&mut self, task: Task) {
        self.local_queue.push_front(task);
    }

    /// Çalıştırılacak bir sonraki task'ı seçer.
    /// 
    /// # Algoritma: Priority-Based Scheduling with Aging
    /// 
    /// 1. Tüm bekleyen task'ların `wait_ticks` sayacı artırılır
    /// 2. Etkili öncelik hesaplanır: `effective = base_priority - (wait_ticks / 50)`
    /// 3. En düşük etkili önceliğe sahip task seçilir (düşük = yüksek öncelik)
    /// 4. Aging sayesinde düşük öncelikli task'lar zamanla terfi eder
    /// 
    /// Bu algoritma hem yüksek öncelikli task'ların hızlı çalışmasını,
    /// hem de düşük öncelikli task'ların aç kalmamasını garanti eder.
    pub fn pick_next(&mut self) -> Option<Task> {
        // Tüm bekleyen task'ların bekleme süresini artır
        for task in self.local_queue.iter_mut() {
            task.wait_ticks = task.wait_ticks.saturating_add(1);
        }
        
        // Her 50 tick'te öncelik 1 derece yükselir
        const AGING_FACTOR: u32 = 50;
        
        // Etkili öncelik hesaplama: düşük değer = yüksek öncelik
        let effective_priority = |task: &Task| -> i32 {
            let base = task.priority as i32;
            let boost = (task.wait_ticks / AGING_FACTOR) as i32;
            base - boost
        };
        
        // En yüksek öncelikli task'ı bul
        let mut best_idx: Option<usize> = None;
        let mut best_eff_priority = i32::MAX;
        
        for (idx, task) in self.local_queue.iter().enumerate() {
            let eff = effective_priority(task);
            if eff < best_eff_priority {
                best_eff_priority = eff;
                best_idx = Some(idx);
            }
        }
        
        // Bulunan task'ı kuyruktan çıkar ve aging sayacını sıfırla
        if let Some(idx) = best_idx {
            let mut task = self.local_queue.remove(idx)?;
            task.wait_ticks = 0;
            return Some(task);
        }
        
        None
    }
}

// ============================================================================
// GLOBAL DURUM
// ============================================================================

lazy_static! {
    /// Global Scheduler instance - Mutex ile korunur
    static ref SCHEDULER: Mutex<Scheduler> = Mutex::new(Scheduler::new());
}

/// Uyuyan task'ların listesi (wake_tick ile birlikte)
static mut SLEEPING_TASKS: Vec<Task> = Vec::new();

/// Idle Task - CPU boşta olduğunda çalışır (HLT instruction)
static mut IDLE_TASK: Option<Task> = None;

/// Şu anda CPU'da çalışan task
static mut CURRENT_TASK: Option<Task> = None;

/// Sistem başlangıcından beri geçen tick sayısı
static TICK_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Bir task'ın kesintisiz çalışabileceği maksimum tick sayısı
const TIME_SLICE: usize = 10;

// ============================================================================
// PUBLIC API
// ============================================================================

/// Scheduler'ı başlatır.
/// Idle task oluşturur ve rastgele sayı üretecini initialize eder.
pub fn init() {
    unsafe {
        IDLE_TASK = Some(Task::idle());
        CURRENT_TASK = None;
    }
    
    crate::serial_println!("Scheduler initialized (Priority-Based with Aging)");
    crate::random::init(get_ticks() as u32 + 0xDEADBEEF);
}

/// Normal öncelikle yeni bir task oluşturur ve kuyruğa ekler.
/// 
/// # Parametreler
/// - `entry_point`: Task'ın başlangıç fonksiyonu (diverging: -> !)
/// 
/// # Dönüş
/// - Oluşturulan task'ın benzersiz ID'si
pub fn spawn(entry_point: fn() -> !) -> TaskId {
    spawn_with_priority(entry_point, Priority::Normal, "unnamed")
}

/// Belirtilen öncelikle yeni bir task oluşturur.
/// 
/// # Parametreler
/// - `entry_point`: Task'ın başlangıç fonksiyonu
/// - `priority`: Task önceliği (High, Normal, Low, Idle)
/// - `name`: Debug için task adı
pub fn spawn_with_priority(entry_point: fn() -> !, priority: Priority, name: &'static str) -> TaskId {
    let task = Task::with_priority(entry_point, priority, name);
    let id = task.id;
    
    x86_64::instructions::interrupts::without_interrupts(|| {
        SCHEDULER.lock().spawn(task);
    });
    
    id
}

/// Sistem tick sayısını döndürür.
pub fn get_ticks() -> usize {
    TICK_COUNT.load(Ordering::Relaxed)
}

/// Timer interrupt'tan çağrılır. Her tick'te:
/// 1. Tick sayacını artırır
/// 2. Uyuyan task'ları kontrol eder
/// 3. Time slice dolmuşsa schedule() çağırır
pub fn tick() {
    let ticks = TICK_COUNT.fetch_add(1, Ordering::Relaxed);
    wake_sleeping_tasks(ticks + 1);
    
    if ticks % TIME_SLICE == 0 {
        schedule();
    }
}

/// Uyuyan task'ları kontrol eder, zamanı gelenleri uyandırır.
fn wake_sleeping_tasks(current_tick: usize) {
    unsafe {
        let mut i = 0;
        while i < SLEEPING_TASKS.len() {
            if let TaskState::Sleeping { wake_tick } = SLEEPING_TASKS[i].state {
                if current_tick >= wake_tick {
                    let mut task = SLEEPING_TASKS.remove(i);
                    task.state = TaskState::Ready;
                    SCHEDULER.lock().spawn(task);
                } else {
                    i += 1;
                }
            } else {
                i += 1;
            }
        }
    }
}

/// Mevcut task'ı belirtilen tick sayısı kadar uyutur.
/// 
/// # Parametreler
/// - `ticks`: Uyunacak tick sayısı (1 tick ≈ 10ms)
pub fn sleep(ticks: usize) {
    let wake_tick = TICK_COUNT.load(Ordering::Relaxed) + ticks;
    
    x86_64::instructions::interrupts::without_interrupts(|| {
        unsafe {
            if let Some(mut current) = CURRENT_TASK.take() {
                current.state = TaskState::Sleeping { wake_tick };
                SLEEPING_TASKS.push(current);
            } else {
                crate::serial_println!("WARNING: Idle task attempted to sleep!");
            }
        }
    });
    
    schedule();
}

/// Mevcut task'ı sonlandırır ve bir daha çalışmaz.
/// Bu fonksiyon geri dönmez (diverging).
pub fn exit() -> ! {
    x86_64::instructions::interrupts::without_interrupts(|| {
        unsafe {
            if let Some(mut current) = CURRENT_TASK.take() {
                current.state = TaskState::Terminated;
                crate::serial_println!("Task {} '{}' terminated", current.id, current.name);
            } else {
                crate::serial_println!("ERROR: Idle task attempted to exit!");
            }
        }
    });
    
    schedule();
    
    // Bu noktaya asla ulaşılmamalı
    loop {
        x86_64::instructions::hlt();
    }
}

// ============================================================================
// CONTEXT SWITCH
// ============================================================================

/// Ana zamanlama fonksiyonu. Task değişimini gerçekleştirir.
/// 
/// # İşleyiş
/// 1. Scheduler'dan bir sonraki task'ı al (priority + aging ile)
/// 2. Mevcut task'ın context'ini kaydet
/// 3. Yeni task'ın context'ini yükle
/// 4. Gerekirse sayfa tablosu değiştir (CR3)
/// 5. Assembly switch_context ile CPU register'larını değiştir
pub fn schedule() {
    x86_64::instructions::interrupts::without_interrupts(|| {
        unsafe {
            // Sonraki task'ı seç
            let next_task_opt = SCHEDULER.lock().pick_next();
            
            // İş yoksa ve zaten idle'daysan, geri dön
            if next_task_opt.is_none() {
                if CURRENT_TASK.is_none() {
                    return;
                }
            }

            // Context pointer'ları hazırla
            let old_context_ptr: *mut TaskContext;
            let new_context_ptr: *const TaskContext;
            
            // Eski task'ı işle
            if let Some(mut current) = CURRENT_TASK.take() {
                if current.state == TaskState::Running {
                    current.state = TaskState::Ready;
                }
                
                if current.state == TaskState::Ready {
                    // Task'ı tekrar kuyruğa ekle
                    let mut scheduler = SCHEDULER.lock();
                    scheduler.spawn(current);
                    let task_ref = scheduler.local_queue.back_mut().unwrap();
                    old_context_ptr = &mut task_ref.context as *mut TaskContext;
                    drop(scheduler);
                } else {
                    // Terminated/Sleeping task için dummy context
                    static mut DUMMY_CONTEXT: TaskContext = TaskContext { 
                        r15:0, r14:0, r13:0, r12:0, rbx:0, rbp:0, rip:0, rsp:0, rflags:0, padding: 0, 
                        fx_state: crate::task::task::FxSaveArea { data: [0; 512] } 
                    };
                    old_context_ptr = &mut DUMMY_CONTEXT;
                }
            } else {
                // Idle'dan çıkıyoruz
                let idle_task = IDLE_TASK.as_mut().unwrap();
                old_context_ptr = &mut idle_task.context;
            }
            
            // Yeni task'ı işle
            if let Some(mut next_task) = next_task_opt {
                next_task.state = TaskState::Running;
                
                // Sayfa tablosu değişimi (Ring3 task'lar için)
                let current_frame = Cr3::read().0;
                let target_frame = match next_task.mode {
                    ExecutionMode::LegacyRing3 => next_task.page_table.unwrap(),
                    ExecutionMode::NativeRust => crate::memory::KERNEL_PML4_FRAME.unwrap()
                };
                if target_frame != current_frame {
                    Cr3::write(target_frame, Cr3Flags::empty());
                }

                CURRENT_TASK = Some(next_task);
                new_context_ptr = &CURRENT_TASK.as_ref().unwrap().context;
            } else {
                // Idle'a geçiyoruz
                let idle_task = IDLE_TASK.as_ref().unwrap();
                new_context_ptr = &idle_task.context;
                CURRENT_TASK = None;
            }
            
            // Assembly ile context switch yap
            switch_context(old_context_ptr, new_context_ptr);
        }
    });
}

// ============================================================================
// ASSEMBLY: CONTEXT SWITCH
// ============================================================================

/// x86_64 için context switch assembly kodu.
/// 
/// RDI = eski context pointer (kayıt yapılacak)
/// RSI = yeni context pointer (yüklenecek)
/// 
/// Callee-saved register'ları ve SSE/FPU durumunu kaydeder/yükler.
global_asm!(r#"
.global switch_context
switch_context:
    // Eski context'i kaydet (RDI'ya)
    mov [rdi + 0], r15
    mov [rdi + 8], r14
    mov [rdi + 16], r13
    mov [rdi + 24], r12
    mov [rdi + 32], rbx
    mov [rdi + 40], rbp
    mov [rdi + 48], rsp
    pushfq
    pop rax
    mov [rdi + 56], rax      // RFLAGS
    mov rax, [rsp]
    mov [rdi + 64], rax      // Return address (RIP)
    fxsave64 [rdi + 80]      // SSE/FPU state
    
    // Yeni context'i yükle (RSI'dan)
    fxrstor64 [rsi + 80]     // SSE/FPU state
    mov r15, [rsi + 0]
    mov r14, [rsi + 8]
    mov r13, [rsi + 16]
    mov r12, [rsi + 24]
    mov rbx, [rsi + 32]
    mov rbp, [rsi + 40]
    mov rax, [rsi + 56]
    push rax
    popfq                     // RFLAGS
    mov rsp, [rsi + 48]
    mov rax, [rsi + 64]
    jmp rax                   // Yeni task'a atla
"#);

unsafe extern "sysv64" {
    /// Assembly'de tanımlanan context switch fonksiyonu
    fn switch_context(old: *mut TaskContext, new: *const TaskContext);
}
