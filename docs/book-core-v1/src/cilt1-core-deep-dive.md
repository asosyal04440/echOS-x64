# Cilt 1 Deep Dive - Core Muhendislik Notlari

Bu bolum, ana cekirdek akista gordugun her konuyu ikinci katmanda derinlestirir.
Burada hedef sadece "calisir" degil, "neden boyle calisir" sorusunu cevaplamaktir.

---

## Bolum D1 - Boot contract ve ilk dogru karar

### D1.1 Firmware ile kernel arasindaki sozlesme

Bir kernel, ilk satirindan once bile bir sozlesmeye baglidir.
Bu sozlesme su basliklari kapsar:

- Nereden cagrildim?
- Hangi bellek araliklari guvenli?
- CPU hangi modda?
- Hangi kesmeler kapali/acik?

echOS tarafinda bu sozlesmenin pratik yuzunu `src/main.rs` ve boot adapter katmani tasir.
Yanlis bir varsayim burada girerse tum sistem ust katmanlarda rastgele gorunur.

### D1.2 Neden ilk asama sade tutulur?

Ilk asama kodu cok is yapmaya kalkarsa su riskler olusur:

- Hata ayiklama zorlasir
- Fault oldugunda geri iz zorlasir
- Platform farklari ilk adimda patlar

Bu nedenle iyi bir cekirdek acilisi iki adimlidir:

1. Minimal stabil acilis
2. Kademeli kapasite acma

echOS'ta ACPI, IOMMU, scheduler ve memory init adimlari bu kademeli modele uygundur.

### D1.3 Worst-case dusunme

Boot yolunda en kotu durum su degildir: "acilmiyor".
En kotu durum: "aciliyor gibi gorunup sessizce bozuk acilmasi".

Bu nedenle fail-closed tutum daha guvenlidir:

- Ozellik aktif oldugundan emin degilsen kapali basla
- Her init adimini logla
- Sadece dogrulanan capability'leri ac

---

## Bolum D2 - Erken bellek, frame tahsisi ve ownership

### D2.1 Bootstrap allocator neden ayridir?

Heap henuz yokken allocator gerekir. Bu paradoksu iki fazli model cozer:

- Faz A: bump benzeri bootstrap frame allocator
- Faz B: tam PMM ve heap allocator

`src/memory/frame_allocator.rs` tam olarak bu gecis problemini cozer.

### D2.2 Kernel fiziksel aralik korumasi

Bootstrap allocatorin en kritik gorevi, kernelin kendi fiziksel araligini
asla yeniden tahsis etmemektir.

Eger bu korunmazsa:

- Kod segmenti ustune yeni frame verilir
- Data veya stack sessizce bozulur
- Semptom gec ortaya cikar ve iz surmek zorlasir

### D2.3 Ownership modeli

Her frame icin tek bir ownership hakikati olmalidir.
Bu hakikat su soruyu tekil cevaplar:

"Bu frame su anda kime ait?"

Ownership belirsizligi olan sistemlerde leak ve double free neredeyse kacinilmazdir.

---

## Bolum D3 - Scheduler sistem tasarimi: tek algoritma yetmez

### D3.1 Neden scheduler ailesi gerekir?

Tek bir policy ile tum is yuklerini optimize etmek imkansiza yakindir.
Bu nedenle echOS'ta policy ailesi kullanilir:

- RT (FIFO/RR) - deterministik oncelik
- CFS - genel amacli adalet
- EEVDF - wakeup/latency hassas secim
- Deadline/EDF - son tarih odakli planlama

Bu ayrim bir luks degil, farkli is siniflarinin dogal ihtiyacidir.

### D3.2 Scheduler secim karar agaci

`src/task/scheduler.rs` icindeki secim akisi, onceligi su sekilde verir:

1. RT varsa once RT
2. Yerel queue doluysa yerel pop
3. Bos ise work stealing
4. Is yoksa idle

Bu karar agaci cache locality ile fairness arasinda pratik bir denge kurar.

### D3.3 Metrik secimi

