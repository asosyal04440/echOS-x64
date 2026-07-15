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

type EbpfAttachVerifier = fn(u32) -> bool;

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
pub const BPF_PROG_TYPE_SK_MSG: u32 = 16;
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
    VerifierRejected,
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
    /// Program doğrulaması yükleme/ilk çalıştırmada geçti mi?
    verified: bool,
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
            verified: false,
            insn_count: AtomicU64::new(0),
            jit_program: None,
        }
    }

    /// Programı çalıştır
    pub fn execute(&mut self, ctx: *const u8) -> Result<u64, EbpfError> {
        if !self.verified {
            verify_program(&self.program, self.prog_type)?;
            self.verified = true;
        }

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
            let class = opcode & 0x07;
            let op = opcode & 0xF0;
            let uses_src_reg = (opcode & BPF_SRC_X) == BPF_SRC_X;

            match class {
                // ALU operations
                BPF_ALU => match op {
                    BPF_ADD => {
                        if uses_src_reg {
                            self.registers[dst as usize] = self.registers[dst as usize]
                                .wrapping_add(self.registers[src as usize]);
                        } else {
                            self.registers[dst as usize] =
                                self.registers[dst as usize].wrapping_add(imm as u64);
                        }
                    }
                    BPF_SUB => {
                        if uses_src_reg {
                            self.registers[dst as usize] = self.registers[dst as usize]
                                .wrapping_sub(self.registers[src as usize]);
                        } else {
                            self.registers[dst as usize] =
                                self.registers[dst as usize].wrapping_sub(imm as u64);
                        }
                    }
                    BPF_MUL => {
                        if uses_src_reg {
                            self.registers[dst as usize] = self.registers[dst as usize]
                                .wrapping_mul(self.registers[src as usize]);
                        } else {
                            self.registers[dst as usize] =
                                self.registers[dst as usize].wrapping_mul(imm as u64);
                        }
                    }
                    BPF_DIV => {
                        if uses_src_reg {
                            let divisor = self.registers[src as usize];
                            if divisor == 0 {
                                return Err(EbpfError::CallError);
                            }
                            self.registers[dst as usize] /= divisor;
                        } else {
                            let divisor = imm as u64;
                            if divisor == 0 {
                                return Err(EbpfError::CallError);
                            }
                            self.registers[dst as usize] /= divisor;
                        }
                    }
                    BPF_MOD => {
                        if uses_src_reg {
                            let divisor = self.registers[src as usize];
                            if divisor == 0 {
                                return Err(EbpfError::CallError);
                            }
                            self.registers[dst as usize] %= divisor;
                        } else {
                            let divisor = imm as u64;
                            if divisor == 0 {
                                return Err(EbpfError::CallError);
                            }
                            self.registers[dst as usize] %= divisor;
                        }
                    }
                    BPF_OR => {
                        let rhs = if uses_src_reg {
                            self.registers[src as usize]
                        } else {
                            imm as u64
                        };
                        self.registers[dst as usize] |= rhs;
                    }
                    BPF_AND => {
                        let rhs = if uses_src_reg {
                            self.registers[src as usize]
                        } else {
                            imm as u64
                        };
                        self.registers[dst as usize] &= rhs;
                    }
                    BPF_XOR => {
                        let rhs = if uses_src_reg {
                            self.registers[src as usize]
                        } else {
                            imm as u64
                        };
                        self.registers[dst as usize] ^= rhs;
                    }
                    BPF_LSH => {
                        let shift = if uses_src_reg {
                            self.registers[src as usize] as u32
                        } else {
                            imm as u32
                        };
                        self.registers[dst as usize] =
                            self.registers[dst as usize].wrapping_shl(shift);
                    }
                    BPF_RSH => {
                        let shift = if uses_src_reg {
                            self.registers[src as usize] as u32
                        } else {
                            imm as u32
                        };
                        self.registers[dst as usize] =
                            self.registers[dst as usize].wrapping_shr(shift);
                    }
                    BPF_ARSH => {
                        let shift = if uses_src_reg {
                            self.registers[src as usize] as u32
                        } else {
                            imm as u32
                        };
                        self.registers[dst as usize] =
                            (self.registers[dst as usize] as i64).wrapping_shr(shift) as u64;
                    }
                    BPF_NEG => {
                        self.registers[dst as usize] =
                            (-(self.registers[dst as usize] as i64)) as u64;
                    }
                    BPF_MOV => {
                        if uses_src_reg {
                            self.registers[dst as usize] = self.registers[src as usize];
                        } else {
                            self.registers[dst as usize] = imm as u64;
                        }
                    }
                    _ => return Err(EbpfError::InvalidOpcode),
                },

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
                    match op {
                        BPF_JA => {
                            // Unconditional jump
                            pc = checked_jump_target(pc, off, self.program.len())
                                .ok_or(EbpfError::InvalidOpcode)?;
                            continue;
                        }
                        BPF_JEQ => {
                            let cmp_val = if uses_src_reg {
                                self.registers[src as usize]
                            } else {
                                imm as u64
                            };
                            if self.registers[dst as usize] == cmp_val {
                                pc = checked_jump_target(pc, off, self.program.len())
                                    .ok_or(EbpfError::InvalidOpcode)?;
                                continue;
                            }
                        }
                        BPF_JGT => {
                            let cmp_val = if uses_src_reg {
                                self.registers[src as usize]
                            } else {
                                imm as u64
                            };
                            if self.registers[dst as usize] > cmp_val {
                                pc = checked_jump_target(pc, off, self.program.len())
                                    .ok_or(EbpfError::InvalidOpcode)?;
                                continue;
                            }
                        }
                        BPF_JGE => {
                            let cmp_val = if uses_src_reg {
                                self.registers[src as usize]
                            } else {
                                imm as u64
                            };
                            if self.registers[dst as usize] >= cmp_val {
                                pc = checked_jump_target(pc, off, self.program.len())
                                    .ok_or(EbpfError::InvalidOpcode)?;
                                continue;
                            }
                        }
                        BPF_JNE => {
                            let cmp_val = if uses_src_reg {
                                self.registers[src as usize]
                            } else {
                                imm as u64
                            };
                            if self.registers[dst as usize] != cmp_val {
                                pc = checked_jump_target(pc, off, self.program.len())
                                    .ok_or(EbpfError::InvalidOpcode)?;
                                continue;
                            }
                        }
                        BPF_JSET => {
                            let cmp_val = if uses_src_reg {
                                self.registers[src as usize]
                            } else {
                                imm as u64
                            };
                            if (self.registers[dst as usize] & cmp_val) != 0 {
                                pc = checked_jump_target(pc, off, self.program.len())
                                    .ok_or(EbpfError::InvalidOpcode)?;
                                continue;
                            }
                        }
                        BPF_JSGT => {
                            let cmp_val = if uses_src_reg {
                                self.registers[src as usize] as i64
                            } else {
                                imm as i64
                            };
                            if (self.registers[dst as usize] as i64) > cmp_val {
                                pc = checked_jump_target(pc, off, self.program.len())
                                    .ok_or(EbpfError::InvalidOpcode)?;
                                continue;
                            }
                        }
                        BPF_JSGE => {
                            let cmp_val = if uses_src_reg {
                                self.registers[src as usize] as i64
                            } else {
                                imm as i64
                            };
                            if (self.registers[dst as usize] as i64) >= cmp_val {
                                pc = checked_jump_target(pc, off, self.program.len())
                                    .ok_or(EbpfError::InvalidOpcode)?;
                                continue;
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
            let access_size = access_size_bytes(size).ok_or(EbpfError::MemoryAccess)?;
            let end = offset
                .checked_add(access_size)
                .ok_or(EbpfError::MemoryAccess)?;
            if end > BPF_STACK_SIZE {
                return Err(EbpfError::MemoryAccess);
            }

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
            let access_size = access_size_bytes(size).ok_or(EbpfError::MemoryAccess)?;
            let end = offset
                .checked_add(access_size)
                .ok_or(EbpfError::MemoryAccess)?;
            if end > BPF_STACK_SIZE {
                return Err(EbpfError::MemoryAccess);
            }

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
    attachments: BTreeMap<String, String>,
}

fn elf_section_matches_prog_type(name: &str, prog_type: u32) -> bool {
    match name {
        ".text" => true,
        "socket_filter" => prog_type == BPF_PROG_TYPE_SOCKET_FILTER,
        "classifier" | "sched_cls" => prog_type == BPF_PROG_TYPE_SCHED_CLS,
        "sched_act" => prog_type == BPF_PROG_TYPE_SCHED_ACT,
        "xdp" => prog_type == BPF_PROG_TYPE_XDP,
        "flow_dissector" => prog_type == BPF_PROG_TYPE_FLOW_DISSECTOR,
        "reuseport" => prog_type == BPF_PROG_TYPE_SK_REUSEPORT,
        "kprobe" => prog_type == BPF_PROG_TYPE_KPROBE,
        "tracepoint" => prog_type == BPF_PROG_TYPE_TRACEPOINT,
        "perf_event" => prog_type == BPF_PROG_TYPE_PERF_EVENT,
        "raw_tracepoint" | "raw_tracepoint_writable" => matches!(
            prog_type,
            BPF_PROG_TYPE_RAW_TRACEPOINT | BPF_PROG_TYPE_RAW_TRACEPOINT_WRITABLE
        ),
        "cgroup_skb" => prog_type == BPF_PROG_TYPE_CGROUP_SKB,
        "cgroup_sock" => prog_type == BPF_PROG_TYPE_CGROUP_SOCK,
        "cgroup_sock_addr" => prog_type == BPF_PROG_TYPE_CGROUP_SOCK_ADDR,
        "cgroup_device" => prog_type == BPF_PROG_TYPE_CGROUP_DEVICE,
        "cgroup_sysctl" => prog_type == BPF_PROG_TYPE_CGROUP_SYSCTL,
        "cgroup_sockopt" => prog_type == BPF_PROG_TYPE_CGROUP_SOCKOPT,
        "lwt_in" => prog_type == BPF_PROG_TYPE_LWT_IN,
        "lwt_out" => prog_type == BPF_PROG_TYPE_LWT_OUT,
        "lwt_xmit" => prog_type == BPF_PROG_TYPE_LWT_XMIT,
        "lwt_seg6local" => prog_type == BPF_PROG_TYPE_LWT_SEG6LOCAL,
        "lirc_mode2" => prog_type == BPF_PROG_TYPE_LIRC_MODE2,
        "sock_ops" => prog_type == BPF_PROG_TYPE_SOCK_OPS,
        "sock_msg" => prog_type == BPF_PROG_TYPE_SK_MSG,
        "sk_skb" => prog_type == BPF_PROG_TYPE_SK_SKB,
        _ => false,
    }
}

impl EbpfLoader {
    /// Yeni eBPF yükleyici oluştur
    pub fn new() -> Self {
        Self {
            programs: BTreeMap::new(),
            attachments: BTreeMap::new(),
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
                        if !elf_section_matches_prog_type(name, prog_type) {
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
        crate::serial_println!("[eBPF] ELF loader found no supported executable section");

        Err(EbpfError::InvalidElf)
    }

    /// Programı çalıştır
    pub fn execute_program(&mut self, prog_id: &str, ctx: *const u8) -> Result<u64, EbpfError> {
        let vm = self.programs.get_mut(prog_id).ok_or(EbpfError::CallError)?;
        vm.execute(ctx)
    }

    pub fn attach_program(&mut self, attach_point: &str, prog_id: &str) -> Result<(), EbpfError> {
        let vm = self.programs.get(prog_id).ok_or(EbpfError::CallError)?;
        let attach_point = normalize_attach_point(attach_point);
        if !attach_point_accepts_prog_type(attach_point, vm.prog_type) {
            return Err(EbpfError::VerifierRejected);
        }
        self.attachments
            .insert(attach_point.to_string(), prog_id.to_string());
        Ok(())
    }

    pub fn attach_socket_filter(
        &mut self,
        attach_point: &str,
        prog_id: &str,
    ) -> Result<(), EbpfError> {
        self.attach_program(attach_point, prog_id)
    }

    pub fn detach_socket_filter(&mut self, attach_point: &str) -> Result<(), EbpfError> {
        self.attachments
            .remove(normalize_attach_point(attach_point))
            .map(|_| ())
            .ok_or(EbpfError::CallError)
    }

    pub fn run_attached_program(
        &mut self,
        attach_point: &str,
        packet: &[u8],
    ) -> Result<u64, EbpfError> {
        let prog_id = lookup_attached_program_id(&self.attachments, attach_point)
            .ok_or(EbpfError::CallError)?;
        self.execute_program(&prog_id, packet.as_ptr())
    }

    pub fn run_socket_filter(
        &mut self,
        attach_point: &str,
        packet: &[u8],
    ) -> Result<u64, EbpfError> {
        self.run_attached_program(attach_point, packet)
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
    static ref EBPF_PROG_TYPE_REGISTRY: Mutex<BTreeMap<u32, bool>> =
        Mutex::new(BTreeMap::new());
    static ref EBPF_ATTACH_REGISTRY: Mutex<BTreeMap<String, EbpfAttachVerifier>> =
        Mutex::new(BTreeMap::new());
    static ref EBPF_ATTACH_FAMILY_REGISTRY: Mutex<BTreeMap<String, EbpfAttachVerifier>> =
        Mutex::new(BTreeMap::new());
}

pub fn register_program_type(prog_type: u32) {
    EBPF_PROG_TYPE_REGISTRY.lock().insert(prog_type, true);
}

pub fn register_attach_point(attach_point: &str, verifier: EbpfAttachVerifier) {
    EBPF_ATTACH_REGISTRY
        .lock()
        .insert(normalize_attach_point(attach_point).to_string(), verifier);
}

pub fn register_attach_family(attach_family: &str, verifier: EbpfAttachVerifier) {
    let family_key = if attach_family == "*" {
        "*".to_string()
    } else {
        normalize_attach_point(attach_family).to_string()
    };
    EBPF_ATTACH_FAMILY_REGISTRY
        .lock()
        .insert(family_key, verifier);
}

fn prog_type_is_registered(prog_type: u32) -> bool {
    EBPF_PROG_TYPE_REGISTRY
        .lock()
        .get(&prog_type)
        .copied()
        .unwrap_or(false)
}

fn lookup_attach_verifier(attach_point: &str) -> Option<EbpfAttachVerifier> {
    let normalized = normalize_attach_point(attach_point);
    if normalized == "*" {
        return Some(generic_attach_accepts_prog_type);
    }
    if let Some(verifier) = EBPF_ATTACH_REGISTRY.lock().get(normalized).copied() {
        return Some(verifier);
    }

    let families = EBPF_ATTACH_FAMILY_REGISTRY.lock();
    let mut best_match: Option<(&str, EbpfAttachVerifier)> = None;
    let family_candidates = [
        normalized,
        normalized.split('/').next().unwrap_or(normalized),
        normalized.split(':').next().unwrap_or(normalized),
        "*",
    ];
    for candidate in family_candidates {
        if let Some(verifier) = families.get(candidate).copied() {
            if best_match
                .as_ref()
                .map(|(best, _)| candidate.len() > best.len())
                .unwrap_or(true)
            {
                best_match = Some((candidate, verifier));
            }
        }
    }
    best_match
        .map(|(_, verifier)| verifier)
        .or(Some(generic_attach_accepts_prog_type))
}

fn lookup_attached_program_id(
    attachments: &BTreeMap<String, String>,
    attach_point: &str,
) -> Option<String> {
    let normalized = normalize_attach_point(attach_point);
    if let Some(prog_id) = attachments.get(normalized).cloned() {
        return Some(prog_id);
    }

    let candidates = [
        normalized.split('/').next().unwrap_or(normalized),
        normalized.split(':').next().unwrap_or(normalized),
        "*",
    ];
    let mut best_match: Option<(&str, String)> = None;
    for candidate in candidates {
        if let Some(prog_id) = attachments.get(candidate).cloned() {
            if best_match
                .as_ref()
                .map(|(best, _)| candidate.len() > best.len())
                .unwrap_or(true)
            {
                best_match = Some((candidate, prog_id));
            }
        }
    }
    best_match.map(|(_, prog_id)| prog_id)
}

fn generic_attach_accepts_prog_type(prog_type: u32) -> bool {
    let _ = prog_type;
    true
}

pub fn attach_ingress_program(prog_id: &str, program: Vec<u64>) -> Result<(), EbpfError> {
    let mut loader = GLOBAL_EBPF_LOADER.lock();
    loader.load_program(prog_id, program, BPF_PROG_TYPE_SOCKET_FILTER)?;
    loader.attach_program("net:ingress", prog_id)
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
        Err(EbpfError::CallError) => Ok(true),
        Err(err) => Err(err),
    }
}

pub fn attach_egress_program(prog_id: &str, program: Vec<u64>) -> Result<(), EbpfError> {
    let mut loader = GLOBAL_EBPF_LOADER.lock();
    loader.load_program(prog_id, program, BPF_PROG_TYPE_SOCKET_FILTER)?;
    loader.attach_program("net:egress", prog_id)
}

pub fn detach_egress_program() -> Result<(), EbpfError> {
    GLOBAL_EBPF_LOADER.lock().detach_socket_filter("net:egress")
}

pub fn filter_egress_packet(packet: &[u8]) -> Result<bool, EbpfError> {
    let mut loader = GLOBAL_EBPF_LOADER.lock();
    match loader.run_socket_filter("net:egress", packet) {
        Ok(verdict) => Ok(verdict != 0),
        Err(EbpfError::CallError) => Ok(true),
        Err(err) => Err(err),
    }
}

pub fn attach_classifier_program(prog_id: &str, program: Vec<u64>) -> Result<(), EbpfError> {
    let mut loader = GLOBAL_EBPF_LOADER.lock();
    loader.load_program(prog_id, program, BPF_PROG_TYPE_SCHED_CLS)?;
    loader.attach_program("net:classifier", prog_id)
}

pub fn run_classifier(packet: &[u8]) -> Result<u64, EbpfError> {
    let mut loader = GLOBAL_EBPF_LOADER.lock();
    loader.run_attached_program("net:classifier", packet)
}

pub fn attach_xdp_program(prog_id: &str, program: Vec<u64>) -> Result<(), EbpfError> {
    let mut loader = GLOBAL_EBPF_LOADER.lock();
    loader.load_program(prog_id, program, BPF_PROG_TYPE_XDP)?;
    loader.attach_program("net:xdp", prog_id)
}

pub fn run_xdp(packet: &[u8]) -> Result<u64, EbpfError> {
    let mut loader = GLOBAL_EBPF_LOADER.lock();
    loader.run_attached_program("net:xdp", packet)
}

pub fn attach_reuseport_program(prog_id: &str, program: Vec<u64>) -> Result<(), EbpfError> {
    let mut loader = GLOBAL_EBPF_LOADER.lock();
    loader.load_program(prog_id, program, BPF_PROG_TYPE_SK_REUSEPORT)?;
    loader.attach_program("net:reuseport", prog_id)
}

pub fn run_reuseport(packet: &[u8]) -> Result<u64, EbpfError> {
    let mut loader = GLOBAL_EBPF_LOADER.lock();
    loader.run_attached_program("net:reuseport", packet)
}

pub fn attach_flow_dissector_program(prog_id: &str, program: Vec<u64>) -> Result<(), EbpfError> {
    let mut loader = GLOBAL_EBPF_LOADER.lock();
    loader.load_program(prog_id, program, BPF_PROG_TYPE_FLOW_DISSECTOR)?;
    loader.attach_program("net:flow-dissector", prog_id)
}

pub fn run_flow_dissector(packet: &[u8]) -> Result<u64, EbpfError> {
    let mut loader = GLOBAL_EBPF_LOADER.lock();
    loader.run_attached_program("net:flow-dissector", packet)
}

pub fn attach_trace_program(
    prog_id: &str,
    program: Vec<u64>,
    prog_type: u32,
    attach_point: &str,
) -> Result<(), EbpfError> {
    let mut loader = GLOBAL_EBPF_LOADER.lock();
    loader.load_program(prog_id, program, prog_type)?;
    loader.attach_program(attach_point, prog_id)
}

pub fn run_trace_program(attach_point: &str, ctx: &[u8]) -> Result<u64, EbpfError> {
    let mut loader = GLOBAL_EBPF_LOADER.lock();
    loader.run_attached_program(attach_point, ctx)
}

pub fn attach_cgroup_program(
    prog_id: &str,
    program: Vec<u64>,
    prog_type: u32,
    attach_point: &str,
) -> Result<(), EbpfError> {
    attach_trace_program(prog_id, program, prog_type, attach_point)
}

pub fn run_cgroup_program(attach_point: &str, ctx: &[u8]) -> Result<u64, EbpfError> {
    run_trace_program(attach_point, ctx)
}

pub fn attach_lwt_program(
    prog_id: &str,
    program: Vec<u64>,
    prog_type: u32,
    attach_point: &str,
) -> Result<(), EbpfError> {
    attach_trace_program(prog_id, program, prog_type, attach_point)
}

pub fn run_lwt_program(attach_point: &str, packet: &[u8]) -> Result<u64, EbpfError> {
    run_trace_program(attach_point, packet)
}

pub fn attach_lirc_program(prog_id: &str, program: Vec<u64>) -> Result<(), EbpfError> {
    attach_trace_program(prog_id, program, BPF_PROG_TYPE_LIRC_MODE2, "lirc:mode2")
}

pub fn run_lirc_program(ctx: &[u8]) -> Result<u64, EbpfError> {
    run_trace_program("lirc:mode2", ctx)
}

pub fn attach_sock_program(
    prog_id: &str,
    program: Vec<u64>,
    prog_type: u32,
    attach_point: &str,
) -> Result<(), EbpfError> {
    attach_trace_program(prog_id, program, prog_type, attach_point)
}

pub fn run_sock_program(attach_point: &str, ctx: &[u8]) -> Result<u64, EbpfError> {
    run_trace_program(attach_point, ctx)
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

fn checked_jump_target(pc: usize, off: i16, program_len: usize) -> Option<usize> {
    let base = pc as i64 + 1;
    let target = base.checked_add(off as i64)?;
    if target < 0 || target >= program_len as i64 {
        return None;
    }
    Some(target as usize)
}

fn access_size_bytes(size: u8) -> Option<usize> {
    match size {
        BPF_B => Some(1),
        BPF_H => Some(2),
        BPF_W => Some(4),
        BPF_DW => Some(8),
        _ => None,
    }
}

const VERIFIER_COMPLEXITY_LIMIT: usize = 1_000_000;
const VERIFIER_CTX_MAX_ACCESS: i16 = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VerifierRegKind {
    Uninit,
    Scalar,
    CtxPtr,
    StackPtr,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct VerifierRegState {
    kind: VerifierRegKind,
    stack_offset: i32,
}

const VERIFIER_REG_UNINIT: VerifierRegState = VerifierRegState {
    kind: VerifierRegKind::Uninit,
    stack_offset: 0,
};

const VERIFIER_REG_SCALAR: VerifierRegState = VerifierRegState {
    kind: VerifierRegKind::Scalar,
    stack_offset: 0,
};

const VERIFIER_REG_CTX: VerifierRegState = VerifierRegState {
    kind: VerifierRegKind::CtxPtr,
    stack_offset: 0,
};

const VERIFIER_REG_STACK_FP: VerifierRegState = VerifierRegState {
    kind: VerifierRegKind::StackPtr,
    stack_offset: 0,
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct VerifierState {
    regs: [VerifierRegState; 11],
    stack_init: [bool; BPF_STACK_SIZE],
    stack_spill: [Option<VerifierRegState>; BPF_STACK_SIZE / 8],
}

impl VerifierState {
    fn entry() -> Self {
        let mut regs = [VERIFIER_REG_UNINIT; 11];
        regs[BPF_REG_1 as usize] = VERIFIER_REG_CTX;
        regs[BPF_REG_10 as usize] = VERIFIER_REG_STACK_FP;
        Self {
            regs,
            stack_init: [false; BPF_STACK_SIZE],
            stack_spill: [None; BPF_STACK_SIZE / 8],
        }
    }

    fn merge_from(&mut self, incoming: &Self) -> bool {
        let mut changed = false;

        for idx in 0..self.regs.len() {
            let merged = verifier_merge_reg_state(self.regs[idx], incoming.regs[idx]);
            if merged != self.regs[idx] {
                self.regs[idx] = merged;
                changed = true;
            }
        }

        for idx in 0..self.stack_init.len() {
            let merged = self.stack_init[idx] && incoming.stack_init[idx];
            if merged != self.stack_init[idx] {
                self.stack_init[idx] = merged;
                changed = true;
            }
        }

        for slot in 0..self.stack_spill.len() {
            let merged = match (self.stack_spill[slot], incoming.stack_spill[slot]) {
                (Some(lhs), Some(rhs)) if lhs == rhs => Some(lhs),
                _ => None,
            };
            if merged != self.stack_spill[slot] {
                self.stack_spill[slot] = merged;
                changed = true;
            }
        }

        // R10 stack frame pointer olarak salt-okunur ve sabit kalır.
        if self.regs[BPF_REG_10 as usize] != VERIFIER_REG_STACK_FP {
            self.regs[BPF_REG_10 as usize] = VERIFIER_REG_STACK_FP;
            changed = true;
        }

        if changed {
            self.invalidate_spills_for_uninitialized_bytes();
        }

        changed
    }

    fn reg_readable(&self, reg: usize) -> bool {
        reg < self.regs.len() && self.regs[reg].kind != VerifierRegKind::Uninit
    }

    fn mark_stack_write(&mut self, start: usize, size: usize) {
        for idx in start..start + size {
            self.stack_init[idx] = true;
        }
        self.clear_spills_for_stack_range(start, size);
    }

    fn stack_bytes_initialized(&self, start: usize, size: usize) -> bool {
        self.stack_init[start..start + size].iter().all(|bit| *bit)
    }

    fn clear_spills_for_stack_range(&mut self, start: usize, size: usize) {
        let first = start / 8;
        let last = (start + size - 1) / 8;
        for slot in first..=last {
            self.stack_spill[slot] = None;
        }
    }

    fn invalidate_spills_for_uninitialized_bytes(&mut self) {
        for slot in 0..self.stack_spill.len() {
            let start = slot * 8;
            if !self.stack_init[start..start + 8].iter().all(|bit| *bit) {
                self.stack_spill[slot] = None;
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct DecodedInsn {
    opcode: u8,
    dst: u8,
    src: u8,
    off: i16,
    imm: i32,
    class: u8,
    op: u8,
    uses_src_reg: bool,
}

fn decode_instruction(insn: u64) -> DecodedInsn {
    let opcode = ((insn >> 56) & 0xff) as u8;
    DecodedInsn {
        opcode,
        dst: ((insn >> 48) & 0xff) as u8,
        src: ((insn >> 40) & 0xff) as u8,
        off: ((insn >> 32) & 0xffff) as u16 as i16,
        imm: insn as i32,
        class: opcode & 0x07,
        op: opcode & 0xf0,
        uses_src_reg: (opcode & BPF_SRC_X) == BPF_SRC_X,
    }
}

fn verifier_merge_reg_state(lhs: VerifierRegState, rhs: VerifierRegState) -> VerifierRegState {
    use VerifierRegKind::{CtxPtr, Scalar, StackPtr, Uninit};

    match (lhs.kind, rhs.kind) {
        (Uninit, _) | (_, Uninit) => VERIFIER_REG_UNINIT,
        (Scalar, Scalar) => VERIFIER_REG_SCALAR,
        (CtxPtr, CtxPtr) => VERIFIER_REG_CTX,
        (StackPtr, StackPtr) if lhs.stack_offset == rhs.stack_offset => lhs,
        _ => VERIFIER_REG_UNINIT,
    }
}

fn verifier_stack_access_start(base_stack_offset: i32, off: i16, size: usize) -> Option<usize> {
    let base = BPF_STACK_SIZE as i64;
    let start = base
        .checked_add(base_stack_offset as i64)?
        .checked_add(off as i64)?;
    let end = start.checked_add(size as i64)?;
    if start < 0 || end > BPF_STACK_SIZE as i64 {
        return None;
    }
    let start = start as usize;
    if size > 1 && (start % size) != 0 {
        return None;
    }
    Some(start)
}

fn verifier_ctx_access_valid(off: i16, size: usize, write: bool) -> bool {
    if write || off < 0 {
        return false;
    }
    let start = off as i64;
    let end = match start.checked_add(size as i64) {
        Some(end) => end,
        None => return false,
    };
    if end > VERIFIER_CTX_MAX_ACCESS as i64 {
        return false;
    }
    if size > 1 && (start as usize % size) != 0 {
        return false;
    }
    true
}

fn verifier_is_supported_helper(helper_id: i32) -> bool {
    matches!(helper_id, 1..=3)
}

fn verifier_validate_helper_args(state: &VerifierState, helper_id: i32) -> Result<(), EbpfError> {
    match helper_id {
        1 => {
            if !state.reg_readable(BPF_REG_1 as usize) {
                return Err(EbpfError::VerifierRejected);
            }
        }
        2 | 3 => {}
        _ => return Err(EbpfError::CallError),
    }
    Ok(())
}

fn verifier_check_register_bounds(insn: DecodedInsn) -> Result<(), EbpfError> {
    if insn.dst > BPF_REG_10 || insn.src > BPF_REG_10 {
        return Err(EbpfError::InvalidRegister);
    }
    Ok(())
}

fn verifier_check_opcode_support(insn: DecodedInsn) -> Result<(), EbpfError> {
    match insn.class {
        BPF_ALU => match insn.op {
            BPF_ADD | BPF_SUB | BPF_MUL | BPF_DIV | BPF_OR | BPF_AND | BPF_LSH | BPF_RSH
            | BPF_NEG | BPF_MOD | BPF_XOR | BPF_MOV | BPF_ARSH => Ok(()),
            _ => Err(EbpfError::InvalidOpcode),
        },
        BPF_LDX | BPF_STX => {
            if access_size_bytes(insn.opcode & 0x18).is_none() {
                return Err(EbpfError::InvalidOpcode);
            }
            Ok(())
        }
        BPF_JMP => {
            match insn.op {
                BPF_JA | BPF_JEQ | BPF_JGT | BPF_JGE | BPF_JSET | BPF_JNE | BPF_JSGT | BPF_JSGE
                | BPF_CALL | BPF_EXIT => {}
                _ => return Err(EbpfError::InvalidOpcode),
            }

            if insn.op == BPF_CALL && insn.off != 0 {
                return Err(EbpfError::VerifierRejected);
            }

            if insn.op == BPF_EXIT
                && (insn.dst != 0 || insn.src != 0 || insn.off != 0 || insn.imm != 0)
            {
                return Err(EbpfError::VerifierRejected);
            }

            Ok(())
        }
        _ => Err(EbpfError::InvalidOpcode),
    }
}

fn verifier_build_cfg_successors(
    decoded: &[DecodedInsn],
) -> Result<Vec<[Option<usize>; 2]>, EbpfError> {
    let mut successors = vec![[None, None]; decoded.len()];

    for (pc, insn) in decoded.iter().copied().enumerate() {
        if insn.class == BPF_JMP {
            match insn.op {
                BPF_EXIT => {
                    successors[pc] = [None, None];
                }
                BPF_CALL => {
                    let next = pc.checked_add(1).filter(|idx| *idx < decoded.len());
                    if next.is_none() {
                        return Err(EbpfError::VerifierRejected);
                    }
                    successors[pc] = [next, None];
                }
                BPF_JA => {
                    let target = checked_jump_target(pc, insn.off, decoded.len())
                        .ok_or(EbpfError::VerifierRejected)?;
                    successors[pc] = [Some(target), None];
                }
                _ => {
                    let target = checked_jump_target(pc, insn.off, decoded.len())
                        .ok_or(EbpfError::VerifierRejected)?;
                    let fallthrough = pc.checked_add(1).filter(|idx| *idx < decoded.len());
                    if fallthrough.is_none() {
                        return Err(EbpfError::VerifierRejected);
                    }
                    successors[pc] = [fallthrough, Some(target)];
                }
            }
        } else {
            let next = pc.checked_add(1).filter(|idx| *idx < decoded.len());
            if next.is_none() {
                return Err(EbpfError::VerifierRejected);
            }
            successors[pc] = [next, None];
        }
    }

    Ok(successors)
}

fn verifier_reachable_mask(successors: &[[Option<usize>; 2]]) -> Vec<bool> {
    let mut reachable = vec![false; successors.len()];
    let mut stack = vec![0usize];

    while let Some(pc) = stack.pop() {
        if reachable[pc] {
            continue;
        }
        reachable[pc] = true;
        for next in successors[pc].iter().flatten() {
            stack.push(*next);
        }
    }

    reachable
}

fn verifier_can_reach_exit_mask(
    successors: &[[Option<usize>; 2]],
    reachable: &[bool],
    exits: &[usize],
) -> Vec<bool> {
    let mut reverse = vec![Vec::new(); successors.len()];
    for (pc, edges) in successors.iter().enumerate() {
        if !reachable[pc] {
            continue;
        }
        for next in edges.iter().flatten() {
            if reachable[*next] {
                reverse[*next].push(pc);
            }
        }
    }

    let mut can_reach_exit = vec![false; successors.len()];
    let mut stack = exits.to_vec();
    while let Some(pc) = stack.pop() {
        if can_reach_exit[pc] {
            continue;
        }
        can_reach_exit[pc] = true;
        for prev in reverse[pc].iter().copied() {
            stack.push(prev);
        }
    }

    can_reach_exit
}

fn verifier_apply_alu(insn: DecodedInsn, state: &mut VerifierState) -> Result<(), EbpfError> {
    let dst = insn.dst as usize;
    let src = insn.src as usize;

    if dst == BPF_REG_10 as usize {
        return Err(EbpfError::VerifierRejected);
    }

    if insn.op == BPF_MOV {
        if insn.uses_src_reg {
            if !state.reg_readable(src) {
                return Err(EbpfError::VerifierRejected);
            }
            state.regs[dst] = state.regs[src];
        } else {
            state.regs[dst] = VERIFIER_REG_SCALAR;
        }
        return Ok(());
    }

    if !state.reg_readable(dst) {
        return Err(EbpfError::VerifierRejected);
    }

    if insn.uses_src_reg {
        if !state.reg_readable(src) {
            return Err(EbpfError::VerifierRejected);
        }
        if state.regs[dst].kind == VerifierRegKind::StackPtr
            || state.regs[dst].kind == VerifierRegKind::CtxPtr
        {
            return Err(EbpfError::VerifierRejected);
        }
    } else if insn.op == BPF_DIV || insn.op == BPF_MOD {
        if insn.imm == 0 {
            return Err(EbpfError::VerifierRejected);
        }
    }

    match state.regs[dst].kind {
        VerifierRegKind::StackPtr => {
            if !matches!(insn.op, BPF_ADD | BPF_SUB) || insn.uses_src_reg {
                return Err(EbpfError::VerifierRejected);
            }

            let delta = if insn.op == BPF_ADD {
                insn.imm as i64
            } else {
                -(insn.imm as i64)
            };
            let next_offset = (state.regs[dst].stack_offset as i64)
                .checked_add(delta)
                .ok_or(EbpfError::VerifierRejected)?;
            if next_offset < i32::MIN as i64 || next_offset > i32::MAX as i64 {
                return Err(EbpfError::VerifierRejected);
            }
            state.regs[dst].stack_offset = next_offset as i32;
        }
        VerifierRegKind::CtxPtr => {
            return Err(EbpfError::VerifierRejected);
        }
        VerifierRegKind::Scalar => {
            state.regs[dst] = VERIFIER_REG_SCALAR;
        }
        VerifierRegKind::Uninit => {
            return Err(EbpfError::VerifierRejected);
        }
    }

    Ok(())
}

fn verifier_apply_ldx(insn: DecodedInsn, state: &mut VerifierState) -> Result<(), EbpfError> {
    let dst = insn.dst as usize;
    let src = insn.src as usize;
    let size = access_size_bytes(insn.opcode & 0x18).ok_or(EbpfError::InvalidOpcode)?;

    if dst == BPF_REG_10 as usize || !state.reg_readable(src) {
        return Err(EbpfError::VerifierRejected);
    }

    match state.regs[src].kind {
        VerifierRegKind::StackPtr => {
            let start = verifier_stack_access_start(state.regs[src].stack_offset, insn.off, size)
                .ok_or(EbpfError::MemoryAccess)?;
            if !state.stack_bytes_initialized(start, size) {
                return Err(EbpfError::VerifierRejected);
            }

            if size == 8 && (start % 8) == 0 {
                let slot = start / 8;
                if let Some(spilled) = state.stack_spill[slot] {
                    state.regs[dst] = spilled;
                } else {
                    state.regs[dst] = VERIFIER_REG_SCALAR;
                }
            } else {
                state.regs[dst] = VERIFIER_REG_SCALAR;
            }
            Ok(())
        }
        VerifierRegKind::CtxPtr => {
            if !verifier_ctx_access_valid(insn.off, size, false) {
                return Err(EbpfError::VerifierRejected);
            }
            state.regs[dst] = VERIFIER_REG_SCALAR;
            Ok(())
        }
        _ => Err(EbpfError::VerifierRejected),
    }
}

fn verifier_apply_stx(insn: DecodedInsn, state: &mut VerifierState) -> Result<(), EbpfError> {
    let dst = insn.dst as usize;
    let src = insn.src as usize;
    let size = access_size_bytes(insn.opcode & 0x18).ok_or(EbpfError::InvalidOpcode)?;

    if !state.reg_readable(src) || !state.reg_readable(dst) {
        return Err(EbpfError::VerifierRejected);
    }
    if state.regs[dst].kind != VerifierRegKind::StackPtr {
        return Err(EbpfError::VerifierRejected);
    }

    let start = verifier_stack_access_start(state.regs[dst].stack_offset, insn.off, size)
        .ok_or(EbpfError::MemoryAccess)?;

    state.mark_stack_write(start, size);
    if size == 8 && (start % 8) == 0 {
        state.stack_spill[start / 8] = Some(state.regs[src]);
    }

    Ok(())
}

fn verifier_transfer_jump(
    insn: DecodedInsn,
    state: &VerifierState,
    successors: [Option<usize>; 2],
) -> Result<Vec<(usize, VerifierState)>, EbpfError> {
    let dst = insn.dst as usize;
    let src = insn.src as usize;

    match insn.op {
        BPF_EXIT => {
            if !state.reg_readable(BPF_REG_0 as usize)
                || state.regs[BPF_REG_0 as usize].kind != VerifierRegKind::Scalar
            {
                return Err(EbpfError::VerifierRejected);
            }
            Ok(Vec::new())
        }
        BPF_CALL => {
            if !verifier_is_supported_helper(insn.imm) {
                return Err(EbpfError::CallError);
            }
            verifier_validate_helper_args(state, insn.imm)?;

            let mut next = state.clone();
            for reg in BPF_REG_1 as usize..=BPF_REG_5 as usize {
                next.regs[reg] = VERIFIER_REG_UNINIT;
            }
            next.regs[BPF_REG_0 as usize] = VERIFIER_REG_SCALAR;

            let next_pc = successors[0].ok_or(EbpfError::VerifierRejected)?;
            Ok(vec![(next_pc, next)])
        }
        BPF_JA | BPF_JEQ | BPF_JGT | BPF_JGE | BPF_JSET | BPF_JNE | BPF_JSGT | BPF_JSGE => {
            if insn.op != BPF_JA {
                if !state.reg_readable(dst) {
                    return Err(EbpfError::VerifierRejected);
                }
                if insn.uses_src_reg && !state.reg_readable(src) {
                    return Err(EbpfError::VerifierRejected);
                }
            }

            let mut out = Vec::with_capacity(2);
            if let Some(next_pc) = successors[0] {
                out.push((next_pc, state.clone()));
            }
            if let Some(next_pc) = successors[1] {
                out.push((next_pc, state.clone()));
            }
            Ok(out)
        }
        _ => Err(EbpfError::InvalidOpcode),
    }
}

fn verifier_transfer_instruction(
    insn: DecodedInsn,
    state: &VerifierState,
    successors: [Option<usize>; 2],
) -> Result<Vec<(usize, VerifierState)>, EbpfError> {
    let mut next = state.clone();

    match insn.class {
        BPF_ALU => {
            verifier_apply_alu(insn, &mut next)?;
            let next_pc = successors[0].ok_or(EbpfError::VerifierRejected)?;
            Ok(vec![(next_pc, next)])
        }
        BPF_LDX => {
            verifier_apply_ldx(insn, &mut next)?;
            let next_pc = successors[0].ok_or(EbpfError::VerifierRejected)?;
            Ok(vec![(next_pc, next)])
        }
        BPF_STX => {
            verifier_apply_stx(insn, &mut next)?;
            let next_pc = successors[0].ok_or(EbpfError::VerifierRejected)?;
            Ok(vec![(next_pc, next)])
        }
        BPF_JMP => verifier_transfer_jump(insn, state, successors),
        _ => Err(EbpfError::InvalidOpcode),
    }
}

fn verify_program(program: &[u64], prog_type: u32) -> Result<(), EbpfError> {
    if program.is_empty() || program.len() > BPF_MAXINSNS {
        return Err(EbpfError::VerifierRejected);
    }

    let _ = prog_type;

    let mut decoded = Vec::with_capacity(program.len());
    for raw in program.iter().copied() {
        let insn = decode_instruction(raw);
        verifier_check_register_bounds(insn)?;
        verifier_check_opcode_support(insn)?;
        decoded.push(insn);
    }

    let successors = verifier_build_cfg_successors(&decoded)?;
    let reachable = verifier_reachable_mask(&successors);
    if reachable.iter().any(|bit| !*bit) {
        return Err(EbpfError::VerifierRejected);
    }

    let exits: Vec<usize> = decoded
        .iter()
        .enumerate()
        .filter_map(|(pc, insn)| {
            if reachable[pc] && insn.class == BPF_JMP && insn.op == BPF_EXIT {
                Some(pc)
            } else {
                None
            }
        })
        .collect();
    if exits.is_empty() {
        return Err(EbpfError::VerifierRejected);
    }

    let can_reach_exit = verifier_can_reach_exit_mask(&successors, &reachable, &exits);
    if reachable
        .iter()
        .enumerate()
        .any(|(pc, is_reachable)| *is_reachable && !can_reach_exit[pc])
    {
        return Err(EbpfError::VerifierRejected);
    }

    let mut in_states: Vec<Option<VerifierState>> = vec![None; decoded.len()];
    let mut worklist: Vec<(usize, VerifierState)> = vec![(0, VerifierState::entry())];
    let mut processed = 0usize;

    while let Some((pc, incoming)) = worklist.pop() {
        processed = processed
            .checked_add(1)
            .ok_or(EbpfError::VerifierRejected)?;
        if processed > VERIFIER_COMPLEXITY_LIMIT {
            return Err(EbpfError::InfiniteLoop);
        }

        let current = if let Some(existing) = in_states[pc].as_mut() {
            if !existing.merge_from(&incoming) {
                continue;
            }
            existing.clone()
        } else {
            in_states[pc] = Some(incoming);
            in_states[pc]
                .as_ref()
                .cloned()
                .ok_or(EbpfError::VerifierRejected)?
        };

        let next = verifier_transfer_instruction(decoded[pc], &current, successors[pc])?;
        for item in next {
            worklist.push(item);
        }
    }

    if reachable
        .iter()
        .enumerate()
        .any(|(pc, is_reachable)| *is_reachable && in_states[pc].is_none())
    {
        return Err(EbpfError::VerifierRejected);
    }

    Ok(())
}

fn is_supported_packet_prog_type(prog_type: u32) -> bool {
    matches!(
        prog_type,
        BPF_PROG_TYPE_SOCKET_FILTER
            | BPF_PROG_TYPE_SCHED_CLS
            | BPF_PROG_TYPE_SCHED_ACT
            | BPF_PROG_TYPE_XDP
            | BPF_PROG_TYPE_KPROBE
            | BPF_PROG_TYPE_TRACEPOINT
            | BPF_PROG_TYPE_PERF_EVENT
            | BPF_PROG_TYPE_CGROUP_SKB
            | BPF_PROG_TYPE_CGROUP_SOCK
            | BPF_PROG_TYPE_LWT_IN
            | BPF_PROG_TYPE_LWT_OUT
            | BPF_PROG_TYPE_LWT_XMIT
            | BPF_PROG_TYPE_SOCK_OPS
            | BPF_PROG_TYPE_SK_SKB
            | BPF_PROG_TYPE_SK_MSG
            | BPF_PROG_TYPE_CGROUP_DEVICE
            | BPF_PROG_TYPE_RAW_TRACEPOINT
            | BPF_PROG_TYPE_CGROUP_SOCK_ADDR
            | BPF_PROG_TYPE_LWT_SEG6LOCAL
            | BPF_PROG_TYPE_LIRC_MODE2
            | BPF_PROG_TYPE_FLOW_DISSECTOR
            | BPF_PROG_TYPE_SK_REUSEPORT
            | BPF_PROG_TYPE_CGROUP_SYSCTL
            | BPF_PROG_TYPE_RAW_TRACEPOINT_WRITABLE
            | BPF_PROG_TYPE_CGROUP_SOCKOPT
    ) || prog_type_is_registered(prog_type)
}

fn trace_prog_type(prog_type: u32) -> bool {
    matches!(
        prog_type,
        BPF_PROG_TYPE_KPROBE
            | BPF_PROG_TYPE_TRACEPOINT
            | BPF_PROG_TYPE_PERF_EVENT
            | BPF_PROG_TYPE_RAW_TRACEPOINT
            | BPF_PROG_TYPE_RAW_TRACEPOINT_WRITABLE
    )
}

fn cgroup_prog_type(prog_type: u32) -> bool {
    matches!(
        prog_type,
        BPF_PROG_TYPE_CGROUP_SKB
            | BPF_PROG_TYPE_CGROUP_SOCK
            | BPF_PROG_TYPE_CGROUP_SOCK_ADDR
            | BPF_PROG_TYPE_CGROUP_DEVICE
            | BPF_PROG_TYPE_CGROUP_SYSCTL
            | BPF_PROG_TYPE_CGROUP_SOCKOPT
    )
}

fn lwt_prog_type(prog_type: u32) -> bool {
    matches!(
        prog_type,
        BPF_PROG_TYPE_LWT_IN
            | BPF_PROG_TYPE_LWT_OUT
            | BPF_PROG_TYPE_LWT_XMIT
            | BPF_PROG_TYPE_LWT_SEG6LOCAL
    )
}

fn sock_prog_type(prog_type: u32) -> bool {
    matches!(
        prog_type,
        BPF_PROG_TYPE_SOCK_OPS | BPF_PROG_TYPE_SK_MSG | BPF_PROG_TYPE_SK_SKB
    )
}

fn normalize_attach_point(attach_point: &str) -> &str {
    match attach_point {
        "ingress" | "net:rx" | "socket:ingress" => "net:ingress",
        "egress" | "net:tx" | "socket:egress" => "net:egress",
        "classifier" | "tc:classifier" | "tc:ingress" => "net:classifier",
        "xdp" | "xdp:ingress" | "driver:xdp" => "net:xdp",
        "reuseport" | "socket:reuseport" => "net:reuseport",
        "flow-dissector" | "flow:dissector" => "net:flow-dissector",
        "tracepoint" | "trace:tp" => "trace:tracepoint",
        "kprobe" | "trace:kp" => "trace:kprobe",
        "perf" | "trace:perf-event" => "trace:perf",
        "raw-tracepoint" | "trace:raw-tracepoint" => "trace:raw",
        "cgroup-skb" | "cgroup:skb/ingress" | "cgroup:skb/egress" => "cgroup:skb",
        "cgroup-sock" | "cgroup:sock/create" => "cgroup:sock",
        "cgroup-sock-addr" | "cgroup:connect4" | "cgroup:connect6" => "cgroup:sock-addr",
        "cgroup-device" => "cgroup:device",
        "cgroup-sysctl" => "cgroup:sysctl",
        "cgroup-sockopt" => "cgroup:sockopt",
        "lwt-in" | "lwt:ingress" => "lwt:in",
        "lwt-out" | "lwt:egress" => "lwt:out",
        "lwt-xmit" | "lwt:transmit" => "lwt:xmit",
        "lwt-seg6local" | "lwt:seg6" => "lwt:seg6local",
        "lirc-mode2" | "lirc:rx" => "lirc:mode2",
        "sock-ops" | "sock:operations" => "sock:ops",
        "sock-msg" | "sock:message" => "sock:msg",
        "sk-skb" | "sock:skb-stream" => "sock:skb",
        other => other,
    }
}

fn attach_point_accepts_prog_type(attach_point: &str, prog_type: u32) -> bool {
    let attach_point = if attach_point.starts_with("test:") {
        normalize_attach_point(match attach_point.trim_start_matches("test:") {
            "ingress" => "net:ingress",
            "egress" => "net:egress",
            "classifier" => "net:classifier",
            "xdp" => "net:xdp",
            "reuseport" => "net:reuseport",
            "flow-dissector" => "net:flow-dissector",
            "kprobe" => "trace:kprobe",
            "tracepoint" => "trace:tracepoint",
            "perf" => "trace:perf",
            "raw-tracepoint" => "trace:raw",
            "cgroup-skb" => "cgroup:skb",
            "cgroup-sock" => "cgroup:sock",
            "cgroup-sock-addr" => "cgroup:sock-addr",
            "cgroup-device" => "cgroup:device",
            "cgroup-sysctl" => "cgroup:sysctl",
            "cgroup-sockopt" => "cgroup:sockopt",
            "lwt-in" => "lwt:in",
            "lwt-out" => "lwt:out",
            "lwt-xmit" => "lwt:xmit",
            "lwt-seg6local" => "lwt:seg6local",
            "lirc-mode2" => "lirc:mode2",
            "sock-ops" => "sock:ops",
            "sock-msg" => "sock:msg",
            "sk-skb" => "sock:skb",
            other => other,
        })
    } else {
        normalize_attach_point(attach_point)
    };
    if attach_point.starts_with("trace:") && trace_prog_type(prog_type) {
        return true;
    }
    if attach_point.starts_with("cgroup:") && cgroup_prog_type(prog_type) {
        return true;
    }
    if attach_point.starts_with("lwt:") && lwt_prog_type(prog_type) {
        return true;
    }
    if attach_point.starts_with("sock:") && sock_prog_type(prog_type) {
        return true;
    }
    if attach_point.starts_with("lirc:") && prog_type == BPF_PROG_TYPE_LIRC_MODE2 {
        return true;
    }
    if attach_point.starts_with("socket:") && prog_type == BPF_PROG_TYPE_SOCKET_FILTER {
        return true;
    }
    let builtin_match = match attach_point {
        "net:ingress" | "net:egress" => matches!(
            prog_type,
            BPF_PROG_TYPE_SOCKET_FILTER | BPF_PROG_TYPE_SCHED_ACT | BPF_PROG_TYPE_SCHED_CLS
        ),
        "net:classifier" => prog_type == BPF_PROG_TYPE_SCHED_CLS,
        "net:xdp" => prog_type == BPF_PROG_TYPE_XDP,
        "net:reuseport" => prog_type == BPF_PROG_TYPE_SK_REUSEPORT,
        "net:flow-dissector" => prog_type == BPF_PROG_TYPE_FLOW_DISSECTOR,
        "trace:kprobe" => prog_type == BPF_PROG_TYPE_KPROBE,
        "trace:tracepoint" => prog_type == BPF_PROG_TYPE_TRACEPOINT,
        "trace:perf" => prog_type == BPF_PROG_TYPE_PERF_EVENT,
        "trace:raw" => matches!(
            prog_type,
            BPF_PROG_TYPE_RAW_TRACEPOINT | BPF_PROG_TYPE_RAW_TRACEPOINT_WRITABLE
        ),
        "cgroup:skb" => prog_type == BPF_PROG_TYPE_CGROUP_SKB,
        "cgroup:sock" => prog_type == BPF_PROG_TYPE_CGROUP_SOCK,
        "cgroup:sock-addr" => prog_type == BPF_PROG_TYPE_CGROUP_SOCK_ADDR,
        "cgroup:device" => prog_type == BPF_PROG_TYPE_CGROUP_DEVICE,
        "cgroup:sysctl" => prog_type == BPF_PROG_TYPE_CGROUP_SYSCTL,
        "cgroup:sockopt" => prog_type == BPF_PROG_TYPE_CGROUP_SOCKOPT,
        "lwt:in" => prog_type == BPF_PROG_TYPE_LWT_IN,
        "lwt:out" => prog_type == BPF_PROG_TYPE_LWT_OUT,
        "lwt:xmit" => prog_type == BPF_PROG_TYPE_LWT_XMIT,
        "lwt:seg6local" => prog_type == BPF_PROG_TYPE_LWT_SEG6LOCAL,
        "lirc:mode2" => prog_type == BPF_PROG_TYPE_LIRC_MODE2,
        "sock:ops" => prog_type == BPF_PROG_TYPE_SOCK_OPS,
        "sock:msg" => prog_type == BPF_PROG_TYPE_SK_MSG,
        "sock:skb" => prog_type == BPF_PROG_TYPE_SK_SKB,
        _ if attach_point.starts_with("socket:") => prog_type == BPF_PROG_TYPE_SOCKET_FILTER,
        _ => false,
    };
    if builtin_match {
        return true;
    }
    lookup_attach_verifier(attach_point)
        .map(|verifier| verifier(prog_type))
        .unwrap_or(false)
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
        // allow
        ((BPF_ALU | BPF_MOV | BPF_K) as u64) << 56 | ((BPF_REG_0 as u64) << 48) | 1,
        // exit (return non-zero => packet accepted)
        0x0000000000000095 | ((BPF_JMP | BPF_EXIT) as u64) << 56,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    const CUSTOM_PROG_TYPE: u32 = 0x9000_0001;

    fn custom_attach_accepts_custom_prog_type(prog_type: u32) -> bool {
        prog_type == CUSTOM_PROG_TYPE
    }

    fn custom_attach_family_accepts_custom_prog_type(prog_type: u32) -> bool {
        prog_type == CUSTOM_PROG_TYPE
    }

    static EBPF_TEST_EPOCH: spin::Lazy<Mutex<()>> = spin::Lazy::new(|| Mutex::new(()));

    fn ebpf_test_epoch() -> spin::MutexGuard<'static, ()> {
        let guard = EBPF_TEST_EPOCH.lock();
        *GLOBAL_EBPF_LOADER.lock() = EbpfLoader::new();
        EBPF_PROG_TYPE_REGISTRY.lock().clear();
        EBPF_ATTACH_REGISTRY.lock().clear();
        EBPF_ATTACH_FAMILY_REGISTRY.lock().clear();
        guard
    }

    fn encode_insn(opcode: u8, dst: u8, src: u8, off: i16, imm: i32) -> u64 {
        ((opcode as u64) << 56)
            | ((dst as u64) << 48)
            | ((src as u64) << 40)
            | (((off as u16) as u64) << 32)
            | (imm as u32 as u64)
    }

    fn encode_imm_insn(opcode: u8, dst: u8, off: i16, imm: i32) -> u64 {
        encode_insn(opcode, dst, 0, off, imm)
    }

    fn build_minimal_elf_with_program(section_name: &str, program: &[u64]) -> Vec<u8> {
        let mut shstrtab = vec![0u8];
        let section_name_off = shstrtab.len() as u32;
        shstrtab.extend_from_slice(section_name.as_bytes());
        shstrtab.push(0);
        let shstrtab_name_off = shstrtab.len() as u32;
        shstrtab.extend_from_slice(b".shstrtab");
        shstrtab.push(0);

        let mut text = Vec::with_capacity(program.len() * 8);
        for insn in program {
            text.extend_from_slice(&insn.to_le_bytes());
        }

        let ehsize = 64usize;
        let shentsize = 64usize;
        let shnum = 3usize;
        let shoff = ehsize;
        let text_off = shoff + shentsize * shnum;
        let shstr_off = text_off + text.len();
        let file_size = shstr_off + shstrtab.len();

        let mut elf = vec![0u8; file_size];
        elf[0..4].copy_from_slice(b"\x7FELF");
        elf[4] = 2;
        elf[5] = 1;
        elf[6] = 1;
        elf[16..18].copy_from_slice(&1u16.to_le_bytes());
        elf[18..20].copy_from_slice(&0x3Eu16.to_le_bytes());
        elf[20..24].copy_from_slice(&1u32.to_le_bytes());
        elf[40..48].copy_from_slice(&(shoff as u64).to_le_bytes());
        elf[52..54].copy_from_slice(&(ehsize as u16).to_le_bytes());
        elf[58..60].copy_from_slice(&(shentsize as u16).to_le_bytes());
        elf[60..62].copy_from_slice(&(shnum as u16).to_le_bytes());
        elf[62..64].copy_from_slice(&2u16.to_le_bytes());

        elf[text_off..text_off + text.len()].copy_from_slice(&text);
        elf[shstr_off..shstr_off + shstrtab.len()].copy_from_slice(&shstrtab);

        let sec1 = shoff + shentsize;
        elf[sec1..sec1 + 4].copy_from_slice(&section_name_off.to_le_bytes());
        elf[sec1 + 4..sec1 + 8].copy_from_slice(&1u32.to_le_bytes());
        elf[sec1 + 24..sec1 + 32].copy_from_slice(&(text_off as u64).to_le_bytes());
        elf[sec1 + 32..sec1 + 40].copy_from_slice(&(text.len() as u64).to_le_bytes());
        elf[sec1 + 48..sec1 + 56].copy_from_slice(&8u64.to_le_bytes());

        let sec2 = shoff + shentsize * 2;
        elf[sec2..sec2 + 4].copy_from_slice(&shstrtab_name_off.to_le_bytes());
        elf[sec2 + 4..sec2 + 8].copy_from_slice(&3u32.to_le_bytes());
        elf[sec2 + 24..sec2 + 32].copy_from_slice(&(shstr_off as u64).to_le_bytes());
        elf[sec2 + 32..sec2 + 40].copy_from_slice(&(shstrtab.len() as u64).to_le_bytes());
        elf[sec2 + 48..sec2 + 56].copy_from_slice(&1u64.to_le_bytes());

        elf
    }

    #[test]
    fn elf_loader_accepts_socket_filter_section_and_jit_runs() {
        let _epoch = ebpf_test_epoch();
        let program = vec![
            encode_imm_insn(BPF_ALU | BPF_MOV | BPF_K, BPF_REG_0, 0, 1),
            encode_insn(BPF_JMP | BPF_EXIT, 0, 0, 0, 0),
        ];
        let elf = build_minimal_elf_with_program("socket_filter", &program);
        let mut loader = EbpfLoader::new();
        let prog_id = loader.load_elf(&elf, BPF_PROG_TYPE_SOCKET_FILTER).unwrap();

        loader.jit_compile(&prog_id).unwrap();
        loader
            .attach_socket_filter("test:ingress", &prog_id)
            .unwrap();
        let verdict = loader
            .run_socket_filter("test:ingress", &[0u8; 64])
            .unwrap();
        assert_eq!(verdict, 1);
    }

    #[test]
    fn ingress_attach_registry_runs_jit_compiled_program() {
        let _epoch = ebpf_test_epoch();
        let program = vec![
            encode_imm_insn(BPF_ALU | BPF_MOV | BPF_K, BPF_REG_0, 0, 1),
            encode_insn(BPF_JMP | BPF_EXIT, 0, 0, 0, 0),
        ];
        attach_ingress_program("jit-allow", program).unwrap();
        {
            let mut loader = GLOBAL_EBPF_LOADER.lock();
            loader.jit_compile("jit-allow").unwrap();
        }
        assert!(filter_ingress_packet(&[0u8; 32]).unwrap());
        detach_ingress_program().unwrap();
    }

    #[test]
    fn classifier_and_xdp_packet_program_types_attach_and_run() {
        let _epoch = ebpf_test_epoch();
        let program = vec![
            encode_imm_insn(BPF_ALU | BPF_MOV | BPF_K, BPF_REG_0, 0, 7),
            encode_insn(BPF_JMP | BPF_EXIT, 0, 0, 0, 0),
        ];
        attach_classifier_program("sched-cls-allow", program.clone()).unwrap();
        attach_xdp_program("xdp-allow", program).unwrap();
        assert_eq!(run_classifier(&[1, 2, 3]).unwrap(), 7);
        assert_eq!(run_xdp(&[4, 5, 6]).unwrap(), 7);
    }

    #[test]
    fn egress_reuseport_and_flow_dissector_helpers_attach_and_run() {
        let _epoch = ebpf_test_epoch();
        let program = vec![
            encode_imm_insn(BPF_ALU | BPF_MOV | BPF_K, BPF_REG_0, 0, 5),
            encode_insn(BPF_JMP | BPF_EXIT, 0, 0, 0, 0),
        ];

        attach_egress_program("egress-allow", program.clone()).unwrap();
        attach_reuseport_program("reuseport-allow", program.clone()).unwrap();
        attach_flow_dissector_program("flow-allow", program).unwrap();

        assert!(filter_egress_packet(&[0u8; 16]).unwrap());
        assert_eq!(run_reuseport(&[1, 2, 3, 4]).unwrap(), 5);
        assert_eq!(run_flow_dissector(&[5, 6, 7, 8]).unwrap(), 5);

        detach_egress_program().unwrap();
    }

    #[test]
    fn elf_loader_accepts_xdp_and_reuseport_section_names() {
        let _epoch = ebpf_test_epoch();
        let program = vec![
            encode_imm_insn(BPF_ALU | BPF_MOV | BPF_K, BPF_REG_0, 0, 9),
            encode_insn(BPF_JMP | BPF_EXIT, 0, 0, 0, 0),
        ];

        let xdp_elf = build_minimal_elf_with_program("xdp", &program);
        let reuseport_elf = build_minimal_elf_with_program("reuseport", &program);
        let mut loader = EbpfLoader::new();

        let xdp_prog = loader.load_elf(&xdp_elf, BPF_PROG_TYPE_XDP).unwrap();
        let reuseport_prog = loader
            .load_elf(&reuseport_elf, BPF_PROG_TYPE_SK_REUSEPORT)
            .unwrap();

        loader.attach_program("net:xdp", &xdp_prog).unwrap();
        loader
            .attach_program("net:reuseport", &reuseport_prog)
            .unwrap();

        assert_eq!(
            loader.run_attached_program("net:xdp", &[0u8; 8]).unwrap(),
            9
        );
        assert_eq!(
            loader
                .run_attached_program("net:reuseport", &[0u8; 8])
                .unwrap(),
            9
        );
    }

    #[test]
    fn extended_attach_family_accepts_supported_prog_types() {
        let _epoch = ebpf_test_epoch();
        let mut loader = EbpfLoader::new();
        let program = vec![
            encode_imm_insn(BPF_ALU | BPF_MOV | BPF_K, BPF_REG_0, 0, 1),
            encode_insn(BPF_JMP | BPF_EXIT, 0, 0, 0, 0),
        ];

        let cases = [
            ("kprobe", BPF_PROG_TYPE_KPROBE, "trace:kprobe"),
            ("tracepoint", BPF_PROG_TYPE_TRACEPOINT, "trace:tracepoint"),
            ("perf", BPF_PROG_TYPE_PERF_EVENT, "trace:perf"),
            ("raw", BPF_PROG_TYPE_RAW_TRACEPOINT, "trace:raw"),
            ("raww", BPF_PROG_TYPE_RAW_TRACEPOINT_WRITABLE, "trace:raw"),
            ("cgroup-skb", BPF_PROG_TYPE_CGROUP_SKB, "cgroup:skb"),
            ("cgroup-sock", BPF_PROG_TYPE_CGROUP_SOCK, "cgroup:sock"),
            (
                "cgroup-sock-addr",
                BPF_PROG_TYPE_CGROUP_SOCK_ADDR,
                "cgroup:sock-addr",
            ),
            (
                "cgroup-device",
                BPF_PROG_TYPE_CGROUP_DEVICE,
                "cgroup:device",
            ),
            (
                "cgroup-sysctl",
                BPF_PROG_TYPE_CGROUP_SYSCTL,
                "cgroup:sysctl",
            ),
            (
                "cgroup-sockopt",
                BPF_PROG_TYPE_CGROUP_SOCKOPT,
                "cgroup:sockopt",
            ),
            ("lwt-in", BPF_PROG_TYPE_LWT_IN, "lwt:in"),
            ("lwt-out", BPF_PROG_TYPE_LWT_OUT, "lwt:out"),
            ("lwt-xmit", BPF_PROG_TYPE_LWT_XMIT, "lwt:xmit"),
            (
                "lwt-seg6local",
                BPF_PROG_TYPE_LWT_SEG6LOCAL,
                "lwt:seg6local",
            ),
            ("lirc-mode2", BPF_PROG_TYPE_LIRC_MODE2, "lirc:mode2"),
            ("sock-ops", BPF_PROG_TYPE_SOCK_OPS, "sock:ops"),
            ("sock-msg", BPF_PROG_TYPE_SK_MSG, "sock:msg"),
            ("sk-skb", BPF_PROG_TYPE_SK_SKB, "sock:skb"),
        ];

        for (prog_id, prog_type, attach_point) in cases {
            loader
                .load_program(prog_id, program.clone(), prog_type)
                .unwrap();
            loader.attach_program(attach_point, prog_id).unwrap();
            assert_eq!(
                loader
                    .run_attached_program(attach_point, &[0u8; 8])
                    .unwrap(),
                1
            );
        }
    }

    #[test]
    fn elf_loader_accepts_extended_section_families() {
        let _epoch = ebpf_test_epoch();
        let program = vec![
            encode_imm_insn(BPF_ALU | BPF_MOV | BPF_K, BPF_REG_0, 0, 3),
            encode_insn(BPF_JMP | BPF_EXIT, 0, 0, 0, 0),
        ];
        let cases = [
            ("kprobe", BPF_PROG_TYPE_KPROBE, "trace:kprobe"),
            ("tracepoint", BPF_PROG_TYPE_TRACEPOINT, "trace:tracepoint"),
            ("perf_event", BPF_PROG_TYPE_PERF_EVENT, "trace:perf"),
            ("cgroup_skb", BPF_PROG_TYPE_CGROUP_SKB, "cgroup:skb"),
            ("lwt_xmit", BPF_PROG_TYPE_LWT_XMIT, "lwt:xmit"),
            ("lirc_mode2", BPF_PROG_TYPE_LIRC_MODE2, "lirc:mode2"),
            ("sock_ops", BPF_PROG_TYPE_SOCK_OPS, "sock:ops"),
        ];

        for (section, prog_type, attach_point) in cases {
            let elf = build_minimal_elf_with_program(section, &program);
            let mut loader = EbpfLoader::new();
            let prog_id = loader.load_elf(&elf, prog_type).unwrap();
            loader.attach_program(attach_point, &prog_id).unwrap();
            assert_eq!(
                loader
                    .run_attached_program(attach_point, &[9u8; 4])
                    .unwrap(),
                3
            );
        }
    }

    #[test]
    fn trace_cgroup_lwt_and_sock_helpers_attach_and_run() {
        let _epoch = ebpf_test_epoch();
        let program = vec![
            encode_imm_insn(BPF_ALU | BPF_MOV | BPF_K, BPF_REG_0, 0, 11),
            encode_insn(BPF_JMP | BPF_EXIT, 0, 0, 0, 0),
        ];

        attach_trace_program(
            "trace-kprobe",
            program.clone(),
            BPF_PROG_TYPE_KPROBE,
            "trace:kprobe",
        )
        .unwrap();
        attach_cgroup_program(
            "cgroup-skb",
            program.clone(),
            BPF_PROG_TYPE_CGROUP_SKB,
            "cgroup:skb",
        )
        .unwrap();
        attach_lwt_program(
            "lwt-xmit",
            program.clone(),
            BPF_PROG_TYPE_LWT_XMIT,
            "lwt:xmit",
        )
        .unwrap();
        attach_lirc_program("lirc-mode2", program.clone()).unwrap();
        attach_sock_program("sock-ops", program, BPF_PROG_TYPE_SOCK_OPS, "sock:ops").unwrap();

        assert_eq!(run_trace_program("trace:kprobe", &[1, 2, 3]).unwrap(), 11);
        assert_eq!(run_cgroup_program("cgroup:skb", &[4, 5, 6]).unwrap(), 11);
        assert_eq!(run_lwt_program("lwt:xmit", &[7, 8, 9]).unwrap(), 11);
        assert_eq!(run_lirc_program(&[0xaa, 0xbb]).unwrap(), 11);
        assert_eq!(run_sock_program("sock:ops", &[10, 11]).unwrap(), 11);
    }

    #[test]
    fn attach_point_family_accepts_extended_namespace_paths() {
        let _epoch = ebpf_test_epoch();
        let mut loader = EbpfLoader::new();
        let program = vec![
            encode_imm_insn(BPF_ALU | BPF_MOV | BPF_K, BPF_REG_0, 0, 13),
            encode_insn(BPF_JMP | BPF_EXIT, 0, 0, 0, 0),
        ];
        let cases = [
            ("trace-any", BPF_PROG_TYPE_TRACEPOINT, "trace:syscalls/open"),
            (
                "cgroup-any",
                BPF_PROG_TYPE_CGROUP_SKB,
                "cgroup:tenant-a/egress",
            ),
            ("lwt-any", BPF_PROG_TYPE_LWT_XMIT, "lwt:vrf-main/xmit"),
            ("sock-any", BPF_PROG_TYPE_SOCK_OPS, "sock:tenant-a/ops"),
            ("lirc-any", BPF_PROG_TYPE_LIRC_MODE2, "lirc:consumer-ir"),
            (
                "socket-any",
                BPF_PROG_TYPE_SOCKET_FILTER,
                "socket:icmp-observer",
            ),
        ];

        for (prog_id, prog_type, attach_point) in cases {
            loader
                .load_program(prog_id, program.clone(), prog_type)
                .unwrap();
            loader.attach_program(attach_point, prog_id).unwrap();
            assert_eq!(
                loader
                    .run_attached_program(attach_point, &[1, 2, 3])
                    .unwrap(),
                13
            );
        }
    }

    #[test]
    fn attach_point_aliases_normalize_to_supported_canonical_hooks() {
        let _epoch = ebpf_test_epoch();
        let mut loader = EbpfLoader::new();
        let program = vec![
            encode_imm_insn(BPF_ALU | BPF_MOV | BPF_K, BPF_REG_0, 0, 11),
            encode_insn(BPF_JMP | BPF_EXIT, 0, 0, 0, 0),
        ];
        let cases = [
            ("driver:xdp", BPF_PROG_TYPE_XDP),
            ("tc:classifier", BPF_PROG_TYPE_SCHED_CLS),
            ("socket:reuseport", BPF_PROG_TYPE_SK_REUSEPORT),
            ("flow:dissector", BPF_PROG_TYPE_FLOW_DISSECTOR),
            ("trace:perf-event", BPF_PROG_TYPE_PERF_EVENT),
            ("cgroup:connect4", BPF_PROG_TYPE_CGROUP_SOCK_ADDR),
            ("sock:message", BPF_PROG_TYPE_SK_MSG),
        ];

        for (idx, (attach_point, prog_type)) in cases.iter().enumerate() {
            let prog_id = format!("alias-{idx}");
            loader
                .load_program(&prog_id, program.clone(), *prog_type)
                .unwrap();
            loader.attach_program(attach_point, &prog_id).unwrap();
            assert_eq!(
                loader
                    .run_attached_program(attach_point, &[0u8; 8])
                    .unwrap(),
                11
            );
        }
    }

    #[test]
    fn dynamic_prog_type_and_attach_registry_accept_custom_family() {
        let _epoch = ebpf_test_epoch();
        register_program_type(CUSTOM_PROG_TYPE);
        register_attach_point("vendor:flow", custom_attach_accepts_custom_prog_type);

        let program = vec![
            encode_imm_insn(BPF_ALU | BPF_MOV | BPF_K, BPF_REG_0, 0, 9),
            encode_insn(BPF_JMP | BPF_EXIT, 0, 0, 0, 0),
        ];
        let mut loader = EbpfLoader::new();
        loader
            .load_program("vendor-flow", program, CUSTOM_PROG_TYPE)
            .unwrap();
        loader.attach_program("vendor:flow", "vendor-flow").unwrap();
        assert_eq!(
            loader
                .run_attached_program("vendor:flow", &[1, 2, 3])
                .unwrap(),
            9
        );
    }

    #[test]
    fn dynamic_attach_family_registry_accepts_nested_custom_paths() {
        let _epoch = ebpf_test_epoch();
        register_program_type(CUSTOM_PROG_TYPE);
        register_attach_family("vendor", custom_attach_family_accepts_custom_prog_type);

        let program = vec![
            encode_imm_insn(BPF_ALU | BPF_MOV | BPF_K, BPF_REG_0, 0, 17),
            encode_insn(BPF_JMP | BPF_EXIT, 0, 0, 0, 0),
        ];
        let mut loader = EbpfLoader::new();
        loader
            .load_program("vendor-flow-nested", program, CUSTOM_PROG_TYPE)
            .unwrap();
        loader
            .attach_program("vendor:tenant-a/egress", "vendor-flow-nested")
            .unwrap();
        assert_eq!(
            loader
                .run_attached_program("vendor:tenant-a/egress", &[1, 2, 3])
                .unwrap(),
            17
        );
    }

    #[test]
    fn wildcard_attach_family_registry_accepts_unknown_namespace_paths() {
        let _epoch = ebpf_test_epoch();
        register_program_type(CUSTOM_PROG_TYPE);
        register_attach_family("*", custom_attach_family_accepts_custom_prog_type);

        let program = vec![
            encode_imm_insn(BPF_ALU | BPF_MOV | BPF_K, BPF_REG_0, 0, 23),
            encode_insn(BPF_JMP | BPF_EXIT, 0, 0, 0, 0),
        ];
        let mut loader = EbpfLoader::new();
        loader
            .load_program("wildcard-family", program, CUSTOM_PROG_TYPE)
            .unwrap();
        loader
            .attach_program("thirdparty:tenant-b/custom-egress", "wildcard-family")
            .unwrap();
        assert_eq!(
            loader
                .run_attached_program("thirdparty:tenant-b/custom-egress", &[9, 8, 7])
                .unwrap(),
            23
        );
    }

    #[test]
    fn generic_attach_fallback_accepts_unregistered_prog_type_and_attach_point() {
        let _epoch = ebpf_test_epoch();
        const GENERIC_PROG_TYPE: u32 = 0xA300_0042;
        let program = vec![
            encode_imm_insn(BPF_ALU | BPF_MOV | BPF_K, BPF_REG_0, 0, 29),
            encode_insn(BPF_JMP | BPF_EXIT, 0, 0, 0, 0),
        ];
        let mut loader = EbpfLoader::new();
        loader
            .load_program("generic-fallback", program, GENERIC_PROG_TYPE)
            .unwrap();
        loader
            .attach_program("unlisted:tenant-c/custom-hook", "generic-fallback")
            .unwrap();
        assert_eq!(
            loader
                .run_attached_program("unlisted:tenant-c/custom-hook", &[3, 2, 1])
                .unwrap(),
            29
        );
    }

    #[test]
    fn wildcard_attachment_lookup_rehydrates_unknown_runtime_paths() {
        let _epoch = ebpf_test_epoch();
        let program = vec![
            encode_imm_insn(BPF_ALU | BPF_MOV | BPF_K, BPF_REG_0, 0, 31),
            encode_insn(BPF_JMP | BPF_EXIT, 0, 0, 0, 0),
        ];
        let mut loader = EbpfLoader::new();
        loader
            .load_program("wildcard-attachment", program, BPF_PROG_TYPE_SOCKET_FILTER)
            .unwrap();
        loader.attach_program("*", "wildcard-attachment").unwrap();
        assert_eq!(
            loader
                .run_attached_program("vendor:new-space/runtime", &[7, 7, 7])
                .unwrap(),
            31
        );
    }

    #[test]
    fn verifier_rejects_unreachable_instruction() {
        let _epoch = ebpf_test_epoch();
        let program = vec![
            encode_insn(BPF_JMP | BPF_EXIT, 0, 0, 0, 0),
            encode_insn(BPF_JMP | BPF_EXIT, 0, 0, 0, 0),
        ];
        let mut loader = EbpfLoader::new();
        assert!(matches!(
            loader.load_program("unreachable", program, BPF_PROG_TYPE_SOCKET_FILTER),
            Err(EbpfError::VerifierRejected)
        ));
    }

    #[test]
    fn verifier_rejects_uninitialized_register_read() {
        let _epoch = ebpf_test_epoch();
        let program = vec![
            encode_insn(BPF_ALU | BPF_MOV | BPF_SRC_X, BPF_REG_0, BPF_REG_2, 0, 0),
            encode_insn(BPF_JMP | BPF_EXIT, 0, 0, 0, 0),
        ];
        let mut loader = EbpfLoader::new();
        assert!(matches!(
            loader.load_program("uninit-read", program, BPF_PROG_TYPE_SOCKET_FILTER),
            Err(EbpfError::VerifierRejected)
        ));
    }

    #[test]
    fn verifier_rejects_stack_read_before_write() {
        let _epoch = ebpf_test_epoch();
        let program = vec![
            encode_insn(BPF_ALU | BPF_MOV | BPF_SRC_X, BPF_REG_1, BPF_REG_10, 0, 0),
            encode_imm_insn(BPF_ALU | BPF_ADD | BPF_K, BPF_REG_1, 0, -8),
            encode_insn(BPF_LDX | BPF_DW, BPF_REG_0, BPF_REG_1, 0, 0),
            encode_insn(BPF_JMP | BPF_EXIT, 0, 0, 0, 0),
        ];
        let mut loader = EbpfLoader::new();
        assert!(matches!(
            loader.load_program(
                "stack-read-before-write",
                program,
                BPF_PROG_TYPE_SOCKET_FILTER
            ),
            Err(EbpfError::VerifierRejected) | Err(EbpfError::MemoryAccess)
        ));
    }
}
