# echOS Cilt 1 - Core Engineering

Bu cilt, echOS ile yeni tanisan ogrenciler icin yazildi.
Hedef: isletim sistemi cekirdegindeki ana kararlarini adim adim anlamak.

## Bu cildin kapsami

- Boot zinciri ve cekirdek acilis akisi
- Scheduler ailesi: RT, CFS, EEVDF, Deadline, work stealing, timing wheel
- Bellek cekirdegi: PMM, buddy, heap, page fault, COW, THP, reclaim, zswap
- Lock-free io_uring ring
- Ag core guvenlik akislari: TLS 1.3, QUIC, WireGuard, HPACK Huffman

## Calisma yontemi

Her hafta ayni sirayla ilerle:

1. 1 dakikada fikir
2. Kod haritasi (echOS dosyalari)
3. Algoritma otopsisi
4. Mini lab
5. Kisa quiz

## 14 haftalik yol haritasi

| Hafta | Konu | Ana dosyalar |
|---|---|---|
| 1 | echOS mimari resmi | `src/lib.rs`, `src/main.rs` |
| 2 | Boot yolu ve init | `src/main.rs`, `src/memory/frame_allocator.rs` |
| 3 | Scheduler girisi ve context switch | `src/task/scheduler.rs` |
| 4 | RT scheduler (FIFO/RR) | `src/task/rt_scheduler.rs` |
| 5 | CFS | `src/task/cfs.rs` |
| 6 | EEVDF + Deadline | `src/task/eevdf.rs`, `src/task/deadline.rs` |
| 7 | Work stealing + Timing wheel | `src/task/deque.rs`, `src/task/timer.rs` |
| 8 | PMM ve zone fallback | `src/memory/fibonacci_pmm.rs` |
| 9 | Fibonacci buddy + TLSF | `src/memory/fibonacci_buddy.rs`, `src/allocator/tlsf.rs` |
| 10 | Page fault, COW, THP | `src/memory/mod.rs` |
| 11 | MGLRU + reclaim + zswap | `src/memory/mglru.rs`, `src/memory/zswap.rs`, `src/memory/mod.rs` |
| 12 | Lock-free io_uring | `src/posix/io_uring_ring.rs` |
| 13 | TLS 1.3 + HPACK Huffman | `src/net/tls.rs`, `src/net/http2_huffman.rs` |
| 14 | QUIC + WireGuard + final proje | `src/net/quic.rs`, `src/net/wireguard.rs` |

---

## Hafta 1 - echOS mimari resmi

### 1 dakikada fikir

Bir cekirdek, ayni anda uc isi yapar:

- CPU zamanini dagitir (scheduler)
- Bellek sahipligini korur (memory manager)
- Cihaz ve ag ile guvenli veri tasir (I/O + net)

### Kod haritasi

- Moduller: `src/lib.rs`
- Giris noktasi: `src/main.rs`
- Gorev sistemi: `src/task/mod.rs`
- Bellek ana modul: `src/memory/mod.rs`

### Mini lab

1. `src/lib.rs` ac.
2. `pub mod task;`, `pub mod memory;`, `pub mod net;` satirlarini bul.
3. Kendi notuna "modul omurgasi" diyagrami ciz.

### Quiz

1. `src/main.rs` neden kritik bir dosyadir?
2. `src/lib.rs` neyi merkezden yonetir?
3. Scheduler ve memory manager birlikte neden okunmali?

---

## Hafta 2 - Boot yolu ve cekirdek init

### 1 dakikada fikir

Makine acildiginda cekirdek dogrudan calismaz.
Firmware/bootloader once bellek, ekran, map gibi bilgileri hazirlar.

![Boot Path](../figures/generated/boot-path.svg)

### Kod haritasi

- Giris ve platform init: `src/main.rs`
- Bootstrap frame allocator: `src/memory/frame_allocator.rs`

### Kod parcasi

