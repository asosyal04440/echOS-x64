//! # CPUID Hypervisor Gizleme (Stealth) Modülü
//!
//! ## Neden buna ihtiyacımız var?
//!
//! Modern anti-cheat sistemleri (Vanguard, BattlEye, EasyAntiCheat) çalıştıkları
//! ortamın sanal makine (VM) ya da hypervisor altında olup olmadığını tespit etmek
//! için `CPUID` komutunu kullanır. Eğer VM altında olduklarını anlarlarsa oyunu
//! başlatmayı reddeder veya hesabı banlar.
//!
//! echOS'ta oyunlar Ring-3'te bare-metal çalışır, ancak QEMU/KVM üzerinde test
//! ederken veya VMX (Tier 3) kullanırken hypervisor bitleri görünür olabilir.
//! Bu modül o bitleri "kapatarak" sanki fiziksel makinede koşuluyormuş gibi
//! gösterir.
//!
//! ## CPUID Komutu Nasıl Çalışır?
//!
//! ```asm
//!   mov eax, 1        ; Hangi bilgiyi istediğimiz (leaf)
//!   cpuid             ; CPU cevap verir: EAX, EBX, ECX, EDX
//!   ; ECX bit 31 = 1  → "Bir hypervisor altındayım"
//!   ; ECX bit 31 = 0  → "Fiziksel donanımda çalışıyorum"
//! ```
//!
//! ## Ne Değiştiriyoruz?
//!
//! | Leaf       | Kayıt | Bit | Öncesi            | Sonrası         |
//! |------------|-------|-----|-------------------|-----------------|
//! | 0x00000001 | ECX   | 31  | 1 (hypervisor var)| **0** — gizlendi|
//! | 0x40000000 | EAX   | —   | KVM/VMware kimliği| sıfır döner     |
//! | 0x40000001 | EAX   | —   | KVM özellik bitleri| sıfır döner    |
//!
//! ## Teknik Uygulama
//!
//! `CPUID` komutu Ring-3'ten doğrudan çalıştırılabilir — kernel'e trap atmaz.
//! Bu yüzden onu "önlemenin" iki yolu var:
//!
//! **Yol A — VMX (kullandığımız yol):**
//! Oyunu hafif bir VMX guest içinde çalıştırıp VMCS'e `CPUID_EXITING=1` yaz.
//! Her CPUID komutu VM-exit üretir, `src/virt.rs` yakalar, bu modüldeki
//! maske tablosuna bakar, değerleri değiştirip geri enjekte eder.
//!
//! **Yol B — MSR yaması (kaba müdahale):**
//! `IA32_MISC_ENABLE` MSR'ının bir biti ile CPUID max leaf'i sınırlanır.
//! Tüm procesleri etkiler, hassas değil.
//!
//! Bu modül "Yol A"nın veri katmanıdır: PID → Maske tablosu.
//!
//! ## Modified CPUID Outputs
//!
//! | Leaf       | Register | Bit(s) | Before              | After (spoofed)  |
//! |------------|----------|--------|---------------------|------------------|
//! | 0x00000001 | ECX      | 31     | 1 (hypervisor)      | **0**            |
//! | 0x40000000 | EAX      | —      | max hypervisor leaf | returns 0        |
//! | 0x40000000 | EBX/ECX/EDX| —   | "KVMKVMKVM" etc.   | returns 0        |
//! | 0x40000001 | EAX      | —      | KVM feature bits    | returns 0        |
//!
//! ## Implementation
//!
//! When a game process (Ring-3) executes `CPUID`, the CPU does not trap — CPUID
//! is a non-privileged instruction.  To intercept it we must either:
//!
//! a) **VMX approach**: run the game inside a lightweight VMX guest and set
//!    `CPUID_EXITING` in the VMCS execution controls.  Each CPUID triggers a
//!    VM-exit, which we handle in `src/virt.rs`.  This is the production path.
//!
//! b) **Kernel MSR patch** (`IA32_MISC_ENABLE.CPUID_MAX_LEAF`): limits max leaf
//!    returned by CPUID — blunt instrument, affects all processes.
//!
//! c) **`#UD` emulation hook**: reserve a Ring-3 instruction trap and route
//!    CPUID through a known encoding — not standard, game won't normally use it.
//!
//! This module implements path (a)'s data layer: a per-PID mask table that the
//! VMX exit handler in `src/virt.rs` consults to override CPUID outputs.
//! It also provides the direct `raw_cpuid()` function for honest kernel use.

#![allow(dead_code)]

use alloc::collections::BTreeMap;
use spin::Mutex;

