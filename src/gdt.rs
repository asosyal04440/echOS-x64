use core::arch::asm;
use core::mem;
#[allow(dead_code)]
#[repr(packed)] // bellekte 8 byte yer kaplamasını sağlar
pub struct GdtEntry {
    limit_low: u16, // Segment limitinin ilk 16 biti (0-15)
    base_low: u16, // Base adresin ilk 16 biti (0-15)
    base_middle: u8, // Base adresin sonraki 8 biti (16-23)
    access: u8, // Erişim baytı (segment özellikleri)
    flags_limit_high: u8, // Flags + limitin yüksek 4 biti (16-19)
    base_high: u8, // Base adresin son 8 biti (24-31)
}

impl GdtEntry {
    pub const fn new(base: u64, limit: u32, access: u8, flags: u8) -> Self {
        Self {
            limit_low: (limit & 0xFFFF) as u16,

            base_low: (base & 0xFFFF) as u16,
            base_middle: ((base >> 16) & 0xFF) as u8,
            access,
            flags_limit_high: ((limit >> 16) as u8) | (flags << 4),
            base_high: ((base >> 24) & 0xFF) as u8,
        }
    }
}

#[repr(C)]
pub struct GdtTable {
    null_entry: GdtEntry, // Zorunlu null descriptor
    code_segment: GdtEntry,
    data_segment: GdtEntry,
    
}
impl GdtTable {
    pub const fn new() -> Self {
        Self{
            null_entry: GdtEntry::new(0,0,0,0),
            code_segment: GdtEntry::new(
                0, // Base address
                0xFFFFF, // Limit
                0x9A, // Present + Executable + Readable
                0xAF, //64-bit code segmen
            ),
            data_segment: GdtEntry::new(
                0,
                0x000F_FFFF,
                0x92, // Present + Writable
                0xCF, // 4KB granularity
            ),
        }
    }
    
    pub fn load(&self) {
        let self_ptr = self as *const Self as u64;
        let gdtr = GdtPointer {
            limit: (mem::size_of::<Self>() - 1) as u16,
            base: self_ptr,
        };
        
        unsafe {
            asm!("lgdt [{}]", in(reg) &gdtr)
        }
    }
}

#[derive(Debug)]
#[repr(C, packed)]
struct GdtPointer {
    limit: u16,
    base: u64,
}
