//! # eBPF (extended Berkeley Packet Filter)
//!
//! echOS için eBPF sanal makinesi ve JIT derleyici.
//! Ağ paket filtreleme, sistem izleme ve güvenlik için kullanılır.
//!
//! ## eBPF Nedir?
//!
//! eBPF, Linux çekirdeğinde çalışan güvenli sanal makine teknolojisidir.
//! Kullanıcı alanında yazılan bytecode'lar çekirdek alanında güvenli bir şekilde çalıştırılır.
//!
//! ## eBPF Mimarisi
//!
//! ```text
//!  Kullanıcı Alanı              Çekirdek Alanı
//!  ┌──────────────┐           ┌──────────────────┐
//!  │ eBPF C Kodu  │           │ eBPF Sanal Makine │
//!  │ clang -O2    │──compile─►│ (JIT veya yorum) │
//!  └──────────────┘           └──────────────────┘
//!         │                           │
//!  ┌──────────────┐           ┌──────────────────┐
//!  │ eBPF Bytecode│──load──►  │ Hook Noktaları  │
//!  │ (ELF dosyası)│           │ (socket, trace) │
//!  └──────────────┘           └──────────────────┘
//! ```
//!
//! ## eBPF Register'ları
//!
//! ```text
//! R0: Return value (dönüş değeri)
//! R1-R5: Function arguments (fonksiyon argümanları)
//! R6-R9: Callee-saved registers (kayıtlı register'lar)
//! R10: Frame pointer (çerçeve işaretçisi)
//! ```

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// eBPF SABİTLERİ
// ============================================================================

/// eBPF register'ları
pub const BPF_REG_0: u8 = 0;
pub const BPF_REG_1: u8 = 1;
pub const BPF_REG_2: u8 = 2;
pub const BPF_REG_3: u8 = 3;
pub const BPF_REG_4: u8 = 4;
pub const BPF_REG_5: u8 = 5;
pub const BPF_REG_6: u8 = 6;
pub const BPF_REG_7: u8 = 7;
pub const BPF_REG_8: u8 = 8;
pub const BPF_REG_9: u8 = 9;
pub const BPF_REG_10: u8 = 10;

/// eBPF opcode'ları
pub const BPF_LD: u8 = 0x00;
pub const BPF_LDX: u8 = 0x01;
pub const BPF_ST: u8 = 0x02;
pub const BPF_STX: u8 = 0x03;
pub const BPF_ALU: u8 = 0x04;
pub const BPF_JMP: u8 = 0x05;
pub const BPF_RET: u8 = 0x06;
pub const BPF_MISC: u8 = 0x07;

/// eBPF class'ları ( opcode'ların üst 4 biti)
pub const BPF_CLASS_LD: u8 = 0x00;
pub const BPF_CLASS_LDX: u8 = 0x10;
pub const BPF_CLASS_ST: u8 = 0x20;
pub const BPF_CLASS_STX: u8 = 0x30;
pub const BPF_CLASS_ALU: u8 = 0x40;
pub const BPF_CLASS_JMP: u8 = 0x50;
pub const BPF_CLASS_RET: u8 = 0x60;
pub const BPF_CLASS_MISC: u8 = 0x70;

/// ALU opcode'ları
pub const BPF_ADD: u8 = 0x00;
pub const BPF_SUB: u8 = 0x10;
pub const BPF_MUL: u8 = 0x20;
pub const BPF_DIV: u8 = 0x30;
pub const BPF_OR: u8 = 0x40;
pub const BPF_AND: u8 = 0x50;
pub const BPF_LSH: u8 = 0x60;
pub const BPF_RSH: u8 = 0x70;
pub const BPF_NEG: u8 = 0x80;
pub const BPF_MOD: u8 = 0x90;
pub const BPF_XOR: u8 = 0xa0;
pub const BPF_MOV: u8 = 0xb0;
pub const BPF_ARSH: u8 = 0xc0;
pub const BPF_END: u8 = 0xd0;

/// BPF ALU operation modifiers
pub const BPF_SRC_K: u8 = 0x00;
pub const BPF_SRC_X: u8 = 0x08;

