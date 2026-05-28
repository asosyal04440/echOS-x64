# echOS Matematiksel Optimizasyon Raporu

Tarih: 2026-05-25  
Kapsam: `C:\Users\Bahadir\Desktop\dersler_ve_projeler\echOS` deposu  
Çıktı yolu: `Optimizasyon/Optimizasyon.md`  
Kod değişikliği: yapılmadı. Bu dosya dışında kaynak koduna patch uygulanmadı.  
Çakışma kontrolü: repo kökünde `Optimizasyon` adlı dosya yoktu; hedef klasör oluşturuldu.

Arşiv kontrolü: [
`D:\echOS Kaynak Arşivi\01_Operating_Systems_and_Kernel\Btrfs Design - Developer Docs.md`,
`D:\echOS Kaynak Arşivi\01_Operating_Systems_and_Kernel\Btrfs On-Disk Format - Developer Docs.md`,
`D:\echOS Kaynak Arşivi\OS kaynaları\Windows NT kernel\Windows System Internals 7e Part 1.md`,
`D:\echOS Kaynak Arşivi\OS kaynaları\Diğer sistemler\The Linux Programming Interface.md`,
`D:\echOS Kaynak Arşivi\OS kaynaları\ACPI_Spec_6_5_Aug29.md`,
`docs/agent/pdf-reference-archive-index.md`
]

Not: Arşivden kod kopyalanmadı. Arşiv yalnızca dosya sistemi, Windows NT, Linux/POSIX, ACPI, Btrfs/XFS ve donanım davranışı için clean-room bilgi kaynağı olarak kullanıldı. Repo genelinde tüm dosya listesi tarandı; derin fonksiyon analizi performans yüzeyi en yüksek dosyalara uygulandı. Küçük sarmalayıcılar, sabit veri tanımları ve test yardımcıları modül altında gruplanmıştır. Emin olmadığım noktalarda bunu açıkça belirttim.

---

## 1. Yönetici Özeti

echOS geniş kapsamlı bir `no_std` Rust işletim sistemi çekirdeği: UEFI boot, scheduler, SMP, bellek yönetimi, dosya sistemleri, ağ yığını, grafik/compositor, Win32/PE katmanı, jail tabanlı aygıtlar, kripto, compression ve SDK parçaları aynı depoda yer alıyor. En yüksek optimizasyon baskısı üç eksende toplanıyor:

1. 8192-core hedefinde O(CPU) taramalar ve global kilitli hot-path yapıları.
2. Storage/network/rendering tarafında per-operation allocation/copy ve busy-poll maliyeti.
3. Dosya sistemi ve path/cache katmanında lineer tarama, `Vec` tabanlı indeks, O(n^2) bakım işleri.

### En büyük 10 optimizasyon fırsatı

| Sıra | Dosya/fonksiyon | Fırsat | Model özeti | Beklenen kazanç | Risk |
|---:|---|---|---|---|---|
| 1 | `src/net/tcp.rs::process_packet`, `process_ipv6_packet` | TCP demux için global lineer bağlantı taraması yerine 4-tuple hash/RCU tablo | `E[lookup]=1+lambda_bucket` | Çok bağlantılı yükte p99 latency ciddi düşer | Orta |
| 2 | `src/task/scheduler.rs::choose_spawn_cpu`, `choose_victim_cpu` | 8192 CPU için O(C) tarama yerine NUMA-local load heap/bitmap | `T_spawn=C_scan*C + C_update*log C_node` | spawn/steal latency yüksek oranda düşer | Yüksek |
| 3 | `src/fs/dcache.rs::lookup`, `shrink`, `compact` | Lookup hit allocation ve O(n^2) LRU işlemlerini sabit maliyetli indekslere çekme | `T_lookup=B+Alloc_clone` -> `B_ref` | Path-heavy workload hızlanır | Orta |
| 4 | `src/drivers/nvme.rs::submit_io_command` | Sync busy-poll yerine adaptive completion polling/IRQ hibriti | `min_B (doorbell+B*cmd+Wq)/B` | CPU tüketimi düşer, throughput artar | Yüksek |
| 5 | `src/fs/ext4.rs::map_block_extent_tree_with_storage` | Extent index/leaf lineer tarama yerine binary search | `O(E)` -> `O(log E)` | Büyük dosya random I/O hızlanır | Düşük-Orta |
| 6 | `src/fs/btrfs.rs::logical_to_physical`, `collect_tree_items` | Chunk/visited/free-space lineer arama yerine interval/BTree indeks | `O(chunks*ops)` -> `O(log chunks)` | Metadata-heavy workload hızlanır | Orta |
| 7 | `src/net/io_uring.rs::pending`, `registered_buffers` | `BTreeMap` ve lineer buffer seçimi yerine sabit slot/slab indeks | `O(log P)+O(B)` -> `O(1)` | syscall batching ve ring throughput artar | Orta |
| 8 | `src/gui/damage.rs`, `src/gfx/tile_renderer.rs` | Sabit redraw eşiği yerine ölçülen pixel/tile/batch maliyetli adaptif karar | `C_partial < C_full` | Render jank ve gereksiz full redraw azalır | Düşük-Orta |
| 9 | `src/win32.rs::init_api_table`, `get_proc_address` | Runtime `BTreeMap<String,...>` kurulumunu statik/perfect hash dispatch'e çevirme | `T_start=sum alloc+insert` -> `O(1)` static lookup | Startup ve memory footprint düşer | Düşük |
| 10 | `src/net/http2.rs`, `src/net/x509.rs`, `src/net/tls.rs` | Parser/crypto doğrulama için arena, cache ve dynamic table skoru | `bytes_saved*freq - memory_cost` | TLS/HTTP2 el sıkışma ve header maliyeti düşer | Orta |

### En riskli 10 dosya/fonksiyon

| Sıra | Dosya/fonksiyon | Risk nedeni |
|---:|---|---|
| 1 | `src/task/scheduler.rs::schedule` | Context switch, FPU state, CR3/PCID, epoch/RCU, IRQ state aynı hot path içinde. |
| 2 | `src/memory/mod.rs::allocate_frame_with_context` | Allocation miss yolunda reclaim, writeback, OOM, scheduler listeleme birleşiyor. |
| 3 | `src/drivers/nvme.rs::submit_io_command`, `drain_completion_queue` | MMIO doorbell, fence, pending ring, completion phase ve timeout etkileşimi. |
| 4 | `src/cpu/smp.rs::send_tlb_shootdown_ipi`, `balance_load` | Cross-core IPI, TLB invalidation ve load balancing cache-coherence etkisi. |
| 5 | `src/net/tcp.rs::process_packet` | Global connection table, accept queue, congestion state ve ACK davranışı aynı akışta. |
| 6 | `src/fs/dcache.rs::rename`, `shrink`, `compact` | Cache tutarlılığı, LRU indeksleri, bucket dizileri ve correctness riski. |
| 7 | `src/fs/ext4.rs::write_file_to_storage` | Metadata update, allocation, read-modify-write ve extent sınırı birlikte. |
| 8 | `src/fs/btrfs.rs::BtrfsCowWriter::commit_transaction` | COW metadata, superblock mirror, free-space accounting. |
| 9 | `src/interrupts/mod.rs` | Çok sayıda unsafe/atomic/IDT/IRQ hot path; derin inceleme sınırlı. |
| 10 | `src/net/tls.rs`, `src/net/x509.rs`, `src/net/dnssec.rs` | Kripto doğrulama, parser karmaşıklığı, allocation ve hata güvenliği. |

### En yüksek beklenen kazanç sağlayan 10 öneri

1. TCP bağlantı demux tablosu: çok bağlantılı network testlerinde lineer bağlantı sayısı bağımlılığını kaldırır.
2. Scheduler NUMA-local load indeksleri: 8192-core hedefinin en büyük seri tarama maliyetini azaltır.
3. NVMe completion batching/polling modeli: CPU spin ve doorbell/fence amortizasyonunu iyileştirir.
4. Dcache LRU/lookup yeniden tasarımı: path-heavy filesystem workload için doğrudan etki.
5. io_uring pending slot ve registered buffer indeksleri: ring kullanımında log/linear maliyeti düşürür.
6. Renderer damage adaptive threshold: GUI latency ve bandwidth kullanımını azaltır.
7. Ext4 extent binary search ve full-block write bypass: storage random I/O hızlanır.
8. Btrfs interval tree/chunk map: COW metadata operasyonları daha ölçeklenir.
9. Win32 statik API dispatch: startup allocation ve lookup maliyeti düşer.
10. Parser arena/cache planı: X.509/TLS/HTTP2/DNSSEC allocation basıncı düşer.

### En kolay uygulanacak 10 öneri

1. `src/fs/ext4.rs` extent leaf/index taramalarında binary search.
2. `src/win32.rs` API isimlerini build-time sıralı statik tabloya alma.
3. `src/net/tcp.rs::time_wait_gc` için tekrar kullanılabilir scratch buffer.
4. `src/net/http2.rs::HpackEncoder` static table lookup için precomputed map.
5. `src/gfx/simd.rs` boyut/eş hizalama eşiklerini ölçümlü constant olarak ayırma.
6. `src/gui/damage.rs` threshold değerlerini counters ile ölçülebilir yapma.
7. `src/fs/namei.rs` component buffer reuse ve symlink recursion limit metriği.
8. `src/fs/btrfs.rs::commit_transaction` used-bytes hesaplamasını mirror döngüsü dışına alma.
9. `src/net/io_uring.rs` registered buffer group indeksini `BTreeMap`/array ile hızlandırma.
10. `src/fs/dcache.rs::lookup` bucket clone allocation'ını kaldırma.

---

## 2. Repo Haritası

### Dil ve framework tespiti

- Ana dil: Rust, `#![no_std]` kernel hedefi.
- Hedefler: `x86_64-unknown-uefi`, host testleri için `x86_64-pc-windows-msvc`.
- Öne çıkan bağımlılıklar: `spin`, `rlsf`, `virtio-drivers`, `smoltcp`, `cosmic-text`, `tinybmp`, `qoi`, RustCrypto, lokal `third_party` yamaları.
- Workspace üyeleri: ana `ech_os`, SDK crate'leri, `third_party/ironshim-rs`, `tools/arch_guard`.

### Ana klasörler ve modüller

| Klasör | Yaklaşık rol | Performans yüzeyi |
|---|---|---|
| `src/task` | Scheduler, CFS/EEVDF, futex, runtime task state | Çok yüksek |
| `src/memory` | PMM/VMM, page cache, reclaim, THP, zswap | Çok yüksek |
| `src/drivers` | NVMe, AHCI, PCI, IOMMU, jail aygıtları, NIC/WiFi/Audio | Çok yüksek |
| `src/fs` | VFS, ext4, btrfs, f2fs, ntfs, dcache/namei | Çok yüksek |
| `src/net` | TCP/IP, HTTP2, QUIC/TLS, io_uring, DNSSEC, gRPC | Çok yüksek |
| `src/gui`, `src/gfx`, `src/services` | Compositor, renderer, damage, display service | Yüksek |
| `src/cpu`, `src/interrupts` | SMP, APIC/interrupt, topology, TLB shootdown | Çok yüksek |
| `src/security`, `src/crypto` | Package/TUF/seed/crypto | Orta-Yüksek |
| `src/shell`, `src/posix`, `src/win32.rs`, `src/pe_loader.rs` | ABI, shell, PE/Win32 uyumluluk | Orta-Yüksek |
| `sdk`, `tools`, `tests`, `benches` | Host-facing SDK, guard tools, ölçüm altyapısı | Orta |

### Büyük dosyalar

| Dosya | Satır sayısı sinyali | Not |
|---|---:|---|
| `src/win32.rs` | ~30327 | Runtime API dispatch, çok sayıda string/table işlemi. |
| `src/gfx/velvet_glove.rs` | ~10162 | Grafik/rendering hot path adayı. |
| `src/shell/mod.rs` | ~9007 | Komut dispatch ve parser/state yüzeyi. |
| `src/fs/f2fs.rs` | ~7609 | Storage metadata ve allocation yüzeyi. |
| `src/posix.rs` | ~5762 | Syscall/ABI yüzeyi. |
| `src/memory/mod.rs` | ~5199 | PMM/VMA/page cache/reclaim. |
| `src/fs/ext4.rs` | ~5002 | Extent, bitmap, file I/O. |
| `src/pe_loader.rs` | ~4315 | PE parsing/import/relocation. |
| `src/net/tls.rs` | ~4237 | TLS handshake/record/crypto. |
| `src/drivers/usb/mod.rs` | ~4174 | Tier-2 aygıt yüzeyi. |

### Kritik çalışma yolları

1. Boot -> ACPI/topology -> SMP init -> scheduler start.
2. Allocation -> PMM/VMM -> page cache/reclaim -> OOM fallback.
3. NVMe/AHCI IRQ/completion -> block layer -> filesystem -> VFS/namei/dcache.
4. NIC RX -> TCP/IP demux -> socket/stream -> HTTP2/TLS/gRPC.
5. GUI event -> damage tracker -> renderer/tile renderer -> display service.
6. PE/Win32 loader -> import resolution -> ABI dispatch.
7. Jail rings -> Tier-2 driver microreboot/backoff -> IPC.

### Tekrar eden şüpheli pattern'ler

- Hot path içinde `Vec` allocation/clone: dcache, namei, TCP packet building, HTTP2/HPACK, parsers.
- Global `Mutex`/`BTreeMap` erişimi: TCP connection tables, io_uring pending, Win32 API table, futex queues.
- O(n) veya O(n^2) maintenance: dcache LRU, Btrfs free-space merge, path component joins, tree traversal visited list.
- `SeqCst` fence kullanımı: NVMe ve bazı atomic path'lerde maliyet modeliyle doğrulanmalı.
- Busy loop/poll: NVMe sync completion ve wait paths.
- Büyük switch/dispatch/string lookup: shell, Win32, PE import.

