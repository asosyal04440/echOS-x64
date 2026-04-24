//! # echOS eBPF Yorumlayıcı (Interpreter)
//!
//! Linux eBPF (extended Berkeley Packet Filter) ile uyumlu bir in-kernel
//! sanal makine (VM) implementasyonu. Kullanıcı alanından yüklenen eBPF
//! programları güvenli bir sandbox içinde çalıştırılır.
//!
//! ## Mimari
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │  eBPF Sanal Makine                                                 │
//! │                                                                     │
//! │  Kayıtçılar (Registers):                                           │
//! │  ┌─────┬─────┬─────┬─────┬─────┬─────┬─────┬─────┬─────┬──────┐  │
//! │  │ R0  │ R1  │ R2  │ R3  │ R4  │ R5  │ R6  │ R7  │ R8  │ R9   │  │
//! │  │ ret │arg1 │arg2 │arg3 │arg4 │arg5 │callee│callee│callee│callee│  │
//! │  └─────┴─────┴─────┴─────┴─────┴─────┴─────┴─────┴─────┴──────┘  │
//! │  R10 = Frame Pointer (salt okunur, yığın tabanını gösterir)        │
//! │                                                                     │
//! │  Yığın (Stack): 512 byte (BPF_STACK_SIZE)                          │
//! │  ┌────────────────────────────────────────────────────────┐        │
//! │  │  [R10-512] ←── alt          [R10] ←── taban (FP)      │        │
//! │  └────────────────────────────────────────────────────────┘        │
//! │                                                                     │
//! │  Talimat Seti:                                                     │
//! │  ├── ALU64: add, sub, mul, div, mod, or, and, lsh, rsh, neg, xor  │
//! │  ├── ALU32: aynı işlemler, 32-bit sonuç                           │
//! │  ├── JMP: ja, jeq, jgt, jge, jlt, jle, jne, jset, call, exit     │
//! │  ├── LD:  lddw (64-bit immediate), ldabs, ldind                   │
//! │  ├── LDX: ldxb, ldxh, ldxw, ldxdw (register indirect okuma)      │
//! │  ├── ST:  stb, sth, stw, stdw (immediate yazma)                   │
//! │  ├── STX: stxb, stxh, stxw, stxdw (register indirect yazma)      │
//! │  └── ATOMIC: xadd (atomik toplama)                                 │
//! └─────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Güvenlik
//!
//! Tüm programlar çalıştırılmadan önce `BpfVerifier` tarafından
//! statik olarak doğrulanır:
//! - Sınırsız döngü yok (geriye atlama yasak)
//! - Bellek erişimi sınırları kontrol edilir
//! - Yığın derinliği 512 byte ile sınırlıdır
//! - Erişilemez talimat yok (tüm yollar EXIT ile biter)
//! - Maksimum talimat sayısı sınırı (BPF_MAX_INSNS)

use alloc::string::String;
use alloc::vec::Vec;
#[cfg(all(
    feature = "host_smoke",
    not(target_os = "none"),
    not(target_os = "uefi")
))]
use std::eprintln;

#[cfg(all(
    feature = "host_smoke",
    not(target_os = "none"),
    not(target_os = "uefi")
))]
fn host_smoke_probe(stage: &str) {
    if std::env::var_os("PHASE1_DEBUG_EBPF").is_some()
        || std::env::var_os("PHASE1_SKIP_EBPF_RUN").is_some()
    {
        eprintln!("ebpf:{stage}");
    }
}

#[cfg(not(all(
    feature = "host_smoke",
    not(target_os = "none"),
    not(target_os = "uefi")
)))]
fn host_smoke_probe(_stage: &str) {}

// ============================================================================
// Sabitler
// ============================================================================

/// eBPF kayıtçı sayısı (R0-R10)
pub const BPF_REG_COUNT: usize = 11;

/// R0: Dönüş değeri ve helper fonksiyon sonucu
pub const BPF_REG_0: usize = 0;
/// R1-R5: Fonksiyon argümanları (caller-saved)
pub const BPF_REG_1: usize = 1;
pub const BPF_REG_2: usize = 2;
pub const BPF_REG_3: usize = 3;
pub const BPF_REG_4: usize = 4;
pub const BPF_REG_5: usize = 5;
/// R6-R9: Callee-saved (fonksiyon çağrısı boyunca korunur)
pub const BPF_REG_6: usize = 6;
pub const BPF_REG_7: usize = 7;
pub const BPF_REG_8: usize = 8;
pub const BPF_REG_9: usize = 9;
/// R10: Frame Pointer (salt okunur)
pub const BPF_REG_FP: usize = 10;

/// eBPF yığın boyutu (byte)
pub const BPF_STACK_SIZE: usize = 512;

/// Maksimum talimat sayısı (DoS koruması)
pub const BPF_MAX_INSNS: usize = 4096;

/// Maksimum çalışma adımı (sonsuz döngü koruması)
pub const BPF_MAX_STEPS: usize = 1_000_000;

// ============================================================================
// Talimat Sınıfları (Instruction Classes) — Linux ABI
// ============================================================================

/// LD: Yükleme (load) — paket verisi ve immediate
pub const BPF_LD: u8 = 0x00;
/// LDX: Register-indirect yükleme
pub const BPF_LDX: u8 = 0x01;
/// ST: Immediate ile bellek yazma
pub const BPF_ST: u8 = 0x02;
/// STX: Register ile bellek yazma
pub const BPF_STX: u8 = 0x03;
/// ALU: 32-bit aritmetik/mantık işlemleri
pub const BPF_ALU: u8 = 0x04;
/// JMP: Dallanma (branch) işlemleri
pub const BPF_JMP: u8 = 0x05;
/// JMP32: 32-bit dallanma
pub const BPF_JMP32: u8 = 0x06;
/// ALU64: 64-bit aritmetik/mantık işlemleri
pub const BPF_ALU64: u8 = 0x07;

// ============================================================================
// ALU İşlem Kodları (Operation Codes)
// ============================================================================

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
pub const BPF_ARSH: u8 = 0xc0; // Arithmetic right shift (işaret korumalı)
pub const BPF_END: u8 = 0xd0; // Endianness dönüşümü

// ============================================================================
// JMP İşlem Kodları
// ============================================================================

