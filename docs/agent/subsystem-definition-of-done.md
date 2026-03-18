# echOS Subsystem Definition of Done

Tarih: 2026-03-12

Bu belge, echOS icinde bir subsystem veya capability'nin ne zaman `Done`,
`Verified`, `Partial` veya `Misleading UX` sayilacagini sabitler.

## Durum Etiketleri

- `Broken`
  - Ana akista calismiyor veya belirgin sekilde yanlis davraniyor.
- `Stubbed`
  - API/export/entrypoint var ama gercek davranis yok.
- `Simulated`
  - Gercek runtime yerine modelleme, fake basari veya synthetic davranis var.
- `Partial`
  - Gercek kod yolu var, ama kapsam veya edge-case'ler yetersiz.
- `Misleading UX`
  - Kullanici yuzeyi kapasiteyi gerceginden daha iyi gosteriyor.
- `Verified`
  - En az tanimlanan kapsamdaki mekanik dogrulama gecerli.
- `Done`
  - Export, davranis, edge-case, test ve kullanici-siniri birlikte kapanmis.

## Genel Done Kurali

Bir is ancak su kosullarda `Done` sayilir:

1. Giris yuzeyi var.
2. Gercek implementasyon var.
3. State degisimi retained veya kalici modelde izlenebilir.
4. Fallback/simulated davranis acikca ayrilmis veya kaldirilmis.
5. En az bir mekanik dogrulama var.
6. Kullaniciya gorunen sinir dokumante edilmis.

## Verified Kurali

Bir is `Verified` sayilabilir ama `Done` olmayabilir.

`Verified` icin asgari:

1. Belirli bir komut, smoke, build veya log sinyali calisiyor.
2. Kapsam siniri acik.
3. Edge-case veya parity eksigi acikca belirtilmis.

## Misleading UX Kurali

Su durumlardan biri varsa `Misleading UX`:

1. UI veya shell "basarili" dili kullaniyor ama backend fallback.
2. Transport hazir ama protocol stack yarim.
3. Export var ama `stub_api` veya benzeri inert yol calisiyor.
4. Heuristic/fake data gercek data gibi sunuluyor.

## Capability Matrix Satir Formati

Her capability satiri su kolonlari tasir:

- `Subsystem`
- `Capability`
- `Status`
- `Code Path`
- `User-Facing Surface`
- `Mechanical Evidence`
- `Boundary`
- `Next Gate`

## Dogrulama Tipleri

- `Build`
- `Targeted unit test`
- `Integration test`
- `QEMU smoke`
- `Serial log marker`
- `Manual visual check`
- `Spec/ABI review`

## Faz 0 Kurali

Faz 0 ancak su durumda kapanir:

1. Capability matrix dosyalari repoda var.
2. Ana shell/CLI yuzeyleri kritik sinirlari acikca soyluyor.
3. `declared`, `implemented`, `verified` ayrimi en az Win32 ve network icin dokumante.
4. TODO/backlog fazlari bu matrixlerle hizali.
