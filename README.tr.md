# echOS-x64

**echOS-x64; Rust `no_std` ile geliştirilen, UEFI, Multiboot2, Limine, SMP zamanlayıcı, bellek yönetimi, dosya sistemleri, ağ yığını ve GUI/compositor araştırmasına odaklanan x86-64 işletim sistemi çekirdeğidir.**

[English README](README.md) · [Teknik rapor](echOS_teknik_rapor.pdf) · [Derleme ve çalıştırma](#derleme)

<div align="center">

```
 ███████╗ ██████╗██╗  ██╗ ██████╗ ███████╗    ██╗  ██╗ ██████╗ ██╗  ██╗
 ██╔════╝██╔════╝██║  ██║██╔═══██╗██╔════╝    ╚██╗██╔╝██╔════╝ ██║  ██║
 █████╗  ██║     ███████║██║   ██║███████╗     ╚███╔╝ ███████╗ ███████║
 ██╔══╝  ██║     ██╔══██║██║   ██║╚════██║     ██╔██╗ ██╔═══██╗╚════██║
 ███████╗╚██████╗██║  ██║╚██████╔╝███████║    ██╔╝ ██╗╚██████╔╝     ██║
 ╚══════╝ ╚═════╝╚═╝  ╚═╝ ╚═════╝ ╚══════╝    ╚═╝  ╚═╝ ╚═════╝      ╚═╝
```

**Rust `no_std` x86-64 işletim sistemi araştırma çekirdeği.**

[![CI: Simics Zero-Tolerance](https://img.shields.io/badge/CI-Simics%20Zero--Tolerance-blueviolet?style=flat-square&logo=github-actions)](/.github/workflows/simics-zero-tolerance.yml)
[![Rust: nightly](https://img.shields.io/badge/Rust-nightly-orange?style=flat-square&logo=rust)](rust-toolchain.toml)
[![Hedef: x86_64-unknown-none](https://img.shields.io/badge/hedef-x86__64--unknown--none-lightgrey?style=flat-square)]()
[![Lisans: AGPL-3.0](https://img.shields.io/badge/lisans-AGPL--3.0-green?style=flat-square)](LICENSE)
[![no_std](https://img.shields.io/badge/no__std-✓-blue?style=flat-square)]()
[![Boot: UEFI](https://img.shields.io/badge/boot-UEFI%20%7C%20Multiboot2%20%7C%20Limine-informational?style=flat-square)]()

</div>

---

## İçindekiler

1. [Genel Bakış](#genel-bakış)
2. [Mimari](#mimari)
3. [Mevcut Durum](#mevcut-durum)
4. [Modül Ağacı](#modül-ağacı)
5. [Derleme](#derleme)
6. [Çalıştırma](#çalıştırma)
7. [CI — Simics Sıfır Tolerans Kapısı](#ci--simics-sıfır-tolerans-kapısı)
8. [Teknik Rapor](#teknik-rapor)
9. [Üçüncü Taraf Bileşenler](#üçüncü-taraf-bileşenler)
10. [Lisans](#lisans)

---

## Genel Bakış

**echOS-x64**, Rust `no_std` ile geliştirilen x86-64 işletim sistemi araştırma çekirdeğidir. Mevcut public repo; boot akışı, çekirdek mimarisi, bellek/zamanlayıcı/sürücü deneyleri, host-side araçlar ve yeniden üretilebilir yerel doğrulama yollarına odaklanır.

Bu README bilinçli olarak konservatiftir: `✅` somut implementasyonu veya repo workflow'u bu ağaçta görünen alanı, `⏳` ise aktif geliştirme, kısmi entegrasyon, hedefe bağlı destek veya daha güçlü doğrulama bekleyen alanı gösterir.

---

## Mimari

```
┌──────────────────────────────────────────────────────────────────────┐
│                       KULLANICI ALANI (gelecek)                      │
│         POSIX API │ Win32 API │ ELF Yükleyici │ PE Yükleyici │ VDSO  │
├──────────────────────────────────────────────────────────────────────┤
│                        SİSTEM ÇAĞRISI ARAYÜZÜ                        │
├────────────┬─────────────┬──────────────┬───────────────────────────┤
│ ZAMANLAYIC │   BELLEK    │  DOSYA SİS.  │          AĞ               │
│  CFS/RT/DL │  PMM + VMM  │ FAT/ext/VFS  │  smoltcp destekli yığın   │
│  SMP/AP    │  Allocator  │ imaj araçları│  protokol deneyleri       │
│  Work-Steal│  paging     │ validasyon   │  paket/cihaz bağlantısı   │
├────────────┴─────────────┴──────────────┴───────────────────────────┤
│                          ÇEKİRDEK KATMAN                             │
│   GDT │ IDT │ APIC │ IOAPIC │ IRQ Domains │ Softirq │ RCU │ Preempt │
├──────────────────────────────────────────────────────────────────────┤
│                          SÜRÜCÜ KATMANI                              │
│  NVMe │ ATA │ VirtIO │ PCI │ USB (HID/CDC/MSD) │ PS/2 │ Ses │ BT   │
├──────────────────────────────────────────────────────────────────────┤
│                         DONANIM (x86-64)                             │
│        UEFI Firmware │ ACPI Tabloları │ TSC │ RDRAND │ AES-NI │ AVX │
└──────────────────────────────────────────────────────────────────────┘
```

**Önyükleme akışı:**

```
UEFI/Multiboot2/Limine
        │
        ▼
  UEFI Girişi (uefi_main)  ──VEYA──  Limine Girişi  ──VEYA──  Multiboot2 Girişi
        │
        ▼
  GOP Framebuffer başlatma  →  Açılış ekranı
        │
        ▼
  ACPI ayrıştırma  →  APIC / IOMMU başlatma  →  SMP AP bringup yolu
        │
        ▼
  PMM + Sayfalama  →  TLSF Heap  →  Güvenlik (SMEP/SMAP/NX)  →  TPM Güvenli Önyükleme
        │
        ▼
  Sürücüler (PCI / NVMe / VirtIO / USB)  →  Dosya sistemi bağlama
        │
        ▼
  Ağ deneyleri  →  GUI/compositor deneyleri  →  kabuk/araçlar
```

---

## Mevcut Durum

| Alan | Durum | Not |
|------|-------|-----|
| Rust `no_std` çekirdek crate'i | ✅ | Ana çekirdek kodu Rust ve bare-metal hedeflere ayrılmış durumda. |
| UEFI build hedefi | ✅ | `x86_64-unknown-uefi` build yolu ve `.efi` artifact'i dokümante edildi. |
| QEMU/OVMF çalıştırma yolu | ✅ | `run_qemu.ps1` yerel smoke-run giriş noktasıdır. |
| Paylaşılabilir UEFI VM ISO | ✅ | `scripts/build_vm_iso.ps1`, `build/appliance/echOS-uefi.iso` üretir. |
| AGPL-3.0 proje lisansı | ✅ | Kök `LICENSE`, manifest metadata ve README rozeti aynı lisansı gösterir. |
| Simics gate araçları | ✅ | Gate script'leri ve log/verdict yolları repo workflow'una dahil. |
| Limine / Multiboot2 yolları | ⏳ | Kodda ve dokümanda var; public çalıştırma yolu olarak UEFI birincil. |
| SMP / AP bring-up | ⏳ | Aktif çekirdek yolu; VirtualBox smoke profili bilinçli olarak tek vCPU. |
| Bellek yönetimi | ⏳ | PMM, paging ve allocator işleri ağaçta var; daha güçlü public proof coverage gerekiyor. |
| Zamanlayıcı | ⏳ | CFS/RT/deadline/work-stealing çalışmaları ağaçta; uçtan uca workload doğrulaması sürüyor. |
| Dosya sistemleri | ⏳ | FAT/ext tarzı/VFS işleri ağaçta; smoke dışı yollar validasyon altında. |
| Ağ | ⏳ | smoltcp destekli çalışma ağaçta; protokol matrisi tamamlanmış gibi sunulmuyor. |
| GUI/compositor | ⏳ | Framebuffer, grafik ve UI deneyleri ağaçta; bitmiş masaüstü ortamı değil. |
| Win32/POSIX/IronShim uyumluluğu | ⏳ | Uyumluluk çalışmaları var; public destek deneysel kabul edilmeli. |
| Donanım sürücü yüzeyi | ⏳ | VirtIO/PCI/depolama/giriş/görüntü işleri aktif; bare-metal donanım kapsamı hedefe göre değişir. |

---

## Modül Ağacı

```
src/
├── main.rs              # Çekirdek giriş noktası (UEFI / Limine / Multiboot2)
├── lib.rs               # Crate kökü, modül bildirimleri
│
├── boot/                # Önyükleme protokol işleyicileri, BootInfo çıkarma
├── acpi/                # ACPI tablo ayrıştırma, AML yorumlayıcı, MADT, GPE
├── apic/                # Local APIC, IO-APIC
├── cpu/                 # SMP AP başlatma, TSC, NUMA, mikrokod, sanallaştırma
├── gdt.rs               # Global Tanımlayıcı Tablosu
├── interrupts/          # IDT, PIC, IRQ chip, IRQ etki alanları, softirq, yeniden eşleme
│
├── memory/              # PMM, sayfalama, TLSF ayrıştırıcı, OOM, THP, zswap
├── allocator/           # Bump, bağlı liste, TLSF, yığın ayrıştırıcıları
│
├── task/                # CFS, RT, Deadline, Ghost zamanlayıcı, SMP iş çalma
├── preempt.rs           # Önalım kontrolü (preempt_disable / enable)
├── rcu.rs               # Read-Copy-Update
├── atomic_ops.rs        # Mimariye özgü atomik işlemler
├── memory_barriers.rs   # SMP bellek engelleri (smp_mb / rmb / wmb)
│
├── fs/                  # FAT32, ext4+günlük, NTFS, F2FS, NFS, FUSE, VFS
├── net/                 # TCP/UDP, TLS 1.3, QUIC, WireGuard, IPSec, HTTP/2
├── drivers/             # NVMe, ATA, VirtIO, PCI, USB, ses, BT, IOMMU
│
├── security/            # SMEP/SMAP, TPM, Güvenli Önyükleme, MAC, seccomp, denetim
├── crypto/              # AES-NI, SHA, Blake3, ChaCha20, Ed25519, Argon2
├── fault/               # Hata enjeksiyonu, monitörler, checkpoint'ler, kurtarma
│
├── gui/                 # Pencere yöneticisi, masaüstü, dock, spotlight, uygulamalar
├── gfx/                 # Tile compositor, SIMD harmanlama, GAL
├── gop/                 # UEFI GOP framebuffer
├── font/                # VGA bitmap font
│
├── ipc/                 # Kanallar, mesajlar
├── tty/                 # TTY katmanı, PTY, ANSI, hat disiplini
├── serial/              # UART hata ayıklama çıktısı
├── shell/               # Etkileşimli kabuk, betikleme, satır editörü
├── syscall.rs           # Sistem çağrısı dağıtıcısı
│
├── posix/               # POSIX uyumluluğu (pipe, msgq, semaphore, dlopen)
├── linux_glue.rs        # Linux çekirdek ABI uyumluluk katmanı
├── elf.rs               # ELF ikili dosya yükleyici
├── pe_loader.rs         # PE/COFF ikili dosya yükleyici
├── win32.rs             # Win32 API öykünmesi
├── ironshim_bridge.rs   # IronShim Windows sürücü köprüsü
├── vdso.rs              # Sanal DSO
├── virt.rs              # VMX/SVM sanallaştırma
└── gpu3d.rs             # 3D GPU API deneyleri
```

---

## Derleme

### Ön Koşullar

- **Rust nightly** — `rust-toolchain.toml` aracılığıyla otomatik yönetilir
- **LLVM / lld** bağlayıcı
- QEMU (yerel test için) veya Intel Simics (CI kapısı için)

```bash
# Rust'ı yükle (gerekiyorsa)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Gerekli hedefleri ekle (araç zinciri dosyası bunu otomatik yapar)
rustup target add x86_64-unknown-none x86_64-unknown-uefi
```

### UEFI Derlemesi (birincil hedef)

```bash
cargo build --target x86_64-unknown-uefi --release
# Çıktı: target/x86_64-unknown-uefi/release/ech_os.efi
```

### Bare-metal Derlemesi (Limine/Multiboot2)

```bash
cargo build --target x86_64-unknown-none --release
# Çıktı: target/x86_64-unknown-none/release/ech_os
```

---

## Çalıştırma

### QEMU (UEFI — OVMF)

```powershell
.\run_qemu.ps1
```

### Paylaşılabilir UEFI VM ISO

```powershell
.\scripts\build_vm_iso.ps1
# Çıktı: build\appliance\echOS-uefi.iso
```

Bu ISO, UEFI/OVMF kullanan VM'lerde optik medya olarak takılır. Legacy BIOS
önyükleme bu artifact'in sözleşmesi değildir; VirtualBox/VMware/QEMU tarafında
EFI/UEFI firmware seçilmelidir.

VirtualBox test profili: `Other/Unknown (64-bit)`, EFI açık, CPU sayısı `1`,
boot sırası disk/optik medya. Mevcut VirtualBox 7.2.x profilinde AP bring-up
ikinci vCPU üzerinde TSS yükleme sırasında triple fault ürettiği için SMP
VirtualBox smoke'ta kapalı tutulur; QEMU/Simics SMP doğrulaması ayrı kapıdır.

Ya da manuel olarak:

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
# Simics GUI'yi başlat
.\run_simics.ps1

# Ya da başsız kapı çalıştırma
Simics\echos-simics\bin\run-gate.bat
```

### Eski Multiboot2 ISO

```bash
# ISO önceden oluşturulmuş:
multiboot_iso/boot/ech_os

qemu-system-x86_64 -cdrom echos.iso -m 512M -serial stdio
```

---

## CI — Simics Sıfır Tolerans Kapısı

`main` / `master` dallarını hedefleyen her çekme isteği, Intel Simics simülatörü üzerinde çalışan **beş eksenli donanım kapısı** tarafından engellenir.

### Kapı eksenleri

| Eksen | Açıklama |
|-------|----------|
| `boot_irq_input` | Temiz UEFI önyükleme, kesme işleme, klavye/fare girişi |
| `syscall_security` | Sistem çağrısı ABI doğruluğu + SMEP/SMAP zorlaması |
| `fs_network` | Dosya sistemi okuma/yazma bütünlüğü + ağ bağlantısı |
| `performance` | Önyükleme süresi, zamanlayıcı gecikmesi, bellek verimi kıyaslamaları |
| `extreme_ironshim` | IronShim Windows sürücü yük testi |

### Kurallar

- **Tek bir FAIL → `çıkış kodu 2` → birleştirme engeli.**
- Kapı günlükleri: `Simics/echos-simics/targets/echos/logs/gate_run_<zaman_damgası>.log`
- Makine tarafından okunabilir karar: `Simics/echos-simics/targets/echos/logs/gate_verdict_<zaman_damgası>.json`

### Manuel kapı çalıştırma

```bat
Simics\echos-simics\bin\run-gate.bat
```

### CI iş akışı

```yaml
# .github/workflows/simics-zero-tolerance.yml
# Runner etiketi: [self-hosted, windows, simics]
```

Sonuçtan bağımsız olarak her kapı çalıştırmasından sonra yapıtlar (günlükler + seri yakalama) yüklenir.

---

## Teknik Rapor

İç tasarım kararlarını, alt sistem mimarisini ve kıyaslamaları kapsayan ayrıntılı teknik rapor şu dosyada mevcuttur:

```
echOS_teknik_rapor.pdf
```

---

## Üçüncü Taraf Bileşenler

| Bileşen | Konum | Lisans |
|---------|-------|--------|
| `virtio-drivers` | `third_party/virtio-drivers` | MIT / Apache-2.0 |
| `core_io` | `third_party/core_io` | MIT |
| `ironshim-rs` | `third_party/ironshim-rs` | gizli |
| `smoltcp` | crates.io | MIT / Apache-2.0 |
| `rcore-fs` | git alt modülü | MIT |

---

## Lisans

echOS-x64 proje kodu **GNU Affero General Public License v3.0 only** kapsamında dağıtılır — ayrıntılar için [`LICENSE`](LICENSE) dosyasına bakınız.

Üçüncü taraf bileşenler yukarıdaki tabloda ve vendored manifestlerde belirtilen kendi upstream lisanslarını korur.

---

<div align="center">

*echOS — çünkü bir işletim sistemini anlamanın en iyi yolu onu inşa etmektir.*

</div>
