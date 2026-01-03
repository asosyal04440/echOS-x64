//! # echOS Global Descriptor Table (GDT)
//! 
//! x86_64 segment tanımlayıcıları ve TSS (Task State Segment) yapılandırması.
//! Kernel/User mode geçişi için segment selector'ları içerir.

use x86_64::VirtAddr;
use x86_64::structures::tss::TaskStateSegment;
use x86_64::structures::gdt::{GlobalDescriptorTable, Descriptor, SegmentSelector};
use lazy_static::lazy_static;

/// Double Fault için IST index
pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;
/// Page Fault için IST index
pub const PAGE_FAULT_IST_INDEX: u16 = 1;
/// General Protection Fault için IST index
pub const GENERAL_PROTECTION_IST_INDEX: u16 = 2;

/// Global TSS (Task State Segment) - context switch için gerekli
static mut TSS: TaskStateSegment = TaskStateSegment::new();

/// TSS exception stack'lerini başlatır.
/// Her kritik exception için ayrı stack ayrılır (Sentinel Architecture).
pub fn init_tss() {
    unsafe {
        const STACK_SIZE: usize = 4096 * 5; // 20KB per exception stack
        
        let allocate_stack = |idx: usize| {
            match idx {
                0 => {
                    static mut STACK_0: [u8; STACK_SIZE] = [0; STACK_SIZE];
                    VirtAddr::from_ptr(&STACK_0) + STACK_SIZE as u64
                },
                1 => {
                    static mut STACK_1: [u8; STACK_SIZE] = [0; STACK_SIZE];
                    VirtAddr::from_ptr(&STACK_1) + STACK_SIZE as u64
                },
                2 => {
                    static mut STACK_2: [u8; STACK_SIZE] = [0; STACK_SIZE];
                    VirtAddr::from_ptr(&STACK_2) + STACK_SIZE as u64
                },
                3 => { static mut S: [u8; STACK_SIZE] = [0; STACK_SIZE]; VirtAddr::from_ptr(&S) + STACK_SIZE as u64 },
                4 => { static mut S: [u8; STACK_SIZE] = [0; STACK_SIZE]; VirtAddr::from_ptr(&S) + STACK_SIZE as u64 },
                5 => { static mut S: [u8; STACK_SIZE] = [0; STACK_SIZE]; VirtAddr::from_ptr(&S) + STACK_SIZE as u64 },
                6 => { static mut S: [u8; STACK_SIZE] = [0; STACK_SIZE]; VirtAddr::from_ptr(&S) + STACK_SIZE as u64 },
                _ => VirtAddr::new(0),
            }
        };

        TSS.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = allocate_stack(0);
        TSS.interrupt_stack_table[PAGE_FAULT_IST_INDEX as usize] = allocate_stack(1);
        TSS.interrupt_stack_table[GENERAL_PROTECTION_IST_INDEX as usize] = allocate_stack(2);
        
        for i in 3..7 {
            TSS.interrupt_stack_table[i] = allocate_stack(i);
        }
    }
}

/// Kernel stack pointer'ını günceller (RSP0).
/// Scheduler task değiştirdiğinde çağrılır.
pub fn set_kernel_stack(stack_top: VirtAddr) {
    unsafe {
        TSS.privilege_stack_table[0] = stack_top;
    }
}

lazy_static! {
    /// Global Descriptor Table ve segment selector'ları
    pub static ref GDT: (GlobalDescriptorTable, Selectors) = {
        let mut gdt = GlobalDescriptorTable::new();
        let code_selector = gdt.append(Descriptor::kernel_code_segment());
        let data_selector = gdt.append(Descriptor::kernel_data_segment());
        
        init_tss();
        let tss_selector = gdt.append(Descriptor::tss_segment(unsafe { &TSS }));
        
        // User Mode Segments (Ring 3)
        // SYSCALL/SYSRET için sıralama önemli!
        let user_data_selector = gdt.append(Descriptor::user_data_segment());
        let user_code_selector = gdt.append(Descriptor::user_code_segment());
        
        (gdt, Selectors { 
            code_selector, 
            data_selector, 
            tss_selector,
            user_code_selector,
            user_data_selector 
        })
    };
}

/// GDT segment selector'ları
pub struct Selectors {
    pub code_selector: SegmentSelector,
    pub data_selector: SegmentSelector,
    pub tss_selector: SegmentSelector,
    pub user_code_selector: SegmentSelector,
    pub user_data_selector: SegmentSelector,
}

/// GDT'yi yükler ve segment register'larını ayarlar.
pub fn init() {
    use x86_64::instructions::tables::load_tss;
    use x86_64::instructions::segmentation::{CS, Segment};
    
    GDT.0.load();
    unsafe {
        CS::set_reg(GDT.1.code_selector);
        load_tss(GDT.1.tss_selector);
        
        use x86_64::instructions::segmentation::{DS, ES, SS};
        DS::set_reg(GDT.1.data_selector);
        ES::set_reg(GDT.1.data_selector);
        SS::set_reg(GDT.1.data_selector);
    }
}
