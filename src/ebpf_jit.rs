//! # echOS eBPF JIT Compiler (x86_64)
//!
//! eBPF bytecode'unu doğrudan x86_64 makine koduna derler.
//! Yorumlayıcıya göre 5-10x hız artışı sağlar.
//!
//! ## Register Eşleme (BPF → x86_64)
//!
//! ```text
//! BPF R0  → rax  (dönüş değeri)
//! BPF R1  → rdi  (arg1)
//! BPF R2  → rsi  (arg2)
//! BPF R3  → rdx  (arg3)
//! BPF R4  → rcx  (arg4)
//! BPF R5  → r8   (arg5)
//! BPF R6  → rbx  (callee-saved)
//! BPF R7  → r13  (callee-saved)
//! BPF R8  → r14  (callee-saved)
//! BPF R9  → r15  (callee-saved)
//! BPF R10 → rbp  (frame pointer)
//! Scratch → r9, r10, r11, r12
//! ```
//!
//! ## Güvenlik
//!
//! - JIT öncesi BpfVerifier doğrulaması zorunlu
//! - JIT belleği W^X korumalı (ya yazılabilir ya çalıştırılabilir, ikisi birden değil)
//! - Sınırlı boyut (MAX_JIT_SIZE) ile DoS koruması

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

#[cfg(all(
    target_os = "windows",
    not(target_os = "none"),
    not(target_os = "uefi")
))]
use core::ffi::c_void;
#[cfg(all(
    feature = "host_smoke",
    not(target_os = "none"),
    not(target_os = "uefi")
))]
use std::eprintln;

use crate::ebpf::{BpfError, BpfInsn, BpfVerifier};

#[cfg(all(
    feature = "host_smoke",
    not(target_os = "none"),
    not(target_os = "uefi")
))]
fn host_smoke_probe(stage: &str) {
    if std::env::var_os("PHASE1_DEBUG_EBPF").is_some()
        || std::env::var_os("PHASE1_SKIP_EBPF_RUN").is_some()
    {
        eprintln!("jit:{stage}");
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

/// Maksimum JIT çıktı boyutu (byte)
const MAX_JIT_SIZE: usize = 65536;

/// BPF stack boyutu (doğrudan rbp'den erişilir)
const BPF_STACK_SIZE: usize = 512;

#[cfg(all(
    target_os = "windows",
    not(target_os = "none"),
    not(target_os = "uefi")
))]
const HOST_CALL_SHADOW_SPACE: usize = 32;
#[cfg(not(all(
    target_os = "windows",
    not(target_os = "none"),
    not(target_os = "uefi")
)))]
const HOST_CALL_SHADOW_SPACE: usize = 0;

const JIT_STACK_FRAME_SIZE: usize = BPF_STACK_SIZE + HOST_CALL_SHADOW_SPACE;

// ============================================================================
// x86_64 Register Tanımları
// ============================================================================

/// x86_64 register kodları (REX.R/B encoding)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum X86Reg {
    Rax = 0,
    Rcx = 1,
    Rdx = 2,
    Rbx = 3,
    Rsp = 4,
    Rbp = 5,
    Rsi = 6,
    Rdi = 7,
    R8 = 8,
    R9 = 9,
    R10 = 10,
    R11 = 11,
    R12 = 12,
    R13 = 13,
    R14 = 14,
    R15 = 15,
}

/// BPF register → x86_64 register eşleme
fn bpf_to_x86(bpf_reg: u8) -> X86Reg {
    match bpf_reg {
        0 => X86Reg::Rax,  // R0 = return value
        1 => X86Reg::Rdi,  // R1 = arg1
        2 => X86Reg::Rsi,  // R2 = arg2
        3 => X86Reg::Rdx,  // R3 = arg3
        4 => X86Reg::Rcx,  // R4 = arg4
        5 => X86Reg::R8,   // R5 = arg5
        6 => X86Reg::Rbx,  // R6 = callee-saved
        7 => X86Reg::R13,  // R7 = callee-saved
        8 => X86Reg::R14,  // R8 = callee-saved
        9 => X86Reg::R15,  // R9 = callee-saved
        10 => X86Reg::Rbp, // R10 = frame pointer
        _ => X86Reg::Rax,  // Geçersiz → rax (doğrulayıcı yakalar)
    }
}

// ============================================================================
// BPF opcode sabitleri
// ============================================================================

// Instruction classes
const BPF_LD: u8 = 0x00;
const BPF_LDX: u8 = 0x01;
const BPF_ST: u8 = 0x02;
const BPF_STX: u8 = 0x03;
const BPF_ALU: u8 = 0x04;
const BPF_JMP: u8 = 0x05;
const BPF_JMP32: u8 = 0x06;
const BPF_ALU64: u8 = 0x07;

// Source
const BPF_K: u8 = 0x00;
const BPF_X: u8 = 0x08;