---

## 3. Global Performans Modeli

Kullanıcı tarafından verilen genel model:

```math
C_system = alpha*L + beta*M + gamma*P + delta*I + epsilon*N + zeta*S + eta*E + lambda*K
```

echOS için modelin gerçek modüllere bağlı genişletilmiş hali:

```math
C_echOS =
  alpha*L
  + beta*M
  + gamma*P
  + delta*I_storage
  + epsilon*N_net
  + zeta*S_sync
  + eta*E_error
  + lambda*K_complexity
  + mu*H_mmio_dma
  + nu*T_cache_tlb
  + rho*D_deadline
```

Değişkenler:

- `L`: uçtan uca latency. Alt bileşenler: `L_sched`, `L_irq`, `L_storage`, `L_net`, `L_render`, `L_pagefault`.
- `M`: resident memory, allocator metadata, page cache, ring buffers, dynamic parser buffers.
- `P`: CPU instruction cost; parser, checksum, compression, crypto, render fill/blend.
- `I_storage`: NVMe/AHCI read/write/flush, filesystem metadata write amplification.
- `N_net`: packet processing, retransmission, ACK, TLS/HTTP2/gRPC framing.
- `S_sync`: global lock, atomic cache-line bounce, RCU grace period, futex queue contention.
- `E_error`: retry, timeout, crash-only microreboot, OOM/reclaim fallback.
- `K_complexity`: bakım ve doğrulama maliyeti; her optimizasyon için risk dengeleyici.
- `H_mmio_dma`: doorbell, DMA mapping, IOMMU, cache maintenance, IRQ coalescing maliyeti.
- `T_cache_tlb`: LLC miss, false sharing, TLB shootdown, PCID/CR3 geçiş maliyeti.
- `D_deadline`: audio/render/network realtime deadline miss ve jitter.

Alt modeller:

```math
L = L_sched + L_irq + L_storage + L_net + L_render + L_pagefault
```

```math
S_sync = sum_i contention_i * wait_i + sum_j atomic_j * cacheline_bounce_j
```

```math
I_storage = sum_req (T_setup + bytes_req / BW_device + W_queue + T_flush)
```

```math
N_net = packets * (T_parse + T_demux + T_copy + T_checksum) + retransmits * T_retx
```

```math
Speedup(N) = 1 / (f_serial + (1 - f_serial)/N + kappa_coherence(N))
```

Neden bu model echOS'a uygun:

- Scheduler, TCP demux ve dcache gibi yerlerde seri taramalar `f_serial` büyütüyor.
- NVMe, IOMMU ve NIC paths `H_mmio_dma` ile doğrudan bağlı.
- SMP/TLB/atomic-heavy modüller için klasik Amdahl yasası tek başına yetmiyor; coherence penalty `kappa_coherence(N)` gerekir.
- GUI/audio/network için ortalama latency değil p95/p99 ve deadline miss daha anlamlı.

Falsify testi:

- Önerilen bir değişiklikten sonra yalnızca `P` azalıyor ama `L`/p99 düşmüyorsa model eksik demektir.
- `S_sync` düşerken `T_cache_tlb` artıyorsa veri yapısı locality kaybı ölçülmelidir.
- NVMe batching throughput artırıp p99 latency bozuyorsa `D_deadline` ağırlığı düşük verilmiştir.

---

## 4. Dosya Bazlı Analiz

### Dosya: `src/task/scheduler.rs`

#### Fonksiyon/Sınıf: `choose_spawn_cpu(task)`

**Mevcut rolü:** Yeni task için CPU seçiyor; online/affinity/queue length/topology sinyalleriyle tüm CPU aralığını tarıyor.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: `O(C)`; `C = cpu_limit`.
- Bellek karmaşıklığı: `O(1)`.
- I/O maliyeti: yok.
- Concurrency riski: queue length okuma anlık ve yarışabilir; yanlış seçim throughput'u etkiler.
- Cache riski: 8192 CPU metadata taraması LLC/TLB baskısı üretir.

**Önerilen matematiksel model:**

```math
T_spawn(C) = C * (c_online + c_affinity + c_queue + c_topology)
```

Değişkenler:

- `C`: seçilebilir CPU sayısı.
- `c_online`: online kontrol maliyeti.
- `c_affinity`: affinity mask kontrol maliyeti.
- `c_queue`: queue length okuma maliyeti.
- `c_topology`: NUMA/package/core ağırlık hesabı.

Optimizasyon hedefi:

```math
minimize T_spawn subject to load_skew <= sigma_max and affinity_valid = true
```

Neden mantıklı: fonksiyon her spawn'da CPU sayısı kadar skor hesaplıyor. 8192-core hedefinde `C` bağımlılığı doğrudan spawn latency'ye dönüşür.

Uygulanabilecek optimizasyon:

- NUMA node başına düşük-yük CPU min-heap veya bucketed bitmap.
- Affinity mask ile node-local aday kümesini kesiştir.
- Her enqueue/dequeue sonrası per-node yük indeksini amortized `O(log C_node)` güncelle.
- Cold path'te tam scan ile heap drift düzelt.

Beklenen etki:

- Latency: yüksek düşüş, özellikle task spawn burst'lerinde.
- Memory: düşük artış; per-node indeks gerekir.
- CPU: yüksek düşüş.
- Throughput: orta-yüksek artış.
- Complexity: artar.

Risk seviyesi: Yüksek.

Doğrulama testi: `benches/scheduling_bench` içinde `C={64,512,2048,8192}` simülasyonu; spawn p50/p99, load skew, remote NUMA oranı ölçülmeli.

Uygulama önceliği: P0.

#### Fonksiyon/Sınıf: `choose_victim_cpu(cpu_id)`

**Mevcut rolü:** Work stealing için kurban CPU seçiyor; tüm CPU'ları tarayıp load/topology/memory pressure skoru çıkarıyor.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: `O(C)` her steal denemesi.
- Bellek karmaşıklığı: `O(1)`.
- I/O maliyeti: yok.
- Concurrency riski: eş zamanlı steal denemeleri aynı yoğun CPU'ya yığılabilir.
- Cache riski: per-CPU queue length metadata cross-core okunur.

**Önerilen matematiksel model:**

```math
Score(v) =
  a*Q_v
  - b*Distance(cpu, v)
  - c*MemPressure_v
  - d*StealCollision_v
```

Değişkenler:

- `Q_v`: victim runqueue uzunluğu.
- `Distance(cpu,v)`: aynı core/package/NUMA uzaklık maliyeti.
- `MemPressure_v`: victim node bellek baskısı.
- `StealCollision_v`: son dönemde aynı victim için başarısız steal sayısı.
- `a,b,c,d`: benchmark ile kalibre edilecek ağırlıklar.

Optimizasyon hedefi:

```math
maximize Score(v) while P(successful_steal) >= p_min
```

Neden mantıklı: mevcut skor zaten yük ve topology ağırlığı kullanıyor; eksik olan şey collision/backoff teriminin ölçülü hale gelmesi ve aday kümenin daraltılması.

Uygulanabilecek optimizasyon:

- Per-node non-empty queue bitmap.
- Randomized top-k victim seçimi.
- Başarısız steal sonrası exponential cooling.
- NUMA-local önce, global scan yalnızca starvation halinde.

Beklenen etki:

- Latency: steal-heavy yükte düşer.
- CPU: cross-core metadata scan azalır.
- Throughput: yüksek core sayısında artar.
- Complexity: orta-yüksek.

Risk seviyesi: Yüksek.

Doğrulama testi: synthetic work-steal benchmark; steal success ratio, failed steal/cycle, remote steal oranı, p99 schedule latency.

Uygulama önceliği: P0.

#### Fonksiyon/Sınıf: `schedule()`

**Mevcut rolü:** IRQ kapalı context switch yolu; RCU quiescent state, task state transition, ghost policy, runqueue pop/steal, CR3/PCID, stack, GS base, FPU mode ve assembly context switch.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: normalde `O(1)`, boş queue/steal path'te `O(C + Q_v)`.
- Bellek karmaşıklığı: `O(1)`.
- I/O maliyeti: MSR/CR3/CPU register maliyeti.
- Concurrency riski: IRQ kapalı süre, RCU epoch, worker queue ve sleeping/zombie list etkileşimi.
- Cache riski: current task metadata, FPU state ve queue metadata farklı cache line'larda olabilir.

**Önerilen matematiksel model:**

```math
L_switch =
  T_irq_off
  + T_queue
  + T_rcu
  + T_addrspace
  + T_fpu
  + T_asm
```

Değişkenler:

- `T_irq_off`: interrupt kapalı kalan süre.
- `T_queue`: local pop + olası steal maliyeti.
- `T_rcu`: quiescent/epoch accounting.
- `T_addrspace`: CR3/PCID/GDT/syscall stack değişimi.
- `T_fpu`: lazy/eager FPU save/restore.
- `T_asm`: assembly switch sabit maliyeti.

Optimizasyon hedefi:

```math
minimize p99(L_switch) and minimize Var(L_switch)
```

Neden mantıklı: scheduler için ortalama değil p99 ve jitter kritik. Context switch birkaç sabit parçanın toplamı; her parça ayrı counter ile ölçülebilir.

Uygulanabilecek optimizasyon:

- IRQ-off bölümünü daha küçük kritik alt bölümlere ayırma ancak correctness kanıtı şart.
- FPU dirty-state doğruluğunu counter ile izleyip eager/lazy kararını workload bazlı seçme.
- Steal path'i `schedule()` dışına prefetch/idle worker hazırlığı ile kısmen taşıma.
- Per-CPU scheduler stats cache-line align denetimi.

Beklenen etki:

- Latency: p99 schedule latency düşer.
- CPU: workload'a bağlı.
- Throughput: CPU-bound tasklarda orta artış.
- Complexity: yüksek.

Risk seviyesi: Yüksek.

Doğrulama testi: context switch microbench; TSC ile `L_switch` bileşenleri; IRQ-off max; FPU-heavy ve FPU-free workload karşılaştırması.

Uygulama önceliği: P1.

#### Fonksiyon/Sınıf: `take_task_from_worker_by_id`, `steal_task_from_victim_by_id`

**Mevcut rolü:** Worker queue içinden belirli task'ı veya victim queue'dan çalıştırılabilir task'ı alıyor; uygun olmayan taskları geçici listeye alıp geri koyuyor.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: `O(Q)`; `Q` queue uzunluğu.
- Bellek karmaşıklığı: deferred task sayısı kadar.
- Concurrency riski: queue lock/ownership ve task state yarışları.
- Cache riski: queue içeriği ardışık değilse locality zayıf.

**Önerilen matematiksel model:**

```math
E[T_pick] = sum_{i=1}^{Q} P(first_runnable_at_i) * i * c_pop
```

Optimizasyon hedefi:

```math
minimize E[T_pick] by keeping runnable tasks near pop end
```

Uygulanabilecek optimizasyon:

- Queue içinde runnable/non-runnable segment ayrımı.
- Uyuyan taskları runqueue dışında state listesine taşımayı sertleştirme.
- Task-id lookup için per-worker sparse index, yalnızca rare targeted removal path'te.

Beklenen etki: queue pollution durumunda latency düşer.

Risk seviyesi: Orta.

Doğrulama testi: sleeping/runnable karışık queue benchmark; pop attempts per selected task.

Uygulama önceliği: P1.

### Dosya: `src/task/cfs.rs`

#### Fonksiyon/Sınıf: `CfsRunQueue::enqueue`, `dequeue`, `pick_next`

**Mevcut rolü:** CFS benzeri vruntime sıralamasıyla task seçimi.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: veri yapısına bağlı; list/Vec ise enqueue/dequeue seçimi `O(n)` olabilir, tree ise `O(log n)`.
- Bellek karmaşıklığı: task sayısı kadar.
- Concurrency riski: runqueue güncellemesi scheduler hot path'e bağlı.
- Cache riski: tree pointer chasing vs Vec locality tradeoff.

**Önerilen matematiksel model:**

```math
vruntime_delta = runtime_ns * NICE_0_LOAD / weight(task)
```

Optimizasyon hedefi:

```math
minimize |service_i/weight_i - service_j/weight_j| for all runnable i,j
```

Neden mantıklı: CFS adalet metriği doğrudan ağırlıklı servis payını dengeler.

Uygulanabilecek optimizasyon:

- Küçük runqueue için sorted small-vector, büyük runqueue için tree hibriti.
- `n_switch` eşiği benchmark ile:

```math
n_star = argmin_n min(T_vec(n), T_tree(n))
```

Beklenen etki: düşük/orta; scheduler fairness korunursa.

Risk seviyesi: Orta.

Doğrulama testi: fairness error, context switch cost, interactive latency.

Uygulama önceliği: P2.

### Dosya: `src/task/eevdf.rs`

#### Fonksiyon/Sınıf: `enqueue`, `dequeue`, `account_runtime`, `pick_next`, `should_preempt`

**Mevcut rolü:** EEVDF deadline/lag tabanlı seçim ve preemption kararı.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: queue veri yapısına bağlı; deadline min seçimi `O(n)` ise büyür.
- Concurrency riski: runtime accounting ile preemption kararı tutarlı olmalı.
- Cache riski: task scheduling entity layout kritik.

**Önerilen matematiksel model:**

```math
lag_i = service_ideal_i - service_actual_i
eligible_i = lag_i >= 0
pick = argmin_i deadline_i among eligible_i
```