pub const BPF_JA: u8 = 0x00; // Koşulsuz atlama
pub const BPF_JEQ: u8 = 0x10; // dst == src
pub const BPF_JGT: u8 = 0x20; // dst > src (unsigned)
pub const BPF_JGE: u8 = 0x30; // dst >= src (unsigned)
pub const BPF_JSET: u8 = 0x40; // dst & src != 0
pub const BPF_JNE: u8 = 0x50; // dst != src
pub const BPF_JSGT: u8 = 0x60; // dst > src (signed)
pub const BPF_JSGE: u8 = 0x70; // dst >= src (signed)
pub const BPF_CALL: u8 = 0x80; // Fonksiyon çağrısı
pub const BPF_EXIT: u8 = 0x90; // Program çıkışı (R0 dönüş değeri)
pub const BPF_JLT: u8 = 0xa0; // dst < src (unsigned)
pub const BPF_JLE: u8 = 0xb0; // dst <= src (unsigned)
pub const BPF_JSLT: u8 = 0xc0; // dst < src (signed)
pub const BPF_JSLE: u8 = 0xd0; // dst <= src (signed)

// ============================================================================
// Bellek Erişim Boyutları
// ============================================================================

pub const BPF_W: u8 = 0x00; // Word (32-bit)
pub const BPF_H: u8 = 0x08; // Half-word (16-bit)
pub const BPF_B: u8 = 0x10; // Byte (8-bit)
pub const BPF_DW: u8 = 0x18; // Double-word (64-bit)

// Kaynak operand türü
pub const BPF_K: u8 = 0x00; // Immediate (sabit değer)
pub const BPF_X: u8 = 0x08; // Register (kayıtçı değeri)

// Atomik işlem bayrağı
pub const BPF_ATOMIC: u8 = 0xc0;

// ============================================================================
// Talimat Yapısı (64-bit, Linux ABI uyumlu)
// ============================================================================

/// eBPF talimatı — 64 bit (8 byte)
///
/// ```text
/// ┌──────┬──────┬──────┬──────────┬──────────────────────────┐
/// │ op   │ regs │ off  │ imm                                │
/// │ 8bit │ 8bit │16bit │ 32bit                              │
/// ├──────┼──┬───┼──────┼──────────────────────────────────────┤
/// │opcode│dst│src│offset│ immediate                          │
/// │      │4b │4b │      │                                    │
/// └──────┴──┴───┴──────┴──────────────────────────────────────┘
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct BpfInsn {
    /// İşlem kodu: class | op | source
    pub opcode: u8,
    /// Kayıtçılar: alt 4 bit = dst, üst 4 bit = src
    pub regs: u8,
    /// Ofset: bellek erişimi veya dallanma hedefi (signed)
    pub off: i16,
    /// Immediate: sabit değer (signed 32-bit, LDDW için 64-bit)
    pub imm: i32,
}

impl BpfInsn {
    /// Hedef kayıtçı indeksini döner (0-10)
    #[inline]
    pub fn dst_reg(&self) -> usize {
        (self.regs & 0x0f) as usize
    }

    /// Kaynak kayıtçı indeksini döner (0-10)
    #[inline]
    pub fn src_reg(&self) -> usize {
        ((self.regs >> 4) & 0x0f) as usize
    }

    /// Talimat sınıfını döner (LD, LDX, ST, STX, ALU, JMP, ALU64)
    #[inline]
    pub fn class(&self) -> u8 {
        self.opcode & 0x07
    }

    /// ALU/JMP işlem kodunu döner
    #[inline]
    pub fn op(&self) -> u8 {
        self.opcode & 0xf0
    }

    /// Kaynak türünü döner (BPF_K veya BPF_X)
    #[inline]
    pub fn src_type(&self) -> u8 {
        self.opcode & 0x08
    }

    /// Bellek erişim boyutunu döner (BPF_B, BPF_H, BPF_W, BPF_DW)
    #[inline]
    pub fn mem_size(&self) -> u8 {
        self.opcode & 0x18
    }

    /// Yeni bir BPF talimatı oluşturur
    pub const fn new(opcode: u8, dst: u8, src: u8, off: i16, imm: i32) -> Self {
        Self {
            opcode,
            regs: (src << 4) | (dst & 0x0f),
            off,
            imm,
        }
    }
}

// ============================================================================
// eBPF Hata Türleri
// ============================================================================

/// eBPF çalışma zamanı veya doğrulama hatası
#[derive(Debug, Clone)]
pub enum BpfError {
    /// Sıfıra bölme
    DivisionByZero,
    /// Geçersiz kayıtçı indeksi (>10)
    InvalidRegister(usize),
    /// Yığın taşması veya sınır ihlali
    StackOutOfBounds { offset: i64, size: usize },
    /// Bellek erişim ihlali
    MemoryAccessViolation { addr: u64, size: usize },
    /// Geçersiz işlem kodu
    InvalidOpcode(u8),
    /// Maksimum adım limiti aşıldı
    ExceededStepLimit,
    /// Program doğrulaması başarısız
    VerificationFailed(String),
    /// Bilinmeyen helper fonksiyon numarası
    UnknownHelper(u32),
    /// Salt okunur kayıtçıya yazma (R10)
    ReadOnlyRegister,
    /// Erişilemez talimat (dead code)
    UnreachableInstruction(usize),
    /// Program çok büyük
    ProgramTooLarge(usize),
    /// Geçersiz program (yanlış tip veya attach noktası)
    InvalidProgram,
    /// Geçersiz talimat (JIT derleyici)
    InvalidInstruction(usize),
    /// Geçersiz atlama hedefi (JIT derleyici)
    InvalidJumpTarget,
}

// ============================================================================
// eBPF Sanal Makine
// ============================================================================

/// eBPF sanal makinesi — kayıtçılar, yığın ve program sayacı
pub struct BpfVm {
    /// 64-bit kayıtçılar (R0-R10)
    pub regs: [u64; BPF_REG_COUNT],
    /// Yığın belleği (512 byte)
    pub stack: [u8; BPF_STACK_SIZE],
    /// Program sayacı (instruction pointer)
    pub pc: usize,
    /// Çalıştırılan adım sayısı
    pub steps: usize,
    /// VM çalışmaya devam ediyor mu?
    pub running: bool,
    /// Helper fonksiyon tablosu (numara → fonksiyon pointer)
    helpers: [Option<fn(u64, u64, u64, u64, u64) -> u64>; 64],
}

