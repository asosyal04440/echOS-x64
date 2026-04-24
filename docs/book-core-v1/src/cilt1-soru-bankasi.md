# Cilt 1 Soru Bankasi

Bu bankada 3 seviye soru var:

- Seviye A: temel kavrama
- Seviye B: uygulama ve yorum
- Seviye C: tasarim ve karar

Toplam hedef: ders boyunca 300+ soru.

---

## Kume 1 - Boot ve init (A)

1. Kernel giris noktasinda ilk kontrol edilen 3 bilgi nedir?
2. UEFI ve Limine yolunun ortak amaci nedir?
3. Bootstrap allocator neden heap allocator'dan ayridir?
4. ACPI tablolari neden scheduler tarafini da etkiler?
5. IOMMU neden guvenlik konusu olarak da ele alinmali?
6. Erken log mekanizmasi neden kritik?
7. Boot asamasinda fail-open neden riskli?
8. `frame_allocator` hangi sorunu cozer?
9. Kernel fiziksel aralik korumasi neden gerekir?
10. "Calisiyor gorunup bozuk acilis" ne demektir?

## Kume 2 - Boot ve init (B)

1. ACPI init fail, IOMMU init success senaryosunu yorumla.
2. Boot contract bozulursa semptomlar neden gec gorunebilir?
3. Iki asamali init modelinin hata ayiklamaya etkisini acikla.
4. Yanlis map varsayiminin memory corruption'a giden yolunu ciz.
5. Boot adimlari arasi bagimlilik grafi cikar.
6. Hangi adimlar "hard blocker", hangileri "degrade" olmalidir?
7. Boot path testi icin minimum smoke senaryosu tasarla.
8. Erken asamada hangi metrikler anlamlidir?
9. Farkli platform boot yolunda ortak cekirdek surface nasil korunur?
10. Boot'ta dogruluk mu hiz mi sorusuna yanit ver.

## Kume 3 - Boot ve init (C)

1. Yeni bir boot adapter eklenecekse entegrasyon sozlesmesini tasarla.
2. "Sade acilis" ilkesini bozmadan ozellik nasil eklenir?
3. Boot fault siniflarini severity modeline ayir.
4. Erken bellek/CPU bilgisi tutarsizsa containment plani yaz.
5. Boot telemetri formati oner.

---

## Kume 4 - Scheduler temel (A)

1. Scheduler'in birinci gorevi nedir?
2. RT queue neden normal queue'dan once gelir?
3. Idle task neden gereklidir?
4. Work stealing neyi duzeltir?
5. CFS neyi optimize eder?
6. EEVDF hangi ek bilgiyle secim yapar?
7. EDF hangi kritere gore secer?
8. RR ile FIFO farki nedir?
9. Vruntime neyi temsil eder?
10. Timeslice ne demektir?

## Kume 5 - Scheduler uygulama (B)

1. `choose_spawn_cpu` akisini yorumla.
2. Queue-length tabanli secimin avantaj/dezavantajini yaz.
3. Affinity maskesi olmasa ne olur?
4. CFS wakeup granularity neyi sinirlar?
5. EEVDF'de `lag` negatifse ne anlarsin?
6. EDF admission fail oldugunda ne donmeli?
7. RT lane tum CPU'yu yutarsa hangi sinif bozulur?
8. Work stealing'in cache etkisini tartis.
9. Timing wheel neden linked-list timer'dan farklidir?
10. Scheduler fairness ve latency neden gerilimlidir?

## Kume 6 - Scheduler tasarim (C)

1. Karma yukte scheduler policy secim tablosu tasarla.
2. CFS+EEVDF birlikte calisacak bir hibrid lane oner.
3. RT bandwidth limitini neye gore secersin?
4. p99 latency odakli tuning plani yaz.
5. Scheduler regression test matrisi olustur.

---

## Kume 7 - Bellek temel (A)

1. PMM nedir?
2. Zone nedir?
3. NORMAL->DMA32->DMA fallback niye vardir?
4. Buddy allocator ne yapar?
5. Fibonacci buddy farki nedir?
6. TLSF neyi hizlandirir?
7. COW ne zaman tetiklenir?
8. THP nedir?
9. MGLRU neyi siniflandirir?
10. ZSwap neyi azaltir?

## Kume 8 - Bellek uygulama (B)

1. Zone fallback artisi hangi baski sinyalidir?
2. Buddy coalesce bug'i nasil fark edilir?
3. TLSF canary neyi yakalar?
4. COW refcount hatasi ne dogurur?
5. THP her region'a neden uygulanmaz?
6. MGLRU victim seciminde hangi iki alan kullanilir?
7. Writeback budget dusuk olursa ne olur?
8. zswap CPU/IO tradeoff'unu acikla.
9. Page fault tür ayrimi neden kritik?
10. OOM killer en son care neden olmalidir?

## Kume 9 - Bellek tasarim (C)

