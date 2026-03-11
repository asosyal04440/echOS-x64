//! # echOS Ön İşleme Engelleme (Preemption) ve Kesme Güvenliği Modülü
//!
//! Tier 1 OS seviyesinde `preempt_count` ve kesme güvenliği yönetimi.
//! Linux `preempt_count` ile aynı mantık, Rust optimizasyonları ile iyileştirilmiş.
//!
//! ## Önişleme (Preemption) Nedir?
//! Ön işleme, çalışan bir görevin (task) başka bir görev lehine yarıda kesilmesidir.
//! Bazı kritik bölümlerde (örn: kilit tutarken, kesme işlerken) bu engellenmeli,
//! aksi hâlde kilitlenme (deadlock) veya veri bozulması yaşanabilir.
//!
//! ## preempt_count Bit Düzeni
//! ```ascii
//! Bit 0: PREEMPT_DISABLE  - Ön işleme devre dışı
//! Bit 1: NEED_RESCHED     - Yeniden zamanlama gerekiyor
//! Bit 2: HARDIRQ          - Donanım kesmesi (IRQ) içinde
//! Bit 3: SOFTIRQ          - Yazılım kesmesi içinde
//! Bit 4: NMI              - Maskelenemeyen kesme içinde
//! Bit 5+: COUNT_OFFSET    - Sayaç ofseti
//! ```

use crate::memory_barriers::{smp_mb, smp_rmb, smp_wmb};
use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

/// Her CPU için ayrı ön işleme sayacı dizisi.
///
/// 8192 CPU'ya kadar destek verir. Her CPU kendi sayacına yazar,
/// bu sayede kilit gerektirmez (lock-free).
static mut PREEMPT_COUNT: [AtomicU32; 8192] = [const { AtomicU32::new(0) }; 8192];

/// Ön işleme devre dışı bırakma birimi — fetch_add(1) ile sayaç olarak kullanılır.
/// Bit 0-7: preempt disable iç içe sayacı (256 seviye).
pub const PREEMPT_DISABLE_BITS: u32 = 1; // bits 0-7
/// Preempt disable maskesi — iç içe sayacın 8 bit genişliği.
pub const PREEMPT_DISABLE_MASK: u32 = 0xFF;
/// Yeniden zamanlama gerektiğini belirten bit (fetch_or/fetch_and ile).
pub const PREEMPT_NEED_RESCHED: u32 = 1 << 8; // bit 8
/// Yazılım kesmesi (Soft IRQ) sayaç birimi — fetch_add(1 << 9) ile.
/// Bit 9-15: softirq iç içe sayacı (128 seviye).
pub const PREEMPT_SOFTIRQ: u32 = 1 << 9; // bits 9-15
/// Softirq maskesi.
pub const PREEMPT_SOFTIRQ_MASK: u32 = 0x7F << 9;
/// Donanım kesmesi (Hard IRQ) sayaç birimi — fetch_add(1 << 16) ile.
/// Bit 16-23: hardirq iç içe sayacı (256 seviye).
pub const PREEMPT_HARDIRQ: u32 = 1 << 16; // bits 16-23
/// Hardirq maskesi.
pub const PREEMPT_HARDIRQ_MASK: u32 = 0xFF << 16;
/// Maskelenemeyen kesme (NMI) bayrağı (fetch_or/fetch_and ile).
/// Bit 24: NMI aktif bayrağı.
pub const PREEMPT_NMI: u32 = 1 << 24; // bit 24
/// Sayaç ofset biti (eski uyumluluk — kullanılmaz).
pub const PREEMPT_COUNT_OFFSET: u32 = 1 << 25; // COUNT_OFFSET

/// Kesme bağlamı (interrupt context) seviyesi.
///
/// Mevcut kodun hangi kesme düzeyinde çalıştığını tanımlar.
/// None = normal görev bağlamı.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterruptContext {
    /// Normal görev bağlamı (kesme yok)
    None,
    /// Donanım kesmesi bağlamı
    HardIRQ,
    /// Yazılım kesmesi bağlamı
    SoftIRQ,
    /// Maskelenemeyen kesme bağlamı
    NMI,
}