/// BPF ALU op codes (combined with operation)
pub const BPF_ALU_OP_AND: u8 = BPF_AND | BPF_SRC_K;
pub const BPF_ALU_OP_OR: u8 = BPF_OR | BPF_SRC_K;
pub const BPF_ALU_OP_XOR: u8 = BPF_XOR | BPF_SRC_K;
pub const BPF_ALU_OP_ADD: u8 = BPF_ADD | BPF_SRC_K;
pub const BPF_ALU_OP_SUB: u8 = BPF_SUB | BPF_SRC_K;
pub const BPF_ALU_OP_MUL: u8 = BPF_MUL | BPF_SRC_K;
pub const BPF_ALU_OP_DIV: u8 = BPF_DIV | BPF_SRC_K;

/// BPF load mode
pub const BPF_ABS: u8 = 0x20;
pub const BPF_IND: u8 = 0x40;
pub const BPF_MEM: u8 = 0x60;
pub const BPF_LEN: u8 = 0x00;

/// BPF K (immediate) value modifier
pub const BPF_K: u8 = 0x00;

/// BPF jump modifiers
pub const BPF_JUMP: u8 = 0x00;

/// JMP opcode'ları
pub const BPF_JA: u8 = 0x00;
pub const BPF_JEQ: u8 = 0x10;
pub const BPF_JGT: u8 = 0x20;
pub const BPF_JGE: u8 = 0x30;
pub const BPF_JSET: u8 = 0x40;
pub const BPF_JNE: u8 = 0x50;
pub const BPF_JSGT: u8 = 0x60;
pub const BPF_JSGE: u8 = 0x70;
pub const BPF_CALL: u8 = 0x80;
pub const BPF_EXIT: u8 = 0x90;

/// Bellek erişim boyutları
pub const BPF_W: u8 = 0x00; // 32-bit
pub const BPF_H: u8 = 0x08; // 16-bit
pub const BPF_B: u8 = 0x10; // 8-bit
pub const BPF_DW: u8 = 0x18; // 64-bit

/// eBPF program tipleri
pub const BPF_PROG_TYPE_SOCKET_FILTER: u32 = 1;
pub const BPF_PROG_TYPE_KPROBE: u32 = 2;
pub const BPF_PROG_TYPE_SCHED_CLS: u32 = 3;
pub const BPF_PROG_TYPE_SCHED_ACT: u32 = 4;
pub const BPF_PROG_TYPE_TRACEPOINT: u32 = 5;
pub const BPF_PROG_TYPE_XDP: u32 = 6;
pub const BPF_PROG_TYPE_PERF_EVENT: u32 = 7;
pub const BPF_PROG_TYPE_CGROUP_SKB: u32 = 8;
pub const BPF_PROG_TYPE_CGROUP_SOCK: u32 = 9;
pub const BPF_PROG_TYPE_LWT_IN: u32 = 10;
pub const BPF_PROG_TYPE_LWT_OUT: u32 = 11;
pub const BPF_PROG_TYPE_LWT_XMIT: u32 = 12;
pub const BPF_PROG_TYPE_SOCK_OPS: u32 = 13;
pub const BPF_PROG_TYPE_SK_SKB: u32 = 14;
pub const BPF_PROG_TYPE_CGROUP_DEVICE: u32 = 15;
pub const BPF_PROG_TYPE_SK_MSG: u8 = 16;
pub const BPF_PROG_TYPE_RAW_TRACEPOINT: u32 = 17;
pub const BPF_PROG_TYPE_CGROUP_SOCK_ADDR: u32 = 18;
pub const BPF_PROG_TYPE_LWT_SEG6LOCAL: u32 = 19;
pub const BPF_PROG_TYPE_LIRC_MODE2: u32 = 20;
pub const BPF_PROG_TYPE_SK_REUSEPORT: u32 = 21;
pub const BPF_PROG_TYPE_FLOW_DISSECTOR: u32 = 22;
pub const BPF_PROG_TYPE_CGROUP_SYSCTL: u32 = 23;
pub const BPF_PROG_TYPE_RAW_TRACEPOINT_WRITABLE: u32 = 24;
pub const BPF_PROG_TYPE_CGROUP_SOCKOPT: u32 = 25;

/// Maksimum eBPF program boyutu
pub const BPF_MAXINSNS: usize = 4096;

/// Stack boyutu (512 byte)
pub const BPF_STACK_SIZE: usize = 512;