Optimizasyon hedefi:

```math
minimize max_i deadline_miss_i while preserving lag_i bounds
```

Uygulanabilecek optimizasyon:

- Eligible min-deadline heap.
- Non-eligible tasklar için next-eligible timestamp bucket.
- Preemption threshold:

```math
preempt if deadline_new + C_switch < deadline_current - jitter_budget
```

Beklenen etki: interactive workload latency azalır.

Risk seviyesi: Orta-Yüksek.

Doğrulama testi: mixed CPU/interactive benchmark; deadline miss histogramı.

Uygulama önceliği: P2.

### Dosya: `src/task/futex.rs`

#### Fonksiyon/Sınıf: `get_queue`, `enqueue`, `wake_matches`, `check_timeouts`, `sys_futex_waitv`

**Mevcut rolü:** Futex adreslerine göre wait queue yönetimi, wake ve timeout kontrolleri.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: queue map lookup `O(log F)` veya hash ise ortalama `O(1)`; wake path queue length'e bağlı.
- Bellek karmaşıklığı: bekleyen task sayısı.
- Concurrency riski: global queue lock varsa thundering herd.
- Cache riski: wait entry'ler farklı allocationlarda dağılır.

**Önerilen matematiksel model:**

```math
T_wake(k,w) = T_lookup + min(k,w)*c_wake + collisions*c_scan
```

Değişkenler:

- `k`: istenen wake sayısı.
- `w`: futex queue waiters sayısı.
- `collisions`: aynı hash bucket içindeki farklı futex adresleri.

Optimizasyon hedefi:

```math
minimize T_wake and minimize false_wake_rate
```

Uygulanabilecek optimizasyon:

- Adres hash sharding.
- Timeout wheel ile `check_timeouts` lineer taramayı azaltma.
- Waitv için adresleri normalize edip tek transaction'da kayıt.

Beklenen etki: contention-heavy userland workload'ta p99 düşer.

Risk seviyesi: Orta.

Doğrulama testi: N-thread futex ping-pong, waitv timeout storm, false wake oranı.

Uygulama önceliği: P1.

### Dosya: `src/rcu.rs`

#### Fonksiyon/Sınıf: `RcuPtr::read`, `update`, `compare_and_swap`, `synchronize_rcu`

**Mevcut rolü:** Read-copy-update pointer okuma/güncelleme, grace period ve callback işleme.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: read `O(1)`, update `O(1)+grace`, synchronize tüm CPU quiescent state bekler.
- Bellek karmaşıklığı: pending callback ve retired object sayısı.
- Concurrency riski: grace period gecikmesi memory blow-up üretir.
- Cache riski: epoch counters cross-core okunur/yazılır.

**Önerilen matematiksel model:**

```math
Memory_retired(t) = lambda_update * E[GracePeriod] * size_object
```

Optimizasyon hedefi:

```math
minimize E[GracePeriod] subject to reader_overhead <= r_max
```

Uygulanabilecek optimizasyon:

- Per-CPU callback batching.
- Idle CPU quiescent hint.
- Grace-period stall detector.
- Hot readers için read-side counter cache-line alignment denetimi.

Beklenen etki: update-heavy RCU yapıların memory pressure'ı düşer.

Risk seviyesi: Yüksek.

Doğrulama testi: RCU torture; callback backlog, grace-period p99, reader overhead.

Uygulama önceliği: P1.

### Dosya: `src/cpu/smp.rs`

#### Fonksiyon/Sınıf: `send_tlb_shootdown_ipi`, `startup_all_aps`, `balance_load`

**Mevcut rolü:** Çok çekirdek init, TLB shootdown IPI, CPU load balancing.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: target CPU sayısına bağlı `O(C_target)`.
- Bellek karmaşıklığı: per-CPU state.
- I/O maliyeti: APIC/IPI ve control register etkileri.
- Concurrency riski: TLB stale window vs batching.
- Cache riski: CPU state metadata cross-core invalidation.

**Önerilen matematiksel model:**

```math
T_shootdown(B,C) = C*T_ipi + B*T_invlpg + T_ack_wait
```

Değişkenler:

- `B`: bir batch içindeki invalidation sayısı.
- `C`: hedef CPU sayısı.
- `T_ipi`: IPI gönderim maliyeti.
- `T_invlpg`: sayfa başına invalidation maliyeti.
- `T_ack_wait`: en yavaş CPU ACK bekleme süresi.

Optimizasyon hedefi:

```math
minimize T_shootdown while stale_window <= W_max
```

Uygulanabilecek optimizasyon:

- Range invalidation batching.
- PCID-aware lazy shootdown.
- Aynı address-space CPU maskesiyle hedef daraltma.

Beklenen etki: mmap/page fault/unmap yoğunluğunda yüksek.

Risk seviyesi: Yüksek.

Doğrulama testi: multi-core mmap/unmap stress; stale access litmus, shootdown p99.

Uygulama önceliği: P1.

### Dosya: `src/memory/mod.rs`

#### Fonksiyon/Sınıf: `allocate_frame_with_context`

**Mevcut rolü:** Frame allocation; ilk deneme başarısızsa reclaim/writeback, tekrar allocation, OOM candidate seçimi ve post-kill reclaim akışı.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: fast path `O(1)`/PMM'e bağlı; miss path `O(R + T)`; `R` reclaim taraması, `T` task sayısı.
- Bellek karmaşıklığı: OOM process info listesi.
- I/O maliyeti: writeback hook tetiklenirse storage I/O.
- Concurrency riski: allocation path içinde scheduler/list/reclaim etkileşimi.
- Cache riski: PMM bitmap, LRU state, cgroup stats farklı bölgelerde.

**Önerilen matematiksel model:**

```math
T_alloc = P_hit*T_pmm + (1-P_hit)*(T_reclaim + T_pmm_retry + P_oom*T_oom)
```

Değişkenler:

- `P_hit`: ilk PMM allocation başarı olasılığı.
- `T_pmm`: PMM allocation maliyeti.
- `T_reclaim`: reclaim/writeback maliyeti.
- `P_oom`: retry sonrası OOM'a düşme olasılığı.
- `T_oom`: task listeleme, skor, kill ve recovery maliyeti.

Optimizasyon hedefi:

```math
minimize E[T_alloc] and minimize P(T_alloc > alloc_budget)
```

Neden mantıklı: allocation latency dağılımı iki modlu; fast path küçük, miss path çok büyük.

Uygulanabilecek optimizasyon:

- Watermark tabanlı pre-reclaim.
- Per-zone/per-node free frame counters ile erken karar.
- OOM candidate skorunu incremental maintain etme.
- Reclaim work'i allocation yapan task'a tamamen yüklemeyen background worker.

Beklenen etki:

- Latency: memory pressure altında p99 düşer.
- Memory: watermark nedeniyle biraz reserved free memory artar.
- CPU: OOM/list scan azalır.
- Throughput: allocation-heavy yükte artar.

Risk seviyesi: Yüksek.

Doğrulama testi: page allocation stress; `P_hit`, p99 allocation latency, reclaim pages/sec, OOM false positive.

Uygulama önceliği: P0.

#### Fonksiyon/Sınıf: VMA mutation ve `merge_adjacent`

**Mevcut rolü:** Address space VMA regionlarını `Vec` üzerinde split/merge/clone ederek güncelliyor.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: `O(V)` veya split/merge ile `O(V log V)` benzeri; `V = VMA sayısı`.
- Bellek karmaşıklığı: geçici clone sayısı kadar.
- Concurrency riski: address-space lock süresi artar.
- Cache riski: `Vec` locality iyi, ama büyük VMA sayısında kopyalama pahalı.

**Önerilen matematiksel model:**

```math
T_vma_update(V,s) = c_scan*V + c_clone*s + c_merge*V
```

Değişkenler:

- `V`: toplam region sayısı.
- `s`: split edilen region sayısı.
- `c_clone`: region clone maliyeti.

Optimizasyon hedefi:

```math
minimize T_vma_update for high V while preserving range query latency
```

Uygulanabilecek optimizasyon:

- Küçük `V` için `Vec`; büyük `V` için interval tree hibriti.
- Dirty region update için in-place split planı.
- Merge kararını her update sonunda tüm liste yerine komşularla sınırlama.

Beklenen etki: mmap-heavy ve loader path'te orta-yüksek.

Risk seviyesi: Orta.

Doğrulama testi: PE loader/mmap benchmark; VMA count sweep, lock hold time.

Uygulama önceliği: P1.

### Dosya: `src/memory/pmm.rs`

#### Fonksiyon/Sınıf: `allocate_frame`, `allocate_contiguous`

**Mevcut rolü:** Bitmap tabanlı frame allocation; son indeks optimizasyonu var, contiguous allocation lineer tarama yapıyor.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: single frame worst-case `O(W)`; contiguous worst-case `O(F * P)`; `W` bitmap word, `F` frame, `P` istenen pages.
- Bellek karmaşıklığı: bitmap.
- Concurrency riski: PMM kilidi varsa allocation bottleneck.
- Cache riski: bitmap scan uzun ve branch-heavy.

**Önerilen matematiksel model:**

```math
E[T_contig(P)] = sum_{i=1}^{F} P(run_free_at_i < P) * c_probe
```

Optimizasyon hedefi:

```math
minimize E[T_contig(P)] and minimize external_fragmentation
```

Uygulanabilecek optimizasyon:

- Order-based buddy/free-run summary bitmap.
- Per-node free area counters.
- Contiguous allocation için run-length hint.
- SIMD/word-level zero bit scan.

Beklenen etki: DMA/hugepage allocation için yüksek.

Risk seviyesi: Orta-Yüksek.

Doğrulama testi: fragmentation corpus; contiguous allocation success latency, external fragmentation index.

Uygulama önceliği: P1.

### Dosya: `src/memory/mglru.rs`, `src/memory/zswap.rs`, `src/memory/thp.rs`

#### Fonksiyon/Sınıf: LRU generation, zswap writeback, THP promotion

**Mevcut rolü:** Page aging, swap compression/writeback ve hugepage promotion için memory pressure kararları.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: page population ve generation sayısına bağlı.
- Bellek karmaşıklığı: page metadata, compressed pages.
- I/O maliyeti: zswap writeback storage path'e bağlanır.
- Cache riski: page metadata taraması.

**Önerilen matematiksel model:**

```math
Benefit_THP = TLB_miss_saved*miss_cost - promotion_copy_cost - fragmentation_penalty
```

```math
Benefit_zswap = IO_saved - (compress_cpu + memory_overhead + writeback_risk)
```

Optimizasyon hedefi:

```math
maximize Benefit_THP + Benefit_zswap under memory_pressure <= pressure_budget
```

Uygulanabilecek optimizasyon:

- THP promotion yalnızca fault locality ve TLB miss counter ile desteklenince.
- zswap için compression ratio EWMA ve writeback queue budget.
- MGLRU refault distance ile eviction skorlaması.

Beklenen etki: memory pressure altında yüksek ama workload bağımlı.

Risk seviyesi: Orta-Yüksek.

Doğrulama testi: memcached-like locality, streaming workload, random fault workload; TLB miss, refault, compression ratio.

Uygulama önceliği: P2.

### Dosya: `src/drivers/nvme.rs`

#### Fonksiyon/Sınıf: `NvmeQueue::submit`, `poll_completion`

**Mevcut rolü:** SQ entry yazıyor, fence sonrası doorbell çalıyor; completion phase/head kontrol ediyor.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: `O(1)`.
- Bellek karmaşıklığı: queue depth sabit.
- I/O maliyeti: MMIO doorbell ve DMA-visible queue memory.
- Concurrency riski: memory ordering hatası veri kaybı üretir.
- Cache riski: SQ/CQ cache line ownership ve MMIO serialize maliyeti.

**Önerilen matematiksel model:**

```math
T_submit_batch(B) = B*T_cmd_write + T_fence + T_doorbell
T_per_cmd(B) = T_submit_batch(B) / B
```

Değişkenler:

- `B`: doorbell başına batch command sayısı.
- `T_cmd_write`: SQE yazma maliyeti.
- `T_fence`: ordering fence maliyeti.
- `T_doorbell`: MMIO write maliyeti.

Optimizasyon hedefi:

```math
minimize T_per_cmd(B) while L_queue(B) <= latency_budget
```

Uygulanabilecek optimizasyon:

- Doorbell batching.
- Release fence ve volatile semantics'in spec'e göre daraltılması; bunu hardware spec doğrulaması olmadan uygulamamak gerekir.
- CQ poll burst limit.
- SQ/CQ ring alignment ve prefetch.

Beklenen etki: yüksek IOPS workload'ta yüksek.

Risk seviyesi: Yüksek.

Doğrulama testi: NVMe queue microbench; IOPS, p99, MMIO writes/op, command timeout.

Uygulama önceliği: P1.

#### Fonksiyon/Sınıf: `submit_io_command`, `drain_completion_queue`

**Mevcut rolü:** Sync path command submit sonrası completion bekliyor; async path pending ring ve completion drain kullanıyor.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: completion latency'ye bağlı busy wait.
- CPU maliyeti: sync path'te bekleme sırasında yüksek.
- Concurrency riski: pending ring head/tail ve completion phase yarışı.
- Cache riski: pending slots ve CQ metadata.

**Önerilen matematiksel model:**

```math
C_wait = p_fast*T_poll + (1-p_fast)*(T_poll_budget + T_irq_sleep + T_wakeup)
```

Optimizasyon hedefi:

```math
minimize CPU_cycles_wait subject to p99_completion_latency <= L_max
```

Uygulanabilecek optimizasyon:

