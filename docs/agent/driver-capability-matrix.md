# echOS Driver Capability Matrix

Tarih: 2026-03-12

## Single-PC Admission Narrowing

2026-04-23 itibariyla "tek ve mutlak OS" gate'i genis PC evreni icin degil,
`single-pc-uefi-nvme-gop-ps2-wired` admission profile'i icin okunmalidir.

Bu profile gore:

- UEFI + NVMe + GOP + PS/2 keyboard fallback + wired Ethernet zorunludur.
- USB input exactness, native DRM atomic modeset, audio, WiFi ve Bluetooth
  single-PC admission gate'inin zorunlu parcasi degildir.
- Bu yuzeyler parity isi olarak acik kalir; fakat target makine profile
  uyuyorsa tek-PC install blocker'i sayilmaz.

Mekanik gate:

1. `echsdk field-profile capture`
2. `echsdk field-profile validate`

Bu gate gecmeden "ilk 5 gorev bitti" denemez.

| Driver Area | Capability | Status | Code Path | Mechanical Evidence | Boundary | Next Gate |
|---|---|---|---|---|---|---|
| VirtIO-Net | transport bring-up | Verified | `src/drivers/virtio_net.rs` | QEMU serial log | upper layers ayri | packet exchange smoke |
| VirtIO FFI | truthful degraded C bridge path | Partial | `src/drivers/virtio_ffi.rs` | source audit + targeted host tests | transport gorunur, ama backend yoksa panic/no-op yerine explicit error verir | replace with real queue/DMA backend |
| USB core | xHCI/USB enumeration | Partial | `src/drivers/usb/mod.rs` | build + controller-slot/control-transfer/interrupt tests | DMA pointers now fail-closed through physical translation; TRB/ERST layouts are DMA-aligned; command TRBs publish into the controller-owned command ring; `Configure Endpoint` now publishes endpoint contexts and per-endpoint rings; transfer events correlate completed interrupt-IN buffers back to the owning slot/endpoint | real hardware enumeration trace |
| USB CDC | serial control/data | Partial | `src/drivers/usb/cdc.rs` | source audit | control/data completeness yok | CDC loopback |
| USB HID | HID boot keyboard/mouse lane | Partial | `src/drivers/usb/hid.rs` | build + interrupt report consumption test | boot-protocol keyboard/mouse path now drives `SET_PROTOCOL`, `SET_IDLE`, LED `SET_REPORT`, and interrupt-IN polling through the USB core; non-boot HID report-descriptor edge cases remain | real HID device trace |
| USB MSC | mass storage | Partial | `src/drivers/usb/mass_storage.rs` | source audit | BOT path completeness eksik | real media transfer |
| Native NIC | feature completeness | Partial | `src/drivers/nic_native.rs` | build | promiscuous/feature gaps | NIC feature suite |
| AHCI | identify/storage metadata | Partial | `src/drivers/ahci.rs` | build | identify completeness eksik | AHCI disk probe suite |
| NVMe | queue/reset/namespace behavior | Partial | `src/drivers/nvme.rs` | build + host smoke references | queue timeout/reset/admin-vs-IO semantics exact degil | NVMe queue/reset suite |
| GPU / DRM | scanout/fence/atomic commit | Partial | `src/drivers/drm.rs`, `src/drivers/gpu_native.rs`, `src/drivers/virtio_gpu.rs` | build + source audit | fence/present/atomic state publication long-tail acik | atomic modeset + fence suite |
| PCIe / MSI | capability walk / interrupt delivery | Partial | `src/drivers/pci.rs`, `src/drivers/pci_root.rs` | source audit | BAR/capability/MSI-X behavior exact degil | PCIe capability + MSI smoke |
| IOMMU | DMA translation / invalidate / PASID | Partial | `src/drivers/iommu.rs` | build + host smoke references | invalidate/PASID/SVA/ATS/PRI semantics exact degil | IOMMU map/invalidate corpus |
| Audio | HDA codec / DMA playback | Partial | `src/drivers/audio.rs` | build | codec path and DMA runtime fidelity eksik | HDA playback suite |
| WiFi jail | discovery / association / data path | Partial | `src/drivers/wifi_jail.rs` | source audit | jailed runtime and packet path exact degil | associate/send-recv smoke |
| Bluetooth jail | pairing / transport / recovery | Partial | `src/drivers/bluetooth.rs` | source audit | pairing/link/runtime fidelity eksik | bluetooth pair/data smoke |
| Linux driver bridge | source/runtime onboarding via IronShim/eLS | Partial | `src/drivers/dispatcher.rs`, `src/ironshim_bridge.rs`, `src/shim_layer.rs` | source audit | supported driver profili ile gercek bind/load/run boundary'si tam kapanmadi | Linux driver lifecycle suite |
| Driver recovery | degraded fallback | Partial | `src/fault/recovery_modules/driver.rs` | source audit | stub/null fallback, real restart protocol degil | recovery scenario tests |

