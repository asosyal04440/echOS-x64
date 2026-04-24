# Cilt 1 Algoritma Gorsel Rehber

Bu bolum, Cilt 1 icindeki ana algoritmalari bir "kod + sekil + failure path" zinciriyle
okutmak icin yazildi. Hedef, soyut anlatimi azaltip karar noktalarini gozle gorulur hale
getirmek.

Okuma protokolu:

1. Once diyagramdaki veri akisini oku.
2. Sonra kod referansini acip ayni akisla eslestir.
3. En son patolojik durumda hangi invariantin kirildigini yaz.

Kisa not: Bu rehberdeki her gorsel, `docs/book-core-v1/figures/generated/` altindaki
kaynaklardan uretilmistir.

---

## G01 - Boot contract ve init pipeline geometrisi

![Boot Path](../figures/generated/boot-path.svg)

### Sekli nasil okumali?

Diyagramin sol tarafi firmware ve loader sozlesmesini, orta kismi erken init publication
sinirlarini, sag tarafi ise scheduler/memory gibi core lane acilisini temsil eder.

Kritik eksen su:

- "Bu asamada capability gercekten aktif mi, yoksa yalnizca flag mi set edildi?"

Eger flag aktif ama altyapi pasif kaldiysa sistem aciliyormus gibi davranir; ariza ise daha
sonra daginik semptomlarla ortaya cikar.

### Yurutum izi

| Asama | Beklenen durum | Tipik kirilma |
|---|---|---|
| Firmware handoff | bellek haritasi tutarli | reserve/usable karismasi |
| Erken map | cekirdek araligi korumali | self-overwrite |
| Capability acilisi | ACPI/IOMMU tutarli | yarim aktif cihaz lane |
| Core lane acilisi | scheduler + memory canli | sessiz degrade |

### Worst-case notu

En pahali bug, "hemen coken" degil, "gec coken" bugtur. Bu nedenle boot lane icin dogru
strateji ortalama hizdan once deterministik hata siniridir.

Model:

\[
L_{boot} = L_{handoff} + L_{early\_map} + L_{capability} + L_{core\_open}
\]

Burada hedef, tek tek terimleri minimuma cekmek degil; her terimin dogruluk kosulunu
ispatlanabilir tutmaktir.

---

## G02 - Scheduler policy arbitration haritasi

![Scheduler Decision](../figures/generated/scheduler-decision.svg)

### Sekildeki ana karar

Arbitration lane, policy secimini "kimin daha hizli oldugu" yerine "hangi gorevin sinifi ne
gerektiriyor" sorusuyla yapar. RT, CFS, EEVDF ve Deadline ayni havuzda yaristirilmaz; sirali
ve kosullu secimle orkestre edilir.

### Kod-karsiligi

- policy secim omurgasi: `src/task/scheduler.rs`
- RT lane: `src/task/rt_scheduler.rs`
- adalet lane: `src/task/cfs.rs`, `src/task/eevdf.rs`
- son tarih lane: `src/task/deadline.rs`

### Degisim tokusu

| Kazanim | Bedel |
|---|---|
| lane-ozel optimizasyon | policy karmasikligi |
| daha iyi tail-latency kontrolu | tuning yuzeyi genisler |
| admission ile zarar sinirlama | telemetry zorunlulugu |

Skew modeli:

\[
Skew = \max_i q_i - \min_i q_i
\]

Skew buyudukce calma maliyeti ve p99 gecikme baskisi artis egilimine girer.

---

## G03 - CFS, EEVDF, EDF yan-yana karsilastirma

![CFS EEVDF EDF Compare](../figures/generated/eevdf-cfs-edf-compare.svg)

Bu gorsel uc farkli secim mantigini ayni eksende gosterir:

- CFS: vruntime adaleti
- EEVDF: eligibility + virtual deadline
- EDF: mutlak son tarih onceligi

### Nerede ayrisiyorlar?

1. CFS adaleti optimize eder.
2. EEVDF wakeup davranisinda daha ince ayrim yapar.
3. EDF admission dogruysa deadline lane icin teorik ustunluk saglar.

### Ana denklem seti

\[
\Delta v = \frac{\Delta t \cdot W_0}{w_i}
\]

\[
U = \sum_i \frac{C_i}{T_i} \le 1
\]

Pratikte ikinci esitlikte guvenlik payi birakilmadan stabil bir lane beklenmez.

### Patolojik durum

Interaktif wakeup firtinasi altinda CFS tek basina agresif preemption uretebilir. EEVDF bu
durumu yumusatir; fakat dilim ayari kotuyse throughput lane zarar gorebilir.

---

## G04 - Timing wheel cascade anatomisi

![Timing Wheel Cascade](../figures/generated/timing-wheel-cascade.svg)

Timing wheel sekli, uzun sureli zamanlayicilarin neden lineer listeye birakilamayacagini acik
gosterir. Alt seviyede sik olaylar, ust seviyede uzun horizon tutulur.

### Operasyonel kurallar