Scheduler performansini tek metrikle olcemezsin.
Asgari su metrik seti gerekir:

- p50/p95/p99 wakeup latency
- context switch maliyeti
- queue depth dagilimi
- CPU load skew (core'lar arasi dengesizlik)

---

## Bolum D4 - RT scheduler: determinism disiplini

### D4.1 FIFO ve RR farki pratikte ne?

- FIFO: gorev birakmadikca devam eder
- RR: ayni prio grubunda dilimle donusur

RR yoksa ayni prio grubunda fairness hizla bozulur.
FIFO yanlis sinifta kullanilirsa starvation riski dogar.

### D4.2 Runtime budget fikri

RT lane, tum CPU'yu sonsuzca yutmamalidir.
Bu nedenle runtime/period mantigi ile bir tavan belirlenir.

Bu, "RT garantisi" ile "sistem canliligi" arasinda zorunlu bir uzlasidir.

---

## Bolum D5 - CFS: adaletin muhendisligi

### D5.1 Vruntime'in sezgisi

Vruntime, "kac milisaniye calisti" degil,
"agirligina gore ne kadar hak etti" sorusuna cevap verir.

Formul:

\[
\Delta v = \frac{\Delta t \cdot W_0}{w_i}
\]

Burada:

- \(\Delta t\): gercek calisma suresi
- \(W_0\): referans agirlik
- \(w_i\): gorevin agirligi

### D5.2 Patolojik durum

Cok sik uyanan interactive gorevler ile uzun CPU-bound gorevler
ayni runqueue'da oldugunda preemption firtinasi dogabilir.

echOS mitigasyonu: wakeup granularity ile "anlik preempt" egi yukseltilir.

### D5.3 CFS icin deney seti

Minimum deney:

- 1 CPU-bound + 1 interactive gorev
- nice degerleri farkli kombinasyonlarda
- p99 response time takibi

---

## Bolum D6 - EEVDF: eligibility katmani

### D6.1 CFS'e ne ekler?

EEVDF sadece "kimin vruntime'i kucuk" bakmaz.
Once su sorar: "bu gorev su an eligible mi?"

Bu sayede wakeup davranisi daha kontrollu olur.

`src/task/eevdf.rs` icindeki kritik alanlar:

- `lag`
- `eligible_vtime`
- `virtual_deadline`

### D6.2 Matematik sezgisi

Lag pozitif ise gorev geridedir; scheduler onu ondeleyebilir.
Lag negatif ise gorev zaten avantajlidir; secim baskisi azalir.

### D6.3 Failure mode

Yanlis `slice_ns` ayari, ya latencyyi bozar ya throughput'u dusurur.
Bu nedenle slice tuning tek benchmark ile degil, profil siniflariyla yapilmalidir.

---

## Bolum D7 - Deadline scheduler: EDF ve CBS

### D7.1 EDF neyi garanti eder?

Tek CPU ve dogru admission altinda EDF teorik olarak optimaldir.
Ama pratikte admission hatasi yaparsan hicbir teori seni kurtarmaz.

### D7.2 Admission kontrolu

Temel kosul:

\[
\sum_i \frac{C_i}{T_i} \le 1
\]

Gercek sistemde guvenlik payi birakilir.
echOS lane'i bunu bandwidth kontrolu ile uygular.

### D7.3 CBS neden var?

CBS, tek gorevin butce asimini sistem genelini bozmayacak sekilde sinirlar.
Butce bitince throttle olur, periodte replenish olur.

---

## Bolum D8 - Work stealing ve lock-free queue semantigi

### D8.1 Neden deque?

Owner tarafi LIFO yapinca cache locality artar.
Stealer tarafi FIFO yapinca owner ile cakisma azalir.

Bu tasarim, hem locality hem balancing acisindan pratikte cok etkilidir.

### D8.2 Atomik ordering sinirlari

Lock-free kodda dogru atomik secimi zorunludur.
`src/task/deque.rs` satirlarinda Acquire/Release/SeqCst kombinasyonu
son eleman yarisini emniyetli yonetmek icin kullanilir.

### D8.3 Worst-case

Yanlis ordering her zaman crash uretmez.
Cogunlukla sessiz veri bozulmasi uretir.
Bu nedenle property test ve stress test beraber gerekir.

---

## Bolum D9 - Timing wheel: O(1) amortized uyanma

### D9.1 Sorun tanimi

Milyonlarca sleeping task varsa,
lineer tarama tabanli timer listesi kabul edilemez.

### D9.2 Hiyerarsik wheel fikri

Kucuk sureler alt seviyede,
uzun sureler ust seviyede tutulur.
Wrap aninda cascade yapilir.

`src/task/timer.rs` bu modeli 4 seviye ile uygular.

### D9.3 Tradeoff

- Arti: O(1) amortized
- Eksi: implementation karmasikligi ve edge-case test yuksekligi

---

## Bolum D10 - PMM, zone ve fallback ekonomisi

### D10.1 Zone gercegi

Fiziksel bellek tek blok degildir.
DMA cihaz limitleri nedeniyle zone siniflari gerekir.

`src/memory/fibonacci_pmm.rs` zone fallback'i acik tanimlar:

NORMAL -> DMA32 -> DMA

### D10.2 Yanlis fallback etkisi

Sik fallback,
normal zone baskisini gizleyip ileri asamada patlama yaratabilir.

Bu yuzden zone bazli telemetri "opsiyonel" degil, zorunludur.

---

## Bolum D11 - Fibonacci buddy: fragmentation ile savas

### D11.1 Neden klasik 2^n degil?

Klasik buddy, bazi boyutlarda gereksiz ic bosluk uretir.
Fibonacci siniflari bu boslugu azaltmayi hedefler.

### D11.2 Coalesce mantigi

Allocator'in degeri sadece allocate hizinda degil,
deallocate sonrasi birlestirme kalitesinde cikar.

`find_buddy`, `split_block`, `try_coalesce` uclusu bu iskeleti tasir.

### D11.3 Risk

Coalesce bug'i uzun sure fark edilmeden birikir ve sonunda
"yeterli bellek var ama tahsis yok" semptomu dogurur.

---

## Bolum D12 - TLSF: gercek zamanli heap disiplini

### D12.1 O(1) ne zaman anlamli?

Gercek zamanli sinifta allocator jitter'i kritikse,
ortalama hiz degil worst-case suren onceliklidir.

TLSF burada deterministic sinir verir.

### D12.2 Koruma katmanlari

echOS sarmalamasinda su guvenlik katmanlari var:

- early heap ayrimi
- heap boundary kontrolu
- canary ve tracker
- null ve align savunmasi

### D12.3 Yan etkiler

Ek guvenlik kontrolu belli bir maliyet getirir.
Ama kernel tarafinda sessiz bozulma maliyeti cok daha yuksektir.

---

## Bolum D13 - Page fault, COW, THP

### D13.1 Fault tipi ayrimi

Her fault ayni degildir:

- protection violation
- non-present lazy map fault

`handle_user_page_fault` bu ayrimi ilk adimda yapar.

### D13.2 COW ownership duzeni

Yazma fault aninda iki yol vardir:

1. Refcount 1 ise writable upgrade
2. Refcount > 1 ise yeni frame + kopya

Bu karar hem performans hem dogruluk acisindan kritiktir.

### D13.3 THP karari

THP her yerde iyi degildir.
Region tipi, shared/cow durumu ve hizalama uygun degilse
4KiB yoluna dusmek daha dogrudur.

---

## Bolum D14 - Reclaim: MGLRU, writeback, zswap

### D14.1 Reclaim stratejisinin kalbi

Bellek baskisinda iki yanlis cok yaygindir:

- Fazla agresif reclaim -> latency patlar
- Fazla pasif reclaim -> OOM yaklasir

echOS reclaim dongusu pressure sinyali, generation age ve writeback budget ile
denge kurmaya calisir.

### D14.2 MGLRU secim mantigi

Victim secimi:

- once en eski generation
- esitlikte dusuk hot score

Bu, salt LRU'ya gore refault davranisini daha iyi yakalar.

### D14.3 Zswap tradeoff

Disk IO yerine RAM compression secmek,
CPU maliyeti ile IO kazanci arasinda degisim tokusu yaratir.

Dogru eksen:

"Bu yukte CPU compression maliyeti disk gecikmesinden kucuk mu?"

---

## Bolum D15 - Lock-free io_uring ring

### D15.1 Publication boundary

Ring tasariminda birinci kural:

"Veri yazilmadan tail publish edilmez."

Bu nedenle `smp_wmb` -> `tail Release` dizilimi cekirdektir.

### D15.2 Batch islemenin etkisi

Batch pop ile:

- bariyer sayisi azalir
- head guncelleme amortize olur

Yuksek throughput lane'inde bu fark buyur.

### D15.3 ABI ve guvenlik

Ring yapisi sadece hiz degil, ABI dogrulugudur.
Struct layout uyumsuzlugu, kullanici-kernel sinirinda sessiz veri bozulmasi yaratir.

---

## Bolum D16 - TLS 1.3, QUIC, WireGuard, HPACK

### D16.1 TLS 1.3 key schedule disiplini

Anahtar takvimi katmanli tasarlandigi icin,
her asamanin yanlis implementasyonu tum guven modelini bozar.

`src/net/tls.rs` HKDF zinciri bu katmanlari acik kodlar.

### D16.2 QUIC parser sinirlari

QUIC frame modeli guclu ama parser acisindan risklidir.
Bu nedenle boyut ve sayi limitleri (ACK ranges gibi) zorunludur.

### D16.3 WireGuard nonce anti-replay

Replay korumasi olmadan tunnel guvenligi eksik kalir.
`receiving_nonce` monotonlugu burada cekirdek savunmadir.

### D16.4 HPACK bit-level decode

Bit-seviyesi decode algoritmalari corner-case sever.
EOS ve padding dogrulamasi fail-closed olmazsa parser aciklari olusur.

---

## Bolum D17 - Olcum, benchmark ve raporlama disiplini

### D17.1 Neyi olcmelisin?

Her alt sistem icin asgari metrik:

- Scheduler: wakeup p99, switch cost
- Memory: reclaim latency, refault rate, zswap hit ratio
- I/O: SQ->CQ completion latency, batch efficiency
- Network: handshake latency, packet loss altinda recovery

### D17.2 Tek benchmark tuzagi

Tek benchmark sonucu ile policy secmek yanlistir.
Farkli yuk profilleri kullan:

- CPU-bound
- IO-bound
- mixed interactive
- burst traffic

### D17.3 Rapor formati

Iyi bir teknik rapor su uc seyi net verir:

1. Senaryo
2. Metrik
3. Karar

"Hizli gorundu" ifadesi teknik karar degildir.

---

## Bolum D18 - Core muhendislikte sik yapilan hatalar

1. Ortalama metrik ile worst-case'i karistirmak
2. Lock-free kodu sadece normal yukte test etmek
3. Admission kontrolunu atlayip deadline garantisi iddia etmek
4. Reclaim ayarlarini sabit yukte optimize edip karisik yukte dogrulamamak
5. Build pipeline'i tek makineye bagli birakmak

---

## Bolum D19 - Cilt 1 final checklist

Bu cildi bitiren ogrencinin su sorulara yazili cevap verebilmesi beklenir:

- CFS ile EEVDF arasinda secim kriterin ne olur?
- EDF admission neden sadece bir formulle bitmez?
- Work stealing deque'de neden son eleman yarisi ozel ele alinir?
- COW fault yolunda refcount kararinin performans etkisi nedir?
- MGLRU victim secimi niye sadece son erisim zamani degil?
- io_uring ring'de publication boundary neden hayati?
- TLS key schedule implementasyonunda en kritik hata sinifi nedir?
- QUIC parser'da limit koymanin guvenlik gerekcesi nedir?

Bu sorularin cevabi hazirsa, Cilt 2'ye gecis teknik olarak saglamdir.
