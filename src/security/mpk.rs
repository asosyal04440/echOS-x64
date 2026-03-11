//! # Memory Protection Keys (MPK/PKEYS)
//!
//! x86_64 PKU desteği varsa CR4.PKE açılır ve PKRU üzerinden alan izinleri yönetilir.

use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

const MAX_PKEYS: usize = 16;
const CR4_PKE_BIT: u64 = 1 << 22;

#[derive(Clone, Copy, Debug)]
pub struct PkeyPerm {
    pub access_disable: bool,
    pub write_disable: bool,
}

impl PkeyPerm {
    pub const fn rw() -> Self {
        Self {
            access_disable: false,
            write_disable: false,
        }
    }

    pub const fn ro() -> Self {
        Self {
            access_disable: false,
            write_disable: true,
        }
    }

    pub const fn no_access() -> Self {
        Self {
            access_disable: true,
            write_disable: true,
        }
    }
}

static MPK_ENABLED: AtomicBool = AtomicBool::new(false);
static PKEY_PERMS: Mutex<[PkeyPerm; MAX_PKEYS]> = Mutex::new([PkeyPerm::rw(); MAX_PKEYS]);

#[cfg(target_arch = "x86_64")]
#[inline(always)]
fn cpu_supports_pku() -> bool {
    let leaf = unsafe { core::arch::x86_64::__cpuid_count(7, 0) };
    (leaf.ecx & (1 << 3)) != 0
}

#[cfg(not(target_arch = "x86_64"))]
#[inline(always)]
fn cpu_supports_pku() -> bool {
    false
}

#[cfg(target_arch = "x86_64")]
#[inline(always)]
unsafe fn read_cr4() -> u64 {
    let value: u64;
    core::arch::asm!(
        "mov {}, cr4",
        out(reg) value,
        options(nomem, nostack, preserves_flags)
    );
    value
}

#[cfg(target_arch = "x86_64")]
#[inline(always)]
unsafe fn write_cr4(value: u64) {
    core::arch::asm!(
        "mov cr4, {}",
        in(reg) value,
        options(nomem, nostack, preserves_flags)
    );
}

#[cfg(target_arch = "x86_64")]
#[inline(always)]
unsafe fn rdpkru() -> u32 {
    let eax: u32;
    let edx: u32;
    core::arch::asm!(
        "rdpkru",
        in("ecx") 0u32,
        out("eax") eax,
        out("edx") edx,
        options(nomem, nostack, preserves_flags)
    );
    let _ = edx;
    eax
}

#[cfg(target_arch = "x86_64")]
#[inline(always)]
unsafe fn wrpkru(value: u32) {
    core::arch::asm!(
        "wrpkru",
        in("ecx") 0u32,
        in("edx") 0u32,
        in("eax") value,
        options(nomem, nostack, preserves_flags)
    );
}

pub fn init() {
    if !cpu_supports_pku() {
        crate::serial_println!("[MPK] PKU unsupported on this CPU");
        return;
    }

    #[cfg(target_arch = "x86_64")]
    unsafe {
        let cr4 = read_cr4();
        if cr4 & CR4_PKE_BIT == 0 {
            write_cr4(cr4 | CR4_PKE_BIT);
        }
        wrpkru(0);
    }

    MPK_ENABLED.store(true, Ordering::SeqCst);
    crate::serial_println!("[MPK] PKRU active (CR4.PKE=1)");
}

pub fn is_enabled() -> bool {
    MPK_ENABLED.load(Ordering::SeqCst)
}

pub fn set_pkey_perm(pkey: u8, perm: PkeyPerm) -> Result<(), &'static str> {
    if pkey as usize >= MAX_PKEYS {
        return Err("invalid pkey");
    }
    if !is_enabled() {
        return Err("mpk disabled");
    }

    #[cfg(target_arch = "x86_64")]
    unsafe {
        let shift = (pkey as u32) * 2;
        let mut pkru = rdpkru();
        pkru &= !(0b11 << shift);
        let encoded = (perm.access_disable as u32) | ((perm.write_disable as u32) << 1);
        pkru |= encoded << shift;
        wrpkru(pkru);
    }

    PKEY_PERMS.lock()[pkey as usize] = perm;
    Ok(())
}

pub fn get_pkey_perm(pkey: u8) -> Option<PkeyPerm> {
    PKEY_PERMS.lock().get(pkey as usize).copied()
}
