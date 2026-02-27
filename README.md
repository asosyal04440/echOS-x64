<div align="center">

```
 ███████╗ ██████╗██╗  ██╗ ██████╗ ███████╗    ██╗  ██╗ ██████╗ ██╗  ██╗
 ██╔════╝██╔════╝██║  ██║██╔═══██╗██╔════╝    ╚██╗██╔╝██╔════╝ ██║  ██║
 █████╗  ██║     ███████║██║   ██║███████╗     ╚███╔╝ ███████╗ ███████║
 ██╔══╝  ██║     ██╔══██║██║   ██║╚════██║     ██╔██╗ ██╔═══██╗╚════██║
 ███████╗╚██████╗██║  ██║╚██████╔╝███████║    ██╔╝ ██╗╚██████╔╝     ██║
 ╚══════╝ ╚═════╝╚═╝  ╚═╝ ╚═════╝ ╚══════╝    ╚═╝  ╚═╝ ╚═════╝      ╚═╝
```

**A modern, bare-metal x86-64 operating system — crafted entirely in Rust.**

[![CI: Simics Zero-Tolerance](https://img.shields.io/badge/CI-Simics%20Zero--Tolerance-blueviolet?style=flat-square&logo=github-actions)](/.github/workflows/simics-zero-tolerance.yml)
[![Rust: nightly](https://img.shields.io/badge/Rust-nightly-orange?style=flat-square&logo=rust)](rust-toolchain.toml)
[![Target: x86_64-unknown-none](https://img.shields.io/badge/target-x86__64--unknown--none-lightgrey?style=flat-square)]()
[![License: MIT](https://img.shields.io/badge/license-MIT-green?style=flat-square)](LICENSE)
[![no_std](https://img.shields.io/badge/no__std-✓-blue?style=flat-square)]()
[![Boot: UEFI](https://img.shields.io/badge/boot-UEFI%20%7C%20Multiboot2%20%7C%20Limine-informational?style=flat-square)]()

> *Runs Doom. Speaks TLS 1.3. Boots from UEFI. — All in Rust, with zero C standard library.*

</div>

---

## Table of Contents

1. [Overview](#overview)
2. [Architecture](#architecture)
3. [Feature Highlights](#feature-highlights)
4. [Module Tree](#module-tree)
5. [Building](#building)
6. [Running](#running)
7. [CI — Simics Zero-Tolerance Gate](#ci--simics-zero-tolerance-gate)
8. [Technical Report](#technical-report)
9. [Third-Party Components](#third-party-components)
10. [License](#license)

---

## Overview

**echOS-x64** is a fully featured, research-grade operating system kernel built from the ground up in **Rust** (`#![no_std]`). It targets the `x86_64` architecture and boots via **UEFI**, **Multiboot2**, or the **Limine** protocol.

The project is not a toy kernel. It implements production-class subsystems including a CFS/RT/Deadline scheduler, TLS 1.3 networking stack, TPM 2.0 Secure Boot, ext4 journaling, IronShim Windows-driver compatibility, a tile-based GPU compositor, and a POSIX/Win32 API emulation layer — all in safe + unsafe Rust, with no external libc.

---

## Architecture

```
┌──────────────────────────────────────────────────────────────────────┐
│                         USER SPACE (future)                          │
│         POSIX API │ Win32 API │ ELF Loader │ PE Loader │ VDSO        │
├──────────────────────────────────────────────────────────────────────┤
│                         SYSTEM CALL INTERFACE                        │
├────────────┬─────────────┬──────────────┬───────────────────────────┤
│  SCHEDULER │   MEMORY    │  FILESYSTEM  │       NETWORKING           │
│  CFS/RT/DL │  PMM + VMM  │ FAT/ext4/    │  TCP/UDP/TLS1.3/QUIC/     │
│  SMP 8192  │  TLSF/Buddy │ NTFS/f2fs/   │  WireGuard/IPSec/HTTP2    │
│  Work-Steal│  THP/zswap  │ NFS/FUSE     │  DNS-over-HTTPS/DoT       │
├────────────┴─────────────┴──────────────┴───────────────────────────┤
│                            KERNEL CORE                               │
│   GDT │ IDT │ APIC │ IOAPIC │ IRQ Domains │ Softirq │ RCU │ Preempt │
├──────────────────────────────────────────────────────────────────────┤
│                           DRIVER LAYER                               │
│  NVMe │ ATA │ VirtIO │ PCI │ USB (HID/CDC/MSD) │ PS/2 │ Audio │ BT  │
├──────────────────────────────────────────────────────────────────────┤
│                          HARDWARE (x86-64)                           │
│        UEFI Firmware │ ACPI Tables │ TSC │ RDRAND │ AES-NI │ AVX    │
└──────────────────────────────────────────────────────────────────────┘
```

**Boot flow:**

```
UEFI/Multiboot2/Limine
        │
        ▼
  UEFI Entry (uefi_main)  ──OR──  Limine Entry  ──OR──  Multiboot2 Entry
        │
        ▼
  GOP Framebuffer init  →  Splash screen
        │
        ▼
  ACPI parse  →  APIC / IOMMU init  →  SMP AP bringup (up to 8192 CPUs)
        │
        ▼
  PMM + Paging  →  TLSF Heap  →  Security (SMEP/SMAP/NX)  →  TPM Secure Boot
        │
        ▼
  Drivers (PCI / NVMe / VirtIO / USB)  →  Filesystem mount
        │
        ▼
  Network stack  →  GUI compositor  →  Shell / Desktop
```

---

## Feature Highlights

### 🧠 Memory Management
| Feature | Details |
|---------|---------|
| Physical Memory Manager | Fibonacci Buddy + PMM (O(1) alloc) |
| Virtual Memory | 4-level page tables, 2 MiB hugepages |
| Heap Allocator | **TLSF** (Two-Level Segregated Fit) + Bump + Linked-List fallback |
| Transparent Huge Pages | THP coalescing daemon |
| Memory Compression | `zswap`-style compressed swap |
| cgroups v2 | Per-task memory limits and accounting |
| OOM Killer | Priority-based victim selection |
| NUMA | Topology-aware allocation |

### ⚡ Scheduler
| Feature | Details |
|---------|---------|
| CFS | Completely Fair Scheduler with virtual runtime (Linux-style) |
| RT Scheduler | SCHED_FIFO / SCHED_RR for real-time tasks |
| Deadline Scheduler | EDF-based (SCHED_DEADLINE) |
| Ghost Scheduler | Google-style in-kernel agent scheduling |
| SMP | Work-stealing Chase-Lev deque, up to **8 192 CPUs** |
| Timer | High-resolution TSC-based timer wheel |
| Futex | Userspace fast-path mutex |
| CPU Affinity | NUMA-aware task pinning |

### 🌐 Networking
| Protocol | Status |
|----------|--------|
| Ethernet / ARP / IPv4 / IPv6 | ✅ |
| TCP / UDP | ✅ (smoltcp-backed) |
| DHCP | ✅ |
| DNS / DNSSEC | ✅ |
| DNS-over-HTTPS (DoH) | ✅ |
| DNS-over-TLS (DoT) | ✅ |
| **TLS 1.3** (from scratch) | ✅ HKDF + ChaCha20 + SHA-2 |
| HTTP/1.1 + HTTP/2 | ✅ |
| WebSocket | ✅ |
| **QUIC** | ✅ |
| **WireGuard** | ✅ |
| IPSec | ✅ |
| Netfilter / iptables-style | ✅ |
| Network Namespaces | ✅ |
| `io_uring`-style async I/O | ✅ |
| Zero-copy networking | ✅ |
| x.509 / PKI | ✅ |

### 📁 File Systems
| FS | Features |
|----|---------|
| **FAT32** | Read/write |
| **ext4** | Journaling (ext4_journal), ACL, xattr, quotas, inotify |
| **NTFS** | Read support |
| **F2FS** | Flash-friendly FS |
| **NFS** | Network file system client |
| **FUSE** | Userspace filesystem protocol |
| File locking | POSIX advisory + mandatory |
| Zero-copy splice | `sendfile`-style |

### 🔒 Security
| Feature | Details |
|---------|---------|
| SMEP / SMAP | CR4 hardware enforcement |
| NX / DEP | W^X page table policy |
| Stack Canary | Per-task canary values |
| ASLR | Randomised kernel & user VA layout |
| **TPM 2.0** | PCR extend, measured boot |
| **UEFI Secure Boot** | PK/KEK/db/dbx chain-of-trust |
| Capability-based Security | POSIX capabilities |
| MAC (SELinux-like) | Mandatory Access Control framework |
| seccomp | Syscall filter policies |
| IMA / EVM | Integrity Measurement Architecture |
| Audit | Kernel audit log |
| Keyring | In-kernel key storage |

### 🔐 Cryptography (hardware-accelerated, no_std)
- **AES-NI** — hardware AES-128/256
- **SHA-256 / SHA-3** — SHA-NI accelerated
- **Blake3** — fast hashing
- **ChaCha20-Poly1305** — AEAD cipher
- **Ed25519** — digital signatures
- **RSA** — asymmetric crypto
- **Argon2** — password hashing
- **HKDF** — TLS 1.3 key derivation

### 🎮 GUI & Graphics
- **Tile-based compositor** with SIMD-accelerated blending
- **VirtIO GPU** + **DRM** backend
- Full **Window Manager** (`echOS-WM`): windows, focus, z-ordering
- **Desktop**, Dock, Spotlight, Mission Control, Spaces (virtual desktops)
- **Font rendering** (TrueType rasterizer, glyph atlas, text layout)
- Built-in apps: Terminal, File Explorer, Text Editor, Image Viewer, Browser, Music Player, Activity Monitor, Settings
- Drag-and-drop, clipboard, notifications, wallpaper

### 🪟 Compatibility Layers
| Layer | Details |
|-------|---------|
| **Win32 API** | Emulation layer for Windows applications |
| **IronShim** | Windows kernel driver compatibility (ironshim-rs) |
| **POSIX** | `pipe`, `msgq`, `semaphore`, `dlopen` |
| **Linux Glue** | Partial Linux kernel ABI compatibility |
| **ELF Loader** | Execute Linux ELF binaries |
| **PE/COFF Loader** | Load Windows PE executables |
| **VDSO** | Virtual DSO for fast syscalls |

### 🛠 Hardware Drivers
- **Storage**: NVMe, ATA/AHCI, VirtIO-blk
- **Network**: VirtIO-net, smoltcp NIC driver
- **Display**: VirtIO-GPU, VirGL (3D), framebuffer DRM
- **Input**: PS/2 keyboard+mouse, USB HID
- **USB**: EHCI/XHCI, HID, CDC, Mass Storage, Hub
- **Bus**: PCI/PCIe (ECAM), I²C, SPI
- **Audio**: AC97/HDA framework
- **Bluetooth**: HCI transport layer
- **Thermal**: ACPI thermal zones
- **Watchdog**: Hardware watchdog timer
- **IOMMU**: VT-d / AMD-Vi

### 🧩 ACPI
- Full ACPI table parsing (MADT, FADT, DSDT, SSDT)
- AML interpreter
- GPE (General Purpose Events)
- ACPI power states (S0–S5)
- Embedded Controller (EC) protocol

### 💀 Fault Tolerance
- **Checkpoint & recovery** — kernel state snapshots
- **Fault injection** — testing subsystem resilience
- **Monitors** — per-subsystem health monitors (CPU, memory, scheduler, IRQ, FS, SMP, driver)
- **Degradation modes** — graceful service degradation
- **Emergency handlers** — last-resort crash recovery
- **Watchdog** — hardware + software watchdog

---

## Module Tree

```
src/
├── main.rs              # Kernel entry point (UEFI / Limine / Multiboot2)
├── lib.rs               # Crate root, module declarations
│
├── boot/                # Boot protocol handlers, BootInfo extraction
├── acpi/                # ACPI table parsing, AML interpreter, MADT, GPE
├── apic/                # Local APIC, IO-APIC
├── cpu/                 # SMP AP startup, TSC, NUMA, microcode, virtualisation
├── gdt.rs               # Global Descriptor Table
├── interrupts/          # IDT, PIC, IRQ chip, IRQ domains, softirq, remapping
│
├── memory/              # PMM, paging, TLSF allocator, OOM, THP, zswap
├── allocator/           # Bump, linked-list, TLSF, stack allocators
│
├── task/                # CFS, RT, Deadline, Ghost scheduler, SMP work-steal
├── preempt.rs           # Preemption control (preempt_disable / enable)
├── rcu.rs               # Read-Copy-Update
├── atomic_ops.rs        # Architecture-specific atomics
├── memory_barriers.rs   # SMP memory barriers (smp_mb / rmb / wmb)
│
├── fs/                  # FAT32, ext4+journal, NTFS, F2FS, NFS, FUSE, VFS
├── net/                 # TCP/UDP, TLS 1.3, QUIC, WireGuard, IPSec, HTTP/2
├── drivers/             # NVMe, ATA, VirtIO, PCI, USB, audio, BT, IOMMU
│
├── security/            # SMEP/SMAP, TPM, Secure Boot, MAC, seccomp, audit
├── crypto/              # AES-NI, SHA, Blake3, ChaCha20, Ed25519, Argon2
├── fault/               # Fault injection, monitors, checkpoints, recovery
│
├── gui/                 # Window manager, desktop, dock, spotlight, apps
├── gfx/                 # Tile compositor, SIMD blending, GAL
├── gop/                 # UEFI GOP framebuffer
├── font/                # VGA bitmap font
│
├── ipc/                 # Channels, messages
├── tty/                 # TTY layer, PTY, ANSI, line discipline
├── serial/              # UART debug output
├── shell/               # Interactive shell, scripting, line editor
├── syscall.rs           # System call dispatcher
│
├── posix/               # POSIX compatibility (pipe, msgq, semaphore, dlopen)
├── linux_glue.rs        # Linux kernel ABI compatibility glue
├── elf.rs               # ELF binary loader
├── pe_loader.rs         # PE/COFF binary loader
├── win32.rs             # Win32 API emulation
├── ironshim_bridge.rs   # IronShim Windows-driver bridge
├── vdso.rs              # Virtual DSO
├── virt.rs              # VMX/SVM virtualisation
├── gpu3d.rs             # 3D GPU API (Vulkan-like)
│
├── doom.rs              # 🎮 Doom port
└── doom_launcher.rs     # Doom launcher
```

---

## Building

### Prerequisites

- **Rust nightly** — managed automatically via `rust-toolchain.toml`
- **LLVM / lld** linker
- QEMU (for local testing) or Intel Simics (for CI gate)

```bash
# Install Rust (if needed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install required targets (toolchain file handles this automatically)
rustup target add x86_64-unknown-none x86_64-unknown-uefi
```

### UEFI Build (primary target)

```bash
cargo build --target x86_64-unknown-uefi --release
# Output: target/x86_64-unknown-uefi/release/ech_os.efi
```

### Bare-metal Build (Limine/Multiboot2)

```bash
cargo build --target x86_64-unknown-none --release
# Output: target/x86_64-unknown-none/release/ech_os
```

---

## Running

### QEMU (UEFI — OVMF)

```powershell
.\run_qemu.ps1
```

Or manually:

```bash
qemu-system-x86_64 \
  -bios ovmf/OVMF.fd \
  -drive format=raw,file=fat:rw:esp/ \
  -m 512M \
  -serial stdio \
  -device virtio-net-pci \
  -device virtio-blk-pci,drive=disk0 \
  -drive id=disk0,file=disk.img,if=none,format=raw
```

### Intel Simics

```powershell
# Launch Simics GUI
.\run_simics.ps1

# Or headless gate run
Simics\echos-simics\bin\run-gate.bat
```

### Multiboot2 ISO

```bash
# ISO is pre-built at:
multiboot_iso/boot/ech_os

qemu-system-x86_64 -cdrom echos.iso -m 512M -serial stdio
```

---

## CI — Simics Zero-Tolerance Gate

Every pull request targeting `main` / `master` is blocked by a **five-axis hardware gate** running on an Intel Simics simulator.

### Gate axes

| Axis | Description |
|------|-------------|
| `boot_irq_input` | Clean UEFI boot, interrupt handling, keyboard/mouse input |
| `syscall_security` | System call ABI correctness + SMEP/SMAP enforcement |
| `fs_network` | Filesystem R/W integrity + network connectivity |
| `performance` | Boot time, scheduler latency, memory throughput benchmarks |
| `extreme_ironshim` | IronShim Windows-driver stress test |

### Rules

- **Any single FAIL → `exit code 2` → merge blocked.**
- Gate logs: `Simics/echos-simics/targets/echos/logs/gate_run_<timestamp>.log`
- Machine-readable verdict: `Simics/echos-simics/targets/echos/logs/gate_verdict_<timestamp>.json`

### Manual gate run

```bat
Simics\echos-simics\bin\run-gate.bat
```

### CI workflow

```yaml
# .github/workflows/simics-zero-tolerance.yml
# Runner label: [self-hosted, windows, simics]
```

Artifacts (logs + serial capture) are uploaded after every gate run regardless of result.

---

## Technical Report

A detailed technical report covering the internal design decisions, subsystem architecture, and benchmarks is available in:

```
echOS_teknik_rapor.pdf
```

---

## Third-Party Components

| Component | Location | License |
|-----------|----------|---------|
| `virtio-drivers` | `third_party/virtio-drivers` | MIT / Apache-2.0 |
| `core_io` | `third_party/core_io` | MIT |
| `ironshim-rs` | `third_party/ironshim-rs` | confidential |
| `smoltcp` | crates.io | MIT / Apache-2.0 |
| `rcore-fs` | git submodule | MIT |

---

## License

This project is distributed under the **MIT License** — see [`LICENSE`](LICENSE) for details.

Certain subsystems (IronShim, Simics gate internals) remain **confidential** and are not included in this public repository.

---

<div align="center">

*echOS — because the best way to understand an OS is to build one.*

</div>