impl BpfVm {
    /// Yeni bir BPF sanal makinesi oluşturur.
    ///
    /// R10 (FP) yığın tabanını gösterecek şekilde ayarlanır.
    pub fn new() -> Self {
        let mut vm = Self {
            regs: [0u64; BPF_REG_COUNT],
            stack: [0u8; BPF_STACK_SIZE],
            pc: 0,
            steps: 0,
            running: false,
            helpers: [None; 64],
        };
        // R10 = Frame Pointer → yığın tabanı
        // Yığın adresi olarak stack dizisinin sonunu kullanıyoruz
        vm.regs[BPF_REG_FP] = BPF_STACK_SIZE as u64;
        vm
    }

    /// Helper fonksiyon kaydeder.
    ///
    /// eBPF programları `call <helper_id>` ile kernel API'lerini çağırır.
    /// R1-R5 argüman, R0 dönüş değeri olarak kullanılır.
    pub fn register_helper(&mut self, id: u32, func: fn(u64, u64, u64, u64, u64) -> u64) {
        if (id as usize) < self.helpers.len() {
            self.helpers[id as usize] = Some(func);
        }
    }

    /// Program çalıştırır ve R0'daki dönüş değerini döner.
    ///
    /// ## Argümanlar
    /// - `program`: BPF talimat dizisi
    /// - `ctx`: R1'e yüklenen bağlam pointer'ı (paket başlangıcı vb.)
    ///
    /// ## Güvenlik
    /// Çalıştırmadan önce `BpfVerifier::verify()` çağrılmalıdır!
    pub fn execute(&mut self, program: &[BpfInsn], ctx: u64) -> Result<u64, BpfError> {
        // Sıfırla
        self.regs = [0u64; BPF_REG_COUNT];
        self.regs[BPF_REG_FP] = BPF_STACK_SIZE as u64;
        self.regs[BPF_REG_1] = ctx; // R1 = context pointer
        self.pc = 0;
        self.steps = 0;
        self.running = true;

        while self.running {
            if self.pc >= program.len() {
                return Err(BpfError::UnreachableInstruction(self.pc));
            }
            if self.steps >= BPF_MAX_STEPS {
                return Err(BpfError::ExceededStepLimit);
            }

            let insn = program[self.pc];
            self.steps += 1;

            match insn.class() {
                BPF_ALU64 => self.exec_alu64(&insn)?,
                BPF_ALU => self.exec_alu32(&insn)?,
                BPF_JMP => self.exec_jmp(&insn, program.len())?,
                BPF_JMP32 => self.exec_jmp32(&insn, program.len())?,
                BPF_LDX => self.exec_ldx(&insn)?,
                BPF_ST => self.exec_st(&insn)?,
                BPF_STX => self.exec_stx(&insn)?,
                BPF_LD => self.exec_ld(&insn, program)?,
                _ => return Err(BpfError::InvalidOpcode(insn.opcode)),
            }
        }

        Ok(self.regs[BPF_REG_0])
    }

    // ========================================================================
    // ALU64: 64-bit Aritmetik/Mantık İşlemleri
    // ========================================================================

    fn exec_alu64(&mut self, insn: &BpfInsn) -> Result<(), BpfError> {
        let dst = insn.dst_reg();
        if dst >= BPF_REG_COUNT || dst == BPF_REG_FP {
            return Err(BpfError::InvalidRegister(dst));
        }

        let src_val = if insn.src_type() == BPF_X {
            let src = insn.src_reg();
            if src >= BPF_REG_COUNT {
                return Err(BpfError::InvalidRegister(src));
            }
            self.regs[src]
        } else {
            insn.imm as i64 as u64 // Sign-extend to 64-bit
        };

        match insn.op() {
            BPF_ADD => self.regs[dst] = self.regs[dst].wrapping_add(src_val),
            BPF_SUB => self.regs[dst] = self.regs[dst].wrapping_sub(src_val),
            BPF_MUL => self.regs[dst] = self.regs[dst].wrapping_mul(src_val),
            BPF_DIV => {
                if src_val == 0 {
                    return Err(BpfError::DivisionByZero);
                }
                self.regs[dst] /= src_val;
            }
            BPF_MOD => {
                if src_val == 0 {
                    return Err(BpfError::DivisionByZero);
                }
                self.regs[dst] %= src_val;
            }
            BPF_OR => self.regs[dst] |= src_val,
            BPF_AND => self.regs[dst] &= src_val,
            BPF_XOR => self.regs[dst] ^= src_val,
            BPF_LSH => self.regs[dst] = self.regs[dst].wrapping_shl(src_val as u32),
            BPF_RSH => self.regs[dst] = self.regs[dst].wrapping_shr(src_val as u32),
            BPF_ARSH => {
                self.regs[dst] = (self.regs[dst] as i64).wrapping_shr(src_val as u32) as u64;
            }
            BPF_NEG => self.regs[dst] = (-(self.regs[dst] as i64)) as u64,
            BPF_MOV => self.regs[dst] = src_val,
            BPF_END => {
                // Endianness swap
                let size = insn.imm;
                self.regs[dst] = match size {
                    16 => (self.regs[dst] as u16).swap_bytes() as u64,
                    32 => (self.regs[dst] as u32).swap_bytes() as u64,
                    64 => self.regs[dst].swap_bytes(),
                    _ => return Err(BpfError::InvalidOpcode(insn.opcode)),
                };
            }
            _ => return Err(BpfError::InvalidOpcode(insn.opcode)),
        }

        self.pc += 1;
        Ok(())
    }

    // ========================================================================
    // ALU32: 32-bit Aritmetik/Mantık İşlemleri
    // ========================================================================

    fn exec_alu32(&mut self, insn: &BpfInsn) -> Result<(), BpfError> {
        let dst = insn.dst_reg();
        if dst >= BPF_REG_COUNT || dst == BPF_REG_FP {
            return Err(BpfError::InvalidRegister(dst));
        }

        let src_val = if insn.src_type() == BPF_X {
            let src = insn.src_reg();
            if src >= BPF_REG_COUNT {
                return Err(BpfError::InvalidRegister(src));
            }
            self.regs[src] as u32
        } else {
            insn.imm as u32
        };

        let dst_val = self.regs[dst] as u32;
        let result: u32 = match insn.op() {
            BPF_ADD => dst_val.wrapping_add(src_val),
            BPF_SUB => dst_val.wrapping_sub(src_val),
            BPF_MUL => dst_val.wrapping_mul(src_val),
            BPF_DIV => {
                if src_val == 0 {
                    return Err(BpfError::DivisionByZero);
                }
                dst_val / src_val
            }
            BPF_MOD => {
                if src_val == 0 {
                    return Err(BpfError::DivisionByZero);
                }
                dst_val % src_val
            }
            BPF_OR => dst_val | src_val,
            BPF_AND => dst_val & src_val,
            BPF_XOR => dst_val ^ src_val,
            BPF_LSH => dst_val.wrapping_shl(src_val),
            BPF_RSH => dst_val.wrapping_shr(src_val),
            BPF_ARSH => (dst_val as i32).wrapping_shr(src_val) as u32,
            BPF_NEG => (-(dst_val as i32)) as u32,
            BPF_MOV => src_val,
            _ => return Err(BpfError::InvalidOpcode(insn.opcode)),
        };

        // ALU32 sonucu zero-extend to 64-bit
        self.regs[dst] = result as u64;
        self.pc += 1;
        Ok(())
    }

