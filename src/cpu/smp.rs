//! # echOS SMP (Symmetric Multi-Processing) Modülü
//!
//! Çoklu işlemci başlatma, yönetimi ve load balancing.
//! Intel INIT-SIPI-SIPI algoritması ile AP (Application Processor) startup.
//!
//! ## Özellikler
//! - Linux-style CPU state machine (OFFLINE → STARTING → ONLINE)
//! - CPU affinity mask desteği
//! - Startup verification
//! - Load balancing
//!
//! ## INIT-SIPI-SIPI AP Başlatma Dizisi (Intel SDM Vol.3A §10.6)
//!
//! ```text
//!   BSP (Bootstrap Processor)              AP (Application Processor)
//!        │                                         │
//!        │── INIT assert IPI ──────────────────►  │ [Sıfırlama]
//!        │   (10 ms bekle)                         │
//!        │── INIT deassert IPI ───────────────►   │
//!        │   (1 ms bekle)                          │
//!        │── SIPI #1 (vektör=0x1000/0x100) ─────► │ [Real Mod'dan başla]
//!        │   (200 µs bekle)                        │  CS:IP = vektör*0x100:0
//!        │── SIPI #2 (yedek) ──────────────────►  │ [Zaten başladıysa yoksay]
//!        │                                         │
//!        │   online_cpus sayacını izle             │── Long Mode'a geç ──►
//!        │◄── online_cpus artışını bekle ──────────│── ap_entry() çalışıyor
//!        │
//!   [AP kurulumu tamamlandı]
//! ```

use crate::memory::active_physical_offset;
use crate::cpu::smp_state::{CpuHotplugState, CPU_STATES, CpuAffinity};
use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use core::arch::global_asm;
use core::sync::atomic::{compiler_fence, AtomicBool, AtomicU32, AtomicUsize, Ordering};
use spin::Mutex;
use x86_64::instructions::interrupts;
use x86_64::registers::control::Cr3;
use x86_64::registers::model_specific::Msr;

/// AP startup kodunun yükleneceği adres (0x1000)
const AP_STARTUP_ADDR: u32 = 0x1000;
const AP_STARTUP_SEGMENT: u32 = AP_STARTUP_ADDR >> 12;

/// APIC register offset'leri
const APIC_ICR_LOW: u32 = 0x300;
const APIC_ICR_HIGH: u32 = 0x310;

/// APIC IPI (Inter-Processor Interrupt) tipleri
const APIC_DELIVERY_INIT: u32 = 0x500;
const APIC_DELIVERY_STARTUP: u32 = 0x600;
const APIC_LEVEL_ASSERT: u32 = 1 << 14;
const APIC_TRIGGER_LEVEL: u32 = 1 << 15;

/// Global SMP durumu
pub static SMP_STATE: Mutex<SmpState> = Mutex::new(SmpState::new());
static TLB_SHOOTDOWN_ACKS: AtomicUsize = AtomicUsize::new(0);
static TLB_SHOOTDOWN_LOCK: Mutex<()> = Mutex::new(());
const TLB_SHOOTDOWN_TIMEOUT_TICKS: usize = 10;
static TLB_SHOOTDOWN_REQUESTS: AtomicUsize = AtomicUsize::new(0);
static TLB_SHOOTDOWN_TIMEOUTS: AtomicUsize = AtomicUsize::new(0);
static TLB_SHOOTDOWN_WATCHDOGS: AtomicUsize = AtomicUsize::new(0);
static TLB_SHOOTDOWN_LAST_TARGETS: AtomicUsize = AtomicUsize::new(0);
static TLB_SHOOTDOWN_LAST_ACKS: AtomicUsize = AtomicUsize::new(0);
static TLB_SHOOTDOWN_LAST_DURATION: AtomicUsize = AtomicUsize::new(0);

/// Her CPU için per-CPU veri yapısı
#[repr(C, align(64))]
pub struct PerCpuData {
    /// CPU ID (0 = BSP, 1+ = AP)
    pub cpu_id: u32,
    /// APIC ID
    pub apic_id: u32,
    /// Çalışan task sayısı
    pub task_count: u32,
    /// Load average (0-100)
    pub load: u32,
    /// Online durumu
    pub online: bool,
    /// Yığın işaretçisi (tüm yığın bellek alanının tepe adresi)
    pub stack_top: u64,
    pub dma_domain: u32,
    /// Cache-line padding (false sharing önlemek için)
    _padding: [u8; 64 - 36],
}

/// SMP durum yapısı
pub struct SmpState {
    /// Toplam CPU sayısı
    pub cpu_count: u32,
    /// Online CPU sayısı
    pub online_cpus: AtomicU32,
    /// APIC base adresi
    pub apic_base: u64,
    pub apic_virt: u64,
    /// Her CPU için ayrı veri dizisi (per-CPU yapıları)
    pub per_cpu_data: Vec<&'static mut PerCpuData>,
    pub syscall_cpu_data: Vec<&'static mut crate::syscall::CpuData>,
    pub syscall_stacks: Vec<&'static mut [u8]>,
    pub cpu_apic_ids: Vec<u32>,
    /// AP startup flag'leri
    pub ap_started: Vec<AtomicBool>,
}

/// Mevcut CPU'nun kimlik numarasını döndür (GS segment tabanından okunur)
pub fn get_current_cpu_id() -> u32 {
    // GS tabanından oku (per-CPU verisi tarafından ayarlanır)
    // BSP için 0 döndürür
    // AP'ler için kendi CPU kimliğini döndürür
    unsafe {
        let cpu_id: u32;
        core::arch::asm!("mov {0}, gs:0", out(reg) cpu_id, options(nostack, pure, readonly));
        cpu_id
    }
}

/// Çevrimiçi (online) CPU sayısını döndür
pub fn online_cpu_count() -> u32 {
    SMP_STATE.lock().online_cpus.load(Ordering::SeqCst)
}

/// Toplam CPU sayısını döndür (çevrimdışı olanlar dahil)
pub fn total_cpu_count() -> u32 {
    SMP_STATE.lock().cpu_count
}

impl SmpState {
    pub const fn new() -> Self {
        Self {
            cpu_count: 1,
            online_cpus: AtomicU32::new(1),
            apic_base: 0,
            apic_virt: 0,
            per_cpu_data: Vec::new(),
            syscall_cpu_data: Vec::new(),
            syscall_stacks: Vec::new(),
            cpu_apic_ids: Vec::new(),
            ap_started: Vec::new(),
    }
}
}


#[repr(C, packed)]
struct ApStartupData {
    pml4_phys: u64,
    entry: u64,
    stack_top: u64,
    cpu_data: u64,
}

global_asm!(include_str!("ap_startup.asm"));

extern "C" {
    static ap_startup_begin: u8;
    static ap_startup_end: u8;
    static ap_startup_data: u8;
}

unsafe fn ap_startup_data_ptr() -> *mut ApStartupData {
    let start = &ap_startup_begin as *const u8 as usize;
    let data = &ap_startup_data as *const u8 as usize;
    let offset = data - start;
    // AP_STARTUP_ADDR fiziksel adresini HHDM üzerinden referans et:
    (crate::memory::active_physical_offset() + AP_STARTUP_ADDR as u64 + offset as u64) as *mut ApStartupData
}