```rust
fn init_platform_iommu() -> bool {
    let cpu_acpi_ok = ech_os::cpu::acpi::init();
    let iommu_tables_ok = ech_os::memory::init_iommu();
    let iommu_hw_ok = ech_os::drivers::iommu::init();
    iommu_tables_ok && iommu_hw_ok
}
```

### Algoritma otopsisi

- Secim: erken asamada map ve tablo dogrulamasi
- Ana risk: hatali init zinciri sonraki tum katmanlari bozar
- echOS mitigasyonu: ACPI, IOMMU table, IOMMU hardware adimlarini ayri kontrol eder

### Mini lab

1. `src/main.rs` icinde `init_platform_iommu` cagrilarini izle.
2. "init basarisiz" log satirlarini not et.
3. Hangi adim fail olursa hangi ozellik kisitlanir yaz.

### Quiz

1. Boot asamasinda neden cok erken hata ayiklama logu gerekir?
2. `frame_allocator` neden ayrica var?
3. IOMMU acilmazsa hangi sinif risk artar?

---

## Hafta 3 - Scheduler girisi ve context switch

### 1 dakikada fikir

Scheduler, CPU'yu birden fazla gorev arasinda bolusturen hakemdir.

![Scheduler Decision](../figures/generated/scheduler-decision.svg)

### Kod haritasi

- Ana scheduler: `src/task/scheduler.rs`
- Moduller: `src/task/mod.rs`

### Kod parcasi

```rust
fn choose_spawn_cpu(task: &Task) -> usize {
    let current_cpu = get_current_cpu_id() as usize;
    let cpu_limit = SMP_SCHEDULER.cpu_count.load(Ordering::Relaxed).max(1) as usize;
    let mut best_cpu = current_cpu.min(cpu_limit.saturating_sub(1));
    // en dusuk kuyruq yuku hedeflenir
    best_cpu
}
```

### Algoritma otopsisi

- Secim: queue-length tabanli CPU secimi
- Ana dezavantaj: yalnizca kuyruk uzunlugu her zaman gercek maliyeti yansitmaz
- echOS mitigasyonu: affinity ve online CPU filtreleri ile secim daraltilir

### Mini lab

1. `choose_spawn_cpu` akisini adim adim yaz.
2. `queued_task_count_usize` neyi olcer?
3. Kendi pseudo kodunla CPU secim fonksiyonu ciz.

### Quiz

1. Scheduler neden her tick karar vermek zorundadir?
2. Affinity maskesi secimi nasil sinirlar?
3. Idle task neden gereklidir?

---

## Hafta 4 - RT scheduler: SCHED_FIFO ve SCHED_RR

### 1 dakikada fikir

Gercek zamanli islerde hedef "ortalama" degil, "deadline kacirmamak"tir.

### Kod haritasi

- `src/task/rt_scheduler.rs`

### Kod parcasi

```rust
pub const RT_PRIO_MIN: i32 = 1;
pub const RT_PRIO_MAX: i32 = 99;
pub const RR_DEFAULT_TIMESLICE: u64 = 100;
```

### Algoritma otopsisi

- Secim: Linux uyumlu RT politika seti
- Ana dezavantaj: FIFO yanlis kullanilirsa dusuk oncelikli gorevler ac kalir
- echOS mitigasyonu: RR timeslice ve RT bandwidth kontrol siniri

### Mini lab

1. `RtTaskInfo::calculate_timeslice` fonksiyonunu oku.
2. Oncelik arttikca dilim nasil degisiyor not et.
3. Bir tablo ciz: prio 10, 50, 90 icin dilim degeri.

### Quiz

1. FIFO ve RR arasindaki temel fark nedir?
2. RT queue neden oncelik kovasi tutar?
3. RT policy ne zaman `Normal` policy'den once gelir?

---

## Hafta 5 - CFS ve vruntime

### 1 dakikada fikir

CFS'in ana cumlesi su: "en az CPU almis olani once calistir".

### Kod haritasi

- `src/task/cfs.rs`

### Kod parcasi

