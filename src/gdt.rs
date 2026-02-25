//! # echOS Global Descriptor Table (GDT)
//!
//! x86_64 segment tanımlayıcıları ve TSS (Task State Segment) yapılandırması.
//! Kernel/User mode geçişi için segment selector'ları içerir.

use alloc::boxed::Box;
use alloc::vec::Vec;
use spin::Mutex;
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};
use x86_64::structures::tss::TaskStateSegment;
use x86_64::VirtAddr;

/// Double Fault için IST index
pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;
/// Page Fault için IST index
pub const PAGE_FAULT_IST_INDEX: u16 = 1;
/// General Protection Fault için IST index
pub const GENERAL_PROTECTION_IST_INDEX: u16 = 2;

struct PerCpuGdt {
    gdt: GlobalDescriptorTable,
    selectors: Selectors,
    tss: *mut TaskStateSegment,
    ist_stacks: [Box<[u8; IST_STACK_SIZE]>; IST_STACK_COUNT],
}

unsafe impl Sync for PerCpuGdt {}

const IST_STACK_SIZE: usize = 4096 * 5;
const IST_STACK_COUNT: usize = 7;

static PER_CPU_GDTS: Mutex<Vec<usize>> = Mutex::new(Vec::new());

/// GDT segment selector'ları
#[derive(Clone, Copy)]
pub struct Selectors {
    pub code_selector: SegmentSelector,
    pub data_selector: SegmentSelector,
    pub tss_selector: SegmentSelector,
    pub user_code_selector: SegmentSelector,
    pub user_data_selector: SegmentSelector,
}

pub fn init() {
    use x86_64::instructions::segmentation::{Segment, CS, DS, ES, SS};
    use x86_64::instructions::tables::load_tss;
    let gdt = ensure_per_cpu_gdt(0, VirtAddr::new(0));
    unsafe {
        (*gdt).gdt.load();
        CS::set_reg((*gdt).selectors.code_selector);
        DS::set_reg((*gdt).selectors.data_selector);
        ES::set_reg((*gdt).selectors.data_selector);
        SS::set_reg((*gdt).selectors.data_selector);
        load_tss((*gdt).selectors.tss_selector);
    }
}

pub fn init_for_cpu(cpu_id: u32, stack_top: VirtAddr) {
    use x86_64::instructions::segmentation::{Segment, CS, DS, ES, SS};
    use x86_64::instructions::tables::load_tss;
    let gdt = ensure_per_cpu_gdt(cpu_id, stack_top);
    unsafe {
        (*gdt).gdt.load();
        CS::set_reg((*gdt).selectors.code_selector);
        DS::set_reg((*gdt).selectors.data_selector);
        ES::set_reg((*gdt).selectors.data_selector);
        SS::set_reg((*gdt).selectors.data_selector);
        load_tss((*gdt).selectors.tss_selector);
    }
}

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

pub fn current_selectors() -> Selectors {
    let cpu_id = crate::cpu::smp::current_cpu_id();
    let ptr = ensure_per_cpu_gdt(cpu_id, VirtAddr::new(0));
    unsafe { (*ptr).selectors }
}

pub fn user_code_selector() -> SegmentSelector {
    current_selectors().user_code_selector
}

pub fn user_data_selector() -> SegmentSelector {
    current_selectors().user_data_selector
}

pub fn kernel_code_selector() -> SegmentSelector {
    current_selectors().code_selector
}

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

fn ensure_per_cpu_gdt(cpu_id: u32, stack_top: VirtAddr) -> *mut PerCpuGdt {
    let mut list = PER_CPU_GDTS.lock();
    let idx = cpu_id as usize;
    if list.len() <= idx {
        list.resize(idx + 1, 0);
    }
    if list[idx] != 0 {
        return list[idx] as *mut PerCpuGdt;
    }
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
    let ptr = Box::into_raw(per_cpu) as usize;
    list[idx] = ptr;
    ptr as *mut PerCpuGdt
}