- Adaptive poll-then-sleep.
- Per-queue completion budget.
- Timeout histogramı ve dynamic poll budget:

```math
poll_budget = clamp(k * p50_completion_cycles, B_min, B_max)
```

Beklenen etki: CPU tüketimi düşer, p99 korunursa net kazanç yüksek.

Risk seviyesi: Yüksek.

Doğrulama testi: sync read/write/flush latency histogramı, CPU cycles/op, timeout count.

Uygulama önceliği: P0.

### Dosya: `src/drivers/ahci.rs`

#### Fonksiyon/Sınıf: `read_sector`, `write_sector`, `flush`, `identify`

**Mevcut rolü:** SATA/AHCI sektör I/O ve flush path.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: sektör sayısı ve command completion'a bağlı.
- I/O maliyeti: command issue, DMA, interrupt/poll.
- Concurrency riski: command slot allocation ve flush ordering.
- Cache riski: DMA buffer alignment.

**Önerilen matematiksel model:**

```math
T_io(s) = T_cmd_setup + s*sector_size/BW + T_completion + P_flush*T_flush
```

Optimizasyon hedefi:

```math
maximize BW_eff = bytes / T_io while preserving write_order
```

Uygulanabilecek optimizasyon:

- Adjacent sector coalescing.
- Read-ahead/write-behind üst block layer'da.
- Flush batching, yalnızca durability boundary'de.

Beklenen etki: sequential workload'ta orta-yüksek.

Risk seviyesi: Orta.

Doğrulama testi: sequential/random AHCI bench, flush/fsync latency.

Uygulama önceliği: P2.

### Dosya: `src/drivers/pci.rs`

#### Fonksiyon/Sınıf: PCI scan, capability/MSI/MSI-X/AER helpers

**Mevcut rolü:** PCI/PCIe config space tarama, capability chain okuma, interrupt capability yapılandırma.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: bus/device/function sayısına ve capability chain uzunluğuna bağlı.
- I/O maliyeti: config space MMIO/port I/O.
- Concurrency riski: init-time ağırlıklı; runtime az.
- Cache riski: düşük.

**Önerilen matematiksel model:**

```math
T_scan = B*D*F*T_cfg_read + devices*T_cap_chain
```

Optimizasyon hedefi:

```math
minimize T_scan without missing devices
```

Uygulanabilecek optimizasyon:

- ECAM segment/bus range pruning.
- Capability offsets cache.
- Init sonrası immutable device registry.

Beklenen etki: boot time orta.

Risk seviyesi: Düşük-Orta.

Doğrulama testi: emulated PCI topology sweep; discovered device count invariant.

Uygulama önceliği: P2.

### Dosya: `src/drivers/iommu.rs`

#### Fonksiyon/Sınıf: DMA mapping/unmapping ve IOTLB invalidation path'leri

**Mevcut rolü:** DMA isolation, IOMMU page table, invalidation.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: mapped page sayısı ve invalidation batch'e bağlı.
- I/O maliyeti: IOMMU register/invalidation queue.
- Concurrency riski: stale DMA translation güvenlik riski.
- Cache riski: page table writes ve IOTLB.

**Önerilen matematiksel model:**

```math
T_dma_map(p) = p*T_pte + T_iotlb_inv(batch) + T_sync
```

Optimizasyon hedefi:

```math
minimize T_dma_map per byte while isolation_risk = 0
```

Uygulanabilecek optimizasyon:

- Mapping cache for long-lived DMA buffers.
- IOTLB invalidation batching.
- Large-page mapping for aligned rings.

Beklenen etki: NVMe/NIC throughput'ta orta-yüksek.

Risk seviyesi: Yüksek.

Doğrulama testi: DMA map/unmap microbench; isolation negative tests; IOTLB invalidation counter.

Uygulama önceliği: P1.

### Dosya: `src/drivers/jail_ring.rs`

#### Fonksiyon/Sınıf: `push`, `pop`, `push_batch`, `pop_batch`, `JailChannel`

**Mevcut rolü:** Tier-2 jailed aygıtlar için ring buffer ve IPC channel.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: single op `O(1)`, batch `O(B)`.
- Bellek karmaşıklığı: ring capacity.
- Concurrency riski: producer/consumer index ordering.
- Cache riski: head/tail aynı cache line'da ise false sharing.

**Önerilen matematiksel model:**

```math
T_msg(B) = (T_fence + B*T_copy + T_notify) / B
```

Optimizasyon hedefi:

```math
maximize messages_per_sec while queue_delay <= D_max
```

Uygulanabilecek optimizasyon:

- Head/tail cache-line padding doğrulaması.
- Adaptive batch size.
- Backpressure threshold:

```math
apply_backpressure if occupancy/capacity > theta
```

Beklenen etki: WiFi/audio/USB jail throughput'ta orta.

Risk seviyesi: Orta.

Doğrulama testi: SPSC/MPMC ring stress; false sharing counters, occupancy histogram.

Uygulama önceliği: P1.

### Dosya: `src/drivers/nic_native.rs`

#### Fonksiyon/Sınıf: RX/TX descriptor rings, `adaptive_coalesce_tick`, `submit_tx`, `poll_rx`

**Mevcut rolü:** Native NIC descriptor management ve interrupt coalescing.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: packet batch size `B`.
- I/O maliyeti: DMA descriptor, MMIO doorbell, IRQ.
- Concurrency riski: ring ownership ve memory ordering.
- Cache riski: descriptor cache line bouncing.

**Önerilen matematiksel model:**

```math
C_packet(B) = (T_irq + T_doorbell + B*T_desc + B*T_parse) / B + W_queue(B)
```

Optimizasyon hedefi:

```math
minimize C_packet with p99_latency <= L_net_budget
```

Uygulanabilecek optimizasyon:

- Interrupt coalescing PID-like kontrol:

```math
coalesce_next = coalesce + kp*(target_irq_rate - irq_rate) - kd*(p99_latency - L_budget)
```

- RX buffer recycling.
- Cache-line separated producer/consumer indices.

Beklenen etki: network throughput'ta yüksek.

Risk seviyesi: Orta-Yüksek.

Doğrulama testi: pktgen, small-packet PPS, large throughput, p99 latency.

Uygulama önceliği: P1.

### Dosya: `src/drivers/wifi_jail.rs`

#### Fonksiyon/Sınıf: BSS scan/association, MLO planner `link_score`, frame/ring handling

**Mevcut rolü:** WiFi jail içinde scan, auth/assoc, EAPOL, MLO link seçimi ve crash recovery/backoff.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: AP aday sayısı ve frame sayısına bağlı.
- I/O maliyeti: jail IPC ve WiFi firmware/device events.
- Concurrency riski: crash-only recovery sırasında state invalidation.
- Cache riski: düşük-orta.

**Önerilen matematiksel model:**

```math
LinkScore =
  a*SNR
  + b*BW
  - c*LossRate
  - d*RTT
  - e*Energy
  - f*SwitchPenalty
```

Optimizasyon hedefi:

```math
maximize E[throughput] - risk_disconnect*Penalty
```

Uygulanabilecek optimizasyon:

- EWMA tabanlı link metrics.
- Bayesian update ile bağlantı başarısı olasılığı.
- Crash backoff için jittered exponential model.

Beklenen etki: bağlantı kararlılığı ve throughput orta.

Risk seviyesi: Orta.

Doğrulama testi: synthetic scan corpus, handoff simulation, crash/recovery latency.

Uygulama önceliği: P2.

### Dosya: `src/drivers/audio_jail.rs`

#### Fonksiyon/Sınıf: ring write/read, period handling, underrun watchdog

**Mevcut rolü:** Audio jail için buffer doluluk, DMA period ve underrun recovery.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: period başına frame sayısı.
- I/O maliyeti: DMA/audio device.
- Concurrency riski: producer/consumer drift.
- Cache riski: ring head/tail false sharing.

**Önerilen matematiksel model:**

```math
underrun_risk = P(service_time > buffered_frames / sample_rate)
```

Optimizasyon hedefi:

```math
minimize underrun_risk + latency_weight*buffered_ms
```

Uygulanabilecek optimizasyon:

- Adaptive period size.
- Buffer occupancy EWMA ile prefill hedefi.
- Watchdog backoff'un audio deadline'a göre sınırlandırılması.

Beklenen etki: glitch azaltma, latency dengesi.

Risk seviyesi: Orta.

Doğrulama testi: audio stress; underrun count, buffered ms, wakeup jitter.

Uygulama önceliği: P2.

### Dosya: `src/fs/dcache.rs`

#### Fonksiyon/Sınıf: `lookup`

**Mevcut rolü:** Parent inode + name ile dentry arıyor; bucket indices clone ediliyor, hit'te LRU touch ve dentry clone var.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: `O(B + name_len)`; `B` bucket length.
- Bellek karmaşıklığı: hit/miss başına bucket clone allocation.
- I/O maliyeti: yok.
- Concurrency riski: global dcache lock varsa allocation lock hold time'ı artırır.
- Cache riski: bucket `Vec` clone ve dentry clone cache baskısı üretir.

**Önerilen matematiksel model:**

```math
T_lookup = c_hash*name_len + c_bucket*B + c_alloc*B + c_clone*dentry_size
```

Optimizasyon hedefi:

```math
minimize T_lookup and allocation_count_lookup
```

Neden mantıklı: kod davranışında bucket'ın kopyalanması hit path'te bile allocation maliyeti ekler.

Uygulanabilecek optimizasyon:

- Bucket clone yerine indeksler üzerinde borrow/snapshot güvenli tarama.
- Dentry return için ref-counted handle veya small copy.
- Name hash yanında parent+name fingerprint.

Beklenen etki:

- Latency: path-heavy workload'ta yüksek.
- Memory: allocation düşer.
- CPU: clone/copy düşer.
- Throughput: VFS lookup artar.

Risk seviyesi: Orta.

Doğrulama testi: path lookup benchmark; allocation count/op, dcache hit p99.

Uygulama önceliği: P0.

#### Fonksiyon/Sınıf: `shrink`, `compact`

**Mevcut rolü:** LRU üzerinden entry eviction ve compaction yapıyor.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: `shrink` içinde `remove(0)` nedeniyle worst-case `O(N^2)`; `compact` bucket update içinde `position` ile `O(N^2)` olabilir.
- Bellek karmaşıklığı: live entry listesi.
- Concurrency riski: uzun lock hold time.
- Cache riski: `Vec` başından silme tüm elemanları kaydırır.

**Önerilen matematiksel model:**

```math
T_shrink(N,K) = sum_{i=0}^{K-1} c_shift*(N-i)
```

Optimizasyon hedefi:

```math
minimize T_shrink to O(K) or O(K log N)
```

Uygulanabilecek optimizasyon:

- LRU için intrusive linked list veya slab index queue.
- `VecDeque` ile front pop.
- Compaction mapping table: old_index -> new_index ile bucket update `O(N+B)`.

Beklenen etki: büyük cache eviction'da çok yüksek.

Risk seviyesi: Orta.

Doğrulama testi: dcache fill/evict 1k/10k/100k entry; lock hold p99.

Uygulama önceliği: P0.

#### Fonksiyon/Sınıf: `rename`

**Mevcut rolü:** Dentry rename akışı; gözlenen akışta eski ad silme ve sonrasında eski adı taşıma mantığı riskli görünüyor.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: delete + insert/steal path.
- Concurrency riski: cache coherence ve correctness riski.
- Cache riski: ikincil.

**Önerilen matematiksel model:**

```math
CorrectnessCost = P(lost_dentry) * RecoveryPenalty
```

Optimizasyon hedefi:

```math
minimize P(lost_dentry) before micro-optimizing rename
```

Neden mantıklı: rename cache path'i performans kadar tutarlılık riski taşıyor. Önce invariant yazılmalı: eski lookup miss, yeni lookup hit, inode aynı.

Uygulanabilecek optimizasyon:

- Atomic rename transaction.
- Invariant testleri.
- Bucket ve LRU index update tek aşamada.

Beklenen etki: correctness; performans ikincil.

Risk seviyesi: Yüksek.

Doğrulama testi: rename collision, overwrite, parent change, concurrent lookup model tests.

Uygulama önceliği: P0.

### Dosya: `src/fs/namei.rs`

#### Fonksiyon/Sınıf: `resolve_inner`, `split_components`, `join_path`

**Mevcut rolü:** Path normalize/resolve; component split, per-component join, symlink recursion ve dcache lookup.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: `O(D*P)` olasılığı; `D` path depth, `P` path length. Tekrar join allocation maliyeti var.
- Bellek karmaşıklığı: component Vec, joined path String, symlink remaining Vec.
- I/O maliyeti: cache miss halinde VFS lookup.
- Concurrency riski: dcache/global VFS lock süresi.
- Cache riski: kısa ömürlü allocation.

**Önerilen matematiksel model:**

```math
T_path = c_split*P + sum_{i=1}^{D}(c_join*i_avg + c_lookup + P_miss_i*T_fs)
```

Optimizasyon hedefi:

```math
minimize allocations_per_component and T_path
```

Uygulanabilecek optimizasyon:

- Path cursor/slice tabanlı traversal.
- Prefix cache: `(cwd, prefix_hash) -> inode`.
- Symlink recursion için bounded stack buffer.
- `join_path` yerine incremental hash/path id.

Beklenen etki: path-heavy workload'ta yüksek.

Risk seviyesi: Orta.

Doğrulama testi: deep path, symlink chain, random path corpus; allocations/path, lookup p99.

Uygulama önceliği: P1.

### Dosya: `src/fs/ext4.rs`

#### Fonksiyon/Sınıf: `bitmap_alloc`, `alloc_inode`

