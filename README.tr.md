<div align="center">

```
 ███████╗ ██████╗██╗  ██╗ ██████╗ ███████╗    ██╗  ██╗ ██████╗ ██╗  ██╗
 ██╔════╝██╔════╝██║  ██║██╔═══██╗██╔════╝    ╚██╗██╔╝██╔════╝ ██║  ██║
 █████╗  ██║     ███████║██║   ██║███████╗     ╚███╔╝ ███████╗ ███████║
 ██╔══╝  ██║     ██╔══██║██║   ██║╚════██║     ██╔██╗ ██╔═══██╗╚════██║
 ███████╗╚██████╗██║  ██║╚██████╔╝███████║    ██╔╝ ██╗╚██████╔╝     ██║
 ╚══════╝ ╚═════╝╚═╝  ╚═╝ ╚═════╝ ╚══════╝    ╚═╝  ╚═╝ ╚═════╝      ╚═╝
```

**Tamamen Rust ile yazılmış, modern bir x86-64 bare-metal işletim sistemi.**

[![CI: Simics Zero-Tolerance](https://img.shields.io/badge/CI-Simics%20Zero--Tolerance-blueviolet?style=flat-square&logo=github-actions)](/.github/workflows/simics-zero-tolerance.yml)
[![Rust: nightly](https://img.shields.io/badge/Rust-nightly-orange?style=flat-square&logo=rust)](rust-toolchain.toml)
[![Hedef: x86_64-unknown-none](https://img.shields.io/badge/hedef-x86__64--unknown--none-lightgrey?style=flat-square)]()
[![Lisans: MIT](https://img.shields.io/badge/lisans-MIT-green?style=flat-square)](LICENSE)
[![no_std](https://img.shields.io/badge/no__std-✓-blue?style=flat-square)]()
[![Boot: UEFI](https://img.shields.io/badge/boot-UEFI%20%7C%20Multiboot2%20%7C%20Limine-informational?style=flat-square)]()

> *Doom çalıştırır. TLS 1.3 konuşur. UEFI'den önyüklenir. — Tamamı Rust, sıfır C standart kütüphanesi.*

</div>

---

## İçindekiler

1. [Genel Bakış](#genel-bakış)
2. [Mimari](#mimari)
3. [Öne Çıkan Özellikler](#öne-çıkan-özellikler)
4. [Modül Ağacı](#modül-ağacı)
5. [Derleme](#derleme)
6. [Çalıştırma](#çalıştırma)
7. [CI — Simics Sıfır Tolerans Kapısı](#ci--simics-sıfır-tolerans-kapısı)
8. [Teknik Rapor](#teknik-rapor)
9. [Üçüncü Taraf Bileşenler](#üçüncü-taraf-bileşenler)
10. [Lisans](#lisans)

---

## Genel Bakış

**echOS-x64**, tamamen **Rust** (`#![no_std]`) ile sıfırdan inşa edilmiş, araştırma düzeyinde tam özellikli bir işletim sistemi çekirdeğidir. `x86_64` mimarisini hedefler ve **UEFI**, **Multiboot2** veya **Limine** protokolüyle önyükleme yapar.

Bu proje bir oyuncak çekirdek değildir. CFS/RT/Deadline zamanlayıcı, TLS 1.3 ağ yığını, TPM 2.0 Güvenli Önyükleme, ext4 günlükleme, IronShim Windows sürücü uyumluluğu, tile tabanlı GPU compositor ve POSIX/Win32 API öykünme katmanı gibi üretim kalitesinde alt sistemler içerir — tamamı güvenli + güvensiz Rust ile, harici libc kullanılmadan.

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
│  CFS/RT/DL │  PMM + VMM  │ FAT/ext4/    │  TCP/UDP/TLS1.3/QUIC/     │
│  SMP 8192  │  TLSF/Buddy │ NTFS/f2fs/   │  WireGuard/IPSec/HTTP2    │
│  Work-Steal│  THP/zswap  │ NFS/FUSE     │  DNS-over-HTTPS/DoT       │
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
  ACPI ayrıştırma  →  APIC / IOMMU başlatma  →  SMP AP bringup (8192 CPU'ya kadar)
        │
        ▼
  PMM + Sayfalama  →  TLSF Heap  →  Güvenlik (SMEP/SMAP/NX)  →  TPM Güvenli Önyükleme
        │
        ▼
  Sürücüler (PCI / NVMe / VirtIO / USB)  →  Dosya sistemi bağlama
        │
        ▼
  Ağ yığını  →  GUI compositor  →  Kabuk / Masaüstü
```

---

## Öne Çıkan Özellikler

### 🧠 Bellek Yönetimi
| Özellik | Detay |
|---------|-------|
| Fiziksel Bellek Yöneticisi | Fibonacci Buddy + PMM (O(1) tahsis) |
| Sanal Bellek | 4 seviyeli sayfa tabloları, 2 MiB büyük sayfalar |
| Heap Ayrıştırıcı | **TLSF** (Two-Level Segregated Fit) + Bump + Linked-List yedek |
| Şeffaf Büyük Sayfalar | THP birleştirme arka plan işlemi |
| Bellek Sıkıştırma | `zswap` tarzı sıkıştırılmış takas |
| cgroups v2 | Görev başına bellek sınırı ve muhasebe |
| OOM Killer | Öncelik tabanlı kurban seçimi |
| NUMA | Topoloji farkında bellek tahsisi |

### ⚡ Zamanlayıcı
| Özellik | Detay |
|---------|-------|
| CFS | Sanal çalışma süreli Tam Adil Zamanlayıcı (Linux tarzı) |
| RT Zamanlayıcı | Gerçek zamanlı görevler için SCHED_FIFO / SCHED_RR |
| Deadline Zamanlayıcı | EDF tabanlı (SCHED_DEADLINE) |
| Ghost Zamanlayıcı | Google tarzı çekirdek içi ajan zamanlama |
| SMP | Chase-Lev iş çalma kuyruğu, **8.192 CPU**'ya kadar |
| Zamanlayıcı | TSC tabanlı yüksek çözünürlüklü zamanlayıcı tekerleği |
| Futex | Kullanıcı alanı hızlı yol mutex |
| CPU Benzitimi | NUMA farkında görev sabitleme |

### 🌐 Ağ
| Protokol | Durum |
|----------|-------|
| Ethernet / ARP / IPv4 / IPv6 | ✅ |
| TCP / UDP | ✅ (smoltcp destekli) |
| DHCP | ✅ |
| DNS / DNSSEC | ✅ |
| DNS-over-HTTPS (DoH) | ✅ |
| DNS-over-TLS (DoT) | ✅ |
| **TLS 1.3** (sıfırdan) | ✅ HKDF + ChaCha20 + SHA-2 |
| HTTP/1.1 + HTTP/2 | ✅ |
| WebSocket | ✅ |
| **QUIC** | ✅ |
| **WireGuard** | ✅ |
| IPSec | ✅ |
| Netfilter / iptables tarzı | ✅ |
| Ağ Ad Alanları | ✅ |
| `io_uring` tarzı asenkron G/Ç | ✅ |
| Sıfır-kopya ağ | ✅ |
| x.509 / PKI | ✅ |

### 📁 Dosya Sistemleri
| DS | Özellikler |
|----|-----------|
| **FAT32** | Okuma/yazma |
| **ext4** | Günlükleme (ext4_journal), ACL, xattr, kotalar, inotify |
| **NTFS** | Okuma desteği |
| **F2FS** | Flash dostu dosya sistemi |
| **NFS** | Ağ dosya sistemi istemcisi |
| **FUSE** | Kullanıcı alanı dosya sistemi protokolü |
| Dosya kilitleme | POSIX danışma + zorunlu kilitleme |
| Sıfır-kopya splice | `sendfile` tarzı |

### 🔒 Güvenlik
| Özellik | Detay |
|---------|-------|
| SMEP / SMAP | CR4 donanım zorlaması |
| NX / DEP | W^X sayfa tablosu politikası |
| Yığın Canary | Görev başına canary değerleri |
| ASLR | Rastgele çekirdek & kullanıcı sanal adres düzeni |
| **TPM 2.0** | PCR genişletme, ölçümlü önyükleme |
| **UEFI Güvenli Önyükleme** | PK/KEK/db/dbx güven zinciri |
| Kapasite Tabanlı Güvenlik | POSIX yetenekleri |
| MAC (SELinux benzeri) | Zorunlu Erişim Kontrolü çerçevesi |
| seccomp | Sistem çağrısı filtre politikaları |
| IMA / EVM | Bütünlük Ölçüm Mimarisi |
| Denetim | Çekirdek denetim günlüğü |
| Anahtar Halkası | Çekirdek içi anahtar deposu |

### 🔐 Kriptografi (donanım hızlandırmalı, no_std)
- **AES-NI** — donanım AES-128/256
- **SHA-256 / SHA-3** — SHA-NI hızlandırmalı
- **Blake3** — hızlı özetleme
- **ChaCha20-Poly1305** — AEAD şifresi
- **Ed25519** — dijital imzalar
- **RSA** — asimetrik kriptografi
- **Argon2** — parola özetleme
- **HKDF** — TLS 1.3 anahtar türetme

### 🎮 GUI ve Grafik
- **Tile tabanlı compositor** — SIMD hızlandırmalı harmanlama
- **VirtIO GPU** + **DRM** arka ucu
- Tam **Pencere Yöneticisi** (`echOS-WM`): pencereler, odak, z-sıralaması
- **Masaüstü**, Dock, Spotlight, Mission Control, Spaces (sanal masaüstleri)
- **Font rendering** (TrueType rasterizör, glyph atlası, metin düzeni)
- Dahili uygulamalar: Terminal, Dosya Gezgini, Metin Editörü, Resim Görüntüleyici, Tarayıcı, Müzik Çalar, Etkinlik Monitörü, Ayarlar
- Sürükle-bırak, pano, bildirimler, duvar kağıdı

### 🪟 Uyumluluk Katmanları
| Katman | Detay |
|--------|-------|
| **Win32 API** | Windows uygulamaları için öykünme katmanı |
| **IronShim** | Windows çekirdek sürücü uyumluluğu (ironshim-rs) |
| **POSIX** | `pipe`, `msgq`, `semaphore`, `dlopen` |
| **Linux Glue** | Kısmi Linux çekirdek ABI uyumluluğu |
| **ELF Yükleyici** | Linux ELF ikili dosyaları çalıştırma |
| **PE/COFF Yükleyici** | Windows PE çalıştırılabilir dosyaları yükleme |
| **VDSO** | Hızlı sistem çağrıları için Sanal DSO |

### 🛠 Donanım Sürücüleri
- **Depolama**: NVMe, ATA/AHCI, VirtIO-blk
- **Ağ**: VirtIO-net, smoltcp NIC sürücüsü
- **Görüntü**: VirtIO-GPU, VirGL (3D), framebuffer DRM
- **Giriş**: PS/2 klavye+fare, USB HID
- **USB**: EHCI/XHCI, HID, CDC, Yığın Depolama, Hub
- **Veri Yolu**: PCI/PCIe (ECAM), I²C, SPI
- **Ses**: AC97/HDA çerçevesi
- **Bluetooth**: HCI taşıma katmanı
- **Termal**: ACPI ısıl bölgeleri
- **Watchdog**: Donanım izleme zamanlayıcısı
- **IOMMU**: VT-d / AMD-Vi

### 🧩 ACPI
- Tam ACPI tablo ayrıştırma (MADT, FADT, DSDT, SSDT)
- AML yorumlayıcısı
- GPE (Genel Amaçlı Olaylar)
- ACPI güç durumları (S0–S5)
- Gömülü Denetleyici (EC) protokolü

### 💀 Hata Toleransı
- **Checkpoint & kurtarma** — çekirdek durum anlık görüntüleri
- **Hata enjeksiyonu** — alt sistem dayanıklılık testleri
- **Monitörler** — alt sistem başına sağlık monitörleri (CPU, bellek, zamanlayıcı, IRQ, DS, SMP, sürücü)
- **Bozulma modları** — zarif hizmet bozulması
- **Acil durum işleyicileri** — son çare kilitlenme kurtarma
- **Watchdog** — donanım + yazılım izleme

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
├── gpu3d.rs             # 3D GPU API'si (Vulkan benzeri)
│
├── doom.rs              # 🎮 Doom portu
└── doom_launcher.rs     # Doom başlatıcı
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

### Multiboot2 ISO

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

Bu proje **MIT Lisansı** kapsamında dağıtılmaktadır — ayrıntılar için [`LICENSE`](LICENSE) dosyasına bakınız.

Belirli alt sistemler (IronShim, Simics kapı iç kısımları) **gizli** olmaya devam etmekte ve bu genel depoya dahil edilmemektedir.

---

<div align="center">

*echOS — çünkü bir işletim sistemini anlamanın en iyi yolu onu inşa etmektir.*

</div>
