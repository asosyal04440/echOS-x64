# EGO TODO

EGO, `echOS Game Optimizer` icin yol haritasi dosyasidir.

## Ana Hukum

EGO'nun ilk hedefi oyunlari otomatik native'e cevirmek degil; oyun/runtime
davranisini gorunur hale getiren, compatibility aciklarini siralayan ve ancak
kanitlanmis hot path'lerde geri alinabilir hizlandirma uygulayan echOS'a ozgu
bir truth engine olmaktir.

Kisa ilke:

```text
Observe first. Specialize later. Deopt always.
```

## Fazlar

### EGO v0 - Observe Only

Hedef: Oyun veya PE test binary davranisini hic degistirmeden izlemek.

- [ ] PE/runtime cagri trace ring buffer tasarla.
- [ ] DLL import/load graph olaylarini kaydet.
- [ ] File/path lookup ve read hotset olaylarini kaydet.
- [ ] Timer, sleep, wait ve frame-loop davranisini kaydet.
- [ ] Thread create/wait/signal olaylarindan ilk thread role siniflandirmasini cikar.
- [ ] Window/surface/present olaylarini kaydet.
- [ ] Trace ciktilarini oyun kimligi altinda capsule dizinine yaz.
- [ ] Trace replay/diff icin ilk host tool taslagini yaz.

Acceptance:

- [ ] Bir test PE'si calisirken trace uretir.
- [ ] Trace hicbir runtime davranisini degistirmez.
- [ ] Ayni binary ikinci kez calistiginda onceki trace ile fark raporu uretilir.

### EGO v1 - Suggest Only

Hedef: EGO sadece oneriler uretir; otomatik optimizasyon yapmaz.

- [ ] DLL graph pre-resolve adaylarini raporla.
- [ ] File hotset read-ahead adaylarini raporla.
- [ ] Memory arena pre-layout adaylarini raporla.
- [ ] Timer fast-path adaylarini raporla.
- [ ] Thread scheduling hint adaylarini raporla.
- [ ] Shader/pipeline cache prewarm adaylarini raporla.

Acceptance:

- [ ] Oneriler semantic degisim yapmadan raporlanir.
- [ ] Her oneri bir kanit satirina baglanir: olay sayisi, tekrar orani, hash veya zaman metriği.
- [ ] Oneri raporu yanlis pozitifleri elle isaretleyebilecek formatta olur.

### EGO v2 - Guarded Fast Paths

Hedef: Yalnizca geri alinabilir ve olculmus hizlandirmalar uygulanir.

- [ ] DLL pre-resolve icin versiyon/hash guard ekle.
- [ ] File hotset read-ahead icin path + size + mtime/hash guard ekle.
- [ ] Memory precommit icin strict bounds guard ekle.
- [ ] Pipeline cache prewarm icin shader hash guard ekle.
- [ ] Deopt guard: mismatch durumunda compatibility slow path'e don.

Acceptance:

- [ ] Her fast path tek bayrakla kapatilabilir.
- [ ] Her fast path capsule versiyonuna baglidir.
- [ ] Mismatch durumunda crash yerine slow path'e donulur.
- [ ] Run 2, Run 1'e gore olculebilir launch-time veya jitter iyilesmesi gosterir.

## Game Capsule Sistemi

Capsule, EGO'nun ogrendigi davranisi guvenli, versiyonlu ve denetlenebilir
sekilde saklayan oyun profili olacaktir.

Ilk dosya fikri:

```text
game-capsules/<game-id>/
  capsule.toml
  imports.graph
  dll-load.graph
  file-hotset.graph
  registry.snapshot
  memory-layout.plan
  thread-classes.plan
  framegraph.plan
  shader-cache.index
  deopt-rules.toml
  benchmark-history.json
```

Ilk kurallar:

- [ ] Capsule input olarak untrusted kabul edilecek.
- [ ] Capsule imzali veya en azindan hashlenmis olacak.
- [ ] Capsule binary hash / version / build-id ile eslesmeden uygulanmayacak.
- [ ] Capsule optimizasyonlari semantic truth yerine sadece hint sayilacak.

## Ilk Demo: Forged PE Gamelet

Minimum test oyunu:

- [ ] PE/COFF executable olarak paketlenir.
- [ ] Minimal DLL importlari yapar.
- [ ] Pencere acar.
- [ ] Event loop dondurur.
- [ ] Asset dosyasi okur.
- [ ] Timer kullanir.
- [ ] Basit surface/frame loop davranisi uretir.

Acceptance:

- [ ] Run 1: baseline trace uretilir.
- [ ] Run 2: trace fark raporu uretilir.
- [ ] Run 3: yalnizca observe/suggest modunda ayni davranis korunur.

## Kirmizi Cizgiler

- [ ] EGO temel PE/Win32 uyumlulugunun yerine gecmeyecek.
- [ ] EGO v0 ve v1 runtime davranisini degistirmeyecek.
- [ ] Eski capsule yeni binary'ye uygulanmayacak.
- [ ] Fast path'ler semantik olarak esdegerlik guard'i olmadan acilmayacak.
- [ ] Anti-cheat, DRM ve launcher hedefleri ilk fazlara sokulmayacak.
- [ ] Proton, Wine, Mesa veya DXVK cekirdek omurga olarak gomulmeyecek.

## Acik Sorular

- [ ] EGO trace formatini binary mi, JSON/TOML + binary blob hibriti mi yapacagiz?
- [ ] Capsule storage runtime package store icinde mi, ayri `/game-capsules` kokunde mi duracak?
- [ ] Ilk PE gamelet repo icinde mi yazilacak, yoksa host tool ile uretilecek test fixture mi olacak?
- [ ] Trace ring buffer kernel tarafinda mi, runtime service tarafinda mi tutulacak?
- [ ] EGO hangi noktada graphics stack'e baglanacak: surface olaylari mi, Vulkan command intent mi?

## Ilk Sprint Backlog

- [ ] `EGO-001`: Trace event semasini tanimla.
- [ ] `EGO-002`: Capsule dizin ve metadata formatini tanimla.
- [ ] `EGO-003`: PE loader/runtime cagri noktalarina observe-only hook tasarimi cikar.
- [ ] `EGO-004`: File hotset trace olaylarini runtime store/path katmanina map et.
- [ ] `EGO-005`: Timer/thread/window olaylari icin minimum event setini belirle.
- [ ] `EGO-006`: Forged PE Gamelet gereksinimlerini netlestir.
- [ ] `EGO-007`: Deopt kurallarinin ilk prensiplerini yaz.
- [ ] `EGO-008`: EGO kanit siniflarini belirle: host-model, QEMU, hardware, compile-only.
