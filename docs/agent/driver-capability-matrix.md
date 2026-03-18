# echOS Driver Capability Matrix

Tarih: 2026-03-12

| Driver Area | Capability | Status | Code Path | Mechanical Evidence | Boundary | Next Gate |
|---|---|---|---|---|---|---|
| VirtIO-Net | transport bring-up | Verified | `src/drivers/virtio_net.rs` | QEMU serial log | upper layers ayri | packet exchange smoke |
| VirtIO FFI | truthful degraded C bridge path | Partial | `src/drivers/virtio_ffi.rs` | source audit + targeted host tests | transport gorunur, ama backend yoksa panic/no-op yerine explicit error verir | replace with real queue/DMA backend |
| USB core | xHCI/USB enumeration | Partial | `src/drivers/usb/mod.rs` | build | placeholder physical/slot semantics | real hardware enumeration trace |
| USB CDC | serial control/data | Partial | `src/drivers/usb/cdc.rs` | source audit | control/data completeness yok | CDC loopback |
| USB HID | HID control/setup | Partial | `src/drivers/usb/hid.rs` | source audit | control transfer edge-cases eksik | HID report test |
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
