//! # echOS Interrupt Yönetimi
//!
//! Bu modül, x86_64 exception ve hardware interrupt'larını yönetir.
//! IDT (Interrupt Descriptor Table) ve PIC (Programmable Interrupt Controller) yapılandırması.

pub mod idt;
pub mod pic;
pub mod softirq;
pub mod irq_chip;

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::arch::x86_64::_rdtsc;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, AtomicUsize, Ordering};
use spin::Mutex;
use x86_64::structures::idt::InterruptDescriptorTable;

// ============================================================================
// IDT YAPISI
// ============================================================================

static USE_IOAPIC: AtomicBool = AtomicBool::new(false);
static INIT_STATE: AtomicU8 = AtomicU8::new(0);
pub const IPI_TLB_VECTOR: u8 = 0xF1;
const SPURIOUS_VECTOR: u8 = 0xFF;
const MSI_VECTOR_START: u8 = 48;
const MSI_VECTOR_END: u8 = 0xEF;
const IRQ_LOG_CAP: usize = 1024;
const DEFAULT_STORM_LIMIT: u64 = 500;
const DEFAULT_IRQ_WATCHDOG_INTERVAL: u64 = 100;
const DEFAULT_IRQ_LATENCY_WARN_CYCLES: u64 = 200_000;
const IRQF_THREADED: u64 = 1 << 0;
const IRQF_FAST_EOI: u64 = 1 << 1;
const IRQF_PERCPU: u64 = 1 << 2;
const IRQF_EDGE: u64 = 1 << 3;
const IRQF_LEVEL: u64 = 1 << 4;
/// Linux IRQF_SHARED — aynı vektörde birden fazla handler
pub const IRQF_SHARED: u64 = 1 << 5;
static IDT_TABLES: Mutex<Vec<usize>> = Mutex::new(Vec::new());
static IRQ_HANDLERS: Mutex<[Option<IrqHandler>; 256]> = Mutex::new([None; 256]);
/// Shared IRQ handler chain — aynı vektörde birden fazla handler
static IRQ_SHARED_CHAINS: Mutex<[Option<Vec<IrqHandler>>; 256]> = Mutex::new({
    const NONE: Option<Vec<IrqHandler>> = None;
    [NONE; 256]
});
static IRQ_THREAD_HANDLERS: Mutex<[Option<IrqHandler>; 256]> = Mutex::new([None; 256]);
static IRQ_FLOWS: Mutex<[IrqFlow; 256]> = Mutex::new([IrqFlow::Level; 256]);
static PCI_IRQ_POLICY: AtomicU8 = AtomicU8::new(PciInterruptPolicy::MsiPreferred as u8);
static IRQ_STORM_LIMIT: AtomicU64 = AtomicU64::new(DEFAULT_STORM_LIMIT);
static IRQ_WATCHDOG_INTERVAL: AtomicU64 = AtomicU64::new(DEFAULT_IRQ_WATCHDOG_INTERVAL);
static IRQ_LATENCY_WARN_CYCLES: AtomicU64 = AtomicU64::new(DEFAULT_IRQ_LATENCY_WARN_CYCLES);
static IRQ_WORKER_STARTED: AtomicBool = AtomicBool::new(false);
static IRQ_DYNAMIC_FLOW_ENABLED: AtomicBool = AtomicBool::new(true);

// ============================================================================
// IRQ DISABLE DEPTH TRACKING (Linux local_irq_save/restore)
// ============================================================================

/// Per-CPU IRQ disable depth (nested disable tracking)
static IRQ_DISABLE_DEPTH: AtomicU64 = AtomicU64::new(0);

/// Interrupt'ları devre dışı bırak ve önceki durumu kaydet (nested)
/// Linux `local_irq_save()` karşılığı
pub fn local_irq_save() -> u64 {
    let flags = x86_64::registers::rflags::read().bits();
    x86_64::instructions::interrupts::disable();
    IRQ_DISABLE_DEPTH.fetch_add(1, Ordering::SeqCst);
    flags
}

/// Interrupt durumunu geri yükle (nested)
/// Linux `local_irq_restore()` karşılığı
pub fn local_irq_restore(flags: u64) {
    let depth = IRQ_DISABLE_DEPTH.fetch_sub(1, Ordering::SeqCst);
    if depth <= 1 {
        // Depth 0'a düştü — interrupt'ları aç (eğer önceki durumda açıksa)
        if (flags & (1 << 9)) != 0 {
            // IF flag set idi
            x86_64::instructions::interrupts::enable();
        }
    }
}

pub fn kick_irq_worker() {
    start_irq_worker();
}

/// Mevcut IRQ disable derinliği
pub fn irq_disable_depth() -> u64 {
    IRQ_DISABLE_DEPTH.load(Ordering::SeqCst)
}

/// Interrupt'lar aktif mi?
pub fn irqs_disabled() -> bool {
    !x86_64::registers::rflags::read().contains(x86_64::registers::rflags::RFlags::INTERRUPT_FLAG)
}
static IRQ_FLOW_CHANGE_COOLDOWN: AtomicU64 = AtomicU64::new(200);

pub type IrqHandler = fn(u8);

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IrqFlow {
    Level = 0,
    Edge = 1,
    FastEoi = 2,
    PerCpu = 3,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PciInterruptPolicy {
    LegacyOnly = 0,
    MsiPreferred = 1,
    MsiXPreferred = 2,
}

lazy_static::lazy_static! {
    static ref IRQ_COUNTS: Vec<AtomicU64> = (0..256).map(|_| AtomicU64::new(0)).collect();
    static ref IRQ_LAST_TICK: Vec<AtomicU64> = (0..256).map(|_| AtomicU64::new(0)).collect();
    static ref IRQ_BURST: Vec<AtomicU64> = (0..256).map(|_| AtomicU64::new(0)).collect();
    static ref IRQ_STORMS: Vec<AtomicU64> = (0..256).map(|_| AtomicU64::new(0)).collect();
    static ref IRQ_TOTAL_LATENCY: Vec<AtomicU64> = (0..256).map(|_| AtomicU64::new(0)).collect();
    static ref IRQ_MAX_LATENCY: Vec<AtomicU64> = (0..256).map(|_| AtomicU64::new(0)).collect();
    static ref IRQ_LATENCY_SAMPLES: Vec<AtomicU64> = (0..256).map(|_| AtomicU64::new(0)).collect();
    static ref IRQ_STORMS_REPORTED: Vec<AtomicU64> = (0..256).map(|_| AtomicU64::new(0)).collect();
    static ref IRQ_LATENCY_REPORTED: Vec<AtomicU64> = (0..256).map(|_| AtomicU64::new(0)).collect();
    static ref IRQ_LOG: Mutex<Vec<IrqEvent>> = Mutex::new(Vec::new());
    static ref IRQ_AFFINITY_MASKS: Vec<AtomicU64> = (0..256).map(|_| AtomicU64::new(0)).collect();
    static ref IRQ_AFFINITY_CURSOR: Vec<AtomicU64> = (0..256).map(|_| AtomicU64::new(0)).collect();
    static ref IRQ_REBALANCE_LAST: Vec<AtomicU64> = (0..256).map(|_| AtomicU64::new(0)).collect();
    static ref IRQ_THREAD_QUEUE: Mutex<VecDeque<u8>> = Mutex::new(VecDeque::new());
    static ref IRQ_FLOW_PREV: Vec<AtomicU8> = (0..256).map(|_| AtomicU8::new(IrqFlow::Level as u8)).collect();
    static ref IRQ_FLOW_LAST_CHANGE: Vec<AtomicU64> = (0..256).map(|_| AtomicU64::new(0)).collect();
    static ref IRQ_FLOW_LOCK_UNTIL: Vec<AtomicU64> = (0..256).map(|_| AtomicU64::new(0)).collect();
}
static WATCHDOG_LAST_TICK: AtomicU64 = AtomicU64::new(0);
static IRQ_LOG_INDEX: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Copy)]
struct IrqEvent {
    vector: u8,
    cpu: u32,
    tsc: u64,
    latency: u64,
}