## Exactness Exit Criteria

Faz 5'in `tam uyumlu/exact` sayilmasi icin su kapilarin kapanmasi gerekir:

1. VirtIO / DMA / probe fidelity
   - `src/drivers/virtio_ffi.rs` gercek queue/DMA/completion backend'ine baglanir
   - probe ve fallback akisi "transport visible" ile "I/O complete" ayrimini korur
2. USB transport exactness
   - xHCI enumeration, control transfer ve CDC/HID/MSC command akislarinda placeholder physical semantics kalmaz
   - controller-slot ownership append-order yerine controller+port determinism ile yayinlanir
   - EP0/control path slot-id vs usb-address ayrimini korur; setup/data/status TRB'leri gercek slot ring ownership'ine yazilir
   - enumeration, loopback ve media-transfer smoke mevcuttur
3. Native hardware behavior
   - NIC, AHCI ve NVMe behavior matrix'i; PCIe/MSI/IOMMU fabric; GPU/DRM publication; ve recovery/hotplug davranisi mekanik corpus ile sinanir
4. Jail-class device fidelity
   - audio / wifi / bluetooth jail yollarinda discovery, runtime state, recovery ve degraded fallback exact contract'a baglanir
   - jail isolation modeli "gorunuyor" seviyesinde degil gercek restart/isolation behavior'iyle sinanir
5. Linux driver onboarding fidelity
   - `src/drivers/dispatcher.rs`, `src/ironshim_bridge.rs`, `src/shim_layer.rs` echOS'un ilan ettigi Linux driver profillerini gercek bind/load/run lifecycle'i ile tasir
   - supported/unsupported boundary'si shell, matrix ve runtime policy gate'te ayni gercegi raporlar
 - driver capability matrix'te behavior-critical `Stubbed` veya `Partial` satirlar exactness iddiasi tasiyan alanlarda kapanir

Bu kosullar kapanmadan Faz 5 yalnizca `truthful/partial`, exact degil.

## First-Five Status After Narrowing

1. Hardware profile selection
   - repo-visible admission profile ve validator ile kapali
2. USB/input exactness
   - repo-visible blocker kapandi: xHCI DMA pointer publication, slot ownership, command submission, endpoint publication, HID class-control, interrupt-IN queueing ve completion-to-buffer korelasyonu artik ayni truthful lane'de
   - kalan is saha kaniti: gercek klavye/fare trace, warm-reset/re-enumeration ve metal smoke
3. NVMe/AHCI/PCIe/MSI/IOMMU exactness
   - acik; single-PC gate icinde NVMe exactness halen aktif blocker
4. GPU/DRM/display daily usability
   - genel parity acik; single-PC gate GOP boot path ile native DRM'e bagli degil
5. Audio/WiFi/Bluetooth
   - genel parity acik; single-PC gate icin kapsam disi