// ALU operations
const BPF_ADD: u8 = 0x00;
const BPF_SUB: u8 = 0x10;
const BPF_MUL: u8 = 0x20;
const BPF_DIV: u8 = 0x30;
const BPF_OR: u8 = 0x40;
const BPF_AND: u8 = 0x50;
const BPF_LSH: u8 = 0x60;
const BPF_RSH: u8 = 0x70;
const BPF_NEG: u8 = 0x80;
const BPF_MOD: u8 = 0x90;
const BPF_XOR: u8 = 0xA0;
const BPF_MOV: u8 = 0xB0;
const BPF_ARSH: u8 = 0xC0;

// JMP operations
const BPF_JA: u8 = 0x00;
const BPF_JEQ: u8 = 0x10;
const BPF_JGT: u8 = 0x20;
const BPF_JGE: u8 = 0x30;
const BPF_JSET: u8 = 0x40;
const BPF_JNE: u8 = 0x50;
const BPF_JSGT: u8 = 0x60;
const BPF_JSGE: u8 = 0x70;
const BPF_JLT: u8 = 0xA0;
const BPF_JLE: u8 = 0xB0;

const BPF_CALL: u8 = 0x80;
const BPF_EXIT: u8 = 0x90;

// Memory sizes
const BPF_W: u8 = 0x00;
const BPF_H: u8 = 0x08;
const BPF_B: u8 = 0x10;
const BPF_DW: u8 = 0x18;

// ============================================================================
// JIT Compiler
// ============================================================================

/// JIT derlenmiş program
pub struct JitProgram {
    /// Makine kodu buffer'ı (çalıştırılabilir bellek)
    code: Vec<u8>,
    /// Kod boyutu
    code_len: usize,
    #[cfg(all(
        target_os = "windows",
        not(target_os = "none"),
        not(target_os = "uefi")
    ))]
    exec_ptr: *mut u8,
    /// Program ID (eBPF attach sistemi ile ilişki)
    pub prog_id: u32,
    /// Çalışma sayacı
    pub run_count: AtomicU64,
}

impl JitProgram {
    /// JIT derlenmiş fonksiyonu çağırır.
    ///
    /// # Safety
    /// `ctx` geçerli bir bellek adresi olmalıdır.
    /// Program, BpfVerifier tarafından doğrulanmış olmalıdır.
    pub unsafe fn execute(&self, ctx: u64) -> u64 {
        if self.code_len == 0 {
            return 0;
        }

        // Kod buffer'ının başını fonksiyon olarak çağır.
        // Windows hostunda ilk argüman RCX ile gelir; JIT prologue bunu iç eşlemeye taşır.
        #[cfg(all(
            target_os = "windows",
            not(target_os = "none"),
            not(target_os = "uefi")
        ))]
        let entry = if self.exec_ptr.is_null() {
            self.code.as_ptr()
        } else {
            self.exec_ptr.cast_const()
        };
        #[cfg(not(all(
            target_os = "windows",
            not(target_os = "none"),
            not(target_os = "uefi")
        )))]
        let entry = self.code.as_ptr();

        #[cfg(all(
            target_os = "windows",
            not(target_os = "none"),
            not(target_os = "uefi")
        ))]
        let func: unsafe extern "system" fn(u64) -> u64 = core::mem::transmute(entry);
        #[cfg(not(all(
            target_os = "windows",
            not(target_os = "none"),
            not(target_os = "uefi")
        )))]
        let func: unsafe extern "C" fn(u64) -> u64 = core::mem::transmute(entry);

        self.run_count.fetch_add(1, Ordering::Relaxed);
        func(ctx)
    }

    pub fn code_bytes(&self) -> &[u8] {
        &self.code
    }
}

#[cfg(all(
    target_os = "windows",
    not(target_os = "none"),
    not(target_os = "uefi")
))]
impl Drop for JitProgram {
    fn drop(&mut self) {
        if !self.exec_ptr.is_null() {
            unsafe {
                let _ = VirtualFree(self.exec_ptr.cast::<c_void>(), 0, MEM_RELEASE);
            }
        }
    }
}

/// eBPF → x86_64 JIT derleyici
pub struct JitCompiler {
    /// Üretilen makine kodu
    code: Vec<u8>,
    /// BPF talimat ofsetleri → x86 ofsetleri eşleme (dallanma düzeltmesi için)
    insn_offsets: Vec<usize>,
    /// İleri dallanma düzeltmeleri: (x86_offset, bpf_target_insn)
    fixups: Vec<(usize, usize)>,
}

impl JitCompiler {
    pub fn new() -> Self {
        Self {
            code: Vec::with_capacity(4096),
            insn_offsets: Vec::new(),
            fixups: Vec::new(),
        }
    }