**Mevcut rolü:** Block/inode bitmaplerinde boş bit arıyor; inode allocation group taraması ve inode table zeroing içeriyor.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: group sayısı ve bitmap boyutu kadar lineer.
- Bellek karmaşıklığı: bitmap buffer.
- I/O maliyeti: bitmap/GD/inode table read/write.
- Concurrency riski: allocation bitmap race; journal ordering.
- Cache riski: bitmap byte/bit branch-heavy scan.

**Önerilen matematiksel model:**

```math
T_alloc_inode = G_scan*T_group + B_scan*T_bitmap + P_zero*T_zero_table + T_metadata_write
```

Optimizasyon hedefi:

```math
minimize E[T_alloc_inode] with free_inode_distribution known
```

Uygulanabilecek optimizasyon:

- Per-group free counters ile dolu group skip.
- Word-level `trailing_ones`/`trailing_zeros`.
- Lazy inode table zero state cache.

Beklenen etki: create-heavy workload'ta orta-yüksek.

Risk seviyesi: Orta.

Doğrulama testi: file create benchmark; bitmap bytes scanned/op, metadata writes/op.

Uygulama önceliği: P1.

#### Fonksiyon/Sınıf: `map_block_extent_tree_with_storage`

**Mevcut rolü:** Extent tree içinde logical block -> physical block mapping yapıyor; index/leaf entries lineer taranıyor.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: `O(depth * entries_per_node)`.
- I/O maliyeti: depth boyunca extent node read.
- Cache riski: metadata node locality.

**Önerilen matematiksel model:**

```math
T_map = depth*T_read_node + entries_scanned*c_compare
```

Optimizasyon hedefi:

```math
minimize entries_scanned from O(E) to O(log E)
```

Neden mantıklı: ext4 extent entries sorted davranışına dayanarak binary search uygulanabilir.

Uygulanabilecek optimizasyon:

- Index node ve leaf extents için binary search.
- Son extent cache:

```math
E[T_map] = P_hit*T_cached + (1-P_hit)*T_tree
```

Beklenen etki: büyük dosya random I/O'da yüksek.

Risk seviyesi: Düşük-Orta.

Doğrulama testi: sparse/fragmented file random read; entries scanned counter.

Uygulama önceliği: P0.

#### Fonksiyon/Sınıf: `write_file_to_storage`

**Mevcut rolü:** Dosya yazma; block mapping, gerekiyorsa allocation/extent insert, read-modify-write ve block write.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: yazılan block sayısı `B` ve map cost.
- I/O maliyeti: partial block read + write; full blockta read gereksiz olabilir.
- Concurrency riski: extent update/journal ordering.
- Cache riski: block buffer reuse.

**Önerilen matematiksel model:**

```math
T_write = B*(T_map + P_alloc*T_alloc + P_partial*T_read_block + T_write_block)
```

Optimizasyon hedefi:

```math
minimize read_amplification = reads_for_write / writes
```

Uygulanabilecek optimizasyon:

- Full-block aligned writes için read bypass.
- Extent allocation batching.
- Per-file last extent cache.

Beklenen etki: sequential write'ta yüksek, partial write'ta orta.

Risk seviyesi: Orta.

Doğrulama testi: aligned vs unaligned write benchmark; read amplification counter, fsck-style invariant checks.

Uygulama önceliği: P1.

### Dosya: `src/fs/btrfs.rs`

#### Fonksiyon/Sınıf: `logical_to_physical`, `read_logical_range`

**Mevcut rolü:** Logical address'i chunk map üzerinden physical stripe'a çeviriyor; chunk map lineer taranıyor.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: `O(C)`; `C = chunk sayısı`.
- I/O maliyeti: data read.
- Cache riski: chunk map büyürse metadata scan.

**Önerilen matematiksel model:**

```math
T_translate = C_scanned*c_compare + T_stripe_calc
```

Optimizasyon hedefi:

```math
minimize T_translate to O(log C)
```

Uygulanabilecek optimizasyon:

- Logical start'a göre sorted interval index.
- Son chunk cache.
- RAID profile için stripe math precompute.

Beklenen etki: büyük filesystem ve random read'te yüksek.

Risk seviyesi: Orta.

Doğrulama testi: chunk count sweep; translate ops/sec, wrong mapping negative tests.

Uygulama önceliği: P1.

#### Fonksiyon/Sınıf: `collect_tree_items`

**Mevcut rolü:** Btrfs tree blocklarını recursive geziyor, visited list ile tekrarları önlüyor, item data kopyalıyor.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: visited `Vec` nedeniyle `O(N^2)` risk; `N = node sayısı`.
- Bellek karmaşıklığı: item data kopyaları.
- I/O maliyeti: tree block reads.
- Concurrency riski: snapshot/generation tutarlılığı.
- Cache riski: recursive traversal locality zayıf.

**Önerilen matematiksel model:**

```math
T_collect = N*T_read_node + N^2*c_visited + bytes_items*c_copy
```

Optimizasyon hedefi:

```math
minimize visited_check from O(N) to O(1) or O(log N)
```

Uygulanabilecek optimizasyon:

- `BTreeSet`/hash set visited by logical address.
- Iterative stack ile recursion kontrolü.
- Item data için borrowed slice mümkün değilse arena.

Beklenen etki: metadata-heavy workload'ta yüksek.

Risk seviyesi: Orta.

Doğrulama testi: synthetic deep/wide Btrfs trees; visited checks, stack depth, item bytes copied.

Uygulama önceliği: P1.

#### Fonksiyon/Sınıf: `BtrfsCowWriter::alloc_block`, `free_block`, `merge_free_space`, `commit_transaction`

**Mevcut rolü:** COW block allocation/free ve transaction commit; free-space list sort/merge, mirrors write.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: allocation first-fit `O(F)`, free merge sort `O(F log F)`, commit allocated extent taraması.
- Bellek karmaşıklığı: free-space extent list.
- I/O maliyeti: superblock mirror writes.
- Concurrency riski: transaction atomicity.

**Önerilen matematiksel model:**

```math
Frag = sum_i gap_i^2 / total_free^2
T_alloc = F_scan*c_compare + size_zero*c_zero
```

Optimizasyon hedefi:

```math
minimize T_alloc + omega*Frag
```

Uygulanabilecek optimizasyon:

- Size-segregated free-space buckets.
- Best-fit for medium blocks, first-fit for large sequential append.
- Commit'te used-bytes hesabını mirror döngüsü dışına almak.

Beklenen etki: write-heavy COW workload'ta orta-yüksek.

Risk seviyesi: Orta.

Doğrulama testi: random allocate/free corpus; fragmentation index, commit time, image invariant checks.

Uygulama önceliği: P2.

### Dosya: `src/fs/vfs_unified.rs`, `src/fs/f2fs.rs`, `src/fs/ntfs.rs`

#### Fonksiyon/Sınıf: VFS dispatch, F2FS/NTFS metadata operations

**Mevcut rolü:** VFS üst katmanı ve alternatif dosya sistemi sürücüleri.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: path, inode, block mapping ve metadata segment sayısına bağlı.
- Bellek karmaşıklığı: inode/dentry/cache state.
- I/O maliyeti: storage metadata read/write.
- Concurrency riski: VFS lock ordering ve per-FS metadata tutarlılığı.
- Cache riski: metadata cache locality.

**Önerilen matematiksel model:**

```math
T_vfs_op = T_path + T_inode + T_fs_dispatch + T_block_map + T_io
```

Optimizasyon hedefi:

```math
minimize T_vfs_op and write_amplification
```

Uygulanabilecek optimizasyon:

- VFS op counters ile hot operation ayrımı.
- FS-specific block mapping cache.
- Metadata prefetch/read-ahead.
- Çok dosya sistemli benchmark corpus.

Beklenen etki: workload bağımlı.

Risk seviyesi: Orta-Yüksek.

Doğrulama testi: POSIX file operation corpus, fs-specific invariant tests, crash consistency simulation.

Uygulama önceliği: P2.

### Dosya: `src/net/tcp.rs`

#### Fonksiyon/Sınıf: `process_packet`, `process_ipv6_packet`

**Mevcut rolü:** TCP packet checksum/parse sonrası global connection table üzerinde port/address eşleştirme yapıyor; bağlantı/listener/accept queue state'i güncelliyor.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: connection sayısı `C_conn` ile `O(C_conn)`.
- Bellek karmaşıklığı: geçici packet/connection state.
- I/O maliyeti: NIC path upstream.
- Concurrency riski: global table lock ve re-lock pattern.
- Cache riski: bağlantı listesi lineer scan, poor locality.

**Önerilen matematiksel model:**

```math
E[T_demux] = c_hash + (1 + lambda_bucket)*c_compare
```

Değişkenler:

- `lambda_bucket = C_conn / H`: hash bucket load factor.
- `H`: bucket sayısı.
- `c_hash`: 4-tuple hash maliyeti.
- `c_compare`: candidate compare maliyeti.

Optimizasyon hedefi:

```math
minimize E[T_demux] and lock_hold_time
```

Neden mantıklı: mevcut davranış bağlantı sayısına lineer; TCP demux doğal olarak 4-tuple key lookup problemidir.

Uygulanabilecek optimizasyon:

- `(local_ip, local_port, remote_ip, remote_port)` hash table.
- Listener lookup için `(local_ip, local_port)` ayrı tablo.
- RCU read-side lookup, update path lock/shard.
- TIME_WAIT için ayrı compact table.

Beklenen etki:

- Latency: çok bağlantıda çok yüksek.
- Memory: hash table kadar artar.
- CPU: lineer scan kalkar.
- Throughput: yüksek artış.

Risk seviyesi: Orta.

Doğrulama testi: 1/100/10k/100k bağlantı demux benchmark; packets/sec, lock hold, p99.

Uygulama önceliği: P0.

#### Fonksiyon/Sınıf: `TcpConnection::send_packet`, `on_packet`

**Mevcut rolü:** Segment oluşturma, IPv4/IPv6 serialize, rx buffer append, ACK/congestion update.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: packet payload boyutu.
- Bellek karmaşıklığı: send path'te segment `Vec`, IPv4 buffer `Vec`, IPv6 serialize `Vec`.
- I/O maliyeti: network send.
- Concurrency riski: ACK ve congestion state update.
- Cache riski: per-packet allocation.

**Önerilen matematiksel model:**

```math
T_send = T_alloc_segment + T_copy_payload + T_checksum + T_ip_serialize + T_nic
```

Optimizasyon hedefi:

```math
minimize allocations_per_packet and copies_per_byte
```

Uygulanabilecek optimizasyon:

- MTU-sized scratch buffer pool.
- Scatter-gather packet builder.
- Delayed ACK:

```math
ACK_now if bytes_unacked > B_ack or timer > T_ack or out_of_order = true
```

Beklenen etki: small packet PPS'te yüksek.

Risk seviyesi: Orta.

Doğrulama testi: small packet send/receive, allocation counter, ACK ratio, retransmit count.

Uygulama önceliği: P1.

#### Fonksiyon/Sınıf: `BbrState` ve `update_rtt`

**Mevcut rolü:** RTT EWMA ve BBR benzeri bandwidth/min RTT/cwnd kontrolü.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: ACK başına `O(1)`.
- Bellek karmaşıklığı: connection state.
- Concurrency riski: yanlış gain cycle throughput/latency dalgalanması.
- Cache riski: düşük.

**Önerilen matematiksel model:**

```math
srtt = (1-alpha)*srtt + alpha*rtt_sample
rttvar = (1-beta)*rttvar + beta*abs(srtt-rtt_sample)
```

```math
cwnd_target = gain * bw_est * min_rtt
```

Optimizasyon hedefi:

```math
maximize delivery_rate - loss_penalty*loss_rate - latency_penalty*queue_delay
```

Uygulanabilecek optimizasyon:

- Gain cycle ilerlemesini round-trip bazlı doğrulama.
- Min RTT aging.
- Loss/ECN sinyaliyle pacing gain düşürme.

Beklenen etki: WAN-like workload'ta orta-yüksek.

Risk seviyesi: Orta-Yüksek.

Doğrulama testi: network simulator; RTT/loss/bandwidth sweep, fairness vs Reno/CUBIC.

Uygulama önceliği: P2.

### Dosya: `src/net/io_uring.rs`

#### Fonksiyon/Sınıf: `enqueue_sqe`, `submit_sqes`, `get_cqe`, `wait_cqe`, `selected_buffer_for_group`

**Mevcut rolü:** SQ/CQ ring, pending operation map, registered buffer selection ve SQPOLL benzeri processing.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: pending `BTreeMap` ise `O(log P)`, buffer group scan `O(B)`.
- Bellek karmaşıklığı: pending map, registered buffers, rings.
- Concurrency riski: SQ/CQ head-tail consistency.
- Cache riski: BTree pointer chasing.

**Önerilen matematiksel model:**

```math
T_submit = S*(T_sqe_write + T_pending_insert) + T_process_budget
T_cqe = T_cqe_read + T_pending_remove
```

Optimizasyon hedefi:

```math
minimize T_pending_insert/remove and W_queue
```

Uygulanabilecek optimizasyon:

- User_data lower bits veya slab slot id ile O(1) pending table.
- Buffer group -> freelist indeks.
- SQPOLL budget:

```math
budget = clamp(ceil(lambda_submit * W_target), B_min, B_max)
```

Beklenen etki: ring-heavy I/O'da yüksek.

Risk seviyesi: Orta.

Doğrulama testi: io_uring submit/complete microbench; ops/sec, p99 wait, BTree node allocations.

Uygulama önceliği: P1.

### Dosya: `src/net/http2.rs`