```rust
pub fn weight_to_vruntime(delta: u64, weight: u64) -> u64 {
    if weight == 0 {
        return delta;
    }
    (delta * CFS_NICE_0_WEIGHT) / weight
}
```

Matematik modeli:

\[
\Delta v = \frac{\Delta t \cdot NICE\_0\_WEIGHT}{weight}
\]

### Algoritma otopsisi

- Secim: adil paylasim icin vruntime
- Ana dezavantaj: wakeup ve interactive islerde adalet ile gecikme arasinda gerilim
- echOS mitigasyonu: `CFS_WAKEUP_GRANULARITY` ile asiri preemption engellenir

### Mini lab

1. `nice_to_weight` fonksiyonunu oku.
2. Iki gorev sec: agirlik 1024 ve 2048.
3. Ayni `delta` icin vruntime artis farkini hesapla.

### Quiz

1. CFS neden "min vruntime" gorevi secer?
2. Nice degeri agirligi nasil etkiler?
3. `min_vruntime` neyi stabil tutar?

---

## Hafta 6 - EEVDF ve Deadline (EDF + CBS)

### 1 dakikada fikir

EEVDF: eligible olanlar icinden en erken sanal deadline.
EDF: mutlak deadline en yakin gorev.

![EEVDF CFS EDF Compare](../figures/generated/eevdf-cfs-edf-compare.svg)

### Kod haritasi

- EEVDF: `src/task/eevdf.rs`
- Deadline: `src/task/deadline.rs`

### Kod parcasi 1 (EEVDF)

```rust
pub fn update_runtime(&self, delta_ns: u64, rq_vtime: u64) {
    let delta_v = delta_ns.saturating_mul(1024) / self.weight.max(1);
    let vr = self.vruntime.fetch_add(delta_v, Ordering::SeqCst) + delta_v;
    let lag = rq_vtime as i64 - vr as i64;
    let eligible = if lag >= 0 { rq_vtime } else { vr };
    self.virtual_deadline.store(eligible.saturating_add(self.slice_ns.load(Ordering::Relaxed).max(1)), Ordering::SeqCst);
}
```

### Kod parcasi 2 (Deadline)

```rust
fn compute_bandwidth(&self, task: &DeadlineTask) -> u64 {
    let runtime = task.runtime.load(Ordering::Relaxed);
    let period = task.period;
    if period == 0 { return 0; }
    (runtime * 10000) / period
}
```

Matematik modeli:

\[
U_i = \frac{C_i}{T_i}, \quad \sum U_i \le 1
\]

### Algoritma otopsisi

- Secim: latency + fairness icin EEVDF, hard deadline icin EDF/CBS
- Ana dezavantaj: admission ve budget ayari yanlis olursa throttle siklasir
- echOS mitigasyonu: bandwidth kontrol ve `replenish` dongusu

### Mini lab

1. EEVDF icin 3 gorevli kucuk tablo yap: `lag`, `eligible`, `virtual_deadline`.
2. Deadline icin `runtime=2`, `period=10` ise `U` hesapla.
3. `check_replenishments` adimini ciz.

### Quiz

1. EEVDF `eligible` kavrami neden ekler?
2. EDF hangi kosulda teorik olarak optimaldir?
3. CBS hangi problemi cozer?

---

## Hafta 7 - Work stealing deque ve Timing wheel

### 1 dakikada fikir

Bir CPU dolu, digeri bos olabilir. Work stealing bu dengesizligi duzeltir.
Timing wheel ise milyonlarca uyuyan gorevi O(1) amortized yonetir.

![Timing Wheel Cascade](../figures/generated/timing-wheel-cascade.svg)

### Kod haritasi

- Deque: `src/task/deque.rs`
- Timer: `src/task/timer.rs`

### Kod parcasi 1 (Deque)