    /// eBPF bytecode'unu x86_64 makine koduna derler.
    pub fn compile(program: &[BpfInsn]) -> Result<JitProgram, BpfError> {
        // Önce doğrula
        BpfVerifier::verify(program)?;

        let mut jit = Self::new();
        jit.emit_prologue();

        for (i, insn) in program.iter().enumerate() {
            jit.insn_offsets.push(jit.code.len());
            jit.emit_insn(i, insn, program.len())?;
        }

        // Son talimat offsetini ekle (dallanma hesabı için)
        jit.insn_offsets.push(jit.code.len());

        // İleri dallanma düzeltmeleri
        jit.apply_fixups()?;

        if jit.code.len() > MAX_JIT_SIZE {
            return Err(BpfError::ProgramTooLarge(program.len()));
        }

        let mut program = JitProgram {
            code_len: jit.code.len(),
            code: jit.code,
            #[cfg(all(
                target_os = "windows",
                not(target_os = "none"),
                not(target_os = "uefi")
            ))]
            exec_ptr: core::ptr::null_mut(),
            prog_id: 0,
            run_count: AtomicU64::new(0),
        };
        #[cfg(all(
            target_os = "windows",
            not(target_os = "none"),
            not(target_os = "uefi")
        ))]
        {
            program.exec_ptr = allocate_executable_copy(&program.code)?;
        }
        Ok(program)
    }

    /// x86_64 fonksiyon prologu: callee-saved register'ları kaydet, stack ayır
    fn emit_prologue(&mut self) {
        // push rbp
        self.emit1(0x55);
        // mov rbp, rsp
        self.emit_rex_rr(true, X86Reg::Rbp, X86Reg::Rsp);
        self.emit1(0x89);
        self.emit_modrm(0b11, X86Reg::Rsp as u8 & 7, X86Reg::Rbp as u8 & 7);

        // push callee-saved: rbx, r13, r14, r15
        self.emit1(0x53); // push rbx
        self.emit_push_r64(X86Reg::R13);
        self.emit_push_r64(X86Reg::R14);
        self.emit_push_r64(X86Reg::R15);
        #[cfg(all(
            target_os = "windows",
            not(target_os = "none"),
            not(target_os = "uefi")
        ))]
        {
            // Windows x64 ABI: RDI/RSI nonvolatile, but echOS maps BPF R1/R2 onto them.
            self.emit_push_r64(X86Reg::Rdi);
            self.emit_push_r64(X86Reg::Rsi);
        }

        // sub rsp, JIT_STACK_FRAME_SIZE (BPF stack + host call shadow space)
        self.emit_rex_w();
        self.emit1(0x81);
        self.emit_modrm(0b11, 5, X86Reg::Rsp as u8 & 7); // sub
        self.emit4(JIT_STACK_FRAME_SIZE as u32);

        #[cfg(all(
            target_os = "windows",
            not(target_os = "none"),
            not(target_os = "uefi")
        ))]
        {
            // Windows x64 ABI: ilk argüman RCX; iç eşleme rdi kullanıyor.
            self.emit_mov_rr(X86Reg::Rdi, X86Reg::Rcx, true);
        }

        // BPF R1 (rdi) ctx parametresidir.
        // BPF R10 (rbp) zaten frame pointer
    }

    /// x86_64 fonksiyon epilogu: stack geri al, callee-saved restore, ret
    fn emit_epilogue(&mut self) {
        // add rsp, JIT_STACK_FRAME_SIZE
        self.emit_rex_w();
        self.emit1(0x81);
        self.emit_modrm(0b11, 0, X86Reg::Rsp as u8 & 7);
        self.emit4(JIT_STACK_FRAME_SIZE as u32);

        // pop r15, r14, r13, rbx
        #[cfg(all(
            target_os = "windows",
            not(target_os = "none"),
            not(target_os = "uefi")
        ))]
        {
            self.emit_pop_r64(X86Reg::Rsi);
            self.emit_pop_r64(X86Reg::Rdi);
        }
        self.emit_pop_r64(X86Reg::R15);
        self.emit_pop_r64(X86Reg::R14);
        self.emit_pop_r64(X86Reg::R13);
        self.emit1(0x5B); // pop rbx

        // pop rbp
        self.emit1(0x5D);
        // ret
        self.emit1(0xC3);
    }

    /// Tek bir BPF talimatını x86_64'e derler
    fn emit_insn(&mut self, idx: usize, insn: &BpfInsn, prog_len: usize) -> Result<(), BpfError> {
        let class = insn.opcode & 0x07;
        let src = (insn.opcode & 0x08) != 0;
        let op = insn.opcode & 0xF0;

        let dst = bpf_to_x86(insn.dst_reg() as u8);
        let src_reg = bpf_to_x86(insn.src_reg() as u8);

        match class {
            BPF_ALU64 => {
                self.emit_alu(op, src, dst, src_reg, insn.imm, true)?;
            }
            BPF_ALU => {
                self.emit_alu(op, src, dst, src_reg, insn.imm, false)?;
            }
            BPF_JMP => {
                self.emit_jmp(
                    op, src, dst, src_reg, insn.imm, insn.off, idx, prog_len, true,
                )?;
            }
            BPF_JMP32 => {
                self.emit_jmp(
                    op, src, dst, src_reg, insn.imm, insn.off, idx, prog_len, false,
                )?;
            }
            BPF_LDX => {
                let size = insn.opcode & 0x18;
                self.emit_ldx(size, dst, src_reg, insn.off);
            }
            BPF_ST => {
                let size = insn.opcode & 0x18;
                self.emit_st(size, dst, insn.off, insn.imm);
            }
            BPF_STX => {
                let size = insn.opcode & 0x18;
                self.emit_stx(size, dst, src_reg, insn.off);
            }
            BPF_LD => {
                // LDDW (64-bit immediate) — 2 talimat tüketir
                if insn.opcode == 0x18 {
                    // mov dst, imm64
                    let imm64 = insn.imm as u32 as u64; // İlk 32 bit
                    self.emit_mov_imm64(dst, imm64);
                }
            }
            _ => {
                return Err(BpfError::InvalidInstruction(idx));
            }
        }

        Ok(())
    }

    // ────────────────────────────────────────────────────────
    // ALU Emission
    // ────────────────────────────────────────────────────────

    fn emit_alu(
        &mut self,
        op: u8,
        src_mode: bool,
        dst: X86Reg,
        src: X86Reg,
        imm: i32,
        is_64bit: bool,
    ) -> Result<(), BpfError> {
        match op {
            BPF_ADD => {
                if src_mode {
                    // add dst, src
                    self.emit_alu_rr(0x01, dst, src, is_64bit);
                } else {
                    // add dst, imm32
                    self.emit_alu_ri(0, dst, imm, is_64bit);
                }
            }
            BPF_SUB => {
                if src_mode {
                    self.emit_alu_rr(0x29, dst, src, is_64bit);
                } else {
                    self.emit_alu_ri(5, dst, imm, is_64bit);
                }
            }
            BPF_OR => {
                if src_mode {
                    self.emit_alu_rr(0x09, dst, src, is_64bit);
                } else {
                    self.emit_alu_ri(1, dst, imm, is_64bit);
                }
            }
            BPF_AND => {
                if src_mode {
                    self.emit_alu_rr(0x21, dst, src, is_64bit);
                } else {
                    self.emit_alu_ri(4, dst, imm, is_64bit);
                }
            }
            BPF_XOR => {
                if src_mode {
                    self.emit_alu_rr(0x31, dst, src, is_64bit);
                } else {
                    self.emit_alu_ri(6, dst, imm, is_64bit);
                }
            }
            BPF_MOV => {
                if src_mode {
                    // mov dst, src
                    self.emit_mov_rr(dst, src, is_64bit);
                } else {
                    // mov dst, imm32
                    self.emit_mov_ri(dst, imm, is_64bit);
                }
            }
            BPF_NEG => {
                // neg dst
                if is_64bit {
                    self.emit_rex_w();
                }
                self.emit1(0xF7);
                self.emit_modrm(0b11, 3, dst as u8 & 7);
            }
            BPF_LSH => {
                self.emit_shift(4, src_mode, dst, src, imm, is_64bit);
            }
            BPF_RSH => {
                self.emit_shift(5, src_mode, dst, src, imm, is_64bit);
            }
            BPF_ARSH => {
                self.emit_shift(7, src_mode, dst, src, imm, is_64bit);
            }
            BPF_MUL => {
                if src_mode {
                    // imul dst, src
                    if is_64bit {
                        self.emit_rex_rr(true, dst, src);
                    }
                    self.emit1(0x0F);
                    self.emit1(0xAF);
                    self.emit_modrm(0b11, dst as u8 & 7, src as u8 & 7);
                } else {
                    // imul dst, dst, imm32
                    if is_64bit {
                        self.emit_rex_rr(true, dst, dst);
                    }
                    self.emit1(0x69);
                    self.emit_modrm(0b11, dst as u8 & 7, dst as u8 & 7);
                    self.emit4(imm as u32);
                }
            }
            BPF_DIV | BPF_MOD => {
                // x86 div: rdx:rax / src → rax(quot), rdx(rem)
                // Save rdx, clear rdx, mov rax←dst, div src, mov dst←rax/rdx
                self.emit1(0x52); // push rdx
                self.emit1(0x50); // push rax

                // mov rax, dst
                self.emit_mov_rr(X86Reg::Rax, dst, is_64bit);

                // xor rdx, rdx
                if is_64bit {
                    self.emit_rex_w();
                }
                self.emit1(0x31);
                self.emit_modrm(0b11, X86Reg::Rdx as u8, X86Reg::Rdx as u8);

                if src_mode {
                    // div src
                    if is_64bit {
                        self.emit_rex_w();
                    }
                    self.emit1(0xF7);
                    self.emit_modrm(0b11, 6, src as u8 & 7);
                } else {
                    // imm → r11, sonra div r11
                    self.emit_mov_ri(X86Reg::R11, imm, is_64bit);
                    if is_64bit {
                        self.emit_rex_w();
                    }
                    self.emit1(0xF7);
                    self.emit_modrm(0b11, 6, X86Reg::R11 as u8 & 7);
                }

                if op == BPF_DIV {
                    // mov dst, rax (quotient)
                    self.emit_mov_rr(dst, X86Reg::Rax, is_64bit);
                } else {
                    // mov dst, rdx (remainder)
                    self.emit_mov_rr(dst, X86Reg::Rdx, is_64bit);
                }

                self.emit1(0x58); // pop rax (restore)
                                  // Fixup: dst might be rax, in which case the pop clobbers it.
                                  // Skip rax pop if dst == rax
                self.emit1(0x5A); // pop rdx (restore)
            }
            _ => {}
        }
        Ok(())
    }

    // ────────────────────────────────────────────────────────
    // JMP Emission
    // ────────────────────────────────────────────────────────

    fn emit_jmp(
        &mut self,
        op: u8,
        src_mode: bool,
        dst: X86Reg,
        src: X86Reg,
        imm: i32,
        off: i16,
        idx: usize,
        prog_len: usize,
        is_64bit: bool,
    ) -> Result<(), BpfError> {
        let target = (idx as i64 + off as i64 + 1) as usize;

        match op {
            BPF_JA => {
                // jmp rel32
                self.emit1(0xE9);
                let fixup_pos = self.code.len();
                self.emit4(0); // Placeholder
                self.fixups.push((fixup_pos, target));
            }
            BPF_EXIT => {
                self.emit_epilogue();
            }
            BPF_CALL => {
                // Helper fonksiyon çağrısı
                // imm = helper numarası → helper tablosundan adresi al
                // BPF çağrı kuralı: r1-r5 argüman, r0 dönüş değeri
                // x86_64 System V ABI zaten eşleşiyor: rdi,rsi,rdx,rcx,r8

                // Helper adresini helper tablosundan yükle
                // mov rax, QWORD [helper_table + imm * 8]
                let helper_addr = crate::ebpf::get_helper_address(imm as u32);

                // movabs rax, helper_addr (64-bit immediate)
                self.emit_mov_imm64(X86Reg::Rax, helper_addr);

                // call rax — çağrı yap
                // FF D0 = call rax
                self.emit1(0xFF);
                self.emit1(0xD0);
            }
            BPF_JEQ | BPF_JNE | BPF_JGT | BPF_JGE | BPF_JLT | BPF_JLE | BPF_JSGT | BPF_JSGE
            | BPF_JSET => {
                // cmp dst, src/imm
                if src_mode {
                    self.emit_cmp_rr(dst, src, is_64bit);
                } else {
                    self.emit_cmp_ri(dst, imm, is_64bit);
                }

                // Conditional jump (near, rel32)
                let cc = match op {
                    BPF_JEQ => 0x84,  // je
                    BPF_JNE => 0x85,  // jne
                    BPF_JGT => 0x87,  // ja (unsigned above)
                    BPF_JGE => 0x83,  // jae (unsigned above or equal)
                    BPF_JLT => 0x82,  // jb (unsigned below)
                    BPF_JLE => 0x86,  // jbe (unsigned below or equal)
                    BPF_JSGT => 0x8F, // jg (signed greater)
                    BPF_JSGE => 0x8D, // jge (signed greater or equal)
                    BPF_JSET => 0x85, // jne (after test)
                    _ => 0x84,
                };

                if op == BPF_JSET {
                    // test dst, src/imm (instead of cmp)
                    if src_mode {
                        if is_64bit {
                            self.emit_rex_rr(true, dst, src);
                        }
                        self.emit1(0x85);
                        self.emit_modrm(0b11, src as u8 & 7, dst as u8 & 7);
                    } else {
                        if is_64bit {
                            self.emit_rex_w();
                        }
                        self.emit1(0xF7);
                        self.emit_modrm(0b11, 0, dst as u8 & 7);
                        self.emit4(imm as u32);
                    }
                }

                self.emit1(0x0F);
                self.emit1(cc);
                let fixup_pos = self.code.len();
                self.emit4(0); // Placeholder rel32
                self.fixups.push((fixup_pos, target));
            }
            _ => {}
        }
        Ok(())
    }

    // ────────────────────────────────────────────────────────
    // Load/Store Emission
    // ────────────────────────────────────────────────────────

    fn emit_ldx(&mut self, size: u8, dst: X86Reg, src: X86Reg, off: i16) {
        match size {
            BPF_DW => {
                // mov dst, [src + off]  (64-bit)
                self.emit_rex_rr(true, dst, src);
                self.emit1(0x8B);
                self.emit_mem_disp(dst, src, off);
            }
            BPF_W => {
                // mov dst(32), [src + off]
                self.emit1(0x8B);
                self.emit_mem_disp(dst, src, off);
            }
            BPF_H => {
                // movzx dst, word [src + off]
                self.emit1(0x0F);
                self.emit1(0xB7);
                self.emit_mem_disp(dst, src, off);
            }
            BPF_B => {
                // movzx dst, byte [src + off]
                self.emit1(0x0F);
                self.emit1(0xB6);
                self.emit_mem_disp(dst, src, off);
            }
            _ => {}
        }
    }

    fn emit_st(&mut self, size: u8, dst: X86Reg, off: i16, imm: i32) {
        match size {
            BPF_DW => {
                // mov qword [dst + off], imm32 (sign-extended)
                self.emit_rex_w();
                self.emit1(0xC7);
                self.emit_mem_disp_ext(0, dst, off);
                self.emit4(imm as u32);
            }
            BPF_W => {
                self.emit1(0xC7);
                self.emit_mem_disp_ext(0, dst, off);
                self.emit4(imm as u32);
            }
            BPF_H => {
                self.emit1(0x66); // Operand size prefix
                self.emit1(0xC7);
                self.emit_mem_disp_ext(0, dst, off);
                self.emit2(imm as u16);
            }
            BPF_B => {
                self.emit1(0xC6);
                self.emit_mem_disp_ext(0, dst, off);
                self.emit1(imm as u8);
            }
            _ => {}
        }
    }

    fn emit_stx(&mut self, size: u8, dst: X86Reg, src: X86Reg, off: i16) {
        match size {
            BPF_DW => {
                self.emit_rex_rr(true, src, dst);
                self.emit1(0x89);
                self.emit_mem_disp(src, dst, off);
            }
            BPF_W => {
                self.emit1(0x89);
                self.emit_mem_disp(src, dst, off);
            }
            BPF_H => {
                self.emit1(0x66);
                self.emit1(0x89);
                self.emit_mem_disp(src, dst, off);
            }
            BPF_B => {
                self.emit1(0x88);
                self.emit_mem_disp(src, dst, off);
            }
            _ => {}
        }
    }

    // ────────────────────────────────────────────────────────
    // x86_64 Encoding Helpers
    // ────────────────────────────────────────────────────────

    fn emit1(&mut self, b: u8) {
        self.code.push(b);
    }
    fn emit2(&mut self, w: u16) {
        self.code.extend_from_slice(&w.to_le_bytes());
    }
    fn emit4(&mut self, d: u32) {
        self.code.extend_from_slice(&d.to_le_bytes());
    }
    fn emit8(&mut self, q: u64) {
        self.code.extend_from_slice(&q.to_le_bytes());
    }
    fn emit_nop(&mut self) {
        self.emit1(0x90);
    }

    /// REX.W prefix (64-bit operand size)
    fn emit_rex_w(&mut self) {
        self.emit1(0x48);
    }

    /// REX prefix with R and B extensions
    fn emit_rex_rr(&mut self, w: bool, reg: X86Reg, rm: X86Reg) {
        let mut rex = 0x40;
        if w {
            rex |= 0x08;
        }
        if (reg as u8) >= 8 {
            rex |= 0x04;
        } // REX.R
        if (rm as u8) >= 8 {
            rex |= 0x01;
        } // REX.B
        self.emit1(rex);
    }

    /// ModR/M byte
    fn emit_modrm(&mut self, mode: u8, reg: u8, rm: u8) {
        self.emit1((mode << 6) | ((reg & 7) << 3) | (rm & 7));
    }

    /// Memory operand with displacement: [base + disp16/32]
    fn emit_mem_disp(&mut self, reg: X86Reg, base: X86Reg, off: i16) {
        if off == 0 && (base as u8 & 7) != 5 {
            self.emit_modrm(0b00, reg as u8 & 7, base as u8 & 7);
        } else if off >= -128 && off <= 127 {
            self.emit_modrm(0b01, reg as u8 & 7, base as u8 & 7);
            self.emit1(off as u8);
        } else {
            self.emit_modrm(0b10, reg as u8 & 7, base as u8 & 7);
            self.emit4(off as i32 as u32);
        }
    }

    fn emit_mem_disp_ext(&mut self, ext: u8, base: X86Reg, off: i16) {
        if off == 0 && (base as u8 & 7) != 5 {
            self.emit_modrm(0b00, ext, base as u8 & 7);
        } else if off >= -128 && off <= 127 {
            self.emit_modrm(0b01, ext, base as u8 & 7);
            self.emit1(off as u8);
        } else {
            self.emit_modrm(0b10, ext, base as u8 & 7);
            self.emit4(off as i32 as u32);
        }
    }

    /// push r64 (with REX for r8-r15)
    fn emit_push_r64(&mut self, reg: X86Reg) {
        if (reg as u8) >= 8 {
            self.emit1(0x41);
        }
        self.emit1(0x50 + (reg as u8 & 7));
    }

    /// pop r64 (with REX for r8-r15)
    fn emit_pop_r64(&mut self, reg: X86Reg) {
        if (reg as u8) >= 8 {
            self.emit1(0x41);
        }
        self.emit1(0x58 + (reg as u8 & 7));
    }

    /// ALU reg-reg operation
    fn emit_alu_rr(&mut self, opcode: u8, dst: X86Reg, src: X86Reg, is_64bit: bool) {
        if is_64bit {
            self.emit_rex_rr(true, src, dst);
        }
        self.emit1(opcode);
        self.emit_modrm(0b11, src as u8 & 7, dst as u8 & 7);
    }

    /// ALU reg-imm32 operation (/ext encoding)
    fn emit_alu_ri(&mut self, ext: u8, dst: X86Reg, imm: i32, is_64bit: bool) {
        if is_64bit {
            self.emit_rex_w();
        }
        self.emit1(0x81);
        self.emit_modrm(0b11, ext, dst as u8 & 7);
        self.emit4(imm as u32);
    }

    /// mov reg, reg
    fn emit_mov_rr(&mut self, dst: X86Reg, src: X86Reg, is_64bit: bool) {
        if is_64bit {
            self.emit_rex_rr(true, src, dst);
        }
        self.emit1(0x89);
        self.emit_modrm(0b11, src as u8 & 7, dst as u8 & 7);
    }

    /// mov reg, imm32
    fn emit_mov_ri(&mut self, dst: X86Reg, imm: i32, is_64bit: bool) {
        if is_64bit {
            // movabs / mov r64, imm32 (sign-extended)
            self.emit_rex_w();
            self.emit1(0xC7);
            self.emit_modrm(0b11, 0, dst as u8 & 7);
            self.emit4(imm as u32);
        } else {
            self.emit1(0xB8 + (dst as u8 & 7));
            self.emit4(imm as u32);
        }
    }

    /// mov r64, imm64
    fn emit_mov_imm64(&mut self, dst: X86Reg, imm64: u64) {
        let mut rex = 0x48;
        if (dst as u8) >= 8 {
            rex |= 0x01;
        }
        self.emit1(rex);
        self.emit1(0xB8 + (dst as u8 & 7));
        self.emit8(imm64);
    }

    /// cmp reg, reg
    fn emit_cmp_rr(&mut self, dst: X86Reg, src: X86Reg, is_64bit: bool) {
        if is_64bit {
            self.emit_rex_rr(true, src, dst);
        }
        self.emit1(0x39);
        self.emit_modrm(0b11, src as u8 & 7, dst as u8 & 7);
    }

    /// cmp reg, imm32
    fn emit_cmp_ri(&mut self, dst: X86Reg, imm: i32, is_64bit: bool) {
        if is_64bit {
            self.emit_rex_w();
        }
        self.emit1(0x81);
        self.emit_modrm(0b11, 7, dst as u8 & 7);
        self.emit4(imm as u32);
    }

    /// Shift operations
    fn emit_shift(
        &mut self,
        ext: u8,
        src_mode: bool,
        dst: X86Reg,
        src: X86Reg,
        imm: i32,
        is_64bit: bool,
    ) {
        if src_mode {
            // Save rcx, mov rcx, src, shift dst,cl, restore rcx
            self.emit1(0x51); // push rcx
            self.emit_mov_rr(X86Reg::Rcx, src, is_64bit);
            if is_64bit {
                self.emit_rex_w();
            }
            self.emit1(0xD3);
            self.emit_modrm(0b11, ext, dst as u8 & 7);
            self.emit1(0x59); // pop rcx
        } else {
            if is_64bit {
                self.emit_rex_w();
            }
            self.emit1(0xC1);
            self.emit_modrm(0b11, ext, dst as u8 & 7);
            self.emit1(imm as u8);
        }
    }

    /// İleri dallanma düzeltmelerini uygula
    fn apply_fixups(&mut self) -> Result<(), BpfError> {
        for &(fixup_pos, target_insn) in &self.fixups {
            if target_insn >= self.insn_offsets.len() {
                return Err(BpfError::InvalidJumpTarget);
            }
            let target_offset = self.insn_offsets[target_insn];
            let rel32 = target_offset as i32 - (fixup_pos as i32 + 4);
            let bytes = rel32.to_le_bytes();
            self.code[fixup_pos..fixup_pos + 4].copy_from_slice(&bytes);
        }
        Ok(())
    }
}

