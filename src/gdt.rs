//! # echOS Global Descriptor Table (GDT)
//!
//! x86_64 segment tanımlayıcıları ve TSS (Task State Segment) yapılandırması.
//! Kernel/User mode geçişi için segment selector'ları içerir.
//!
//! ## GDT Nedir?
//! x86_64 mimarisinde GDT, çekirdeğin bellek segmentlerini tanımladığı
//! bir tablodur. Modern 64-bit sistemlerde segmentasyon fiilen kullanılmaz
//! (düz bellek modeli), ancak GDT hâlâ zorunludur çünkü:
//! - Kernel/User mod ayrımını (ring 0 / ring 3) sağlar
//! - TSS (Task State Segment) üzerinden interrupt stack geçişini yönetir
//! - SYSCALL/SYSRET mekanizması için segment selector ihtiyacı vardır

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};
use spin::Mutex;
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};
use x86_64::structures::tss::TaskStateSegment;
use x86_64::VirtAddr;

/// Double Fault istisnası için IST (Interrupt Stack Table) dizin numarası.
/// Double fault kendi stack'inde oluşabilir, bu yüzden ayrı stack gerektirir.
pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;
/// Page Fault istisnası için IST dizin numarası
pub const PAGE_FAULT_IST_INDEX: u16 = 1;
/// General Protection Fault istisnası için IST dizin numarası
pub const GENERAL_PROTECTION_IST_INDEX: u16 = 2;

/// CPU başına düşen GDT yapısı.
/// SMP (Simetrik Çok İşlemcili) sistemlerde her CPU kendi GDT'sine sahiptir.
/// Bu sayede CPU'lar birbirinin stack ve segment ayarlarına müdahale edemez.
struct PerCpuGdt {
    gdt: GlobalDescriptorTable,
    selectors: Selectors,
    tss: *mut TaskStateSegment,
    ist_stacks: [Box<[u8; IST_STACK_SIZE]>; IST_STACK_COUNT],
}

/// PerCpuGdt Sync: global statik erişim güvenli çünkü her CPU kendi
/// dizinine erişir ve mutex koruması altındadır.
unsafe impl Sync for PerCpuGdt {}

/// Her IST stack'i için ayrılan boyut: 5 sayfa (20 KB)
const IST_STACK_SIZE: usize = 4096 * 5;
/// IST giriş sayısı — x86_64 en fazla 7 IST girişi destekler
const IST_STACK_COUNT: usize = 7;

/// CPU başına GDT işaretçilerinin global listesi.
/// Her eleman, ilgili CPU'nun PerCpuGdt yapısına ham pointer içerir.
static PER_CPU_GDTS: Mutex<Vec<usize>> = Mutex::new(Vec::new());
static S3_RESUME_BSP_GDT: AtomicUsize = AtomicUsize::new(0);

/// GDT segment selector'ları.
/// Her selector, GDT'deki ilgili tanımlayıcıya işaret eder.
/// Selector değeri = tablo dizini << 3 | TI | RPL formatındadır.
#[derive(Clone, Copy)]
pub struct Selectors {
    pub code_selector: SegmentSelector,
    pub data_selector: SegmentSelector,
    pub tss_selector: SegmentSelector,
    pub user_code_selector: SegmentSelector,
    pub user_data_selector: SegmentSelector,
}

/// Önyükleme CPU'su (BSP) için GDT'yi başlatır.
/// Segmentleri yükler ve TSS'yi aktif eder.
pub fn init() {
    let gdt = ensure_per_cpu_gdt(0, VirtAddr::new(0));
    unsafe {
        load_per_cpu_gdt(gdt);
    }
}