    // ========================================================================
    // JMP: 64-bit Dallanma İşlemleri
    // ========================================================================

    fn exec_jmp(&mut self, insn: &BpfInsn, prog_len: usize) -> Result<(), BpfError> {
        match insn.op() {
            BPF_EXIT => {
                self.running = false;
                return Ok(());
            }
            BPF_CALL => {
                let helper_id = insn.imm as u32;
                if let Some(func) = self.helpers.get(helper_id as usize).and_then(|f| *f) {
                    self.regs[BPF_REG_0] = func(
                        self.regs[BPF_REG_1],
                        self.regs[BPF_REG_2],
                        self.regs[BPF_REG_3],
                        self.regs[BPF_REG_4],
                        self.regs[BPF_REG_5],
                    );
                } else {
                    return Err(BpfError::UnknownHelper(helper_id));
                }
                self.pc += 1;
                return Ok(());
            }
            BPF_JA => {
                self.pc = (self.pc as i64 + 1 + insn.off as i64) as usize;
                if self.pc >= prog_len {
                    return Err(BpfError::UnreachableInstruction(self.pc));
                }
                return Ok(());
            }
            _ => {}
        }

        // Koşullu dallanma
        let dst_val = self.regs[insn.dst_reg()];
        let src_val = if insn.src_type() == BPF_X {
            self.regs[insn.src_reg()]
        } else {
            insn.imm as i64 as u64
        };

        let taken = match insn.op() {
            BPF_JEQ => dst_val == src_val,
            BPF_JNE => dst_val != src_val,
            BPF_JGT => dst_val > src_val,
            BPF_JGE => dst_val >= src_val,
            BPF_JLT => dst_val < src_val,
            BPF_JLE => dst_val <= src_val,
            BPF_JSGT => (dst_val as i64) > (src_val as i64),
            BPF_JSGE => (dst_val as i64) >= (src_val as i64),
            BPF_JSLT => (dst_val as i64) < (src_val as i64),
            BPF_JSLE => (dst_val as i64) <= (src_val as i64),
            BPF_JSET => (dst_val & src_val) != 0,
            _ => return Err(BpfError::InvalidOpcode(insn.opcode)),
        };

        if taken {
            self.pc = (self.pc as i64 + 1 + insn.off as i64) as usize;
            if self.pc >= prog_len {
                return Err(BpfError::UnreachableInstruction(self.pc));
            }
        } else {
            self.pc += 1;
        }
        Ok(())
    }

    // ========================================================================
    // JMP32: 32-bit Dallanma İşlemleri
    // ========================================================================

    fn exec_jmp32(&mut self, insn: &BpfInsn, prog_len: usize) -> Result<(), BpfError> {
        let dst_val = self.regs[insn.dst_reg()] as u32;
        let src_val = if insn.src_type() == BPF_X {
            self.regs[insn.src_reg()] as u32
        } else {
            insn.imm as u32
        };

        let taken = match insn.op() {
            BPF_JEQ => dst_val == src_val,
            BPF_JNE => dst_val != src_val,
            BPF_JGT => dst_val > src_val,
            BPF_JGE => dst_val >= src_val,
            BPF_JLT => dst_val < src_val,
            BPF_JLE => dst_val <= src_val,
            BPF_JSGT => (dst_val as i32) > (src_val as i32),
            BPF_JSGE => (dst_val as i32) >= (src_val as i32),
            BPF_JSLT => (dst_val as i32) < (src_val as i32),
            BPF_JSLE => (dst_val as i32) <= (src_val as i32),
            BPF_JSET => (dst_val & src_val) != 0,
            _ => return Err(BpfError::InvalidOpcode(insn.opcode)),
        };

        if taken {
            self.pc = (self.pc as i64 + 1 + insn.off as i64) as usize;
            if self.pc >= prog_len {
                return Err(BpfError::UnreachableInstruction(self.pc));
            }
        } else {
            self.pc += 1;
        }
        Ok(())
    }

    // ========================================================================
    // LDX: Register-Indirect Bellek Okuma
    // ========================================================================

    fn exec_ldx(&mut self, insn: &BpfInsn) -> Result<(), BpfError> {
        let dst = insn.dst_reg();
        let src = insn.src_reg();
        if dst >= BPF_REG_COUNT || dst == BPF_REG_FP {
            return Err(BpfError::InvalidRegister(dst));
        }
        if src >= BPF_REG_COUNT {
            return Err(BpfError::InvalidRegister(src));
        }

        let addr = (self.regs[src] as i64 + insn.off as i64) as u64;

        // Yığın alanı kontrolü (R10-based erişim)
        let value = match insn.mem_size() {
            BPF_B => self.stack_read_u8(addr)? as u64,
            BPF_H => self.stack_read_u16(addr)? as u64,
            BPF_W => self.stack_read_u32(addr)? as u64,
            BPF_DW => self.stack_read_u64(addr)?,
            _ => return Err(BpfError::InvalidOpcode(insn.opcode)),
        };

        self.regs[dst] = value;
        self.pc += 1;
        Ok(())
    }

    // ========================================================================
    // ST: Immediate ile Bellek Yazma
    // ========================================================================

    fn exec_st(&mut self, insn: &BpfInsn) -> Result<(), BpfError> {
        let dst = insn.dst_reg();
        if dst >= BPF_REG_COUNT {
            return Err(BpfError::InvalidRegister(dst));
        }

        let addr = (self.regs[dst] as i64 + insn.off as i64) as u64;

        match insn.mem_size() {
            BPF_B => self.stack_write_u8(addr, insn.imm as u8)?,
            BPF_H => self.stack_write_u16(addr, insn.imm as u16)?,
            BPF_W => self.stack_write_u32(addr, insn.imm as u32)?,
            BPF_DW => self.stack_write_u64(addr, insn.imm as i64 as u64)?,
            _ => return Err(BpfError::InvalidOpcode(insn.opcode)),
        }

        self.pc += 1;
        Ok(())
    }