// ============================================================================
// eBPF HATASI
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EbpfError {
    /// Geçersiz opcode
    InvalidOpcode,
    /// Geçersiz register
    InvalidRegister,
    /// Bellek erişim hatası
    MemoryAccess,
    /// Bölüm hatası (segmentation fault)
    SegmentationFault,
    /// Stack taşması
    StackOverflow,
    /// sonsuz döngü
    InfiniteLoop,
    /// Fonksiyon çağrısı hatası
    CallError,
    /// JIT derleme hatası
    JitError,
    InvalidElf,
    UnsupportedJit,
    VerifierRejected,
    UnsupportedAttach,
}

// ============================================================================
// eBPF SANAL MAKİNESİ
// ============================================================================

/// eBPF sanal makinesi
pub struct EbpfVm {
    /// Register'lar (R0-R10)
    registers: [u64; 11],
    /// Stack
    stack: [u8; BPF_STACK_SIZE],
    /// Program bytecode
    program: Vec<u64>,
    /// Program tipi
    prog_type: u32,
    /// Çalıştırılan instruction sayısı
    insn_count: AtomicU64,
    /// JIT derlenmiş kod (varsa)
    jit_program: Option<Vec<crate::ebpf::BpfInsn>>,
}

impl EbpfVm {
    /// Yeni eBPF sanal makinesi oluştur
    pub fn new(program: Vec<u64>, prog_type: u32) -> Self {
        Self {
            registers: [0; 11],
            stack: [0; BPF_STACK_SIZE],
            program,
            prog_type,
            insn_count: AtomicU64::new(0),
            jit_program: None,
        }
    }

    /// Programı çalıştır
    pub fn execute(&mut self, ctx: *const u8) -> Result<u64, EbpfError> {
        // Register'ları sıfırla
        self.registers.fill(0);
        self.registers[BPF_REG_10 as usize] = BPF_STACK_SIZE as u64; // Frame pointer

        // Context'i R1'e yükle
        self.registers[BPF_REG_1 as usize] = ctx as u64;

        // JIT kod varsa onu kullan, yoksa interpreter
        if let Some(ref jit_program) = self.jit_program.take() {
            // JIT kodu var, çalıştır
            let result = self.execute_jit(jit_program, ctx);
            // JIT kodu geri koy (borrowed olduğu için)
            self.jit_program = Some(jit_program.clone());
            result
        } else {
            // Interpreter modunda çalıştır
            self.execute_interpreter()
        }
    }

