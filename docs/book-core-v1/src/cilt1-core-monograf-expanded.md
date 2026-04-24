# Cilt 1 Genisletilmis Monograf

Bu monograf, Cilt 1'in derin katmanidir. Her baslikta kod, algoritma, worst-case ve olcum disiplini birlikte verilir.

## M01 - Boot, platform init ve erken dogruluk

### Kod baglami

- Ana dosya: `src/main.rs`
- Sembol: `init_platform_iommu` -> `src/main.rs:188`
- Sembol: `parse_swap_cmdline` -> `src/main.rs:221`
- Sembol: `serial_init` -> `src/main.rs:144`
- Sembol: `panic_handler` -> `src/main.rs:259`

### Cekirdek fikir

Bu alt sistemde ana karar, `src/main.rs` icindeki ownership ve state publication sinirlarini net tutmaktir.
Kodu okurken once veri yapisini, sonra state gecislerini, en son hata donuslerini takip etmek gerekir.
Aksi halde kod calissa bile neden oyle tasarlandigini anlamak zorlasir ve yanlis optimizasyon riski artar.

### Matematik modeli

- Model: `L_boot = L_firmware + L_loader + L_kernel_early + L_subsystem_init`
- Not: Bu model karar destek icindir; tek basina dogruluk ispatı olarak kullanilmaz.

### Worst-case-first

- En kotu durum: Erken init sozlesmesi bozulursa semptom gec ve daginik gorulur.
- Semptom: p99/p999 gecikme artisi, tutarsiz state, veya sessiz veri bozulmasi.
- Erken tespit: invariant kontrolu, guard sayaclari, stress altinda deterministik test tekrarlandirilmasi.

### Algoritma otopsisi

1. Neden secildi: Bu algoritma hedeflenen workload sinifi icin sabit ve anlasilir karar yuzeyi sunar.
2. Ana dezavantaj: Yanlis tuning veya eksik guard ile patolojik yukte performans/correctness kaybi uretebilir.
3. echOS mitigasyonu: Fail-closed init, capability bazli acilis, adim bazli loglama.

### Invariant denetim cercevesi

- Denetim kurali: Dosyadaki tum atomik veya state degiskenlerini listele.
- Denetim kurali: Hangi fonksiyonun ownership devrettigini isaretle.
- Denetim kurali: Hata donen tum yollari tabloya dok.
- Denetim kurali: Bir invarianti sec ve nasil bozulabilecegini yaz.
- Denetim kurali: O invarianti koruyan satiri dosyada bulup not al.

### Olcum ve benchmark paketi

- Metrik 1: p50/p95/p99 latency
- Metrik 2: throughput veya servis hizi
- Metrik 3: hata/geri alma sayaci
- Metrik 4: queue depth veya pressure sinyali
- Metrik 5: regressions arasi fark tablosu

### Failure envelope analizi

- Failure paterni A: Ortalama metrik iyi, p99 kotu. Bu durumda kuyruk ve publication siniri odakli inceleme gerekir.
- Failure paterni B: Throughput yuksek ama hata orani artiyor. Bu durumda guard limiti ve admission politikasi yeniden ele alinmali.
- Failure paterni C: Testte temiz, sahada bozuk. Bu genelde eksik stress profili veya environment farki kaynaklidir.

### Cekirdek cikarsama seti

- Bu alt sistemin karar agacini ezbersiz aciklayabilmek
- Bir bug raporunu dogru katmana indirebilmek
- Performans iddiasini metrikle savunabilmek
- Mitigasyon secimini nedenleriyle yazabilmek

### Dosya basi notlarindan alinti

> # EchOS Çekirdek Giriş Noktası
> Bu dosya çekirdeğin ana giriş noktasını içerir. UEFI ve Limine (bare-metal)
> olmak üzere iki önyükleme ortamını destekler:
> - **UEFI modu** (`target_os = "uefi"`): UEFI firmware tarafından çağrılır,
> framebuffer başlatılır, splash ekranı gösterilir ve GUI sistemi devreye alınır.
> - **Limine modu** (varsayılan): Limine önyükleyici protokolü üzerinden bellek
> haritası alınır, sayfa tabloları kurulur ve çekirdek tam olarak başlatılır.
> `#![no_std]` ve `#![no_main]` nitelikleri, standart kütüphane ve C çalışma
> zamanı bağımlılığı olmaksızın doğrudan donanım üzerinde çalışmayı sağlar.

---

## M02 - Bootstrap frame allocator ve fiziksel aralik korumasi

### Kod baglami

- Ana dosya: `src/memory/frame_allocator.rs`
- Sembol: `allocate_frame_internal` -> `src/memory/frame_allocator.rs:153`
- Sembol: `allocate_contiguous` -> `src/memory/frame_allocator.rs:117`
- Sembol: `overlaps_kernel` -> `src/memory/frame_allocator.rs:143`
- Sembol: `kernel_phys_range` -> `src/memory/frame_allocator.rs:93`

### Cekirdek fikir

Bu alt sistemde ana karar, `src/memory/frame_allocator.rs` icindeki ownership ve state publication sinirlarini net tutmaktir.
Kodu okurken once veri yapisini, sonra state gecislerini, en son hata donuslerini takip etmek gerekir.
Aksi halde kod calissa bile neden oyle tasarlandigini anlamak zorlasir ve yanlis optimizasyon riski artar.

### Matematik modeli

- Model: `F_free = F_total - F_used - F_reserved`
- Not: Bu model karar destek icindir; tek basina dogruluk ispatı olarak kullanilmaz.

### Worst-case-first

- En kotu durum: Kernel image araligi korunmazsa self-corruption olusur.
- Semptom: p99/p999 gecikme artisi, tutarsiz state, veya sessiz veri bozulmasi.
- Erken tespit: invariant kontrolu, guard sayaclari, stress altinda deterministik test tekrarlandirilmasi.

### Algoritma otopsisi

1. Neden secildi: Bu algoritma hedeflenen workload sinifi icin sabit ve anlasilir karar yuzeyi sunar.
2. Ana dezavantaj: Yanlis tuning veya eksik guard ile patolojik yukte performans/correctness kaybi uretebilir.
3. echOS mitigasyonu: Kernel fiziksel araligi explicit hesaplanip tahsis disi tutulur.

### Invariant denetim cercevesi

- Denetim kurali: Dosyadaki tum atomik veya state degiskenlerini listele.
- Denetim kurali: Hangi fonksiyonun ownership devrettigini isaretle.
- Denetim kurali: Hata donen tum yollari tabloya dok.
- Denetim kurali: Bir invarianti sec ve nasil bozulabilecegini yaz.
- Denetim kurali: O invarianti koruyan satiri dosyada bulup not al.

### Olcum ve benchmark paketi

- Metrik 1: p50/p95/p99 latency
- Metrik 2: throughput veya servis hizi
- Metrik 3: hata/geri alma sayaci
- Metrik 4: queue depth veya pressure sinyali
- Metrik 5: regressions arasi fark tablosu

### Failure envelope analizi

- Failure paterni A: Ortalama metrik iyi, p99 kotu. Bu durumda kuyruk ve publication siniri odakli inceleme gerekir.
- Failure paterni B: Throughput yuksek ama hata orani artiyor. Bu durumda guard limiti ve admission politikasi yeniden ele alinmali.
- Failure paterni C: Testte temiz, sahada bozuk. Bu genelde eksik stress profili veya environment farki kaynaklidir.

### Cekirdek cikarsama seti

- Bu alt sistemin karar agacini ezbersiz aciklayabilmek
- Bir bug raporunu dogru katmana indirebilmek
- Performans iddiasini metrikle savunabilmek
- Mitigasyon secimini nedenleriyle yazabilmek

### Dosya basi notlarindan alinti

> # Multiboot2 / Limine Fiziksel Frame Ayırıcısı
> Kernel önyükleme aşamasına özel, ileri-doğru büyüyen (bump) ayırıcı.
> ## Neden Bump Ayırıcı?
> Kernel başlarken henüz yığın (heap) veya PMM hazır değildir.
> Bu aşamada yalnızca sıralı frame tahsis yeterlidir:
> ```
> Multiboot2 Bellek Haritası:
> [Bölge 1: Available] → [Bölge 2: Reserved] → [Bölge 3: Available] → ...
> next_frame işaretçisi ileri doğru ilerler:
> │ frame[0] │ frame[1] │ frame[2] │ frame[3] │ ... │
> │ kernel   │ kernel   │ ← next_frame         │
> Tahsis: next_frame alınır, next_frame += FRAME_SIZE
> Serbest bırakma: DESTEKLENMIYOR (bootstrap fazı için gereksiz)
> ```
> ## Kernel Fiziksel Aralık Koruması

---

## M03 - SMP scheduler karar modeli

### Kod baglami

- Ana dosya: `src/task/scheduler.rs`
- Sembol: `choose_spawn_cpu` -> `src/task/scheduler.rs:97`
- Sembol: `enqueue_boxed_task` -> `src/task/scheduler.rs:98`
- Sembol: `publish_worker_load` -> `src/task/scheduler.rs:99`
- Sembol: `update_cpu_count` -> `src/task/scheduler.rs:234`

### Cekirdek fikir

Bu alt sistemde ana karar, `src/task/scheduler.rs` icindeki ownership ve state publication sinirlarini net tutmaktir.
Kodu okurken once veri yapisini, sonra state gecislerini, en son hata donuslerini takip etmek gerekir.
Aksi halde kod calissa bile neden oyle tasarlandigini anlamak zorlasir ve yanlis optimizasyon riski artar.

### Matematik modeli

- Model: `Skew = max(queue_len_i) - min(queue_len_i)`
- Not: Bu model karar destek icindir; tek basina dogruluk ispatı olarak kullanilmaz.

### Worst-case-first

- En kotu durum: Load skew artarsa tail latency patlar.
- Semptom: p99/p999 gecikme artisi, tutarsiz state, veya sessiz veri bozulmasi.
- Erken tespit: invariant kontrolu, guard sayaclari, stress altinda deterministik test tekrarlandirilmasi.

### Algoritma otopsisi

1. Neden secildi: Bu algoritma hedeflenen workload sinifi icin sabit ve anlasilir karar yuzeyi sunar.
2. Ana dezavantaj: Yanlis tuning veya eksik guard ile patolojik yukte performans/correctness kaybi uretebilir.
3. echOS mitigasyonu: Work stealing + queue telemetrisi + affinity filtreleri.

### Invariant denetim cercevesi

- Denetim kurali: Dosyadaki tum atomik veya state degiskenlerini listele.
- Denetim kurali: Hangi fonksiyonun ownership devrettigini isaretle.
- Denetim kurali: Hata donen tum yollari tabloya dok.
- Denetim kurali: Bir invarianti sec ve nasil bozulabilecegini yaz.
- Denetim kurali: O invarianti koruyan satiri dosyada bulup not al.

### Olcum ve benchmark paketi

- Metrik 1: p50/p95/p99 latency
- Metrik 2: throughput veya servis hizi
- Metrik 3: hata/geri alma sayaci
- Metrik 4: queue depth veya pressure sinyali
- Metrik 5: regressions arasi fark tablosu