/// AP startup kodunu belleğe yükler
/// AP'ler SIPI ile 0x1000 fiziksel adresine atlarlar (SIPI vector 0x1 * 0x100 = 0x1000)
/// Bu yüzden kod doğrudan fiziksel adres 0x1000'e yazılmalı (identity mapped bölgede)
unsafe fn load_ap_startup_code() {
    let src_ptr = &ap_startup_begin as *const u8;
    let size = (&ap_startup_end as *const u8 as usize) - (&ap_startup_begin as *const u8 as usize);

    // KERNEL_PML4 içerisine 0x1000 için kimlik eşlemesi (identity mapping) kur.
    // Bu olmadan AP Paging+Long mode açtığında fetch hatası (Triple Fault) yaşar.
    crate::memory::map_identity(AP_STARTUP_ADDR as u64, 4096);

    // AP startup kodunu fiziksel 0x1000 adresine kopyalamak için HHDM adresini kullan:
    let dest_ptr = (crate::memory::active_physical_offset() + AP_STARTUP_ADDR as u64) as *mut u8;

    crate::serial_println!(
        "SMP: copying AP startup code to phys=0x{:x} (virt=0x{:x})",
        AP_STARTUP_ADDR as u64,
        dest_ptr as u64
    );

    // Hata ayıklama: Kaynak kodunu dök
            let src_slice = core::slice::from_raw_parts(src_ptr, 128);
            crate::serial_println!("SMP: Source code (128 bytes): {:02x?}", src_slice);

            core::ptr::copy_nonoverlapping(src_ptr, dest_ptr, size);

            // Hata ayıklama: Hedef kodunu dök
            let dest_slice = core::slice::from_raw_parts(dest_ptr, 128);
            crate::serial_println!("SMP: Dest code (128 bytes): {:02x?}", dest_slice);

    // Bellek görünürlüğünü garantile (tüm CPU'larda okuma sıralaması)
    core::sync::atomic::fence(Ordering::SeqCst);

    crate::serial_println!("SMP: AP startup code copied");
    let mut pml4_phys = crate::memory::KERNEL_PML4_PHYS;
    if pml4_phys == 0 {
        let (pml4_frame, _) = Cr3::read();
        pml4_phys = pml4_frame.start_address().as_u64();
    }
    if pml4_phys > 0xFFFF_FFFF {
        crate::serial_println!(
            "SMP: WARNING: PML4 > 4GB ({:#x}), AP startup may fail!",
            pml4_phys
        );
    }
    crate::serial_println!("SMP: AP PML4 phys={:#x}", pml4_phys);
    let data = &mut *ap_startup_data_ptr();
    data.pml4_phys = pml4_phys;
    data.entry = crate::cpu::ap::ap_entry as *const () as u64;
    data.stack_top = 0;
    data.cpu_data = 0;
    compiler_fence(Ordering::SeqCst);
}

unsafe fn prepare_ap_startup_data(stack_top: u64, cpu_data: u64) {
    let data = &mut *ap_startup_data_ptr();

    // Get PML4 physical address — PML4 fiziksel adresini al
    let mut pml4_phys = crate::memory::KERNEL_PML4_PHYS;
    if pml4_phys == 0 {
        let (pml4_frame, _) = Cr3::read();
        pml4_phys = pml4_frame.start_address().as_u64();
    }

    // ap_entry sanal adresini HHDM adresine dönüştürme işlemi
    // UEFI modunda çekirdek, UEFI tarafından fiziksel bir adrese yüklenir ve kimlik eşlemeli
    // ya da düşük bir sanal adrese eşlenmiş olabilir. Bu adresi, AP'nin çekirdek PML4 ile
    // sayfalama etkinleştirdikten sonra erişebileceği bir sanal adrese dönüştürmemiz gerekir.
    //
    // UEFI'de çekirdek genellikle düşük bir fiziksel adrese yüklenir (örn. 0x7d461ff0)
    // ve aldığımız fonksiyon işaretçisi zaten o adreste olur (üst yarıda değil).
    // Bu adresi doğrudan fiziksel adres olarak kullanıp HHDM'e dönüştürebiliriz.
    // echOS'ta çekirdeğin `.text` bölümü çalıştırılabilir (XD=0) olarak eşlenmiştir.
    // Ancak tüm HHDM (Üst Yarı Doğrudan Eşleme), güvenlik amacıyla açıkça
    // NX (Çalıştırma Yok / No-Execute) biti ile işaretlenmiştir.
    // HHDM adresini AP'ye gönderirsek, `call rax` anında triple-fault oluşturur.
    // AP'ye `.text` bölümündeki orijinal çalıştırılabilir sanal adresi vermek ZORUNLUDUR.
    let entry_virtual = crate::cpu::ap::ap_entry as *const () as u64;

    data.pml4_phys = pml4_phys;
    data.entry = entry_virtual;  // Gerçek çalıştırılabilir sanal adresi sakla
    data.stack_top = stack_top;
    data.cpu_data = cpu_data;

    // Hizalama sorunlarını önlemek için değerleri yerel değişkenlere oku (packed struct)
    let pml4 = data.pml4_phys;
    let entry = data.entry;
    let stack = data.stack_top;
    let cpu = data.cpu_data;

    crate::serial_println!("SMP: AP startup data prepared:");
    crate::serial_println!("  pml4_phys = {:#x}", pml4);
    crate::serial_println!("  entry (virtual) = {:#x}", entry);
    crate::serial_println!("  stack_top = {:#x}", stack);
    crate::serial_println!("  cpu_data = {:#x}", cpu);

    compiler_fence(Ordering::SeqCst);
}

fn has_x2apic() -> bool {
    crate::cpu::CPU_INFO.lock().has_x2apic
}

/// APIC register oku
unsafe fn read_apic_reg(reg: u32) -> u32 {
    if has_x2apic() {
        let msr = 0x800 + (reg >> 4);
        return Msr::new(msr).read() as u32;
    }
    let apic_ptr = apic_mmio_base();
    core::ptr::read_volatile(apic_ptr.add((reg >> 2) as usize))
}

/// APIC register yaz
unsafe fn write_apic_reg(reg: u32, value: u32) {
    if has_x2apic() {
        let msr = 0x800 + (reg >> 4);
        Msr::new(msr).write(value as u64);
        return;
    }
    let apic_ptr = apic_mmio_base();
    core::ptr::write_volatile(apic_ptr.add((reg >> 2) as usize), value);
}

unsafe fn apic_mmio_base() -> *mut u32 {
    let mut state = SMP_STATE.lock();
    if state.apic_virt != 0 {
        return state.apic_virt as *mut u32;
    }
    let msr_base = Msr::new(0x1B).read() & 0xFFFFF000;
    let mut apic_base = state.apic_base;
    if apic_base == 0 || apic_base != msr_base {
        apic_base = msr_base;
        state.apic_base = apic_base;
    }
    let mapped = crate::memory::map_mmio(apic_base, 0x1000);
    let virt = if !mapped.is_null() {
        mapped as u64
    } else {
        active_physical_offset() + apic_base
    };
    state.apic_virt = virt;
    virt as *mut u32
}

