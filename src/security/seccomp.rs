//! # Seccomp (Secure Computing Mode)
//!
//! System call filtering with BPF.

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// SECCOMP CONSTANTS
// ============================================================================

/// Seccomp modes
pub const SECCOMP_MODE_DISABLED: u32 = 0;
pub const SECCOMP_MODE_STRICT: u32 = 1;
pub const SECCOMP_MODE_FILTER: u32 = 2;

/// Seccomp actions
pub const SECCOMP_RET_KILL_PROCESS: u32 = 0x80000000;
pub const SECCOMP_RET_KILL_THREAD: u32 = 0x00000000;
pub const SECCOMP_RET_KILL: u32 = SECCOMP_RET_KILL_THREAD;
pub const SECCOMP_RET_TRAP: u32 = 0x00030000;
pub const SECCOMP_RET_ERRNO: u32 = 0x00050000;
pub const SECCOMP_RET_TRACE: u32 = 0x7ff00000;
pub const SECCOMP_RET_LOG: u32 = 0x7ffc0000;
pub const SECCOMP_RET_ALLOW: u32 = 0x7fff0000;

/// Seccomp return mask
pub const SECCOMP_RET_ACTION: u32 = 0x7fff0000;
pub const SECCOMP_RET_DATA: u32 = 0x0000ffff;

/// Strict mode allowed syscalls
pub const SECCOMP_STRICT_ALLOWED: &[i32] = &[
    0,  // read
    1,  // write
    2,  // open
    3,  // close
    60, // exit
    231, // exit_group
    9,  // mmap
    12, // brk
    59, // execve
];

/// BPF instruction classes
pub const BPF_CLASS_LD: u16 = 0x00;
pub const BPF_CLASS_LDX: u16 = 0x01;
pub const BPF_CLASS_ST: u16 = 0x02;
pub const BPF_CLASS_STX: u16 = 0x03;
pub const BPF_CLASS_ALU: u16 = 0x04;
pub const BPF_CLASS_JMP: u16 = 0x05;
pub const BPF_CLASS_RET: u16 = 0x06;
pub const BPF_CLASS_MISC: u16 = 0x07;

/// BPF size modifiers
pub const BPF_SIZE_W: u16 = 0x00;
pub const BPF_SIZE_H: u16 = 0x08;
pub const BPF_SIZE_B: u16 = 0x10;
pub const BPF_SIZE_DW: u16 = 0x18;

/// BPF mode modifiers
pub const BPF_MODE_IMM: u16 = 0x00;
pub const BPF_MODE_ABS: u16 = 0x20;
pub const BPF_MODE_IND: u16 = 0x40;
pub const BPF_MODE_MEM: u16 = 0x60;
pub const BPF_MODE_LEN: u16 = 0x80;
pub const BPF_MODE_MSH: u16 = 0xa0;

/// BPF source modifiers
pub const BPF_SRC_K: u16 = 0x00;
pub const BPF_SRC_X: u16 = 0x08;

/// BPF jump conditions
pub const BPF_JMP_JA: u16 = 0x00;
pub const BPF_JMP_JEQ: u16 = 0x10;
pub const BPF_JMP_JGT: u16 = 0x20;
pub const BPF_JMP_JGE: u16 = 0x30;
pub const BPF_JMP_JSET: u16 = 0x40;

// ============================================================================
// BPF INSTRUCTION
// ============================================================================

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct BpfInstruction {
    pub code: u16,
    pub jt: u8,
    pub jf: u8,
    pub k: u32,
}

impl BpfInstruction {
    pub fn new(code: u16, jt: u8, jf: u8, k: u32) -> Self {
        Self { code, jt, jf, k }
    }

    /// Load immediate
    pub fn ld_imm(k: u32) -> Self {
        Self::new(BPF_CLASS_LD | BPF_SIZE_W | BPF_MODE_IMM, 0, 0, k)
    }

    /// Load from seccomp data (absolute)
    pub fn ld_abs(offset: u32) -> Self {
        Self::new(BPF_CLASS_LD | BPF_SIZE_W | BPF_MODE_ABS, 0, 0, offset)
    }

    /// Jump if equal
    pub fn jeq(k: u32, jt: u8, jf: u8) -> Self {
        Self::new(BPF_CLASS_JMP | BPF_JMP_JEQ | BPF_SRC_K, jt, jf, k)
    }

    /// Jump if greater
    pub fn jgt(k: u32, jt: u8, jf: u8) -> Self {
        Self::new(BPF_CLASS_JMP | BPF_JMP_JGT | BPF_SRC_K, jt, jf, k)
    }

    /// Jump if greater or equal
    pub fn jge(k: u32, jt: u8, jf: u8) -> Self {
        Self::new(BPF_CLASS_JMP | BPF_JMP_JGE | BPF_SRC_K, jt, jf, k)
    }