pub fn init_idt_for_cpu(cpu_id: u32) -> &'static InterruptDescriptorTable {
    crate::serial_println!("init_idt_for_cpu: cpu_id={}", cpu_id);
    let idx = cpu_id as usize;
    crate::serial_println!("init_idt_for_cpu: acquiring lock...");
    let mut list = IDT_TABLES.lock();
    crate::serial_println!("init_idt_for_cpu: lock acquired. list.len()={}", list.len());
    if list.len() <= idx {
        crate::serial_println!("init_idt_for_cpu: resizing list to {}", idx + 1);
        list.resize(idx + 1, 0);
    }
    if list[idx] == 0 {
        crate::serial_println!("init_idt_for_cpu: building new IDT (Box::new)");
        let idt = Box::new(build_idt());
        let ptr = Box::into_raw(idt) as usize;
        crate::serial_println!("init_idt_for_cpu: IDT allocated at {:#x}", ptr);
        list[idx] = ptr;
    }
    crate::serial_println!("init_idt_for_cpu: returning IDT");
    unsafe { &*(list[idx] as *const InterruptDescriptorTable) }
}

/// Interrupt sistemini başlatır.
/// IDT'yi yükler ve PIC'i yapılandırır.
pub fn init() {
    init_global_once();
    init_per_cpu();
    start_irq_worker();
    // Softirq + IRQ chip subsystem başlat
    softirq::init();
    irq_chip::init();
}

pub fn init_per_cpu() {
    let cpu_id = crate::cpu::smp::current_cpu_id();
    let idt = init_idt_for_cpu(cpu_id);
    idt.load();
}

pub fn enable_ioapic() -> bool {
    if !crate::acpi::init() {
        crate::serial_println!("ACPI init failed");
        return false;
    }
    let apic_info = crate::acpi::get_apic_info();
    if apic_info.io_apics.is_empty() {
        crate::serial_println!("IOAPIC not found");
        return false;
    }
    if crate::apic::lapic::init().is_err() {
        crate::serial_println!("LAPIC init failed");
        return false;
    }
    let bsp = crate::cpu::CPU_INFO.lock().bsp_apic_id as u8;
    if !crate::apic::ioapic::init(&apic_info, bsp) {
        crate::serial_println!("IOAPIC init failed");
        return false;
    }
    USE_IOAPIC.store(true, Ordering::SeqCst);
    unsafe {
        crate::drivers::apic::disable_pic();
    }
    crate::serial_println!("IOAPIC enabled");
    true
}

pub fn ioapic_enabled() -> bool {
    USE_IOAPIC.load(Ordering::SeqCst)
}

pub fn register_irq_handler(vector: u8, handler: IrqHandler) {
    let mut handlers = IRQ_HANDLERS.lock();
    handlers[vector as usize] = Some(handler);
}

pub fn request_irq(vector: u8, handler: IrqHandler, flags: u64) -> bool {
    let flow = flow_from_flags(flags);
    set_irq_type(vector, flow);

    if (flags & IRQF_SHARED) != 0 {
        // Shared IRQ — handler chain'e ekle
        let mut chains = IRQ_SHARED_CHAINS.lock();
        if chains[vector as usize].is_none() {
            chains[vector as usize] = Some(Vec::new());
        }
        chains[vector as usize].as_mut().unwrap().push(handler);
        // İlk handler'ı primary olarak kaydet
        let mut primary = IRQ_HANDLERS.lock();
        if primary[vector as usize].is_none() {
            primary[vector as usize] = Some(handler);
        }
    } else if (flags & IRQF_THREADED) != 0 {
        let mut handlers = IRQ_HANDLERS.lock();
        handlers[vector as usize] = None;
        let mut thread_handlers = IRQ_THREAD_HANDLERS.lock();
        thread_handlers[vector as usize] = Some(handler);
        start_irq_worker();
    } else {
        register_irq_handler(vector, handler);
        let mut thread_handlers = IRQ_THREAD_HANDLERS.lock();
        thread_handlers[vector as usize] = None;
    }
    true
}

pub fn request_threaded_irq(
    vector: u8,
    top: Option<IrqHandler>,
    thread: IrqHandler,
    flags: u64,
) -> bool {
    let flow = flow_from_flags(flags);
    set_irq_type(vector, flow);
    let mut handlers = IRQ_HANDLERS.lock();
    handlers[vector as usize] = top;
    let mut thread_handlers = IRQ_THREAD_HANDLERS.lock();
    thread_handlers[vector as usize] = Some(thread);
    start_irq_worker();
    true
}

pub fn free_irq(vector: u8) {
    let mut handlers = IRQ_HANDLERS.lock();
    handlers[vector as usize] = None;
    let mut thread_handlers = IRQ_THREAD_HANDLERS.lock();
    thread_handlers[vector as usize] = None;
    let mut flows = IRQ_FLOWS.lock();
    flows[vector as usize] = IrqFlow::Level;
    IRQ_AFFINITY_MASKS[vector as usize].store(0, Ordering::Relaxed);
}

pub fn set_irq_type(vector: u8, flow: IrqFlow) {
    let ticks = get_ticks();
    IRQ_FLOW_PREV[vector as usize].store(flow as u8, Ordering::Relaxed);
    IRQ_FLOW_LAST_CHANGE[vector as usize].store(ticks, Ordering::Relaxed);
    IRQ_FLOW_LOCK_UNTIL[vector as usize].store(ticks, Ordering::Relaxed);
    apply_irq_flow(vector, flow, ticks, false);
}

pub fn set_irq_affinity_mask(vector: u8, mask: u64) {
    IRQ_AFFINITY_MASKS[vector as usize].store(mask, Ordering::Relaxed);
    if vector >= 32 && vector <= 47 && ioapic_enabled() {
        let apic_id = select_apic_id_for_vector(vector);
        crate::apic::ioapic::set_irq_affinity(vector - 32, apic_id as u8);
    }
}

pub fn set_irq_dynamic_flow(enabled: bool) {
    IRQ_DYNAMIC_FLOW_ENABLED.store(enabled, Ordering::SeqCst);
}

pub fn irq_dynamic_flow_enabled() -> bool {
    IRQ_DYNAMIC_FLOW_ENABLED.load(Ordering::SeqCst)
}

pub fn set_irq_flow_cooldown(ticks: u64) {
    IRQ_FLOW_CHANGE_COOLDOWN.store(ticks, Ordering::SeqCst);
}

pub fn irq_affinity_mask(vector: u8) -> u64 {
    IRQ_AFFINITY_MASKS[vector as usize].load(Ordering::Relaxed)
}

pub fn allocate_msi_vector(handler: IrqHandler) -> Option<u8> {
    let vector = VECTOR_ALLOCATOR.lock().alloc_vector()?;
    register_irq_handler(vector, handler);
    Some(vector)
}

pub fn allocate_msi_vectors(count: usize, handler: IrqHandler) -> Option<Vec<u8>> {
    if count == 0 {
        return None;
    }
    let vectors = VECTOR_ALLOCATOR.lock().alloc_range(count)?;
    for &vector in &vectors {
        register_irq_handler(vector, handler);
    }
    Some(vectors)
}

pub fn release_msi_vector(vector: u8) {
    VECTOR_ALLOCATOR.lock().free_vector(vector);
    let mut handlers = IRQ_HANDLERS.lock();
    handlers[vector as usize] = None;
}

pub fn release_msi_vectors(vectors: &[u8]) {
    let mut allocator = VECTOR_ALLOCATOR.lock();
    let mut handlers = IRQ_HANDLERS.lock();
    for &vector in vectors {
        allocator.free_vector(vector);
        handlers[vector as usize] = None;
    }
}

pub fn set_irq_affinity(irq: u8, apic_id: u8) {
    crate::apic::ioapic::set_irq_affinity(irq, apic_id);
    let vector = 32u8.wrapping_add(irq);
    let mask = apic_id_to_mask(apic_id as u32);
    if mask != 0 {
        IRQ_AFFINITY_MASKS[vector as usize].store(mask, Ordering::Relaxed);
    }
}

pub fn set_pci_interrupt_policy(policy: PciInterruptPolicy) {
    PCI_IRQ_POLICY.store(policy as u8, Ordering::SeqCst);
}

pub fn pci_interrupt_policy() -> PciInterruptPolicy {
    match PCI_IRQ_POLICY.load(Ordering::SeqCst) {
        0 => PciInterruptPolicy::LegacyOnly,
        2 => PciInterruptPolicy::MsiXPreferred,
        _ => PciInterruptPolicy::MsiPreferred,
    }
}

pub fn set_irq_storm_limit(limit: u64) {
    IRQ_STORM_LIMIT.store(limit, Ordering::Relaxed);
}

pub fn set_irq_watchdog_interval(interval: u64) {
    IRQ_WATCHDOG_INTERVAL.store(interval, Ordering::Relaxed);
}