/// IPI (Inter-Processor Interrupt) gönder
unsafe fn send_ipi(dest_apic_id: u32, delivery_mode: u32, vector: u32) {
    if has_x2apic() {
        let icr = ((dest_apic_id as u64) << 32) | (delivery_mode as u64) | (vector as u64);
        Msr::new(0x830).write(icr);
        let mut spins = 0u32;
        while (Msr::new(0x830).read() & (1 << 12)) != 0 {
            spins = spins.saturating_add(1);
            if spins > 1_000_000 {
                crate::serial_println!("SMP: IPI delivery timeout");
                break;
            }
            core::hint::spin_loop();
        }
        return;
    }
    // ICR_HIGH: hedef APIC kimliği (çökme 24'e kaydırılır)
    write_apic_reg(APIC_ICR_HIGH, dest_apic_id << 24);

    // ICR_LOW: iletim modu + kesme vektörü
    let icr_low = delivery_mode | vector;
    write_apic_reg(APIC_ICR_LOW, icr_low);

    // IPI gönderimini bekle
    let mut spins = 0u32;
    while (read_apic_reg(APIC_ICR_LOW) & (1 << 12)) != 0 {
        spins = spins.saturating_add(1);
        if spins > 1_000_000 {
            crate::serial_println!("SMP: IPI delivery timeout");
            break;
        }
        core::hint::spin_loop();
    }
}

pub fn send_tlb_shootdown_ipi() {
    let _guard = TLB_SHOOTDOWN_LOCK.lock();
    let current_apic = read_local_apic_id();
    let state = SMP_STATE.lock();
    let mut targets = Vec::new();
    for cpu in state.per_cpu_data.iter() {
        if !cpu.online {
            continue;
        }
        if cpu.apic_id == current_apic {
            continue;
        }
        targets.push(cpu.apic_id);
    }
    drop(state);
    if targets.is_empty() {
        return;
    }
    TLB_SHOOTDOWN_REQUESTS.fetch_add(1, Ordering::SeqCst);
    TLB_SHOOTDOWN_LAST_TARGETS.store(targets.len(), Ordering::SeqCst);
    TLB_SHOOTDOWN_ACKS.store(0, Ordering::SeqCst);
    compiler_fence(Ordering::SeqCst);
    for apic_id in targets.iter().copied() {
        unsafe {
            send_ipi(apic_id, 0, crate::interrupts::IPI_TLB_VECTOR as u32);
        }
    }
    if !interrupts::are_enabled() {
        return;
    }
    let start_ticks = crate::task::scheduler::get_ticks();
    while TLB_SHOOTDOWN_ACKS.load(Ordering::SeqCst) < targets.len() {
        let elapsed = crate::task::scheduler::get_ticks().saturating_sub(start_ticks);
        if elapsed > TLB_SHOOTDOWN_TIMEOUT_TICKS {
            break;
        }
        core::hint::spin_loop();
    }
    let mut acks = TLB_SHOOTDOWN_ACKS.load(Ordering::SeqCst);
    let mut elapsed = crate::task::scheduler::get_ticks().saturating_sub(start_ticks);
    if acks < targets.len() {
        TLB_SHOOTDOWN_TIMEOUTS.fetch_add(1, Ordering::SeqCst);
        TLB_SHOOTDOWN_WATCHDOGS.fetch_add(1, Ordering::SeqCst);
        for apic_id in targets.iter().copied() {
            unsafe {
                send_ipi(apic_id, 0, crate::interrupts::IPI_TLB_VECTOR as u32);
            }
        }
        let retry_start = crate::task::scheduler::get_ticks();
        while TLB_SHOOTDOWN_ACKS.load(Ordering::SeqCst) < targets.len() {
            let retry_elapsed = crate::task::scheduler::get_ticks().saturating_sub(retry_start);
            if retry_elapsed > TLB_SHOOTDOWN_TIMEOUT_TICKS {
                break;
            }
            core::hint::spin_loop();
        }
        acks = TLB_SHOOTDOWN_ACKS.load(Ordering::SeqCst);
        elapsed = crate::task::scheduler::get_ticks().saturating_sub(start_ticks);
    }
    TLB_SHOOTDOWN_LAST_ACKS.store(acks, Ordering::SeqCst);
    TLB_SHOOTDOWN_LAST_DURATION.store(elapsed, Ordering::SeqCst);
}

pub fn notify_tlb_shootdown_ack() {
    TLB_SHOOTDOWN_ACKS.fetch_add(1, Ordering::SeqCst);
}

#[derive(Clone, Copy, Debug)]
pub struct TlbShootdownStats {
    pub requests: usize,
    pub timeouts: usize,
    pub watchdogs: usize,
    pub last_targets: usize,
    pub last_acks: usize,
    pub last_duration_ticks: usize,
}

pub fn tlb_shootdown_stats() -> TlbShootdownStats {
    TlbShootdownStats {
        requests: TLB_SHOOTDOWN_REQUESTS.load(Ordering::SeqCst),
        timeouts: TLB_SHOOTDOWN_TIMEOUTS.load(Ordering::SeqCst),
        watchdogs: TLB_SHOOTDOWN_WATCHDOGS.load(Ordering::SeqCst),
        last_targets: TLB_SHOOTDOWN_LAST_TARGETS.load(Ordering::SeqCst),
        last_acks: TLB_SHOOTDOWN_LAST_ACKS.load(Ordering::SeqCst),
        last_duration_ticks: TLB_SHOOTDOWN_LAST_DURATION.load(Ordering::SeqCst),
    }
}

