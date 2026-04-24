# Cilt 1 Kritik Kontrol Listesi

Bu bolum, Cilt 1 kapsamindaki temel kontrol maddelerini soru formundan cikarip
operasyonel inceleme listesi olarak sunar.

Kontrol maddeleri uc lane'de okunur:

- Kavramsal dogruluk
- Kod-yol tutarliligi
- Failure-path mitigasyonu

---


- Seviye A: temel kavrama
- Seviye B: uygulama ve yorum
- Seviye C: tasarim ve karar


---


## Kontrol Kumesi 1 - Boot ve init (A)


- Kontrol maddesi: Kernel giris noktasinda ilk kontrol edilen 3 bilgi nedir.
- Kontrol maddesi: UEFI ve Limine yolunun ortak amaci nedir.
- Kontrol maddesi: Bootstrap allocator neden heap allocator'dan ayridir.
- Kontrol maddesi: ACPI tablolari neden scheduler tarafini da etkiler.
- Kontrol maddesi: IOMMU neden guvenlik konusu olarak da ele alinmali.
- Kontrol maddesi: Erken log mekanizmasi neden kritik.
- Kontrol maddesi: Boot asamasinda fail-open neden riskli.
- Kontrol maddesi: `frame_allocator` hangi sorunu cozer.
- Kontrol maddesi: Kernel fiziksel aralik korumasi neden gerekir.
- Kontrol maddesi: "Calisiyor gorunup bozuk acilis" ne demektir.


## Kontrol Kumesi 2 - Boot ve init (B)


- Kontrol maddesi: ACPI init fail, IOMMU init success senaryosunu yorumla.
- Kontrol maddesi: Boot contract bozulursa semptomlar neden gec gorunebilir.
- Kontrol maddesi: Iki asamali init modelinin hata ayiklamaya etkisini acikla.
- Kontrol maddesi: Yanlis map varsayiminin memory corruption'a giden yolunu ciz.
- Kontrol maddesi: Boot adimlari arasi bagimlilik grafi cikar.
- Kontrol maddesi: Hangi adimlar "hard blocker", hangileri "degrade" olmalidir.
- Kontrol maddesi: Boot path testi icin minimum smoke senaryosu tasarla.
- Kontrol maddesi: Erken asamada hangi metrikler anlamlidir.
- Kontrol maddesi: Farkli platform boot yolunda ortak cekirdek surface nasil korunur.
- Kontrol maddesi: Boot'ta dogruluk mu hiz mi sorusuna yanit ver.


## Kontrol Kumesi 3 - Boot ve init (C)


- Kontrol maddesi: Yeni bir boot adapter eklenecekse entegrasyon sozlesmesini tasarla.
- Kontrol maddesi: "Sade acilis" ilkesini bozmadan ozellik nasil eklenir.
- Kontrol maddesi: Boot fault siniflarini severity modeline ayir.
- Kontrol maddesi: Erken bellek/CPU bilgisi tutarsizsa containment plani yaz.
- Kontrol maddesi: Boot telemetri formati oner.

---


## Kontrol Kumesi 4 - Scheduler temel (A)


- Kontrol maddesi: Scheduler'in birinci gorevi nedir.
- Kontrol maddesi: RT queue neden normal queue'dan once gelir.
- Kontrol maddesi: Idle task neden gereklidir.
- Kontrol maddesi: Work stealing neyi duzeltir.
- Kontrol maddesi: CFS neyi optimize eder.
- Kontrol maddesi: EEVDF hangi ek bilgiyle secim yapar.
- Kontrol maddesi: EDF hangi kritere gore secer.
- Kontrol maddesi: RR ile FIFO farki nedir.
- Kontrol maddesi: Vruntime neyi temsil eder.
- Kontrol maddesi: Timeslice ne demektir.


## Kontrol Kumesi 5 - Scheduler uygulama (B)


- Kontrol maddesi: `choose_spawn_cpu` akisini yorumla.
- Kontrol maddesi: Queue-length tabanli secimin avantaj/dezavantajini yaz.
- Kontrol maddesi: Affinity maskesi olmasa ne olur.
- Kontrol maddesi: CFS wakeup granularity neyi sinirlar.
- Kontrol maddesi: EEVDF'de `lag` negatifse ne anlarsin.
- Kontrol maddesi: EDF admission fail oldugunda ne donmeli.
- Kontrol maddesi: RT lane tum CPU'yu yutarsa hangi sinif bozulur.
- Kontrol maddesi: Work stealing'in cache etkisini tartis.
- Kontrol maddesi: Timing wheel neden linked-list timer'dan farklidir.
- Kontrol maddesi: Scheduler fairness ve latency neden gerilimlidir.


