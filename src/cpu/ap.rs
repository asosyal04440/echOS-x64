//! # AP (Application Processor) Başlatma Modülü
//!
//! SMP sistemlerde BSP dışındaki her işlemci çekirdeği için çalışan giriş noktasını
//! tanımlar. AP'ler INIT-SIPI-SIPI dizisi ile uyandırılıp bu fonksiyona atlarlar.
//! GDT ve IDT sırasıyla yüklenir, ardından LAPIC, per-CPU veri ve kesme sistemi
//! başlatılır; son adımda zamanlayıcının boşta döngüsüne girilir.

use crate::syscall::{init_cpu_data, CpuData};
use x86_64::VirtAddr;

#[no_mangle]
pub extern "sysv64" fn ap_entry(cpu_data: &'static mut CpuData) -> ! {
    // KRİTİK: Per-CPU verisi İLK olarak başlatılmalı (per-CPU erişimi için zorunlu)
    unsafe {
        init_cpu_data(cpu_data as *mut CpuData);
    }

    let cpu_id = cpu_data.cpu_id;

    // KRİTİK SIRA: GDT'yi IDT'den ÖNCE yüklemeliyiz!
    // Erken boot AP trampolin kodu CS=0x18 kullanır. Kernel GDT ise CS=0x08 kullanır
    // ve 0x18'i TSS olarak tanımlar. IDT CS=0x18 iken oluşturulursa, tüm interrupt
    // kapıları yeni GDT'deki TSS selektörüne işaret eder ve her interrupt'ta #GP(0x18) oluşur.
    crate::gdt::init_for_cpu(cpu_id, VirtAddr::new(cpu_data.kernel_stack_top));

    unsafe {
        // Bu CPU için cpu_data içindeki cpu_id kullanılarak IDT elle yüklenir
        let idt = crate::interrupts::init_idt_for_cpu(cpu_id);
        idt.load();
    }

    // Artık istisna tetikleyebilecek işlemler güvenle yapılabilir
    // HAM UART HATA AYIKLAMA: COM1 portuna (0x3f8) doğrudan 'A' yaz
    unsafe {
        x86_64::instructions::port::Port::<u8>::new(0x3f8).write(b'A');
    }
    // HAM UART HATA AYIKLAMA: COM1 portuna (0x3f8) doğrudan 'B' yaz
    unsafe {
        x86_64::instructions::port::Port::<u8>::new(0x3f8).write(b'B');
    }

    // cpu_data'dan CPU kimliğini zaten biliyoruz.
    // Erken boot sırasında MMIO/MSR okumak yerine APIC kimliğini küresel durumdan alabiliriz.
    let apic_id = {
        let mut id = cpu_id;
        if let Some(state) = crate::cpu::smp::SMP_STATE.try_lock() {
            id = state.cpu_apic_ids.get(cpu_id as usize).copied().unwrap_or(cpu_id);
        }
        id
    };

    // HAM UART HATA AYIKLAMA: COM1 portuna (0x3f8) doğrudan 'C' yaz
    unsafe {
        x86_64::instructions::port::Port::<u8>::new(0x3f8).write(b'C');
    }
    unsafe {
        crate::debug::serial::EMERGENCY_SERIAL.write_byte(b'D');
    }
    let _ = crate::apic::lapic::init();
    unsafe {
        crate::debug::serial::EMERGENCY_SERIAL.write_byte(b'E');
    }
    // GS_BASE'i cpu_data'ya yönlendir — kernel per-CPU veri erişimi için kritik
    unsafe {
        use x86_64::registers::model_specific::Msr;
        Msr::new(0xC0000101).write(cpu_data as *mut CpuData as u64);
    }
    // Not: GDT ve IDT başlangıçta zaten yüklendi — init_per_cpu() tekrar çağrılmamalı
    // Not: IDT başlangıçta zaten yüklendi — init_per_cpu() tekrar çağrılmamalı
    unsafe {
        crate::debug::serial::EMERGENCY_SERIAL.write_byte(b'G');
        x86_64::instructions::port::Port::<u8>::new(0x3f8).write(b'W');
    }
    crate::cpu::smp::mark_cpu_online(cpu_id, apic_id);
    unsafe {
        crate::debug::serial::EMERGENCY_SERIAL.write_byte(b'H');
    }
    unsafe {
        x86_64::instructions::interrupts::enable();
    }
    unsafe {
        crate::debug::serial::EMERGENCY_SERIAL.write_byte(b'I');
    }
    crate::task::scheduler::idle_loop();
}
