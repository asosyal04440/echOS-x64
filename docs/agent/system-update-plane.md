# echOS System Update Plane

Bu belge, "muhendislik PC -> update sunucusu -> echOS test makinesi" zincirini ISO yeniden basmadan calistiracak canonical update tasarimidir.

## Taarruz Plani

1. Ownership boundary: engineering makinesi yalnizca imzali artifact uretir; test makinesi yalnizca manifest tabanli installer ve boot-control state machine calistirir.
2. Queue/model: updater tek global "download-and-apply" mutex yerine typed session + staged operation dizisi kullanir; live app bundle lane ile reboot-gerektiren slot lane ayni plan icinde birlikte var olabilir.
3. Hardware map: inactive sistem slotuna yazilan payload `boot::appliance` A/B state'i ile arm edilir; paylasilan `/apps` store yalnizca live-safe package lane icin kullanilir.
4. Cache/contention: installer veri yolu tek-writer publication modelinde kalir; indirilen artifact'lar hash/digest dogrulandiktan sonra immutable staging alanina alinip sonra atomik state flip yapilir.
5. Validation: signed update-index, artifact digest, staged boot success, rollback reason ve package trust gate'leri ayni installer contract'inin zorunlu parcasi olur.

## Neden Iki Katman

echOS icin "guncelleme" tek bir sey degil:

- User/package lane:
  - `.bhd` bundle veya curated seed catalog guncellemeleri
  - aktif sistem yeniden baslamadan kurulabilir
  - user/service bundle'lari `/apps/.bundles` ve `/apps/.content` lane'ine yazar
  - seed catalog artifact'i `/seed/apps` altina persist edilir ve runtime catalog aninda refresh edilir

- Platform lane:
  - kernel, boot payload ve slot rollback ile ayni kaderi paylasmasi gereken sistem image payload'i
  - inactive slot'a stage edilir
  - `boot::appliance::begin_update(...)` ile reboot sonrasi aktif edilir
  - boot health fail ederse rollback olur

- Reboot-gerektiren service lane:
  - reboot isteyen service bundle aktif slot root'una yazilmaz
  - artifact bytes inactive slot'un `/config/update/slot-stage/...` alanina dogrudan stage edilir
  - yeni slot ilk kez ayaga kalktiginda runtime staged journal'i uygular
  - staged service install basarisizsa `AppBasketReady` commit edilmez ve A/B rollback authority korunur

Bu ayrim olmadan ya her seyi gereksiz yere reboot ettirirsin ya da kernel/sistem-servisi degisikliklerini paylasilan live store'a yazarak rollback'i bozarsin.

## Canonical Akis

1. Engineering PC
   - `echsdk` veya host publisher araclari yeni artifact'lari uretir.
   - Her artifact icin:
     - artifact id
     - version
     - digest
     - artifact class
     - reboot gereksinimi
     - package id veya target slot metadata
   - Hepsi tek signed `update-index` altinda yayinlanir.

2. Update server
   - immutable artifact blob store
   - signed `update-index`
   - channel bazli yayin (`stable`, `preview`, `engineering`)
   - delta retention + rollback icin en az bir onceki platform jenerasyonu

3. echOS test makinesi
   - `UpdateInstaller` privileged service `update-index`i indirir veya local store yolundan okur
   - index imzasi ve artifact digest'leri dogrulanir
   - `src/update.rs::plan_update(...)` benzeri saf planlayici artifact setini iki lane'e ayirir:
     - live install
     - inactive-slot staging
   - installer bu plana gore uygular

4. Installer apply phase
   - Revocation feed varsa once fail-closed rotate edilir
   - live-safe user/service bundles store lane'ine kurulur
   - curated seed bundle `/seed/apps` altina persist edilir ve catalog refresh edilir
   - platform image inactive slot'a ham sektor yazimi ile stage edilir
   - reboot-gerektiren service bundle'lar inactive slot F2FS root'unda `/config/update/slot-stage/...` altina journal ile stage edilir
   - boot-control pending slot arm edilir
   - reboot istenir

5. First boot after update
   - `boot::appliance` pending slot'u aktif eder
   - desktop runtime staged slot journal'ini commit eder
   - stage publication `LoaderEntry -> ... -> AppBasketReady` ilerler
   - boot success gorulurse pending slot commit olur
   - panic/attempt exhaustion durumunda stable slot'a rollback olur

## Engineering Constraints

- Update index imzasi package imzasindan ayri tutulmali.
  - package signer compromise ile platform rollout yetkisi ayni olmamali
- User bundle lane ve platform lane ayni digest namespace'i paylasabilir ama ayni apply surface'i paylasmamalidir.
- Reboot-gerektiren service bundle'lar icin rollback authority, installin eski slot yerine yeni slot root'unda yapilmasi ve `mark_boot_success()` oncesi commit edilmesi ile korunur.
- Installer typed state makinesi olmadan "indirirken kur" modeli fail-closed davranmaz; kismi indirme ve yarim kurulum riskli kalir.

## Minimum Repo Programi

1. `src/update.rs`
   - update artifact classifier
   - live vs inactive-slot planlayici
   - conflict detection

2. `CS-1` extension
   - package registry yanina `UpdateInstaller` privileged service eklendi
   - shell `pkg update inspect|apply|status` yalnizca bu servis uzerinden konusur

3. Host publisher
   - `echsdk update publish <spec> <signed-index>`
   - `echsdk update inspect <signed-index>`

4. Target installer
   - signed index inspect/apply
   - `/config/update/last-report.txt` state yayini
   - platform image stage notu
   - live revocation rotate + package install + seed refresh
   - target-slot staged service journal + first-boot commit

## Hard Boundary

Bu belge canli delta-patching veya hot kernel text swap iddia etmiyor. Mevcut repo gerceginde dogru ilk urun:

- app/package updates: live
- platform image updates: staged reboot with A/B slot
- reboot-gerektirmeyen service bundle updates: live
- reboot-gerektiren service bundle updates: target-slot staged journal + first-boot commit
- rollback authority: `boot::appliance`

Kernel live patching ancak sembol versiyonlama, text double-buffering, module ABI ve recovery corpus kapandiginda ayri bir program olarak acilmali.