## Kontrol Kumesi 6 - Scheduler tasarim (C)


- Kontrol maddesi: Karma yukte scheduler policy secim tablosu tasarla.
- Kontrol maddesi: CFS+EEVDF birlikte calisacak bir hibrid lane oner.
- Kontrol maddesi: RT bandwidth limitini neye gore secersin.
- Kontrol maddesi: p99 latency odakli tuning plani yaz.
- Kontrol maddesi: Scheduler regression test matrisi olustur.

---


## Kontrol Kumesi 7 - Bellek temel (A)


- Kontrol maddesi: PMM nedir.
- Kontrol maddesi: Zone nedir.
- Kontrol maddesi: NORMAL->DMA32->DMA fallback niye vardir.
- Kontrol maddesi: Buddy allocator ne yapar.
- Kontrol maddesi: Fibonacci buddy farki nedir.
- Kontrol maddesi: TLSF neyi hizlandirir.
- Kontrol maddesi: COW ne zaman tetiklenir.
- Kontrol maddesi: THP nedir.
- Kontrol maddesi: MGLRU neyi siniflandirir.
- Kontrol maddesi: ZSwap neyi azaltir.


## Kontrol Kumesi 8 - Bellek uygulama (B)


- Kontrol maddesi: Zone fallback artisi hangi baski sinyalidir.
- Kontrol maddesi: Buddy coalesce bug'i nasil fark edilir.
- Kontrol maddesi: TLSF canary neyi yakalar.
- Kontrol maddesi: COW refcount hatasi ne dogurur.
- Kontrol maddesi: THP her region'a neden uygulanmaz.
- Kontrol maddesi: MGLRU victim seciminde hangi iki alan kullanilir.
- Kontrol maddesi: Writeback budget dusuk olursa ne olur.
- Kontrol maddesi: zswap CPU/IO tradeoff'unu acikla.
- Kontrol maddesi: Page fault tür ayrimi neden kritik.
- Kontrol maddesi: OOM killer en son care neden olmalidir.


## Kontrol Kumesi 9 - Bellek tasarim (C)


- Kontrol maddesi: Memory pressure altinda politika gecis tasarla.
- Kontrol maddesi: THP etkinlik heuristigi oner.
- Kontrol maddesi: Reclaim loop icin guardrail metrikleri belirle.
- Kontrol maddesi: zswap pool doluluk alarm politikasi ciz.
- Kontrol maddesi: Heap butunluk kontrolu icin olay kaydi modeli yaz.

---


## Kontrol Kumesi 10 - io_uring ve lock-free (A)


- Kontrol maddesi: SQ ve CQ farki nedir.
- Kontrol maddesi: Neden ring boyutu 2'nin kuvvetidir.
- Kontrol maddesi: `RING_MASK` ne ise yarar.
- Kontrol maddesi: Acquire ve Release neyi saglar.
- Kontrol maddesi: `smp_wmb` neden gerekir.
- Kontrol maddesi: `smp_rmb` neden gerekir.
- Kontrol maddesi: Batch pop ne kazandirir.
- Kontrol maddesi: Overflow sayaci neyi gosterir.
- Kontrol maddesi: Producer/consumer rolleri nasil ayrilir.
- Kontrol maddesi: Lock-free neden her zaman kolay degildir.


## Kontrol Kumesi 11 - io_uring uygulama (B)


- Kontrol maddesi: Tail erken publish edilirse hangi bug dogar.
- Kontrol maddesi: Head guncellemesi gecikirse ne olur.
- Kontrol maddesi: Volatile okuma/yazma neden kullanilmis olabilir.
- Kontrol maddesi: Ring dolu durumda dogru davranis nedir.
- Kontrol maddesi: Batch boyutu tuning'i nasil yapilir.
- Kontrol maddesi: Memory ordering testlerini nasil yazarsin.
- Kontrol maddesi: Single producer varsayimi nerede kritik.
- Kontrol maddesi: Multi consumer senaryosu nasil bozulabilir.
- Kontrol maddesi: ABI uyumlulugu neden testlenmeli.
- Kontrol maddesi: CQ overflow artisi hangi soruna isaret eder.


## Kontrol Kumesi 12 - io_uring tasarim (C)


