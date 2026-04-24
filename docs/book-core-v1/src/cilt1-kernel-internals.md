# echOS Internal - Core Kernel Engineering

Bu ciltin amaci, bir ders notu ozetinden ziyade dogrudan cekirdek ic tasarimi anlatmaktir.
Metin, echOS kod tabanindaki karar sinirlarini, trade-off'lari ve failure-path davranisini
tek tek aciklar.

Bu kitapta ana eksen su: "Bu alt sistem neden boyle yazildi ve hangi kosulda bozulur?"

## 1. Cilt kapsam siniri

Bu ciltte yalnizca core lane yer alir:

- boot, erken init, platform capability aktivasyonu
- scheduler policy ailesi (RT, CFS, EEVDF, Deadline)
- SMP work-stealing ve timer altyapisi
- fiziksel/virtual bellek yonetimi, COW, THP, reclaim, zswap
- lock-free io_uring ring
- TLS 1.3, QUIC, WireGuard, HPACK decode

Bu sinir bilerek dar tutulur. Cihaz suruculeri ve ust katman runtime/uyumluluk lane'i
ayri ciltte ele alinacaktir.

---

## 2. Boot zinciri ve erken karar noktasi

Kernelin en pahali hatalari genelde en erken asamada ekilir.
Bu nedenle boot path sadece "acildi" gibi ikili bir kriterle olculmez.

Asil kriter:

- capability tablosu tutarli mi?
- bellek araligi sahipligi dogru mu?
- erken map/flag kurallari fail-closed mu?

### 2.1 Giris contract'i

`src/main.rs` icindeki init akisi, dogrudan capability aktivasyon zinciri uzerinden ilerler.
`init_platform_iommu` adimi bu zincirde kritik bir publication point'tir.

```rust
fn init_platform_iommu() -> bool {
    let cpu_acpi_ok = ech_os::cpu::acpi::init();
    let iommu_tables_ok = ech_os::memory::init_iommu();
    let iommu_hw_ok = ech_os::drivers::iommu::init();
    iommu_tables_ok && iommu_hw_ok
}
```

Bu fonksiyonun varlik nedeni basit bir bool donmek degildir.
Asil amac, "tablo hazir ama cihaz kapali" gibi yarim durumlari explicit ayirmaktir.

### 2.2 Early memory ownership

`src/memory/frame_allocator.rs` bootstrap lane'i,
ana PMM devreye girmeden once gecici ama guvenli tahsis saglar.

Buradaki temel invariant:

- kernel image fiziksel araligi bir daha tahsis edilmez

Bu invariant bozulursa semptom cogu zaman gec gelir:

- random crash
- context switch sonrasi bozulma
- paging yapisinda gecikmeli kirilma

### 2.3 Worst-case-first notu

Boot lane'de en tehlikeli durum "hard fail" degil "silent degrade"dir.
Bu nedenle fail-open yerine fail-closed davranis tercih edilir.

---

## 3. Scheduler omurgasi: policy orkestrasyonu

Tek policy ile tum workload siniflari optimize edilemez.
echOS bu nedenle policy ailesi kullanir ve secimi cekirdek scheduler lane'inde yapar.

Kod baglami:

- `src/task/scheduler.rs`
- `src/task/rt_scheduler.rs`
- `src/task/cfs.rs`
- `src/task/eevdf.rs`
- `src/task/deadline.rs`

### 3.1 Per-CPU queue secimi

Spawn lane'inde CPU secimi `choose_spawn_cpu` ile yapilir.

```rust
fn choose_spawn_cpu(task: &Task) -> usize {
    let current_cpu = get_current_cpu_id() as usize;
    let cpu_limit = SMP_SCHEDULER.cpu_count.load(Ordering::Relaxed).max(1) as usize;
    let mut best_cpu = current_cpu.min(cpu_limit.saturating_sub(1));
    let mut best_load = queued_task_count_usize(best_cpu);
    // online + affinity + load karsilastirma
    best_cpu
}
```

Bu secim, mutlak optimum iddiasi tasimaz.
Pratik hedef, queue skew'i sinirlayarak tail latency patlamasini azaltmaktir.

Model:

\[
Skew = \max_i q_i - \min_i q_i
\]

Skew buyudukce cross-core calma zorunlulugu artar.

### 3.2 RT lane (FIFO / RR)

`src/task/rt_scheduler.rs` icinde RT lane
oncelik kovalarini `BTreeMap<i32, Vec<Box<Task>>>` ile tutar.

RT lane'in dogasi geregi iki risk vardir:

1. starvation
2. budget asimi

Mitigasyon satirlari:

- `RR_DEFAULT_TIMESLICE`, `RR_MIN_TIMESLICE`, `RR_MAX_TIMESLICE`
- `rt_runtime`, `rt_period`, `rt_runtime_enabled`

