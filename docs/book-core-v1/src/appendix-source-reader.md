# Ek E - Kaynak Kod Okuma Rehberi

Bu ek, echOS kodunu verimli okumak icin pratik bir metot verir.

## 1) Dosyayi acmadan once sor

- Bu dosyanin ownership siniri ne?
- Hangi alt sistemi besliyor?
- Hangi hata sinifini kapatmak icin yazilmis?

## 2) Fonksiyon okuma sirasi

Bir dosyada su sirayla ilerle:

1. Giris yorumu ve sabitler
2. Veri yapilari
3. Top-level API fonksiyonlari
4. Yardimci/private fonksiyonlar
5. Testler

Bu sirayla okuma, ayrintiya erken bogulmayi azaltir.

## 3) Scheduler dosyalari icin yol

- `scheduler.rs` ile karar agaci
- `rt_scheduler.rs` ile policy farklari
- `cfs.rs` ile adalet modeli
- `eevdf.rs` ile eligibility modeli
- `deadline.rs` ile admission modeli

## 4) Memory dosyalari icin yol

- `fibonacci_pmm.rs` ile zone siniri
- `fibonacci_buddy.rs` ile split/coalesce
- `tlsf.rs` ile heap wrapper guvenligi
- `mod.rs` ile fault/reclaim butun resmi

## 5) Lock-free dosyalar icin kontrol listesi

- Hangi degisken kim tarafindan yaziliyor?
- Publication noktasi nerede?
- Acquire/Release/SeqCst secimi neden oyle?
- Yaris durumu hangi satirda kapatiliyor?

## 6) Parser/state machine dosyalari icin kontrol listesi

- Giris dogrulama var mi?
- Boyut/sayi limitleri fail-closed mu?
- State gecisleri acik enum uzerinden mi?
- Hata kodlari anlamli ayriliyor mu?

## 7) Kendi not formatin

Her dosya icin 6 satirlik not tut:

1. Dosya amaci
2. En kritik fonksiyon
3. En kritik veri yapisi
4. Bir failure mode
5. Bir mitigasyon
6. Bir test fikri

Bu not disiplini, Cilt 2'de hizli ilerlemeni saglar.