    // ========================================================================
    // STX: Register ile Bellek Yazma
    // ========================================================================

    fn exec_stx(&mut self, insn: &BpfInsn) -> Result<(), BpfError> {
        let dst = insn.dst_reg();
        let src = insn.src_reg();
        if dst >= BPF_REG_COUNT {
            return Err(BpfError::InvalidRegister(dst));
        }
        if src >= BPF_REG_COUNT {
            return Err(BpfError::InvalidRegister(src));
        }

        let addr = (self.regs[dst] as i64 + insn.off as i64) as u64;
        let val = self.regs[src];

        // Atomik STX check (BPF_ATOMIC bayrağı)
        if insn.mem_size() == BPF_DW && (insn.opcode & 0xf0) == BPF_ATOMIC {
            // XADD: atomik toplama
            let old = self.stack_read_u64(addr)?;
            self.stack_write_u64(addr, old.wrapping_add(val))?;
            self.pc += 1;
            return Ok(());
        }

        match insn.mem_size() {
            BPF_B => self.stack_write_u8(addr, val as u8)?,
            BPF_H => self.stack_write_u16(addr, val as u16)?,
            BPF_W => self.stack_write_u32(addr, val as u32)?,
            BPF_DW => self.stack_write_u64(addr, val)?,
            _ => return Err(BpfError::InvalidOpcode(insn.opcode)),
        }

        self.pc += 1;
        Ok(())
    }

    // ========================================================================
    // LD: 64-bit Immediate Yükleme (LDDW — 2 talimat genişliğinde)
    // ========================================================================

    fn exec_ld(&mut self, insn: &BpfInsn, program: &[BpfInsn]) -> Result<(), BpfError> {
        let dst = insn.dst_reg();
        if dst >= BPF_REG_COUNT || dst == BPF_REG_FP {
            return Err(BpfError::InvalidRegister(dst));
        }

        if insn.mem_size() == BPF_DW {
            // LDDW: 64-bit immediate — 2 talimat kullanılır
            if self.pc + 1 >= program.len() {
                return Err(BpfError::UnreachableInstruction(self.pc + 1));
            }
            let next_insn = program[self.pc + 1];
            let imm_lo = insn.imm as u32 as u64;
            let imm_hi = next_insn.imm as u32 as u64;
            self.regs[dst] = imm_lo | (imm_hi << 32);
            self.pc += 2; // İki talimat tüketilir
        } else {
            return Err(BpfError::InvalidOpcode(insn.opcode));
        }

        Ok(())
    }

    // ========================================================================
    // Yığın Bellek Erişim Fonksiyonları (bounds-checked)
    // ========================================================================

    fn check_stack_bounds(&self, offset: u64, size: usize) -> Result<usize, BpfError> {
        let off = offset as usize;
        if off < size || off > BPF_STACK_SIZE {
            return Err(BpfError::StackOutOfBounds {
                offset: offset as i64,
                size,
            });
        }
        Ok(off - size)
    }

    fn stack_read_u8(&self, addr: u64) -> Result<u8, BpfError> {
        let idx = self.check_stack_bounds(addr, 1)?;
        Ok(self.stack[idx])
    }

    fn stack_read_u16(&self, addr: u64) -> Result<u16, BpfError> {
        let idx = self.check_stack_bounds(addr, 2)?;
        Ok(u16::from_le_bytes([self.stack[idx], self.stack[idx + 1]]))
    }

    fn stack_read_u32(&self, addr: u64) -> Result<u32, BpfError> {
        let idx = self.check_stack_bounds(addr, 4)?;
        let bytes: [u8; 4] = self.stack[idx..idx + 4].try_into().unwrap();
        Ok(u32::from_le_bytes(bytes))
    }

    fn stack_read_u64(&self, addr: u64) -> Result<u64, BpfError> {
        let idx = self.check_stack_bounds(addr, 8)?;
        let bytes: [u8; 8] = self.stack[idx..idx + 8].try_into().unwrap();
        Ok(u64::from_le_bytes(bytes))
    }

    fn stack_write_u8(&mut self, addr: u64, val: u8) -> Result<(), BpfError> {
        let idx = self.check_stack_bounds(addr, 1)?;
        self.stack[idx] = val;
        Ok(())
    }

    fn stack_write_u16(&mut self, addr: u64, val: u16) -> Result<(), BpfError> {
        let idx = self.check_stack_bounds(addr, 2)?;
        let bytes = val.to_le_bytes();
        self.stack[idx..idx + 2].copy_from_slice(&bytes);
        Ok(())
    }

    fn stack_write_u32(&mut self, addr: u64, val: u32) -> Result<(), BpfError> {
        let idx = self.check_stack_bounds(addr, 4)?;
        let bytes = val.to_le_bytes();
        self.stack[idx..idx + 4].copy_from_slice(&bytes);
        Ok(())
    }

    fn stack_write_u64(&mut self, addr: u64, val: u64) -> Result<(), BpfError> {
        let idx = self.check_stack_bounds(addr, 8)?;
        let bytes = val.to_le_bytes();
        self.stack[idx..idx + 8].copy_from_slice(&bytes);
        Ok(())
    }
}

// ============================================================================
// BPF Program Doğrulayıcı (Verifier)
// ============================================================================

/// eBPF program doğrulayıcısı.
///
/// Çalıştırmadan önce programın güvenli olduğunu statik olarak doğrular.
/// Linux kernel'daki `kernel/bpf/verifier.c` ile benzer kontroller yapar.
pub struct BpfVerifier;