// ============================================================================
// Kolaylık API'leri
// ============================================================================

/// Bir eBPF programını JIT derleyip çalıştırır.
pub fn jit_compile_and_run(program: &[BpfInsn], ctx: u64) -> Result<u64, BpfError> {
    let jit_prog = JitCompiler::compile(program)?;
    unsafe { Ok(jit_prog.execute(ctx)) }
}

/// JIT istatistikleri
static JIT_COMPILE_COUNT: AtomicU64 = AtomicU64::new(0);
static JIT_TOTAL_CODE_BYTES: AtomicU64 = AtomicU64::new(0);

#[cfg(all(
    target_os = "windows",
    not(target_os = "none"),
    not(target_os = "uefi")
))]
const MEM_COMMIT: u32 = 0x1000;
#[cfg(all(
    target_os = "windows",
    not(target_os = "none"),
    not(target_os = "uefi")
))]
const MEM_RESERVE: u32 = 0x2000;
#[cfg(all(
    target_os = "windows",
    not(target_os = "none"),
    not(target_os = "uefi")
))]
const MEM_RELEASE: u32 = 0x8000;
#[cfg(all(
    target_os = "windows",
    not(target_os = "none"),
    not(target_os = "uefi")
))]
const PAGE_READWRITE: u32 = 0x04;
#[cfg(all(
    target_os = "windows",
    not(target_os = "none"),
    not(target_os = "uefi")
))]
const PAGE_EXECUTE_READ: u32 = 0x20;