/// AP (Application Processor) başlat
pub unsafe fn startup_ap(apic_id: u32, cpu_id: u32) -> bool {
    crate::serial_println!("Starting AP {} with APIC ID {}", cpu_id, apic_id);
    let (stack_top, cpu_data) = {
        let state = SMP_STATE.lock();
        
        crate::serial_println!("SMP: per_cpu_data.len() = {}", state.per_cpu_data.len());
        crate::serial_println!("SMP: Looking for cpu_id = {}", cpu_id);
        
        let s = state
            .per_cpu_data
            .get(cpu_id as usize)
            .map(|data| {
                crate::serial_println!("SMP: Found per_cpu_data for cpu_id {}: stack_top = {:#x}", cpu_id, data.stack_top);
                data.stack_top
            })
            .unwrap_or(0);
        let c = state
            .syscall_cpu_data
            .get(cpu_id as usize)
            .map(|data| *data as *const _ as u64)
            .unwrap_or(0);
            
        crate::serial_println!("SMP DEBUG: Entire syscall_cpu_data vector content:");
        crate::serial_println!("SMP DEBUG: Vector buffer address: {:#x}", state.syscall_cpu_data.as_ptr() as u64);
        for (i, d) in state.syscall_cpu_data.iter().enumerate() {
            crate::serial_println!("  [{}] = {:#x}", i, *d as *const _ as u64);
        }
        
        (s, c)
    };
    
    if stack_top == 0 || cpu_data == 0 {
        crate::serial_println!("SMP: ERROR: Invalid AP startup data for cpu_id {}", cpu_id);
        CPU_STATES.set_state(cpu_id, CpuHotplugState::Broken);
        return false;
    }
    
    // CPU başlatılıyor olarak işaretle
    CPU_STATES.set_state(cpu_id, CpuHotplugState::Bringup);
    
    let target_online = SMP_STATE
        .lock()
        .online_cpus
        .load(Ordering::Acquire)
        .saturating_add(1);
    prepare_ap_startup_data(stack_top, cpu_data);

    // Yeniden deneme seçeneğiyle INIT-SIPI-SIPI dizisi gönder
    const MAX_RETRIES: u32 = 3;

    for attempt in 0..MAX_RETRIES {
        if attempt > 0 {
            crate::serial_println!("SMP: Retry attempt {} for AP {}", attempt, cpu_id);
        }

        // INIT sinyali gönder (assert)
        send_ipi(
            apic_id,
            APIC_DELIVERY_INIT | APIC_LEVEL_ASSERT | APIC_TRIGGER_LEVEL,
            0,
        );
        crate::serial_println!("SMP: INIT assert sent to AP {}", cpu_id);
        delay_ms(10);

        // INIT sinyali geri al (deassert)
        send_ipi(apic_id, APIC_DELIVERY_INIT | APIC_TRIGGER_LEVEL, 0);
        crate::serial_println!("SMP: INIT deassert sent to AP {}", cpu_id);
        delay_ms(1);

        // Birinci SIPI gönder
        send_ipi(apic_id, APIC_DELIVERY_STARTUP, AP_STARTUP_SEGMENT as u32);
        crate::serial_println!("SMP: SIPI 1 sent to AP {}", cpu_id);
        delay_us(200);

        // İkinci SIPI gönder (Intel spec. gereksinimi — yedek)
        send_ipi(apic_id, APIC_DELIVERY_STARTUP, AP_STARTUP_SEGMENT as u32);
        crate::serial_println!("SMP: SIPI 2 sent to AP {}", cpu_id);
        delay_us(200);

        // AP'nin online olmasını bekle
        if wait_for_online(target_online) {
            crate::serial_println!("SMP: AP {} successfully started on attempt {}", cpu_id, attempt + 1);
            // State zaten mark_cpu_online'da ONLINE olarak ayarlanacak
            return true;
        }
        
        crate::serial_println!("SMP: AP {} did not respond on attempt {}", cpu_id, attempt + 1);
    }
    
    crate::serial_println!("SMP: WARNING: Failed to start AP {} after {} attempts", cpu_id, MAX_RETRIES);
    CPU_STATES.set_state(cpu_id, CpuHotplugState::Broken);
    false
}

/// AP başladı mı kontrol et (timeout ile)
fn wait_for_online(target_online: u32) -> bool {
    // Zaman aşımı önemli ölçüde artırıldı. AP önyüklemesi: bellek tahsisi,
    // konsol çıktısı ve GDT/IDT kurulumu gibi işlemler yapar.
    const MAX_SPIN_ITERATIONS: u32 = 500;
    
    x86_64::instructions::interrupts::without_interrupts(|| {
        let online_ptr = {
            let state = SMP_STATE.lock();
            &state.online_cpus as *const AtomicU32
        };
        
        for i in 0..MAX_SPIN_ITERATIONS {
            let current = unsafe { (*online_ptr).load(Ordering::Acquire) };
            if current >= target_online {
                crate::serial_println!("SMP: AP came online after {} iterations", i);
                return true;
            }
            
            unsafe { delay_us(100); }
        }
        
        crate::serial_println!("SMP: Timeout waiting for AP after {} iterations", MAX_SPIN_ITERATIONS);
        false
    })
}

pub fn mark_cpu_online(cpu_id: u32, apic_id: u32) {
    unsafe { x86_64::instructions::port::Port::<u8>::new(0x3f8).write(b'X'); }
    if cpu_id < SMP_STATE.lock().ap_started.len() as u32 {
        unsafe { x86_64::instructions::port::Port::<u8>::new(0x3f8).write(b'Y'); }
        {
            let mut state = SMP_STATE.lock();
            unsafe { x86_64::instructions::port::Port::<u8>::new(0x3f8).write(b'Z'); }
            if let Some(per_cpu) = state.per_cpu_data.get_mut(cpu_id as usize) {
                per_cpu.cpu_id = cpu_id;
                per_cpu.apic_id = apic_id;
                per_cpu.online = true;
            }
            state.ap_started[cpu_id as usize].store(true, Ordering::Release);
            state.online_cpus.fetch_add(1, Ordering::AcqRel);
        }
        // CPU state machine'i ONLINE olarak güncelle
        CPU_STATES.set_state(cpu_id, CpuHotplugState::Online);
        crate::serial_println!("AP {} started successfully (state: ONLINE)", cpu_id);
    }
}

/// Tüm AP'leri başlat
pub fn startup_all_aps() {
    // BSP per-cpu kurulumu — AP başlatma döngüsünden farklı bir yığın çerçevesinde
    // yığın geçişini garantilemek için ayrı fonksiyon çağrılır
    initialize_bsp_per_cpu();

    // KRİTİK: Yığın geçişinden sonra cpu_count'u SMP_STATE'ten TEKRAR okumalıyız
    // çünkü yığın geçişi önceki tüm yerel değişkenleri geçersiz kılar
    prepare_ap_per_cpu_data();

    // Zamanlayıcıyı başlat
    let cpu_count_for_scheduler = SMP_STATE.lock().cpu_count;
    crate::task::scheduler::update_cpu_count(cpu_count_for_scheduler);

    // Tek CPU için erken dönüş
    if cpu_count_for_scheduler <= 1 {
        crate::serial_println!("SMP: startup_all_aps cpu_count={}", cpu_count_for_scheduler);
        crate::serial_println!("SMP: {}/{} CPUs online", 
            SMP_STATE.lock().online_cpus.load(Ordering::Acquire),
            cpu_count_for_scheduler);
        return;
    }

    // AP başlangıç kodunu yükle
    crate::serial_println!("SMP: startup_all_aps cpu_count={}", cpu_count_for_scheduler);
    crate::serial_println!("SMP: loading AP startup code");
    unsafe {
        load_ap_startup_code();
    }
    crate::serial_println!("SMP: AP startup code ready");

    // Yapılandırmaya bağlı olarak paralel veya sıralı AP başlatma
    let mut successful_aps = 0;
    let mut failed_aps = 0;

    if CPU_STATES.is_parallel_bringup() && cpu_count_for_scheduler > 2 {
        // PARALEL BAŞLATMA: Birden fazla AP'yi aynı anda başlat
        crate::serial_println!("SMP: Using PARALLEL bringup mode");
        successful_aps = parallel_startup_aps(cpu_count_for_scheduler);
    } else {
        // SIRASAL BAŞLATMA: AP'leri tek tek başlat (geleneksel yöntem)
        crate::serial_println!("SMP: Using SEQUENTIAL bringup mode");
        for cpu_id in 1..cpu_count_for_scheduler {
            let apic_id = SMP_STATE
                .lock()
                .cpu_apic_ids
                .get(cpu_id as usize)
                .copied()
                .unwrap_or(cpu_id);
            crate::serial_println!("SMP: starting AP {} (apic_id={})", cpu_id, apic_id);
            unsafe {
                if startup_ap(apic_id, cpu_id) {
                    crate::serial_println!("AP {} started successfully", cpu_id);
                    successful_aps += 1;
                } else {
                    crate::serial_println!("WARNING: Failed to start AP {} - continuing with remaining CPUs", cpu_id);
                    failed_aps += 1;
                }
            }
        }
    }

    let final_online = SMP_STATE.lock().online_cpus.load(Ordering::Acquire);
    crate::serial_println!(
        "SMP: Startup complete - {}/{} CPUs online ({} successful, {} failed)",
        final_online,
        cpu_count_for_scheduler,
        successful_aps,
        failed_aps
    );
    
    if failed_aps > 0 {
        crate::serial_println!("SMP: System will continue with {} CPU(s)", final_online);
    }
    
    // BSP YIĞIN GEÇİŞİ KALDIRILDI: Yığın işaretçisini burada üzerine yazmak
    // kernel_main'e dönüş adresini yok eder! BSP, UEFI tarafından sağlanan
    // yığını kullanmaya devam eder.
}