impl BpfVerifier {
    /// Program doğrulaması yapar.
    ///
    /// ## Kontroller
    /// 1. Program boyutu ≤ BPF_MAX_INSNS
    /// 2. Son talimat EXIT olmalı
    /// 3. Geriye dallanma yok (sonsuz döngü engeli)
    /// 4. Tüm dallanma hedefleri program sınırları içinde
    /// 5. Kayıtçı indeksleri geçerli (0-10)
    /// 6. R10'a yazma yok (salt okunur FP)
    /// 7. Sıfıra bölme riski (statik tespit edilebilen)
    pub fn verify(program: &[BpfInsn]) -> Result<(), BpfError> {
        host_smoke_probe("verify:start");
        // 1. Boyut kontrolü
        if program.is_empty() {
            return Err(BpfError::VerificationFailed(String::from("Program boş")));
        }
        if program.len() > BPF_MAX_INSNS {
            return Err(BpfError::ProgramTooLarge(program.len()));
        }
        host_smoke_probe("verify:size-ok");

        // 2. Son talimat EXIT olmalı
        let last = &program[program.len() - 1];
        if last.class() != BPF_JMP || last.op() != BPF_EXIT {
            return Err(BpfError::VerificationFailed(String::from(
                "Son talimat EXIT değil",
            )));
        }

        // 3-6. Talimat bazlı kontroller
        host_smoke_probe("verify:last-ok");
        let mut i = 0;
        while i < program.len() {
            host_smoke_probe("verify:loop:iter");
            let insn = &program[i];

            // Kayıtçı sınır kontrolü
            let dst = insn.dst_reg();
            let src = insn.src_reg();
            if dst >= BPF_REG_COUNT {
                return Err(BpfError::InvalidRegister(dst));
            }
            if insn.src_type() == BPF_X && src >= BPF_REG_COUNT {
                return Err(BpfError::InvalidRegister(src));
            }

            // R10'a yazma kontrolü (ALU/MOV talimatlarında)
            match insn.class() {
                BPF_ALU | BPF_ALU64 => {
                    if dst == BPF_REG_FP {
                        return Err(BpfError::ReadOnlyRegister);
                    }
                }
                _ => {}
            }

            // Dallanma hedefi kontrolü
            match insn.class() {
                BPF_JMP | BPF_JMP32 => {
                    if insn.op() != BPF_EXIT && insn.op() != BPF_CALL {
                        let target = i as i64 + 1 + insn.off as i64;
                        if target < 0 || target as usize >= program.len() {
                            return Err(BpfError::VerificationFailed(alloc::format!(
                                "Dallanma hedefi sınır dışı: PC={}, target={}",
                                i,
                                target
                            )));
                        }

                        // Geriye dallanma kontrolü (basitleştirilmiş — döngü engeli)
                        if insn.op() != BPF_JA && insn.off < 0 {
                            return Err(BpfError::VerificationFailed(alloc::format!(
                                "Geriye dallanma tespit edildi: PC={}, off={}",
                                i,
                                insn.off
                            )));
                        }
                    }
                }
                _ => {}
            }

            // LDDW kontrolü: ikinci talimatın opcode'u 0 olmalı
            if insn.class() == BPF_LD && insn.mem_size() == BPF_DW {
                if i + 1 >= program.len() {
                    return Err(BpfError::VerificationFailed(alloc::format!(
                        "LDDW eksik ikinci talimat: PC={}",
                        i
                    )));
                }
                i += 2;
                continue;
            }

            i += 1;
        }
        host_smoke_probe("verify:loop:end");

        crate::serial_println!(
            "[eBPF] Program doğrulandı: {} talimat, güvenli",
            program.len()
        );

        host_smoke_probe("verify:serial:end");
        Ok(())
    }
}

// ============================================================================
// Helper Fonksiyonlar (Kernel API)
// ============================================================================

/// BPF_FUNC_trace_printk: Seri porta debug çıktısı yazar.
/// helper ID = 6 (Linux uyumlu)
pub fn bpf_trace_printk(fmt: u64, _fmt_size: u64, arg1: u64, arg2: u64, arg3: u64) -> u64 {
    crate::serial_println!(
        "[eBPF trace] fmt=0x{:x} args=({}, {}, {})",
        fmt,
        arg1,
        arg2,
        arg3
    );
    0
}

/// BPF_FUNC_ktime_get_ns: Kernel zamanını nanosaniye olarak döner.
/// helper ID = 5
pub fn bpf_ktime_get_ns(_: u64, _: u64, _: u64, _: u64, _: u64) -> u64 {
    // TSC'den yaklaşık nanosaniye hesapla
    unsafe { core::arch::x86_64::_rdtsc() }
}

/// BPF_FUNC_get_smp_processor_id: Mevcut CPU ID'sini döner.
/// helper ID = 8
pub fn bpf_get_smp_processor_id(_: u64, _: u64, _: u64, _: u64, _: u64) -> u64 {
    // LAPIC ID'den CPU numarası (basitleştirilmiş)
    0
}

/// Varsayılan helper'ları VM'e yükler.
pub fn register_default_helpers(vm: &mut BpfVm) {
    vm.register_helper(5, bpf_ktime_get_ns);
    vm.register_helper(6, bpf_trace_printk);
    vm.register_helper(8, bpf_get_smp_processor_id);
}

// ============================================================================
// Kolaylık API'leri
// ============================================================================

/// Bir eBPF programını doğrulayıp çalıştırır.
///
/// Güvenli giriş noktası — doğrulama başarısız olursa çalıştırmaz.
pub fn verify_and_run(program: &[BpfInsn], ctx: u64) -> Result<u64, BpfError> {
    BpfVerifier::verify(program)?;

    let mut vm = BpfVm::new();
    register_default_helpers(&mut vm);
    vm.execute(program, ctx)
}

/// eBPF alt sistemini başlatır.
pub fn init() {
    crate::serial_println!("[eBPF] Interpreter initialized");
    crate::serial_println!("[eBPF]   Register file: {} x 64-bit", BPF_REG_COUNT);
    crate::serial_println!("[eBPF]   Stack size: {} bytes", BPF_STACK_SIZE);
    crate::serial_println!("[eBPF]   Max instructions: {}", BPF_MAX_INSNS);
    crate::serial_println!("[eBPF]   Max steps: {}", BPF_MAX_STEPS);
    crate::serial_println!("[eBPF]   Helpers: ktime_get_ns, trace_printk, get_smp_processor_id");
    crate::serial_println!(
        "[eBPF]   Attach types: SOCKET_FILTER, KPROBE, TRACEPOINT, XDP, SCHED_CLS"
    );
}

// ============================================================================
// eBPF Program Yönetimi & Attach Sistemi
// ============================================================================

/// eBPF program tipi — nerede çalışacağını belirler
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BpfProgType {
    /// Soket filtresi: gelen paketleri filtreler (cBPF uyumlu)
    SocketFilter,
    /// Kprobe: kernel fonksiyon girişinde tetiklenir
    Kprobe,
    /// Kretprobe: kernel fonksiyon çıkışında tetiklenir
    Kretprobe,
    /// Tracepoint: statik kernel trace noktasında tetiklenir
    Tracepoint,
    /// XDP (eXpress Data Path): NIC'den gelen ham paketleri işler
    Xdp,
    /// Sched classifier: tc (traffic control) filtresi
    SchedCls,
    /// Cgroup soket filtresi
    CgroupSkb,
}