pub fn set_irq_latency_warn_cycles(cycles: u64) {
    IRQ_LATENCY_WARN_CYCLES.store(cycles, Ordering::Relaxed);
}

pub fn irq_storm_limit() -> u64 {
    IRQ_STORM_LIMIT.load(Ordering::Relaxed)
}

pub fn irq_watchdog_interval() -> u64 {
    IRQ_WATCHDOG_INTERVAL.load(Ordering::Relaxed)
}

pub fn irq_latency_warn_cycles() -> u64 {
    IRQ_LATENCY_WARN_CYCLES.load(Ordering::Relaxed)
}

pub fn irq_latency_stats(vector: u8) -> (u64, u64, u64) {
    let samples = IRQ_LATENCY_SAMPLES[vector as usize].load(Ordering::Relaxed);
    let total = IRQ_TOTAL_LATENCY[vector as usize].load(Ordering::Relaxed);
    let max = IRQ_MAX_LATENCY[vector as usize].load(Ordering::Relaxed);
    let avg = if samples == 0 { 0 } else { total / samples };
    (max, avg, samples)
}

pub fn irq_count(vector: u8) -> u64 {
    IRQ_COUNTS[vector as usize].load(Ordering::Relaxed)
}

pub fn clear_irq_metrics() {
    for v in 0u8..=255 {
        IRQ_COUNTS[v as usize].store(0, Ordering::Relaxed);
        IRQ_LAST_TICK[v as usize].store(0, Ordering::Relaxed);
        IRQ_BURST[v as usize].store(0, Ordering::Relaxed);
        IRQ_STORMS[v as usize].store(0, Ordering::Relaxed);
        IRQ_TOTAL_LATENCY[v as usize].store(0, Ordering::Relaxed);
        IRQ_MAX_LATENCY[v as usize].store(0, Ordering::Relaxed);
        IRQ_LATENCY_SAMPLES[v as usize].store(0, Ordering::Relaxed);
        IRQ_STORMS_REPORTED[v as usize].store(0, Ordering::Relaxed);
        IRQ_LATENCY_REPORTED[v as usize].store(0, Ordering::Relaxed);
    }
    WATCHDOG_LAST_TICK.store(0, Ordering::Relaxed);
    IRQ_LOG_INDEX.store(0, Ordering::Relaxed);
    IRQ_LOG.lock().clear();
}

pub fn simulate_irq(vector: u8, iterations: u64) {
    for _ in 0..iterations {
        record_irq(vector);
        dispatch_irq(vector);
    }
}

pub fn calibrate_irq_latency_warn(vector: u8, iterations: u64, multiplier: u64) -> u64 {
    if iterations == 0 {
        return irq_latency_warn_cycles();
    }
    let factor = multiplier.max(1);
    simulate_irq(vector, iterations);
    let (max, _, _) = irq_latency_stats(vector);
    if max == 0 {
        return irq_latency_warn_cycles();
    }
    let warn = max.saturating_mul(factor);
    set_irq_latency_warn_cycles(warn);
    warn
}

pub fn resolve_msi_target(vector: u8, apic_id: u32) -> u32 {
    let mask = irq_affinity_mask(vector);
    if mask == 0 {
        apic_id
    } else {
        select_apic_id_for_vector(vector)
    }
}

pub fn msi_target_apic_id(vector: u8) -> u32 {
    let info = crate::cpu::CPU_INFO.lock();
    resolve_msi_target(vector, info.bsp_apic_id)
}

// ============================================================================
// EXCEPTION HANDLERS (CPU Hataları)
// ============================================================================

use core::arch::asm;
use x86_64::instructions::tlb;
use x86_64::structures::idt::InterruptStackFrame;

#[derive(Debug)]
struct RegisterDump {
    rax: u64,
    rbx: u64,
    rcx: u64,
    rdx: u64,
    rsi: u64,
    rdi: u64,
    rbp: u64,
    r8: u64,
    r9: u64,
    r10: u64,
    r11: u64,
    r12: u64,
    r13: u64,
    r14: u64,
    r15: u64,
    rip: u64,
    rsp: u64,
    rflags: u64,
    cs: u64,
    ss: u64,
    cr2: u64,
    cr3: u64,
    cr4: u64,
}

fn capture_registers(stack_frame: &InterruptStackFrame) -> RegisterDump {
    let (rax, rbx, rcx, rdx, rsi, rdi, rbp, r8, r9, r10, r11, r12, r13, r14, r15): (
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
    );
    unsafe {
        asm!(
            "mov {0}, rax",
            "mov {1}, rbx",
            "mov {2}, rcx",
            "mov {3}, rdx",
            "mov {4}, rsi",
            "mov {5}, rdi",
            "mov {6}, rbp",
            "mov {7}, r8",
            "mov {8}, r9",
            "mov {9}, r10",
            "mov {10}, r11",
            "mov {11}, r12",
            "mov {12}, r13",
            "mov {13}, r14",
            "mov {14}, r15",
            out(reg) rax,
            out(reg) rbx,
            out(reg) rcx,
            out(reg) rdx,
            out(reg) rsi,
            out(reg) rdi,
            out(reg) rbp,
            out(reg) r8,
            out(reg) r9,
            out(reg) r10,
            out(reg) r11,
            out(reg) r12,
            out(reg) r13,
            out(reg) r14,
            out(reg) r15,
            options(nomem, nostack, preserves_flags),
        );
    }
    let cr2 = x86_64::registers::control::Cr2::read().as_u64();
    let (cr3, _) = x86_64::registers::control::Cr3::read();
    let cr4 = x86_64::registers::control::Cr4::read().bits();
    RegisterDump {
        rax,
        rbx,
        rcx,
        rdx,
        rsi,
        rdi,
        rbp,
        r8,
        r9,
        r10,
        r11,
        r12,
        r13,
        r14,
        r15,
        rip: stack_frame.instruction_pointer.as_u64(),
        rsp: stack_frame.stack_pointer.as_u64(),
        rflags: stack_frame.cpu_flags,
        cs: stack_frame.code_segment,
        ss: stack_frame.stack_segment,
        cr2,
        cr3: cr3.start_address().as_u64(),
        cr4,
    }
}

fn dump_registers(stack_frame: &InterruptStackFrame) {
    let regs = capture_registers(stack_frame);
    crate::serial_println!("CPU STATE: {:#?}", regs);
}

/// Sıfıra bölme hatası (Divide by Zero)
extern "x86-interrupt" fn divide_error_handler(stack_frame: InterruptStackFrame) {
    panic!("EXCEPTION: DIVIDE ERROR\n{:#?}", stack_frame);
}

/// Debug breakpoint (INT 3)
extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    crate::serial_println!("EXCEPTION: BREAKPOINT\n{:#?}", stack_frame);
}

use x86_64::structures::idt::PageFaultErrorCode;

fn classify_user_stack_fault(addr: u64) -> Option<&'static str> {
    let (stack_base, stack_top) = crate::memory::user_stack_bounds();
    let _stack_size =
        (crate::memory::USER_STACK_PAGES as u64).saturating_mul(crate::memory::PAGE_SIZE as u64);
    let guard_end = stack_base.saturating_add(crate::memory::PAGE_SIZE as u64);
    if addr < guard_end {
        Some("STACK_OVERFLOW")
    } else if addr >= stack_top && addr <= crate::memory::USER_SPACE_END {
        Some("STACK_UNDERFLOW")
    } else {
        None
    }
}

fn user_fault_exit(kind: &str, rip: u64, addr: Option<u64>, code: Option<u64>) {
    match (addr, code) {
        (Some(addr), Some(code)) => crate::serial_println!(
            "USER FAULT: {} RIP={:#x} ADDR={:#x} CODE={:#x}",
            kind,
            rip,
            addr,
            code
        ),
        (Some(addr), None) => {
            crate::serial_println!("USER FAULT: {} RIP={:#x} ADDR={:#x}", kind, rip, addr)
        }
        (None, Some(code)) => {
            crate::serial_println!("USER FAULT: {} RIP={:#x} CODE={:#x}", kind, rip, code)
        }
        (None, None) => crate::serial_println!("USER FAULT: {} RIP={:#x}", kind, rip),
    }
    crate::task::scheduler::exit(1);
}