### Failure envelope analizi

- Failure paterni A: Ortalama metrik iyi, p99 kotu. Bu durumda kuyruk ve publication siniri odakli inceleme gerekir.
- Failure paterni B: Throughput yuksek ama hata orani artiyor. Bu durumda guard limiti ve admission politikasi yeniden ele alinmali.
- Failure paterni C: Testte temiz, sahada bozuk. Bu genelde eksik stress profili veya environment farki kaynaklidir.

### Cekirdek cikarsama seti

- Bu alt sistemin karar agacini ezbersiz aciklayabilmek
- Bir bug raporunu dogru katmana indirebilmek
- Performans iddiasini metrikle savunabilmek
- Mitigasyon secimini nedenleriyle yazabilmek

### Dosya basi notlarindan alinti

> # echOS Görev Zamanlayıcı (Task Scheduler)
> Bu modül, işletim sisteminin preemptive multitasking desteğini sağlar.
> Öncelik tabanlı yaşlandırma (Priority-Based Aging) ve
> Chase-Lev iş çalma (Work Stealing) algoritmasıyla görevleri adil biçimde zamanlar.
> ## Zamanlayıcı Seçim Mantığı
> ```text
> ┌──────────────────────────────────────────────────────────┐
> │          ZAMANLAYICI SEÇİM KARAR AKIŞI                  │
> │                                                          │
> │  Timer Interrupt geldi (tick)                           │
> │       ↓                                                  │
> │  1. RT görevi var mı? (rt_scheduler)                   │
> │     Evet → en yüksek öncelikli RT görevi çalışır       │
> │     (SCHED_FIFO: bloke edilene kadar, SCHED_RR: dilime) │
> │       ↓ Hayır                                           │
> │  2. Yerel Worker kuyruğunda görev var mı?              │
> │     Evet → pop() ile al (LIFO — önbellek dostu)        │
> │       ↓ Hayır                                           │
> │  3. En yüklü CPU'dan iş çal (Work Stealing)            │
> │     steal() → başka CPU'nun kuyruğunun başından al     │
> │       ↓ Yoksa                                           │

---

## M04 - RT scheduler: FIFO/RR ve runtime limiti

### Kod baglami

- Ana dosya: `src/task/rt_scheduler.rs`
- Sembol: `calculate_timeslice` -> `src/task/rt_scheduler.rs:162`
- Sembol: `enqueue` -> `src/task/rt_scheduler.rs:244`
- Sembol: `tick` -> `src/task/rt_scheduler.rs:64`
- Sembol: `set_sched_param` -> `src/task/rt_scheduler.rs:350`

### Cekirdek fikir

Bu alt sistemde ana karar, `src/task/rt_scheduler.rs` icindeki ownership ve state publication sinirlarini net tutmaktir.
Kodu okurken once veri yapisini, sonra state gecislerini, en son hata donuslerini takip etmek gerekir.
Aksi halde kod calissa bile neden oyle tasarlandigini anlamak zorlasir ve yanlis optimizasyon riski artar.

### Matematik modeli

- Model: `slice = s_min + alpha(prio) * (s_max - s_min)`
- Not: Bu model karar destek icindir; tek basina dogruluk ispatı olarak kullanilmaz.

### Worst-case-first

- En kotu durum: Yanlis policy secimi starvation ve jitter uretir.
- Semptom: p99/p999 gecikme artisi, tutarsiz state, veya sessiz veri bozulmasi.
- Erken tespit: invariant kontrolu, guard sayaclari, stress altinda deterministik test tekrarlandirilmasi.

### Algoritma otopsisi

1. Neden secildi: Bu algoritma hedeflenen workload sinifi icin sabit ve anlasilir karar yuzeyi sunar.
2. Ana dezavantaj: Yanlis tuning veya eksik guard ile patolojik yukte performans/correctness kaybi uretebilir.
3. echOS mitigasyonu: RR dilimi ve RT bandwidth governance.

### Invariant denetim cercevesi

- Denetim kurali: Dosyadaki tum atomik veya state degiskenlerini listele.
- Denetim kurali: Hangi fonksiyonun ownership devrettigini isaretle.
- Denetim kurali: Hata donen tum yollari tabloya dok.
- Denetim kurali: Bir invarianti sec ve nasil bozulabilecegini yaz.
- Denetim kurali: O invarianti koruyan satiri dosyada bulup not al.

### Olcum ve benchmark paketi

- Metrik 1: p50/p95/p99 latency
- Metrik 2: throughput veya servis hizi
- Metrik 3: hata/geri alma sayaci
- Metrik 4: queue depth veya pressure sinyali
- Metrik 5: regressions arasi fark tablosu

### Failure envelope analizi

- Failure paterni A: Ortalama metrik iyi, p99 kotu. Bu durumda kuyruk ve publication siniri odakli inceleme gerekir.
- Failure paterni B: Throughput yuksek ama hata orani artiyor. Bu durumda guard limiti ve admission politikasi yeniden ele alinmali.
- Failure paterni C: Testte temiz, sahada bozuk. Bu genelde eksik stress profili veya environment farki kaynaklidir.

### Cekirdek cikarsama seti

- Bu alt sistemin karar agacini ezbersiz aciklayabilmek
- Bir bug raporunu dogru katmana indirebilmek
- Performans iddiasini metrikle savunabilmek
- Mitigasyon secimini nedenleriyle yazabilmek

### Dosya basi notlarindan alinti

> # Gerçek Zamanlı Zamanlayıcı (Real-Time Scheduler)
> POSIX gerçek zamanlı zamanlama politikalarının uygulaması.
> SCHED_FIFO ve SCHED_RR (Round-Robin) desteklenir.
> ## SCHED_FIFO — İlk Gelen İlk Çalışır
> ```text
> Öncelik 99 [T1] → CPU almadan bırakmaz!
> T1 tamamlanır veya engellenirse sonraki seçilir.
> Öncelik 50 [T2, T3]  ← T2 önce geldi, T2 çalışır
> T2 biter → T3 çalışır
> Öncelik 1  [T4]
> KURAL: Düşük öncelik asla yüksek öncelikli varken çalışmaz!
> ```
> ## SCHED_RR — Round-Robin Gerçek Zamanlı
> ```text
> Öncelik 99 [T1, T2, T3]

---

## M05 - CFS: vruntime adalet motoru

### Kod baglami

- Ana dosya: `src/task/cfs.rs`
- Sembol: `weight_to_vruntime` -> `src/task/cfs.rs:82`
- Sembol: `enqueue` -> `src/task/cfs.rs:112`
- Sembol: `pick_next` -> `src/task/cfs.rs:20`
- Sembol: `check_preempt_wakeup` -> `src/task/cfs.rs:315`

### Cekirdek fikir

Bu alt sistemde ana karar, `src/task/cfs.rs` icindeki ownership ve state publication sinirlarini net tutmaktir.
Kodu okurken once veri yapisini, sonra state gecislerini, en son hata donuslerini takip etmek gerekir.
Aksi halde kod calissa bile neden oyle tasarlandigini anlamak zorlasir ve yanlis optimizasyon riski artar.

### Matematik modeli

- Model: `Delta_v = (Delta_t * NICE0) / weight`
- Not: Bu model karar destek icindir; tek basina dogruluk ispatı olarak kullanilmaz.

### Worst-case-first

- En kotu durum: Wakup-heavy yukte asiri preemption ve fairness gerilimi.
- Semptom: p99/p999 gecikme artisi, tutarsiz state, veya sessiz veri bozulmasi.
- Erken tespit: invariant kontrolu, guard sayaclari, stress altinda deterministik test tekrarlandirilmasi.

### Algoritma otopsisi

1. Neden secildi: Bu algoritma hedeflenen workload sinifi icin sabit ve anlasilir karar yuzeyi sunar.
2. Ana dezavantaj: Yanlis tuning veya eksik guard ile patolojik yukte performans/correctness kaybi uretebilir.
3. echOS mitigasyonu: Wakeup granularity ve min_vruntime clamp.

### Invariant denetim cercevesi

- Denetim kurali: Dosyadaki tum atomik veya state degiskenlerini listele.
- Denetim kurali: Hangi fonksiyonun ownership devrettigini isaretle.
- Denetim kurali: Hata donen tum yollari tabloya dok.
- Denetim kurali: Bir invarianti sec ve nasil bozulabilecegini yaz.
- Denetim kurali: O invarianti koruyan satiri dosyada bulup not al.

### Olcum ve benchmark paketi

- Metrik 1: p50/p95/p99 latency
- Metrik 2: throughput veya servis hizi
- Metrik 3: hata/geri alma sayaci
- Metrik 4: queue depth veya pressure sinyali
- Metrik 5: regressions arasi fark tablosu

### Failure envelope analizi

- Failure paterni A: Ortalama metrik iyi, p99 kotu. Bu durumda kuyruk ve publication siniri odakli inceleme gerekir.
- Failure paterni B: Throughput yuksek ama hata orani artiyor. Bu durumda guard limiti ve admission politikasi yeniden ele alinmali.
- Failure paterni C: Testte temiz, sahada bozuk. Bu genelde eksik stress profili veya environment farki kaynaklidir.

### Cekirdek cikarsama seti

- Bu alt sistemin karar agacini ezbersiz aciklayabilmek
- Bir bug raporunu dogru katmana indirebilmek
- Performans iddiasini metrikle savunabilmek
- Mitigasyon secimini nedenleriyle yazabilmek

### Dosya basi notlarindan alinti