    /// Return
    pub fn ret(k: u32) -> Self {
        Self::new(BPF_CLASS_RET | BPF_SRC_K, 0, 0, k)
    }
}

// ============================================================================
// BPF PROGRAM
// ============================================================================

#[derive(Clone, Debug)]
pub struct BpfProgram {
    pub instructions: Vec<BpfInstruction>,
    pub pc: usize,
}

impl BpfProgram {
    pub fn new() -> Self {
        Self {
            instructions: Vec::new(),
            pc: 0,
        }
    }

    /// Add instruction
    pub fn add(&mut self, instr: BpfInstruction) {
        self.instructions.push(instr);
    }

    /// Execute program
    pub fn execute(&self, data: &SeccompData) -> u32 {
        let mut regs = BpfRegisters::new();
        let mut pc: usize = 0;
        
        while pc < self.instructions.len() {
            let instr = &self.instructions[pc];
            
            match instr.code & 0x07 {
                BPF_CLASS_LD => {
                    let mode = instr.code & 0xe0;
                    if mode == BPF_MODE_ABS {
                        // Load from seccomp data at offset k
                        let offset = instr.k as usize;
                        let val = data.get_field(offset);
                        regs.a = val;
                    } else if mode == BPF_MODE_IMM {
                        regs.a = instr.k;
                    }
                    pc += 1;
                }
                BPF_CLASS_JMP => {
                    let cond = instr.code & 0xf0;
                    let match_val = if cond == BPF_JMP_JEQ {
                        regs.a == instr.k
                    } else if cond == BPF_JMP_JGT {
                        regs.a > instr.k
                    } else if cond == BPF_JMP_JGE {
                        regs.a >= instr.k
                    } else if cond == BPF_JMP_JSET {
                        (regs.a & instr.k) != 0
                    } else {
                        // JA - always jump
                        pc = pc.wrapping_add(instr.k as usize);
                        continue;
                    };
                    
                    if match_val {
                        pc = pc.wrapping_add(instr.jt as usize).wrapping_add(1);
                    } else {
                        pc = pc.wrapping_add(instr.jf as usize).wrapping_add(1);
                    }
                }
                BPF_CLASS_RET => {
                    return instr.k;
                }
                BPF_CLASS_ALU => {
                    // ALU operations
                    pc += 1;
                }
                _ => {
                    pc += 1;
                }
            }
        }
        
        SECCOMP_RET_ALLOW
    }
}

struct BpfRegisters {
    a: u32,
    x: u32,
}

impl BpfRegisters {
    fn new() -> Self {
        Self { a: 0, x: 0 }
    }
}

// ============================================================================
// SECCOMP DATA
// ============================================================================

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SeccompData {
    pub nr: i32,           // System call number
    pub arch: u32,         // Architecture
    pub instruction_pointer: u64,
    pub args: [u64; 6],    // System call arguments
}

impl SeccompData {
    pub fn new(nr: i32, args: [u64; 6]) -> Self {
        Self {
            nr,
            arch: 0xC000003E, // x86_64
            instruction_pointer: 0,
            args,
        }
    }

    /// Get field at offset
    pub fn get_field(&self, offset: usize) -> u32 {
        match offset {
            0 => self.nr as u32,
            4 => self.arch,
            8..=15 => {
                let idx = (offset - 8) / 8;
                if idx < 6 {
                    (self.args[idx] >> ((offset % 8) * 8)) as u32
                } else {
                    0
                }
            }
            _ => 0,
        }
    }
}

// ============================================================================
// SECCOMP FILTER
// ============================================================================

pub struct SeccompFilter {
    /// Filter ID
    pub id: u32,
    /// BPF program
    pub program: BpfProgram,
    /// Default action
    pub default_action: u32,
    /// Flags
    pub flags: u32,
    /// Reference count
    pub ref_count: AtomicU32,
}

impl SeccompFilter {
    pub fn new(id: u32, program: BpfProgram, default_action: u32, flags: u32) -> Self {
        Self {
            id,
            program,
            default_action,
            flags,
            ref_count: AtomicU32::new(1),
        }
    }

    /// Evaluate filter for syscall
    pub fn evaluate(&self, data: &SeccompData) -> u32 {
        let result = self.program.execute(data);
        
        // Extract action
        let action = result & SECCOMP_RET_ACTION;
        
        if action == 0 {
            // No action specified, use default
            self.default_action
        } else {
            action
        }
    }
}

// ============================================================================
// SECCOMP CONTEXT (PER-TASK)
// ============================================================================

pub struct SeccompContext {
    /// Mode
    pub mode: AtomicU32,
    /// Filter (for mode 2)
    pub filter: Mutex<Option<Arc<SeccompFilter>>>,
    /// No new privs
    pub no_new_privs: AtomicBool,
    /// Sync mode
    pub sync: AtomicBool,
}