/// Ön işleme devre dışı bırakma muhafızı (RAII guard).
///
/// Oluşturulduğunda ön işlemeyi devre dışı bırakır,
/// düşürüldüğünde (Drop) otomatik olarak yeniden etkinleştirir.
/// Bu pattern, erken dönüş veya panic durumlarında güvenlik sağlar.
pub struct PreemptDisableGuard {
    cpu_id: u32,
    old_count: u32,
}

impl PreemptDisableGuard {
    /// Ön işlemeyi devre dışı bırakıp yeni bir muhafız oluşturur.
    pub fn new() -> Self {
        let cpu_id = crate::cpu::smp::current_cpu_id();
        let old_count = preempt_count_inc(cpu_id, PREEMPT_DISABLE_BITS);

        Self { cpu_id, old_count }
    }

    /// Ön işlemenin şu an devre dışı olup olmadığını kontrol eder.
    pub fn is_disabled(&self) -> bool {
        let current_count = get_preempt_count(self.cpu_id);
        (current_count & PREEMPT_DISABLE_MASK) != 0
    }
}

impl Drop for PreemptDisableGuard {
    /// Muhafız düşürüldüğünde ön işlemeyi otomatik yeniden etkinleştirir.
    fn drop(&mut self) {
        preempt_count_dec(self.cpu_id, PREEMPT_DISABLE_BITS);
    }
}

/// Donanım kesmesi (Hard IRQ) bağlamı muhafızı.
///
/// Donanım kesmesi işleyicisi çalışırken bu muhafız aktif olur.
/// `PREEMPT_HARDIRQ` bitini set eder, düşürüldüğünde temizler.
pub struct HardIRQGuard {
    cpu_id: u32,
    old_count: u32,
}

impl HardIRQGuard {
    /// Donanım kesmesi bağlamına girer ve muhafız oluşturur.
    pub fn new() -> Self {
        let cpu_id = crate::cpu::smp::current_cpu_id();
        let old_count = preempt_count_inc(cpu_id, PREEMPT_HARDIRQ);

        // Bellek bariyeri: kesme işleyicisi içindeki işlemler dışarıdan önce görülmeli
        smp_mb();

        Self { cpu_id, old_count }
    }

    /// Mevcut kesme bağlamı seviyesini döner.
    pub fn context_level(&self) -> InterruptContext {
        InterruptContext::HardIRQ
    }
}

impl Drop for HardIRQGuard {
    /// Donanım kesmesi bağlamından çıkar ve bellek bariyeri uygular.
    fn drop(&mut self) {
        preempt_count_dec(self.cpu_id, PREEMPT_HARDIRQ);
        smp_mb();
    }
}

/// Yazılım kesmesi (Soft IRQ) bağlamı muhafızı.
///
/// Ağ paket işleme, zamanlayıcı geri çağrısı gibi ertelenmiş işler
/// için Soft IRQ bağlamı kullanılır.
pub struct SoftIRQGuard {
    cpu_id: u32,
    old_count: u32,
}

impl SoftIRQGuard {
    /// Yazılım kesmesi bağlamına girer ve muhafız oluşturur.
    pub fn new() -> Self {
        let cpu_id = crate::cpu::smp::current_cpu_id();
        let old_count = preempt_count_inc(cpu_id, PREEMPT_SOFTIRQ);

        smp_mb();

        Self { cpu_id, old_count }
    }

    /// Mevcut kesme bağlamı seviyesini döner.
    pub fn context_level(&self) -> InterruptContext {
        InterruptContext::SoftIRQ
    }
}

impl Drop for SoftIRQGuard {
    /// Yazılım kesmesi bağlamından çıkar ve bellek bariyeri uygular.
    fn drop(&mut self) {
        preempt_count_dec(self.cpu_id, PREEMPT_SOFTIRQ);
        smp_mb();
    }
}

/// Maskelenemeyen Kesme (NMI) bağlamı muhafızı.
///
/// NMI, donanım hataları veya watchdog gibi acil durumlarda tetiklenir.
/// Bu bağlamda uyku ya da kilit alma gibi işlemler yapılamaz.
pub struct NMIGuard {
    cpu_id: u32,
    old_count: u32,
}

impl NMIGuard {
    /// NMI bağlamına girer ve muhafız oluşturur.
    pub fn new() -> Self {
        let cpu_id = crate::cpu::smp::current_cpu_id();
        let old_count = preempt_count_inc(cpu_id, PREEMPT_NMI);

        smp_mb();

        Self { cpu_id, old_count }
    }