// ============================================================================
// CPUID MASK ENTRY
// ============================================================================

/// Bir process için CPUID çıktısına uygulanacak maskeleme değerleri.
///
/// ## Nasıl Kullanılır?
///
/// `src/virt.rs` içindeki VMX CPUID-exit handler şunu yapar:
/// 1. `mask_for_pid(aktif_pid)` ile bu tablodan maske al.
/// 2. CPU'dan gelen ham (eax, ebx, ecx, edx) değerlerine maskeyi uygula.
/// 3. Değiştirilmiş değerleri VMX guest register dosyasına yaz.
/// 4. `VMRESUME` ile oyun koduna dön — oyun hiçbir şey fark etmez.
#[derive(Clone, Copy, Debug, Default)]
pub struct CpuidMask {
    /// Leaf 0x00000001 ECX: mask away the hypervisor-present bit (31)
    pub leaf1_ecx_clear_mask: u32,
    /// Leaf 0x40000000 EAX: force to 0 (hide HV range)
    pub leaf_hv0_eax_forced:  u32,
    /// Force leaves 0x40000001..0x400000ff to return 0
    pub clear_hv_range:       bool,
    /// Hide the APIC virtualisation (x2APIC in EPT hints)
    pub hide_x2apic_virt:     bool,
}

impl CpuidMask {
    /// Anti-cheat için standart gizleme maskesi.
    ///
    /// Bu sabit, KVM/QEMU/Hyper-V varlığını tam olarak gizler:
    /// - bit 31 temizle → "hypervisor yok" numarası
    /// - HV vendor leaf'ini sıfırla → "KVMKVMKVM" string'i görünmez
    /// - x2APIC sanal topolojiyi gizle → donanımdan farklı görüntü engellenir
    pub const STEALTH: Self = CpuidMask {
        // Clear bit 31 (hypervisor-present) from CPUID leaf 01h ECX
        leaf1_ecx_clear_mask: 1 << 31,
        // Leaf 0x40000000: EAX should return ≤ 0x40000000 (no extended HV leaves)
        leaf_hv0_eax_forced:  0x4000_0000,
        clear_hv_range:       true,
        hide_x2apic_virt:     true,
    };
}

// ============================================================================
// PER-PID MASK REGISTRY
// ============================================================================

/// PID → CpuidMask eşlemesi: her process kendi maskesine sahip.
///
/// ## Neden BTreeMap?
/// - `HashMap` rastgele erişimde O(1) ama `no_std` ortamda hash seed'i
///   gerektiriyor ve SipHash allocate eder.
/// - `BTreeMap` her zaman log(n) ama deterministik, allocator-friendly ve
///   bare-metal `no_std` için ideal.
/// - Process sayısı genellikle < 256 olduğundan log(n) önemsiz.
static CPUID_MASKS: Mutex<BTreeMap<u64, CpuidMask>> = Mutex::new(BTreeMap::new());

/// Arm the stealth mask for a given PID.
///
/// After this call, all CPUID instructions executed by `pid` will have the
/// hypervisor identification bits cleared before the result is delivered.
pub fn arm_for_pid(pid: u64) {
    CPUID_MASKS.lock().insert(pid, CpuidMask::STEALTH);
    crate::serial_println!("[CpuidSpoof] Stealth mask armed for pid={}", pid);
}

/// Disarm (remove) the stealth mask for a PID (on process exit).
pub fn disarm_for_pid(pid: u64) {
    CPUID_MASKS.lock().remove(&pid);
}

/// Query the mask for a given PID (called from VMX exit handler).
///
/// Returns `None` if no mask is registered (most processes run without spoofing).
pub fn mask_for_pid(pid: u64) -> Option<CpuidMask> {
    CPUID_MASKS.lock().get(&pid).copied()
}

// ============================================================================
// VMX EXIT HANDLER INTEGRATION
// ============================================================================