/// Sayfa hatası (Page Fault)
/// User mode'da oluşursa task sonlandırılır, kernel mode'da panic.
extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    use x86_64::registers::control::Cr2;

    let cs = stack_frame.code_segment;
    if (cs & 3) == 3 {
        let rip = stack_frame.instruction_pointer.as_u64();
        let addr = Cr2::read().as_u64();
        if crate::memory::handle_user_page_fault(addr, error_code) {
            return;
        }
        let kind = classify_user_stack_fault(addr).unwrap_or("PAGE_FAULT");
        user_fault_exit(kind, rip, Some(addr), Some(error_code.bits() as u64));
    } else {
        crate::serial_println!("EXCEPTION: PAGE FAULT");
        crate::serial_println!("Accessed Address: {:?}", Cr2::read());
        crate::serial_println!("Error Code: {:?}", error_code);
        crate::serial_println!("{:#?}", stack_frame);
        dump_registers(&stack_frame);
        panic!("Page fault");
    }
}

/// Çift hata (Double Fault) - kurtarılamaz
extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame,
    _error_code: u64,
) -> ! {
    dump_registers(&stack_frame);
    panic!("EXCEPTION: DOUBLE FAULT\n{:#?}", stack_frame);
}

/// Genel koruma hatası (General Protection Fault)
extern "x86-interrupt" fn general_protection_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    let cs = stack_frame.code_segment;
    if (cs & 3) == 3 {
        let rip = stack_frame.instruction_pointer.as_u64();
        user_fault_exit("GENERAL_PROTECTION_FAULT", rip, None, Some(error_code));
    } else {
        dump_registers(&stack_frame);
        panic!(
            "EXCEPTION: GENERAL PROTECTION FAULT (code: {})\n{:#?}",
            error_code, stack_frame
        );
    }
}

/// Geçersiz opcode hatası (#UD)
extern "x86-interrupt" fn invalid_opcode_handler(stack_frame: InterruptStackFrame) {
    let rip = stack_frame.instruction_pointer.as_u64();
    let cs = stack_frame.code_segment;
    if (cs & 3) == 3 {
        user_fault_exit("INVALID_OPCODE", rip, None, None);
    }
    crate::serial_println!("EXCEPTION: INVALID OPCODE (#UD) at RIP={:#x}", rip);
    crate::serial_println!("{:#?}", stack_frame);
    panic!("Invalid Opcode");
}

extern "x86-interrupt" fn nmi_handler(_stack_frame: InterruptStackFrame) {
    record_irq(2);
    dispatch_irq(2);
}

extern "x86-interrupt" fn spurious_interrupt_handler(_stack_frame: InterruptStackFrame) {
    record_irq(SPURIOUS_VECTOR);
}

// ============================================================================
// HARDWARE INTERRUPT HANDLERS (IRQs)
// ============================================================================

/// Sistem tick sayacı
static TICKS: AtomicU64 = AtomicU64::new(0);

/// Timer interrupt handler (IRQ0)
/// Her tick'te scheduler'ı çağırır.
extern "x86-interrupt" fn timer_interrupt_handler(_stack_frame: InterruptStackFrame) {
    record_irq(32);
    let needs_eoi = dispatch_irq(32);
    let ticks = TICKS.fetch_add(1, Ordering::Relaxed) + 1;
    let tsc = unsafe { _rdtsc() };
    crate::random::add_entropy(tsc ^ ticks);

    // Scheduler'a tick bildir
    crate::task::scheduler::tick();
    watchdog_poll(ticks);

    // vDSO'ya yeni zamanı yaz (1 tick = 10ms sayarak)
    let seconds_since_boot = ticks / 100;
    let nsec_since_boot = (ticks % 100) * 10_000_000;
    crate::vdso::update_time(seconds_since_boot, nsec_since_boot);

    if needs_eoi {
        irq_eoi(32);
    }

    // Softirq dispatch — Linux: irq_exit() → invoke_softirq()
    if crate::interrupts::softirq::softirq_pending() {
        crate::interrupts::softirq::do_softirq();
    }

    // TSC-deadline mode: sonraki deadline'ı yeniden arm et
    if crate::apic::lapic::is_tsc_deadline() {
        let freq = crate::apic::lapic::tsc_frequency();
        if freq > 0 {
            crate::apic::lapic::deadline_arm(freq / 100); // 10ms
        }
    }
}

extern "x86-interrupt" fn tlb_shootdown_handler(_stack_frame: InterruptStackFrame) {
    tlb::flush_all();
    crate::cpu::smp::notify_tlb_shootdown_ack();
    crate::apic::lapic::eoi();
}

use pc_keyboard::{layouts, HandleControl, Keyboard, ScancodeSet1};

lazy_static::lazy_static! {
    /// PS/2 Keyboard decoder
    static ref KEYBOARD: Mutex<Keyboard<layouts::Us104Key, ScancodeSet1>> = {
        Mutex::new(Keyboard::new(
            ScancodeSet1::new(),
            layouts::Us104Key,
            HandleControl::Ignore,
        ))
    };
}

/// Keyboard interrupt handler (IRQ1)
/// Scancode'u decode eder ve input queue'ya ekler.
extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
    use x86_64::instructions::port::Port;

    let mut port = Port::new(0x60);
    let scancode: u8 = unsafe { port.read() };
    let tsc = unsafe { _rdtsc() };
    crate::random::add_entropy(tsc ^ (scancode as u64));

    let result = KEYBOARD.lock().add_byte(scancode);
    match result {
        Ok(Some(key_event)) => {
            if let Some(key) = KEYBOARD.lock().process_keyevent(key_event) {
                crate::keyboard::push_key(key);
                use crate::drivers::input::InputEvent;
                crate::drivers::input::push_event(InputEvent::Keyboard(key));
            }
        }
        Ok(None) => {}
        Err(_) => {}
    }

    record_irq(33);
    let needs_eoi = dispatch_irq(33);
    if needs_eoi {
        irq_eoi(33);
    }
}

/// Mouse interrupt handler (IRQ12)
/// Raw byte'ı input queue'ya ekler (Fast-Path).
extern "x86-interrupt" fn mouse_interrupt_handler(_stack_frame: InterruptStackFrame) {
    use crate::drivers::input::InputEvent;
    use x86_64::instructions::port::Port;

    let mut data = Port::<u8>::new(0x60);
    let byte = unsafe { data.read() };
    let tsc = unsafe { _rdtsc() };
    crate::random::add_entropy(tsc ^ (byte as u64));

    crate::drivers::input::push_event(InputEvent::MouseByte(byte));

    record_irq(44);
    let needs_eoi = dispatch_irq(44);
    if needs_eoi {
        irq_eoi(44);
    }
}

/// Toplam geçen tick sayısını döndürür.
pub fn get_ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}