/// Parallel AP startup - birden fazla AP'yi aynı anda başlat
/// Linux kernel parallel bringup benzeri implementasyon
fn parallel_startup_aps(cpu_count: u32) -> u32 {
    use core::sync::atomic::AtomicU32;
    
    // Batch boyutu: aynı anda kaç AP başlatılacak
    // Intel, en fazla 4 paralel SIPI gönderilmesini önerir
    const BATCH_SIZE: u32 = 4;
    
    let mut successful = 0u32;
    let total_aps = cpu_count - 1; // BSP hariç
    
    // AP'leri batch'ler halinde başlat
    for batch_start in (1..cpu_count).step_by(BATCH_SIZE as usize) {
        let batch_end = (batch_start + BATCH_SIZE).min(cpu_count);
        let batch_size = batch_end - batch_start;
        
        crate::serial_println!("SMP: Parallel batch {}-{} ({} APs)", 
            batch_start, batch_end - 1, batch_size);
        
        // 1. Tüm AP'lere INIT gönder (broadcast)
        for cpu_id in batch_start..batch_end {
            let apic_id = SMP_STATE
                .lock()
                .cpu_apic_ids
                .get(cpu_id as usize)
                .copied()
                .unwrap_or(cpu_id);
            
            // State'i BRINGUP olarak işaretle
            CPU_STATES.set_state(cpu_id, CpuHotplugState::Bringup);
            
            // INIT gönder
            unsafe {
                send_ipi(apic_id, APIC_DELIVERY_INIT | APIC_LEVEL_ASSERT | APIC_TRIGGER_LEVEL, 0);
            }
        }
        
        // INIT deassert bekle
        unsafe { delay_ms(10); }
        
        // 2. Tüm AP'lere INIT deassert
        for cpu_id in batch_start..batch_end {
            let apic_id = SMP_STATE
                .lock()
                .cpu_apic_ids
                .get(cpu_id as usize)
                .copied()
                .unwrap_or(cpu_id);
            unsafe {
                send_ipi(apic_id, APIC_DELIVERY_INIT | APIC_TRIGGER_LEVEL, 0);
            }
        }
        
        unsafe { delay_ms(1); }
        
        // 3. Tüm AP'lere SIPI gönder (broadcast)
        for cpu_id in batch_start..batch_end {
            let apic_id = SMP_STATE
                .lock()
                .cpu_apic_ids
                .get(cpu_id as usize)
                .copied()
                .unwrap_or(cpu_id);
            unsafe {
                send_ipi(apic_id, APIC_DELIVERY_STARTUP, AP_STARTUP_SEGMENT as u32);
            }
        }
        
        // AP'lerin başlamasını bekle
        unsafe { delay_us(200); }
        
        // 4. İkinci SIPI (Intel spec)
        for cpu_id in batch_start..batch_end {
            let apic_id = SMP_STATE
                .lock()
                .cpu_apic_ids
                .get(cpu_id as usize)
                .copied()
                .unwrap_or(cpu_id);
            unsafe {
                send_ipi(apic_id, APIC_DELIVERY_STARTUP, AP_STARTUP_SEGMENT as u32);
            }
        }
        
        // 5. Tüm AP'lerin online olmasını bekle
        let target_online = SMP_STATE.lock().online_cpus.load(Ordering::Acquire) + batch_size;
        let batch_success = wait_for_batch_online(target_online, batch_size);
        
        successful += batch_success;
    }
    
    successful
}

/// Bir batch AP'nin online olmasını bekle
fn wait_for_batch_online(target_online: u32, batch_size: u32) -> u32 {
    const MAX_WAIT_MS: u32 = 500;  // 500ms timeout
    let mut elapsed = 0u32;
    
    loop {
        let current = SMP_STATE.lock().online_cpus.load(Ordering::Acquire);
        if current >= target_online {
            return batch_size;
        }
        
        unsafe { delay_ms(1); }
        elapsed += 1;
        
        if elapsed > MAX_WAIT_MS {
            // Timeout - kaç AP başarılı oldu?
            let current = SMP_STATE.lock().online_cpus.load(Ordering::Acquire);
            let started = current.saturating_sub(target_online - batch_size);
            crate::serial_println!("SMP: Parallel batch timeout, {}/{} APs online", started, batch_size);
            return started;
        }
    }
}

/// Fiziksel bellekten yığın (stack) tahsis et — heap bozulmasını önlemek için TLSF'yi atlar.
/// Çerçeve tahsisi için global bellek yöneticisini kullanır; yığın HHDM ile eşlenir.
unsafe fn allocate_stack_phys() -> Option<(&'static mut [u8], u64)> {
    let stack_size = crate::syscall::SYSCALL_STACK_SIZE;
    let frame_size = 4096u64;
    let frames_needed = ((stack_size as u64 + frame_size - 1) / frame_size) as usize;

    crate::serial_println!("SMP: Allocating {} bytes ({} frames) from physical memory", stack_size, frames_needed);

    // Global bellek yöneticisi ile ardışık fiziksel çerçeveler tahsis et
    let mm = crate::memory::global_memory_manager_mut()?;
    let start_frame = mm.allocate_contiguous_frames(frames_needed)?;
    let phys_start = start_frame.start_address().as_u64();

    crate::serial_println!("SMP: Allocated contiguous physical memory at {:#x}", phys_start);

    // HHDM üzerinden sanal adrese eşle
    let hhdm = crate::memory::active_physical_offset();
    let virt_start = phys_start + hhdm;

    // Belleği sıfırla (güvensiz başlangıç değerlerini temizle)
    core::ptr::write_bytes(virt_start as *mut u8, 0, stack_size);

    let stack_ptr = virt_start as *mut u8;
    let stack_slice = core::slice::from_raw_parts_mut(stack_ptr, stack_size);

    // Hizalanmış yığın tepesini hesapla (16 bayt hizalama — ABI gereksinimi)
    let mut stack_top = virt_start + stack_size as u64;
    stack_top &= !0xFu64;

    crate::serial_println!("SMP: Stack allocated at virt={:#x}, top={:#x}", virt_start, stack_top);

    Some((stack_slice, stack_top))
}