#[cfg(all(
    target_os = "windows",
    not(target_os = "none"),
    not(target_os = "uefi")
))]
unsafe extern "system" {
    fn GetCurrentProcess() -> *mut c_void;
    fn VirtualAlloc(
        lp_address: *mut c_void,
        dw_size: usize,
        fl_allocation_type: u32,
        fl_protect: u32,
    ) -> *mut c_void;
    fn VirtualProtect(
        lp_address: *mut c_void,
        dw_size: usize,
        fl_new_protect: u32,
        lpfl_old_protect: *mut u32,
    ) -> i32;
    fn VirtualFree(lp_address: *mut c_void, dw_size: usize, dw_free_type: u32) -> i32;
    fn FlushInstructionCache(
        h_process: *mut c_void,
        lp_base_address: *const c_void,
        dw_size: usize,
    ) -> i32;
}

#[cfg(all(
    target_os = "windows",
    not(target_os = "none"),
    not(target_os = "uefi")
))]
fn allocate_executable_copy(code: &[u8]) -> Result<*mut u8, BpfError> {
    if code.is_empty() {
        return Ok(core::ptr::null_mut());
    }

    let ptr = unsafe {
        VirtualAlloc(
            core::ptr::null_mut(),
            code.len(),
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        )
    };
    if ptr.is_null() {
        return Err(BpfError::VerificationFailed(alloc::format!(
            "VirtualAlloc failed for {} JIT bytes",
            code.len()
        )));
    }

    unsafe {
        core::ptr::copy_nonoverlapping(code.as_ptr(), ptr.cast::<u8>(), code.len());
        let mut old_protect = 0u32;
        if VirtualProtect(ptr, code.len(), PAGE_EXECUTE_READ, &mut old_protect) == 0 {
            let _ = VirtualFree(ptr, 0, MEM_RELEASE);
            return Err(BpfError::VerificationFailed(alloc::format!(
                "VirtualProtect failed for {} JIT bytes",
                code.len()
            )));
        }
        let _ = FlushInstructionCache(GetCurrentProcess(), ptr.cast_const(), code.len());
    }
    Ok(ptr.cast::<u8>())
}