- kisa horizon -> alt wheel
- uzun horizon -> ust wheel
- wrap oldugunda cascade

### Kaza noktasi

Cascade unutulursa timer "kaybolmaz"; gecikir. Bu gecikme scheduler lane'e jitter olarak
yansir ve kuyruk adaletini bozar.

\[
T_{insert} \approx O(1),\quad T_{tick} \approx O(1)
\]

Bu amortized kazanim, dogru seviye gecisi kurallari bozulmadigi surece gecerlidir.

---

## G05 - Work-stealing deque race kesiti

Deque lane, owner ve stealer tarafini ayri uc noktalardan surerek cache davranisini korur.
Asil kritik yer son eleman yarisidir; burada CAS kazanani veriyi alir, kaybeden geri cekilir.

Kisa iz:

```text
owner: bottom--, read slot, CAS(top)
stealer: read top, read slot, CAS(top)
```

### Neden sekil gerekir?

Bu lane'de hata cogu zaman crash degil sessiz bozulma olarak gelir. Sekil, memory-order
sinirlarini zihinde sabitleyip "hangi okuma hangi yazidan sonra gorulebilir" sorusunu
somutlastirir.

---

## G06 - Zone fallback topolojisi

![Memory Zone Fallback](../figures/generated/memory-zones-fallback.svg)

DMA kisitlari nedeniyle fiziksel bellek tek havuz gibi davranamaz. Sekil, NORMAL -> DMA32 ->
DMA zincirini sade ama karara yardimci bir formda verir.

### Yorum

Fallback artisi tek basina "hata" degildir; fakat su durumu isaret eder:

- ust zone baskisi kalici hale geliyor
- gelecekteki tahsislerde tail-risk buyuyor

### Olcum paketi

| Metrik | Esik sinyali |
|---|---|
| fallback_orani | surekli artis |
| zone_fragmentation | birlesme kalitesi dususu |
| alloc_tail_latency | p99 sivrilme |

---

## G07 - Fibonacci buddy split/coalesce cikisi

![Fibonacci Buddy Split Coalesce](../figures/generated/fibonacci-buddy-split-coalesce.svg)

Bu gorsel, allocatorin yalnizca tahsis degil iade kalitesiyle de degerlendirilecegini
hatirlatir. Tahsis hizi yuksek ama coalesce zayifsa bir sure sonra serbest alan olmasina ragmen
tahsis basarisizligi gorulebilir.

### Ana hata siniflari

1. buddy adresi yanlis hesap
2. birlesme adimi eksik
3. fragmentasyonun gizli birikmesi

### Mitigasyon

- split/collapse adimlarini olay kaydina yaz
- uzun kosuda free-list dagilimini takip et
- "yeterli alan var ama allocate yok" sinyalini blocker kabul et

---

## G08 - Page fault, COW ve THP karar agaci

![Page Fault COW THP](../figures/generated/page-fault-cow-thp.svg)

Sekil iki soruyu ayni diyagramda toplar:

- fault tipi ne?
- bu fault COW mu, lazy map mi, THP denemesi mi ister?

### Dallanma noktasi

`PROTECTION_VIOLATION` + `WRITE` kombinasyonu COW lane'e gitmelidir. Bu kosul yanlis route
olursa izin modeli bozulur.

### Karar matrisi

| Kosul | Yol |
|---|---|
| protection + write + shared | COW copy/remap |
| non-present + lazy region | lazy map |
| buyuk, uygun hizalama | THP dene |
| uygun degil | 4KiB fallback |

### Guvenlik notu

W^X kirpmasi bu agacin kenarinda degil merkezindedir; map flag sanitization yol disina
tasinmamali.

---

## G09 - Reclaim, MGLRU ve zswap ortak resmi

![MGLRU Reclaim ZSwap](../figures/generated/mglru-reclaim-zswap.svg)

Bellek baskisi tek adimlik bir sorun degildir. Sekildeki akis reclaim, writeback ve zswap
hatlarini ortak bir geri-besleme sistemi gibi ele alir.

### Dinamik denge

\[
\rho = \frac{\lambda_{dirty}}{\mu_{writeback}}
\]

Uzun sure \(\rho > 1\) kalmasi, writeback kuyrugunun kendi kendine toparlanamayacagini gosterir.
Bu durumda sadece daha fazla reclaim denemek genelde yeterli olmaz.

### Operasyonel karar

- pressure artiyor + refault artiyorsa: victim secimi ve generation ayari gozden gecir
- pressure artiyor + dirty kuyrugu buyuyor: writeback budget ve servis hizini ayarla
- IO pahali + CPU bossa: zswap kazanci yuksek olur

---

## G10 - io_uring publication boundary haritasi

![io_uring Lock Free](../figures/generated/io-uring-lockfree.svg)

Bu sekildeki tek kritik cizgi sudur: veri yazilmadan `tail` publish edilmez.

### Uygulama duzeni

1. SQE yaz
2. `smp_wmb`
3. `tail.store(..., Release)`