#### Fonksiyon/Sınıf: `HpackEncoder::encode`, `HpackDecoder::decode`

**Mevcut rolü:** HPACK header encode/decode; static table linear scan, dynamic table insert ve header map allocation.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: header count * static table size; dynamic insert `O(D)` başa insert ise.
- Bellek karmaşıklığı: `String`, `BTreeMap`, decoded buffer.
- Concurrency riski: düşük.
- Cache riski: header allocation yoğun.

**Önerilen matematiksel model:**

```math
Benefit_header(h) = freq(h)*bytes_saved(h) - size(h)*memory_cost - insert_cost(h)
```

Optimizasyon hedefi:

```math
maximize sum_h Benefit_header(h)
```

Uygulanabilecek optimizasyon:

- Static table perfect hash.
- Dynamic table ring buffer.
- Huffman decode için table-driven streaming.
- Header map yerine small-vector pair list, sonra gerekiyorsa map.

Beklenen etki: HTTP2 request yoğunluğunda orta-yüksek.

Risk seviyesi: Orta.

Doğrulama testi: HPACK corpus; bytes encoded, allocations/request, decode ns/header.

Uygulama önceliği: P1.

#### Fonksiyon/Sınıf: `Http2Connection::process_frame`, `build_request`

**Mevcut rolü:** Frame processing, stream data append, request map build.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: frame payload bytes.
- Bellek karmaşıklığı: stream buffer ve request map.
- Concurrency riski: flow-control window correctness.
- Cache riski: per-frame allocations.

**Önerilen matematiksel model:**

```math
W_stream = bytes_in_flight / window_size
Backpressure if W_stream > theta
```

Optimizasyon hedefi:

```math
maximize throughput while memory_per_stream <= M_stream_budget
```

Uygulanabilecek optimizasyon:

- Stream data için chunked buffer.
- Flow-control update threshold.
- Frame payload length guard invariant; emin değilim tüm çağrılarda garanti ediliyor mu, test şart.

Beklenen etki: çok streamli workload'ta orta.

Risk seviyesi: Orta.

Doğrulama testi: HTTP2 multiplex load, malformed frame corpus, memory/stream.

Uygulama önceliği: P2.

### Dosya: `src/net/tls.rs`, `src/net/x509.rs`, `src/net/dnssec.rs`, `src/net/ipsec.rs`

#### Fonksiyon/Sınıf: TLS handshake/record, X.509 parse/verify, DNSSEC validation, IPsec packet transform

**Mevcut rolü:** Kripto protokol parse/doğrulama ve packet transform.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: certificate chain length, ASN.1 nodes, signature count, packet bytes.
- Bellek karmaşıklığı: parsed structures, temporary `Vec`/maps.
- I/O maliyeti: network path.
- Concurrency riski: cache invalidation ve time validity.
- Cache riski: parser allocation ve crypto data locality.

**Önerilen matematiksel model:**

```math
T_verify = C_chain*T_cert_parse + S_sig*T_signature + R_revocation*T_revocation + T_policy
```

Optimizasyon hedefi:

```math
minimize T_verify while false_accept_rate = 0
```

Uygulanabilecek optimizasyon:

- DER arena parser.
- Certificate/public-key verification cache:

```math
E[T] = P_cache_hit*T_hit + (1-P_cache_hit)*T_verify
```

- DNSSEC RRset canonical form cache.
- IPsec SA lookup hash table.

Beklenen etki: handshake-heavy ve DNSSEC-heavy workload'ta yüksek.

Risk seviyesi: Yüksek.

Doğrulama testi: Wycheproof-like crypto corpus, malformed ASN.1, chain cache hit/miss benchmark, constant-time checks.

Uygulama önceliği: P1.

### Dosya: `src/net/grpc.rs`

#### Fonksiyon/Sınıf: protobuf encode/decode, stream body handling

**Mevcut rolü:** gRPC frame/protobuf serialization ve stream body yönetimi.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: message bytes ve field count.
- Bellek karmaşıklığı: encoded/decoded `Vec`, maps.
- I/O maliyeti: HTTP2/TLS alt katmanı.
- Cache riski: serialization copy.

**Önerilen matematiksel model:**

```math
T_rpc = T_h2 + T_tls + T_proto_parse + copies*bytes*c_copy
```

Optimizasyon hedefi:

```math
minimize copies and allocations per RPC
```

Uygulanabilecek optimizasyon:

- Streaming decoder.
- Field descriptor cache.
- Zero-copy slices for length-delimited fields.

Beklenen etki: büyük message workload'ta orta-yüksek.

Risk seviyesi: Orta.

Doğrulama testi: protobuf corpus; allocations/RPC, bytes copied/RPC, p99.

Uygulama önceliği: P2.

### Dosya: `src/gui/damage.rs`

#### Fonksiyon/Sınıf: `DamageTracker::mark_rect`, `take`, `partial_redraw_cost`

**Mevcut rolü:** Dirty rect normalize/merge, threshold üstünde tile compaction, partial vs full redraw maliyet tahmini.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: rect sayısı küçükken normalize `O(R^2)`, tile compaction `O(Tiles)`.
- Bellek karmaşıklığı: rect list ve tile occupancy bitmap.
- Concurrency riski: display pipeline state.
- Cache riski: tile occupancy allocation.

**Önerilen matematiksel model:**

```math
C_partial = c_px*sum_i area_i + c_rect*R + c_tile*T_dirty
C_full = c_px*A_screen
```

Optimizasyon hedefi:

```math
choose partial iff C_partial < tau*C_full
```

Değişkenler:

- `area_i`: dirty rect alanı.
- `R`: dirty rect sayısı.
- `T_dirty`: dirty tile sayısı.
- `A_screen`: ekran alanı.
- `c_px,c_rect,c_tile`: ölçümden gelen maliyet katsayıları.
- `tau`: güvenlik katsayısı.

Neden mantıklı: mevcut kodda sabit eşik var; gerçek maliyet pixel alanı kadar batch/rect overhead'e de bağlı.

Uygulanabilecek optimizasyon:

- Runtime EWMA ile `c_px,c_rect,c_tile` kalibrasyonu.
- Tile occupancy buffer reuse.
- Rect merge kararında overlap ratio:

```math
merge if area(union) < area(a)+area(b)+merge_overhead/c_px
```

Beklenen etki:

- Latency: GUI p99 düşer.
- Memory: buffer reuse ile allocation düşer.
- CPU: gereksiz full redraw azalır.
- Throughput: frame rate stabilitesi artar.

Risk seviyesi: Düşük-Orta.

Doğrulama testi: animated windows, small cursor moves, full-screen changes; frame time p95/p99, pixels redrawn/frame.

Uygulama önceliği: P0.

### Dosya: `src/gfx/tile_renderer.rs`

#### Fonksiyon/Sınıf: tile selection, `render_to_framebuffer`, `select_level`

**Mevcut rolü:** Dirty tile ve render level seçimiyle framebuffer üretimi.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: tile count ve primitive count.
- Bellek karmaşıklığı: tile buffers.
- I/O maliyeti: framebuffer write.
- Cache riski: tile traversal order ve framebuffer stride.

**Önerilen matematiksel model:**

```math
T_render = T_tiles + T_primitives + T_blit
= N_tiles*c_tile + N_prims*c_prim + bytes_fb/BW_mem
```

Optimizasyon hedefi:

```math
minimize T_render and maximize cache_line_reuse
```

Uygulanabilecek optimizasyon:

- Tile order = memory stride order.
- Primitive binning.
- Level selection based on estimated overdraw:

```math
level = argmin_l (quality_loss_l*Q_weight + render_cost_l)
```

Beklenen etki: orta-yüksek.

Risk seviyesi: Orta.

Doğrulama testi: render scenes corpus; cache miss counter, frame time.

Uygulama önceliği: P1.

### Dosya: `src/gfx/simd.rs`

#### Fonksiyon/Sınıf: SIMD copy/blend/fill/blur helpers

**Mevcut rolü:** Pixel operations için SIMD hızlandırma.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: bytes/pixels.
- Bellek karmaşıklığı: `O(1)`.
- I/O maliyeti: memory bandwidth.
- Cache riski: alignment ve non-temporal threshold.

**Önerilen matematiksel model:**

```math
T_copy(n) = T_setup + n/BW_path + T_tail(n mod vector_width)
```

Optimizasyon hedefi:

```math
select path = argmin_path T_copy_path(n, alignment)
```

Uygulanabilecek optimizasyon:

- Runtime dispatch by size/alignment/CPU features.
- Non-temporal store yalnızca `n > n_star` ve reuse düşükse.
- Blur için separable kernel ve tile halo reuse.

Beklenen etki: büyük blit/fill'de yüksek.

Risk seviyesi: Orta.

Doğrulama testi: size/alignment sweep, bandwidth GB/s, visual checksum.

Uygulama önceliği: P1.

### Dosya: `src/gui/renderer.rs`, `src/services/ech_display.rs`

#### Fonksiyon/Sınıf: render frame compile/present, compositor snapshot/order

**Mevcut rolü:** UI object render planı, compositor order, display present.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: object/window count, dirty regions, text glyphs.
- Bellek karmaşıklığı: render command buffers/snapshots.
- I/O maliyeti: framebuffer/display handoff.
- Concurrency riski: snapshot state invalidation.
- Cache riski: command list traversal.

**Önerilen matematiksel model:**

```math
T_frame = T_snapshot + T_layout + T_raster + T_text + T_present
```

Optimizasyon hedefi:

```math
minimize p99(T_frame) and missed_vsync_count
```

Uygulanabilecek optimizasyon:

- Render command caching by stable node id.
- Text glyph atlas hit ratio optimization.
- Present plan diffing:

```math
RedrawUtility(node) = visual_delta(node) * area(node) / render_cost(node)
```

Beklenen etki: UI workload'ta orta-yüksek.

Risk seviyesi: Orta.

Doğrulama testi: compositor scenes; frame time, glyph atlas hit, redraw area.

Uygulama önceliği: P1.

### Dosya: `src/win32.rs`

#### Fonksiyon/Sınıf: `init_api_table`, `get_proc_address`, fallback handler `stub_api`

**Mevcut rolü:** Win32 modül/API isimlerini runtime `BTreeMap<String, BTreeMap<String, fn>>` ile kuruyor ve isimle lookup yapıyor; desteklenmeyen API'ler fallback handler'a bağlanabiliyor.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: init sırasında `O(A log A)` insert ve çok sayıda string allocation; lookup `O(log M + log A)`.
- Bellek karmaşıklığı: module/API string ve tree node allocation.
- I/O maliyeti: yok.
- Concurrency riski: global table mutex.
- Cache riski: BTree pointer chasing.

**Önerilen matematiksel model:**

```math
T_init_api = A*(T_string_alloc + T_tree_insert)
T_lookup = T_module_lookup + T_api_lookup
```

Optimizasyon hedefi:

```math
minimize T_init_api and T_lookup without changing exported API semantics
```

Uygulanabilecek optimizasyon:

- Build-time generated sorted static table.
- Perfect hash veya two-level enum dispatch.
- Fallback handler'a giden API'leri ayrı support bitmap ile raporlama.

Beklenen etki:

- Startup: yüksek düşüş.
- Memory: yüksek düşüş.
- CPU: lookup düşük.
- Complexity: build step artar.

Risk seviyesi: Düşük-Orta.

Doğrulama testi: API table init ns, allocations, import resolution corpus.

Uygulama önceliği: P1.

### Dosya: `src/pe_loader.rs`

#### Fonksiyon/Sınıf: PE parse, section mapping, import resolution, relocation, TLS callbacks

**Mevcut rolü:** PE binary load ve Win32 import/relocation işlemleri.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: section count, import count, relocation entries.
- Bellek karmaşıklığı: mapped sections ve temporary parse buffers.
- I/O maliyeti: executable read.
- Concurrency riski: address-space/VMA update.
- Cache riski: relocation table scan.

**Önerilen matematiksel model:**

```math
T_load = S*T_section + I*T_import_lookup + R*T_reloc + TLS*T_callback
```

Optimizasyon hedefi:

```math
minimize T_load and page_faults_on_startup
```

Uygulanabilecek optimizasyon:

- Import lookup cache with Win32 static table.
- Relocation block streaming.
- Demand mapping for cold sections, ancak correctness ve fault path hazırsa.

Beklenen etki: process startup'ta orta-yüksek.

Risk seviyesi: Orta.

Doğrulama testi: PE corpus; load time, page faults, import lookup count.

Uygulama önceliği: P2.

### Dosya: `src/shell/mod.rs`, `src/shell/advanced.rs`

#### Fonksiyon/Sınıf: command dispatch, tokenizer/parser, glob/history/completion

**Mevcut rolü:** Shell komut işleme, scripting, glob expansion, completion ve history.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: input length, command count, filesystem glob breadth.
- Bellek karmaşıklığı: token/AST/string allocation.
- I/O maliyeti: glob ve command file access.
- Concurrency riski: state/history locks.
- Cache riski: büyük dispatcher branch predictability.

**Önerilen matematiksel model:**

```math
T_shell = T_tokenize(n) + T_parse(tokens) + T_expand(glob) + T_dispatch
```

```math
T_glob = sum_{depth=0}^{D} dirs_depth * entries_per_dir * match_cost
```

Optimizasyon hedefi:

```math
minimize T_expand and bound worst_case_glob
```

Uygulanabilecek optimizasyon:

- Command trie/perfect hash.
- Glob breadth/depth budget ve streaming expansion.
- Parser arena.
- Completion cache invalidated by directory mtime/generation.

Beklenen etki: interactive responsiveness orta.

Risk seviyesi: Düşük-Orta.

