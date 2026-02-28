//! # APIC (Gelişmiş Programlanabilir Kesme Denetleyicisi) Modülü
//!
//! Bu modül, çok işlemcili x86-64 sistemlerde kesme yönetimini sağlar.
//! İki alt modül içerir:
//! - `lapic`: Her CPU çekirdeğine özel Yerel APIC (Local APIC) — zamanlama,
//!   IPI (Inter-Processor Interrupt) gönderme ve EOI işlemleri için kullanılır.
//! - `ioapic`: Sistem genelinde G/Ç APIC (I/O APIC) — donanım kesmelerini
//!   (klavye, saat vb.) doğru CPU'ya yönlendiren redirection table'u yönetir.
//!
//! ## APIC Mimarisine Genel Bakış
//!
//! ```text
//!  ┌─────────────────────────────────────────────────────────────────┐
//!  │                       Sistem Veriyolu                           │
//!  │                                                                  │
//!  │   ┌───────────────────────────┐     ┌──────────────────────┐   │
//!  │   │        I/O APIC           │     │    CPU0              │   │
//!  │   │  (Donanım → CPU yönlendirme)│◄──►│   ┌──────────────┐  │   │
//!  │   │                           │     │   │   LAPIC0     │  │   │
//!  │   │  Klavye IRQ1 → CPU0 v=33 │     │   │  (Timer/EOI) │  │   │
//!  │   │  Mouse  IRQ4 → CPU1 v=36 │     │   └──────────────┘  │   │
//!  │   │  PCIe   IRQ9 → CPU0 v=41 │     └──────────────────────┘   │
//!  │   │         ...              │                                  │
//!  │   └───────────────────────────┘     ┌──────────────────────┐   │
//!  │                                     │    CPU1              │   │
//!  │                                     │   ┌──────────────┐  │   │
//!  │                                     │   │   LAPIC1     │  │   │
//!  │                                     │   │  (Timer/EOI) │  │   │
//!  │                                     │   └──────────────┘  │   │
//!  │                                     └──────────────────────┘   │
//!  └─────────────────────────────────────────────────────────────────┘
//!
//!  Kesme akışı:
//!  Donanım sinyali → I/O APIC Redirection Table → LAPIC → IDT → İşleyici → EOI
//! ```

pub mod ioapic;
pub mod lapic;