Bu sayede RT lane tum CPU'yu limitsiz yutamaz.

### 3.3 CFS lane (vruntime)

CFS'in cekirdek denklemi kodda acik:

```rust
pub fn weight_to_vruntime(delta: u64, weight: u64) -> u64 {
    if weight == 0 {
        return delta;
    }
    (delta * CFS_NICE_0_WEIGHT) / weight
}
```

Formul:

\[
\Delta v = \frac{\Delta t \cdot W_0}{w_i}
\]

`w_i` buyudukce ayni calisma suresinde daha az vruntime yazilir.
Bu da daha yuksek pay anlamina gelir.

Risk:

- wakeup-heavy interaktif lane'de asiri preemption

Mitigasyon:

- `CFS_WAKEUP_GRANULARITY`
- `min_vruntime` clamp

### 3.4 EEVDF lane

`src/task/eevdf.rs` lane'i CFS'e eligibility boyutu ekler.

```rust
pub fn update_runtime(&self, delta_ns: u64, rq_vtime: u64) {
    let delta_v = delta_ns.saturating_mul(1024) / self.weight.max(1);
    let vr = self.vruntime.fetch_add(delta_v, Ordering::SeqCst) + delta_v;
    let lag = rq_vtime as i64 - vr as i64;
    let eligible = if lag >= 0 { rq_vtime } else { vr };
    self.eligible_vtime.store(eligible, Ordering::SeqCst);
    self.virtual_deadline.store(eligible.saturating_add(slice), Ordering::SeqCst);
}
```

Bu lane'in avantaji wakeup kararinda daha iyi discriminasyon saglamasidir.

Risk:

- `slice_ns` tuning'i hataliysa ya latency ya throughput lane'i bozulur

### 3.5 Deadline lane (EDF + CBS)

`src/task/deadline.rs` admission kontrolu ile gelir:

```rust
fn compute_bandwidth(&self, task: &DeadlineTask) -> u64 {
    let runtime = task.runtime.load(Ordering::Relaxed);
    let period = task.period;
    if period == 0 { return 0; }
    (runtime * 10000) / period
}
```

Temel kosul:

\[
\sum_i \frac{C_i}{T_i} \le 1
\]

CBS replenish dongusu budget asimi yapan taski throttle eder,
period sonu yeniden acarak sistem canliligini korur.

### 3.6 Policy arbitration notu

Policy'ler birbiriyle yarismiyor; orkestre ediliyor.
Asil tasarim basarisi policy secim sirasinin deterministik olmasidir.

---

## 4. SMP work-stealing: Chase-Lev deque

Kod baglami: `src/task/deque.rs`

Owner lane:

- `push` ve `pop` alt uctan

Stealer lane:

- `steal` ust uctan

Bu ayrim cache locality + contention azaltimi birlikte getirir.

### 4.1 Son eleman yarisi

`pop` ve `steal` ayni anda son elemana gelebilir.
Kod bunu `compare_exchange` ile cozer.

```rust
if self.inner.top.compare_exchange(t, t.wrapping_add(1), Ordering::SeqCst, Ordering::Relaxed).is_ok() {
    return Some(unsafe { Box::from_raw(task_ptr) });
}
```

Yanlis ordering secimi burada sessiz bozulma uretebilir.

### 4.2 Kilit yoksa garanti yok mu?

Var; ama garantiyi lock degil memory model verir.
Bu nedenle lock-free lane'de test disiplini daha katidir.

Minimum test seti:

- steal/pop race stress
- empty/full boundary fuzz
- ABA benzeri pointer reuse taramasi

---

## 5. Timer altyapisi: hiyerarsik timing wheel

Kod baglami: `src/task/timer.rs`

Bu lane'in hedefi, sleeping task yonetimini lineer taramadan cikarmaktir.

Temel kural:

- kisa sureler alt seviyede
- uzun sureler ust seviyede
- wrap aninda cascade

```rust
if diff < WHEEL_SIZE {
    self.wheels[0][idx].push_back(task);
} else if diff < 1 << (2 * WHEEL_BITS) {
    self.wheels[1][idx].push_back(task);
}
```

Amortized model:

\[
T_{insert} \approx O(1), \quad T_{tick} \approx O(1)
\]

Risk:

- cascade unutulursa wakeup drift birikir

---

## 6. PMM: zone-aware fiziksel bellek tahsisi

Kod baglami: `src/memory/fibonacci_pmm.rs`

Zone model:

- DMA
- DMA32
- NORMAL

Fallback zinciri kodda explicit:

```rust
fn fallback(self) -> Option<MemoryZone> {
    match self {
        MemoryZone::Normal => Some(MemoryZone::Dma32),
        MemoryZone::Dma32 => Some(MemoryZone::Dma),
        MemoryZone::Dma => None,
    }
}
```