/// Fiziksel bellekten küçük bir yapı tahsis et (TLSF'yi atlar).
/// Tahsis edilen belleğe kalıcı değiştirilebilir referans döndürür.
unsafe fn allocate_struct_phys<T>() -> Option<&'static mut T> {
    let align = core::mem::align_of::<T>() as u64;

    // Bir çerçeve (4096 bayt) tahsis et — herhangi bir yapı için fazlasıyla yeterli
    let mm = crate::memory::global_memory_manager_mut()?;
    let frame = mm.allocate_contiguous_frames(1)?;
    let phys = frame.start_address().as_u64();

    // HHDM üzerinden sanal adrese eşle
    let hhdm = crate::memory::active_physical_offset();
    let virt = phys + hhdm;

    // Belleği sıfırla
    core::ptr::write_bytes(virt as *mut u8, 0, 4096);

    // Hizalamayı garantile (T türünün hizalama gereksinimini karşıla)
    let aligned_virt = (virt + (align - 1)) & !(align - 1);
    
    Some(&mut *(aligned_virt as *mut T))
}

/// Tüm AP'ler için per-CPU veri yapılarını hazırla.
/// TLSF heap bozulmasını önlemek için TÜM tahsislerde fiziksel bellek kullanılır.
fn prepare_ap_per_cpu_data() {
    crate::serial_println!("SMP: About to read cpu_count");
    let cpu_count = SMP_STATE.lock().cpu_count;
    crate::serial_println!("SMP: cpu_count = {}, preparing AP per-cpu data", cpu_count);
    
    for cpu_id in 1..cpu_count {
        crate::serial_println!("SMP: Creating per_cpu_data for cpu_id {}", cpu_id);
        let apic_id = SMP_STATE.lock()
            .cpu_apic_ids
            .get(cpu_id as usize)
            .copied()
            .unwrap_or(cpu_id);
        
        // Fiziksel bellekten yığın tahsis et (TLSF'yi atla)
        let (stack, stack_top) = unsafe {
            match allocate_stack_phys() {
                Some(s) => s,
                None => {
                    crate::serial_println!("SMP: ERROR: Failed to allocate stack for cpu_id {}", cpu_id);
                    continue;
                }
            }
        };

        crate::serial_println!("SMP: cpu_id {} stack_top = {:#x}", cpu_id, stack_top);

        // Fiziksel bellekten CpuData tahsis et (TLSF'yi atla)
        let cpu_data = unsafe {
            match allocate_struct_phys::<crate::syscall::CpuData>() {
                Some(d) => {
                    d.user_rsp_scratch = 0;
                    d.kernel_stack_top = stack_top;
                    d.cpu_id = cpu_id;
                    d.user_rip = 0;
                    d.user_rflags = 0;
                    d.irq_depth = 0;
                    d
                }
                None => {
                    crate::serial_println!("SMP: ERROR: Failed to allocate CpuData for cpu_id {}", cpu_id);
                    continue;
                }
            }
        };
        
        crate::serial_println!("SMP: cpu_id {} allocated CpuData at {:#x}", cpu_id, cpu_data as *const _ as u64);
        
        // Fiziksel bellekten PerCpuData tahsis et (TLSF'yi atla)
        let per_cpu = unsafe {
            match allocate_struct_phys::<PerCpuData>() {
                Some(d) => {
                    d.cpu_id = cpu_id;
                    d.apic_id = apic_id;
                    d.task_count = 0;
                    d.load = 0;
                    d.online = false;
                    d.stack_top = stack_top;
                    d.dma_domain = 0;
                    d._padding = [0; 64 - 36];
                    d
                }
                None => {
                    crate::serial_println!("SMP: ERROR: Failed to allocate PerCpuData for cpu_id {}", cpu_id);
                    continue;
                }
            }
        };
        
        let mut state = SMP_STATE.lock();
        state.per_cpu_data.push(per_cpu);
        state.ap_started.push(AtomicBool::new(false));
        state.syscall_cpu_data.push(cpu_data);
        state.syscall_stacks.push(stack);
        crate::serial_println!("SMP: cpu_id {} added to per_cpu_data (len={})", cpu_id, state.per_cpu_data.len());
    }
    crate::serial_println!("SMP: AP per-cpu data preparation complete");
}
/// BSP (Önyükleme İşlemcisi / Bootstrap Processor) için per-CPU veri yapılarını başlat.
/// TLSF heap bozulmasını önlemek için TÜM tahsislerde fiziksel bellek kullanılır.
fn initialize_bsp_per_cpu() {
    crate::serial_println!("SMP: BSP per-cpu setup begin");
    let need_setup = SMP_STATE.lock().per_cpu_data.is_empty();

    if need_setup {
        let bsp_apic_id = SMP_STATE.lock().cpu_apic_ids.get(0).copied().unwrap_or(0);

        // BSP için fiziksel bellekten PerCpuData tahsis et (TLSF'yi atla)
        let per_cpu = unsafe {
            match allocate_struct_phys::<PerCpuData>() {
                Some(d) => {
                    d.cpu_id = 0;
                    d.apic_id = bsp_apic_id;
                    d.task_count = 0;
                    d.load = 0;
                    d.online = true;
                    d.stack_top = 0;
                    d.dma_domain = 0;
                    d._padding = [0; 64 - 36];
                    d
                }
                None => {
                    crate::serial_println!("SMP: FATAL: Failed to allocate BSP PerCpuData");
                    return;
                }
            }
        };

        // BSP için fiziksel bellekten yığın tahsis et (TLSF'yi atla)
        let (stack, stack_top) = unsafe {
            match allocate_stack_phys() {
                Some(s) => s,
                None => {
                    crate::serial_println!("SMP: FATAL: Failed to allocate BSP stack");
                    return;
                }
            }
        };

        // BSP için fiziksel bellekten CpuData tahsis et (TLSF'yi atla)
        let cpu_data = unsafe {
            match allocate_struct_phys::<crate::syscall::CpuData>() {
                Some(d) => {
                    d.user_rsp_scratch = 0;
                    d.kernel_stack_top = stack_top;
                    d.cpu_id = 0;
                    d.user_rip = 0;
                    d.user_rflags = 0;
                    d.irq_depth = 0;
                    d
                }
                None => {
                    crate::serial_println!("SMP: FATAL: Failed to allocate BSP CpuData");
                    return;
                }
            }
        };

        // Pointer'ları kaydet
        let cpu_data_ptr = cpu_data as *mut crate::syscall::CpuData;
        
        // SMP state'e ekle
        {
            let mut state = SMP_STATE.lock();
            state.per_cpu_data.push(per_cpu);
            state.ap_started.push(AtomicBool::new(true));
            state.syscall_cpu_data.push(cpu_data);
            state.syscall_stacks.push(stack);
        }

        // Sistem çağrısı (syscall) CPU verisini başlat
        unsafe {
            crate::syscall::init_cpu_data(cpu_data_ptr);
        }
        
        // BSP stack_top'u kaydet - stack switch için lazım olacak
        SMP_STATE.lock().per_cpu_data[0].stack_top = stack_top;
    }
    crate::serial_println!("SMP: BSP per-cpu setup done");
}