fn build_idt() -> InterruptDescriptorTable {
    let mut idt = InterruptDescriptorTable::new();
    idt.divide_error.set_handler_fn(divide_error_handler);
    idt.breakpoint.set_handler_fn(breakpoint_handler);
    idt.non_maskable_interrupt.set_handler_fn(nmi_handler);
    unsafe {
        idt.double_fault
            .set_handler_fn(double_fault_handler)
            .set_stack_index(crate::gdt::DOUBLE_FAULT_IST_INDEX);
        idt.page_fault
            .set_handler_fn(page_fault_handler)
            .set_stack_index(crate::gdt::PAGE_FAULT_IST_INDEX);
        idt.general_protection_fault
            .set_handler_fn(general_protection_fault_handler);
    }
    idt.invalid_opcode.set_handler_fn(invalid_opcode_handler);
    idt[32].set_handler_fn(timer_interrupt_handler);
    idt[33].set_handler_fn(keyboard_interrupt_handler);
    idt[44].set_handler_fn(mouse_interrupt_handler);
    idt[34].set_handler_fn(irq2_interrupt_handler);
    idt[35].set_handler_fn(irq3_interrupt_handler);
    idt[36].set_handler_fn(irq4_interrupt_handler);
    idt[37].set_handler_fn(irq5_interrupt_handler);
    idt[38].set_handler_fn(irq6_interrupt_handler);
    idt[39].set_handler_fn(irq7_interrupt_handler);
    idt[40].set_handler_fn(irq8_interrupt_handler);
    idt[41].set_handler_fn(irq9_interrupt_handler);
    idt[42].set_handler_fn(irq10_interrupt_handler);
    idt[43].set_handler_fn(irq11_interrupt_handler);
    idt[45].set_handler_fn(irq13_interrupt_handler);
    idt[46].set_handler_fn(irq14_interrupt_handler);
    idt[47].set_handler_fn(irq15_interrupt_handler);
    idt[48].set_handler_fn(irq16_interrupt_handler);
    idt[49].set_handler_fn(irq17_interrupt_handler);
    idt[50].set_handler_fn(irq18_interrupt_handler);
    idt[51].set_handler_fn(irq19_interrupt_handler);
    idt[52].set_handler_fn(irq20_interrupt_handler);
    idt[53].set_handler_fn(irq21_interrupt_handler);
    idt[54].set_handler_fn(irq22_interrupt_handler);
    idt[55].set_handler_fn(irq23_interrupt_handler);
    idt[56].set_handler_fn(irq24_interrupt_handler);
    idt[57].set_handler_fn(irq25_interrupt_handler);
    idt[58].set_handler_fn(irq26_interrupt_handler);
    idt[59].set_handler_fn(irq27_interrupt_handler);
    idt[60].set_handler_fn(irq28_interrupt_handler);
    idt[61].set_handler_fn(irq29_interrupt_handler);
    idt[62].set_handler_fn(irq30_interrupt_handler);
    idt[63].set_handler_fn(irq31_interrupt_handler);
    idt[IPI_TLB_VECTOR as usize].set_handler_fn(tlb_shootdown_handler);
    idt[SPURIOUS_VECTOR as usize].set_handler_fn(spurious_interrupt_handler);

    // ================================================================
    // EXCEPTION HARDENING — eksik 10 CPU exception vektörü
    // ================================================================
    idt.overflow.set_handler_fn(overflow_handler);                      // #OF (4)
    idt.bound_range_exceeded.set_handler_fn(bound_range_handler);       // #BR (5)
    idt.device_not_available.set_handler_fn(device_not_available_handler); // #NM (7)
    idt.segment_not_present.set_handler_fn(segment_not_present_handler); // #NP (11)
    idt.stack_segment_fault.set_handler_fn(stack_segment_handler);      // #SS (12)
    idt.x87_floating_point.set_handler_fn(x87_fp_handler);              // #MF (16)
    idt.alignment_check.set_handler_fn(alignment_check_handler);        // #AC (17)
    idt.machine_check.set_handler_fn(machine_check_handler);            // #MC (18)
    idt.simd_floating_point.set_handler_fn(simd_fp_handler);            // #XM (19)
    idt.virtualization.set_handler_fn(virtualization_handler);           // #VE (20)

    idt
}

fn init_global_once() {
    if INIT_STATE
        .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }
    VECTOR_ALLOCATOR.lock().init();
    #[cfg(not(target_os = "uefi"))]
    {
        if enable_ioapic() {
            crate::apic::ioapic::enable_irq(0);
            crate::apic::ioapic::enable_irq(1);
        } else {
            pic::init();
        }
    }
    #[cfg(target_os = "uefi")]
    {
        if enable_ioapic() {
            crate::apic::ioapic::enable_irq(0);
            crate::apic::ioapic::enable_irq(1);
        } else {
            pic::init();
        }
    }
    INIT_STATE.store(2, Ordering::SeqCst);
}

fn record_irq(vector: u8) {
    IRQ_COUNTS[vector as usize].fetch_add(1, Ordering::Relaxed);
    let tick = get_ticks();
    let last = IRQ_LAST_TICK[vector as usize].swap(tick, Ordering::Relaxed);
    if last == tick {
        let burst = IRQ_BURST[vector as usize].fetch_add(1, Ordering::Relaxed) + 1;
        if burst > IRQ_STORM_LIMIT.load(Ordering::Relaxed) {
            IRQ_STORMS[vector as usize].fetch_add(1, Ordering::Relaxed);
            if vector >= 32 && vector <= 47 {
                crate::apic::ioapic::disable_irq(vector - 32);
            }
        }
    } else {
        IRQ_BURST[vector as usize].store(0, Ordering::Relaxed);
    }
}

fn dispatch_irq(vector: u8) -> bool {
    let flow = { IRQ_FLOWS.lock()[vector as usize] };
    let handler = { IRQ_HANDLERS.lock()[vector as usize] };
    let threaded = { IRQ_THREAD_HANDLERS.lock()[vector as usize] };
    if flow == IrqFlow::FastEoi {
        irq_eoi(vector);
    }
    if let Some(func) = handler {
        let start = unsafe { _rdtsc() };
        func(vector);
        let end = unsafe { _rdtsc() };
        let delta = end.wrapping_sub(start);
        IRQ_TOTAL_LATENCY[vector as usize].fetch_add(delta, Ordering::Relaxed);
        IRQ_LATENCY_SAMPLES[vector as usize].fetch_add(1, Ordering::Relaxed);
        let mut current = IRQ_MAX_LATENCY[vector as usize].load(Ordering::Relaxed);
        while delta > current {
            match IRQ_MAX_LATENCY[vector as usize].compare_exchange(
                current,
                delta,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(next) => current = next,
            }
        }
        let cpu = crate::cpu::smp::current_cpu_id();
        let idx = IRQ_LOG_INDEX.fetch_add(1, Ordering::Relaxed);
        let mut log = IRQ_LOG.lock();
        let event = IrqEvent {
            vector,
            cpu,
            tsc: end,
            latency: delta,
        };
        if log.len() < IRQ_LOG_CAP {
            log.push(event);
        } else {
            let slot = idx % IRQ_LOG_CAP;
            log[slot] = event;
        }
    }
    if threaded.is_some() {
        enqueue_threaded_irq(vector);
    }
    flow != IrqFlow::FastEoi
}

pub fn print_irq_report() {
    let mut top_storms: Vec<(u8, u64)> = Vec::new();
    let mut top_latency: Vec<(u8, u64)> = Vec::new();
    let mut top_counts: Vec<(u8, u64)> = Vec::new();
    let mut total_count = 0u64;
    for v in 0u8..=255 {
        let count = IRQ_COUNTS[v as usize].load(Ordering::Relaxed);
        let storms = IRQ_STORMS[v as usize].load(Ordering::Relaxed);
        let max_latency = IRQ_MAX_LATENCY[v as usize].load(Ordering::Relaxed);
        if count > 0 {
            total_count = total_count.saturating_add(count);
            top_counts.push((v, count));
        }
        if storms > 0 {
            top_storms.push((v, storms));
        }
        if max_latency > 0 {
            top_latency.push((v, max_latency));
        }
    }
    top_storms.sort_by(|a, b| b.1.cmp(&a.1));
    top_latency.sort_by(|a, b| b.1.cmp(&a.1));
    top_counts.sort_by(|a, b| b.1.cmp(&a.1));
    crate::serial_println!("[IRQ] report begin");
    crate::serial_println!(
        "[IRQ] config storm_limit={} watchdog_interval={} latency_warn_cycles={}",
        IRQ_STORM_LIMIT.load(Ordering::Relaxed),
        IRQ_WATCHDOG_INTERVAL.load(Ordering::Relaxed),
        IRQ_LATENCY_WARN_CYCLES.load(Ordering::Relaxed)
    );
    crate::serial_println!("[IRQ] total_count={}", total_count);
    for (v, count) in top_counts.into_iter().take(8) {
        crate::serial_println!("[IRQ] count vector={} count={}", v, count);
    }
    for (v, storms) in top_storms.into_iter().take(8) {
        crate::serial_println!("[IRQ] storm vector={} storms={}", v, storms);
    }
    for (v, max_latency) in top_latency.into_iter().take(8) {
        let total = IRQ_TOTAL_LATENCY[v as usize].load(Ordering::Relaxed);
        let samples = IRQ_LATENCY_SAMPLES[v as usize].load(Ordering::Relaxed);
        let avg = if samples == 0 { 0 } else { total / samples };
        crate::serial_println!(
            "[IRQ] latency vector={} max_cycles={} avg_cycles={} samples={}",
            v,
            max_latency,
            avg,
            samples
        );
    }
    crate::serial_println!("[IRQ] report end");
}

pub fn print_irq_log_recent(max: usize) {
    let log = IRQ_LOG.lock();
    let len = log.len();
    if len == 0 {
        crate::serial_println!("[IRQ] log empty");
        return;
    }
    let count = max.min(len);
    let base = IRQ_LOG_INDEX.load(Ordering::Relaxed);
    crate::serial_println!("[IRQ] log begin count={}", count);
    for i in 0..count {
        let idx = if len < IRQ_LOG_CAP {
            len - count + i
        } else {
            (base + IRQ_LOG_CAP - count + i) % IRQ_LOG_CAP
        };
        let event = log[idx];
        crate::serial_println!(
            "[IRQ] log vector={} cpu={} tsc={} latency={}",
            event.vector,
            event.cpu,
            event.tsc,
            event.latency
        );
    }
    crate::serial_println!("[IRQ] log end");
}