```rust
pub fn steal(&self) -> Option<Box<T>> {
    let t = self.inner.top.load(Ordering::Acquire);
    core::sync::atomic::fence(Ordering::SeqCst);
    let b = self.inner.bottom.load(Ordering::Acquire);
    if t < b {
        let idx = (t as usize) % DEQUE_SIZE;
        let task_ptr = self.inner.buffer[idx].load(Ordering::Relaxed);
        if self.inner.top.compare_exchange(t, t.wrapping_add(1), Ordering::SeqCst, Ordering::Relaxed).is_ok() {
            return Some(unsafe { Box::from_raw(task_ptr) });
        }
    }
    None
}
```

### Kod parcasi 2 (Timing Wheel)

```rust
if diff < WHEEL_SIZE {
    let idx = wake_tick & WHEEL_MASK;
    self.wheels[0][idx].push_back(task);
} else if diff < 1 << (2 * WHEEL_BITS) {
    let idx = (wake_tick >> WHEEL_BITS) & WHEEL_MASK;
    self.wheels[1][idx].push_back(task);
}
```

### Algoritma otopsisi

- Secim: Chase-Lev deque + hiyerarsik wheel
- Ana dezavantaj: lock-free kodda ordering hatasi zor bulunur
- echOS mitigasyonu: Acquire/Release/SeqCst sinirlari acik satirlarda uygulanir

### Mini lab

1. Deque icin `push/pop/steal` siralarini tek tabloya yaz.
2. Timing wheel icin `wake_tick` 30, 300, 70000 degerlerinde level secimini hesapla.
3. CAS yarisi kaybedilince neden `None` donduruldugunu acikla.

### Quiz

1. Neden owner `pop` sondan, stealer bastan alir?
2. Timing wheel neden tek linked-list taramaya gore ustundur?
3. Memory fence kaldirilirsa ne riski dogar?

---

## Hafta 8 - PMM ve zone fallback

### 1 dakikada fikir

Tum fiziksel bellek her cihaz icin esit degildir.
Eski DMA cihazlari sadece dusuk adres bolgesine erisebilir.

![Memory Zones Fallback](../figures/generated/memory-zones-fallback.svg)

### Kod haritasi

- `src/memory/fibonacci_pmm.rs`

### Kod parcasi

```rust
fn fallback(self) -> Option<MemoryZone> {
    match self {
        MemoryZone::Normal => Some(MemoryZone::Dma32),
        MemoryZone::Dma32 => Some(MemoryZone::Dma),
        MemoryZone::Dma => None,
    }
}
```

```rust
pub fn allocate_from_zone(&mut self, zone: MemoryZone) -> Option<PhysFrame> {
    if let Some(frame) = self.try_allocate_zone(zone) {
        return Some(frame);
    }
    let mut fallback = zone.fallback();
    while let Some(fz) = fallback {
        if let Some(frame) = self.try_allocate_zone(fz) {
            return Some(frame);
        }
        fallback = fz.fallback();
    }
    None
}
```

### Algoritma otopsisi

- Secim: zone-aware tahsis + fallback zinciri
- Ana dezavantaj: fallback artis oranlari bellek baskisini gizleyebilir
- echOS mitigasyonu: zone bazli istatistik tutulur

### Mini lab

1. `MemoryZone::from_addr` esiklerini not et.
2. Bir cihazin `DMA32` istegi basarisizsa takip eden yolu yaz.
3. `zone_stats` ile hangi metriği izlemen gerektigini belirle.

### Quiz

1. DMA zone neden vardir?
2. Neden dogrudan `NORMAL` disina cikis gerekir?
3. Fallback zinciri bitince hangi durum olusur?

---

## Hafta 9 - Fibonacci buddy ve TLSF heap

### 1 dakikada fikir

Buddy sistemi fiziksel bloklari boler/birlestirir.
TLSF ise heap icinde O(1) sinif secimi yapar.

![Fibonacci Split Coalesce](../figures/generated/fibonacci-buddy-split-coalesce.svg)

### Kod haritasi

- Buddy: `src/memory/fibonacci_buddy.rs`
- Heap: `src/allocator/tlsf.rs`