    /// Interpreter modunda çalıştır
    fn execute_interpreter(&mut self) -> Result<u64, EbpfError> {
        let mut pc = 0; // Program counter

        while pc < self.program.len() {
            self.insn_count.fetch_add(1, Ordering::SeqCst);

            let insn = self.program[pc];
            let opcode = ((insn >> 56) & 0xFF) as u8;
            let dst = ((insn >> 48) & 0xFF) as u8;
            let src = ((insn >> 40) & 0xFF) as u8;
            let off = ((insn >> 32) & 0xFFFF) as i16;
            let imm = insn as i32;

            match opcode {
                // ALU operations
                BPF_ALU => {
                    let alu_op = ((insn >> 52) & 0x0F) as u8;
                    match alu_op {
                        BPF_ADD => {
                            if (insn & (1 << 32)) != 0 {
                                // ADD64 imm
                                self.registers[dst as usize] =
                                    self.registers[dst as usize].wrapping_add(imm as u64);
                            } else {
                                // ADD64 src
                                self.registers[dst as usize] = self.registers[dst as usize]
                                    .wrapping_add(self.registers[src as usize]);
                            }
                        }
                        BPF_SUB => {
                            if (insn & (1 << 32)) != 0 {
                                // SUB64 imm
                                self.registers[dst as usize] =
                                    self.registers[dst as usize].wrapping_sub(imm as u64);
                            } else {
                                // SUB64 src
                                self.registers[dst as usize] = self.registers[dst as usize]
                                    .wrapping_sub(self.registers[src as usize]);
                            }
                        }
                        BPF_MUL => {
                            if (insn & (1 << 32)) != 0 {
                                // MUL64 imm
                                self.registers[dst as usize] =
                                    self.registers[dst as usize].wrapping_mul(imm as u64);
                            } else {
                                // MUL64 src
                                self.registers[dst as usize] = self.registers[dst as usize]
                                    .wrapping_mul(self.registers[src as usize]);
                            }
                        }
                        BPF_DIV => {
                            if (insn & (1 << 32)) != 0 {
                                // DIV64 imm
                                let divisor = imm as u64;
                                if divisor == 0 {
                                    return Err(EbpfError::CallError);
                                }
                                self.registers[dst as usize] /= divisor;
                            } else {
                                // DIV64 src
                                let divisor = self.registers[src as usize];
                                if divisor == 0 {
                                    return Err(EbpfError::CallError);
                                }
                                self.registers[dst as usize] /= divisor;
                            }
                        }
                        BPF_MOV => {
                            if (insn & (1 << 32)) != 0 {
                                // MOV64 imm
                                self.registers[dst as usize] = imm as u64;
                            } else {
                                // MOV64 src
                                self.registers[dst as usize] = self.registers[src as usize];
                            }
                        }
                        _ => return Err(EbpfError::InvalidOpcode),
                    }
                }

                // Memory operations
                BPF_LDX => {
                    let size = ((insn >> 56) & 0x18) as u8;
                    let src_reg = src;
                    let dst_reg = dst;

                    let addr = self.registers[src_reg as usize] + off as u64;
                    let value = self.memory_read(addr, size)?;
                    self.registers[dst_reg as usize] = value;
                }

                BPF_STX => {
                    let size = ((insn >> 56) & 0x18) as u8;
                    let src_reg = src;
                    let dst_reg = dst;

                    let addr = self.registers[dst_reg as usize] + off as u64;
                    let value = self.registers[src_reg as usize];
                    self.memory_write(addr, value, size)?;
                }

                // Jump operations
                BPF_JMP => {
                    let jmp_op = ((insn >> 52) & 0x0F) as u8;
                    match jmp_op {
                        BPF_JA => {
                            // Unconditional jump
                            pc += off as usize;
                            continue;
                        }
                        BPF_JEQ => {
                            let cmp_val = if (insn & (1 << 32)) != 0 {
                                imm as u64
                            } else {
                                self.registers[src as usize]
                            };
                            if self.registers[dst as usize] == cmp_val {
                                pc += off as usize;
                            }
                        }
                        BPF_JGT => {
                            let cmp_val = if (insn & (1 << 32)) != 0 {
                                imm as u64
                            } else {
                                self.registers[src as usize]
                            };
                            if self.registers[dst as usize] > cmp_val {
                                pc += off as usize;
                            }
                        }
                        BPF_JGE => {
                            let cmp_val = if (insn & (1 << 32)) != 0 {
                                imm as u64
                            } else {
                                self.registers[src as usize]
                            };
                            if self.registers[dst as usize] >= cmp_val {
                                pc += off as usize;
                            }
                        }
                        BPF_JNE => {
                            let cmp_val = if (insn & (1 << 32)) != 0 {
                                imm as u64
                            } else {
                                self.registers[src as usize]
                            };
                            if self.registers[dst as usize] != cmp_val {
                                pc += off as usize;
                            }
                        }
                        BPF_CALL => {
                            // Builtin function call
                            let result = self.builtin_call(imm)?;
                            self.registers[BPF_REG_0 as usize] = result;
                        }
                        BPF_EXIT => {
                            return Ok(self.registers[BPF_REG_0 as usize]);
                        }
                        _ => return Err(EbpfError::InvalidOpcode),
                    }
                }

                _ => return Err(EbpfError::InvalidOpcode),
            }

            pc += 1;

            // Sonsuz döngü koruması
            if self.insn_count.load(Ordering::SeqCst) > 1000000 {
                return Err(EbpfError::InfiniteLoop);
            }
        }

        Ok(self.registers[BPF_REG_0 as usize])
    }

    /// JIT derlenmiş kodu çalıştır
    fn execute_jit(
        &mut self,
        jit_program: &[crate::ebpf::BpfInsn],
        ctx: *const u8,
    ) -> Result<u64, EbpfError> {
        crate::ebpf_jit::jit_compile_and_run(jit_program, ctx as u64).map_err(map_jit_error)
    }