fn watchdog_poll(ticks: u64) {
    let interval = IRQ_WATCHDOG_INTERVAL.load(Ordering::Relaxed);
    if interval == 0 || ticks % interval != 0 {
        return;
    }
    if WATCHDOG_LAST_TICK.swap(ticks, Ordering::Relaxed) == ticks {
        return;
    }
    for v in 0u8..=255 {
        let flow = { IRQ_FLOWS.lock()[v as usize] };
        if flow == IrqFlow::PerCpu {
            continue;
        }
        let storms = IRQ_STORMS[v as usize].load(Ordering::Relaxed);
        let last_reported = IRQ_STORMS_REPORTED[v as usize].load(Ordering::Relaxed);
        let stormed = storms > last_reported;
        if stormed {
            IRQ_STORMS_REPORTED[v as usize].store(storms, Ordering::Relaxed);
            crate::serial_println!("[IRQ] storm vector={} storms={}", v, storms);
            try_rebalance_vector(v, ticks);
        }
        let max_latency = IRQ_MAX_LATENCY[v as usize].load(Ordering::Relaxed);
        let reported_latency = IRQ_LATENCY_REPORTED[v as usize].load(Ordering::Relaxed);
        let warn = IRQ_LATENCY_WARN_CYCLES.load(Ordering::Relaxed);
        let latency_warned = max_latency > reported_latency && max_latency > warn;
        if latency_warned {
            IRQ_LATENCY_REPORTED[v as usize].store(max_latency, Ordering::Relaxed);
            crate::serial_println!("[IRQ] latency warn vector={} max_cycles={}", v, max_latency);
            try_rebalance_vector(v, ticks);
        }
        try_adjust_flow(v, ticks, stormed, latency_warned);
    }
}

fn try_rebalance_vector(vector: u8, ticks: u64) {
    let last = IRQ_REBALANCE_LAST[vector as usize].load(Ordering::Relaxed);
    let interval = IRQ_WATCHDOG_INTERVAL.load(Ordering::Relaxed).max(1);
    if ticks.saturating_sub(last) < interval {
        return;
    }
    IRQ_REBALANCE_LAST[vector as usize].store(ticks, Ordering::Relaxed);
    if vector >= 32 && vector <= 47 && ioapic_enabled() {
        let apic_id = select_apic_id_for_vector(vector);
        crate::apic::ioapic::set_irq_affinity(vector - 32, apic_id as u8);
    }
}

fn try_adjust_flow(vector: u8, ticks: u64, stormed: bool, latency_warned: bool) {
    if !IRQ_DYNAMIC_FLOW_ENABLED.load(Ordering::Relaxed) {
        return;
    }
    if vector < 32 || vector > 47 {
        return;
    }
    if !stormed && !latency_warned {
        return;
    }
    let flow = { IRQ_FLOWS.lock()[vector as usize] };
    if flow == IrqFlow::PerCpu || flow == IrqFlow::FastEoi {
        return;
    }
    let lock_until = IRQ_FLOW_LOCK_UNTIL[vector as usize].load(Ordering::Relaxed);
    if ticks < lock_until {
        return;
    }
    let last_change = IRQ_FLOW_LAST_CHANGE[vector as usize].load(Ordering::Relaxed);
    let cooldown = IRQ_FLOW_CHANGE_COOLDOWN.load(Ordering::Relaxed).max(1);
    if ticks.saturating_sub(last_change) < cooldown {
        if stormed && flow == IrqFlow::Edge {
            let prev = IRQ_FLOW_PREV[vector as usize].load(Ordering::Relaxed);
            let prev_flow = match prev {
                1 => IrqFlow::Edge,
                2 => IrqFlow::FastEoi,
                3 => IrqFlow::PerCpu,
                _ => IrqFlow::Level,
            };
            let target = if prev_flow == IrqFlow::Edge {
                IrqFlow::Level
            } else {
                prev_flow
            };
            apply_irq_flow(vector, target, ticks, true);
            IRQ_FLOW_LOCK_UNTIL[vector as usize].store(ticks + cooldown, Ordering::Relaxed);
        } else if latency_warned && flow == IrqFlow::Level {
            let prev = IRQ_FLOW_PREV[vector as usize].load(Ordering::Relaxed);
            let prev_flow = match prev {
                1 => IrqFlow::Edge,
                2 => IrqFlow::FastEoi,
                3 => IrqFlow::PerCpu,
                _ => IrqFlow::Level,
            };
            if prev_flow != IrqFlow::Level {
                apply_irq_flow(vector, prev_flow, ticks, true);
                IRQ_FLOW_LOCK_UNTIL[vector as usize].store(ticks + cooldown, Ordering::Relaxed);
            }
        }
        return;
    }
    let desired = if stormed {
        IrqFlow::Level
    } else if latency_warned {
        IrqFlow::Edge
    } else {
        return;
    };
    if desired != flow {
        apply_irq_flow(vector, desired, ticks, true);
        IRQ_FLOW_LOCK_UNTIL[vector as usize].store(ticks + cooldown, Ordering::Relaxed);
    }
}

fn start_irq_worker() {
    if !crate::task::scheduler::is_ready() {
        return;
    }

    if IRQ_WORKER_STARTED
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
    {
        crate::task::scheduler::spawn_with_priority(
            irq_thread_worker,
            crate::task::Priority::High,
            "irq_thread",
        );
    }
}

fn enqueue_threaded_irq(vector: u8) {
    IRQ_THREAD_QUEUE.lock().push_back(vector);
}

fn irq_thread_worker() -> ! {
    loop {
        let next = { IRQ_THREAD_QUEUE.lock().pop_front() };
        if let Some(vector) = next {
            let handler = { IRQ_THREAD_HANDLERS.lock()[vector as usize] };
            if let Some(func) = handler {
                func(vector);
            }
        } else {
            crate::task::scheduler::sleep(1);
        }
    }
}

fn flow_from_flags(flags: u64) -> IrqFlow {
    if (flags & IRQF_FAST_EOI) != 0 {
        IrqFlow::FastEoi
    } else if (flags & IRQF_PERCPU) != 0 {
        IrqFlow::PerCpu
    } else if (flags & IRQF_EDGE) != 0 {
        IrqFlow::Edge
    } else if (flags & IRQF_LEVEL) != 0 {
        IrqFlow::Level
    } else {
        IrqFlow::Level
    }
}

fn apply_irq_flow(vector: u8, flow: IrqFlow, ticks: u64, dynamic: bool) {
    let mut flows = IRQ_FLOWS.lock();
    let current = flows[vector as usize];
    if current == flow {
        return;
    }
    if dynamic {
        IRQ_FLOW_PREV[vector as usize].store(current as u8, Ordering::Relaxed);
    } else {
        IRQ_FLOW_PREV[vector as usize].store(flow as u8, Ordering::Relaxed);
    }
    IRQ_FLOW_LAST_CHANGE[vector as usize].store(ticks, Ordering::Relaxed);
    flows[vector as usize] = flow;
    if vector >= 32 && vector <= 47 && ioapic_enabled() {
        match flow {
            IrqFlow::Edge => crate::apic::ioapic::set_irq_trigger_mode(vector - 32, Some(false)),
            IrqFlow::Level => crate::apic::ioapic::set_irq_trigger_mode(vector - 32, Some(true)),
            _ => crate::apic::ioapic::set_irq_trigger_mode(vector - 32, None),
        }
    }
}

fn apic_id_to_mask(apic_id: u32) -> u64 {
    let state = crate::cpu::smp::SMP_STATE.lock();
    if let Some(pos) = state.cpu_apic_ids.iter().position(|id| *id == apic_id) {
        if pos < 64 {
            return 1u64 << pos;
        }
    }
    0
}

fn select_apic_id_for_vector(vector: u8) -> u32 {
    let state = crate::cpu::smp::SMP_STATE.lock();
    let cpu_count = state.cpu_apic_ids.len();
    if cpu_count == 0 {
        return crate::cpu::CPU_INFO.lock().bsp_apic_id;
    }
    let mut mask = IRQ_AFFINITY_MASKS[vector as usize].load(Ordering::Relaxed);
    if mask == 0 {
        if cpu_count >= 64 {
            mask = u64::MAX;
        } else {
            mask = (1u64 << cpu_count) - 1;
        }
    } else if cpu_count < 64 {
        mask &= (1u64 << cpu_count) - 1;
    }
    let cursor = IRQ_AFFINITY_CURSOR[vector as usize].fetch_add(1, Ordering::Relaxed) as usize;
    let idx = select_cpu_index(mask, cursor, cpu_count).unwrap_or(0);
    state.cpu_apic_ids[idx]
}