/// Reload the BSP descriptor tables after an S3 firmware wake.
///
/// The resume trampoline uses a small private GDT while switching back to long
/// mode. The next interrupt must see echOS' kernel code selector and a valid
/// TSS/IST set, so S3 uses a fresh BSP GDT image instead of reusing a descriptor
/// whose TSS entry may have been marked busy by the pre-suspend `ltr`.
pub fn reload_bsp_after_resume() {
    let mut gdt = S3_RESUME_BSP_GDT.swap(0, Ordering::AcqRel) as *mut PerCpuGdt;
    {
        let mut list = PER_CPU_GDTS.lock();
        if list.is_empty() {
            list.resize(1, 0);
        }
        if gdt.is_null() {
            gdt = list[0] as *mut PerCpuGdt;
        } else {
            list[0] = gdt as usize;
        }
    }
    if gdt.is_null() {
        return;
    }
    unsafe {
        load_per_cpu_gdt(gdt);
    }
}

/// Prepare a fresh BSP GDT/TSS image while the allocator is still fully live.
pub fn prepare_bsp_resume_gdt() {
    let gdt = allocate_per_cpu_gdt(VirtAddr::new(0));
    S3_RESUME_BSP_GDT.store(gdt as usize, Ordering::Release);
}

/// Belirtilen CPU için GDT'yi başlatır (AP — Application Processor).
/// SMP başlatma sırasında her ikincil CPU için çağrılır.
pub fn init_for_cpu(cpu_id: u32, stack_top: VirtAddr) {
    let gdt = ensure_per_cpu_gdt(cpu_id, stack_top);
    unsafe {
        load_per_cpu_gdt(gdt);
    }
}

pub fn prepare_for_cpu(cpu_id: u32, stack_top: VirtAddr) {
    let _ = ensure_per_cpu_gdt(cpu_id, stack_top);
}

/// Çekirdek yığını (kernel stack) tepesini TSS'e yazar.
/// Sistem çağrısı (SYSCALL) sırasında CPU bu değere geçer.
pub fn set_kernel_stack(stack_top: VirtAddr) {
    let cpu_id = crate::cpu::smp::current_cpu_id() as usize;
    let list = PER_CPU_GDTS.lock();
    if let Some(ptr) = list.get(cpu_id).copied() {
        if ptr != 0 {
            unsafe {
                let tss = (*(ptr as *mut PerCpuGdt)).tss;
                if !tss.is_null() {
                    (*tss).privilege_stack_table[0] = stack_top;
                }
            }
        }
    }
}

/// Mevcut CPU'nun TSS.RSP0 değerini okur (diagnostic amaçlı).
///
/// TSS.RSP0: Ring 3'ten Ring 0'a geçişte (interrupt/SYSCALL) CPU'nun
/// kullanacağı kernel stack tepesidir. Bu değer 0 veya geçersizse,
/// herhangi bir fault/interrupt triple fault'a yol açar.
pub fn current_tss_rsp0() -> u64 {
    let cpu_id = crate::cpu::smp::current_cpu_id() as usize;
    let list = PER_CPU_GDTS.lock();
    if let Some(ptr) = list.get(cpu_id).copied() {
        if ptr != 0 {
            unsafe {
                let tss = (*(ptr as *mut PerCpuGdt)).tss;
                if !tss.is_null() {
                    return (*tss).privilege_stack_table[0].as_u64();
                }
            }
        }
    }
    0
}

/// Mevcut CPU'nun aktif selector değerlerini döndürür.
pub fn current_selectors() -> Selectors {
    let cpu_id = crate::cpu::smp::current_cpu_id();
    let ptr = ensure_per_cpu_gdt(cpu_id, VirtAddr::new(0));
    unsafe { (*ptr).selectors }
}

/// Kullanıcı kod (ring 3) selector'ını döndürür.
pub fn user_code_selector() -> SegmentSelector {
    current_selectors().user_code_selector
}

/// Kullanıcı veri (ring 3) selector'ını döndürür.
pub fn user_data_selector() -> SegmentSelector {
    current_selectors().user_data_selector
}

/// Çekirdek kod (ring 0) selector'ını döndürür.
pub fn kernel_code_selector() -> SegmentSelector {
    current_selectors().code_selector
}