> # CFS (Completely Fair Scheduler - Tamamen Adil Zamanlayıcı)
> Linux çekirdeğinden ilham alınan, sanal çalışma zamanı (vruntime) tabanlı
> adil zamanlama algoritması.
> ## Temel Fikir
> Her task bir "sanal saat"e (vruntime) sahiptir. Zamanlayıcı her zaman
> en düşük vruntime değerine sahip task'ı çalıştırır. Bu sayede tüm
> task'lar CPU zamanından "eşit" pay alır.
> ## CFS Red-Black Tree (Kırmızı-Siyah Ağaç) Yapısı
> ```text
> [vruntime=50]  <-- En yüksek öncelik (kök)
> /              \
> [vruntime=30]        [vruntime=80]
> /          \
> [vruntime=10]  [vruntime=40]
> ^
> pick_next() bu task'ı seçer (en sol yaprak = en küçük vruntime)
> ```
> ## vruntime Hesaplama
> ```

---

## M06 - EEVDF: eligible_vtime ve virtual deadline

### Kod baglami

- Ana dosya: `src/task/eevdf.rs`
- Sembol: `update_runtime` -> `src/task/eevdf.rs:44`
- Sembol: `pick_next` -> `src/task/eevdf.rs:135`
- Sembol: `should_preempt` -> `src/task/eevdf.rs:145`
- Sembol: `stats` -> `src/task/eevdf.rs:162`

### Cekirdek fikir

Bu alt sistemde ana karar, `src/task/eevdf.rs` icindeki ownership ve state publication sinirlarini net tutmaktir.
Kodu okurken once veri yapisini, sonra state gecislerini, en son hata donuslerini takip etmek gerekir.
Aksi halde kod calissa bile neden oyle tasarlandigini anlamak zorlasir ve yanlis optimizasyon riski artar.

### Matematik modeli

- Model: `lag = rq_vtime - vruntime`
- Not: Bu model karar destek icindir; tek basina dogruluk ispatı olarak kullanilmaz.

### Worst-case-first

- En kotu durum: Yanlis slice ve lag dengesi wakeup davranisini bozar.
- Semptom: p99/p999 gecikme artisi, tutarsiz state, veya sessiz veri bozulmasi.
- Erken tespit: invariant kontrolu, guard sayaclari, stress altinda deterministik test tekrarlandirilmasi.

### Algoritma otopsisi

1. Neden secildi: Bu algoritma hedeflenen workload sinifi icin sabit ve anlasilir karar yuzeyi sunar.
2. Ana dezavantaj: Yanlis tuning veya eksik guard ile patolojik yukte performans/correctness kaybi uretebilir.
3. echOS mitigasyonu: Lag tabanli eligibility + deadline siralama.

### Invariant denetim cercevesi

- Denetim kurali: Dosyadaki tum atomik veya state degiskenlerini listele.
- Denetim kurali: Hangi fonksiyonun ownership devrettigini isaretle.
- Denetim kurali: Hata donen tum yollari tabloya dok.
- Denetim kurali: Bir invarianti sec ve nasil bozulabilecegini yaz.
- Denetim kurali: O invarianti koruyan satiri dosyada bulup not al.

### Olcum ve benchmark paketi

- Metrik 1: p50/p95/p99 latency
- Metrik 2: throughput veya servis hizi
- Metrik 3: hata/geri alma sayaci
- Metrik 4: queue depth veya pressure sinyali
- Metrik 5: regressions arasi fark tablosu

### Failure envelope analizi

- Failure paterni A: Ortalama metrik iyi, p99 kotu. Bu durumda kuyruk ve publication siniri odakli inceleme gerekir.
- Failure paterni B: Throughput yuksek ama hata orani artiyor. Bu durumda guard limiti ve admission politikasi yeniden ele alinmali.
- Failure paterni C: Testte temiz, sahada bozuk. Bu genelde eksik stress profili veya environment farki kaynaklidir.

### Cekirdek cikarsama seti

- Bu alt sistemin karar agacini ezbersiz aciklayabilmek
- Bir bug raporunu dogru katmana indirebilmek
- Performans iddiasini metrikle savunabilmek
- Mitigasyon secimini nedenleriyle yazabilmek

### Dosya basi notlarindan alinti

> EEVDF scheduler çekirdeği (Earliest Eligible Virtual Deadline First).
> Bu modül Faz-I scheduler backlog'u için:
> - lag tracking
> - runqueue başına sanal zaman (vtime)
> - deadline tabanlı preemption
> sağlar.

---

## M07 - Deadline scheduler: EDF/CBS admission ve replenish

### Kod baglami

- Ana dosya: `src/task/deadline.rs`
- Sembol: `compute_bandwidth` -> `src/task/deadline.rs:238`
- Sembol: `check_replenishments` -> `src/task/deadline.rs:293`
- Sembol: `consume_runtime` -> `src/task/deadline.rs:149`
- Sembol: `enqueue` -> `src/task/deadline.rs:236`

### Cekirdek fikir

Bu alt sistemde ana karar, `src/task/deadline.rs` icindeki ownership ve state publication sinirlarini net tutmaktir.
Kodu okurken once veri yapisini, sonra state gecislerini, en son hata donuslerini takip etmek gerekir.
Aksi halde kod calissa bile neden oyle tasarlandigini anlamak zorlasir ve yanlis optimizasyon riski artar.

### Matematik modeli

- Model: `U = C/T, sum(U_i) <= 1`
- Not: Bu model karar destek icindir; tek basina dogruluk ispatı olarak kullanilmaz.

### Worst-case-first

- En kotu durum: Admission ihlali deadline miss patlamasi uretir.
- Semptom: p99/p999 gecikme artisi, tutarsiz state, veya sessiz veri bozulmasi.
- Erken tespit: invariant kontrolu, guard sayaclari, stress altinda deterministik test tekrarlandirilmasi.

### Algoritma otopsisi

1. Neden secildi: Bu algoritma hedeflenen workload sinifi icin sabit ve anlasilir karar yuzeyi sunar.
2. Ana dezavantaj: Yanlis tuning veya eksik guard ile patolojik yukte performans/correctness kaybi uretebilir.
3. echOS mitigasyonu: Bandwidth limiti + periodik replenish + throttle.

### Invariant denetim cercevesi

- Denetim kurali: Dosyadaki tum atomik veya state degiskenlerini listele.
- Denetim kurali: Hangi fonksiyonun ownership devrettigini isaretle.
- Denetim kurali: Hata donen tum yollari tabloya dok.
- Denetim kurali: Bir invarianti sec ve nasil bozulabilecegini yaz.
- Denetim kurali: O invarianti koruyan satiri dosyada bulup not al.

### Olcum ve benchmark paketi

- Metrik 1: p50/p95/p99 latency
- Metrik 2: throughput veya servis hizi
- Metrik 3: hata/geri alma sayaci
- Metrik 4: queue depth veya pressure sinyali
- Metrik 5: regressions arasi fark tablosu

### Failure envelope analizi

- Failure paterni A: Ortalama metrik iyi, p99 kotu. Bu durumda kuyruk ve publication siniri odakli inceleme gerekir.
- Failure paterni B: Throughput yuksek ama hata orani artiyor. Bu durumda guard limiti ve admission politikasi yeniden ele alinmali.
- Failure paterni C: Testte temiz, sahada bozuk. Bu genelde eksik stress profili veya environment farki kaynaklidir.

### Cekirdek cikarsama seti

- Bu alt sistemin karar agacini ezbersiz aciklayabilmek
- Bir bug raporunu dogru katmana indirebilmek
- Performans iddiasini metrikle savunabilmek
- Mitigasyon secimini nedenleriyle yazabilmek

### Dosya basi notlarindan alinti

> # Deadline Zamanlayıcısı (EDF - Earliest Deadline First)
> En Erken Son Tarih Önce (EDF) gerçek zamanlı zamanlama politikası.
> POSIX SCHED_DEADLINE politikasının uygulaması.
> ## EDF (Earliest Deadline First) Nedir?
> EDF, gerçek zamanlı sistemlerde matematiksel olarak optimal olan
> tek-işlemcili zamanlama algoritmasıdır. Her an, son tarihi en yakın
> olan task çalıştırılır.
> ## Zaman Ekseni Diyagramı
> ```text
> Task A: period=8,  deadline=8,  runtime=2
> Task B: period=5,  deadline=5,  runtime=2
> Task C: period=10, deadline=10, runtime=3
> Zaman:  0  1  2  3  4  5  6  7  8  9  10
> |--|--|--|--|--|--|--|--|--|--|--|
> Task A:  AA          AA
> Task B:    BB  BB       BB
> Task C:          CCC         CCC

---

## M08 - Chase-Lev deque: lock-free race analizi

### Kod baglami

- Ana dosya: `src/task/deque.rs`
- Sembol: `push` -> `src/task/deque.rs:26`
- Sembol: `pop` -> `src/task/deque.rs:14`
- Sembol: `steal` -> `src/task/deque.rs:17`
- Sembol: `compare_exchange` -> `src/task/deque.rs:136`

### Cekirdek fikir

Bu alt sistemde ana karar, `src/task/deque.rs` icindeki ownership ve state publication sinirlarini net tutmaktir.
Kodu okurken once veri yapisini, sonra state gecislerini, en son hata donuslerini takip etmek gerekir.
Aksi halde kod calissa bile neden oyle tasarlandigini anlamak zorlasir ve yanlis optimizasyon riski artar.

### Matematik modeli

- Model: `contention ~= P(last_element_race)`
- Not: Bu model karar destek icindir; tek basina dogruluk ispatı olarak kullanilmaz.

### Worst-case-first

- En kotu durum: Ordering bug'i sessiz veri bozulmasi yaratir.
- Semptom: p99/p999 gecikme artisi, tutarsiz state, veya sessiz veri bozulmasi.
- Erken tespit: invariant kontrolu, guard sayaclari, stress altinda deterministik test tekrarlandirilmasi.

### Algoritma otopsisi

1. Neden secildi: Bu algoritma hedeflenen workload sinifi icin sabit ve anlasilir karar yuzeyi sunar.
2. Ana dezavantaj: Yanlis tuning veya eksik guard ile patolojik yukte performans/correctness kaybi uretebilir.
3. echOS mitigasyonu: Acquire/Release/SeqCst sinirlarinin explicit kullanimi.

### Invariant denetim cercevesi

- Denetim kurali: Dosyadaki tum atomik veya state degiskenlerini listele.
- Denetim kurali: Hangi fonksiyonun ownership devrettigini isaretle.
- Denetim kurali: Hata donen tum yollari tabloya dok.
- Denetim kurali: Bir invarianti sec ve nasil bozulabilecegini yaz.
- Denetim kurali: O invarianti koruyan satiri dosyada bulup not al.

### Olcum ve benchmark paketi

- Metrik 1: p50/p95/p99 latency
- Metrik 2: throughput veya servis hizi
- Metrik 3: hata/geri alma sayaci
- Metrik 4: queue depth veya pressure sinyali
- Metrik 5: regressions arasi fark tablosu

### Failure envelope analizi

- Failure paterni A: Ortalama metrik iyi, p99 kotu. Bu durumda kuyruk ve publication siniri odakli inceleme gerekir.
- Failure paterni B: Throughput yuksek ama hata orani artiyor. Bu durumda guard limiti ve admission politikasi yeniden ele alinmali.
- Failure paterni C: Testte temiz, sahada bozuk. Bu genelde eksik stress profili veya environment farki kaynaklidir.

### Cekirdek cikarsama seti

- Bu alt sistemin karar agacini ezbersiz aciklayabilmek
- Bir bug raporunu dogru katmana indirebilmek
- Performans iddiasini metrikle savunabilmek
- Mitigasyon secimini nedenleriyle yazabilmek

### Dosya basi notlarindan alinti

> # Chase-Lev Lock-Free Work-Stealing Deque (Çift Uçlu Kuyruk)
> Bu modül, SMP (Çok İşlemcili) sistemlerde iş yükü dengemesi için
> kullanılan kilit-serbest (lock-free) Chase-Lev Deque algoritmasını uygular.
> ## Temel Fikir: Work Stealing (İş Çalma)
> ```text
> CPU 0 (meşgul)               CPU 1 (boşta)
> ┌──────────────┐              ┌──────────────┐
> │ Worker       │              │ Worker       │
> │ bottom=5     │              │ bottom=0     │
> │ [T1,T2,T3,T4,T5]           │ []           │
> │       ↑ pop()              │       ↑      │
> │       (LIFO, yerel erişim) │              │
> │                            │ Stealer      │
> │  ←───────────steal()──────┤ (T1'i çalar) │
> └──────────────┘              └──────────────┘
> CPU 0 sondan alır (pop),
> Stealer baştan alır (steal) — çakışma azaltılır!
> ```
> ## Bellek Sıralaması (Memory Ordering)

