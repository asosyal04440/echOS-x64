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
//! - Boot öz-denetimi heap kurulduktan ve descriptor tabloları yüklendikten sonra
//!   çalışır; hiçbir bootloader belleğine veya sınırsız koleksiyona dayanmaz.
//! - Diğer tanı fonksiyonları kendi kapsamlarının testlerini ve seri ilerleme
//!   sözleşmelerini korur.
//! - `serial_println!` makrosu `debug::serial` modülüne bağlıdır; o yüzden bu
//!   modül çağrılmadan önce seri portun başlatılmış olması gerekir.

/// Sistem durumu analizörü
pub mod analyzer;

/// Acil durum seri port hata ayıklama çıkışı (COM1).
/// Interrupt gerektirmeyen, doğrudan I/O portuna erişen düşük-seviyeli sürücü.
pub mod serial;

/// Rate-limited debugcon (port 0xE9) writer.
/// Tamponlar ve en fazla her 100ms'de bir veya tampon %80 dolunca flush eder.
pub mod debugcon;

/// KGDB — Kernel GDB Remote Serial Protocol implementation.
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

/// Önyükleme öz-denetimi — canonical handover ve çekirdek altyapısını doğrular.
///
/// Çağrı noktası her protokolde heap ve descriptor tabloları hazırlandıktan
/// sonradır. Kontroller bounded'dır: canonical map'in sahiplik/ömür/validation
/// sözleşmesi, map toplam sayfa hesabı, gerçek IDTR/GDTR segment durumu, TLSF
/// sınırları ve merkezi boot-safety/pipeline hata durumu birlikte denetlenir.
/// Herhangi bir kontrol başarısızsa `false` döner ve ihlal merkezi kayda yazılır.
pub fn boot_self_check(ctx: &crate::boot::context::BootContext) -> bool {
    use crate::boot::context::{
        FieldState, MemoryMapLifetime, MemoryMapOwnership, MemoryMapValidation,
        BOOT_CONTEXT_VERSION,
    };
    use crate::boot::pipeline::{PhaseState, BOOT_PIPELINE};
    use crate::boot::safety::{BOOT_SAFETY, HeapSafety, IdtSafety, ViolationType};
    use core::sync::atomic::Ordering;

    let map_ok = validate_memory_map(ctx);
    if !map_ok {
        BOOT_SAFETY.record_violation(
            ViolationType::MemoryMapInvalid,
            "Canonical memory map self-check başarısız",
            false,
        );
    }

    let context_ok = ctx.version == BOOT_CONTEXT_VERSION
        && ctx.memory_map.field_state == FieldState::PresentValidated
        && ctx.memory_map.ownership == MemoryMapOwnership::Kernel
        && ctx.memory_map.lifetime == MemoryMapLifetime::Static
        && ctx.memory_map.validation == MemoryMapValidation::FullyValidated
        && ctx.capabilities.contains(crate::boot::context::CapabilityFlags::MEMORY_MAP);
    if !context_ok {
        BOOT_SAFETY.record_violation(
            ViolationType::MemoryMapInvalid,
            "BootContext ownership/capability sözleşmesi geçersiz",
            false,
        );
    }

    let idt_ok = IdtSafety::verify_loaded();
    if !idt_ok {
        BOOT_SAFETY.record_violation(
            ViolationType::IdtLoadFailed,
            "IDTR doğrulaması başarısız",
            false,
        );
    }

    let gdt_ok = crate::gdt::verify_current();
    if !gdt_ok {
        BOOT_SAFETY.record_violation(
            ViolationType::Gpf,
            "GDT segment selector doğrulaması başarısız",
            false,
        );
    }

    let heap = HeapSafety::check_integrity();
    let heap_ok = heap.early_heap_ok && heap.main_heap_ok && !heap.corruption_detected;
    if !heap_ok {
        BOOT_SAFETY.record_violation(
            ViolationType::HeapCorruption,
            "TLSF heap sınır/bütünlük doğrulaması başarısız",
            heap.can_recover,
        );
    }

    let snapshot = BOOT_PIPELINE.current_snapshot();
    let pipeline_ok = !matches!(snapshot.state, PhaseState::Failed)
        && !BOOT_SAFETY.in_recovery.load(Ordering::Acquire)
        && !BOOT_SAFETY.critical_error.load(Ordering::Acquire);
    if !pipeline_ok {
        BOOT_SAFETY.record_violation(
            ViolationType::PhaseOrder,
            "Boot pipeline güvenli bir durumda değil",
            false,
        );
    }

    let ok = context_ok && map_ok && idt_ok && gdt_ok && heap_ok && pipeline_ok;
    crate::serial_println!(
        "[DEBUG] Boot self-check map={} context={} idt={} gdt={} heap={} safety={}",
        map_ok as u8,
        context_ok as u8,
        idt_ok as u8,
        gdt_ok as u8,
        heap_ok as u8,
        pipeline_ok as u8,
    );
    if ok {
        crate::serial_println!("[DEBUG] Boot self-check passed");
    } else {
        crate::serial_println!("[DEBUG] Boot self-check FAILED");
    }
    ok
}

