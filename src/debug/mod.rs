//! # echOS Debug Modülü
//!
//! Hata ayıklama araçları: seri port çıkışı ve sistem durum analizörü.
//!
//! ## Modül Ağaç Yapısı
//!
//! ```text
//! debug/
//! ├── mod.rs        ← Bu dosya: önyükleme testleri ve genel hata ayıklama
//! ├── serial.rs     ← Acil durum seri port sürücüsü (COM1, 0x3F8)
//! └── analyzer.rs   ← Sistem durumu analizörü
//! ```
//!
//! ## Önyükleme Hata Ayıklama Akışı
//!
//! ```text
//!  [UEFI/BIOS Önyükleyici]
//!         │
//!         ▼
//!  boot_self_check()           ← Temel sistem bütünlüğünü doğrular
//!         │
//!         ▼
//!  run_ring3_smoketest()       ← Kullanıcı alanı (Ring 3) temel işlevsellik testi
//!         │
//!         ├──► run_vm_security_tests()   ← Sanal bellek güvenlik denetimleri
//!         ├──► run_vm_stress_tests()     ← Sanal bellek yük/stres testleri
//!         └──► run_irq_stress_tests()    ← Kesme denetleyicisi (IRQ) stres testleri
//! ```
//!
//! ## Tasarım Notları
//!
//! - Tüm fonksiyonlar şu an **stub** (iskelet) aşamasındadır; `serial_println!` ile
//!   yalnızca seri porta ilerleme mesajı yazarlar.
//! - Gerçek test mantığı ilerleyen sürümlerde bu fonksiyonların içine eklenecektir.
//! - `serial_println!` makrosu `debug::serial` modülüne bağlıdır; o yüzden bu
//!   modül çağrılmadan önce seri portun başlatılmış olması gerekir.

/// Sistem durumu analizörü
pub mod analyzer;

/// Acil durum seri port hata ayıklama çıkışı (COM1).
/// Interrupt gerektirmeyen, doğrudan I/O portuna erişen basit sürücü.
pub mod serial;

/// Rate-limited debugcon (port 0xE9) writer.
/// Tamponlar ve en fazla her 100ms'de bir veya tampon %80 dolunca flush eder.
pub mod debugcon;

/// KGDB — Kernel GDB Remote Serial Protocol stub.
/// Seri port üzerinden GDB RSP ile çekirdek seviyesi hata ayıklama.
pub mod kgdb;

/// Ftrace — fonksiyon izleme altyapısı (function tracer, function_graph, irqsoff).
pub mod ftrace;

/// Kdump — çekirdek çöküş dökümü (register capture, stack trace, ELF64 vmcore).
pub mod kdump;

/// Strace — per-process sistem çağrısı izleme.
pub mod strace;

/// Perf — donanım performans sayaçları (PMU profiling).
pub mod perf;

/// Performance Audit — NVMe IOPS, NIC throughput, jail latency benchmark.
pub mod perf_audit;

/// Önyükleme öz-denetimi — temel sistem bütünlüğünü doğrular.
///
/// Çekirdek tamamen başlamadan önce çağrılır. Başarılı olursa `true` döner.
/// İleride bellek haritası, IDT ve GDT doğrulamaları bu fonksiyona eklenecektir.
pub fn boot_self_check() -> bool {
    crate::serial_println!("[DEBUG] Boot self-check passed");
    true
}

/// Ring 3 duman testi — temel kullanıcı alanı işlevselliğini doğrular.
///
/// x86-64 mimarisinde Ring 3, en düşük ayrıcalık seviyesidir (CPL=3).
/// Bu test ilerleyen sürümlerde gerçek kullanıcı süreçleri başlatarak
/// sistem çağrısı geçişlerini (syscall/sysret) denetleyecektir.
pub fn run_ring3_smoketest() {
    crate::serial_println!("[DEBUG] Ring3 smoketest (stub)");
}

/// VM security tests
pub fn run_vm_security_tests() {
    crate::serial_println!("[DEBUG] VM security tests (stub)");
}

/// VM stress tests
pub fn run_vm_stress_tests() {
    crate::serial_println!("[DEBUG] VM stress tests (stub)");
}

/// IRQ stress tests
pub fn run_irq_stress_tests() {
    crate::serial_println!("[DEBUG] IRQ stress tests (stub)");
}