---

## M09 - Hiyerarsik timing wheel

### Kod baglami

- Ana dosya: `src/task/timer.rs`
- Sembol: `schedule` -> `src/task/timer.rs:25`
- Sembol: `tick` -> `src/task/timer.rs:17`
- Sembol: `cascade` -> `src/task/timer.rs:137`
- Sembol: `WHEEL_SIZE` -> `src/task/timer.rs:43`

### Cekirdek fikir

Bu alt sistemde ana karar, `src/task/timer.rs` icindeki ownership ve state publication sinirlarini net tutmaktir.
Kodu okurken once veri yapisini, sonra state gecislerini, en son hata donuslerini takip etmek gerekir.
Aksi halde kod calissa bile neden oyle tasarlandigini anlamak zorlasir ve yanlis optimizasyon riski artar.

### Matematik modeli

- Model: `T_manage ~= O(1) amortized`
- Not: Bu model karar destek icindir; tek basina dogruluk ispatı olarak kullanilmaz.

### Worst-case-first

- En kotu durum: Cascade atlanirsa wakeup gecikmeleri birikir.
- Semptom: p99/p999 gecikme artisi, tutarsiz state, veya sessiz veri bozulmasi.
- Erken tespit: invariant kontrolu, guard sayaclari, stress altinda deterministik test tekrarlandirilmasi.

### Algoritma otopsisi

1. Neden secildi: Bu algoritma hedeflenen workload sinifi icin sabit ve anlasilir karar yuzeyi sunar.
2. Ana dezavantaj: Yanlis tuning veya eksik guard ile patolojik yukte performans/correctness kaybi uretebilir.
3. echOS mitigasyonu: Level wrap noktalarinda zorunlu cascade yolu.

### Invariant denetim cercevesi

- Denetim kurali: Dosyadaki tum atomik veya state degiskenlerini listele.
- Denetim kurali: Hangi fonksiyonun ownership devrettigini isaretle.
- Denetim kurali: Hata donen tum yollari tabloya dok.
- Denetim kurali: Bir invarianti sec ve nasil bozulabilecegini yaz.
- Denetim kurali: O invarianti koruyan satiri dosyada bulup not al.

### Olcum ve benchmark paketi

- Metrik 1: p50/p95/p99 latency
- Metrik 2: throughput veya servis hizi
- Metrik 3: hata/geri alma sayaci
- Metrik 4: queue depth veya pressure sinyali
- Metrik 5: regressions arasi fark tablosu

### Failure envelope analizi

- Failure paterni A: Ortalama metrik iyi, p99 kotu. Bu durumda kuyruk ve publication siniri odakli inceleme gerekir.
- Failure paterni B: Throughput yuksek ama hata orani artiyor. Bu durumda guard limiti ve admission politikasi yeniden ele alinmali.
- Failure paterni C: Testte temiz, sahada bozuk. Bu genelde eksik stress profili veya environment farki kaynaklidir.

### Cekirdek cikarsama seti

- Bu alt sistemin karar agacini ezbersiz aciklayabilmek
- Bir bug raporunu dogru katmana indirebilmek
- Performans iddiasini metrikle savunabilmek
- Mitigasyon secimini nedenleriyle yazabilmek

### Dosya basi notlarindan alinti

> # echOS Zaman Çarkı (Timing Wheel)
> Bu modül, yüksek performanslı timer yönetimi için "Hierarchical Timing Wheel"
> (Hiyerarşik Zaman Çarkı) algoritmasını uygular.
> O(N) karmaşıklığına sahip basit bir liste taraması yerine, O(1) sabit zamanlı
> ekleme ve silme işlemleri sunar. Milyonlarca task uyusa bile sistem performansı düşmez.
> Kaynak: "Hashed and Hierarchical Timing Wheels", Varghese & Lauck (1987)
> ## Hiyerarşik Zaman Çarkı Algoritması
> ```text
> ┌──────────────────────────────────────────────────────────────┐
> │             HİYERARŞİK ZAMAN ÇARKI (4 SEVİYE)              │
> │                                                              │
> │  Seviye 1 (Hızlı Çark):  0 - 255 tick      → 256 slot       │
> │  Seviye 2:                256 - 65535 tick  → 256 slot       │
> │  Seviye 3:                65536 - 16M tick  → 256 slot       │
> │  Seviye 4:                16M - 4G tick     → 256 slot       │
> │                                                              │
> │  Her tick'te:                                                │
> │    1. Seviye 1'deki şu anki slot işlenir → O(1)             │
> │    2. Çark başa döndüğünde Cascade (Şelale) tetiklenir:     │

---

## M10 - Zone-aware PMM fallback mimarisi

### Kod baglami

- Ana dosya: `src/memory/fibonacci_pmm.rs`
- Sembol: `fallback` -> `src/memory/fibonacci_pmm.rs:51`
- Sembol: `allocate_from_zone` -> `src/memory/fibonacci_pmm.rs:223`
- Sembol: `allocate_contiguous_from_zone` -> `src/memory/fibonacci_pmm.rs:240`
- Sembol: `zone_stats` -> `src/memory/fibonacci_pmm.rs:353`

### Cekirdek fikir

Bu alt sistemde ana karar, `src/memory/fibonacci_pmm.rs` icindeki ownership ve state publication sinirlarini net tutmaktir.
Kodu okurken once veri yapisini, sonra state gecislerini, en son hata donuslerini takip etmek gerekir.
Aksi halde kod calissa bile neden oyle tasarlandigini anlamak zorlasir ve yanlis optimizasyon riski artar.

### Matematik modeli

- Model: `Pressure_zone = fallback_count / req_count`
- Not: Bu model karar destek icindir; tek basina dogruluk ispatı olarak kullanilmaz.

### Worst-case-first

- En kotu durum: Sik fallback gizli kapasite krizini maskeler.
- Semptom: p99/p999 gecikme artisi, tutarsiz state, veya sessiz veri bozulmasi.
- Erken tespit: invariant kontrolu, guard sayaclari, stress altinda deterministik test tekrarlandirilmasi.

### Algoritma otopsisi

1. Neden secildi: Bu algoritma hedeflenen workload sinifi icin sabit ve anlasilir karar yuzeyi sunar.
2. Ana dezavantaj: Yanlis tuning veya eksik guard ile patolojik yukte performans/correctness kaybi uretebilir.
3. echOS mitigasyonu: Zone telemetrisi ve reclaim tetigi.

### Invariant denetim cercevesi

- Denetim kurali: Dosyadaki tum atomik veya state degiskenlerini listele.
- Denetim kurali: Hangi fonksiyonun ownership devrettigini isaretle.
- Denetim kurali: Hata donen tum yollari tabloya dok.
- Denetim kurali: Bir invarianti sec ve nasil bozulabilecegini yaz.
- Denetim kurali: O invarianti koruyan satiri dosyada bulup not al.

### Olcum ve benchmark paketi

- Metrik 1: p50/p95/p99 latency
- Metrik 2: throughput veya servis hizi
- Metrik 3: hata/geri alma sayaci
- Metrik 4: queue depth veya pressure sinyali
- Metrik 5: regressions arasi fark tablosu

### Failure envelope analizi

- Failure paterni A: Ortalama metrik iyi, p99 kotu. Bu durumda kuyruk ve publication siniri odakli inceleme gerekir.
- Failure paterni B: Throughput yuksek ama hata orani artiyor. Bu durumda guard limiti ve admission politikasi yeniden ele alinmali.
- Failure paterni C: Testte temiz, sahada bozuk. Bu genelde eksik stress profili veya environment farki kaynaklidir.

### Cekirdek cikarsama seti

- Bu alt sistemin karar agacini ezbersiz aciklayabilmek
- Bir bug raporunu dogru katmana indirebilmek
- Performans iddiasini metrikle savunabilmek
- Mitigasyon secimini nedenleriyle yazabilmek

### Dosya basi notlarindan alinti

> # Fibonacci PMM — Zone Tabanlı Fiziksel Bellek Yönetimi
> Fibonacci Buddy System + Linux tarzı Zone tahsis mekanizması.
> ## Bellek Bölgeleri (Memory Zones)
> x86_64'te DMA cihazlarının erişebildiği adres aralıkları donanıma bağlıdır.
> Linux'un `mm/mmzone.h` tasarımı esas alınmıştır:
> ```
> Fiziksel Adres Uzayı:
> 0x0000_0000          0x100_0000 (16 MB)     0x1_0000_0000 (4 GB)
> │                    │                        │
> ▼                    ▼                        ▼
> ┌────────────────────┬───────────────────────┬──────────────── ···
> │    ZONE_DMA        │     ZONE_DMA32         │   ZONE_NORMAL
> │  0 → 16 MB         │  16 MB → 4 GB          │   4 GB → ∞
> │  ISA DMA (24-bit)  │  PCI 32-bit DMA        │   sınırsız
> └────────────────────┴───────────────────────┴──────────────── ···
> ```
> ## Zone Seçim Mantığı (Fallback Zinciri)

---

## M11 - Fibonacci buddy split/coalesce

### Kod baglami

- Ana dosya: `src/memory/fibonacci_buddy.rs`
- Sembol: `split_block` -> `src/memory/fibonacci_buddy.rs:148`
- Sembol: `try_coalesce` -> `src/memory/fibonacci_buddy.rs:158`
- Sembol: `find_buddy` -> `src/memory/fibonacci_buddy.rs:172`
- Sembol: `utilization` -> `src/memory/fibonacci_buddy.rs:240`

### Cekirdek fikir

Bu alt sistemde ana karar, `src/memory/fibonacci_buddy.rs` icindeki ownership ve state publication sinirlarini net tutmaktir.
Kodu okurken once veri yapisini, sonra state gecislerini, en son hata donuslerini takip etmek gerekir.
Aksi halde kod calissa bile neden oyle tasarlandigini anlamak zorlasir ve yanlis optimizasyon riski artar.

### Matematik modeli

- Model: `F(n) = F(n-1) + F(n-2)`
- Not: Bu model karar destek icindir; tek basina dogruluk ispatı olarak kullanilmaz.

### Worst-case-first

- En kotu durum: Yanlis buddy hesabinda leak veya overlap olur.
- Semptom: p99/p999 gecikme artisi, tutarsiz state, veya sessiz veri bozulmasi.
- Erken tespit: invariant kontrolu, guard sayaclari, stress altinda deterministik test tekrarlandirilmasi.

### Algoritma otopsisi

1. Neden secildi: Bu algoritma hedeflenen workload sinifi icin sabit ve anlasilir karar yuzeyi sunar.
2. Ana dezavantaj: Yanlis tuning veya eksik guard ile patolojik yukte performans/correctness kaybi uretebilir.
3. echOS mitigasyonu: Adres bazli buddy aritmetigi + recursive coalesce.

### Invariant denetim cercevesi

- Denetim kurali: Dosyadaki tum atomik veya state degiskenlerini listele.
- Denetim kurali: Hangi fonksiyonun ownership devrettigini isaretle.
- Denetim kurali: Hata donen tum yollari tabloya dok.
- Denetim kurali: Bir invarianti sec ve nasil bozulabilecegini yaz.
- Denetim kurali: O invarianti koruyan satiri dosyada bulup not al.

### Olcum ve benchmark paketi

- Metrik 1: p50/p95/p99 latency
- Metrik 2: throughput veya servis hizi
- Metrik 3: hata/geri alma sayaci
- Metrik 4: queue depth veya pressure sinyali
- Metrik 5: regressions arasi fark tablosu

### Failure envelope analizi

- Failure paterni A: Ortalama metrik iyi, p99 kotu. Bu durumda kuyruk ve publication siniri odakli inceleme gerekir.
- Failure paterni B: Throughput yuksek ama hata orani artiyor. Bu durumda guard limiti ve admission politikasi yeniden ele alinmali.
- Failure paterni C: Testte temiz, sahada bozuk. Bu genelde eksik stress profili veya environment farki kaynaklidir.

### Cekirdek cikarsama seti

- Bu alt sistemin karar agacini ezbersiz aciklayabilmek
- Bir bug raporunu dogru katmana indirebilmek
- Performans iddiasini metrikle savunabilmek
- Mitigasyon secimini nedenleriyle yazabilmek

### Dosya basi notlarindan alinti

> # FİBONACCI BUDDY SİSTEMİ - Bellek Yönetiminde Fibonacci Serisi
> ## Klasik Buddy vs Fibonacci Buddy Karşılaştırması
> ### Klasik Buddy Allocator (2'nin kuvvetleri):
> ```
> Boyutlar: 1, 2, 4, 8, 16, 32, 64, 128, 256, 512 sayfa...
> Sorun: 5 sayfa istenirse 8 sayfa verilir → %37 iç parçalanma
> ```
> ### Fibonacci Buddy Allocator (Fibonacci dizisi):
> ```
> Dizi:    1, 2, 3, 5, 8, 13, 21, 34, 55, 89, 144...
> Özellik: F(n) = F(n-1) + F(n-2)
> → 13 sayfayı bölerken: 8 + 5 = 13 (her parça da Fibonacci!)
> → 5 sayfa istenirse 5 sayfa verilir → %0 iç parçalanma
> ```
> ## Fibonacci Bölme Algoritması (SPLIT):
> ```
> Büyük blok: F(6) = 13 sayfa
> /            \
> F(5)=8           F(4)=5   ← iki Fibonacci bloğuna bölünür