/// Mevcut CPU'da gerçek donanım register'larının beklenen değerlerle
/// eşleşip eşleşmediğini doğrular.
/// GDT yolsuzluğunu (corruption) tespit etmek için kullanılır.
pub fn verify_current() -> bool {
    use x86_64::instructions::segmentation::{Segment, CS, DS, ES, SS};
    let cpu_id = crate::cpu::smp::current_cpu_id();
    let ptr = ensure_per_cpu_gdt(cpu_id, VirtAddr::new(0));
    if ptr.is_null() {
        return false;
    }
    let gdt = unsafe { &*(ptr as *const PerCpuGdt) };
    let selectors = gdt.selectors;
    let cs = CS::get_reg();
    let ds = DS::get_reg();
    let es = ES::get_reg();
    let ss = SS::get_reg();
    cs == selectors.code_selector
        && ds == selectors.data_selector
        && es == selectors.data_selector
        && ss == selectors.data_selector
}

/// Belirtilen CPU için PerCpuGdt yapısını döndürür;
/// henüz yoksa oluşturur (lazy initialization).
///
/// GDT oluşturma akışı:
/// ```text
/// CPU ID var mı listede?
///   ├── Evet → Mevcut pointer'ı döndür
///   └── Hayır → Yeni GDT oluştur:
///         1. IST stack'lerini tahsis et (7 adet × 20 KB)
///         2. TSS oluştur; IST pointer'larını doldur
///         3. GDT'ye segmentleri ekle (kernel, data, TSS, user)
///         4. Listeye kaydet ve pointer döndür
/// ```
fn ensure_per_cpu_gdt(cpu_id: u32, stack_top: VirtAddr) -> *mut PerCpuGdt {
    let mut list = PER_CPU_GDTS.lock();
    let idx = cpu_id as usize;
    if list.len() <= idx {
        list.resize(idx + 1, 0);
    }
    if list[idx] != 0 {
        return list[idx] as *mut PerCpuGdt;
    }
    let ptr = allocate_per_cpu_gdt(stack_top) as usize;
    list[idx] = ptr;
    ptr as *mut PerCpuGdt
}

fn allocate_per_cpu_gdt(stack_top: VirtAddr) -> *mut PerCpuGdt {
    let ist_stacks: [Box<[u8; IST_STACK_SIZE]>; IST_STACK_COUNT] =
        core::array::from_fn(|_| Box::new([0u8; IST_STACK_SIZE]));
    let tss = Box::leak(Box::new(TaskStateSegment::new()));
    for i in 0..IST_STACK_COUNT {
        let top = VirtAddr::from_ptr(ist_stacks[i].as_ptr()).as_u64() + IST_STACK_SIZE as u64;
        tss.interrupt_stack_table[i] = VirtAddr::new(top);
    }
    if stack_top.as_u64() != 0 {
        tss.privilege_stack_table[0] = stack_top;
    }
    let mut gdt = GlobalDescriptorTable::new();
    let code_selector = gdt.add_entry(Descriptor::kernel_code_segment());
    let data_selector = gdt.add_entry(Descriptor::kernel_data_segment());
    let tss_ptr = tss as *mut TaskStateSegment;
    let tss_ref = unsafe { &*tss_ptr };
    let tss_selector = gdt.add_entry(Descriptor::tss_segment(tss_ref));
    let user_data_selector = gdt.add_entry(Descriptor::user_data_segment());
    let user_code_selector = gdt.add_entry(Descriptor::user_code_segment());
    let selectors = Selectors {
        code_selector,
        data_selector,
        tss_selector,
        user_code_selector,
        user_data_selector,
    };
    let per_cpu = Box::new(PerCpuGdt {
        gdt,
        selectors,
        tss: tss_ptr,
        ist_stacks,
    });
    Box::into_raw(per_cpu)
}

unsafe fn load_per_cpu_gdt(gdt: *mut PerCpuGdt) {
    use x86_64::instructions::segmentation::{Segment, CS, DS, ES, SS};
    use x86_64::instructions::tables::load_tss;

    (*gdt).gdt.load();
    CS::set_reg((*gdt).selectors.code_selector);
    DS::set_reg((*gdt).selectors.data_selector);
    ES::set_reg((*gdt).selectors.data_selector);
    SS::set_reg((*gdt).selectors.data_selector);
    load_tss((*gdt).selectors.tss_selector);
}