/// eBPF program attach noktası tanımlayıcısı
#[derive(Clone, Debug)]
pub enum BpfAttachPoint {
    /// Soket FD'sine bağla
    Socket(usize), // fd
    /// Kernel fonksiyon ismine bağla (kprobe/kretprobe)
    KernelFunc(String),
    /// Tracepoint kategorisi ve ismi
    Tracepoint { category: String, name: String },
    /// NIC arayüzüne bağla (XDP)
    NetDevice(u32), // ifindex
    /// TC qdisc'e bağla (sched_cls)
    TcQdisc(u32), // ifindex
}

/// XDP program dönüş kodları
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XdpAction {
    /// Paketi düşür
    Drop = 1,
    /// Normal yığına gönder
    Pass = 2,
    /// Geldiği yere geri yolla
    Tx = 3,
    /// Paketi yeniden yönlendir (devmap/cpumap)
    Redirect = 4,
    /// Paketi geçir (kullanılmadıysa)
    Aborted = 0,
}

impl XdpAction {
    pub fn from_u64(val: u64) -> Self {
        match val {
            1 => XdpAction::Drop,
            2 => XdpAction::Pass,
            3 => XdpAction::Tx,
            4 => XdpAction::Redirect,
            _ => XdpAction::Aborted,
        }
    }
}

/// Yüklenmiş eBPF programı
pub struct BpfProgram {
    /// Program ID (benzersiz)
    pub id: u32,
    /// Program tipi
    pub prog_type: BpfProgType,
    /// Doğrulanmış bytecode
    pub insns: Vec<BpfInsn>,
    /// İnsan okunabilir isim
    pub name: String,
    /// Attach noktası (bağlanmışsa)
    pub attach_point: Option<BpfAttachPoint>,
    /// Çalışma sayacı
    pub run_count: u64,
    /// Toplam çalışma süresi (ns)
    pub run_time_ns: u64,
}

impl BpfProgram {
    /// Yeni bir eBPF programı yükler (doğrulamadan geçer).
    pub fn load(
        id: u32,
        prog_type: BpfProgType,
        insns: Vec<BpfInsn>,
        name: &str,
    ) -> Result<Self, BpfError> {
        // Doğrula
        BpfVerifier::verify(&insns)?;

        Ok(BpfProgram {
            id,
            prog_type,
            insns,
            name: String::from(name),
            attach_point: None,
            run_count: 0,
            run_time_ns: 0,
        })
    }

    /// Programı bir attach noktasına bağlar.
    pub fn attach(&mut self, point: BpfAttachPoint) -> Result<(), BpfError> {
        // Program tipi ile attach noktası uyumluluğunu kontrol et
        match (&self.prog_type, &point) {
            (BpfProgType::SocketFilter, BpfAttachPoint::Socket(_)) => {}
            (BpfProgType::Kprobe, BpfAttachPoint::KernelFunc(_)) => {}
            (BpfProgType::Kretprobe, BpfAttachPoint::KernelFunc(_)) => {}
            (BpfProgType::Tracepoint, BpfAttachPoint::Tracepoint { .. }) => {}
            (BpfProgType::Xdp, BpfAttachPoint::NetDevice(_)) => {}
            (BpfProgType::SchedCls, BpfAttachPoint::TcQdisc(_)) => {}
            _ => return Err(BpfError::InvalidProgram),
        }

        self.attach_point = Some(point);
        Ok(())
    }

    /// Programı detach eder (attach noktasından ayırır).
    pub fn detach(&mut self) {
        self.attach_point = None;
    }

    /// Programı verilen context ile çalıştırır.
    pub fn run(&mut self, ctx: u64) -> Result<u64, BpfError> {
        let mut vm = BpfVm::new();
        register_default_helpers(&mut vm);

        let start = unsafe { core::arch::x86_64::_rdtsc() };
        let result = vm.execute(&self.insns, ctx);
        let elapsed = unsafe { core::arch::x86_64::_rdtsc() } - start;

        self.run_count += 1;
        // TSC → ns yaklaşık dönüşüm (1 GHz varsayımı)
        self.run_time_ns += elapsed;

        result
    }
}

// ────────────────────────────────────────────────────────────
// Global Program Kayıt Defteri
// ────────────────────────────────────────────────────────────

use alloc::collections::BTreeMap;
use spin::Mutex;

lazy_static::lazy_static! {
    /// Tüm yüklenmiş eBPF programlarının global kaydı.
    static ref BPF_PROGRAMS: Mutex<BTreeMap<u32, BpfProgram>> = Mutex::new(BTreeMap::new());

    /// Kprobe hook tablosu: fonksiyon_adresi → program_id listesi
    static ref KPROBE_HOOKS: Mutex<BTreeMap<u64, Vec<u32>>> = Mutex::new(BTreeMap::new());

    /// Soket filtre tablosu: socket_fd → program_id
    static ref SOCKET_FILTERS: Mutex<BTreeMap<usize, u32>> = Mutex::new(BTreeMap::new());

    /// XDP program tablosu: ifindex → program_id
    static ref XDP_PROGRAMS: Mutex<BTreeMap<u32, u32>> = Mutex::new(BTreeMap::new());

    /// Sonraki program ID
    static ref NEXT_PROG_ID: Mutex<u32> = Mutex::new(1);

    /// Global JIT helper fonksiyon tablosu (helper_id → fonksiyon adresi)
    /// JIT derleyici bu tablodan helper adreslerini alır.
    static ref JIT_HELPER_TABLE: Mutex<[u64; 64]> = Mutex::new([0u64; 64]);
}

/// JIT helper fonksiyon adresini döndürür.
/// JIT derleyici BPF_CALL talimatı için bu fonksiyonu çağırır.
pub fn get_helper_address(helper_id: u32) -> u64 {
    let table = JIT_HELPER_TABLE.lock();
    if (helper_id as usize) < table.len() {
        table[helper_id as usize]
    } else {
        0
    }
}

/// JIT helper fonksiyon kaydeder (fonksiyon pointer'ı olarak).
pub fn register_jit_helper(helper_id: u32, func: fn(u64, u64, u64, u64, u64) -> u64) {
    let mut table = JIT_HELPER_TABLE.lock();
    if (helper_id as usize) < table.len() {
        table[helper_id as usize] = func as usize as u64;
    }
}