    /// Bellek oku
    fn memory_read(&self, addr: u64, size: u8) -> Result<u64, EbpfError> {
        // Stack erişimi
        if addr >= (BPF_STACK_SIZE as u64 - 512) && addr < BPF_STACK_SIZE as u64 {
            let offset = (addr - (BPF_STACK_SIZE as u64 - 512)) as usize;
            match size {
                BPF_B => Ok(self.stack[offset] as u64),
                BPF_H => {
                    Ok(u16::from_le_bytes([self.stack[offset], self.stack[offset + 1]]) as u64)
                }
                BPF_W => Ok(u32::from_le_bytes([
                    self.stack[offset],
                    self.stack[offset + 1],
                    self.stack[offset + 2],
                    self.stack[offset + 3],
                ]) as u64),
                BPF_DW => Ok(u64::from_le_bytes([
                    self.stack[offset],
                    self.stack[offset + 1],
                    self.stack[offset + 2],
                    self.stack[offset + 3],
                    self.stack[offset + 4],
                    self.stack[offset + 5],
                    self.stack[offset + 6],
                    self.stack[offset + 7],
                ])),
                _ => Err(EbpfError::MemoryAccess),
            }
        } else {
            // Bu VM şu an yalnızca eBPF stack penceresini adreslenebilir kabul eder.
            Err(EbpfError::MemoryAccess)
        }
    }

    /// Bellek yaz
    fn memory_write(&mut self, addr: u64, value: u64, size: u8) -> Result<(), EbpfError> {
        // Stack erişimi
        if addr >= (BPF_STACK_SIZE as u64 - 512) && addr < BPF_STACK_SIZE as u64 {
            let offset = (addr - (BPF_STACK_SIZE as u64 - 512)) as usize;
            match size {
                BPF_B => self.stack[offset] = value as u8,
                BPF_H => {
                    let bytes = (value as u16).to_le_bytes();
                    self.stack[offset] = bytes[0];
                    self.stack[offset + 1] = bytes[1];
                }
                BPF_W => {
                    let bytes = (value as u32).to_le_bytes();
                    self.stack[offset..offset + 4].copy_from_slice(&bytes);
                }
                BPF_DW => {
                    let bytes = value.to_le_bytes();
                    self.stack[offset..offset + 8].copy_from_slice(&bytes);
                }
                _ => return Err(EbpfError::MemoryAccess),
            }
        } else {
            return Err(EbpfError::MemoryAccess);
        }
        Ok(())
    }

    /// Builtin fonksiyon çağrısı
    fn builtin_call(&mut self, func_id: i32) -> Result<u64, EbpfError> {
        match func_id {
            1 => {
                // bpf_trace_printk
                crate::serial_println!("[eBPF] Trace print helper invoked");
                Ok(0)
            }
            2 => {
                // bpf_ktime_get_ns
                Ok(crate::interrupts::get_ticks() as u64 * 1000000) // ns cinsinden
            }
            3 => {
                // bpf_get_prandom_u32
                Ok(crate::random::next_u32() as u64)
            }
            _ => Err(EbpfError::CallError),
        }
    }

    /// JIT derleme
    pub fn jit_compile(&mut self) -> Result<(), EbpfError> {
        self.jit_program = Some(translate_program_to_linux_abi(&self.program)?);
        Ok(())
    }
}

// ============================================================================
// eBPF PROGRAM YÜKLEYİCİ
// ============================================================================

/// eBPF program yükleyicisi
pub struct EbpfLoader {
    programs: BTreeMap<String, EbpfVm>,
    socket_filters: BTreeMap<String, String>,
}

impl EbpfLoader {
    /// Yeni eBPF yükleyici oluştur
    pub fn new() -> Self {
        Self {
            programs: BTreeMap::new(),
            socket_filters: BTreeMap::new(),
        }
    }

    pub fn load_program(
        &mut self,
        prog_id: &str,
        program: Vec<u64>,
        prog_type: u32,
    ) -> Result<(), EbpfError> {
        verify_program(&program, prog_type)?;
        self.programs
            .insert(prog_id.to_string(), EbpfVm::new(program, prog_type));
        Ok(())
    }