---

## M12 - TLSF heap wrapper guvenligi

### Kod baglami

- Ana dosya: `src/allocator/tlsf.rs`
- Sembol: `insert_free_region_ptr` -> `src/allocator/tlsf.rs:86`
- Sembol: `alloc_from_main_heap` -> `src/allocator/tlsf.rs:290`
- Sembol: `dealloc_to_main_heap` -> `src/allocator/tlsf.rs:306`
- Sembol: `check_integrity` -> `src/allocator/tlsf.rs:220`

### Cekirdek fikir

Bu alt sistemde ana karar, `src/allocator/tlsf.rs` icindeki ownership ve state publication sinirlarini net tutmaktir.
Kodu okurken once veri yapisini, sonra state gecislerini, en son hata donuslerini takip etmek gerekir.
Aksi halde kod calissa bile neden oyle tasarlandigini anlamak zorlasir ve yanlis optimizasyon riski artar.

### Matematik modeli

- Model: `T_alloc ~= O(1), T_free ~= O(1)`
- Not: Bu model karar destek icindir; tek basina dogruluk ispatı olarak kullanilmaz.

### Worst-case-first

- En kotu durum: Heap metadata bozulmasi gec fark edilir.
- Semptom: p99/p999 gecikme artisi, tutarsiz state, veya sessiz veri bozulmasi.
- Erken tespit: invariant kontrolu, guard sayaclari, stress altinda deterministik test tekrarlandirilmasi.

### Algoritma otopsisi

1. Neden secildi: Bu algoritma hedeflenen workload sinifi icin sabit ve anlasilir karar yuzeyi sunar.
2. Ana dezavantaj: Yanlis tuning veya eksik guard ile patolojik yukte performans/correctness kaybi uretebilir.
3. echOS mitigasyonu: Canary, tracker, boundary guard ve erken heap ayrimi.

### Invariant denetim cercevesi

- Denetim kurali: Dosyadaki tum atomik veya state degiskenlerini listele.
- Denetim kurali: Hangi fonksiyonun ownership devrettigini isaretle.
- Denetim kurali: Hata donen tum yollari tabloya dok.
- Denetim kurali: Bir invarianti sec ve nasil bozulabilecegini yaz.
- Denetim kurali: O invarianti koruyan satiri dosyada bulup not al.

### Olcum ve benchmark paketi

- Metrik 1: p50/p95/p99 latency
- Metrik 2: throughput veya servis hizi
- Metrik 3: hata/geri alma sayaci
- Metrik 4: queue depth veya pressure sinyali
- Metrik 5: regressions arasi fark tablosu

### Failure envelope analizi

- Failure paterni A: Ortalama metrik iyi, p99 kotu. Bu durumda kuyruk ve publication siniri odakli inceleme gerekir.
- Failure paterni B: Throughput yuksek ama hata orani artiyor. Bu durumda guard limiti ve admission politikasi yeniden ele alinmali.
- Failure paterni C: Testte temiz, sahada bozuk. Bu genelde eksik stress profili veya environment farki kaynaklidir.

### Cekirdek cikarsama seti

- Bu alt sistemin karar agacini ezbersiz aciklayabilmek
- Bir bug raporunu dogru katmana indirebilmek
- Performans iddiasini metrikle savunabilmek
- Mitigasyon secimini nedenleriyle yazabilmek

### Dosya basi notlarindan alinti

> # echOS TLSF Allocator
> TLSF (Two-Level Segregated Fit — İki Düzeyli Ayrılmış Uyum) heap allocator sarmalayıcısı.
> O(1) allocation/deallocation performansı sağlar.
> ## TLSF Algoritması Nedir?
> TLSF, gerçek zamanlı sistemler için tasarlanmış bir bellek yönetim algoritmasıdır.
> Temel fikir: serbest blokları boyutlarına göre iki boyutlu bir bitmap indeksiyle
> organize etmektir. Bu sayede hem allocation hem deallocation O(1)'de tamamlanır.
> ## İki Düzeyli İndeks Yapısı:
> ```
> 1. Düzey (FLI - First Level Index):
> Blok boyutunun log2'si → hangi büyüklük sınıfında?
> Örn: 128-255 byte → FLI=7
> 2. Düzey (SLI - Second Level Index):
> Büyüklük sınıfı içinde daha ince ayrım
> Örn: 128-143 → SLI=0, 144-159 → SLI=1 ...
> Bitmap:
> +----+----+----+----+----+
> | FL | SL | SL | SL | .. |   <- Hangi yuvada serbest blok var?

---

## M13 - User page fault, COW ve THP karari

### Kod baglami

- Ana dosya: `src/memory/mod.rs`
- Sembol: `handle_user_page_fault` -> `src/memory/mod.rs:20`
- Sembol: `handle_cow_fault` -> `src/memory/mod.rs:26`
- Sembol: `try_map_thp_anon` -> `src/memory/mod.rs:31`
- Sembol: `sanitize_user_map_flags` -> `src/memory/mod.rs:1438`

### Cekirdek fikir

Bu alt sistemde ana karar, `src/memory/mod.rs` icindeki ownership ve state publication sinirlarini net tutmaktir.
Kodu okurken once veri yapisini, sonra state gecislerini, en son hata donuslerini takip etmek gerekir.
Aksi halde kod calissa bile neden oyle tasarlandigini anlamak zorlasir ve yanlis optimizasyon riski artar.

### Matematik modeli

- Model: `Fault_path = decision(prot, write, present, vma_kind)`
- Not: Bu model karar destek icindir; tek basina dogruluk ispatı olarak kullanilmaz.

### Worst-case-first

- En kotu durum: Yanlis fault ayrimi permission bypass veya crash uretir.
- Semptom: p99/p999 gecikme artisi, tutarsiz state, veya sessiz veri bozulmasi.
- Erken tespit: invariant kontrolu, guard sayaclari, stress altinda deterministik test tekrarlandirilmasi.

### Algoritma otopsisi

1. Neden secildi: Bu algoritma hedeflenen workload sinifi icin sabit ve anlasilir karar yuzeyi sunar.
2. Ana dezavantaj: Yanlis tuning veya eksik guard ile patolojik yukte performans/correctness kaybi uretebilir.
3. echOS mitigasyonu: Fail-closed fault ayrimi ve map flag sanitization.

### Invariant denetim cercevesi

- Denetim kurali: Dosyadaki tum atomik veya state degiskenlerini listele.
- Denetim kurali: Hangi fonksiyonun ownership devrettigini isaretle.
- Denetim kurali: Hata donen tum yollari tabloya dok.
- Denetim kurali: Bir invarianti sec ve nasil bozulabilecegini yaz.
- Denetim kurali: O invarianti koruyan satiri dosyada bulup not al.

### Olcum ve benchmark paketi

- Metrik 1: p50/p95/p99 latency
- Metrik 2: throughput veya servis hizi
- Metrik 3: hata/geri alma sayaci
- Metrik 4: queue depth veya pressure sinyali
- Metrik 5: regressions arasi fark tablosu

### Failure envelope analizi

- Failure paterni A: Ortalama metrik iyi, p99 kotu. Bu durumda kuyruk ve publication siniri odakli inceleme gerekir.
- Failure paterni B: Throughput yuksek ama hata orani artiyor. Bu durumda guard limiti ve admission politikasi yeniden ele alinmali.
- Failure paterni C: Testte temiz, sahada bozuk. Bu genelde eksik stress profili veya environment farki kaynaklidir.

### Cekirdek cikarsama seti

- Bu alt sistemin karar agacini ezbersiz aciklayabilmek
- Bir bug raporunu dogru katmana indirebilmek
- Performans iddiasini metrikle savunabilmek
- Mitigasyon secimini nedenleriyle yazabilmek

### Dosya basi notlarindan alinti

> # echOS Bellek Yönetimi — Ana Modül
> Fiziksel ve sanal bellek yönetiminin tüm katmanlarını barındıran ana modül.
> UEFI/Multiboot2 bellek haritasından başlayarak kullanıcı alanı sayfa hatalarına
> kadar tüm bellek yönetim akışını koordine eder.
> ## Modül Mimarisi
> ```
> ┌─────────────────────────────────────────────────────────┐
> │                    Kullanıcı Alanı                      │
> │  mmap / munmap / mprotect / brk / madvise              │
> └──────────────────────┬──────────────────────────────────┘
> │ sistem çağrısı
> ┌──────────────────────▼──────────────────────────────────┐
> │              AddressSpace + VMA Yönetimi                │
> │  Vma { start, end, flags, kind, cow, shared }           │
> │  VmaKind::Anonymous | File | Image                      │
> └──────────────────────┬──────────────────────────────────┘
> │ sayfa hatası → handle_user_page_fault()
> ┌──────────────────────▼──────────────────────────────────┐
> │               Sayfa Hatası İşleyici                     │
> │  handle_anon_lazy_fault()  → sıfır sayfa tahsis        │
> │  handle_image_lazy_fault() → ELF segmentini yükle      │