fn select_cpu_index(mask: u64, cursor: usize, cpu_count: usize) -> Option<usize> {
    if mask == 0 || cpu_count == 0 {
        return None;
    }
    let limit = cpu_count.min(64);
    for offset in 0..limit {
        let idx = (cursor + offset) % limit;
        if ((mask >> idx) & 1) != 0 {
            return Some(idx);
        }
    }
    None
}

fn irq_eoi(vector: u8) {
    if USE_IOAPIC.load(Ordering::SeqCst) {
        crate::apic::lapic::eoi();
    } else {
        unsafe {
            pic::PICS.lock().notify_end_of_interrupt(vector);
        }
    }
}

struct VectorAllocator {
    used: [bool; 256],
}

static VECTOR_ALLOCATOR: Mutex<VectorAllocator> =
    Mutex::new(VectorAllocator { used: [false; 256] });

impl VectorAllocator {
    fn init(&mut self) {
        for v in 0..32 {
            self.used[v] = true;
        }
        self.used[IPI_TLB_VECTOR as usize] = true;
        self.used[SPURIOUS_VECTOR as usize] = true;
        for v in 32..=47 {
            self.used[v] = true;
        }
    }

    fn alloc_vector(&mut self) -> Option<u8> {
        for v in MSI_VECTOR_START..=MSI_VECTOR_END {
            if !self.used[v as usize] {
                self.used[v as usize] = true;
                return Some(v);
            }
        }
        None
    }

    fn alloc_range(&mut self, count: usize) -> Option<Vec<u8>> {
        let count_u8 = u8::try_from(count).ok()?;
        if count == 0 || count > (MSI_VECTOR_END - MSI_VECTOR_START + 1) as usize {
            return None;
        }
        let max_start = MSI_VECTOR_END - count_u8 + 1;
        let mut start = MSI_VECTOR_START;
        while start <= max_start {
            let mut ok = true;
            for offset in 0..count_u8 {
                if self.used[(start + offset) as usize] {
                    ok = false;
                    start += offset + 1;
                    break;
                }
            }
            if ok {
                let mut vectors = Vec::with_capacity(count);
                for offset in 0..count_u8 {
                    let vector = start + offset;
                    self.used[vector as usize] = true;
                    vectors.push(vector);
                }
                return Some(vectors);
            }
        }
        None
    }

    fn free_vector(&mut self, vector: u8) {
        if vector >= MSI_VECTOR_START && vector <= MSI_VECTOR_END {
            self.used[vector as usize] = false;
        }
    }
}

extern "x86-interrupt" fn irq2_interrupt_handler(_stack_frame: InterruptStackFrame) {
    record_irq(34);
    let needs_eoi = dispatch_irq(34);
    if needs_eoi {
        irq_eoi(34);
    }
}

extern "x86-interrupt" fn irq3_interrupt_handler(_stack_frame: InterruptStackFrame) {
    record_irq(35);
    let needs_eoi = dispatch_irq(35);
    if needs_eoi {
        irq_eoi(35);
    }
}

extern "x86-interrupt" fn irq4_interrupt_handler(_stack_frame: InterruptStackFrame) {
    record_irq(36);
    let needs_eoi = dispatch_irq(36);
    if needs_eoi {
        irq_eoi(36);
    }
}

extern "x86-interrupt" fn irq5_interrupt_handler(_stack_frame: InterruptStackFrame) {
    record_irq(37);
    let needs_eoi = dispatch_irq(37);
    if needs_eoi {
        irq_eoi(37);
    }
}

extern "x86-interrupt" fn irq6_interrupt_handler(_stack_frame: InterruptStackFrame) {
    record_irq(38);
    let needs_eoi = dispatch_irq(38);
    if needs_eoi {
        irq_eoi(38);
    }
}

extern "x86-interrupt" fn irq7_interrupt_handler(_stack_frame: InterruptStackFrame) {
    record_irq(39);
    let needs_eoi = dispatch_irq(39);
    if needs_eoi {
        irq_eoi(39);
    }
}

extern "x86-interrupt" fn irq8_interrupt_handler(_stack_frame: InterruptStackFrame) {
    record_irq(40);
    let needs_eoi = dispatch_irq(40);
    if needs_eoi {
        irq_eoi(40);
    }
}

extern "x86-interrupt" fn irq9_interrupt_handler(_stack_frame: InterruptStackFrame) {
    record_irq(41);
    let needs_eoi = dispatch_irq(41);
    if needs_eoi {
        irq_eoi(41);
    }
}

extern "x86-interrupt" fn irq10_interrupt_handler(_stack_frame: InterruptStackFrame) {
    record_irq(42);
    let needs_eoi = dispatch_irq(42);
    if needs_eoi {
        irq_eoi(42);
    }
}

extern "x86-interrupt" fn irq11_interrupt_handler(_stack_frame: InterruptStackFrame) {
    record_irq(43);
    let needs_eoi = dispatch_irq(43);
    if needs_eoi {
        irq_eoi(43);
    }
}

extern "x86-interrupt" fn irq13_interrupt_handler(_stack_frame: InterruptStackFrame) {
    record_irq(45);
    let needs_eoi = dispatch_irq(45);
    if needs_eoi {
        irq_eoi(45);
    }
}

extern "x86-interrupt" fn irq14_interrupt_handler(_stack_frame: InterruptStackFrame) {
    record_irq(46);
    let needs_eoi = dispatch_irq(46);
    if needs_eoi {
        irq_eoi(46);
    }
}

extern "x86-interrupt" fn irq15_interrupt_handler(_stack_frame: InterruptStackFrame) {
    record_irq(47);
    let needs_eoi = dispatch_irq(47);
    if needs_eoi {
        irq_eoi(47);
    }
}

extern "x86-interrupt" fn irq16_interrupt_handler(_stack_frame: InterruptStackFrame) {
    record_irq(48);
    let needs_eoi = dispatch_irq(48);
    if needs_eoi {
        irq_eoi(48);
    }
}

extern "x86-interrupt" fn irq17_interrupt_handler(_stack_frame: InterruptStackFrame) {
    record_irq(49);
    let needs_eoi = dispatch_irq(49);
    if needs_eoi {
        irq_eoi(49);
    }
}

extern "x86-interrupt" fn irq18_interrupt_handler(_stack_frame: InterruptStackFrame) {
    record_irq(50);
    let needs_eoi = dispatch_irq(50);
    if needs_eoi {
        irq_eoi(50);
    }
}

extern "x86-interrupt" fn irq19_interrupt_handler(_stack_frame: InterruptStackFrame) {
    record_irq(51);
    let needs_eoi = dispatch_irq(51);
    if needs_eoi {
        irq_eoi(51);
    }
}

extern "x86-interrupt" fn irq20_interrupt_handler(_stack_frame: InterruptStackFrame) {
    record_irq(52);
    let needs_eoi = dispatch_irq(52);
    if needs_eoi {
        irq_eoi(52);
    }
}

extern "x86-interrupt" fn irq21_interrupt_handler(_stack_frame: InterruptStackFrame) {
    record_irq(53);
    let needs_eoi = dispatch_irq(53);
    if needs_eoi {
        irq_eoi(53);
    }
}

extern "x86-interrupt" fn irq22_interrupt_handler(_stack_frame: InterruptStackFrame) {
    record_irq(54);
    let needs_eoi = dispatch_irq(54);
    if needs_eoi {
        irq_eoi(54);
    }
}

extern "x86-interrupt" fn irq23_interrupt_handler(_stack_frame: InterruptStackFrame) {
    record_irq(55);
    let needs_eoi = dispatch_irq(55);
    if needs_eoi {
        irq_eoi(55);
    }
}

extern "x86-interrupt" fn irq24_interrupt_handler(_stack_frame: InterruptStackFrame) {
    record_irq(56);
    let needs_eoi = dispatch_irq(56);
    if needs_eoi {
        irq_eoi(56);
    }
}

extern "x86-interrupt" fn irq25_interrupt_handler(_stack_frame: InterruptStackFrame) {
    record_irq(57);
    let needs_eoi = dispatch_irq(57);
    if needs_eoi {
        irq_eoi(57);
    }
}

