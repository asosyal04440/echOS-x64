# Cilt 1 Kod Yuruyus Rehberi

Bu bolumde, echOS cekirdek kodu uzerinden sistematik kod yuruyus adimlari verilir.
Her yuruyus dogrudan kaynak dosya ve karar noktasi eslestirmesi uzerinden ilerler.


Bu bolumde kod yuruyuslerinin ayrintili uygulama adimlari verilir.
Her yuruyus, dogrudan echOS kaynak dosyasi uzerinde ilerler.

## Kod Yuruyus formati

Her yuruyus su sabit formatta gelir:

1. Hedef
2. On bilgi
3. Adim adim uygulama
4. Beklenen cikti
5. Hata avi gorevi
6. Degerlendirme rubrigi

---

## Kod Yuruyus 1 - Cekirdek modul haritalama

### Hedef

`src/lib.rs` uzerinden cekirdek modul omurgasini cikarmak.

### Adimlar

1. `src/lib.rs` dosyasini ac.
2. Tum `pub mod` satirlarini siniflandir:
   - execution core
   - memory/storage
   - io/net
   - compatibility/runtime
3. Kendi modul agacini ciz.

### Beklenen cikti

- En az 4 ana kategori
- Her kategori icin 3+ modul

---

## Kod Yuruyus 2 - Boot yolunda failure analizi

### Hedef

`src/main.rs` icindeki init adimlarinda fail-closed davranisi anlamak.

### Adimlar

1. `init_platform_iommu` fonksiyonunu bul.
2. Uc adimi ayir:
   - ACPI init
   - IOMMU tablo init
   - IOMMU hardware init
3. Her adim basarisiz olursa hangi semptom cikar yaz.

### Beklenen cikti

- 3 adimli hata matrisi

---

## Kod Yuruyus 3 - Scheduler secim izi

### Hedef

Scheduler karar agacini kaynak koddan tekrar uretmek.

### Adimlar

1. `src/task/scheduler.rs` ac.
2. RT oncelik, local queue, steal, idle sirasini cikar.
3. 4 farkli senaryo yaz:
   - RT var
   - RT yok, local dolu
   - local bos, remote dolu
   - tumu bos

### Beklenen cikti

- Senaryo->secim tablosu

---

## Kod Yuruyus 4 - RT policy tuning

### Hedef

RR dilimlerinin oncelige gore degisimini incelemek.

### Adimlar

1. `src/task/rt_scheduler.rs` icinde `calculate_timeslice` fonksiyonunu oku.
2. Prio 5, 30, 60, 90 icin ornek dilim hesapla.
3. Kucuk tablo olustur.

### Beklenen cikti

- Oncelik/dilim tablosu

---

## Kod Yuruyus 5 - CFS vruntime deneyi

### Hedef

Vruntime formulunu sayisal ornekle dogrulamak.

### Adimlar

1. `src/task/cfs.rs` icinde `weight_to_vruntime` fonksiyonunu bul.
2. Ayni `delta` icin iki agirlik sec.
3. Hangi task daha hizli vruntime biriktiriyor hesapla.

### Beklenen cikti

- Hesap adimlari ve yorum

---

## Kod Yuruyus 6 - EEVDF eligibility tablosu

### Hedef

`lag`, `eligible_vtime`, `virtual_deadline` iliskisini kavramak.

### Adimlar

1. `src/task/eevdf.rs` icinde `update_runtime` fonksiyonunu satir satir oku.
2. Uc task icin varsayim degerleri belirle.
3. Her task icin `lag` ve `vd` hesap tablosu olustur.

### Beklenen cikti

- Elle hesaplanmis EEVDF secim sirasi

---

## Kod Yuruyus 7 - Deque race walkthrough

### Hedef

Son eleman yarisinda `CAS` davranisini anlamak.

### Adimlar

1. `src/task/deque.rs` icinde `pop` ve `steal` fonksiyonlarini oku.
2. Son eleman oldugu durumda iki thread senaryosu ciz.
3. CAS kazanma/kaybetme akislarini ayri yaz.

### Beklenen cikti

- Yarisa ait zaman cizelgesi

---

## Kod Yuruyus 8 - Timing wheel seviye secimi

### Hedef

Uyuma surelerine gore wheel seviyesini dogru secmek.

### Adimlar

1. `src/task/timer.rs` icinde `schedule` fonksiyonunu bul.
2. `diff` degerleri icin level sec:
   - 12
   - 320
   - 80_000
   - 20_000_000
3. Cascade anlarini not et.

### Beklenen cikti

- Diff->level tablosu

---

## Kod Yuruyus 9 - Zone fallback deney tasarimi

### Hedef

Zone fallback zincirinin etkisini olcmek.

### Adimlar

1. `src/memory/fibonacci_pmm.rs` icinde `allocate_from_zone` akisini cikar.
2. Farkli istek tipleri icin fallback yolu ciz.
3. Zone istatistiklerinin neyi gosterecegini yaz.

### Beklenen cikti

- Zone baski raporu sablonu

---

## Kod Yuruyus 10 - Buddy split/coalesce izleme

### Hedef

Fibonacci split ve coalesce adimlarini adres seviyesinde takip etmek.

### Adimlar

1. `src/memory/fibonacci_buddy.rs` icinde `split_block` ve `try_coalesce` fonksiyonlarini oku.
2. F(6)=13 bloktan 3 sayfa tahsisi senaryosu ciz.
3. Sonra serbest birakip birlesme adimlarini goster.

### Beklenen cikti

- Adres ve boyut tablosu

---

## Kod Yuruyus 11 - COW fault yurutme izi

### Hedef

COW fault aninda refcount kararini anlamak.

### Adimlar

1. `src/memory/mod.rs` icinde `handle_user_page_fault` ve `handle_cow_fault` fonksiyonlarini ac.
2. Refcount=1 ve Refcount>1 icin iki akis ciz.
3. Hangi adimda kopya alindigini not et.

### Beklenen cikti

- Iki farkli COW akisi

---

## Kod Yuruyus 12 - Reclaim ve writeback butce deneyi

### Hedef

`kswapd` dongusunde reclaim ve writeback butce davranisini okumak.

### Adimlar

1. `src/memory/mod.rs` icinde `memory_reclaim_daemon` ve `process_writeback_budget` fonksiyonlarini oku.
2. Budget kucuk/buyuk oldugunda beklenen davranisi yaz.
3. MGLRU ve zswap baglantisini tabloya koy.

### Beklenen cikti

- pressure->reclaim->writeback akis tablosu

---

## Kod Yuruyus 13 - io_uring publication boundary

### Hedef

SQ/CQ ring'de dogru publication sirasini anlamak.

### Adimlar

1. `src/posix/io_uring_ring.rs` icinde `push/pop` ciftlerini bul.
2. `smp_wmb` ve `smp_rmb` noktalarini isaretle.
3. "Yanlis siralama" alternatifi yaz ve etkisini tartis.

### Beklenen cikti

- Producer/consumer siralama cizelgesi

---

## Kod Yuruyus 14 - Net parser ve state machine guvenligi

### Hedef

TLS/QUIC/WireGuard/HPACK alanlarinda guard mekanizmalarini bulmak.

### Adimlar

1. `src/net/tls.rs` icinde handshake sirasini cikar.
2. `src/net/quic.rs` icinde ACK range limitini bul.
3. `src/net/wireguard.rs` icinde replay guard satirlarini bul.
4. `src/net/http2_huffman.rs` icinde padding/EOS hatalarini not et.

### Beklenen cikti

- Guard listesi + hangi saldiri sinifini azalttigi

---
