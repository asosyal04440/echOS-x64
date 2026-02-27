//! # APIC (Gelişmiş Programlanabilir Kesme Denetleyicisi) Modülü
//!
//! Bu modül, çok işlemcili x86-64 sistemlerde kesme yönetimini sağlar.
//! İki alt modül içerir:
//! - `lapic`: Her CPU çekirdeğine özel Yerel APIC (Local APIC) — zamanlama,
//!   IPI (Inter-Processor Interrupt) gönderme ve EOI işlemleri için kullanılır.
//! - `ioapic`: Sistem genelinde G/Ç APIC (I/O APIC) — donanım kesmelerini
//!   (klavye, saat vb.) doğru CPU'ya yönlendiren redirection table'u yönetir.

pub mod ioapic;
pub mod lapic;