impl SeccompContext {
    pub fn new() -> Self {
        Self {
            mode: AtomicU32::new(SECCOMP_MODE_DISABLED),
            filter: Mutex::new(None),
            no_new_privs: AtomicBool::new(false),
            sync: AtomicBool::new(false),
        }
    }

    /// Set strict mode
    pub fn set_strict(&self) -> Result<(), SeccompError> {
        if self.mode.load(Ordering::SeqCst) != SECCOMP_MODE_DISABLED {
            return Err(SeccompError::AlreadySet);
        }
        
        self.mode.store(SECCOMP_MODE_STRICT, Ordering::SeqCst);
        Ok(())
    }

    /// Set filter mode
    pub fn set_filter(&self, filter: Arc<SeccompFilter>) -> Result<(), SeccompError> {
        let current_mode = self.mode.load(Ordering::SeqCst);
        
        if current_mode == SECCOMP_MODE_STRICT {
            return Err(SeccompError::AlreadySet);
        }
        
        *self.filter.lock() = Some(filter);
        self.mode.store(SECCOMP_MODE_FILTER, Ordering::SeqCst);
        Ok(())
    }

    /// Check syscall
    pub fn check_syscall(&self, nr: i32, args: [u64; 6]) -> u32 {
        let mode = self.mode.load(Ordering::SeqCst);
        
        match mode {
            SECCOMP_MODE_DISABLED => SECCOMP_RET_ALLOW,
            SECCOMP_MODE_STRICT => {
                if SECCOMP_STRICT_ALLOWED.contains(&nr) {
                    SECCOMP_RET_ALLOW
                } else {
                    SECCOMP_RET_KILL_PROCESS
                }
            }
            SECCOMP_MODE_FILTER => {
                if let Some(filter) = self.filter.lock().as_ref() {
                    let data = SeccompData::new(nr, args);
                    filter.evaluate(&data)
                } else {
                    SECCOMP_RET_ALLOW
                }
            }
            _ => SECCOMP_RET_ALLOW,
        }
    }

    /// Get mode
    pub fn get_mode(&self) -> u32 {
        self.mode.load(Ordering::SeqCst)
    }
}

// ============================================================================
// SECCOMP MANAGER
// ============================================================================

pub struct SeccompManager {
    filters: Mutex<BTreeMap<u32, Arc<SeccompFilter>>>,
    next_filter_id: AtomicU32,
    stats: Mutex<SeccompStats>,
}

#[derive(Clone, Debug, Default)]
pub struct SeccompStats {
    pub filters_count: u32,
    pub syscalls_filtered: u64,
    pub syscalls_allowed: u64,
    pub processes_killed: u64,
}

impl SeccompManager {
    pub fn new() -> Self {
        Self {
            filters: Mutex::new(BTreeMap::new()),
            next_filter_id: AtomicU32::new(1),
            stats: Mutex::new(SeccompStats::default()),
        }
    }

    /// Create filter from BPF program
    pub fn create_filter(&self, program: BpfProgram, default_action: u32, flags: u32) -> Arc<SeccompFilter> {
        let id = self.next_filter_id.fetch_add(1, Ordering::SeqCst);
        let filter = Arc::new(SeccompFilter::new(id, program, default_action, flags));
        
        self.filters.lock().insert(id, filter.clone());
        
        let mut stats = self.stats.lock();
        stats.filters_count += 1;
        
        filter
    }

    /// Get statistics
    pub fn get_stats(&self) -> SeccompStats {
        self.stats.lock().clone()
    }
}

lazy_static::lazy_static! {
    pub static ref SECCOMP: SeccompManager = SeccompManager::new();
}

// ============================================================================
// ERROR TYPE
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeccompError {
    AlreadySet,
    InvalidFilter,
    PermissionDenied,
}

// ============================================================================
// SYSCALL INTERFACE
// ============================================================================

pub fn sys_seccomp(mode: u32, flags: u32, filter_prog: Option<&[BpfInstruction]>) -> i32 {
    match mode {
        SECCOMP_MODE_DISABLED => 0,
        SECCOMP_MODE_STRICT => {
            // Would set strict mode on current task
            0
        }
        SECCOMP_MODE_FILTER => {
            if let Some(prog) = filter_prog {
                let mut program = BpfProgram::new();
                for instr in prog {
                    program.add(*instr);
                }
                
                let filter = SECCOMP.create_filter(program, SECCOMP_RET_KILL_PROCESS, flags);
                // Would attach to current task
                0
            } else {
                -22
            }
        }
        _ => -22,
    }
}

// ============================================================================
// INITIALIZATION
// ============================================================================

pub fn init() {
    crate::serial_println!("[SECCOMP] Subsystem initialized");
}