    /// ELF dosyasından program yükle
    pub fn load_elf(&mut self, elf_data: &[u8], prog_type: u32) -> Result<String, EbpfError> {
        if elf_data.len() >= 64
            && &elf_data[..4] == b"\x7FELF"
            && elf_data[4] == 2
            && elf_data[5] == 1
        {
            let read_u16 = |offset: usize| -> Option<u16> {
                elf_data
                    .get(offset..offset + 2)
                    .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
            };
            let read_u32 = |offset: usize| -> Option<u32> {
                elf_data
                    .get(offset..offset + 4)
                    .map(|bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
            };
            let read_u64 = |offset: usize| -> Option<u64> {
                elf_data.get(offset..offset + 8).map(|bytes| {
                    u64::from_le_bytes([
                        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6],
                        bytes[7],
                    ])
                })
            };

            let shoff = read_u64(40).ok_or(EbpfError::InvalidElf)? as usize;
            let shentsize = read_u16(58).ok_or(EbpfError::InvalidElf)? as usize;
            let shnum = read_u16(60).ok_or(EbpfError::InvalidElf)? as usize;
            let shstrndx = read_u16(62).ok_or(EbpfError::InvalidElf)? as usize;
            if shentsize >= 64
                && shnum > 0
                && shstrndx < shnum
                && shoff + shentsize * shnum <= elf_data.len()
            {
                let shstr_off = shoff + shentsize * shstrndx;
                let shstr_data_off =
                    read_u64(shstr_off + 24).ok_or(EbpfError::InvalidElf)? as usize;
                let shstr_size = read_u64(shstr_off + 32).ok_or(EbpfError::InvalidElf)? as usize;
                if shstr_data_off + shstr_size <= elf_data.len() {
                    let shstrtab = &elf_data[shstr_data_off..shstr_data_off + shstr_size];
                    for idx in 0..shnum {
                        let shdr = shoff + idx * shentsize;
                        let name_off = read_u32(shdr).ok_or(EbpfError::InvalidElf)? as usize;
                        let section_type = read_u32(shdr + 4).ok_or(EbpfError::InvalidElf)?;
                        let data_off = read_u64(shdr + 24).ok_or(EbpfError::InvalidElf)? as usize;
                        let data_size = read_u64(shdr + 32).ok_or(EbpfError::InvalidElf)? as usize;
                        if name_off >= shstrtab.len()
                            || section_type != 1
                            || data_size == 0
                            || data_size % 8 != 0
                        {
                            continue;
                        }
                        let end = shstrtab[name_off..]
                            .iter()
                            .position(|b| *b == 0)
                            .map(|pos| name_off + pos)
                            .unwrap_or(shstrtab.len());
                        let Some(name) = core::str::from_utf8(&shstrtab[name_off..end]).ok() else {
                            continue;
                        };
                        if !matches!(name, ".text" | "socket_filter" | "classifier") {
                            continue;
                        }
                        if data_off + data_size > elf_data.len() {
                            return Err(EbpfError::InvalidElf);
                        }

                        let mut program = Vec::with_capacity(data_size / 8);
                        for chunk in elf_data[data_off..data_off + data_size].chunks_exact(8) {
                            program.push(u64::from_le_bytes([
                                chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5],
                                chunk[6], chunk[7],
                            ]));
                        }

                        let prog_id = format!("elf:{}:{}", name, self.programs.len());
                        self.load_program(&prog_id, program, prog_type)?;
                        crate::serial_println!("[eBPF] ELF section {} loaded as {}", name, prog_id);
                        return Ok(prog_id);
                    }
                }
            }
        }
        // ELF sihirli baytları doğrulanır; section-to-bytecode dönüşümü destek açılana kadar fail-closed kalır.
        let _ = prog_type;
        if elf_data.len() < 16 || &elf_data[..4] != b"\x7FELF" {
            return Err(EbpfError::InvalidElf);
        }
        crate::serial_println!("[eBPF] ELF loader reached unsupported section parser boundary");

        Err(EbpfError::InvalidElf)
    }

    /// Programı çalıştır
    pub fn execute_program(&mut self, prog_id: &str, ctx: *const u8) -> Result<u64, EbpfError> {
        let vm = self.programs.get_mut(prog_id).ok_or(EbpfError::CallError)?;
        vm.execute(ctx)
    }

    pub fn attach_socket_filter(
        &mut self,
        attach_point: &str,
        prog_id: &str,
    ) -> Result<(), EbpfError> {
        let vm = self.programs.get(prog_id).ok_or(EbpfError::CallError)?;
        if vm.prog_type != BPF_PROG_TYPE_SOCKET_FILTER {
            return Err(EbpfError::UnsupportedAttach);
        }
        self.socket_filters
            .insert(attach_point.to_string(), prog_id.to_string());
        Ok(())
    }

    pub fn detach_socket_filter(&mut self, attach_point: &str) -> Result<(), EbpfError> {
        self.socket_filters
            .remove(attach_point)
            .map(|_| ())
            .ok_or(EbpfError::CallError)
    }

    pub fn run_socket_filter(
        &mut self,
        attach_point: &str,
        packet: &[u8],
    ) -> Result<u64, EbpfError> {
        let prog_id = self
            .socket_filters
            .get(attach_point)
            .cloned()
            .ok_or(EbpfError::UnsupportedAttach)?;
        self.execute_program(&prog_id, packet.as_ptr())
    }

    /// Programı JIT derle
    pub fn jit_compile(&mut self, prog_id: &str) -> Result<(), EbpfError> {
        let vm = self.programs.get_mut(prog_id).ok_or(EbpfError::CallError)?;
        vm.jit_compile()
    }
}