    /// Mevcut kesme bağlamı seviyesini döner.
    pub fn context_level(&self) -> InterruptContext {
        InterruptContext::NMI
    }
}

impl Drop for NMIGuard {
    /// NMI bağlamından çıkar ve bellek bariyeri uygular.
    fn drop(&mut self) {
        preempt_count_dec(self.cpu_id, PREEMPT_NMI);
        smp_mb();
    }
}

/// Belirtilen CPU için mevcut ön işleme sayacını döner.
///
/// Bu fonksiyon Relaxed sıralama kullanır; çağıran taraf
/// gerektiğinde bariyer uygulamalıdır.
pub fn get_preempt_count(cpu_id: u32) -> u32 {
    unsafe { PREEMPT_COUNT[cpu_id as usize].load(Ordering::Relaxed) }
}

/// Ön işleme sayacını belirtilen bitler kadar artırır.
///
/// Sayacı artırdıktan sonra yazma bariyeri uygulayarak
/// değişikliğin diğer CPU'lara görünür olmasını sağlar.
pub fn preempt_count_inc(cpu_id: u32, bits: u32) -> u32 {
    unsafe {
        let old_count = PREEMPT_COUNT[cpu_id as usize].fetch_add(bits, Ordering::Relaxed);
        smp_wmb();
        old_count
    }
}

/// Ön işleme sayacını belirtilen bitler kadar azaltır.
///
/// Sayacı azalttıktan sonra yazma bariyeri uygulayarak
/// değişikliğin diğer CPU'lara görünür olmasını sağlar.
pub fn preempt_count_dec(cpu_id: u32, bits: u32) -> u32 {
    unsafe {
        let old_count = PREEMPT_COUNT[cpu_id as usize].fetch_sub(bits, Ordering::Relaxed);
        smp_wmb();
        old_count
    }
}

/// Geçerli CPU'da ön işlemenin etkin olup olmadığını kontrol eder.
///
/// `PREEMPT_DISABLE_BITS` biti temizse ön işleme etkindir.
pub fn preempt_enabled() -> bool {
    let cpu_id = crate::cpu::smp::current_cpu_id();
    let count = get_preempt_count(cpu_id);
    (count & PREEMPT_DISABLE_MASK) == 0
}

/// Geçerli CPU'nun kesme bağlamında (IRQ, SoftIRQ veya NMI) olup olmadığını kontrol eder.
pub fn in_interrupt() -> bool {
    let cpu_id = crate::cpu::smp::current_cpu_id();
    let count = get_preempt_count(cpu_id);
    (count & (PREEMPT_HARDIRQ_MASK | PREEMPT_SOFTIRQ_MASK | PREEMPT_NMI)) != 0
}

/// Geçerli CPU'nun kesme bağlamı seviyesini döner.
///
/// NMI > HardIRQ > SoftIRQ > None öncelik sırasıyla kontrol edilir.
pub fn get_interrupt_context() -> InterruptContext {
    let cpu_id = crate::cpu::smp::current_cpu_id();
    let count = get_preempt_count(cpu_id);

    if (count & PREEMPT_NMI) != 0 {
        InterruptContext::NMI
    } else if (count & PREEMPT_HARDIRQ_MASK) != 0 {
        InterruptContext::HardIRQ
    } else if (count & PREEMPT_SOFTIRQ_MASK) != 0 {
        InterruptContext::SoftIRQ
    } else {
        InterruptContext::None
    }
}

/// Geçerli CPU'nun NMI bağlamında olup olmadığını kontrol eder.
pub fn in_nmi() -> bool {
    let cpu_id = crate::cpu::smp::current_cpu_id();
    let count = get_preempt_count(cpu_id);
    (count & PREEMPT_NMI) != 0
}

/// Geçerli CPU'nun donanım kesmesi (HardIRQ) bağlamında olup olmadığını kontrol eder.
pub fn in_hardirq() -> bool {
    let cpu_id = crate::cpu::smp::current_cpu_id();
    let count = get_preempt_count(cpu_id);
    (count & PREEMPT_HARDIRQ_MASK) != 0
}

/// Geçerli CPU'nun yazılım kesmesi (SoftIRQ) bağlamında olup olmadığını kontrol eder.
pub fn in_softirq() -> bool {
    let cpu_id = crate::cpu::smp::current_cpu_id();
    let count = get_preempt_count(cpu_id);
    (count & PREEMPT_SOFTIRQ_MASK) != 0
}