extern "x86-interrupt" fn irq26_interrupt_handler(_stack_frame: InterruptStackFrame) {
    record_irq(58);
    let needs_eoi = dispatch_irq(58);
    if needs_eoi {
        irq_eoi(58);
    }
}

extern "x86-interrupt" fn irq27_interrupt_handler(_stack_frame: InterruptStackFrame) {
    record_irq(59);
    let needs_eoi = dispatch_irq(59);
    if needs_eoi {
        irq_eoi(59);
    }
}

extern "x86-interrupt" fn irq28_interrupt_handler(_stack_frame: InterruptStackFrame) {
    record_irq(60);
    let needs_eoi = dispatch_irq(60);
    if needs_eoi {
        irq_eoi(60);
    }
}

extern "x86-interrupt" fn irq29_interrupt_handler(_stack_frame: InterruptStackFrame) {
    record_irq(61);
    let needs_eoi = dispatch_irq(61);
    if needs_eoi {
        irq_eoi(61);
    }
}

extern "x86-interrupt" fn irq30_interrupt_handler(_stack_frame: InterruptStackFrame) {
    record_irq(62);
    let needs_eoi = dispatch_irq(62);
    if needs_eoi {
        irq_eoi(62);
    }
}

extern "x86-interrupt" fn irq31_interrupt_handler(_stack_frame: InterruptStackFrame) {
    record_irq(63);
    let needs_eoi = dispatch_irq(63);
    if needs_eoi {
        irq_eoi(63);
    }
}

// ============================================================================
// EXCEPTION HARDENING — 10 Eksik CPU Exception Handler
// Linux kernel/traps.c karşılığı
// ============================================================================

/// #OF — Overflow (INT 4)
extern "x86-interrupt" fn overflow_handler(stack_frame: InterruptStackFrame) {
    let cs = stack_frame.code_segment;
    if (cs & 3) == 3 {
        user_fault_exit("OVERFLOW", stack_frame.instruction_pointer.as_u64(), None, None);
    }
    crate::serial_println!("EXCEPTION: OVERFLOW (#OF)\n{:#?}", stack_frame);
    panic!("Overflow");
}

/// #BR — Bound Range Exceeded (INT 5)
extern "x86-interrupt" fn bound_range_handler(stack_frame: InterruptStackFrame) {
    let cs = stack_frame.code_segment;
    if (cs & 3) == 3 {
        user_fault_exit("BOUND_RANGE", stack_frame.instruction_pointer.as_u64(), None, None);
    }
    crate::serial_println!("EXCEPTION: BOUND RANGE EXCEEDED (#BR)\n{:#?}", stack_frame);
    panic!("Bound Range Exceeded");
}

/// #NM — Device Not Available (INT 7) — FPU/SSE lazy save
extern "x86-interrupt" fn device_not_available_handler(stack_frame: InterruptStackFrame) {
    // FPU/SSE kullanılmadığında tetiklenir — CR0.TS flag
    // Kernel: TS flag'i temizle ve FPU state'i yükle
    unsafe {
        // CR0.TS bit'ini temizle (Task Switched)
        let cr0 = x86_64::registers::control::Cr0::read();
        x86_64::registers::control::Cr0::write(
            cr0 & !x86_64::registers::control::Cr0Flags::TASK_SWITCHED,
        );
    }
    crate::serial_println!(
        "[EXCEPTION] #NM: Device Not Available at RIP={:#x} — TS cleared",
        stack_frame.instruction_pointer.as_u64()
    );
}

/// #NP — Segment Not Present (INT 11)
extern "x86-interrupt" fn segment_not_present_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    let cs = stack_frame.code_segment;
    if (cs & 3) == 3 {
        user_fault_exit(
            "SEGMENT_NOT_PRESENT",
            stack_frame.instruction_pointer.as_u64(),
            None,
            Some(error_code),
        );
    }
    crate::serial_println!(
        "EXCEPTION: SEGMENT NOT PRESENT (#NP) code={:#x}\n{:#?}",
        error_code,
        stack_frame
    );
    dump_registers(&stack_frame);
    panic!("Segment Not Present");
}

/// #SS — Stack Segment Fault (INT 12)
extern "x86-interrupt" fn stack_segment_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    let cs = stack_frame.code_segment;
    if (cs & 3) == 3 {
        user_fault_exit(
            "STACK_SEGMENT_FAULT",
            stack_frame.instruction_pointer.as_u64(),
            None,
            Some(error_code),
        );
    }
    crate::serial_println!(
        "EXCEPTION: STACK SEGMENT FAULT (#SS) code={:#x}\n{:#?}",
        error_code,
        stack_frame
    );
    dump_registers(&stack_frame);
    panic!("Stack Segment Fault");
}

/// #MF — x87 Floating-Point Error (INT 16)
extern "x86-interrupt" fn x87_fp_handler(stack_frame: InterruptStackFrame) {
    let cs = stack_frame.code_segment;
    if (cs & 3) == 3 {
        user_fault_exit("X87_FP_ERROR", stack_frame.instruction_pointer.as_u64(), None, None);
    }
    crate::serial_println!("EXCEPTION: x87 FLOATING POINT (#MF)\n{:#?}", stack_frame);
    panic!("x87 Floating-Point Exception");
}

/// #AC — Alignment Check (INT 17)
extern "x86-interrupt" fn alignment_check_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    let cs = stack_frame.code_segment;
    if (cs & 3) == 3 {
        user_fault_exit(
            "ALIGNMENT_CHECK",
            stack_frame.instruction_pointer.as_u64(),
            None,
            Some(error_code),
        );
    }
    crate::serial_println!(
        "EXCEPTION: ALIGNMENT CHECK (#AC) code={:#x}\n{:#?}",
        error_code,
        stack_frame
    );
    panic!("Alignment Check");
}

/// #MC — Machine Check (INT 18) — kurtarılamaz donanım hatası
extern "x86-interrupt" fn machine_check_handler(stack_frame: InterruptStackFrame) -> ! {
    crate::serial_println!("EXCEPTION: MACHINE CHECK (#MC) — FATAL HARDWARE ERROR");
    crate::serial_println!("{:#?}", stack_frame);
    dump_registers(&stack_frame);
    // MCi_STATUS MSR'larını oku (IA32_MC0_STATUS = 0x401)
    for i in 0u32..4 {
        let msr_addr = 0x401 + i * 4;
        unsafe {
            let val = x86_64::registers::model_specific::Msr::new(msr_addr).read();
            if val & (1u64 << 63) != 0 {
                // Valid bit set
                crate::serial_println!("[MC] Bank {} STATUS: {:#018x}", i, val);
            }
        }
    }
    panic!("Machine Check Exception");
}

/// #XM — SIMD Floating-Point (INT 19)
extern "x86-interrupt" fn simd_fp_handler(stack_frame: InterruptStackFrame) {
    let cs = stack_frame.code_segment;
    if (cs & 3) == 3 {
        user_fault_exit("SIMD_FP_ERROR", stack_frame.instruction_pointer.as_u64(), None, None);
    }
    // MXCSR register'ından hata detayını oku
    let mut mxcsr: u32 = 0;
    unsafe {
        core::arch::asm!("stmxcsr [{}]", in(reg) &mut mxcsr as *mut u32, options(nostack));
    }
    crate::serial_println!(
        "EXCEPTION: SIMD FLOATING POINT (#XM) MXCSR={:#010x}\n{:#?}",
        mxcsr,
        stack_frame
    );
    panic!("SIMD Floating-Point Exception");
}

/// #VE — Virtualization Exception (INT 20)
extern "x86-interrupt" fn virtualization_handler(stack_frame: InterruptStackFrame) {
    crate::serial_println!("EXCEPTION: VIRTUALIZATION (#VE)\n{:#?}", stack_frame);
    panic!("Virtualization Exception");
}

// ============================================================================
// IRQ STATISTICS FOR MONITORING
// ============================================================================

/// IRQ statistics for monitoring
#[derive(Clone, Debug)]
pub struct IrqStats {
    pub storm_count: u64,
    pub total_irqs: u64,
    pub spurious_count: u64,
}

/// Get IRQ statistics for monitoring
pub fn get_stats() -> IrqStats {
    let mut total = 0u64;
    let mut storms = 0u64;
    
    for i in 0..256 {
        total += IRQ_COUNTS[i].load(Ordering::SeqCst);
        storms += IRQ_STORMS_REPORTED[i].load(Ordering::SeqCst);
    }
    
    IrqStats {
        storm_count: storms,
        total_irqs: total,
        spurious_count: 0, // Would need tracking
    }
}

