# Onsoz

Bu kitap, echOS'un cekirdek muhendislik kararlarini ogrenciye "sifirdan" anlatarak,
kurs notu degil, referans seviye bir kaynak olma hedefiyle yazildi.

## Neden bu kadar buyuk bir cilt?

Bu ciltin hedefi ozet degil, derinliktir. Lisans seviyesinde baslayip,
ileri seviye cekirdek tartismalarina kadar uzanan bir yol sunuyoruz.

Kisa ozet kitaplar genelde su sorulari bos birakir:

- Bu algoritma neden secildi?
- Nerede patlar?
- Uretim ortaminda nasil korunur?

Bu kitapta her bolum bu uc soruya dogrudan cevap verir.

## Kimler icin?

- Isletim sistemi dersini alan lisans ogrencileri
- Sistem programlama ogrenmek isteyen yeni mezunlar
- Bare-metal Rust alanina gecmek isteyen gelistiriciler
- echOS'a katki vermek isteyen topluluk uyeleri

## Okuma stratejisi

Bu cilt lineer okunabilir. Ama asagidaki strateji daha verimlidir:

1. Konu bolumunu oku
2. Kod referansini acip satir satir izle
3. Invariant denetim listesini cikar
4. Ariza/mitigasyon notunu ayni bolume isle

## Dili neden sade tuttuk?

Sade dil, teknik derinligin dusmani degildir.
Sade anlatim, derin teknik ayrintiyi daha fazla kisinin kullanabilir hale getirir.

Bu nedenle kitapta iki katman var:

- Katman 1: sezgisel aciklama
- Katman 2: kod ve algoritma detayi

## Cilt yapisi

Bu cilt sadece core muhendisligi kapsar:

- Boot ve init
- Scheduler ailesi
- Bellek yonetimi
- Lock-free I/O
- Ag cekirdegi guvenlik algoritmalari

Sonraki ciltler:

- Cilt 2: surucu ve donanim derinlesme
- Cilt 3: uyumluluk, runtime, urunlestirme

## Kullanim sozlesmesi

Bu kitapta verilen kod yollari, echOS deposundaki dosyalara dayalidir.
Bolumleri calisirken dogrudan kaynak dosyayi acman beklenir.

Bu kitap bir "okuma" metni oldugu kadar, bir "uygulama" metnidir.