Bu tasarim cihaz sinirlari nedeniyle zorunludur; secim degildir.

### 6.1 Fallback economics

Sik fallback iki seyi ayni anda soyler:

1. ust zone baskida
2. kapasite dagilimi hedef workload'a uymuyor

Bu nedenle fallback sayaci sadece log degil,
reclaim tetigine girdi olmalidir.

---

## 7. Fibonacci buddy allocator

Kod baglami: `src/memory/fibonacci_buddy.rs`

Kritik fonksiyon uclusu:

- `find_buddy`
- `split_block`
- `try_coalesce`

```rust
fn find_buddy(&self, addr: PhysAddr, idx: usize) -> PhysAddr {
    let block_size = FIBONACCI_SERIES[idx];
    let offset_pages = (addr.as_u64() - self.base_address.as_u64()) / PAGE_SIZE as u64;
    let buddy_offset_pages = offset_pages ^ (block_size as u64);
    PhysAddr::new(self.base_address.as_u64() + buddy_offset_pages * PAGE_SIZE as u64)
}
```

Buradaki XOR tabanli buddy hesaplama, yanlis implementasyonlarda
en kolay bozulacak noktadir.

Risk sinifi:

- leak
- overlap allocation
- coalesce cikmazi

---

## 8. TLSF wrapper ve heap butunlugu

Kod baglami: `src/allocator/tlsf.rs`

echOS lane'i saf allocator cagrisiyla yetinmez.
Guvenlik sarmalasi ekler:

- early heap / main heap ayrimi
- heap range dogrulamasi
- canary takibi (`HEAP_CANARY_MAGIC`)
- corruption sayaci

Bu lane'de karar su:

- bir miktar overhead kabul et
- sessiz heap bozulmasini fail-closed yakala

Bu cekirdek kodu icin dogru trade-off'tur.

---

## 9. User page fault pipeline

Kod baglami: `src/memory/mod.rs`

Giriste fault ayrimi:

