# echOS Single-PC Admission Profile

Tarih: 2026-04-23

Bu belge, "echOS'u denek PC'ye tek ve mutlak OS olarak kur" hedefi icin
genis donanim iddiasi yerine tek bir admission profile tanimlar.

## Taarruz Plani

1. Ownership boundary: release/admission profile yalnizca tek fiziksel makine sinifini kabul eder; profile girmeyen cihazlar parity blocker degil, explicit kapsam disidir.
2. Queue/model: input, display, storage ve radio yuzeyi "gerekli" ve "opsiyonel/kapsam disi" olarak ayrilir; unsupported lane sessiz fallback yerine fail-closed raporlanir.
3. Hardware map: firmware = UEFI, boot storage = NVMe, display = GOP, input = PS/2 keyboard fallback, network = wired Ethernet.
4. Cache/contention: parity tartismasi genis vendor matrisi yerine tek machine contract ustune kurulur; exactness enerjisi daraltılmis cihaz yoluna harcanir.
5. Validation: target makine once capture edilir, sonra bu profile karsi mekanik olarak validate edilir; profile uymayan sistem "field candidate" sayilmaz.

## Canonical Profile

Admission profile adi: `single-pc-uefi-nvme-gop-ps2-wired`

Zorunlu:

- Firmware: UEFI
- Boot disk: NVMe
- Display bring-up: UEFI GOP ile acilis
- Primary keyboard path: PS/2 keyboard fallback mevcut olmali
- Network bring-up: wired Ethernet

Bilincli olarak admission profile disi:

- USB keyboard/mouse zorunlulugu
- Native DRM atomic modeset exactness
- Audio playback
- WiFi
- Bluetooth

## Neden Bu Daraltma

Bu profile gecmeden su iddialar dogru kurulamaz:

- "USB/input exactness kapandi"
- "GPU/display metal ustunde gunluk kullanima hazir"
- "Audio/WiFi/Bluetooth parity tamam"

Cunku bunlar tek bir makine kontratina baglanmadan genis cihaz evreni icin
dogru veya yanlis diye raporlanamaz. Admission profile bu belirsizligi keser.

## Ilk 5 Blokerin Yeni Yorumu

1. Gercek donanim profili daraltilmamisti.
   - Bu belge ve validator ile kapanir.
2. USB/input exactness acikti.
   - Admission path'te USB input zorunlu degil; PS/2 keyboard fallback zorunlu.
3. NVMe/AHCI/PCIe/MSI/IOMMU exactness acikti.
   - Admission path yalniz NVMe boot'e daraltilir; AHCI tek-OS gate icinde degil.
   - NVMe exactness halen aktif engineering isi olmaya devam eder.
4. GPU/DRM/display gunluk kullanilabilirlik acikti.
   - Admission path GOP scanout ile acilir; native DRM parity bu gate'in disindadir.
5. Audio/WiFi/Bluetooth partial idi.
   - Admission path'te kapsam disi ilan edilir; tek-OS gate'i bunlara baglanmaz.

## Mekanik Validation

1. `.\scripts\capture_physical_profile.ps1 -OutputPath artifacts\field\candidate.json`
2. `.\scripts\validate_physical_profile.ps1 -ProfilePath artifacts\field\candidate.json`

Validator su kosullari fail-closed zorlar:

- UEFI firmware
- en az bir NVMe denetleyici
- en az bir wired NIC
- en az bir PS/2 keyboard aygiti

Validator su yuzeyleri advisory olarak raporlar:

- USB xHCI denetleyicileri
- display adapter listesi
- audio / WiFi / Bluetooth varligi

## Hard Boundary

Bu belge USB, native DRM, audio, WiFi veya Bluetooth parity'sinin
tamamlandigini iddia etmez. Sadece single-PC install gate'ini daraltir.
Bu daraltma olmadan "ilk 5 gorev" genel donanim evreni icin kapatilmis
sayilamaz.
