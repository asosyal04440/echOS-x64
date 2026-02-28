//! echOS, Rust dilinde yazılmış bare-metal bir işletim sistemidir.
//! Windows ve Linux driver'larını çalıştırabilecek hibrit bir mimari hedefler.
//!
//! ## Modüller
//!
//! - **acpi**: ACPI tablolarını okuma ve sistem konfigürasyonu
//! - **allocator**: Heap bellek yönetimi (TLSF algoritması)
//! - **boot**: UEFI boot işlemleri
//! - **cpu**: GDT, IDT ve interrupt yönetimi
//! - **drivers**: Donanım sürücüleri (mouse, keyboard, disk)
//! - **font**: VGA font rendering
//! - **fs**: Dosya sistemi desteği (FAT32)
//! - **gfx**: Grafik engine (tile-based compositor)
//! - **gop**: UEFI GOP framebuffer
//! - **gui**: Kullanıcı arayüzü (windows, desktop, theme)
//! - **interrupts**: x86_64 interrupt handling
//! - **ipc**: Inter-process communication
//! - **memory**: Sayfa tablosu ve bellek yönetimi
//! - **task**: Preemptive multitasking scheduler

#![no_std]
#![feature(abi_x86_interrupt)]
#![feature(c_variadic)]
#![allow(dead_code)]
#![allow(clippy::all)]
#![allow(
    bad_asm_style,
    non_camel_case_types,
    unused_assignments,
    unused_imports,
    unused_mut,
    unused_unsafe,
    unused_variables,
    unreachable_patterns
)]

extern crate alloc;

// ============================================================================
// MODÜLLER
// ============================================================================

/// Heap bellek allocator'ları
pub mod allocator;

pub mod acpi;

/// Windows PE/COFF ikili dosyalarını yükleyen loader.
/// PE formatı, Windows .exe ve .dll dosyalarında kullanılan yürütülebilir format standardıdır.
pub mod pe_loader;

/// Win32 API öykünmesi (emulation) — Windows programlarının echOS üzerinde çalışmasını sağlar.
/// Sistem çağrılarını yakalayıp echOS karşılıklarına yönlendirir.
pub mod win32;

/// Doom oyun portu — klasik id Software oyununun echOS üzerindeki versiyonu.
pub mod doom;

/// Doom WAD dosyası indirici ve oyun başlatıcısı.
pub mod doom_launcher;

pub struct KernelBootContext {
    pub physical_memory_offset: u64,
}

/// UEFI boot işlemleri
pub mod boot;

/// CPU yapılandırması (GDT, IDT, SMP, ACPI)
pub mod cpu;

/// VGA bitmap fontları
pub mod font;

/// Dosya sistemi (FAT32)
pub mod fs;

/// UEFI GOP framebuffer
pub mod gop;

pub mod splash;

/// PS/2 klavye sürücüsü — tuş basma/bırakma olaylarını interrupt üzerinden alır
/// ve bir ring buffer'da saklar.
pub mod keyboard;

/// Bellek ve sayfa tablosu yönetimi
pub mod memory;

/// Serial port debug çıktısı
pub mod serial;

/// Task scheduler ve context switch
pub mod task;

/// Komut satırı shell
pub mod shell;

/// Donanım sürücüleri
pub mod drivers;

pub mod apic;

/// x86_64 interrupt handler'ları — donanım kesintilerini ve CPU istisnalarını yönetir.
/// Her interrupt vektörü için IDT kaydı barındırır.
pub mod interrupts;

/// Inter-process communication (süreçler arası iletişim)
pub mod ipc;

/// Grafiksel kullanıcı arayüzü
pub mod gui;

/// Tile-based grafik engine
pub mod gfx;

/// Global Descriptor Table
pub mod gdt;

/// Sistem çağrısı arayüzü — kullanıcı alanından çekirdek servislerine erişim kapısı.
pub mod syscall;

/// Rastgele sayı üretici
pub mod random;
pub mod tty;
pub mod memory_barriers;
pub mod rcu;
pub mod preempt;
pub mod atomic_ops;
pub mod hotplug;
pub mod numa;
pub mod power;
pub mod topology;
pub mod affinity;

/// Debug araçları
pub mod debug;

/// Güvenlik alt sistemi (SMEP/SMAP, Stack Canary, ASLR, NX/DEP, W^X)
pub mod security;

/// Hata yönetimi ve çökmeden koruma sistemi — kernel paniklerini yakalar
/// ve sistemi kurtarmaya çalışır.
pub mod fault;

/// Ağ alt sistemi (TCP/IP, Socket API, DNS, DHCP)
pub mod net;

/// Donanım hızlandırmalı kriptografi (AES-NI, SHA-NI)
pub mod crypto;

pub mod elf;
pub mod linux_glue;
pub mod posix;
pub mod shim_layer;
pub mod ironshim_bridge;
pub mod vdso;

/// Sanallaştırma desteği (VMX/SVM, EPT) — hypervisor yetenekleri sağlar.
/// Intel VT-x ve AMD-V donanım sanallaştırmasını kullanır.
pub mod virt;

/// GPU 3D API — Vulkan benzeri grafik API'si.
/// Shader, render pass ve pipeline kavramlarını uygular.
pub mod gpu3d;
