//! # echOS IDT Modülü (Interrupt Descriptor Table)
//!
//! Bu dosya, IDT kavramına giriş için bir soyut tanımlayıcıdır.
//! Gerçek IDT oluşturma ve yükleme işlemleri `interrupts/mod.rs`
//! içindeki `build_idt()` ve `init_idt_for_cpu()` fonksiyonlarında yapılır.
//!
//! ## IDT Nedir?
//!
//! IDT (Interrupt Descriptor Table), x86_64'te CPU'nun her bir interrupt
//! ve exception için hangi handler fonksiyonunu çağıracağını tarif eden
//! 256 girişli bir tablodur. `LIDT` komutuyla CPU'ya yüklenir.
//!
//! ```text
//!  Bellek Yerleşimi:
//!  ┌─────────────────────────────────────────┐
//!  │  IDTR Register  →  [Base Adres | Limit] │
//!  └────────────────────┬────────────────────┘
//!                       │
//!                       ▼
//!  ┌──────┬────────────────────────────────────────────────┐
//!  │  0   │  Gate Descriptor (16 byte)  ← Divide Error     │
//!  │  1   │  Gate Descriptor (16 byte)  ← Debug            │
//!  │  2   │  Gate Descriptor (16 byte)  ← NMI              │
//!  │  3   │  Gate Descriptor (16 byte)  ← Breakpoint       │
//!  │ ...  │  ...                                            │
//!  │  32  │  Gate Descriptor (16 byte)  ← IRQ0 (Timer)     │
//!  │  33  │  Gate Descriptor (16 byte)  ← IRQ1 (Keyboard)  │
//!  │ ...  │  ...                                            │
//!  │ 255  │  Gate Descriptor (16 byte)  ← Spurious (APIC)  │
//!  └──────┴────────────────────────────────────────────────┘
//! ```
//!
//! ## Gate Descriptor Yapısı (64-bit Interrupt Gate)
//!
//! ```text
//!  127      96 95  80 79 72 71   64 63       32 31      16 15   0
//!  ┌──────────┬──────┬─────┬───────┬───────────┬──────────┬──────┐
//!  │ Reserved │ Offs │ Att │  IST  │  Offs[63: │ Selector │ Offs │
//!  │          │[31:16│ ribu│       │  32]      │ (CS)     │[15:0]│
//!  └──────────┴──────┴─────┴───────┴───────────┴──────────┴──────┘
//!    Offset   : Handler fonksiyonunun tam 64-bit adresi
//!    Selector : Hangi kod segmenti (GDT'de kernel CS = 0x08)
//!    IST      : Interrupt Stack Table indeksi (0=mevcut stack, 1-7=ayrı)
//!    Attr     : P(resent)=1, DPL(ring), Type=0xE(interrupt)/0xF(trap)
//! ```
//!
//! ## IST (Interrupt Stack Table) Kullanımı
//!
//! Double Fault ve Page Fault için IST kullanılır. Bu, stack bozulması
//! durumunda bile handler'ın güvenli bir stack'e sahip olmasını sağlar.
//! TSS (Task State Segment) içindeki 7 ayrı stack pointer'dan birini kullanır.
//!
//! echOS'ta tanımlanan IST slotları:
//!   IST[DOUBLE_FAULT_IST_INDEX] → Double Fault güvenli stack
//!   IST[PAGE_FAULT_IST_INDEX]   → Page Fault güvenli stack

// Kullanılmıyor olabilir, ancak yapısal tutarlılık için tutuluyor.