---

## M14 - Reclaim daemon, writeback budget ve pressure

### Kod baglami

- Ana dosya: `src/memory/mod.rs`
- Sembol: `memory_reclaim_daemon` -> `src/memory/mod.rs:40`
- Sembol: `reclaim_pages_global` -> `src/memory/mod.rs:70`
- Sembol: `process_writeback_budget` -> `src/memory/mod.rs:105`
- Sembol: `start_reclaim_daemon` -> `src/memory/mod.rs:2438`

### Cekirdek fikir

Bu alt sistemde ana karar, `src/memory/mod.rs` icindeki ownership ve state publication sinirlarini net tutmaktir.
Kodu okurken once veri yapisini, sonra state gecislerini, en son hata donuslerini takip etmek gerekir.
Aksi halde kod calissa bile neden oyle tasarlandigini anlamak zorlasir ve yanlis optimizasyon riski artar.

### Matematik modeli

- Model: `rho = lambda_dirty / mu_writeback`
- Not: Bu model karar destek icindir; tek basina dogruluk ispatı olarak kullanilmaz.

### Worst-case-first

- En kotu durum: rho > 1 kalirsa writeback kuyrugu patlar.
- Semptom: p99/p999 gecikme artisi, tutarsiz state, veya sessiz veri bozulmasi.
- Erken tespit: invariant kontrolu, guard sayaclari, stress altinda deterministik test tekrarlandirilmasi.

### Algoritma otopsisi

1. Neden secildi: Bu algoritma hedeflenen workload sinifi icin sabit ve anlasilir karar yuzeyi sunar.
2. Ana dezavantaj: Yanlis tuning veya eksik guard ile patolojik yukte performans/correctness kaybi uretebilir.
3. echOS mitigasyonu: Budget tabanli writeback ve pressure sinyali.

### Invariant denetim cercevesi

- Denetim kurali: Dosyadaki tum atomik veya state degiskenlerini listele.
- Denetim kurali: Hangi fonksiyonun ownership devrettigini isaretle.
- Denetim kurali: Hata donen tum yollari tabloya dok.
- Denetim kurali: Bir invarianti sec ve nasil bozulabilecegini yaz.
- Denetim kurali: O invarianti koruyan satiri dosyada bulup not al.

### Olcum ve benchmark paketi

- Metrik 1: p50/p95/p99 latency
- Metrik 2: throughput veya servis hizi
- Metrik 3: hata/geri alma sayaci
- Metrik 4: queue depth veya pressure sinyali
- Metrik 5: regressions arasi fark tablosu

### Failure envelope analizi

- Failure paterni A: Ortalama metrik iyi, p99 kotu. Bu durumda kuyruk ve publication siniri odakli inceleme gerekir.
- Failure paterni B: Throughput yuksek ama hata orani artiyor. Bu durumda guard limiti ve admission politikasi yeniden ele alinmali.
- Failure paterni C: Testte temiz, sahada bozuk. Bu genelde eksik stress profili veya environment farki kaynaklidir.

### Cekirdek cikarsama seti

- Bu alt sistemin karar agacini ezbersiz aciklayabilmek
- Bir bug raporunu dogru katmana indirebilmek
- Performans iddiasini metrikle savunabilmek
- Mitigasyon secimini nedenleriyle yazabilmek

### Dosya basi notlarindan alinti

> # echOS Bellek Yönetimi — Ana Modül
> Fiziksel ve sanal bellek yönetiminin tüm katmanlarını barındıran ana modül.
> UEFI/Multiboot2 bellek haritasından başlayarak kullanıcı alanı sayfa hatalarına
> kadar tüm bellek yönetim akışını koordine eder.
> ## Modül Mimarisi
> ```
> ┌─────────────────────────────────────────────────────────┐
> │                    Kullanıcı Alanı                      │
> │  mmap / munmap / mprotect / brk / madvise              │
> └──────────────────────┬──────────────────────────────────┘
> │ sistem çağrısı
> ┌──────────────────────▼──────────────────────────────────┐
> │              AddressSpace + VMA Yönetimi                │
> │  Vma { start, end, flags, kind, cow, shared }           │
> │  VmaKind::Anonymous | File | Image                      │
> └──────────────────────┬──────────────────────────────────┘
> │ sayfa hatası → handle_user_page_fault()
> ┌──────────────────────▼──────────────────────────────────┐
> │               Sayfa Hatası İşleyici                     │
> │  handle_anon_lazy_fault()  → sıfır sayfa tahsis        │
> │  handle_image_lazy_fault() → ELF segmentini yükle      │

---

## M15 - MGLRU generation ve victim secimi

### Kod baglami

- Ana dosya: `src/memory/mglru.rs`
- Sembol: `on_access` -> `src/memory/mglru.rs:111`
- Sembol: `age_tick` -> `src/memory/mglru.rs:153`
- Sembol: `pick_victim` -> `src/memory/mglru.rs:198`
- Sembol: `record_refault` -> `src/memory/mglru.rs:181`

### Cekirdek fikir

Bu alt sistemde ana karar, `src/memory/mglru.rs` icindeki ownership ve state publication sinirlarini net tutmaktir.
Kodu okurken once veri yapisini, sonra state gecislerini, en son hata donuslerini takip etmek gerekir.
Aksi halde kod calissa bile neden oyle tasarlandigini anlamak zorlasir ve yanlis optimizasyon riski artar.

### Matematik modeli

- Model: `victim = argmin(generation, hot_score)`
- Not: Bu model karar destek icindir; tek basina dogruluk ispatı olarak kullanilmaz.

### Worst-case-first

- En kotu durum: Yanlis aging policy refault dalgasi uretir.
- Semptom: p99/p999 gecikme artisi, tutarsiz state, veya sessiz veri bozulmasi.
- Erken tespit: invariant kontrolu, guard sayaclari, stress altinda deterministik test tekrarlandirilmasi.

### Algoritma otopsisi

1. Neden secildi: Bu algoritma hedeflenen workload sinifi icin sabit ve anlasilir karar yuzeyi sunar.
2. Ana dezavantaj: Yanlis tuning veya eksik guard ile patolojik yukte performans/correctness kaybi uretebilir.
3. echOS mitigasyonu: Generation + access_count + refault promotion.

### Invariant denetim cercevesi

- Denetim kurali: Dosyadaki tum atomik veya state degiskenlerini listele.
- Denetim kurali: Hangi fonksiyonun ownership devrettigini isaretle.
- Denetim kurali: Hata donen tum yollari tabloya dok.
- Denetim kurali: Bir invarianti sec ve nasil bozulabilecegini yaz.
- Denetim kurali: O invarianti koruyan satiri dosyada bulup not al.

### Olcum ve benchmark paketi

- Metrik 1: p50/p95/p99 latency
- Metrik 2: throughput veya servis hizi
- Metrik 3: hata/geri alma sayaci
- Metrik 4: queue depth veya pressure sinyali
- Metrik 5: regressions arasi fark tablosu

### Failure envelope analizi

- Failure paterni A: Ortalama metrik iyi, p99 kotu. Bu durumda kuyruk ve publication siniri odakli inceleme gerekir.
- Failure paterni B: Throughput yuksek ama hata orani artiyor. Bu durumda guard limiti ve admission politikasi yeniden ele alinmali.
- Failure paterni C: Testte temiz, sahada bozuk. Bu genelde eksik stress profili veya environment farki kaynaklidir.

### Cekirdek cikarsama seti

- Bu alt sistemin karar agacini ezbersiz aciklayabilmek
- Bir bug raporunu dogru katmana indirebilmek
- Performans iddiasini metrikle savunabilmek
- Mitigasyon secimini nedenleriyle yazabilmek

### Dosya basi notlarindan alinti

> Multi-Gen LRU (MGLRU) — sıcak/soğuk sayfa sınıflandırması.
> Bu modül, klasik active/inactive LRU'ya ek olarak sayfaları nesiller
> (generation) üzerinden takip eder. Amaç:
> 1. Hot/cold ayrımını erişim geri-bildirimiyle yapmak
> 2. Reclaim sırasında en eski nesilden başlamak
> 3. Refault durumunda sayfayı hızlıca sıcak nesle taşımak

---

## M16 - ZSwap compression pipeline

### Kod baglami

- Ana dosya: `src/memory/zswap.rs`
- Sembol: `compress` -> `src/memory/zswap.rs:96`
- Sembol: `decompress` -> `src/memory/zswap.rs:98`
- Sembol: `ZSWAP_DEFAULT_POOL_PERCENT` -> `src/memory/zswap.rs:83`
- Sembol: `Compressor` -> `src/memory/zswap.rs:26`

### Cekirdek fikir

Bu alt sistemde ana karar, `src/memory/zswap.rs` icindeki ownership ve state publication sinirlarini net tutmaktir.
Kodu okurken once veri yapisini, sonra state gecislerini, en son hata donuslerini takip etmek gerekir.
Aksi halde kod calissa bile neden oyle tasarlandigini anlamak zorlasir ve yanlis optimizasyon riski artar.

### Matematik modeli

- Model: `Gain = IO_saved - CPU_compress_cost`
- Not: Bu model karar destek icindir; tek basina dogruluk ispatı olarak kullanilmaz.

### Worst-case-first

- En kotu durum: Yanlis algoritma secimi CPU'yu bogar.
- Semptom: p99/p999 gecikme artisi, tutarsiz state, veya sessiz veri bozulmasi.
- Erken tespit: invariant kontrolu, guard sayaclari, stress altinda deterministik test tekrarlandirilmasi.

### Algoritma otopsisi

1. Neden secildi: Bu algoritma hedeflenen workload sinifi icin sabit ve anlasilir karar yuzeyi sunar.
2. Ana dezavantaj: Yanlis tuning veya eksik guard ile patolojik yukte performans/correctness kaybi uretebilir.
3. echOS mitigasyonu: Pool limiti, compressor secimi ve fallback yolu.

### Invariant denetim cercevesi

- Denetim kurali: Dosyadaki tum atomik veya state degiskenlerini listele.
- Denetim kurali: Hangi fonksiyonun ownership devrettigini isaretle.
- Denetim kurali: Hata donen tum yollari tabloya dok.
- Denetim kurali: Bir invarianti sec ve nasil bozulabilecegini yaz.
- Denetim kurali: O invarianti koruyan satiri dosyada bulup not al.

### Olcum ve benchmark paketi

- Metrik 1: p50/p95/p99 latency
- Metrik 2: throughput veya servis hizi
- Metrik 3: hata/geri alma sayaci
- Metrik 4: queue depth veya pressure sinyali
- Metrik 5: regressions arasi fark tablosu

### Failure envelope analizi

- Failure paterni A: Ortalama metrik iyi, p99 kotu. Bu durumda kuyruk ve publication siniri odakli inceleme gerekir.
- Failure paterni B: Throughput yuksek ama hata orani artiyor. Bu durumda guard limiti ve admission politikasi yeniden ele alinmali.
- Failure paterni C: Testte temiz, sahada bozuk. Bu genelde eksik stress profili veya environment farki kaynaklidir.

### Cekirdek cikarsama seti

- Bu alt sistemin karar agacini ezbersiz aciklayabilmek
- Bir bug raporunu dogru katmana indirebilmek
- Performans iddiasini metrikle savunabilmek
- Mitigasyon secimini nedenleriyle yazabilmek

### Dosya basi notlarindan alinti

> # ZSwap / ZRam — Sıkıştırılmış Bellek Tasas Havuzu
> Kirli sayfaları diske yazmadan önce RAM içinde sıkıştıran ara katman.
> ## ZSwap Neden Gerekli?
> Geleneksel swap akışı çok yavaştır:
> ```
> kirli sayfa → diske yaz (ms cinsinden gecikme) → disk oku → RAM'e geri yükle
> ```
> ZSwap bu akışa sıkıştırma enjekte eder:
> ```
> kirli sayfa → sıkıştır (LZ4/ZSTD) → zpool'a yaz (RAM'de)
> ↓ havuz doldu
> takas alanına geri yaz (disk)
> ```
> ## ZSwap Boru Hattı (Pipeline):
> ```
> Uygulama sayfası (4 KB)
> │
> ▼