pub fn update_cpu_load(cpu_id: u32, load: u32) {
    // Lock'sız erişim için atomic kullanmak daha iyi olurdu ama
    // şimdilik basitçe lock alıp güncelliyoruz.
    // DEADLOCK ÖNLEME: Interrupt handler içinden çağrıldığı için
    // try_lock kullanmalıyız. Eğer lock alınamazsa güncellemeyi atla.
    if let Some(mut state) = SMP_STATE.try_lock() {
        if let Some(per_cpu) = state.per_cpu_data.get_mut(cpu_id as usize) {
            per_cpu.load = load;
            per_cpu.task_count = load; // task_count ile load aynı şey şimdilik
        }
    }
}

/// CFS-style Load Balancer
/// Linux CFS (Completely Fair Scheduler) load balancing'den esinlenilmiş
/// 
/// Özellikler:
/// - Active balancing (IPI ile task migration)
/// - Load average tracking
/// - NUMA awareness (hazır ama aktif değil)
/// - CPU affinity consideration
/// - Isolation respect
pub fn balance_load() {
    let state = SMP_STATE.lock();

    let online_count = state.online_cpus.load(Ordering::Acquire);
    if online_count < 2 {
        return; // SMP yok
    }

    // Yük istatistiklerini topla
    let mut total_load: u32 = 0;
    let mut max_load: u32 = 0;
    let mut min_load: u32 = u32::MAX;
    let mut overloaded_cpu = 0;
    let mut underloaded_cpu = 0;
    let mut loads: [(u32, u32); 256] = [(0, 0); 256]; // (cpu_id, load)
    let mut load_count = 0;

    crate::serial_println!("--- SMP CFS Load Balance Report ---");
    for cpu in state.per_cpu_data.iter() {
        if !cpu.online {
            continue;
        }
        let load = cpu.load;
        total_load += load;
        
        if load > max_load {
            max_load = load;
            overloaded_cpu = cpu.cpu_id;
        }
        if load < min_load {
            min_load = load;
            underloaded_cpu = cpu.cpu_id;
        }
        
        loads[load_count] = (cpu.cpu_id, load);
        load_count += 1;

        crate::serial_println!("CPU {}: Load {} tasks, Online: {}, Isolated: {}", 
            cpu.cpu_id, load, cpu.online, CPU_STATES.is_isolated(cpu.cpu_id));
    }

    let avg_load = if online_count > 0 { total_load / online_count } else { 0 };
    crate::serial_println!("Total: {}, Avg: {}, Max: {} (CPU {}), Min: {} (CPU {})", 
        total_load, avg_load, max_load, overloaded_cpu, min_load, underloaded_cpu);
    
    // ACTIVE BALANCING: Eğer load dengesizliği varsa
    let imbalance_threshold = (avg_load as f32 * 0.25) as u32; // %25 tolerance
    if max_load > avg_load + imbalance_threshold && min_load < avg_load {
        crate::serial_println!("CFS: Active balancing triggered - imbalance detected");
        
        // İzole CPU'ları atla
        if !CPU_STATES.is_isolated(overloaded_cpu) && !CPU_STATES.is_isolated(underloaded_cpu) {
            // Task migration önerisi (gerçek migration scheduler'da yapılacak)
            let tasks_to_migrate = (max_load - avg_load) / 2;
            crate::serial_println!("CFS: Suggesting migration of {} tasks from CPU {} to CPU {}", 
                tasks_to_migrate, overloaded_cpu, underloaded_cpu);
            
            // IPI ile active balancing (scheduler'a bildir)
            // Bu gerçek implementation'da scheduler'ın runqueue'larını değiştirecek
            trigger_active_balance(overloaded_cpu, underloaded_cpu, tasks_to_migrate);
        }
    }
    
    // Load average güncelle (exponential moving average)
    update_load_average(loads, load_count);
}

/// Active balancing tetikle (IPI ile)
fn trigger_active_balance(from_cpu: u32, to_cpu: u32, tasks: u32) {
    // Gerçek implementation'da:
    // 1. from_cpu'ya IPI gönder
    // 2. from_cpu scheduler'dan task'ları al
    // 3. to_cpu'ya push et
    
    // Şimdilik sadece log
    crate::serial_println!("CFS: Active balance - would migrate {} tasks from CPU {} to {}", 
        tasks, from_cpu, to_cpu);
}

/// Load average güncelle (exponential moving average)
fn update_load_average(loads: [(u32, u32); 256], count: usize) {
    // Linux tarzı load average (1min, 5min, 15min)
    // Şimdilik basit tracking
    static LOAD_HISTORY: Mutex<[(u64, u32); 256]> = Mutex::new([(0, 0); 256]);
    
    let mut history = LOAD_HISTORY.lock();
    for i in 0..count {
        let (cpu_id, load) = loads[i];
        let idx = cpu_id as usize;
        if idx < 256 {
            // Üstel hareketli ortalama: yeni = 0.9 × eski + 0.1 × mevcut
            // (Linux 1/5/15 dk. load average hesabının basitleştirilmiş versiyonu)
            let old = history[idx].1 as f32;
            let new = 0.9 * old + 0.1 * load as f32;
            history[idx] = (history[idx].0 + 1, new as u32);
        }
    }
}

/// CFS-style Load Balancer
pub fn find_least_loaded_cpu(affinity: &CpuAffinity) -> Option<u32> {
    let state = SMP_STATE.lock();
    let mut min_load = u32::MAX;
    let mut best_cpu = None;
    
    for cpu in state.per_cpu_data.iter() {
        if !cpu.online || CPU_STATES.is_isolated(cpu.cpu_id) {
            continue;
        }
        
        // Affinity kontrolü
        if !affinity.can_run_on(cpu.cpu_id) {
            continue;
        }
        
        if cpu.load < min_load {
            min_load = cpu.load;
            best_cpu = Some(cpu.cpu_id);
        }
    }
    
    best_cpu
}

/// NUMA-aware task placement (hazır ama aktif değil)
pub fn find_numa_aware_cpu(_numa_node: u32, affinity: &CpuAffinity) -> Option<u32> {
    // NUMA desteği eklendiğinde kullanılacak
    find_least_loaded_cpu(affinity)
}

/// Ticket Lock implementasyonu (Linux'tan esinlenme)
pub struct TicketLock {
    next_ticket: AtomicU32,
    current_ticket: AtomicU32,
}

impl TicketLock {
    pub const fn new() -> Self {
        Self {
            next_ticket: AtomicU32::new(0),
            current_ticket: AtomicU32::new(0),
        }
    }

    pub fn lock(&self) {
        let ticket = self.next_ticket.fetch_add(1, Ordering::Relaxed);

        while self.current_ticket.load(Ordering::Acquire) != ticket {
            core::hint::spin_loop();
        }
    }

    pub fn unlock(&self) {
        let current = self.current_ticket.load(Ordering::Relaxed);
        self.current_ticket.store(current + 1, Ordering::Release);
    }
}