Consumer tarafi bunun simetriyidir:

1. `tail.load(Acquire)`
2. `smp_rmb`
3. SQE oku

### Neden bu kadar kati?

Lock-free lane'de yanlis sira hemen fark edilmez; aralikli corrupt completion olarak gelebilir.
Bu nedenle ordering dogrulamasini yalniz birim testine birakmak yetersizdir, stress tekrar
zorunludur.

---

## G11 - TLS 1.3 key schedule gorunum

![TLS13 Handshake](../figures/generated/tls13-handshake.svg)

Gorsel, early secret -> handshake secret -> master secret zincirini satir satir takip etmeyi
kolaylastirir. Buradaki risk, primitive seciminden cok state gecisinin transcript ile
uyusmasindadir.

### Kontrol listesi

- mesaj sirasi tutarli mi?
- transcript hash zinciri atlamasiz mi?
- key derivation adimlari ayni baglam etiketiyle mi cagriliyor?

### Failure modeli

State makinesi bir adim atlarsa semptom bazen "hemen fail" degil, uzak noktada decrypt veya
verify hatasi olarak gorulur. Bu nedenle handshake olaylarini adim bazli izlemek daha saglikli
teshis uretir.

---

## G12 - QUIC paket akisi ve parser guard noktasi

![QUIC Flow](../figures/generated/quic-flow.svg)

QUIC gorseli, packet/stream/cipher durumlarinin ayni anda ilerledigini gosterdigi icin parser
sertlestirme kararlarini daha okunur kilar.

### Guard mantigi

ACK range limiti gibi sinirlar sadece performans icin degil, parser amplification riskini
sinirlamak icin de vardir.

### Inceleme sorusu

Bir paket serisinde stream state degisimi ile ACK decode maliyeti ayni anda artis gosteriyorsa,
bottleneck parser mi yoksa congestion lane mi? Bu ayirimi yapmadan tuning kararina gitmek hatali
olur.

---

## G13 - WireGuard handshake ve replay koruma

![WireGuard Handshake](../figures/generated/wireguard-handshake.svg)

WireGuard lane sade gorunur ama nonce monotonlugu bozulursa guvenlik siniri dogrudan kirilir.
Sekil, el sikisma ve veri paketinin ayni replay modeline bagli oldugunu gosterir.

### Nonce kurali

- yeni nonce <= son nonce ise paket reddedilir

Bu kadar kisa bir kural, kernel tarafinda en yuksek etkiyi ureten guvenlik kontrollerinden
biridir.

### Policy baglantisi

`allowed_ips` filtresi kripto dogrulamadan bagimsiz degildir; dogrulanmis paket bile route
politikasina tersse kabul edilmemelidir.

---

## G14 - HPACK Huffman decode bit-seviyesi harita

![HPACK Huffman Decode](../figures/generated/hpack-huffman-decode.svg)

Bit-seviyesi parserlarda en cok gorulen hata, "tamamlanmamis kodu sessiz kabul etme" hatasidir.
Bu gorsel, EOS ve padding denetimini decode sonuna birakmanin neden gerekli oldugunu netlestirir.

### Fail-closed noktasi

Padding denetimi gecilemezse cikis uretilmez. Eksik denetim, ust katmana bozuk ama gorunurde
gecerli veri tasiyabilir.

### Hedef test kumesi

1. dogru kod + dogru padding
2. dogru kod + bozuk padding
3. kesik bit dizisi
4. EOS benzeri sahte sonlanma

---

## G15 - Algoritmalar arasi zincirleme ariza resmi

Tek bir lane bozuldugunda digerleri de etkilenir. Asagidaki ornek tipik bir zincirdir:

1. scheduler skew artar
2. reclaim gecikir
3. dirty queue buyur
4. io completion gecikir
5. network timeout artar

Bu zinciri kesmek icin alt sistem bazli degil, ortak telemetry paneli gerekir.

Ortak panelde en az su alanlar bulunmali:

- p99 latency
- queue depth dagilimi
- pressure sinyali
- drop/overflow sayaçlari
- retry ve timeout sayisi

---

## G16 - Gorselden karara gecis icin uygulama cetveli

Bu cetvel, bolumu okuyan kisinin "sekli gordum" asamasindan "karar verebiliyorum" asamasina
gecmesini hedefler.

| Adim | Uretilen cikti |
|---|---|
| Diyagram okuma | veri akis notu |
| Kod esleme | fonksiyon-listesi |
| Invariant cikarma | 3 maddelik kontrol listesi |
| Failure simule etme | semptom tablosu |
| Mitigasyon secimi | gerekceli patch plani |
| Dogrulama | metrik oncesi/sonrasi raporu |

### Kapanis notu

Gorsel tek basina yeterli degildir; ama karar noktalarini sabitleyen en etkili yardimci
katmandir. Bu nedenle Cilt 1'in geri kalaninda her algoritma secimi en az bir akis diyagrami
ve bir patolojik senaryo ile birlikte okunmalidir.