---

## M17 - Lock-free io_uring publication boundaries

### Kod baglami

- Ana dosya: `src/posix/io_uring_ring.rs`
- Sembol: `push` -> `src/posix/io_uring_ring.rs:298`
- Sembol: `pop` -> `src/posix/io_uring_ring.rs:338`
- Sembol: `pop_batch` -> `src/posix/io_uring_ring.rs:371`
- Sembol: `process_submissions` -> `src/posix/io_uring_ring.rs:594`

### Cekirdek fikir

Bu alt sistemde ana karar, `src/posix/io_uring_ring.rs` icindeki ownership ve state publication sinirlarini net tutmaktir.
Kodu okurken once veri yapisini, sonra state gecislerini, en son hata donuslerini takip etmek gerekir.
Aksi halde kod calissa bile neden oyle tasarlandigini anlamak zorlasir ve yanlis optimizasyon riski artar.

### Matematik modeli

- Model: `occupancy = tail - head`
- Not: Bu model karar destek icindir; tek basina dogruluk ispatı olarak kullanilmaz.

### Worst-case-first

- En kotu durum: Tail erken publish edilirse stale read olur.
- Semptom: p99/p999 gecikme artisi, tutarsiz state, veya sessiz veri bozulmasi.
- Erken tespit: invariant kontrolu, guard sayaclari, stress altinda deterministik test tekrarlandirilmasi.

### Algoritma otopsisi

1. Neden secildi: Bu algoritma hedeflenen workload sinifi icin sabit ve anlasilir karar yuzeyi sunar.
2. Ana dezavantaj: Yanlis tuning veya eksik guard ile patolojik yukte performans/correctness kaybi uretebilir.
3. echOS mitigasyonu: smp_wmb/smp_rmb ve Acquire/Release disiplini.

### Invariant denetim cercevesi

- Denetim kurali: Dosyadaki tum atomik veya state degiskenlerini listele.
- Denetim kurali: Hangi fonksiyonun ownership devrettigini isaretle.
- Denetim kurali: Hata donen tum yollari tabloya dok.
- Denetim kurali: Bir invarianti sec ve nasil bozulabilecegini yaz.
- Denetim kurali: O invarianti koruyan satiri dosyada bulup not al.

### Olcum ve benchmark paketi

- Metrik 1: p50/p95/p99 latency
- Metrik 2: throughput veya servis hizi
- Metrik 3: hata/geri alma sayaci
- Metrik 4: queue depth veya pressure sinyali
- Metrik 5: regressions arasi fark tablosu

### Failure envelope analizi

- Failure paterni A: Ortalama metrik iyi, p99 kotu. Bu durumda kuyruk ve publication siniri odakli inceleme gerekir.
- Failure paterni B: Throughput yuksek ama hata orani artiyor. Bu durumda guard limiti ve admission politikasi yeniden ele alinmali.
- Failure paterni C: Testte temiz, sahada bozuk. Bu genelde eksik stress profili veya environment farki kaynaklidir.

### Cekirdek cikarsama seti

- Bu alt sistemin karar agacini ezbersiz aciklayabilmek
- Bir bug raporunu dogru katmana indirebilmek
- Performans iddiasini metrikle savunabilmek
- Mitigasyon secimini nedenleriyle yazabilmek

### Dosya basi notlarindan alinti

> # echOS Lock-Free io_uring Ring Buffer
> Linux io_uring uyumlu, YEDİ SIFIR KİLİT prensibine göre tasarlanmış
> Submission Queue (SQ) ve Completion Queue (CQ) ring buffer implementasyonu.
> ## Mimari
> ```text
> ┌─────────────────────────────────────────────────────────────────┐
> │  io_uring Lock-Free Ring Architecture                          │
> │                                                                │
> │  Kullanıcı Alanı (Producer)           Kernel (Consumer)        │
> │  ┌──────────────┐                     ┌──────────────┐        │
> │  │  SQE yazma   │ ───smp_wmb()───►    │  SQE okuma   │        │
> │  │  tail++      │    (sfence)         │  head++      │        │
> │  └──────────────┘                     └──────────────┘        │
> │                                                                │
> │  Kernel (Producer)                    Kullanıcı (Consumer)     │
> │  ┌──────────────┐                     ┌──────────────┐        │
> │  │  CQE yazma   │ ───smp_wmb()───►    │  CQE okuma   │        │
> │  │  tail++      │    (sfence)         │  head++      │        │
> │  └──────────────┘                     └──────────────┘        │
> │                                                                │
> │  Sıralama Garantisi:                                          │

---

## M18 - TLS 1.3 handshake ve key schedule

### Kod baglami

- Ana dosya: `src/net/tls.rs`
- Sembol: `derive_handshake_secret` -> `src/net/tls.rs:443`
- Sembol: `derive_master_secret` -> `src/net/tls.rs:459`
- Sembol: `hkdf_expand_label` -> `src/net/tls.rs:444`
- Sembol: `process_server_hello` -> `src/net/tls.rs:572`

### Cekirdek fikir

Bu alt sistemde ana karar, `src/net/tls.rs` icindeki ownership ve state publication sinirlarini net tutmaktir.
Kodu okurken once veri yapisini, sonra state gecislerini, en son hata donuslerini takip etmek gerekir.
Aksi halde kod calissa bile neden oyle tasarlandigini anlamak zorlasir ve yanlis optimizasyon riski artar.

### Matematik modeli

- Model: `Master = HKDF(HandshakeSecret, 0)`
- Not: Bu model karar destek icindir; tek basina dogruluk ispatı olarak kullanilmaz.

### Worst-case-first

- En kotu durum: State gecisi veya transcript hatasi guven modeli kirar.
- Semptom: p99/p999 gecikme artisi, tutarsiz state, veya sessiz veri bozulmasi.
- Erken tespit: invariant kontrolu, guard sayaclari, stress altinda deterministik test tekrarlandirilmasi.

### Algoritma otopsisi

1. Neden secildi: Bu algoritma hedeflenen workload sinifi icin sabit ve anlasilir karar yuzeyi sunar.
2. Ana dezavantaj: Yanlis tuning veya eksik guard ile patolojik yukte performans/correctness kaybi uretebilir.
3. echOS mitigasyonu: Tipli handshake state ve explicit key schedule adimlari.

### Invariant denetim cercevesi

- Denetim kurali: Dosyadaki tum atomik veya state degiskenlerini listele.
- Denetim kurali: Hangi fonksiyonun ownership devrettigini isaretle.
- Denetim kurali: Hata donen tum yollari tabloya dok.
- Denetim kurali: Bir invarianti sec ve nasil bozulabilecegini yaz.
- Denetim kurali: O invarianti koruyan satiri dosyada bulup not al.

### Olcum ve benchmark paketi

- Metrik 1: p50/p95/p99 latency
- Metrik 2: throughput veya servis hizi
- Metrik 3: hata/geri alma sayaci
- Metrik 4: queue depth veya pressure sinyali
- Metrik 5: regressions arasi fark tablosu

### Failure envelope analizi

- Failure paterni A: Ortalama metrik iyi, p99 kotu. Bu durumda kuyruk ve publication siniri odakli inceleme gerekir.
- Failure paterni B: Throughput yuksek ama hata orani artiyor. Bu durumda guard limiti ve admission politikasi yeniden ele alinmali.
- Failure paterni C: Testte temiz, sahada bozuk. Bu genelde eksik stress profili veya environment farki kaynaklidir.

### Cekirdek cikarsama seti

- Bu alt sistemin karar agacini ezbersiz aciklayabilmek
- Bir bug raporunu dogru katmana indirebilmek
- Performans iddiasini metrikle savunabilmek
- Mitigasyon secimini nedenleriyle yazabilmek

### Dosya basi notlarindan alinti

> # TLS 1.3 Protokolü (Transport Layer Security)
> echOS için TLS 1.3 el sıkışma durum makinesi.
> ## TLS 1.3 Nedir?
> TLS (Transport Layer Security), ağ üzerindeki iletişimi kriptografik olarak
> güvence altına alan protokoldür. HTTPS, SMTPS, FTPS ve daha birçok protokolün
> temelini oluşturur.
> ## TLS 1.3 El Sıkışma Diyagramı
> ```
> İstemci                              Sunucu
> |                                    |
> |---- ClientHello ------------------>|  Desteklenen cipher suites, key_share
> |                                    |
> |<--- ServerHello -------------------|  Cipher suite seçimi, key_share
> |<--- {EncryptedExtensions} ---------|  Şifrelenmiş uzantılar
> |<--- {Certificate} -----------------|  Sunucu sertifikası
> |<--- {CertificateVerify} -----------|  Sertifika imzası
> |<--- {Finished} --------------------|  El sıkışma MAC'i
> |                                    |
> |---- {Finished} ------------------->|  İstemci onayı

---

## M19 - QUIC frame parser ve ACK guard

### Kod baglami

- Ana dosya: `src/net/quic.rs`
- Sembol: `encode_varint` -> `src/net/quic.rs:526`
- Sembol: `decode_varint` -> `src/net/quic.rs:681`
- Sembol: `decode` -> `src/net/quic.rs:292`
- Sembol: `MAX_ACK_RANGES` -> `src/net/quic.rs:104`

### Cekirdek fikir

Bu alt sistemde ana karar, `src/net/quic.rs` icindeki ownership ve state publication sinirlarini net tutmaktir.
Kodu okurken once veri yapisini, sonra state gecislerini, en son hata donuslerini takip etmek gerekir.
Aksi halde kod calissa bile neden oyle tasarlandigini anlamak zorlasir ve yanlis optimizasyon riski artar.

