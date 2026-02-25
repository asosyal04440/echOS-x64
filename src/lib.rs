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

/// PE/COFF loader for Windows binaries
pub mod pe_loader;

/// Win32 API emulation
pub mod win32;

/// Doom game port
pub mod doom;

/// Doom downloader and launcher
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

/// PS/2 keyboard handler
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

/// x86_64 interrupt handlers
pub mod interrupts;

/// Inter-process communication
pub mod ipc;

/// Grafiksel kullanıcı arayüzü
pub mod gui;

/// Tile-based grafik engine
pub mod gfx;

/// Global Descriptor Table
pub mod gdt;

/// System call interface
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

/// Fault management and anti-crash system
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

/// Virtualization support (VMX/SVM, EPT)
pub mod virt;

/// GPU 3D API (Vulkan-like)
pub mod gpu3d;