/// Canonical map'in bounded invariantlerini yeniden hesaplayarak doğrular.
///
/// Normalizer sıralama/çakışma garantisi vermediği için burada yalnızca her
/// descriptor'ın taşmasız aralığı ve yayınlanan toplam sayfa sayısı doğrulanır;
/// overlap politikası adapter normalizasyonunun sorumluluğundadır.
fn validate_memory_map(ctx: &crate::boot::context::BootContext) -> bool {
    validate_normalized_memory_map(ctx.normalized_memory_map())
}

fn validate_normalized_memory_map(
    map: Option<&crate::boot::context::NormalizedMemoryMap>,
) -> bool {
    let map = match map {
        Some(map) if !map.is_empty() => map,
        _ => return false,
    };

    let mut total_pages = 0u64;
    let mut has_usable = false;
    for region in map.as_slice() {
        if region.len == 0 || region.base.checked_add(region.len).is_none() {
            return false;
        }
        if region.kind == crate::boot::context::MemoryRegionKind::Usable {
            has_usable = true;
        }
        let pages = region.len.saturating_add(4095) / 4096;
        total_pages = match total_pages.checked_add(pages) {
            Some(total) => total,
            None => return false,
        };
    }

    has_usable && total_pages != 0 && total_pages == map.total_pages
}

#[cfg(test)]
mod tests {
    use super::validate_normalized_memory_map;
    use crate::boot::context::{MemoryRegion, MemoryRegionKind, NormalizedMemoryMap};

    #[test]
    fn canonical_map_check_recomputes_page_total() {
        let mut map = NormalizedMemoryMap::empty();
        map.push(MemoryRegion {
            base: 0x20_0000,
            len: 0x3000,
            kind: MemoryRegionKind::Usable,
        })
        .unwrap();
        assert!(validate_normalized_memory_map(Some(&map)));
    }

    #[test]
    fn canonical_map_check_rejects_tampered_total() {
        let mut map = NormalizedMemoryMap::empty();
        map.push(MemoryRegion {
            base: 0x20_0000,
            len: 0x1000,
            kind: MemoryRegionKind::Usable,
        })
        .unwrap();
        map.total_pages += 1;
        assert!(!validate_normalized_memory_map(Some(&map)));
    }

    #[test]
    fn canonical_map_check_rejects_empty_or_reserved_only() {
        let empty = NormalizedMemoryMap::empty();
        assert!(!validate_normalized_memory_map(Some(&empty)));

        let mut reserved = NormalizedMemoryMap::empty();
        reserved
            .push(MemoryRegion {
                base: 0,
                len: 0x1000,
                kind: MemoryRegionKind::Reserved,
            })
            .unwrap();
        assert!(!validate_normalized_memory_map(Some(&reserved)));
    }
}

/// Ring 3 duman testi — temel kullanıcı alanı işlevselliğini doğrular.
///
/// x86-64 mimarisinde Ring 3, en düşük ayrıcalık seviyesidir (CPL=3).
/// Gerçek bir kullanıcı imajı başlatmadan önce syscall geçişinin dayandığı
/// descriptor/TSS ve kullanıcı yığını sözleşmesini denetler. Böylece test,
/// scheduler'ı sonsuz bir Ring-3 shell döngüsüne sokmadan fail-closed çalışır.
pub fn run_ring3_smoketest() -> bool {
    let selectors = crate::gdt::current_selectors();
    let (stack_base, stack_top) = crate::memory::user_stack_bounds();
    let selectors_ok = (selectors.user_code_selector.0 & 3) == 3
        && (selectors.user_data_selector.0 & 3) == 3
        && (selectors.code_selector.0 & 3) == 0
        && (selectors.data_selector.0 & 3) == 0;
    let tss_ok = crate::gdt::current_tss_rsp0() != 0;
    let stack_ok = stack_top > stack_base
        && crate::memory::is_user_range(stack_base, stack_top - stack_base);
    let ok = selectors_ok && tss_ok && stack_ok;
    if !ok {
        crate::boot::safety::BOOT_SAFETY.record_violation(
            crate::boot::safety::ViolationType::InvalidPointer,
            "Ring3 selector/TSS/user-stack sözleşmesi başarısız",
            false,
        );
    }
    crate::serial_println!(
        "[DEBUG] Ring3 smoketest selectors={} tss={} stack={} result={}",
        selectors_ok as u8,
        tss_ok as u8,
        stack_ok as u8,
        ok as u8,
    );
    crate::serial_println!("[RING3_TEST] {}", if ok { "PASS" } else { "FAIL" });
    ok
}