### Matematik modeli

- Model: `RTT_connect ~= 1 * RTT (1-RTT)`
- Not: Bu model karar destek icindir; tek basina dogruluk ispatı olarak kullanilmaz.

### Worst-case-first

- En kotu durum: Parser limitsizligi memory amplification yapar.
- Semptom: p99/p999 gecikme artisi, tutarsiz state, veya sessiz veri bozulmasi.
- Erken tespit: invariant kontrolu, guard sayaclari, stress altinda deterministik test tekrarlandirilmasi.

### Algoritma otopsisi

1. Neden secildi: Bu algoritma hedeflenen workload sinifi icin sabit ve anlasilir karar yuzeyi sunar.
2. Ana dezavantaj: Yanlis tuning veya eksik guard ile patolojik yukte performans/correctness kaybi uretebilir.
3. echOS mitigasyonu: ACK range limiti ve frame decode guardlari.

### Invariant denetim cercevesi

- Denetim kurali: Dosyadaki tum atomik veya state degiskenlerini listele.
- Denetim kurali: Hangi fonksiyonun ownership devrettigini isaretle.
- Denetim kurali: Hata donen tum yollari tabloya dok.
- Denetim kurali: Bir invarianti sec ve nasil bozulabilecegini yaz.
- Denetim kurali: O invarianti koruyan satiri dosyada bulup not al.

### Olcum ve benchmark paketi

- Metrik 1: p50/p95/p99 latency
- Metrik 2: throughput veya servis hizi
- Metrik 3: hata/geri alma sayaci
- Metrik 4: queue depth veya pressure sinyali
- Metrik 5: regressions arasi fark tablosu

### Failure envelope analizi

- Failure paterni A: Ortalama metrik iyi, p99 kotu. Bu durumda kuyruk ve publication siniri odakli inceleme gerekir.
- Failure paterni B: Throughput yuksek ama hata orani artiyor. Bu durumda guard limiti ve admission politikasi yeniden ele alinmali.
- Failure paterni C: Testte temiz, sahada bozuk. Bu genelde eksik stress profili veya environment farki kaynaklidir.

### Cekirdek cikarsama seti

- Bu alt sistemin karar agacini ezbersiz aciklayabilmek
- Bir bug raporunu dogru katmana indirebilmek
- Performans iddiasini metrikle savunabilmek
- Mitigasyon secimini nedenleriyle yazabilmek

### Dosya basi notlarindan alinti

> # QUIC Protokolü (RFC 9000)
> HTTP/3'ün taşıma katmanı: UDP tabanlı, TLS 1.3 ile şifreli, çok akışlı (multiplexed)
> ve bağlantı geçişi destekleyen modern aktarım protokolü.
> ## QUIC Nedir?
> QUIC, TCP'nin sınırlılıklarını aşmak için Google tarafından geliştirilen ve
> IETF tarafından RFC 9000 ile standartlaştırılan aktarım protokolüdür.
> ## TCP vs QUIC Karşılaştırması
> ```
> TCP + TLS 1.3:                QUIC (v1):
> ─────────────────────────     ────────────────────────
> TCP SYN                  →    Initial (ClientHello)
> TCP SYN-ACK              ←    Initial + Handshake
> TCP ACK                  →
> TLS ClientHello          →    (tek RTT el sıkışma) ←────────────┐
> TLS ServerHello          ←                                       │
> TLS Finished(C+S)        →←   1-RTT ile bağlantı kurulur        │
> 0-RTT ile HEMEN veri gönderilebilir │
> Toplam: 2 RTT             Toplam: 1 RTT (0-RTT: 0)──────────────┘
> ```

---

## M20 - WireGuard handshake, nonce ve replay koruma

### Kod baglami

- Ana dosya: `src/net/wireguard.rs`
- Sembol: `initiate_handshake` -> `src/net/wireguard.rs:451`
- Sembol: `encrypt_packet` -> `src/net/wireguard.rs:253`
- Sembol: `decrypt_packet` -> `src/net/wireguard.rs:297`
- Sembol: `is_allowed_ip` -> `src/net/wireguard.rs:229`

### Cekirdek fikir

Bu alt sistemde ana karar, `src/net/wireguard.rs` icindeki ownership ve state publication sinirlarini net tutmaktir.
Kodu okurken once veri yapisini, sonra state gecislerini, en son hata donuslerini takip etmek gerekir.
Aksi halde kod calissa bile neden oyle tasarlandigini anlamak zorlasir ve yanlis optimizasyon riski artar.

### Matematik modeli

- Model: `nonce_next > nonce_prev`
- Not: Bu model karar destek icindir; tek basina dogruluk ispatı olarak kullanilmaz.

### Worst-case-first

- En kotu durum: Nonce tekrarinda replay kabul riski.
- Semptom: p99/p999 gecikme artisi, tutarsiz state, veya sessiz veri bozulmasi.
- Erken tespit: invariant kontrolu, guard sayaclari, stress altinda deterministik test tekrarlandirilmasi.

### Algoritma otopsisi

1. Neden secildi: Bu algoritma hedeflenen workload sinifi icin sabit ve anlasilir karar yuzeyi sunar.
2. Ana dezavantaj: Yanlis tuning veya eksik guard ile patolojik yukte performans/correctness kaybi uretebilir.
3. echOS mitigasyonu: Monoton nonce kontrolu ve session state.

### Invariant denetim cercevesi

- Denetim kurali: Dosyadaki tum atomik veya state degiskenlerini listele.
- Denetim kurali: Hangi fonksiyonun ownership devrettigini isaretle.
- Denetim kurali: Hata donen tum yollari tabloya dok.
- Denetim kurali: Bir invarianti sec ve nasil bozulabilecegini yaz.
- Denetim kurali: O invarianti koruyan satiri dosyada bulup not al.

### Olcum ve benchmark paketi

- Metrik 1: p50/p95/p99 latency
- Metrik 2: throughput veya servis hizi
- Metrik 3: hata/geri alma sayaci
- Metrik 4: queue depth veya pressure sinyali
- Metrik 5: regressions arasi fark tablosu

### Failure envelope analizi

- Failure paterni A: Ortalama metrik iyi, p99 kotu. Bu durumda kuyruk ve publication siniri odakli inceleme gerekir.
- Failure paterni B: Throughput yuksek ama hata orani artiyor. Bu durumda guard limiti ve admission politikasi yeniden ele alinmali.
- Failure paterni C: Testte temiz, sahada bozuk. Bu genelde eksik stress profili veya environment farki kaynaklidir.

### Cekirdek cikarsama seti

- Bu alt sistemin karar agacini ezbersiz aciklayabilmek
- Bir bug raporunu dogru katmana indirebilmek
- Performans iddiasini metrikle savunabilmek
- Mitigasyon secimini nedenleriyle yazabilmek

### Dosya basi notlarindan alinti

> # WireGuard VPN Protokolü
> Modern, yüksek performanslı VPN protokolü.
> RFC önerisi: https://www.wireguard.com/papers/wireguard.pdf
> ## WireGuard Nedir?
> WireGuard, önceki VPN protokollerine (OpenVPN, IPSec) göre çok daha basit
> ve güvenli bir tünel protokolüdür. Linux kernel'ine 5.6'da entegre edildi.
> ## WireGuard El Sıkışma Akışı (Noise Protocol Çerçevesi)
> ```
> Başlatıcı (Initiator)              Yanıtlayıcı (Responder)
> |                                    |
> |--- Initiation Msg (Type 1) ------->|   DHKE + kimlik doğr.
> |<-- Response Msg (Type 2) ----------|   DHKE tamamla
> |                                    |
> |=== Transport Msg (Type 4) ========>|   Şifreli tünel aktif
> |<== Transport Msg (Type 4) =========|
> Her mesaj ChaCha20-Poly1305 ile şifrelenir.
> Anahtar türetme için HKDF kullanılır.
> ```

---

## M21 - HPACK Huffman decode fail-closed modeli

### Kod baglami

- Ana dosya: `src/net/http2_huffman.rs`
- Sembol: `decode_huffman` -> `src/net/http2_huffman.rs:144`
- Sembol: `BitIterator` -> `src/net/http2_huffman.rs:61`
- Sembol: `InvalidPadding` -> `src/net/http2_huffman.rs:7`
- Sembol: `EosInString` -> `src/net/http2_huffman.rs:8`

### Cekirdek fikir

Bu alt sistemde ana karar, `src/net/http2_huffman.rs` icindeki ownership ve state publication sinirlarini net tutmaktir.
Kodu okurken once veri yapisini, sonra state gecislerini, en son hata donuslerini takip etmek gerekir.
Aksi halde kod calissa bile neden oyle tasarlandigini anlamak zorlasir ve yanlis optimizasyon riski artar.

### Matematik modeli

- Model: `Decode = traverse(bits) + padding_validation`
- Not: Bu model karar destek icindir; tek basina dogruluk ispatı olarak kullanilmaz.

### Worst-case-first

- En kotu durum: EOS/padding hatalari parser acigi uretir.
- Semptom: p99/p999 gecikme artisi, tutarsiz state, veya sessiz veri bozulmasi.
- Erken tespit: invariant kontrolu, guard sayaclari, stress altinda deterministik test tekrarlandirilmasi.

### Algoritma otopsisi

1. Neden secildi: Bu algoritma hedeflenen workload sinifi icin sabit ve anlasilir karar yuzeyi sunar.
2. Ana dezavantaj: Yanlis tuning veya eksik guard ile patolojik yukte performans/correctness kaybi uretebilir.
3. echOS mitigasyonu: InvalidPadding ve EosInString ile fail-closed cikis.

### Invariant denetim cercevesi

- Denetim kurali: Dosyadaki tum atomik veya state degiskenlerini listele.
- Denetim kurali: Hangi fonksiyonun ownership devrettigini isaretle.
- Denetim kurali: Hata donen tum yollari tabloya dok.
- Denetim kurali: Bir invarianti sec ve nasil bozulabilecegini yaz.
- Denetim kurali: O invarianti koruyan satiri dosyada bulup not al.

### Olcum ve benchmark paketi

- Metrik 1: p50/p95/p99 latency
- Metrik 2: throughput veya servis hizi
- Metrik 3: hata/geri alma sayaci
- Metrik 4: queue depth veya pressure sinyali
- Metrik 5: regressions arasi fark tablosu

### Failure envelope analizi

- Failure paterni A: Ortalama metrik iyi, p99 kotu. Bu durumda kuyruk ve publication siniri odakli inceleme gerekir.
- Failure paterni B: Throughput yuksek ama hata orani artiyor. Bu durumda guard limiti ve admission politikasi yeniden ele alinmali.
- Failure paterni C: Testte temiz, sahada bozuk. Bu genelde eksik stress profili veya environment farki kaynaklidir.

### Cekirdek cikarsama seti

- Bu alt sistemin karar agacini ezbersiz aciklayabilmek
- Bir bug raporunu dogru katmana indirebilmek
- Performans iddiasini metrikle savunabilmek
- Mitigasyon secimini nedenleriyle yazabilmek

---
