# echOS Cilt 1 - Kernel Internals

Bu klasor, echOS'un core kernel internals cildi icin kaynaklari ve build hattini icerir.

## Icerik modu

Varsayilan kitap modu `internals` olarak ayarlidir.
Bu modda cilt, hafta etiketi tasimayan, konu-merkezli bir cekirdek metni ureterek
direct internal engineering anlatimi verir.

Ana kaynak:

- `src/cilt1-kernel-internals.md`

Yardimci kaynaklar:

- `src/00-onsoz.md`
- `src/cilt1-core-deep-dive.md`
- `src/cilt1-algoritma-gorsel-rehber.md`
- `src/cilt1-sozluk.md`
- `src/appendix-algorithm-atlas.md`
- `src/appendix-code-index.md`
- `src/appendix-source-reader.md`
- `src/appendix-math-notes.md`
- `src/appendix-kaynaklar.md`

Legacy kaynaklar repoda tutulur ancak varsayilan build'e dahil edilmez:

- `src/cilt1-core-monograf-expanded.md`
- `src/cilt1-core-api-katalogu.md`
- `src/cilt1-vaka-ve-cozumler.md`
- `src/cilt1-ders-plani-ve-haftalik-uygulama.md`
- `src/cilt1-soru-bankasi-1000.md`
- `src/cilt1-lab-kilavuzu.md`
- `src/cilt1-soru-bankasi.md`
- `src/cilt1-kritik-kontrol-listesi.md`

Not: internals modunda hard-core lane icin su genisletilmis metinler build'e dahildir:

- `src/cilt1-core-monograf-expanded.md` (mini soru bloklari temizlenmis)
- `src/cilt1-kod-ve-matematik-derinlesme.md`
- `src/cilt1-kod-matematik-atlas-v2.md`
- `src/cilt1-core-sembol-dokumu.md`

`cilt1-core-ariza-atlasi.md`, `cilt1-core-api-katalogu.md` ve `cilt1-core-denetim-korpusu.md` repoda tutulur ancak
default internals build'e dahil edilmez.

## Araclar

- `scripts/render-diagrams.ps1`: Mermaid -> SVG
- `scripts/build_pdf.js`: Pandoc -> HTML -> Puppeteer PDF
- `scripts/build.ps1`: build giris scripti + internals terminology gate
- `scripts/ai_risk_scan.py`: PDF sayfa-bazli AI risk taramasi (limit: %20)

## Build

PowerShell:

```powershell
.\docs\book-core-v1\scripts\build.ps1 -Mode internals
```

NPM:

```powershell
npm run build
```

Legacy metinle build:

```powershell
npm run build:legacy
```

## Internals terminology gate

`-Mode internals` ile build calisirken `scripts/build.ps1`, uretilen HTML ciktiyi
tarar ve su ifadelerden biri gorunurse build'i fail eder:

- `hafta`
- `haftalik`
- `week` / `weekly`

Amac: konu-merkezli internals anlatiminda hafta-bazli framing'in geri sizmasini
otomatik engellemek.

## AI risk kapisi

Varsayilan build sonunda `ai_risk_scan.py` calisir ve su dosyalari uretir:

- `out/ai-risk-report.json`
- `out/ai-risk-report.txt`

Kural:

- Herhangi bir sayfa riski `%20` ustundeyse build fail olur.

Elle tarama:

```powershell
npm run ai-scan
```