/// Geçerli CPU için yeniden zamanlama (reschedule) bayrağını ayarlar.
///
/// Bu bayrak set edildiğinde, ön işleme etkinleşince zamanlayıcı çalıştırılır.
pub fn set_need_resched() {
    let cpu_id = crate::cpu::smp::current_cpu_id();
    unsafe {
        PREEMPT_COUNT[cpu_id as usize].fetch_or(PREEMPT_NEED_RESCHED, Ordering::Relaxed);
    }
    smp_mb();
}

/// Geçerli CPU'da yeniden zamanlama bayrağının set olup olmadığını kontrol eder.
pub fn need_resched() -> bool {
    let cpu_id = crate::cpu::smp::current_cpu_id();
    let count = get_preempt_count(cpu_id);
    (count & PREEMPT_NEED_RESCHED) != 0
}

/// Geçerli CPU için yeniden zamanlama bayrağını temizler.
pub fn clear_need_resched() {
    let cpu_id = crate::cpu::smp::current_cpu_id();
    unsafe {
        PREEMPT_COUNT[cpu_id as usize].fetch_and(!PREEMPT_NEED_RESCHED, Ordering::Relaxed);
    }
    smp_mb();
}

/// Geçerli bağlamda ön işlemenin güvenli olup olmadığını kontrol eder.
///
/// Kesme bağlamında değilse VE ön işleme etkinse güvenlidir.
pub fn preemptible() -> bool {
    !in_interrupt() && preempt_enabled()
}

/// Geçerli bağlamda zamanlamanın güvenli olup olmadığını kontrol eder.
///
/// NMI veya HardIRQ bağlamında zamanlama yapılamaz.
pub fn schedulable() -> bool {
    !in_nmi() && !in_hardirq()
}

/// Bir CPU'nun ön işleme durumunu özetleyen yapı.
///
/// Hata ayıklama ve izleme amaçlı kullanılır.
#[derive(Debug, Clone, Copy)]
pub struct PreemptStats {
    pub cpu_id: u32,
    pub preempt_count: u32,
    pub preempt_disabled: bool,
    pub in_interrupt: bool,
    pub interrupt_context: InterruptContext,
    pub need_resched: bool,
}

impl PreemptStats {
    /// Geçerli CPU'nun ön işleme durumunu anlık görüntü olarak alır.
    pub fn current() -> Self {
        let cpu_id = crate::cpu::smp::current_cpu_id();
        let count = get_preempt_count(cpu_id);

        Self {
            cpu_id,
            preempt_count: count,
            preempt_disabled: (count & PREEMPT_DISABLE_MASK) != 0,
            in_interrupt: (count & (PREEMPT_HARDIRQ_MASK | PREEMPT_SOFTIRQ_MASK | PREEMPT_NMI))
                != 0,
            interrupt_context: get_interrupt_context(),
            need_resched: (count & PREEMPT_NEED_RESCHED) != 0,
        }
    }

    /// Belirtilen CPU'nun ön işleme durumunu anlık görüntü olarak alır.
    pub fn for_cpu(cpu_id: u32) -> Self {
        let count = get_preempt_count(cpu_id);

        Self {
            cpu_id,
            preempt_count: count,
            preempt_disabled: (count & PREEMPT_DISABLE_MASK) != 0,
            in_interrupt: (count & (PREEMPT_HARDIRQ_MASK | PREEMPT_SOFTIRQ_MASK | PREEMPT_NMI))
                != 0,
            interrupt_context: if (count & PREEMPT_NMI) != 0 {
                InterruptContext::NMI
            } else if (count & PREEMPT_HARDIRQ_MASK) != 0 {
                InterruptContext::HardIRQ
            } else if (count & PREEMPT_SOFTIRQ_MASK) != 0 {
                InterruptContext::SoftIRQ
            } else {
                InterruptContext::None
            },
            need_resched: (count & PREEMPT_NEED_RESCHED) != 0,
        }
    }
}

