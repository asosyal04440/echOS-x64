
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

extern crate alloc;

// ============================================================================
// MODÜLLER
// ============================================================================

/// ACPI tablolarını okuma
pub mod acpi;

/// Heap bellek allocator'ları
pub mod allocator;

/// UEFI boot işlemleri
pub mod boot;

/// CPU yapılandırması (GDT, IDT)
pub mod cpu;

/// VGA bitmap fontları
pub mod font;

/// Dosya sistemi (FAT32)
pub mod fs;

/// UEFI GOP framebuffer
pub mod gop;

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

/// Debug araçları
pub mod debug;