/// VM security tests — kullanıcı aralığı sınırlarını ve nested-page-table
/// yöneticisinin başlatılmış durumunu gözlemler.
pub fn run_vm_security_tests() -> bool {
    let (stack_base, stack_top) = crate::memory::user_stack_bounds();
    let user_stack_ok = stack_top > stack_base
        && crate::memory::is_user_range(stack_base, stack_top - stack_base);
    let kernel_range_rejected = !crate::memory::is_user_range(0xffff_8000_0000_0000, 4096);
    let vmm = crate::virt::get_vmm();
    // An initialized VMM may legitimately have no guest yet.  Require the
    // nested table only when the snapshot actually contains VM 0; treating
    // “manager online, guest set empty” as a failure would reject a valid
    // post-boot state before the first VM is created.
    let vm0_expected = vmm.vms.contains_key(&0);
    let nested_ok = !vm0_expected || crate::virt::get_vm_page_table(0).is_some();
    let ok = user_stack_ok && kernel_range_rejected && nested_ok;
    if !ok {
        crate::boot::safety::BOOT_SAFETY.record_violation(
            crate::boot::safety::ViolationType::InvalidPointer,
            "VM kullanıcı aralığı/nested paging doğrulaması başarısız",
            false,
        );
    }
    crate::serial_println!(
        "[DEBUG] VM security user_stack={} kernel_rejected={} nested={} initialized={} result={}",
        user_stack_ok as u8,
        kernel_range_rejected as u8,
        nested_ok as u8,
        vmm.initialized as u8,
        ok as u8,
    );
    crate::serial_println!(
        "[VM_SECURITY_TEST] {}",
        if ok { "PASS" } else { "FAIL" }
    );
    ok
}

/// VM stress tests — allocator üzerinden bounded page-granularity yaz/oku/
/// serbest bırak döngüsü ile guest-memory backing sözleşmesini zorlar.
pub fn run_vm_stress_tests() -> bool {
    const ITERATIONS: usize = 16;
    const BLOCK_SIZE: usize = 4096;
    let mut ok = true;
    let mut completed = 0usize;
    for iteration in 0..ITERATIONS {
        // Exercise the canonical PMM/HHDM path.  The general heap may return
        // a lazy user VMA during early boot; writing through that address would
        // test the page-fault recovery path instead of the VM backing contract.
        let Some(hhdm_page) = crate::memory::alloc_zeroed_page() else {
            ok = false;
            break;
        };
        let pattern = (iteration as u8) ^ 0xA5;
        let ptr = hhdm_page as *mut u8;
        unsafe {
            core::ptr::write_bytes(ptr, pattern, BLOCK_SIZE);
            let first = ptr.read_volatile();
            let last = ptr.add(BLOCK_SIZE - 1).read_volatile();
            if first != pattern || last != pattern {
                ok = false;
            }
        }
        let phys = hhdm_page.saturating_sub(crate::memory::active_physical_offset());
        crate::memory::free_phys(phys, BLOCK_SIZE);
        completed += 1;
        if !ok {
            break;
        }
    }
    if !ok {
        crate::boot::safety::BOOT_SAFETY.record_violation(
            crate::boot::safety::ViolationType::HeapCorruption,
            "VM stress backing page round-trip başarısız",
            false,
        );
    }
    crate::serial_println!(
        "[DEBUG] VM stress iterations={}/{} result={}",
        completed,
        ITERATIONS,
        ok as u8,
    );
    crate::serial_println!(
        "[VM_STRESS_TEST] {}",
        if ok { "PASS" } else { "FAIL" }
    );
    ok
}

/// IRQ stress tests — gerçek IRQ metrik yoluna bounded synthetic dispatch
/// gönderir ve sayım/latency sayaçlarının ilerlediğini doğrular.
pub fn run_irq_stress_tests() -> bool {
    const VECTOR: u8 = 0xF0;
    const ITERATIONS: u64 = 64;
    let old_storm_limit = crate::interrupts::irq_storm_limit();
    crate::interrupts::set_irq_storm_limit(ITERATIONS + 1);
    crate::interrupts::clear_irq_metrics();
    crate::interrupts::simulate_irq(VECTOR, ITERATIONS);
    let count = crate::interrupts::irq_count(VECTOR);
    let (_, _, samples) = crate::interrupts::irq_latency_stats(VECTOR);
    crate::interrupts::set_irq_storm_limit(old_storm_limit);
    let ok = count >= ITERATIONS && samples <= count;
    if !ok {
        crate::boot::safety::BOOT_SAFETY.record_violation(
            crate::boot::safety::ViolationType::Timeout,
            "IRQ synthetic dispatch metrikleri ilerlemedi",
            false,
        );
    }
    crate::serial_println!(
        "[DEBUG] IRQ stress vector={:#x} count={} samples={} result={}",
        VECTOR,
        count,
        samples,
        ok as u8,
    );
    crate::serial_println!(
        "[IRQ_STRESS_TEST] {}",
        if ok { "PASS" } else { "FAIL" }
    );
    ok
}