### Kod parcasi (Buddy)

```rust
fn find_buddy(&self, addr: PhysAddr, idx: usize) -> PhysAddr {
    let block_size = FIBONACCI_SERIES[idx];
    let offset_pages = (addr.as_u64() - self.base_address.as_u64()) / PAGE_SIZE as u64;
    let buddy_offset_pages = offset_pages ^ (block_size as u64);
    PhysAddr::new(self.base_address.as_u64() + buddy_offset_pages * PAGE_SIZE as u64)
}
```

### Kod parcasi (TLSF katmani)

```rust
match tlsf.allocate(layout) {
    Some(ptr) => ptr.as_ptr(),
    None => core::ptr::null_mut(),
}
```

### Algoritma otopsisi

- Secim: PMM icin Fibonacci buddy, heap icin TLSF
- Ana dezavantaj: buddy split/coalesce patikasi ve TLSF metadata disiplini dikkat ister
- echOS mitigasyonu: canary, allocation tracker, heap boundary kontrolleri

### Mini lab

1. Buddy icin `F(6)=13` bloktan `3` sayfa tahsisini kagit ustunde bol.
2. TLSF dosyasinda `HEAP_CANARY_MAGIC` kullanim amacini yaz.
3. `is_valid_heap_ptr` kontrolunun neyi kapattigini acikla.

### Quiz

1. Fibonacci boyut dizisi ne kazandirir?
2. Coalesce neden recursive?
3. Heap canary bozulursa hangi sinif hatadan suphelenirsin?

---

## Hafta 10 - Page fault, COW ve THP

### 1 dakikada fikir

Page fault her zaman hata degildir; bazen lazy allocation'in normal adimidir.

![Page Fault COW THP](../figures/generated/page-fault-cow-thp.svg)

### Kod haritasi

- `src/memory/mod.rs`

### Kod parcasi 1

```rust
pub fn handle_user_page_fault(addr: u64, error: PageFaultErrorCode) -> bool {
    let aligned = addr & !(PAGE_SIZE as u64 - 1);
    if !is_user_address(aligned) { return false; }
    if error.contains(PageFaultErrorCode::PROTECTION_VIOLATION) {
        if error.contains(PageFaultErrorCode::CAUSED_BY_WRITE) {
            return handle_cow_fault(aligned);
        }
        return false;
    }
    handle_lazy_fault(aligned)
}
```

### Kod parcasi 2

```rust
fn try_map_thp_anon(...) -> bool {
    if region.shared || region.cow { return false; }
    // 2MiB huge page map
    // map result basarisizsa 4KiB frame'leri geri ver
    true
}
```

### Kod parcasi 3

```rust
fn handle_cow_fault(addr: u64) -> bool {
    // refcount > 1 ise yeni frame ayir, eskiyi kopyala, writable remap et
    true
}
```

### Algoritma otopsisi

- Secim: lazy fault + COW + THP
- Ana dezavantaj: fault patikasi gecikme ve karma davranis uretebilir
- echOS mitigasyonu: split/rollback kontrolleri, refcount ve map sonucuna gore fail-closed

### Mini lab

1. `handle_user_page_fault` karar agacini ciz.
2. COW yazma fault adimlarini 7 satirlik akisa indir.
3. THP map fail olursa hangi geri alma adimi var not et.

### Quiz

1. COW neden memory tasarrufu saglar?
2. THP hangi kosulda map edilmez?
3. Lazy fault ve protection fault nasil ayrilir?

---

## Hafta 11 - MGLRU, reclaim ve zswap

### 1 dakikada fikir

Bellek baskisinda "hangi sayfayi atalim" sorusu performansi belirler.
MGLRU bunu nesil bazli yapar; zswap diskten once RAM icinde sikistirma dener.

![MGLRU Reclaim ZSwap](../figures/generated/mglru-reclaim-zswap.svg)

### Kod haritasi