impl Default for EbpfLoader {
    fn default() -> Self {
        Self::new()
    }
}

lazy_static::lazy_static! {
    static ref GLOBAL_EBPF_LOADER: Mutex<EbpfLoader> = Mutex::new(EbpfLoader::new());
}

pub fn attach_ingress_program(prog_id: &str, program: Vec<u64>) -> Result<(), EbpfError> {
    let mut loader = GLOBAL_EBPF_LOADER.lock();
    loader.load_program(prog_id, program, BPF_PROG_TYPE_SOCKET_FILTER)?;
    loader.attach_socket_filter("net:ingress", prog_id)
}

pub fn detach_ingress_program() -> Result<(), EbpfError> {
    GLOBAL_EBPF_LOADER
        .lock()
        .detach_socket_filter("net:ingress")
}

pub fn filter_ingress_packet(packet: &[u8]) -> Result<bool, EbpfError> {
    let mut loader = GLOBAL_EBPF_LOADER.lock();
    match loader.run_socket_filter("net:ingress", packet) {
        Ok(verdict) => Ok(verdict != 0),
        Err(EbpfError::UnsupportedAttach) => Ok(true),
        Err(err) => Err(err),
    }
}

// ============================================================================
// eBPF SOKET FİLTRESİ
// ============================================================================

/// eBPF soket filtresi
pub struct EbpfSocketFilter {
    vm: EbpfVm,
}

impl EbpfSocketFilter {
    /// Yeni soket filtresi oluştur
    pub fn new(program: Vec<u64>) -> Self {
        Self {
            vm: EbpfVm::new(program, BPF_PROG_TYPE_SOCKET_FILTER),
        }
    }

    pub fn from_verified_program(program: Vec<u64>) -> Result<Self, EbpfError> {
        verify_program(&program, BPF_PROG_TYPE_SOCKET_FILTER)?;
        Ok(Self::new(program))
    }

    /// Paketi filtrele
    pub fn filter_packet(&mut self, packet: &[u8]) -> Result<u64, EbpfError> {
        let ctx = packet.as_ptr();
        self.vm.execute(ctx)
    }
}

// ============================================================================
// MODÜL BAŞLATMA
// ============================================================================

/// eBPF modülünü başlat
pub fn init() {
    crate::serial_println!("[eBPF] eBPF module initialized");
}

fn verify_program(program: &[u64], prog_type: u32) -> Result<(), EbpfError> {
    if program.is_empty() || program.len() > BPF_MAXINSNS {
        return Err(EbpfError::VerifierRejected);
    }

    if prog_type != BPF_PROG_TYPE_SOCKET_FILTER {
        return Err(EbpfError::UnsupportedAttach);
    }

    let mut has_exit = false;
    for (pc, insn) in program.iter().copied().enumerate() {
        let opcode = ((insn >> 56) & 0xff) as u8;
        let dst = ((insn >> 48) & 0x0f) as u8;
        let src = ((insn >> 40) & 0x0f) as u8;
        if dst > BPF_REG_10 || src > BPF_REG_10 {
            return Err(EbpfError::InvalidRegister);
        }

        let class = opcode & 0x07;
        match class {
            BPF_LD | BPF_LDX | BPF_ST | BPF_STX | BPF_ALU | BPF_JMP | BPF_RET | BPF_MISC => {}
            _ => return Err(EbpfError::InvalidOpcode),
        }

        if opcode == (BPF_JMP | BPF_EXIT) {
            has_exit = true;
        }

        if pc == program.len() - 1 && opcode != (BPF_JMP | BPF_EXIT) {
            return Err(EbpfError::VerifierRejected);
        }
    }

    if !has_exit {
        return Err(EbpfError::VerifierRejected);
    }

    Ok(())
}