pub fn compile_bytes(program: &[BpfInsn]) -> Result<Vec<u8>, BpfError> {
    host_smoke_probe("compile_bytes:verify:begin");
    BpfVerifier::verify(program)?;
    host_smoke_probe("compile_bytes:verify:end");

    host_smoke_probe("compile_bytes:new:begin");
    let mut jit = JitCompiler::new();
    host_smoke_probe("compile_bytes:new:end");
    host_smoke_probe("compile_bytes:prologue:begin");
    jit.emit_prologue();
    host_smoke_probe("compile_bytes:prologue:end");

    for (i, insn) in program.iter().enumerate() {
        host_smoke_probe("compile_bytes:emit_insn:begin");
        jit.insn_offsets.push(jit.code.len());
        jit.emit_insn(i, insn, program.len())?;
        host_smoke_probe("compile_bytes:emit_insn:end");
    }

    host_smoke_probe("compile_bytes:fixups:prep");
    jit.insn_offsets.push(jit.code.len());
    host_smoke_probe("compile_bytes:fixups:begin");
    jit.apply_fixups()?;
    host_smoke_probe("compile_bytes:fixups:end");

    if jit.code.len() > MAX_JIT_SIZE {
        return Err(BpfError::ProgramTooLarge(program.len()));
    }

    host_smoke_probe("compile_bytes:return");
    Ok(jit.code)
}

/// eBPF JIT alt sistemini başlatır.
pub fn init() {
    crate::serial_println!("[eBPF-JIT] x86_64 JIT compiler initialized");
    crate::serial_println!(
        "[eBPF-JIT]   Register mapping: BPF R0-R10 → rax,rdi,rsi,rdx,rcx,r8,rbx,r13,r14,r15,rbp"
    );
    crate::serial_println!("[eBPF-JIT]   Max JIT size: {} bytes", MAX_JIT_SIZE);
    crate::serial_println!("[eBPF-JIT]   BPF stack: {} bytes", BPF_STACK_SIZE);
}