Doğrulama testi: long script, pathological glob, completion latency p95.

Uygulama önceliği: P2.

### Dosya: `src/posix.rs`, `src/win32_abi.rs`

#### Fonksiyon/Sınıf: syscall/ABI translation paths

**Mevcut rolü:** User/kernel ABI boundary, POSIX/Win32 compatibility.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: syscall türüne bağlı.
- Bellek karmaşıklığı: argument conversion buffers.
- I/O maliyeti: syscall target.
- Concurrency riski: user pointer validation ve state locks.
- Cache riski: dispatch table locality.

**Önerilen matematiksel model:**

```math
T_syscall = T_entry + T_validate_args + T_dispatch + T_body + T_exit
```

Optimizasyon hedefi:

```math
minimize T_validate_args + T_dispatch while safety_violation = 0
```

Uygulanabilecek optimizasyon:

- Syscall dispatch table dense indexing.
- User pointer validation range cache per address space.
- Copyin/copyout batch validation.

Beklenen etki: syscall-heavy workload'ta orta-yüksek.

Risk seviyesi: Yüksek.

Doğrulama testi: syscall microbench; invalid pointer fuzz, Spectre boundary tests.

Uygulama önceliği: P1.

### Dosya: `src/userland/ech_db.rs`

#### Fonksiyon/Sınıf: query/storage/index operations

**Mevcut rolü:** Userland database-like storage/query altyapısı.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: query predicate ve record count'a bağlı.
- Bellek karmaşıklığı: index/table buffers.
- I/O maliyeti: storage backend.
- Concurrency riski: transaction/visibility state.
- Cache riski: scan vs index locality.

**Önerilen matematiksel model:**

```math
Cost(query) = rows_scanned*c_scan + index_lookups*c_index + bytes_io/BW + result_bytes*c_copy
```

Optimizasyon hedefi:

```math
choose plan = argmin Cost(plan)
```

Uygulanabilecek optimizasyon:

- Simple cardinality stats.
- Covering index for hot queries.
- Batch writes and WAL group commit if durability layer exists.

Beklenen etki: query-heavy workload'ta yüksek, mevcut kullanım yoğunluğuna emin değilim.

Risk seviyesi: Orta.

Doğrulama testi: synthetic query corpus; rows scanned/query, index hit, p99.

Uygulama önceliği: P3.

### Dosya: `src/security/seed_store.rs`, `src/security/tuf.rs`, `src/security/package.rs`

#### Fonksiyon/Sınıf: seed/package metadata verify, TUF role validation

**Mevcut rolü:** Güvenli metadata, imza ve package doğrulama.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: metadata size, role count, signature count.
- Bellek karmaşıklığı: parsed metadata.
- I/O maliyeti: package metadata read.
- Concurrency riski: trust root update atomicity.
- Cache riski: parser allocation.

**Önerilen matematiksel model:**

```math
T_tuf = R*T_role_parse + S*T_sig_verify + E*T_expiry_check + D*T_delegation
```

Optimizasyon hedefi:

```math
minimize T_tuf while rollback_acceptance = 0
```

Uygulanabilecek optimizasyon:

- Verified metadata cache keyed by digest.
- Incremental role validation.
- Signature batch verify yalnızca algoritma uygunsa.

Beklenen etki: package update path'te orta.

Risk seviyesi: Orta-Yüksek.

Doğrulama testi: TUF conformance corpus; rollback/freeze/mix-and-match attacks.

Uygulama önceliği: P2.

### Dosya: `src/compression/lz4.rs`, `src/compression/lzo1x.rs`, `src/compression/zstd.rs`, `src/compression/deflate.rs`

#### Fonksiyon/Sınıf: compression/decompression hot loops

**Mevcut rolü:** Sıkıştırma/açma algoritmaları; storage/network/memory pressure paths'e bağlanabilir.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: input bytes, match search/window.
- Bellek karmaşıklığı: history/window.
- Cache riski: sliding window locality.

**Önerilen matematiksel model:**

```math
Utility_compress = IO_saved(bytes,ratio) - CPU_cost(level,input_entropy)
```

Optimizasyon hedefi:

```math
choose level = argmax Utility_compress
```

Uygulanabilecek optimizasyon:

- Entropy sampling ile compression level seçimi.
- Small block bypass threshold:

```math
compress iff bytes*(1-ratio_est)/BW_io > T_cpu_compress
```

Beklenen etki: zswap/storage/network pipeline'da workload bağımlı.

Risk seviyesi: Orta.

Doğrulama testi: Calgary/Silesia-like corpus; cycles/byte, ratio, end-to-end I/O time.

Uygulama önceliği: P2.

### Dosya: `src/ml/onnx_runtime.rs`

#### Fonksiyon/Sınıf: tensor graph execution/operators

**Mevcut rolü:** ONNX benzeri graph/tensor runtime.

**Mevcut tahmini maliyet:**

- Zaman karmaşıklığı: operator FLOPs ve tensor shape.
- Bellek karmaşıklığı: activation buffers.
- Cache riski: tensor layout, tiling, SIMD.

**Önerilen matematiksel model:**

```math
T_op = FLOPs / PeakFLOPs_eff + bytes_moved / BW_mem
```

Optimizasyon hedefi:

```math
minimize max(T_compute, T_memory)
```

Uygulanabilecek optimizasyon:

- Operator fusion.
- Tensor arena reuse by liveness intervals.
- Matmul/conv tiling:

```math
tile = argmin bytes_moved(tile) subject to tile_working_set <= L1_or_L2
```

Beklenen etki: ML workload varsa yüksek; kullanım sıklığına emin değilim.

Risk seviyesi: Orta.

Doğrulama testi: ONNX operator corpus; cycles/op, memory peak, numerical diff.

Uygulama önceliği: P3.

### Dosya: `sdk/*`, `tools/arch_guard/*`, `benches/*`

#### Fonksiyon/Sınıf: SDK ABI, guard tooling, benchmark harness

**Mevcut rolü:** Host/tooling ve ölçüm altyapısı.

**Mevcut tahmini maliyet:**

- Runtime kernel hot path etkisi düşük.
- Benchmark doğruluğu kritik.

**Önerilen matematiksel model:**

```math
MeasurementError = abs(metric_observed - metric_true) / metric_true
```

Optimizasyon hedefi:

```math
minimize MeasurementError before optimizing kernel code
```

Uygulanabilecek optimizasyon:

- Benchmark warmup/iteration standardization.
- Counter names and units consistency.
- Regression thresholds for p50/p95/p99.

Beklenen etki: optimizasyon karar kalitesi artar.

Risk seviyesi: Düşük.

Doğrulama testi: repeated benchmark variance; CI trend stability.

Uygulama önceliği: P1.

---

## 5. Fonksiyon Bazlı Matematiksel Optimizasyonlar

| Dosya | Fonksiyon | Mevcut karmaşıklık | Önerilen denklem/model | Optimizasyon türü | Beklenen kazanç | Risk | Öncelik |
|---|---|---:|---|---|---|---|---|
| `src/task/scheduler.rs` | `choose_spawn_cpu` | `O(C)` | `T_spawn=C*(c_online+c_affinity+c_queue+c_topology)` | Per-NUMA load index | Yüksek | Yüksek | P0 |
| `src/task/scheduler.rs` | `choose_victim_cpu` | `O(C)` | `Score=aQ-bD-cM-dCollision` | Victim top-k/bitmap | Yüksek | Yüksek | P0 |
| `src/task/scheduler.rs` | `schedule` | `O(1)` / steal `O(C+Q)` | `L_switch=sum components` | p99 component budget | Orta-Yüksek | Yüksek | P1 |
| `src/memory/mod.rs` | `allocate_frame_with_context` | fast `O(1)`, miss `O(R+T)` | `T_alloc=P_hit*T_pmm+(1-P_hit)*...` | Pre-reclaim, OOM cache | Yüksek | Yüksek | P0 |
| `src/memory/pmm.rs` | `allocate_contiguous` | `O(F*P)` | `E[T_contig]=sum P(run<P)*c` | Buddy/run summary | Yüksek | Orta-Yüksek | P1 |
| `src/cpu/smp.rs` | `send_tlb_shootdown_ipi` | `O(C_target)` | `T=C*T_ipi+B*T_invlpg+T_ack` | Shootdown batching | Yüksek | Yüksek | P1 |
| `src/drivers/nvme.rs` | `NvmeQueue::submit` | `O(1)` | `T_per_cmd=(B*T_cmd+T_fence+T_db)/B` | Doorbell batching | Yüksek | Yüksek | P1 |
| `src/drivers/nvme.rs` | `submit_io_command` | wait-bound | `C_wait=p_fast*T_poll+...` | Poll-then-sleep | Yüksek | Yüksek | P0 |
| `src/drivers/jail_ring.rs` | `push_batch/pop_batch` | `O(B)` | `T_msg=(T_fence+B*T_copy+T_notify)/B` | Adaptive batching | Orta | Orta | P1 |
| `src/fs/dcache.rs` | `lookup` | `O(B)+alloc` | `T=c_hash*n+c_bucket*B+c_alloc*B` | No-clone lookup | Yüksek | Orta | P0 |
| `src/fs/dcache.rs` | `shrink` | `O(N^2)` | `T=sum c_shift*(N-i)` | VecDeque/intrusive LRU | Çok yüksek | Orta | P0 |
| `src/fs/namei.rs` | `resolve_inner` | `O(D*P)` | `T=c_split*P+sum(c_join*i+c_lookup)` | Path cursor/prefix cache | Yüksek | Orta | P1 |
| `src/fs/ext4.rs` | `map_block_extent_tree_with_storage` | `O(depth*E)` | `T=depth*T_read+E*c_compare` | Binary search | Yüksek | Düşük-Orta | P0 |
| `src/fs/ext4.rs` | `write_file_to_storage` | `O(B*T_map)` | `T=B*(T_map+P_partial*T_read+T_write)` | Full-block bypass | Orta-Yüksek | Orta | P1 |
| `src/fs/btrfs.rs` | `logical_to_physical` | `O(C)` | `T=C*c_compare+T_stripe` | Interval index | Yüksek | Orta | P1 |
| `src/fs/btrfs.rs` | `collect_tree_items` | `O(N^2)` risk | `T=N*T_read+N^2*c_visited` | Visited set/arena | Yüksek | Orta | P1 |
| `src/net/tcp.rs` | `process_packet` | `O(C_conn)` | `E[T]=c_hash+(1+lambda)*c_compare` | 4-tuple hash | Çok yüksek | Orta | P0 |
| `src/net/tcp.rs` | `send_packet` | `O(payload)+alloc` | `T=T_alloc+T_copy+T_checksum+T_ip` | Buffer pool/scatter-gather | Yüksek | Orta | P1 |
| `src/net/io_uring.rs` | `enqueue_sqe/get_cqe` | `O(log P)` | `T=T_ring+T_pending` | Slot table | Yüksek | Orta | P1 |
| `src/net/http2.rs` | `HpackEncoder::encode` | `O(H*S)` | `Benefit=freq*bytes_saved-size*cost` | Static hash/dynamic score | Orta-Yüksek | Orta | P1 |
| `src/gui/damage.rs` | `take/partial_redraw_cost` | `O(R^2)` small, `O(T)` tile | `C_partial<c_full` | Adaptive threshold | Yüksek | Düşük-Orta | P0 |
| `src/gfx/simd.rs` | copy/blend/fill | `O(n)` | `T=T_setup+n/BW+T_tail` | Runtime path threshold | Orta-Yüksek | Orta | P1 |
| `src/win32.rs` | `init_api_table` | `O(A log A)+alloc` | `T=A*(alloc+insert)` | Static table | Yüksek startup | Düşük-Orta | P1 |
| `src/pe_loader.rs` | import/reloc | `O(I+R)` | `T=S*T_sec+I*T_import+R*T_reloc` | Import cache/streaming | Orta | Orta | P2 |
| `src/shell/advanced.rs` | glob/parser | worst high | `T_glob=sum dirs*entries*match` | Budget/cache/trie | Orta | Düşük-Orta | P2 |

---

## 6. Yeni Denklem Adayları

### 6.1 NUMA-Aware Steal Utility

Kullanım alanı: `src/task/scheduler.rs::choose_victim_cpu`

Formül:

```math
U_steal(v) = a*Q_v - b*NUMA_distance(v) - c*MemPressure_v - d*RecentFailedSteals_v
```

Değişkenler:

- `Q_v`: victim queue runnable task sayısı.
- `NUMA_distance(v)`: local CPU ile victim arasındaki topology uzaklığı.
- `MemPressure_v`: victim node pressure metriği.
- `RecentFailedSteals_v`: kısa penceredeki başarısız steal sayısı.

Hedef: `maximize U_steal(v)`; başarılı steal olasılığı yüksek, cache uzaklığı düşük victim seçmek.

Kod karşılığı: per-node victim candidate bitmap + failure EWMA.

Doğrulama: steal success ratio, remote NUMA steal oranı, p99 schedule latency.

Risk: Yüksek.

### 6.2 Allocation Miss Expected Cost

Kullanım alanı: `src/memory/mod.rs::allocate_frame_with_context`

Formül:

```math
E[T_alloc] = P_hit*T_fast + (1-P_hit)*(T_reclaim + P_oom*T_oom)
```

Hedef: `P_hit` artırmak, miss path maliyetini allocation çağrısından önce dağıtmak.

Kod karşılığı: watermark pre-reclaim, incremental OOM score cache.

Doğrulama: allocation p99, `P_hit`, reclaim latency.

Risk: Yüksek.

### 6.3 NVMe Doorbell Amortization