fn translate_program_to_linux_abi(program: &[u64]) -> Result<Vec<crate::ebpf::BpfInsn>, EbpfError> {
    let mut translated = Vec::with_capacity(program.len());
    for insn in program.iter().copied() {
        let opcode = ((insn >> 56) & 0xff) as u8;
        let dst = ((insn >> 48) & 0xff) as u8;
        let src = ((insn >> 40) & 0xff) as u8;
        let off = ((insn >> 32) & 0xffff) as i16;
        let imm = insn as i32;
        if dst > BPF_REG_10 || src > BPF_REG_10 {
            return Err(EbpfError::InvalidRegister);
        }
        translated.push(crate::ebpf::BpfInsn::new(opcode, dst, src, off, imm));
    }
    Ok(translated)
}

fn map_jit_error(err: crate::ebpf::BpfError) -> EbpfError {
    match err {
        crate::ebpf::BpfError::DivisionByZero => EbpfError::CallError,
        crate::ebpf::BpfError::InvalidRegister(_) => EbpfError::InvalidRegister,
        crate::ebpf::BpfError::StackOutOfBounds { .. }
        | crate::ebpf::BpfError::MemoryAccessViolation { .. } => EbpfError::MemoryAccess,
        crate::ebpf::BpfError::InvalidOpcode(_) | crate::ebpf::BpfError::InvalidInstruction(_) => {
            EbpfError::InvalidOpcode
        }
        crate::ebpf::BpfError::ExceededStepLimit => EbpfError::InfiniteLoop,
        crate::ebpf::BpfError::VerificationFailed(_)
        | crate::ebpf::BpfError::ProgramTooLarge(_)
        | crate::ebpf::BpfError::InvalidProgram
        | crate::ebpf::BpfError::InvalidJumpTarget
        | crate::ebpf::BpfError::UnreachableInstruction(_) => EbpfError::VerifierRejected,
        crate::ebpf::BpfError::UnknownHelper(_) | crate::ebpf::BpfError::ReadOnlyRegister => {
            EbpfError::CallError
        }
    }
}

/// Örnek eBPF socket-filter programı oluştur
pub fn create_simple_program() -> Vec<u64> {
    vec![
        // R0 = len(skb)
        0x0000000000000085 | ((BPF_LD | BPF_W | BPF_ABS) as u64) << 56 | 0x00 << 32,
        // if R0 < 14: goto 10
        0x0000000000000016
            | ((BPF_JMP | BPF_JGT | BPF_K) as u64) << 56
            | 0x00 << 48
            | 0x00 << 40
            | 0x0e << 32,
        // R0 = 0
        0x00000000000000b7
            | ((BPF_ALU | BPF_MOV | BPF_K) as u64) << 56
            | 0x00 << 48
            | 0x00 << 40
            | 0x00 << 32,
        // exit
        0x0000000000000095 | ((BPF_JMP | BPF_EXIT) as u64) << 56,
        // R1 = *(u8 *)(skb + 12)
        0x0000000000000071
            | ((BPF_LDX | BPF_B | BPF_ABS) as u64) << 56
            | 0x01 << 48
            | 0x00 << 40
            | 0x0c << 32,
        // R1 &= 0x0f
        0x0000000000000047
            | ((BPF_ALU | BPF_AND | BPF_K) as u64) << 56
            | 0x01 << 48
            | 0x00 << 40
            | 0x0f << 32,
        // R1 == 0x06: goto 9
        0x0000000000000015
            | ((BPF_JMP | BPF_JEQ | BPF_K) as u64) << 56
            | 0x01 << 48
            | 0x00 << 40
            | 0x06 << 32
            | 0x01 << 32,
        // R0 = 0
        0x00000000000000b7
            | ((BPF_ALU | BPF_MOV | BPF_K) as u64) << 56
            | 0x00 << 48
            | 0x00 << 40
            | 0x00 << 32,
        // exit
        0x0000000000000095 | ((BPF_JMP | BPF_EXIT) as u64) << 56,
        // R0 = skb->len
        0x0000000000000085 | ((BPF_LD | BPF_W | BPF_ABS) as u64) << 56 | 0x00 << 32,
        // exit
        0x0000000000000095 | ((BPF_JMP | BPF_EXIT) as u64) << 56,
    ]
}