- MGLRU: `src/memory/mglru.rs`
- zswap: `src/memory/zswap.rs`
- reclaim daemon: `src/memory/mod.rs`

### Kod parcasi 1 (MGLRU)

```rust
const MGLRU_GENERATIONS: u64 = 8;
const HOT_REF_THRESHOLD: u16 = 3;
const COLD_EVICTION_AGE: u64 = 2;
```

### Kod parcasi 2 (kswapd)

```rust
fn memory_reclaim_daemon() -> ! {
    loop {
        damon::age(now);
        mglru::age_generations(now);
        if should_reclaim_background() {
            reclaim_pages_global(KSWAPD_RECLAIM_BATCH);
            process_writeback_budget(WRITEBACK_BUDGET_FAST);
        }
    }
}
```

### Kod parcasi 3 (writeback)

```rust
fn process_writeback_budget(budget: usize) -> usize {
    // queue'dan al, token kontrol et, disk writeback yap
    done
}
```

### Algoritma otopsisi

- Secim: generation tabanli reclaim + zswap pipeline
- Ana dezavantaj: yanlis tunel oranlari CPU sikistirma maliyetini artirabilir
- echOS mitigasyonu: budget tabanli writeback ve pressure sinyali ile dongu

### Mini lab

1. MGLRU victim secim kriterini yaz.
2. zswap akisini 4 adimda ciz.
3. kswapd dongusunde hangi metriklerle reclaim tetikleniyor not et.

### Quiz

1. Refault promotion neden gerekli?
2. zswap ile zram arasindaki fark nedir?
3. Writeback budget neden tek adim degil dongu?

---

## Hafta 12 - Lock-free io_uring

### 1 dakikada fikir

io_uring ile kullanici ve cekirdek iki ring uzerinden konusur:

- SQ: is istegi
- CQ: tamamlanma sonucu

![io_uring Lock Free](../figures/generated/io-uring-lockfree.svg)

### Kod haritasi

- `src/posix/io_uring_ring.rs`

### Kod parcasi

```rust
pub fn push(&self, sqe: RingSqe) -> Result<u32, ()> {
    // 1) entry yaz
    // 2) smp_wmb
    // 3) tail Release
    Ok(index as u32)
}

pub fn pop(&self) -> Option<RingSqe> {
    // 1) tail Acquire
    // 2) smp_rmb
    // 3) entry oku
    // 4) head Release
    Some(sqe)
}
```

### Algoritma otopsisi

- Secim: mutex yerine atomics + barrier
- Ana dezavantaj: ordering bug'lari testte gec yakalanabilir
- echOS mitigasyonu: `smp_wmb/smp_rmb` ve head/tail publication siniri acik korunur

### Mini lab

1. SQ producer/consumer rollerini tek tabloya yaz.
2. `push` icinde bariyer sirasi degisirse ne olur sorusuna cevap ver.
3. `pop_batch` neden amortized kazanc verir not et.

### Quiz

1. Neden ring boyutu 2'nin kuvveti secilir?
2. `RING_MASK` neyi hizlandirir?
3. Acquire/Release ciftinin gorevi nedir?

---

## Hafta 13 - TLS 1.3 ve HPACK Huffman

### 1 dakikada fikir

TLS 1.3, baglantiyi sifreli ve dogrulanmis hale getirir.
HPACK Huffman, HTTP/2 header verisini daha az byte ile tasir.

![TLS13 Handshake](../figures/generated/tls13-handshake.svg)
![HPACK Huffman Decode](../figures/generated/hpack-huffman-decode.svg)

### Kod haritasi

- TLS: `src/net/tls.rs`
- Huffman: `src/net/http2_huffman.rs`

### Kod parcasi 1 (TLS key schedule)

```rust
let hkdf = Hkdf::<Sha256>::new(Some(&derived_secret), shared_secret);
hkdf.expand(b"", &mut handshake_secret).ok();
```

### Kod parcasi 2 (HPACK decode)

```rust
for bit in BitIterator::new(buf.iter()) {
    // current code topla, table'da match ara
    // EOS ise hata, symbol ise output'a yaz
}
```

