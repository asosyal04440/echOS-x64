# echOS-x64

**echOS-x64 is a Rust `no_std` x86-64 operating system kernel for UEFI, Multiboot2, Limine, SMP scheduling, memory management, filesystems, networking, and GUI/compositor research.**

[Türkçe README](README.tr.md) · [Technical report](echOS_teknik_rapor.pdf) · [Build and run](#building)

<div align="center">

```
 ███████╗ ██████╗██╗  ██╗ ██████╗ ███████╗    ██╗  ██╗ ██████╗ ██╗  ██╗
 ██╔════╝██╔════╝██║  ██║██╔═══██╗██╔════╝    ╚██╗██╔╝██╔════╝ ██║  ██║
 █████╗  ██║     ███████║██║   ██║███████╗     ╚███╔╝ ███████╗ ███████║
 ██╔══╝  ██║     ██╔══██║██║   ██║╚════██║     ██╔██╗ ██╔═══██╗╚════██║
 ███████╗╚██████╗██║  ██║╚██████╔╝███████║    ██╔╝ ██╗╚██████╔╝     ██║
 ╚══════╝ ╚═════╝╚═╝  ╚═╝ ╚═════╝ ╚══════╝    ╚═╝  ╚═╝ ╚═════╝      ╚═╝
```

**Rust `no_std` x86-64 operating system research kernel.**

[![CI: Simics Zero-Tolerance](https://img.shields.io/badge/CI-Simics%20Zero--Tolerance-blueviolet?style=flat-square&logo=github-actions)](/.github/workflows/simics-zero-tolerance.yml)
[![Rust: nightly](https://img.shields.io/badge/Rust-nightly-orange?style=flat-square&logo=rust)](rust-toolchain.toml)
[![Target: x86_64-unknown-none](https://img.shields.io/badge/target-x86__64--unknown--none-lightgrey?style=flat-square)]()
[![License: AGPL-3.0](https://img.shields.io/badge/license-AGPL--3.0-green?style=flat-square)](LICENSE)
[![no_std](https://img.shields.io/badge/no__std-✓-blue?style=flat-square)]()
[![Boot: UEFI](https://img.shields.io/badge/boot-UEFI%20%7C%20Multiboot2%20%7C%20Limine-informational?style=flat-square)]()

</div>

---

## Table of Contents

1. [Overview](#overview)
2. [Architecture](#architecture)
3. [Current Status](#current-status)
4. [Module Tree](#module-tree)
5. [Building](#building)
6. [Running](#running)
7. [CI — Simics Zero-Tolerance Gate](#ci--simics-zero-tolerance-gate)
8. [Technical Report](#technical-report)
9. [Third-Party Components](#third-party-components)
10. [License](#license)

---

## Overview

**echOS-x64** is a Rust `no_std` x86-64 operating-system research kernel. The current public repository focuses on boot flow, kernel architecture, memory/scheduler/driver experiments, host-side tooling, and reproducible local validation paths.

This README is intentionally conservative: `✅` means the capability has a concrete implementation or repository workflow visible in this tree; `⏳` means the subsystem is under active development, partially integrated, target-specific, or still needs stronger validation before it should be presented as complete.

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
│  CFS/RT/DL │  PMM + VMM  │ FAT/ext/VFS  │  smoltcp-backed stack     │
│  SMP/AP    │  Allocators │ image tools  │  protocol experiments     │
│  Work-Steal│  paging     │ validation   │  packet/device plumbing   │
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
  ACPI parse  →  APIC / IOMMU init  →  SMP AP bringup path
        │
        ▼
  PMM + Paging  →  TLSF Heap  →  Security (SMEP/SMAP/NX)  →  TPM Secure Boot
        │
        ▼
  Drivers (PCI / NVMe / VirtIO / USB)  →  Filesystem mount
        │
        ▼
  Network experiments  →  GUI/compositor experiments  →  shell/tooling
```

---

## Current Status

| Area | Status | Notes |
|------|--------|-------|
| Rust `no_std` kernel crate | ✅ | Primary kernel code is Rust with explicit bare-metal targets. |
| UEFI build target | ✅ | `x86_64-unknown-uefi` build path and `.efi` artifact are documented. |
| QEMU/OVMF launch path | ✅ | `run_qemu.ps1` is the local smoke-run entrypoint. |
| Shareable UEFI VM ISO | ✅ | `scripts/build_vm_iso.ps1` emits `build/appliance/echOS-uefi.iso`. |
| AGPL-3.0 project licensing | ✅ | Root `LICENSE`, manifest metadata, and README badge agree. |
| Simics gate tooling | ✅ | Gate scripts and log/verdict locations are part of the repository workflow. |
| Limine / Multiboot2 paths | ⏳ | Present in the code and docs, but UEFI is the primary public run path. |
| SMP / AP bring-up | ⏳ | Active kernel path; VirtualBox smoke profile is intentionally single-vCPU. |
| Memory manager stack | ⏳ | PMM, paging, and allocator work exists; broader invariants still need tighter public proof coverage. |
| Scheduler stack | ⏳ | CFS/RT/deadline/work-stealing work is in-tree; end-to-end workload validation is still ongoing. |
| Filesystems | ⏳ | FAT/ext-style/VFS work is in-tree; treat non-smoke paths as under validation. |
| Networking | ⏳ | smoltcp-backed work is in-tree; protocol matrix is not presented as complete. |
| GUI/compositor | ⏳ | Framebuffer, graphics, and UI experiments are in-tree; not a finished desktop environment. |
| Win32/POSIX/IronShim compatibility | ⏳ | Compatibility work exists, but public support should be treated as experimental. |
| Hardware driver surface | ⏳ | VirtIO/PCI/storage/input/display work is active; bare-metal hardware coverage varies by target. |

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
└── gpu3d.rs             # 3D GPU API experiments
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

### Shareable UEFI VM ISO

```powershell
.\scripts\build_vm_iso.ps1
# Output: build\appliance\echOS-uefi.iso
```

Attach this ISO as optical media in a VM configured for UEFI/OVMF firmware.
Legacy BIOS boot is outside this artifact contract; select EFI/UEFI firmware in
VirtualBox, VMware, or QEMU.

VirtualBox test profile: `Other/Unknown (64-bit)`, EFI enabled, `1` CPU, and
disk/optical media first in the boot order. VirtualBox 7.2.x currently trips the
AP bring-up path on the second vCPU while loading the TSS, so SMP is disabled for
the VirtualBox smoke profile; QEMU/Simics SMP validation is a separate gate.

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

### Legacy Multiboot2 ISO

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

echOS-x64 project code is distributed under the **GNU Affero General Public License v3.0 only** — see [`LICENSE`](LICENSE) for details.

Third-party components keep their own upstream licenses as listed above and in their vendored manifests.

---

<div align="center">

*echOS — because the best way to understand an OS is to build one.*

</div>