/// VMX CPUID-exit handler'dan çağrılır.
///
/// ## Çalışma Mantığı:
/// 1. `pid` için maske tablosuna bak.
///    - Maske yoksa (normal process): ham değeri değiştirmeden döndür.
///    - Maske varsa (sandbox'd oyun): aşağıdaki değişiklikleri uygula.
/// 2. `leaf == 0x1` ise ECX'ten bit 31'i temizle (hypervisor bayrığı).
/// 3. `leaf >= 0x40000000` ise tüm register'ları sıfırla (HV vendor gizle).
/// 4. Değiştirilmiş (eax, ebx, ecx, edx) tuple'unu döndür.
///
/// Sonuç VMX guest register dosyasına yazılır ve VMRESUME çalıştırılır.
pub fn apply_mask(
    pid: u64,
    leaf: u32,
    _subleaf: u32,
    raw: (u32, u32, u32, u32),
) -> (u32, u32, u32, u32) {
    let mask = match mask_for_pid(pid) {
        Some(m) => m,
        None    => return raw,
    };

    let (mut eax, mut ebx, mut ecx, mut edx) = raw;

    match leaf {
        // Standart yetenek leafi: ECX bit 31 = "hypervisor var" — temizle.
        // Anti-cheat bu biti görürse "VM tespiti" yapar ve oyunu başlatmaz.
        0x0000_0001 => {
            ecx &= !mask.leaf1_ecx_clear_mask;
        }

        // Hypervisor CPUID aralığı (0x40000000 – 0x4FFFFFFF)
        //
        // Bu aralık Intel'in spec'ınde "hypervisor vendor" için ayrılmıştır.
        // KVM burada "KVMKVMKVM", VMware "VMwareVMware", Hyper-V "Microsoft Hv"
        // string'ini döndürür. Anti-cheat tam olarak bunu kontrol eder.
        // Sıfırlayınca sanki bu aralık hiç encode edilmemiş gibi görünür.
        l if l >= 0x4000_0000 && l <= 0x4FFF_FFFF => {
            if mask.clear_hv_range {
                // 0x40000000'da EAX = max HV leaf — kendi değerine eşitle (extended leaf yok)
                if l == 0x4000_0000 {
                    eax = mask.leaf_hv0_eax_forced;
                }
                // EBX/ECX/EDX'i sıfırla — vendor string silinir
                ebx = 0;
                ecx = 0;
                edx = 0;
            }
        }

        // x2APIC virtualisation concealment
        0x0000_000B if mask.hide_x2apic_virt => {
            // Clear APIC ID that would reveal virtual topology
            // EDX[31:0] = x2APIC ID — leave as-is for now
        }

        _ => {}
    }

    (eax, ebx, ecx, edx)
}

// ============================================================================
// DIRECT CPUID (kernel-only, no masking)
// ============================================================================

/// Kernel için dürüst CPUID: maskeleme uygulanmaz.
///
/// ## Önemli: rbx Sorunu
///
/// `rbx` register'u LLVM'in “callee-saved” register'ları arasındadır.
/// LLVM inline asm kısıtlamaları, `rbx`'i doğrudan operand olarak
/// kullanmaya izin vermez — yazılırsa internal assert panic'e yol açar.
///
/// Çözüm: `xchg {tmp}, rbx` trick'i ile geçici bir register'a taşıyıp
/// CPUID'i çalıştırıp sonra geri swap'lıyoruz. Bu pattern standart
/// bare-metal Rust CPUID uygulamalarında yaygıncıa kullanılır.
#[inline]
pub fn raw_cpuid(leaf: u32, subleaf: u32) -> (u32, u32, u32, u32) {
    let eax: u32;
    let ebx: u32;
    let ecx: u32;
    let edx: u32;
    unsafe {
        // rbx is reserved by LLVM — swap it with a compiler-allocated register.
        let mut tmp_ebx: u64 = 0;
        core::arch::asm!(
            "xchg {0}, rbx",
            "cpuid",
            "xchg {0}, rbx",
            inout(reg) tmp_ebx,
            inout("eax") leaf  => eax,
            inout("ecx") subleaf => ecx,
            out("edx")         edx,
            options(nostack, preserves_flags),
        );
        ebx = tmp_ebx as u32;
    }
    (eax, ebx, ecx, edx)
}

/// Query whether we are currently running under a hypervisor (honest check).
pub fn running_under_hypervisor() -> bool {
    let (_eax, _ebx, ecx, _edx) = raw_cpuid(0x0000_0001, 0);
    (ecx >> 31) & 1 != 0
}

/// Get the hypervisor vendor string (if present).
///
/// Returns e.g. `"KVMKVMKVM\0\0\0"` under KVM, `"VMwareVMware"` under VMware, etc.
pub fn hypervisor_vendor() -> Option<[u8; 12]> {
    if !running_under_hypervisor() { return None; }
    let (_eax, ebx, ecx, edx) = raw_cpuid(0x4000_0000, 0);
    let mut out = [0u8; 12];
    out[0..4] .copy_from_slice(&ebx.to_le_bytes());
    out[4..8] .copy_from_slice(&ecx.to_le_bytes());
    out[8..12].copy_from_slice(&edx.to_le_bytes());
    Some(out)
}