### Algoritma otopsisi

- Secim: TLS 1.3 HKDF zinciri + tablo tabanli Huffman decode
- Ana dezavantaj: state machine gecisleri ve padding kurallari karmasik olabilir
- echOS mitigasyonu: enum tabanli handshake tipleri ve padding/EOS fail-closed hatalari

### Mini lab

1. TLS handshake mesajlarini sira ile yaz.
2. Huffman decode icin `InvalidPadding` hangi durumda gelir acikla.
3. `CipherSuite` seciminin key length uzerindeki etkisini not et.

### Quiz

1. HKDF neden katmanli kullanilir?
2. TLS 1.3 ile onceki surumler arasinda en buyuk fark nedir?
3. HPACK decode neden bit seviyesinde ilerler?

---

## Hafta 14 - QUIC, WireGuard ve final proje

### 1 dakikada fikir

QUIC, UDP uzerinde cok akisli ve hizli bir tasima modeli sunar.
WireGuard, daha dar ama guvenli VPN tunnel modeli sunar.

![QUIC Flow](../figures/generated/quic-flow.svg)
![WireGuard Handshake](../figures/generated/wireguard-handshake.svg)

### Kod haritasi

- QUIC: `src/net/quic.rs`
- WireGuard: `src/net/wireguard.rs`

### Kod parcasi 1 (QUIC ACK guard)

```rust
const MAX_ACK_RANGES: u64 = 256;
if ack_range_count > MAX_ACK_RANGES {
    return None;
}
```

### Kod parcasi 2 (WireGuard replay guard)

```rust
if session.receiving_nonce != WG_NONCE_UNINITIALIZED && nonce <= session.receiving_nonce {
    return Err(WgError::ReplayDetected);
}
```

### Algoritma otopsisi

- Secim: QUIC varint/frame modeli + WireGuard nonce/replay kontrolu
- Ana dezavantaj: protokol durum makinesi buyudukce parser ve state bug riski artar
- echOS mitigasyonu: frame tiplerini enum ile ayrim, boyut ve tekrar kontrol sinirlari

### Mini lab

1. QUIC frame encode/decode adimlarini kendi ciziminle goster.
2. WireGuard nonce tekrarinda neden paket reddedilir acikla.
3. `allowed_ips` filtresini hangi katmanda uyguladigini bul.

### Quiz

1. QUIC neden TCP'ye gore farkli davranir?
2. WireGuard neden kucuk kod tabaniyla bilinir?
3. Replay korumasi olmazsa ne olur?

---

## Donem sonu mini proje

Hedef: Bu ciltteki tum konseptleri bir test senaryosunda birlestirmek.

### Proje gorevi

- Senaryo: 4 CPU, karisik RT + normal gorev yukleri, bellek baskisi, ag trafigi
- Beklenen cikti:
  - Scheduler secim davranisinin log analizi
  - Reclaim ve zswap etkisinin metrik tablosu
  - io_uring gecikme olcumu
  - TLS/QUIC/WireGuard state gecis ozeti

### Teslim paketi

- 1 teknik rapor (6-10 sayfa)
- 1 metrik tablosu
- 1 hata analizi bolumu

---

## Sinav hazirlik kontrol listesi

- CFS vruntime denklemini aciklayabiliyor musun?
- EEVDF `eligible_vtime` mantigini ornekle gosterebiliyor musun?
- EDF admission mantiginda `U=C/T` yorumlayabiliyor musun?
- Work stealing deque'de CAS yarisi sonucunu aciklayabiliyor musun?
- COW fault akisini 6 adimda yazabiliyor musun?
- MGLRU victim secimini nesil ve hot score ile yorumlayabiliyor musun?
- io_uring `smp_wmb` ve `smp_rmb` sirasini dogru cizebiliyor musun?
- TLS key schedule ve QUIC ACK range sinirini nedenleriyle anlatabiliyor musun?