```rust
pub fn handle_user_page_fault(addr: u64, error: PageFaultErrorCode) -> bool {
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

Bu ayrim satiri cok kritik cunku yanlis route su riskleri dogurur:

- COW yerine lazy map (izin ihlali)
- user/kernel range karisimi

### 9.1 W^X enforcement

`enforce_wx` ve `sanitize_user_map_flags` lane'i,
WRITABLE + EXEC kombinasyonunu fail-closed kirpar.

Bu, exploit yuzeyini ciddi azaltir.

### 9.2 COW lane

`handle_cow_fault` iki ayrik patika kullanir:

- refcount <= 1: writable upgrade
- refcount > 1: yeni frame + copy + remap

Burada refcount muhasebesi dogruluk lane'inin merkezidir.

### 9.3 THP lane

`try_map_thp_anon` dogrudan 2MiB map denemesi yapar,
eligibility tutmazsa 4KiB lane'e geri duser.

THP her workload icin dogru degildir.
Bu nedenle eligibility ve rollback zorunludur.

---

## 10. MGLRU, reclaim ve writeback

Kod baglami:

- `src/memory/mglru.rs`
- `src/memory/mod.rs`

MGLRU lane'i page'leri generation + hot_score ile siniflar.

Victim secimi:

```rust
candidate.generation < curr.generation
|| (candidate.generation == curr.generation && candidate.hot_score < curr.hot_score)
```

Yani sadece "en eski" degil, esitlikte "en soguk" sayfa secilir.

### 10.1 Reclaim daemon

`memory_reclaim_daemon` dongusu:

- `damon::age(now)`
- `mglru::age_generations(now)`
- `reclaim_pages_global(...)`
- `process_writeback_budget(...)`

Bu lane, pressure sinyaline gore hiz degistirir.

### 10.2 Writeback budget modeli

`process_writeback_budget(budget)` kuyruk servis hizini sinirlar.

Kaba model:

\[
\rho = \frac{\lambda_{dirty}}{\mu_{writeback}}
\]

Uzun sure \(\rho > 1\) kalirsa kuyruk buyur, reclaim lane'i sismeye baslar.

---

## 11. ZSwap lane

Kod baglami: `src/memory/zswap.rs`

Bu lane disk swap oncesi RAM icinde compression tamponudur.

Mevcut implementasyonda `Lz4Compressor` ve `ZstdCompressor` ayni cekirdek RLE formatini
kullanir. Bu pratik bir no_std trade-off'tur.

Ana karar denklemi:

\[
Gain = IO_{saved} - CPU_{compress}
\]

Workload'a gore bu denklemin isareti degisir.
Bu nedenle tek compressor secimi yerine policy secimi gerekebilir.

---

## 12. Lock-free io_uring ring

Kod baglami: `src/posix/io_uring_ring.rs`

Bu alt sistemin kalbi publication boundary'dir.

SQ push sirasi:

1. entry yaz
2. `smp_wmb`
3. `tail.store(..., Release)`

SQ pop sirasi:

1. `tail.load(Acquire)`
2. `smp_rmb`
3. entry oku
4. `head.store(..., Release)`

Kod:

```rust
crate::memory_barriers::smp_wmb();
self.tail.store(tail.wrapping_add(1), Ordering::Release);
```

Bu sinir bozulursa stale read ve corrupt completion riski dogar.

### 12.1 Batch lane

`pop_batch` bariyer maliyetini amortize eder.
Yoğun submission trafiginde farki net gorunur.

---

## 13. TLS 1.3 key schedule lane

Kod baglami: `src/net/tls.rs`

`KeySchedule` yapisi,
early -> handshake -> master sekret zincirini acik tutar.

```rust
let hkdf = Hkdf::<Sha256>::new(Some(&derived_secret), shared_secret);
hkdf.expand(b"", &mut handshake_secret).ok();
```

Asil risk, kripto primitive secimi degil,
state gecislerinin transcript ile tutarsizlasmasidir.

Bu nedenle handshake state makinesi ve mesaj sirasinin
fail-closed dogrulanmasi gerekir.

---

## 14. QUIC parser lane

Kod baglami: `src/net/quic.rs`

QUIC varint decode lane'i parser saldiri yuzeyidir.

Guard satiri:

```rust
const MAX_ACK_RANGES: u64 = 256;
```

Bu limit, parser tarafinda memory amplification riskini sinirlar.

Risk sinifi:

- limitsiz range parse
- offset/length tasmasi
- stream state desync

Mitigasyon lane'i:

- decode sinirlari
- frame bazli explicit enum ayrimi

---

## 15. WireGuard lane

Kod baglami: `src/net/wireguard.rs`

Replay koruma satiri:

```rust
if session.receiving_nonce != WG_NONCE_UNINITIALIZED && nonce <= session.receiving_nonce {
    return Err(WgError::Replay);
}
```

Burada tasarim acik:

- nonce monotonlugu bozulursa paket reddedilir

Bu dogrudan anti-replay guvencesidir.

Ek policy lane'i:

- `allowed_ips` kontrolu ile route policy enforcement

---

## 16. HPACK Huffman decode lane

Kod baglami: `src/net/http2_huffman.rs`

Decode pipeline:

- bit iterator
- code-length tablosu
- symbol emit
- EOS/padding validation

Kritik guard:

```rust
if (right_align_eos & mask) != right_align_current {
    return Err(HuffmanDecodeError::InvalidPadding);
}
```

Bu satir, parser lane'inde fail-closed cikisi saglar.

---

## 17. Cross-subsystem failure analizi

Core lane hatalari tek alt sistemde kalmaz.

Ornek yayilim:

1. scheduler jitter artar
2. reclaim gecikir
3. writeback kuyrugu buyur
4. io latency patlar
5. network timeout artar

Bu nedenle core lane izleme modeli ortak metrik seti ister:

- p99 latency
- queue depth
- pressure signal
- drop/overflow counters

---

## 18. Invariant listesi (operasyonel)

Bu ciltteki en kritik invariants:

1. Boot capability lane'i yarim aktif kalmaz.
2. User map lane'i W^X kurali disina cikmaz.
3. COW lane'i refcount muhasebesini kaybetmez.
4. RT lane limitsiz CPU tuketimine acik birakilmaz.
5. Work-stealing deque son-eleman yarisini CAS ile kapatir.
6. io_uring publication sirasinda tail, veriden once publish edilmez.
7. QUIC parser lane'i ACK range limitini asmaz.
8. WireGuard lane'i replay nonce'u kabul etmez.
9. HPACK lane'i invalid padding'i sessiz yutmaz.

Bu invariants release kapisinda dogrudan kontrol listesi olarak kullanilmalidir.

---

## 19. Performans tuning disiplin notu

Core tuning, tek benchmark ile yapilmaz.
Asgari workload seti:

- CPU-bound
- IO-bound
- mixed interactive
- memory pressure + network burst

Her tuning degisikligi su bes metrikle raporlanmalidir:

1. p50/p95/p99 latency
2. throughput
3. error/overflow sayaci
4. memory pressure ve reclaim davranisi
5. regresyon fark tablosu

---

## 20. Son soz: core lane'in muhendislik ahlaki

Core kernel kodu, "ortalamada iyi" olma luksune sahip degildir.
Burada kalite kriteri su ucude ayni anda saglamak zorundadir:

- dogruluk
- worst-case kontrolu
- gozlenebilirlik

echOS core lane'inin degeri, yalnizca calismasinda degil,
patolojik kosulda nasil davrandiginin bilinmesinde yatar.