- Kontrol maddesi: MPMC ring'e gecis icin yeni ownership modeli tasarla.
- Kontrol maddesi: Zero-copy lane eklersen hangi guard'lar gerekir.
- Kontrol maddesi: fd ve user pointer dogrulama sinirini ciz.
- Kontrol maddesi: Queue depth adaptif kontrol modeli oner.
- Kontrol maddesi: Lock-free regression test corpusu planla.

---


## Kontrol Kumesi 13 - Ag guvenlik temel (A)


- Kontrol maddesi: TLS 1.3 neyi guvence altina alir.
- Kontrol maddesi: QUIC neden UDP uzerinden calisir.
- Kontrol maddesi: WireGuard'in ana fikri nedir.
- Kontrol maddesi: HPACK Huffman neyi sikistirir.
- Kontrol maddesi: Key schedule neden katmanlidir.
- Kontrol maddesi: ACK range limiti neden konur.
- Kontrol maddesi: Nonce replay korumasi neyi engeller.
- Kontrol maddesi: EOS padding hatasi ne demektir.
- Kontrol maddesi: Handshake state machine niye zorunlu.
- Kontrol maddesi: Cipher suite secimi neyi etkiler.


## Kontrol Kumesi 14 - Ag guvenlik uygulama (B)


- Kontrol maddesi: TLS mesaj sirasini bozan bir senaryoda ne olur.
- Kontrol maddesi: QUIC parser limitlerini kaldirmanin riski nedir.
- Kontrol maddesi: WireGuard nonce yeniden kullanimi ne dogurur.
- Kontrol maddesi: HPACK decode'da invalid padding neden fail-closed olmali.
- Kontrol maddesi: QUIC stream coklamasi hangi problemi cozer.
- Kontrol maddesi: TLS transcript hash neden kritik.
- Kontrol maddesi: AEAD tag kontrolu yoksa ne olur.
- Kontrol maddesi: QUIC ACK range cok buyuk olursa hangi kaynak tukenir.
- Kontrol maddesi: WireGuard allowed_ips filtresi neden policy konusudur.
- Kontrol maddesi: Handshake timeout modeli nasil secilir.


## Kontrol Kumesi 15 - Ag guvenlik tasarim (C)


- Kontrol maddesi: Cekirdek ag stack'i icin parser hardening checklisti yaz.
- Kontrol maddesi: TLS/QUIC telemetry seti tasarla.
- Kontrol maddesi: WireGuard session anahtar yenileme politikasi oner.
- Kontrol maddesi: HPACK decode fuzz plani ciz.
- Kontrol maddesi: Ag tarafi regression corpusunu siniflandir.

---


## Kontrol Kumesi 16 - Karma final sorular (A+B)


- Kontrol maddesi: Scheduler ve reclaim arasindaki dolayli etkilesimi acikla.
- Kontrol maddesi: COW artisi scheduler latencyyi nasil etkileyebilir.
- Kontrol maddesi: io_uring yuksek yukte bellek baskisini nasil tetikler.
- Kontrol maddesi: QUIC trafik patlamasi hangi cekirdek metriklerini degistirir.
- Kontrol maddesi: RT lane aktifken writeback budget niye onemlidir.
- Kontrol maddesi: THP etkinligi io_uring throughput'a nasil etki eder.
- Kontrol maddesi: MGLRU refault artisinda scheduler semptomu gorulur mu.
- Kontrol maddesi: DNS/HPACK parser guard'lari memory pressure ile nasil baglanir.
- Kontrol maddesi: WireGuard decrypt lane'i neden CPU-affinity isteyebilir.
- Kontrol maddesi: Tek bir metric ile tum sistemi neden yonetemezsin.


## Kontrol Kumesi 17 - Karma tasarim sorulari (C)


- Kontrol maddesi: "Laptop sinifi" ve "sunucu sinifi" icin ayri policy profilleri tasarla.
- Kontrol maddesi: Core alt sistemleri icin ortak alarm esikleri belirle.
- Kontrol maddesi: Cilt 2'ye geciste hangi teknik borclar zorunlu kapanmali.
- Kontrol maddesi: Fail-closed ilkesiyle performans arasinda karar matrisin ne olur.
- Kontrol maddesi: Bir release oncesi "core readiness review" sablonu yaz.

---


## Raporlama notasyonu


Her soruda bu mini formati kullan:

- Kontrol maddesi: Kisa cevap.
- Kontrol maddesi: Kod referansi.
- Kontrol maddesi: Risk ve mitigasyon.

Bu format, ezber yerine muhendislik muhakemesi olusturur.