1. Memory pressure altinda politika gecis tasarla.
2. THP etkinlik heuristigi oner.
3. Reclaim loop icin guardrail metrikleri belirle.
4. zswap pool doluluk alarm politikasi ciz.
5. Heap butunluk kontrolu icin olay kaydi modeli yaz.

---

## Kume 10 - io_uring ve lock-free (A)

1. SQ ve CQ farki nedir?
2. Neden ring boyutu 2'nin kuvvetidir?
3. `RING_MASK` ne ise yarar?
4. Acquire ve Release neyi saglar?
5. `smp_wmb` neden gerekir?
6. `smp_rmb` neden gerekir?
7. Batch pop ne kazandirir?
8. Overflow sayaci neyi gosterir?
9. Producer/consumer rolleri nasil ayrilir?
10. Lock-free neden her zaman kolay degildir?

## Kume 11 - io_uring uygulama (B)

1. Tail erken publish edilirse hangi bug dogar?
2. Head guncellemesi gecikirse ne olur?
3. Volatile okuma/yazma neden kullanilmis olabilir?
4. Ring dolu durumda dogru davranis nedir?
5. Batch boyutu tuning'i nasil yapilir?
6. Memory ordering testlerini nasil yazarsin?
7. Single producer varsayimi nerede kritik?
8. Multi consumer senaryosu nasil bozulabilir?
9. ABI uyumlulugu neden testlenmeli?
10. CQ overflow artisi hangi soruna isaret eder?

## Kume 12 - io_uring tasarim (C)

1. MPMC ring'e gecis icin yeni ownership modeli tasarla.
2. Zero-copy lane eklersen hangi guard'lar gerekir?
3. fd ve user pointer dogrulama sinirini ciz.
4. Queue depth adaptif kontrol modeli oner.
5. Lock-free regression test corpusu planla.

---

## Kume 13 - Ag guvenlik temel (A)

1. TLS 1.3 neyi guvence altina alir?
2. QUIC neden UDP uzerinden calisir?
3. WireGuard'in ana fikri nedir?
4. HPACK Huffman neyi sikistirir?
5. Key schedule neden katmanlidir?
6. ACK range limiti neden konur?
7. Nonce replay korumasi neyi engeller?
8. EOS padding hatasi ne demektir?
9. Handshake state machine niye zorunlu?
10. Cipher suite secimi neyi etkiler?

## Kume 14 - Ag guvenlik uygulama (B)

1. TLS mesaj sirasini bozan bir senaryoda ne olur?
2. QUIC parser limitlerini kaldirmanin riski nedir?
3. WireGuard nonce yeniden kullanimi ne dogurur?
4. HPACK decode'da invalid padding neden fail-closed olmali?
5. QUIC stream coklamasi hangi problemi cozer?
6. TLS transcript hash neden kritik?
7. AEAD tag kontrolu yoksa ne olur?
8. QUIC ACK range cok buyuk olursa hangi kaynak tukenir?
9. WireGuard allowed_ips filtresi neden policy konusudur?
10. Handshake timeout modeli nasil secilir?

## Kume 15 - Ag guvenlik tasarim (C)

1. Cekirdek ag stack'i icin parser hardening checklisti yaz.
2. TLS/QUIC telemetry seti tasarla.
3. WireGuard session anahtar yenileme politikasi oner.
4. HPACK decode fuzz plani ciz.
5. Ag tarafi regression corpusunu siniflandir.

---

## Kume 16 - Karma final sorular (A+B)

1. Scheduler ve reclaim arasindaki dolayli etkilesimi acikla.
2. COW artisi scheduler latencyyi nasil etkileyebilir?
3. io_uring yuksek yukte bellek baskisini nasil tetikler?
4. QUIC trafik patlamasi hangi cekirdek metriklerini degistirir?
5. RT lane aktifken writeback budget niye onemlidir?
6. THP etkinligi io_uring throughput'a nasil etki eder?
7. MGLRU refault artisinda scheduler semptomu gorulur mu?
8. DNS/HPACK parser guard'lari memory pressure ile nasil baglanir?
9. WireGuard decrypt lane'i neden CPU-affinity isteyebilir?
10. Tek bir metric ile tum sistemi neden yonetemezsin?

## Kume 17 - Karma tasarim sorulari (C)

1. "Laptop sinifi" ve "sunucu sinifi" icin ayri policy profilleri tasarla.
2. Core alt sistemleri icin ortak alarm esikleri belirle.
3. Cilt 2'ye geciste hangi teknik borclar zorunlu kapanmali?
4. Fail-closed ilkesiyle performans arasinda karar matrisin ne olur?
5. Bir release oncesi "core readiness review" sablonu yaz.

---

## Cevaplama onerisi

Her soruda bu mini formati kullan:

1. Kisa cevap
2. Kod referansi
3. Risk ve mitigasyon

Bu format, ezber yerine muhendislik muhakemesi olusturur.