Kullanım alanı: `src/drivers/nvme.rs::NvmeQueue::submit`

Formül:

```math
Cost_per_cmd(B) = (T_fence + T_doorbell + B*T_sqe) / B + QueueDelay(B)
```

Hedef: `minimize Cost_per_cmd(B)` ve latency budget'ı aşmamak.

Kod karşılığı: adaptive batching, per-queue doorbell budget.

Doğrulama: IOPS, p99 latency, MMIO writes/op.

Risk: Yüksek.

### 6.4 Dcache Eviction Utility

Kullanım alanı: `src/fs/dcache.rs::shrink`

Formül:

```math
EvictScore(d) = a*Age_d - b*HitRate_d - c*Dirty_d - e*PathDepthPenalty_d
```

Hedef: `maximize EvictScore` ile en düşük yeniden kullanım değerli dentry'yi atmak.

Kod karşılığı: LRU + frequency bit hibriti.

Doğrulama: dcache hit ratio, eviction cost, lookup p99.

Risk: Orta.

### 6.5 Path Prefix Cache Utility

Kullanım alanı: `src/fs/namei.rs::resolve_inner`

Formül:

```math
U_prefix(p) = freq(p)*saved_components(p)*T_lookup - memory(p)*M_cost - invalidations(p)*I_cost
```

Hedef: cache'e alınacak path prefixlerini seçmek.

Kod karşılığı: cwd/root generation keyed prefix cache.

Doğrulama: path allocations, lookup p99, invalidation count.

Risk: Orta.

### 6.6 TCP Demux Hash Load

Kullanım alanı: `src/net/tcp.rs::process_packet`

Formül:

```math
E[lookup] = 1 + lambda_bucket
lambda_bucket = connections / buckets
```

Hedef: `lambda_bucket <= 1.5` tutmak.

Kod karşılığı: resizable/sharded 4-tuple table.

Doğrulama: packet demux ns vs connection count.

Risk: Orta.

### 6.7 Adaptive Damage Redraw

Kullanım alanı: `src/gui/damage.rs`

Formül:

```math
DrawPartial = (c_px*A_dirty + c_rect*R + c_tile*T_dirty) < tau*c_px*A_screen
```

Hedef: frame time ve redraw bandwidth'i azaltmak.

Kod karşılığı: measured EWMA coefficients.

Doğrulama: pixels/frame, frame p99, missed vsync.

Risk: Düşük-Orta.

### 6.8 HPACK Dynamic Table Benefit

Kullanım alanı: `src/net/http2.rs::HpackEncoder`

Formül:

```math
Benefit(h) = freq(h)*bytes_saved(h) - size(h)*memory_cost - eviction_cost(h)
```

Hedef: dynamic table'a yalnızca pozitif beklenen faydalı header eklemek.

Kod karşılığı: header frequency counters ve ring table.

Doğrulama: encoded bytes/request, allocations/header.

Risk: Orta.

### 6.9 TLB Shootdown Batch Decision

Kullanım alanı: `src/cpu/smp.rs::send_tlb_shootdown_ipi`

Formül:

```math
Batch if k*T_ipi_saved > StaleRisk(window) + DelayCost(window)
```

Hedef: IPI sayısını düşürürken stale translation riskini sıfır güvenlik sınırında tutmak.

Kod karşılığı: address-space scoped invalidation queue.

Doğrulama: shootdown count, stale access litmus.

Risk: Yüksek.

### 6.10 Compression Decision Utility

Kullanım alanı: `src/compression/*`, `src/memory/zswap.rs`

Formül:

```math
Compress if bytes*(1-ratio_est)/BW_io > cycles_compress/CPU_freq + latency_penalty
```

Hedef: compression yalnızca end-to-end faydalıysa çalışsın.

Kod karşılığı: entropy sampling + EWMA ratio.

Doğrulama: cycles/byte, ratio, end-to-end I/O latency.

Risk: Orta.

### 6.11 io_uring SQPOLL Budget

Kullanım alanı: `src/net/io_uring.rs::submit_sqes`

Formül:

```math
Budget = clamp(ceil(lambda_submit * W_target), B_min, B_max)
```

Hedef: submit burstlerini yeterli işleyip CPU spin'i sınırlamak.

Kod karşılığı: moving average submit rate ve completion delay.

Doğrulama: queue depth, p99 wait, CPU cycles/op.

Risk: Orta.

### 6.12 Win32 Import Dispatch Cost

Kullanım alanı: `src/win32.rs`, `src/pe_loader.rs`

Formül:

```math
T_imports = sum_modules T_module_lookup + sum_apis T_api_lookup
```

Hedef: `T_lookup` değerini runtime tree lookup'tan static indexed lookup'a düşürmek.

Kod karşılığı: generated static table/perfect hash.

Doğrulama: PE load time, API lookup ns, allocations.

Risk: Düşük-Orta.

---

## 7. Önceliklendirilmiş Yol Haritası

### P0

- `src/net/tcp.rs`: 4-tuple hash/RCU demux tablosu.
- `src/task/scheduler.rs`: `choose_spawn_cpu` ve `choose_victim_cpu` için NUMA-local aday indeksleri.
- `src/fs/dcache.rs`: `lookup` allocation kaldırma, `shrink/compact` O(n^2) düzeltme, `rename` invariant testleri.
- `src/drivers/nvme.rs`: sync completion için adaptive poll-then-sleep ve completion budget.
- `src/fs/ext4.rs`: extent mapping binary search.
- `src/gui/damage.rs`: adaptive redraw maliyet katsayıları.
- `src/memory/mod.rs`: allocation miss path için ölçüm ve pre-reclaim watermark.

### P1

- `src/net/io_uring.rs`: pending slot table ve registered buffer group indeksleri.
- `src/memory/pmm.rs`: contiguous allocation için free-run/buddy summary.
- `src/cpu/smp.rs`: shootdown batching deneysel, önce litmus test.
- `src/gfx/simd.rs`: size/alignment dispatch thresholds.
- `src/win32.rs`: static generated API table.
- `src/fs/namei.rs`: path cursor ve prefix cache.
- `src/net/http2.rs`: HPACK static lookup ve dynamic table utility.
- `src/net/tls.rs`/`x509.rs`: verification cache ve parser allocation ölçümü.

### P2

- `src/fs/btrfs.rs`: interval chunk map ve free-space buckets.
- `src/fs/f2fs.rs`/`vfs_unified.rs`: FS-specific metadata caches.
- `src/task/cfs.rs`/`eevdf.rs`: queue veri yapısı hibriti ve fairness instrumentation.
- `src/drivers/nic_native.rs`: adaptive interrupt coalescing kontrol modeli.
- `src/drivers/wifi_jail.rs`: Bayesian link score.
- `src/drivers/audio_jail.rs`: adaptive period/prefill.
- `src/pe_loader.rs`: import cache ve relocation streaming.
- `src/security/*`: metadata verification cache.

### P3

- `src/userland/ech_db.rs`: cardinality-based query planner.
- `src/ml/onnx_runtime.rs`: operator fusion ve tensor liveness allocator.
- Compression utility modelinin tüm zswap/storage pipeline'a yayılması.
- Renderer command cache için node-id incremental invalidation.
- Kernel-wide cost model CI trend dashboard.

---

## 8. Benchmark ve Ölçüm Planı

Bu rapor kod değiştirmediği için aşağıdaki komutlar öneridir; burada çalıştırılmış kabul edilmemelidir.

### Genel doğrulama

| Metrik | Ölçüm yaklaşımı |
|---|---|
| Startup time | UEFI/Simics/QEMU boot timestamp; PE/Win32 init ayrı counter. |
| Average latency | TSC ring buffer; modül bazlı begin/end event. |
| p95/p99 latency | Histogram: scheduler switch, allocation, NVMe completion, TCP demux, frame render. |
| CPU usage | perf counters veya kernel internal cycles/op. |
| Memory usage | allocator stats, page cache, retired RCU bytes, ring occupancy. |
| Allocation count | host tests ve kernel allocator counters. |
| Disk I/O | bytes/op, read amplification, flush count, queue depth. |
| Network round trips | TCP retransmit/ACK ratio, HTTP2 frames/request. |
| Cache hit ratio | dcache, page cache, prefix cache, cert cache, glyph atlas. |
| Error rate | timeout, retransmit, OOM, underrun, jail restart. |
| Throughput | IOPS, PPS, RPC/s, frames/s, syscalls/s. |

### Önerilen komutlar ve test türleri

```powershell
cargo test --target x86_64-pc-windows-msvc --lib -q
```

Amaç: host uyumlu unit/regression testleri.

```powershell
cargo check --target x86_64-unknown-uefi --features simics -q
```

Amaç: UEFI/simics build path doğrulaması.

```powershell
cargo bench --features nightly --bench scheduling_bench
cargo bench --features nightly --bench memory_bench
cargo bench --features nightly --bench filesystem_bench
cargo bench --features nightly --bench network_bench
```

Amaç: scheduler, memory, FS ve network değişiklikleri için pre/post regression. `nightly` gereksinimi repo ayarlarına göre doğrulanmalı.

```powershell
.\run_simics.ps1 -CheckOnly -RequireVmp
```

Amaç: Simics ortam uygunluğu.

```powershell
.\.qoder\skills\echos-architect\scripts\echos_gate.ps1 -Paths src\task\scheduler.rs,src\net\tcp.rs,src\fs\dcache.rs
```

Amaç: değişiklik yapılan path'lerde mimari kapı. Bu raporda source path değişmediği için öneri olarak yazıldı.

### Modül bazlı benchmark tasarımları

- Scheduler: `C={64,512,2048,8192}`, runnable/sleeping oranı, NUMA uzaklık matrisi; ölç: spawn ns, steal success, schedule p99.
- Memory: allocation hit/miss, reclaim pressure, contiguous allocation fragmentation; ölç: p99 alloc, `P_hit`, `T_reclaim`.
- NVMe: queue depth sweep, sync/async, batch size; ölç: IOPS, p99, CPU cycles/op, MMIO writes/op.
- Dcache/namei: path depth, cache hit ratio, rename corpus; ölç: allocations/path, lookup ns, invariant failures.
- Ext4/Btrfs: fragmented large file random read/write; ölç: map entries scanned, read amplification, metadata writes.
- TCP: connection count sweep; ölç: demux ns/packet, lock hold, pps.
- io_uring: SQ depth and CQ drain budget; ölç: ops/sec, p99 wait, pending map cost.
- HTTP2/TLS: header corpus and cert chain corpus; ölç: allocations/request, verify ms, cache hit.
- GUI/render: dirty rect patterns; ölç: pixels redrawn, frame p99, missed vsync.
- Jail rings/audio/WiFi: occupancy and restart scenarios; ölç: underrun, recovery latency, messages/sec.

---

## 9. Uygulanmaması Gereken Optimizasyonlar

- Benchmark olmadan `SeqCst` fence'leri daha zayıf ordering'e indirmek. NVMe/NIC/IOMMU paths'te bu doğrudan veri kaybı veya güvenlik açığı riski taşır.
- Scheduler `schedule()` içinde büyük refactor'a ölçümsüz başlamak. Önce bileşen latency histogramı gerekir.
- Dcache rename correctness testleri olmadan LRU/lookup performans değişikliğiyle aynı anda rename davranışını değiştirmek.
- TCP congestion control gain değerlerini sentetik tek workload'a göre sabitlemek. RTT/loss/bandwidth matrisi gerekir.
- Full redraw eşiğini yalnızca görsel sezgiyle değiştirmek. Pixel/tile/batch maliyeti ölçülmeli.
- Parserlarda unsafe zero-copy dönüşümleri fuzz corpus olmadan yapmak.
- TLB shootdown batching'i stale-window litmus testleri olmadan açmak.
- Compression'ı her küçük buffer'a uygulamak; CPU maliyeti I/O kazancını geçebilir.
- Runtime API table gibi cold path'leri optimize ederken destek kapsamı raporlamasını kaybetmek.
- Tek bir veri yapısını tüm boyutlarda kullanmak. Küçük `n` için `Vec`, büyük `n` için tree/hash hibriti çoğu modülde daha iyi olabilir.

---

## 10. Sonuç

echOS için en mantıklı genel strateji, önce matematiksel olarak ölçülebilir seri darboğazları kaldırmaktır: TCP demux lineer taraması, scheduler CPU scan'leri, dcache O(n^2) maintenance, PMM/reclaim miss path ve NVMe wait/doorbell amortizasyonu. Bunlar hem global `C_echOS` modelinde büyük katsayılı değişkenlere bağlı, hem de doğrulaması net benchmarklarla yapılabilir.

İkinci aşamada storage/render/parser katmanlarında allocation/copy azaltılmalı: ext4 extent binary search, Btrfs interval index, HPACK/static lookup, render damage cost model, Win32 static dispatch. Bu sınıf değişiklikler daha düşük riskle latency ve memory footprint kazancı sağlar.

Üçüncü aşamada yüksek riskli ama büyük ölçek kazancı getirecek işler ele alınmalı: TLB shootdown batching, RCU grace-period tuning, IOMMU mapping cache, congestion/adaptive coalescing kontrol sistemleri. Bu alanlarda önce counter, litmus test ve workload matrisi kurulmadan kod değişikliği yapılmamalıdır.

Raporun ana falsification ilkesi şudur: her denklem, ilgili fonksiyonun gerçek counter'larıyla yanlışlanabilir olmalıdır. Önerilen optimizasyon bir benchmarkta beklenen metriği iyileştirmiyor veya başka bir terimi daha fazla bozuyorsa model güncellenmeli, optimizasyon geri alınmalı ya da yalnızca workload-specific hale getirilmelidir.