/// Ön işleme alt sistemini başlatır.
///
/// Tüm CPU'ların sayaçlarını sıfırlar. Sistem açılışında çağrılmalıdır.
pub fn init() {
    crate::serial_println!("Preempt: Initializing preemption subsystem");

    let cpu_count = crate::cpu::smp::get_cpu_count();
    for cpu_id in 0..cpu_count {
        unsafe {
            PREEMPT_COUNT[cpu_id as usize].store(0, Ordering::Relaxed);
        }
    }

    crate::serial_println!("Preempt: Initialized for {} CPUs", cpu_count);
}

/// Ön işleme hata ayıklama yardımcıları.
pub mod debug {
    use super::*;

    /// Geçerli CPU'nun ön işleme durumunu güzel biçimde seri porta yazdırır.
    pub fn print_preempt_state() {
        let stats = PreemptStats::current();
        crate::serial_println!("Preempt State:");
        crate::serial_println!("  CPU: {}", stats.cpu_id);
        crate::serial_println!("  Count: 0x{:x}", stats.preempt_count);
        crate::serial_println!("  Disabled: {}", stats.preempt_disabled);
        crate::serial_println!("  In Interrupt: {}", stats.in_interrupt);
        crate::serial_println!("  Context: {:?}", stats.interrupt_context);
        crate::serial_println!("  Need Resched: {}", stats.need_resched);
    }

    /// Tüm CPU'ların ön işleme durumunu seri porta yazdırır.
    pub fn print_all_cpu_states() {
        let cpu_count = crate::cpu::smp::get_cpu_count();

        crate::serial_println!("=== All CPU Preempt States ===");
        for cpu_id in 0..cpu_count {
            let stats = PreemptStats::for_cpu(cpu_id);
            crate::serial_println!(
                "CPU {}: count=0x{:x}, disabled={}, interrupt={:?}, need_resched={}",
                cpu_id,
                stats.preempt_count,
                stats.preempt_disabled,
                stats.interrupt_context,
                stats.need_resched
            );
        }
        crate::serial_println!("=== End CPU States ===");
    }

    /// Tüm CPU'lardaki ön işleme durumlarının tutarlılığını doğrular.
    ///
    /// Çelişkili durum kombinasyonları (örn: hem kesme bağlamı hem devre dışı)
    /// bulunursa uyarı yazdırır ve `false` döner.
    pub fn validate_preempt_state() -> bool {
        let cpu_count = crate::cpu::smp::get_cpu_count();
        let mut valid = true;

        for cpu_id in 0..cpu_count {
            let stats = PreemptStats::for_cpu(cpu_id);

            // Geçersiz durum kombinasyonlarını kontrol et
            if stats.in_interrupt && stats.preempt_disabled {
                crate::serial_println!(
                    "Preempt Warning: CPU {} has both interrupt and disabled",
                    cpu_id
                );
                valid = false;
            }

            if stats.interrupt_context == InterruptContext::None && stats.in_interrupt {
                crate::serial_println!(
                    "Preempt Error: CPU {} inconsistent interrupt state",
                    cpu_id
                );
                valid = false;
            }
        }

        valid
    }
}

/// Ön işleme-güvenli uyku fonksiyonu.
///
/// Ön işleme etkinse zamanlayıcıyı çağırır; devre dışıysa
/// döngü ile meşgul bekler (spin wait). Kesme bağlamında çağrılmamalıdır.
pub fn preemptible_sleep(ticks: usize) {
    if preemptible() {
        crate::task::scheduler::sleep(ticks);
    } else {
        // Uyku yapılamaz, döngü ile meşgul bekle
        for _ in 0..ticks {
            core::hint::spin_loop();
        }
    }
}

/// Ön işleme-güvenli zamanlama fonksiyonu.
///
/// Yalnızca zamanlama güvenli bağlamda (NMI veya HardIRQ değilse) çalışır.
pub fn preemptible_schedule() {
    if schedulable() {
        crate::task::scheduler::schedule();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preempt_disable() {
        let _guard = PreemptDisableGuard::new();
        assert!(!preempt_enabled());
    }

    #[test]
    fn test_interrupt_context() {
        let _guard = HardIRQGuard::new();
        assert_eq!(get_interrupt_context(), InterruptContext::HardIRQ);
        assert!(in_interrupt());
        assert!(in_hardirq());
    }

    #[test]
    fn test_need_resched() {
        set_need_resched();
        assert!(need_resched());
        clear_need_resched();
        assert!(!need_resched());
    }
}