/// Basit RCU (Read-Copy-Update) implementasyonu
pub struct SimpleRcu {
    grace_counter: AtomicU32,
    reader_count: AtomicU32,
}

impl SimpleRcu {
    pub const fn new() -> Self {
        Self {
            grace_counter: AtomicU32::new(0),
            reader_count: AtomicU32::new(0),
        }
    }

    pub fn read_lock(&self) -> u32 {
        self.reader_count.fetch_add(1, Ordering::Acquire);
        self.grace_counter.load(Ordering::Relaxed)
    }

    pub fn read_unlock(&self) {
        self.reader_count.fetch_sub(1, Ordering::Release);
    }

    pub fn synchronize(&self) {
        let start_counter = self.grace_counter.load(Ordering::Relaxed);
        self.grace_counter.fetch_add(1, Ordering::Release);

        // Tüm reader'ların bitmesini bekle
        while self.reader_count.load(Ordering::Acquire) > 0 {
            core::hint::spin_loop();
        }

        // Hoşgörü periyodunu bekle (tüm okuyucular bitmeden yazıcı devam edemez)
        while self.grace_counter.load(Ordering::Acquire) == start_counter {
            core::hint::spin_loop();
        }
    }
}

/// Milisaniye cinsinden döngü tabanlı gecikme
pub fn delay_ms(ms: u32) {
    // QEMU TCG (yazılım öykünücüsü) için gecikme döngüsü önemli ölçüde artırıldı
    for _ in 0..ms * 100000 {
        core::hint::spin_loop();
        unsafe {
            core::arch::asm!("nop");
        }
    }
}

pub fn delay_us(us: u32) {
    for _ in 0..us * 100 {
        core::hint::spin_loop();
        unsafe {
            core::arch::asm!("nop");
        }
    }
}

/// Belirtilen CPU'yu başlatır
pub fn start_cpu(cpu_id: u32) -> Result<(), &'static str> {
    // CPU başlatma mantığı
    // Şimdilik basit bir implementasyon:
    if cpu_id >= get_cpu_count() {
        return Err("Invalid CPU ID");
    }
    
    // TODO: Gerçek CPU başlatma kodu buraya eklenecek
    // Örnek: APIC kullanarak CPU'yu uyandırma
    
    Ok(())
}

/// Belirtilen CPU'yu durdurur
pub fn stop_cpu(cpu_id: u32) -> Result<(), &'static str> {
    // CPU durdurma mantığı
    // Şimdilik basit bir implementasyon:
    if cpu_id >= get_cpu_count() {
        return Err("Invalid CPU ID");
    }
    
    if cpu_id == 0 {
        return Err("Cannot stop boot CPU");
    }
    
    // TODO: Gerçek CPU durdurma kodu buraya eklenecek
    // Örnek: APIC kullanarak CPU'yu uyutma
    
    Ok(())
}

/// Sistemdeki toplam CPU sayısını döndürür
pub fn get_cpu_count() -> u32 {
    SMP_STATE.lock().cpu_count
}

/// SMP başlatma
pub fn init() {
    crate::serial_println!("Initializing SMP...");

    // BSP'yi online olarak işaretle
    CPU_STATES.init_bsp();
    
    // ACPI'den CPU bilgilerini al
    let cpu_count_from_acpi = if let Some(acpi_info) = crate::cpu::acpi::get_cpu_info() {
        let mut state = SMP_STATE.lock();
        state.apic_base = acpi_info.apic_base;
        
        // DEBUG: ACPI'den gelen bilgileri logla
        crate::serial_println!("SMP DEBUG: bsp_apic_id = {}", acpi_info.bsp_apic_id);
        crate::serial_println!("SMP DEBUG: cpu_list = {:?}", acpi_info.cpu_list);
        
        // APIC ID'leri doğru sırala: BSP ilk, sonra AP'ler
        // ACPI cpu_list zaten tüm CPU'ları içerir, BSP'yi çıkar ve ayrı ekle
        let cpu_count = acpi_info.cpu_list.len();
        
        // CPU state machine'i güncelle
        CPU_STATES.set_cpu_count(cpu_count as u32);
        
        // Yeniden boyutlandırmayı önlemek için vektörleri tam kapasiteyle önceden tahsis et
        state.per_cpu_data = Vec::with_capacity(cpu_count);
        state.syscall_cpu_data = Vec::with_capacity(cpu_count);
        state.syscall_stacks = Vec::with_capacity(cpu_count);
        state.ap_started = Vec::with_capacity(cpu_count);

        // Vec işlemlerinden kaçınmak için APIC ID'lerini sabit boyutlu dizide sakla
        for (i, &id) in acpi_info.cpu_list.iter().enumerate() {
            if i < 16 {
                state.cpu_apic_ids.push(id);
            }
        }

        // Algılanan tüm CPU'ları etkinleştir (Linux 8192 CPU'ya kadar destekler)
        state.cpu_count = cpu_count as u32;
        
        crate::serial_println!("SMP: Found {} CPUs via ACPI, activating all", state.cpu_count);
        cpu_count
    } else {
        crate::serial_println!("SMP: Using CPUID detection");
        // CPUID'den CPU sayısını tahmin et
        let mut state = SMP_STATE.lock();
        state.cpu_count = 1;  // Single CPU
        state.cpu_apic_ids = vec![0];
        1
    };

    let cpu_count = SMP_STATE.lock().cpu_count;
    crate::random::init_per_cpu_entropy(cpu_count);
    
    if crate::apic::lapic::init().is_err() {
        crate::serial_println!("SMP: LAPIC init failed");
    }

    startup_all_aps();
    if SMP_STATE.lock().cpu_count == 1 {
        crate::serial_println!("SMP: Single processor system");
    }
}


pub fn read_local_apic_id() -> u32 {
    if has_x2apic() {
        return unsafe { Msr::new(0x802).read() as u32 };
    }
    unsafe { read_apic_reg(0x20) >> 24 }
}

pub fn current_cpu_id() -> u32 {
    // SMP init olmadan önce çağrılabilir, bu durumda BSP (CPU 0) döndür
    let apic_id = unsafe { read_apic_reg(0x20) >> 24 };
    
    // SMP_STATE lock almayı dene, başarısız olursa BSP döndür
    let state = match SMP_STATE.try_lock() {
        Some(s) => s,
        None => return 0, // SMP init olmadı, BSP döndür
    };
    
    state
        .cpu_apic_ids
        .iter()
        .position(|id| *id == apic_id)
        .unwrap_or(0) as u32
}

pub fn current_dma_domain() -> u32 {
    let cpu_id = current_cpu_id() as usize;
    let state = SMP_STATE.lock();
    state
        .per_cpu_data
        .get(cpu_id)
        .map(|data| data.dma_domain)
        .unwrap_or(0)
}

pub fn set_current_dma_domain(domain_id: u32) {
    let cpu_id = current_cpu_id() as usize;
    let mut state = SMP_STATE.lock();
    if let Some(data) = state.per_cpu_data.get_mut(cpu_id) {
        data.dma_domain = domain_id;
    }
}