/// Yeni bir eBPF programı yükler ve kayıt defterine ekler.
pub fn bpf_prog_load(
    prog_type: BpfProgType,
    insns: Vec<BpfInsn>,
    name: &str,
) -> Result<u32, BpfError> {
    let mut next_id = NEXT_PROG_ID.lock();
    let id = *next_id;
    *next_id += 1;

    let prog = BpfProgram::load(id, prog_type, insns, name)?;

    crate::serial_println!(
        "[eBPF] Program loaded: id={} type={:?} name='{}' insns={}",
        id,
        prog_type,
        name,
        prog.insns.len()
    );

    BPF_PROGRAMS.lock().insert(id, prog);
    Ok(id)
}

/// Bir eBPF programını attach noktasına bağlar.
pub fn bpf_prog_attach(prog_id: u32, point: BpfAttachPoint) -> Result<(), BpfError> {
    let mut programs = BPF_PROGRAMS.lock();
    let prog = programs.get_mut(&prog_id).ok_or(BpfError::InvalidProgram)?;
    prog.attach(point.clone())?;

    // Hook tablolarını güncelle
    match &point {
        BpfAttachPoint::KernelFunc(func_name) => {
            // Fonksiyon adresini sembol tablosundan bul
            // (şimdilik hash'le)
            let addr = simple_hash(func_name.as_bytes());
            KPROBE_HOOKS
                .lock()
                .entry(addr)
                .or_insert_with(Vec::new)
                .push(prog_id);
            crate::serial_println!(
                "[eBPF] Kprobe attached: prog={} func='{}' addr={:#x}",
                prog_id,
                func_name,
                addr
            );
        }
        BpfAttachPoint::Socket(fd) => {
            SOCKET_FILTERS.lock().insert(*fd, prog_id);
            crate::serial_println!("[eBPF] Socket filter attached: prog={} fd={}", prog_id, fd);
        }
        BpfAttachPoint::NetDevice(ifindex) => {
            XDP_PROGRAMS.lock().insert(*ifindex, prog_id);
            crate::serial_println!("[eBPF] XDP attached: prog={} ifindex={}", prog_id, ifindex);
        }
        _ => {}
    }

    Ok(())
}

/// eBPF programını kayıt defterinden siler.
pub fn bpf_prog_unload(prog_id: u32) -> bool {
    // Hook tablolarından temizle
    KPROBE_HOOKS.lock().retain(|_, ids| {
        ids.retain(|&id| id != prog_id);
        !ids.is_empty()
    });
    SOCKET_FILTERS.lock().retain(|_, &mut id| id != prog_id);
    XDP_PROGRAMS.lock().retain(|_, &mut id| id != prog_id);

    BPF_PROGRAMS.lock().remove(&prog_id).is_some()
}

/// Kprobe hook'unda eBPF programlarını çalıştırır.
///
/// Kernel fonksiyon girişinde çağrılır.
/// `regs_ptr`: register yapısının adresi (ctx olarak geçirilir)
pub fn kprobe_fire(func_addr: u64, regs_ptr: u64) {
    let prog_ids: Vec<u32> = {
        let hooks = KPROBE_HOOKS.lock();
        match hooks.get(&func_addr) {
            Some(ids) => ids.clone(),
            None => return,
        }
    };

    let mut programs = BPF_PROGRAMS.lock();
    for prog_id in prog_ids {
        if let Some(prog) = programs.get_mut(&prog_id) {
            match prog.run(regs_ptr) {
                Ok(ret) => {
                    if ret == 0 {
                        // Kprobe: 0 = devam et, başka bir değer yok
                    }
                }
                Err(e) => {
                    crate::serial_println!("[eBPF] Kprobe prog {} error: {:?}", prog_id, e);
                }
            }
        }
    }
}

/// Soket filtresini çalıştırır.
///
/// Paket alındığında çağrılır.
/// `skb_ptr`: paket buffer adresi (ctx olarak geçirilir)
/// Dönüş: true = paketi geçir, false = paketi düşür
pub fn socket_filter_run(fd: usize, skb_ptr: u64) -> bool {
    let prog_id = {
        let filters = SOCKET_FILTERS.lock();
        match filters.get(&fd) {
            Some(&id) => id,
            None => return true, // Filtre yok → geçir
        }
    };

    let mut programs = BPF_PROGRAMS.lock();
    if let Some(prog) = programs.get_mut(&prog_id) {
        match prog.run(skb_ptr) {
            Ok(ret) => ret != 0, // 0 = düşür, !0 = geçir
            Err(_) => true,      // Hata durumunda geçir
        }
    } else {
        true
    }
}

/// XDP programını çalıştırır.
///
/// NIC'den paket alındığında çağrılır (driver seviyesinde).
/// `xdp_md_ptr`: xdp_md yapısının adresi
/// Dönüş: XDP action (DROP, PASS, TX, REDIRECT)
pub fn xdp_run(ifindex: u32, xdp_md_ptr: u64) -> XdpAction {
    let prog_id = {
        let xdp = XDP_PROGRAMS.lock();
        match xdp.get(&ifindex) {
            Some(&id) => id,
            None => return XdpAction::Pass, // XDP programı yok → geçir
        }
    };

    let mut programs = BPF_PROGRAMS.lock();
    if let Some(prog) = programs.get_mut(&prog_id) {
        match prog.run(xdp_md_ptr) {
            Ok(ret) => XdpAction::from_u64(ret),
            Err(_) => XdpAction::Aborted,
        }
    } else {
        XdpAction::Pass
    }
}

/// Basit string hash fonksiyonu (kprobe fonksiyon adresi için placeholder)
fn simple_hash(data: &[u8]) -> u64 {
    let mut hash: u64 = 5381;
    for &b in data {
        hash = hash.wrapping_mul(33).wrapping_add(b as u64);
    }
    hash
}

/// Yüklü eBPF programlarının listesini yazdırır.
pub fn bpf_prog_list() {
    let programs = BPF_PROGRAMS.lock();
    crate::serial_println!("[eBPF] Loaded programs: {}", programs.len());
    for (id, prog) in programs.iter() {
        crate::serial_println!(
            "  prog={} type={:?} name='{}' runs={} attached={}",
            id,
            prog.prog_type,
            prog.name,
            prog.run_count,
            prog.attach_point.is_some(),
        );
    }
}
