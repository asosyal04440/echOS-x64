# echOS Secure Boot Local Flow

Bu akış repo tarafında tekrar edilebilir Secure Boot artifact zinciri üretir:

1. PK / KEK / db / dbx bundle üret
2. `ech_os` UEFI binary'sini `db` anahtarıyla imzala
3. AUTH dosyalarını OVMF içinde guest-side authenticated variable akışıyla enroll et
4. Signed `BOOTX64.EFI` ile boot smoke al

Repo içinde bunun için iki aşamalı QEMU harness var:

1. `.\scripts\run_secure_boot_qemu_smoke.ps1 -Phase prepare`
2. Aynı vars-store ile `.\scripts\run_secure_boot_qemu_smoke.ps1 -Phase verify`

## Araçlar

PATH içinde şunlar olmalı:

- `openssl`
- `cert-to-efi-sig-list`
- `sign-efi-sig-list`
- `sbsign`
- `sbverify`

## 1. Secure Boot bundle üret

```powershell
.\scripts\generate_secure_boot_bundle.ps1 -OutputDir .\artifacts\secure_boot
```

Bu komut şunları üretir:

- `PK.{crt,key,esl,auth}`
- `KEK.{crt,key,esl,auth}`
- `db.{crt,key,esl,auth}`
- `dbx.{esl,auth}`
- `dbx-bootstrap.{crt,key}`
- `secure_boot_manifest.json`

Not:

- `dbx` şu an bootstrap revoked cert ile doldurulur; amaç değişkenin var olması ve echOS runtime parser'ının fail-closed kalmasıdır.
- Production dağıtımında gerçek revoke girdileriyle değiştirilmelidir.

## 2. echOS EFI artifact'ını imzala

Hazır unsigned EFI dosyan varsa:

```powershell
.\scripts\sign_uefi_secure_boot.ps1 `
  -UnsignedImage .\path\to\BOOTX64.EFI `
  -SignedImage .\artifacts\secure_boot\BOOTX64.EFI `
  -Certificate .\artifacts\secure_boot\db.crt `
  -PrivateKey .\artifacts\secure_boot\db.key
```

Derleyip hemen imzalamak için:

```powershell
.\scripts\build_signed_uefi.ps1 `
  -BundleDir .\artifacts\secure_boot `
  -Profile release `
  -SignedImage .\artifacts\secure_boot\BOOTX64.EFI
```

## 3. Enroll

### OVMF / QEMU

En güvenli tekrar edilebilir yol:

1. OVMF'i setup mode veya custom-key mode ile aç
2. `.\scripts\run_secure_boot_qemu_smoke.ps1 -Phase prepare -ForceVarsReset` çalıştır
3. QEMU appliance ESP üzerinde şu FAT 8.3 alias'ları hazır gelir:
   - `PK.AUT`  => `PK.auth`
   - `KEK.AUT` => `KEK.auth`
   - `DB.AUT`  => `db.auth`
   - `DBX.AUT` => `dbx.auth`
   - `SBENROLL.ON` => guest-side auto-enroll trigger
4. Prepare boot sırasında echOS şu sırayla authenticated variable yazımı yapar:
   - `PK.AUT`
   - `KEK.AUT`
   - `DB.AUT`
   - `DBX.AUT`
5. Yazım bittiğinde echOS aynı vars-store ile warm reset ister
6. Prepare fazı tamamlandıktan sonra doğrulamayı koş:

```powershell
.\scripts\run_secure_boot_qemu_smoke.ps1 -Phase verify
```

Bu verify komutu:

- secure OVMF code image kullanır
- signed `BOOTX64.EFI` ile appliance diskini yeniden üretir
- mevcut vars-store'u tekrar takar
- serial log içinde şu marker'ları zorlar:
  - `[UEFI] Runtime services verified`
  - `[UEFI] Secure Boot databases available`
  - `[UEFI] Loaded image signature OK`

### Fiziksel makine

Repo tarafı artifact setini üretir; gerçek enroll firmware menüsü, OEM secure boot UI veya platformunda kullandığın update aracı üzerinden yapılmalıdır. Bu adım makine erişimi ve gerçek anahtar sahipliği gerektirir.
Repo tarafı artifact setini üretir. Fiziksel makinede otomatik enroll kullanılmayacaksa gerçek firmware menüsü, OEM secure boot UI veya platformunda kullandığın update aracı üzerinden aynı AUTH payload'ları uygulaman gerekir.

## 4. Doğrulama

Beklenen minimum doğrulama:

1. `sbverify --cert db.crt BOOTX64.EFI`
2. echOS boot sırasında:
   - Secure Boot açık
   - runtime services verified
   - secure boot databases available
   - loaded image signature OK

## Sınır

Bu akış repo-visible Secure Boot zincirini kurar. Şu iki konu hâlâ operasyonel karardır:

- 2023 CA mı yoksa OEM/self-signed zincir mi kullanılacak
- Production `dbx` içine hangi revoke girdilerinin yerleştirileceği
