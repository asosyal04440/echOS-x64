# Cilt 1 Core Denetim Korpusu

Bu bolum, onceki genis soru kumesinin tamamini hard-core muhendislik denetim
maddelerine donusturur. Her satir, belirli bir kod yolda dogrulanacak bir kontrol
ifadesi olarak yazilir.

Denetim protokolu:

1. Maddeyi oku ve ilgili dosyada satir referansini cikar.
2. Invarianti yazili hale getir ve ihlal semptomunu not et.
3. Mitigasyon satirini ayni raporda belirt.

---



## Protokol 01 - Boot, platform init ve erken dogruluk


- Denetim maddesi: `Boot, platform init ve erken dogruluk` baglaminda `init_platform_iommu` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `Boot, platform init ve erken dogruluk` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `Boot, platform init ve erken dogruluk` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/main.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `Boot, platform init ve erken dogruluk` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `Boot, platform init ve erken dogruluk` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `Boot, platform init ve erken dogruluk` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `Boot, platform init ve erken dogruluk` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `Boot, platform init ve erken dogruluk` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `Boot, platform init ve erken dogruluk` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `Boot, platform init ve erken dogruluk` baglaminda `serial_init` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `Boot, platform init ve erken dogruluk` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `Boot, platform init ve erken dogruluk` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/main.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `Boot, platform init ve erken dogruluk` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `Boot, platform init ve erken dogruluk` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `Boot, platform init ve erken dogruluk` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `Boot, platform init ve erken dogruluk` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `Boot, platform init ve erken dogruluk` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `Boot, platform init ve erken dogruluk` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `Boot, platform init ve erken dogruluk` baglaminda `init_platform_iommu` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `Boot, platform init ve erken dogruluk` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `Boot, platform init ve erken dogruluk` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/main.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `Boot, platform init ve erken dogruluk` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `Boot, platform init ve erken dogruluk` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `Boot, platform init ve erken dogruluk` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `Boot, platform init ve erken dogruluk` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `Boot, platform init ve erken dogruluk` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `Boot, platform init ve erken dogruluk` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `Boot, platform init ve erken dogruluk` baglaminda `serial_init` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `Boot, platform init ve erken dogruluk` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `Boot, platform init ve erken dogruluk` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/main.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `Boot, platform init ve erken dogruluk` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `Boot, platform init ve erken dogruluk` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `Boot, platform init ve erken dogruluk` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `Boot, platform init ve erken dogruluk` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `Boot, platform init ve erken dogruluk` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `Boot, platform init ve erken dogruluk` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `Boot, platform init ve erken dogruluk` baglaminda `init_platform_iommu` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `Boot, platform init ve erken dogruluk` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `Boot, platform init ve erken dogruluk` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/main.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `Boot, platform init ve erken dogruluk` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `Boot, platform init ve erken dogruluk` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `Boot, platform init ve erken dogruluk` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `Boot, platform init ve erken dogruluk` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `Boot, platform init ve erken dogruluk` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `Boot, platform init ve erken dogruluk` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.


## Protokol 02 - Bootstrap frame allocator ve fiziksel aralik korumasi


- Denetim maddesi: `Bootstrap frame allocator ve fiziksel aralik korumasi` baglaminda `allocate_frame_internal` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `Bootstrap frame allocator ve fiziksel aralik korumasi` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `Bootstrap frame allocator ve fiziksel aralik korumasi` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/memory/frame_allocator.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `Bootstrap frame allocator ve fiziksel aralik korumasi` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `Bootstrap frame allocator ve fiziksel aralik korumasi` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `Bootstrap frame allocator ve fiziksel aralik korumasi` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `Bootstrap frame allocator ve fiziksel aralik korumasi` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `Bootstrap frame allocator ve fiziksel aralik korumasi` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `Bootstrap frame allocator ve fiziksel aralik korumasi` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `Bootstrap frame allocator ve fiziksel aralik korumasi` baglaminda `overlaps_kernel` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `Bootstrap frame allocator ve fiziksel aralik korumasi` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `Bootstrap frame allocator ve fiziksel aralik korumasi` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/memory/frame_allocator.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `Bootstrap frame allocator ve fiziksel aralik korumasi` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `Bootstrap frame allocator ve fiziksel aralik korumasi` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `Bootstrap frame allocator ve fiziksel aralik korumasi` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `Bootstrap frame allocator ve fiziksel aralik korumasi` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `Bootstrap frame allocator ve fiziksel aralik korumasi` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `Bootstrap frame allocator ve fiziksel aralik korumasi` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `Bootstrap frame allocator ve fiziksel aralik korumasi` baglaminda `allocate_frame_internal` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `Bootstrap frame allocator ve fiziksel aralik korumasi` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `Bootstrap frame allocator ve fiziksel aralik korumasi` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/memory/frame_allocator.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `Bootstrap frame allocator ve fiziksel aralik korumasi` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `Bootstrap frame allocator ve fiziksel aralik korumasi` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `Bootstrap frame allocator ve fiziksel aralik korumasi` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `Bootstrap frame allocator ve fiziksel aralik korumasi` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `Bootstrap frame allocator ve fiziksel aralik korumasi` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `Bootstrap frame allocator ve fiziksel aralik korumasi` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `Bootstrap frame allocator ve fiziksel aralik korumasi` baglaminda `overlaps_kernel` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `Bootstrap frame allocator ve fiziksel aralik korumasi` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `Bootstrap frame allocator ve fiziksel aralik korumasi` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/memory/frame_allocator.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `Bootstrap frame allocator ve fiziksel aralik korumasi` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `Bootstrap frame allocator ve fiziksel aralik korumasi` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `Bootstrap frame allocator ve fiziksel aralik korumasi` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `Bootstrap frame allocator ve fiziksel aralik korumasi` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `Bootstrap frame allocator ve fiziksel aralik korumasi` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `Bootstrap frame allocator ve fiziksel aralik korumasi` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `Bootstrap frame allocator ve fiziksel aralik korumasi` baglaminda `allocate_frame_internal` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `Bootstrap frame allocator ve fiziksel aralik korumasi` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `Bootstrap frame allocator ve fiziksel aralik korumasi` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/memory/frame_allocator.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `Bootstrap frame allocator ve fiziksel aralik korumasi` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `Bootstrap frame allocator ve fiziksel aralik korumasi` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `Bootstrap frame allocator ve fiziksel aralik korumasi` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `Bootstrap frame allocator ve fiziksel aralik korumasi` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `Bootstrap frame allocator ve fiziksel aralik korumasi` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `Bootstrap frame allocator ve fiziksel aralik korumasi` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.


## Protokol 03 - SMP scheduler karar modeli


- Denetim maddesi: `SMP scheduler karar modeli` baglaminda `choose_spawn_cpu` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `SMP scheduler karar modeli` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `SMP scheduler karar modeli` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/task/scheduler.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `SMP scheduler karar modeli` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `SMP scheduler karar modeli` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `SMP scheduler karar modeli` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `SMP scheduler karar modeli` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `SMP scheduler karar modeli` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `SMP scheduler karar modeli` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `SMP scheduler karar modeli` baglaminda `publish_worker_load` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `SMP scheduler karar modeli` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `SMP scheduler karar modeli` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/task/scheduler.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `SMP scheduler karar modeli` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `SMP scheduler karar modeli` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `SMP scheduler karar modeli` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `SMP scheduler karar modeli` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `SMP scheduler karar modeli` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `SMP scheduler karar modeli` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `SMP scheduler karar modeli` baglaminda `choose_spawn_cpu` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `SMP scheduler karar modeli` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `SMP scheduler karar modeli` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/task/scheduler.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `SMP scheduler karar modeli` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `SMP scheduler karar modeli` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `SMP scheduler karar modeli` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `SMP scheduler karar modeli` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `SMP scheduler karar modeli` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `SMP scheduler karar modeli` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `SMP scheduler karar modeli` baglaminda `publish_worker_load` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `SMP scheduler karar modeli` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `SMP scheduler karar modeli` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/task/scheduler.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `SMP scheduler karar modeli` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `SMP scheduler karar modeli` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `SMP scheduler karar modeli` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `SMP scheduler karar modeli` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `SMP scheduler karar modeli` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `SMP scheduler karar modeli` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `SMP scheduler karar modeli` baglaminda `choose_spawn_cpu` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `SMP scheduler karar modeli` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `SMP scheduler karar modeli` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/task/scheduler.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `SMP scheduler karar modeli` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `SMP scheduler karar modeli` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `SMP scheduler karar modeli` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `SMP scheduler karar modeli` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `SMP scheduler karar modeli` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `SMP scheduler karar modeli` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.


## Protokol 04 - RT scheduler: FIFO/RR ve runtime limiti


- Denetim maddesi: `RT scheduler: FIFO/RR ve runtime limiti` baglaminda `calculate_timeslice` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `RT scheduler: FIFO/RR ve runtime limiti` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `RT scheduler: FIFO/RR ve runtime limiti` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/task/rt_scheduler.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `RT scheduler: FIFO/RR ve runtime limiti` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `RT scheduler: FIFO/RR ve runtime limiti` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `RT scheduler: FIFO/RR ve runtime limiti` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `RT scheduler: FIFO/RR ve runtime limiti` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `RT scheduler: FIFO/RR ve runtime limiti` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `RT scheduler: FIFO/RR ve runtime limiti` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `RT scheduler: FIFO/RR ve runtime limiti` baglaminda `tick` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `RT scheduler: FIFO/RR ve runtime limiti` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `RT scheduler: FIFO/RR ve runtime limiti` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/task/rt_scheduler.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `RT scheduler: FIFO/RR ve runtime limiti` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `RT scheduler: FIFO/RR ve runtime limiti` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `RT scheduler: FIFO/RR ve runtime limiti` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `RT scheduler: FIFO/RR ve runtime limiti` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `RT scheduler: FIFO/RR ve runtime limiti` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `RT scheduler: FIFO/RR ve runtime limiti` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `RT scheduler: FIFO/RR ve runtime limiti` baglaminda `calculate_timeslice` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `RT scheduler: FIFO/RR ve runtime limiti` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `RT scheduler: FIFO/RR ve runtime limiti` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/task/rt_scheduler.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `RT scheduler: FIFO/RR ve runtime limiti` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `RT scheduler: FIFO/RR ve runtime limiti` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `RT scheduler: FIFO/RR ve runtime limiti` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `RT scheduler: FIFO/RR ve runtime limiti` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `RT scheduler: FIFO/RR ve runtime limiti` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `RT scheduler: FIFO/RR ve runtime limiti` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `RT scheduler: FIFO/RR ve runtime limiti` baglaminda `tick` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `RT scheduler: FIFO/RR ve runtime limiti` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `RT scheduler: FIFO/RR ve runtime limiti` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/task/rt_scheduler.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `RT scheduler: FIFO/RR ve runtime limiti` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `RT scheduler: FIFO/RR ve runtime limiti` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `RT scheduler: FIFO/RR ve runtime limiti` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `RT scheduler: FIFO/RR ve runtime limiti` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `RT scheduler: FIFO/RR ve runtime limiti` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `RT scheduler: FIFO/RR ve runtime limiti` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `RT scheduler: FIFO/RR ve runtime limiti` baglaminda `calculate_timeslice` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `RT scheduler: FIFO/RR ve runtime limiti` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `RT scheduler: FIFO/RR ve runtime limiti` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/task/rt_scheduler.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `RT scheduler: FIFO/RR ve runtime limiti` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `RT scheduler: FIFO/RR ve runtime limiti` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `RT scheduler: FIFO/RR ve runtime limiti` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `RT scheduler: FIFO/RR ve runtime limiti` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `RT scheduler: FIFO/RR ve runtime limiti` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `RT scheduler: FIFO/RR ve runtime limiti` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.


## Protokol 05 - CFS: vruntime adalet motoru


- Denetim maddesi: `CFS: vruntime adalet motoru` baglaminda `weight_to_vruntime` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `CFS: vruntime adalet motoru` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `CFS: vruntime adalet motoru` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/task/cfs.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `CFS: vruntime adalet motoru` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `CFS: vruntime adalet motoru` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `CFS: vruntime adalet motoru` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `CFS: vruntime adalet motoru` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `CFS: vruntime adalet motoru` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `CFS: vruntime adalet motoru` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `CFS: vruntime adalet motoru` baglaminda `pick_next` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `CFS: vruntime adalet motoru` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `CFS: vruntime adalet motoru` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/task/cfs.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `CFS: vruntime adalet motoru` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `CFS: vruntime adalet motoru` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `CFS: vruntime adalet motoru` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `CFS: vruntime adalet motoru` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `CFS: vruntime adalet motoru` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `CFS: vruntime adalet motoru` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `CFS: vruntime adalet motoru` baglaminda `weight_to_vruntime` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `CFS: vruntime adalet motoru` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `CFS: vruntime adalet motoru` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/task/cfs.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `CFS: vruntime adalet motoru` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `CFS: vruntime adalet motoru` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `CFS: vruntime adalet motoru` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `CFS: vruntime adalet motoru` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `CFS: vruntime adalet motoru` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `CFS: vruntime adalet motoru` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `CFS: vruntime adalet motoru` baglaminda `pick_next` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `CFS: vruntime adalet motoru` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `CFS: vruntime adalet motoru` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/task/cfs.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `CFS: vruntime adalet motoru` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `CFS: vruntime adalet motoru` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `CFS: vruntime adalet motoru` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `CFS: vruntime adalet motoru` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `CFS: vruntime adalet motoru` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `CFS: vruntime adalet motoru` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `CFS: vruntime adalet motoru` baglaminda `weight_to_vruntime` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `CFS: vruntime adalet motoru` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `CFS: vruntime adalet motoru` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/task/cfs.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `CFS: vruntime adalet motoru` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `CFS: vruntime adalet motoru` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `CFS: vruntime adalet motoru` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `CFS: vruntime adalet motoru` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `CFS: vruntime adalet motoru` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `CFS: vruntime adalet motoru` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.


## Protokol 06 - EEVDF: eligible_vtime ve virtual deadline


- Denetim maddesi: `EEVDF: eligible_vtime ve virtual deadline` baglaminda `update_runtime` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `EEVDF: eligible_vtime ve virtual deadline` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `EEVDF: eligible_vtime ve virtual deadline` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/task/eevdf.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `EEVDF: eligible_vtime ve virtual deadline` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `EEVDF: eligible_vtime ve virtual deadline` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `EEVDF: eligible_vtime ve virtual deadline` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `EEVDF: eligible_vtime ve virtual deadline` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `EEVDF: eligible_vtime ve virtual deadline` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `EEVDF: eligible_vtime ve virtual deadline` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `EEVDF: eligible_vtime ve virtual deadline` baglaminda `should_preempt` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `EEVDF: eligible_vtime ve virtual deadline` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `EEVDF: eligible_vtime ve virtual deadline` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/task/eevdf.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `EEVDF: eligible_vtime ve virtual deadline` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `EEVDF: eligible_vtime ve virtual deadline` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `EEVDF: eligible_vtime ve virtual deadline` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `EEVDF: eligible_vtime ve virtual deadline` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `EEVDF: eligible_vtime ve virtual deadline` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `EEVDF: eligible_vtime ve virtual deadline` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `EEVDF: eligible_vtime ve virtual deadline` baglaminda `update_runtime` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `EEVDF: eligible_vtime ve virtual deadline` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `EEVDF: eligible_vtime ve virtual deadline` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/task/eevdf.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `EEVDF: eligible_vtime ve virtual deadline` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `EEVDF: eligible_vtime ve virtual deadline` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `EEVDF: eligible_vtime ve virtual deadline` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `EEVDF: eligible_vtime ve virtual deadline` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `EEVDF: eligible_vtime ve virtual deadline` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `EEVDF: eligible_vtime ve virtual deadline` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `EEVDF: eligible_vtime ve virtual deadline` baglaminda `should_preempt` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `EEVDF: eligible_vtime ve virtual deadline` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `EEVDF: eligible_vtime ve virtual deadline` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/task/eevdf.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `EEVDF: eligible_vtime ve virtual deadline` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `EEVDF: eligible_vtime ve virtual deadline` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `EEVDF: eligible_vtime ve virtual deadline` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `EEVDF: eligible_vtime ve virtual deadline` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `EEVDF: eligible_vtime ve virtual deadline` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `EEVDF: eligible_vtime ve virtual deadline` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `EEVDF: eligible_vtime ve virtual deadline` baglaminda `update_runtime` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `EEVDF: eligible_vtime ve virtual deadline` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `EEVDF: eligible_vtime ve virtual deadline` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/task/eevdf.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `EEVDF: eligible_vtime ve virtual deadline` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `EEVDF: eligible_vtime ve virtual deadline` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `EEVDF: eligible_vtime ve virtual deadline` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `EEVDF: eligible_vtime ve virtual deadline` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `EEVDF: eligible_vtime ve virtual deadline` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `EEVDF: eligible_vtime ve virtual deadline` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.


## Protokol 07 - Deadline scheduler: EDF/CBS admission ve replenish


- Denetim maddesi: `Deadline scheduler: EDF/CBS admission ve replenish` baglaminda `compute_bandwidth` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `Deadline scheduler: EDF/CBS admission ve replenish` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `Deadline scheduler: EDF/CBS admission ve replenish` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/task/deadline.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `Deadline scheduler: EDF/CBS admission ve replenish` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `Deadline scheduler: EDF/CBS admission ve replenish` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `Deadline scheduler: EDF/CBS admission ve replenish` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `Deadline scheduler: EDF/CBS admission ve replenish` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `Deadline scheduler: EDF/CBS admission ve replenish` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `Deadline scheduler: EDF/CBS admission ve replenish` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `Deadline scheduler: EDF/CBS admission ve replenish` baglaminda `consume_runtime` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `Deadline scheduler: EDF/CBS admission ve replenish` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `Deadline scheduler: EDF/CBS admission ve replenish` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/task/deadline.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `Deadline scheduler: EDF/CBS admission ve replenish` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `Deadline scheduler: EDF/CBS admission ve replenish` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `Deadline scheduler: EDF/CBS admission ve replenish` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `Deadline scheduler: EDF/CBS admission ve replenish` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `Deadline scheduler: EDF/CBS admission ve replenish` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `Deadline scheduler: EDF/CBS admission ve replenish` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `Deadline scheduler: EDF/CBS admission ve replenish` baglaminda `compute_bandwidth` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `Deadline scheduler: EDF/CBS admission ve replenish` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `Deadline scheduler: EDF/CBS admission ve replenish` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/task/deadline.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `Deadline scheduler: EDF/CBS admission ve replenish` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `Deadline scheduler: EDF/CBS admission ve replenish` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `Deadline scheduler: EDF/CBS admission ve replenish` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `Deadline scheduler: EDF/CBS admission ve replenish` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `Deadline scheduler: EDF/CBS admission ve replenish` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `Deadline scheduler: EDF/CBS admission ve replenish` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `Deadline scheduler: EDF/CBS admission ve replenish` baglaminda `consume_runtime` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `Deadline scheduler: EDF/CBS admission ve replenish` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `Deadline scheduler: EDF/CBS admission ve replenish` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/task/deadline.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `Deadline scheduler: EDF/CBS admission ve replenish` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `Deadline scheduler: EDF/CBS admission ve replenish` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `Deadline scheduler: EDF/CBS admission ve replenish` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `Deadline scheduler: EDF/CBS admission ve replenish` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `Deadline scheduler: EDF/CBS admission ve replenish` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `Deadline scheduler: EDF/CBS admission ve replenish` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `Deadline scheduler: EDF/CBS admission ve replenish` baglaminda `compute_bandwidth` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `Deadline scheduler: EDF/CBS admission ve replenish` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `Deadline scheduler: EDF/CBS admission ve replenish` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/task/deadline.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `Deadline scheduler: EDF/CBS admission ve replenish` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `Deadline scheduler: EDF/CBS admission ve replenish` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `Deadline scheduler: EDF/CBS admission ve replenish` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `Deadline scheduler: EDF/CBS admission ve replenish` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `Deadline scheduler: EDF/CBS admission ve replenish` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `Deadline scheduler: EDF/CBS admission ve replenish` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.


## Protokol 08 - Chase-Lev deque: lock-free race analizi


- Denetim maddesi: `Chase-Lev deque: lock-free race analizi` baglaminda `push` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `Chase-Lev deque: lock-free race analizi` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `Chase-Lev deque: lock-free race analizi` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/task/deque.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `Chase-Lev deque: lock-free race analizi` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `Chase-Lev deque: lock-free race analizi` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `Chase-Lev deque: lock-free race analizi` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `Chase-Lev deque: lock-free race analizi` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `Chase-Lev deque: lock-free race analizi` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `Chase-Lev deque: lock-free race analizi` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `Chase-Lev deque: lock-free race analizi` baglaminda `steal` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `Chase-Lev deque: lock-free race analizi` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `Chase-Lev deque: lock-free race analizi` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/task/deque.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `Chase-Lev deque: lock-free race analizi` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `Chase-Lev deque: lock-free race analizi` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `Chase-Lev deque: lock-free race analizi` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `Chase-Lev deque: lock-free race analizi` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `Chase-Lev deque: lock-free race analizi` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `Chase-Lev deque: lock-free race analizi` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `Chase-Lev deque: lock-free race analizi` baglaminda `push` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `Chase-Lev deque: lock-free race analizi` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `Chase-Lev deque: lock-free race analizi` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/task/deque.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `Chase-Lev deque: lock-free race analizi` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `Chase-Lev deque: lock-free race analizi` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `Chase-Lev deque: lock-free race analizi` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `Chase-Lev deque: lock-free race analizi` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `Chase-Lev deque: lock-free race analizi` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `Chase-Lev deque: lock-free race analizi` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `Chase-Lev deque: lock-free race analizi` baglaminda `steal` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `Chase-Lev deque: lock-free race analizi` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `Chase-Lev deque: lock-free race analizi` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/task/deque.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `Chase-Lev deque: lock-free race analizi` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `Chase-Lev deque: lock-free race analizi` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `Chase-Lev deque: lock-free race analizi` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `Chase-Lev deque: lock-free race analizi` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `Chase-Lev deque: lock-free race analizi` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `Chase-Lev deque: lock-free race analizi` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `Chase-Lev deque: lock-free race analizi` baglaminda `push` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `Chase-Lev deque: lock-free race analizi` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `Chase-Lev deque: lock-free race analizi` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/task/deque.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `Chase-Lev deque: lock-free race analizi` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `Chase-Lev deque: lock-free race analizi` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `Chase-Lev deque: lock-free race analizi` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `Chase-Lev deque: lock-free race analizi` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `Chase-Lev deque: lock-free race analizi` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `Chase-Lev deque: lock-free race analizi` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.


## Protokol 09 - Hiyerarsik timing wheel


- Denetim maddesi: `Hiyerarsik timing wheel` baglaminda `schedule` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `Hiyerarsik timing wheel` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `Hiyerarsik timing wheel` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/task/timer.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `Hiyerarsik timing wheel` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `Hiyerarsik timing wheel` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `Hiyerarsik timing wheel` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `Hiyerarsik timing wheel` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `Hiyerarsik timing wheel` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `Hiyerarsik timing wheel` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `Hiyerarsik timing wheel` baglaminda `cascade` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `Hiyerarsik timing wheel` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `Hiyerarsik timing wheel` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/task/timer.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `Hiyerarsik timing wheel` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `Hiyerarsik timing wheel` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `Hiyerarsik timing wheel` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `Hiyerarsik timing wheel` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `Hiyerarsik timing wheel` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `Hiyerarsik timing wheel` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `Hiyerarsik timing wheel` baglaminda `schedule` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `Hiyerarsik timing wheel` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `Hiyerarsik timing wheel` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/task/timer.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `Hiyerarsik timing wheel` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `Hiyerarsik timing wheel` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `Hiyerarsik timing wheel` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `Hiyerarsik timing wheel` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `Hiyerarsik timing wheel` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `Hiyerarsik timing wheel` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `Hiyerarsik timing wheel` baglaminda `cascade` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `Hiyerarsik timing wheel` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `Hiyerarsik timing wheel` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/task/timer.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `Hiyerarsik timing wheel` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `Hiyerarsik timing wheel` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `Hiyerarsik timing wheel` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `Hiyerarsik timing wheel` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `Hiyerarsik timing wheel` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `Hiyerarsik timing wheel` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `Hiyerarsik timing wheel` baglaminda `schedule` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `Hiyerarsik timing wheel` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `Hiyerarsik timing wheel` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/task/timer.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `Hiyerarsik timing wheel` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `Hiyerarsik timing wheel` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `Hiyerarsik timing wheel` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `Hiyerarsik timing wheel` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `Hiyerarsik timing wheel` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `Hiyerarsik timing wheel` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.


## Protokol 10 - Zone-aware PMM fallback mimarisi


- Denetim maddesi: `Zone-aware PMM fallback mimarisi` baglaminda `fallback` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `Zone-aware PMM fallback mimarisi` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `Zone-aware PMM fallback mimarisi` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/memory/fibonacci_pmm.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `Zone-aware PMM fallback mimarisi` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `Zone-aware PMM fallback mimarisi` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `Zone-aware PMM fallback mimarisi` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `Zone-aware PMM fallback mimarisi` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `Zone-aware PMM fallback mimarisi` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `Zone-aware PMM fallback mimarisi` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `Zone-aware PMM fallback mimarisi` baglaminda `allocate_contiguous_from_zone` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `Zone-aware PMM fallback mimarisi` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `Zone-aware PMM fallback mimarisi` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/memory/fibonacci_pmm.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `Zone-aware PMM fallback mimarisi` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `Zone-aware PMM fallback mimarisi` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `Zone-aware PMM fallback mimarisi` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `Zone-aware PMM fallback mimarisi` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `Zone-aware PMM fallback mimarisi` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `Zone-aware PMM fallback mimarisi` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `Zone-aware PMM fallback mimarisi` baglaminda `fallback` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `Zone-aware PMM fallback mimarisi` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `Zone-aware PMM fallback mimarisi` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/memory/fibonacci_pmm.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `Zone-aware PMM fallback mimarisi` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `Zone-aware PMM fallback mimarisi` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `Zone-aware PMM fallback mimarisi` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `Zone-aware PMM fallback mimarisi` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `Zone-aware PMM fallback mimarisi` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `Zone-aware PMM fallback mimarisi` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `Zone-aware PMM fallback mimarisi` baglaminda `allocate_contiguous_from_zone` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `Zone-aware PMM fallback mimarisi` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `Zone-aware PMM fallback mimarisi` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/memory/fibonacci_pmm.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `Zone-aware PMM fallback mimarisi` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `Zone-aware PMM fallback mimarisi` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `Zone-aware PMM fallback mimarisi` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `Zone-aware PMM fallback mimarisi` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `Zone-aware PMM fallback mimarisi` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `Zone-aware PMM fallback mimarisi` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `Zone-aware PMM fallback mimarisi` baglaminda `fallback` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `Zone-aware PMM fallback mimarisi` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `Zone-aware PMM fallback mimarisi` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/memory/fibonacci_pmm.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `Zone-aware PMM fallback mimarisi` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `Zone-aware PMM fallback mimarisi` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `Zone-aware PMM fallback mimarisi` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `Zone-aware PMM fallback mimarisi` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `Zone-aware PMM fallback mimarisi` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `Zone-aware PMM fallback mimarisi` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.


## Protokol 11 - Fibonacci buddy split/coalesce


- Denetim maddesi: `Fibonacci buddy split/coalesce` baglaminda `split_block` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `Fibonacci buddy split/coalesce` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `Fibonacci buddy split/coalesce` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/memory/fibonacci_buddy.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `Fibonacci buddy split/coalesce` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `Fibonacci buddy split/coalesce` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `Fibonacci buddy split/coalesce` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `Fibonacci buddy split/coalesce` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `Fibonacci buddy split/coalesce` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `Fibonacci buddy split/coalesce` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `Fibonacci buddy split/coalesce` baglaminda `find_buddy` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `Fibonacci buddy split/coalesce` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `Fibonacci buddy split/coalesce` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/memory/fibonacci_buddy.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `Fibonacci buddy split/coalesce` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `Fibonacci buddy split/coalesce` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `Fibonacci buddy split/coalesce` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `Fibonacci buddy split/coalesce` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `Fibonacci buddy split/coalesce` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `Fibonacci buddy split/coalesce` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `Fibonacci buddy split/coalesce` baglaminda `split_block` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `Fibonacci buddy split/coalesce` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `Fibonacci buddy split/coalesce` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/memory/fibonacci_buddy.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `Fibonacci buddy split/coalesce` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `Fibonacci buddy split/coalesce` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `Fibonacci buddy split/coalesce` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `Fibonacci buddy split/coalesce` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `Fibonacci buddy split/coalesce` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `Fibonacci buddy split/coalesce` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `Fibonacci buddy split/coalesce` baglaminda `find_buddy` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `Fibonacci buddy split/coalesce` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `Fibonacci buddy split/coalesce` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/memory/fibonacci_buddy.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `Fibonacci buddy split/coalesce` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `Fibonacci buddy split/coalesce` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `Fibonacci buddy split/coalesce` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `Fibonacci buddy split/coalesce` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `Fibonacci buddy split/coalesce` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `Fibonacci buddy split/coalesce` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `Fibonacci buddy split/coalesce` baglaminda `split_block` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `Fibonacci buddy split/coalesce` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `Fibonacci buddy split/coalesce` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/memory/fibonacci_buddy.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `Fibonacci buddy split/coalesce` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `Fibonacci buddy split/coalesce` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `Fibonacci buddy split/coalesce` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `Fibonacci buddy split/coalesce` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `Fibonacci buddy split/coalesce` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `Fibonacci buddy split/coalesce` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.


## Protokol 12 - TLSF heap wrapper guvenligi


- Denetim maddesi: `TLSF heap wrapper guvenligi` baglaminda `insert_free_region_ptr` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `TLSF heap wrapper guvenligi` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `TLSF heap wrapper guvenligi` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/allocator/tlsf.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `TLSF heap wrapper guvenligi` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `TLSF heap wrapper guvenligi` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `TLSF heap wrapper guvenligi` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `TLSF heap wrapper guvenligi` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `TLSF heap wrapper guvenligi` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `TLSF heap wrapper guvenligi` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `TLSF heap wrapper guvenligi` baglaminda `dealloc_to_main_heap` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `TLSF heap wrapper guvenligi` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `TLSF heap wrapper guvenligi` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/allocator/tlsf.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `TLSF heap wrapper guvenligi` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `TLSF heap wrapper guvenligi` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `TLSF heap wrapper guvenligi` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `TLSF heap wrapper guvenligi` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `TLSF heap wrapper guvenligi` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `TLSF heap wrapper guvenligi` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `TLSF heap wrapper guvenligi` baglaminda `insert_free_region_ptr` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `TLSF heap wrapper guvenligi` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `TLSF heap wrapper guvenligi` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/allocator/tlsf.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `TLSF heap wrapper guvenligi` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `TLSF heap wrapper guvenligi` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `TLSF heap wrapper guvenligi` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `TLSF heap wrapper guvenligi` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `TLSF heap wrapper guvenligi` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `TLSF heap wrapper guvenligi` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `TLSF heap wrapper guvenligi` baglaminda `dealloc_to_main_heap` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `TLSF heap wrapper guvenligi` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `TLSF heap wrapper guvenligi` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/allocator/tlsf.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `TLSF heap wrapper guvenligi` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `TLSF heap wrapper guvenligi` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `TLSF heap wrapper guvenligi` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `TLSF heap wrapper guvenligi` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `TLSF heap wrapper guvenligi` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `TLSF heap wrapper guvenligi` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `TLSF heap wrapper guvenligi` baglaminda `insert_free_region_ptr` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `TLSF heap wrapper guvenligi` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `TLSF heap wrapper guvenligi` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/allocator/tlsf.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `TLSF heap wrapper guvenligi` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `TLSF heap wrapper guvenligi` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `TLSF heap wrapper guvenligi` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `TLSF heap wrapper guvenligi` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `TLSF heap wrapper guvenligi` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `TLSF heap wrapper guvenligi` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.


## Protokol 13 - User page fault, COW ve THP karari


- Denetim maddesi: `User page fault, COW ve THP karari` baglaminda `handle_user_page_fault` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `User page fault, COW ve THP karari` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `User page fault, COW ve THP karari` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/memory/mod.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `User page fault, COW ve THP karari` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `User page fault, COW ve THP karari` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `User page fault, COW ve THP karari` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `User page fault, COW ve THP karari` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `User page fault, COW ve THP karari` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `User page fault, COW ve THP karari` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `User page fault, COW ve THP karari` baglaminda `try_map_thp_anon` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `User page fault, COW ve THP karari` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `User page fault, COW ve THP karari` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/memory/mod.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `User page fault, COW ve THP karari` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `User page fault, COW ve THP karari` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `User page fault, COW ve THP karari` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `User page fault, COW ve THP karari` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `User page fault, COW ve THP karari` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `User page fault, COW ve THP karari` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `User page fault, COW ve THP karari` baglaminda `handle_user_page_fault` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `User page fault, COW ve THP karari` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `User page fault, COW ve THP karari` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/memory/mod.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `User page fault, COW ve THP karari` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `User page fault, COW ve THP karari` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `User page fault, COW ve THP karari` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `User page fault, COW ve THP karari` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `User page fault, COW ve THP karari` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `User page fault, COW ve THP karari` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `User page fault, COW ve THP karari` baglaminda `try_map_thp_anon` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `User page fault, COW ve THP karari` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `User page fault, COW ve THP karari` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/memory/mod.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `User page fault, COW ve THP karari` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `User page fault, COW ve THP karari` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `User page fault, COW ve THP karari` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `User page fault, COW ve THP karari` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `User page fault, COW ve THP karari` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `User page fault, COW ve THP karari` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `User page fault, COW ve THP karari` baglaminda `handle_user_page_fault` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `User page fault, COW ve THP karari` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `User page fault, COW ve THP karari` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/memory/mod.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `User page fault, COW ve THP karari` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `User page fault, COW ve THP karari` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `User page fault, COW ve THP karari` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `User page fault, COW ve THP karari` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `User page fault, COW ve THP karari` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `User page fault, COW ve THP karari` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.


## Protokol 14 - Reclaim daemon, writeback budget ve pressure


- Denetim maddesi: `Reclaim daemon, writeback budget ve pressure` baglaminda `memory_reclaim_daemon` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `Reclaim daemon, writeback budget ve pressure` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `Reclaim daemon, writeback budget ve pressure` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/memory/mod.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `Reclaim daemon, writeback budget ve pressure` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `Reclaim daemon, writeback budget ve pressure` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `Reclaim daemon, writeback budget ve pressure` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `Reclaim daemon, writeback budget ve pressure` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `Reclaim daemon, writeback budget ve pressure` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `Reclaim daemon, writeback budget ve pressure` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `Reclaim daemon, writeback budget ve pressure` baglaminda `process_writeback_budget` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `Reclaim daemon, writeback budget ve pressure` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `Reclaim daemon, writeback budget ve pressure` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/memory/mod.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `Reclaim daemon, writeback budget ve pressure` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `Reclaim daemon, writeback budget ve pressure` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `Reclaim daemon, writeback budget ve pressure` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `Reclaim daemon, writeback budget ve pressure` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `Reclaim daemon, writeback budget ve pressure` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `Reclaim daemon, writeback budget ve pressure` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `Reclaim daemon, writeback budget ve pressure` baglaminda `memory_reclaim_daemon` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `Reclaim daemon, writeback budget ve pressure` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `Reclaim daemon, writeback budget ve pressure` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/memory/mod.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `Reclaim daemon, writeback budget ve pressure` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `Reclaim daemon, writeback budget ve pressure` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `Reclaim daemon, writeback budget ve pressure` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `Reclaim daemon, writeback budget ve pressure` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `Reclaim daemon, writeback budget ve pressure` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `Reclaim daemon, writeback budget ve pressure` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `Reclaim daemon, writeback budget ve pressure` baglaminda `process_writeback_budget` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `Reclaim daemon, writeback budget ve pressure` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `Reclaim daemon, writeback budget ve pressure` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/memory/mod.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `Reclaim daemon, writeback budget ve pressure` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `Reclaim daemon, writeback budget ve pressure` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `Reclaim daemon, writeback budget ve pressure` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `Reclaim daemon, writeback budget ve pressure` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `Reclaim daemon, writeback budget ve pressure` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `Reclaim daemon, writeback budget ve pressure` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `Reclaim daemon, writeback budget ve pressure` baglaminda `memory_reclaim_daemon` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `Reclaim daemon, writeback budget ve pressure` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `Reclaim daemon, writeback budget ve pressure` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/memory/mod.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `Reclaim daemon, writeback budget ve pressure` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `Reclaim daemon, writeback budget ve pressure` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `Reclaim daemon, writeback budget ve pressure` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `Reclaim daemon, writeback budget ve pressure` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `Reclaim daemon, writeback budget ve pressure` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `Reclaim daemon, writeback budget ve pressure` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.


## Protokol 15 - MGLRU generation ve victim secimi


- Denetim maddesi: `MGLRU generation ve victim secimi` baglaminda `on_access` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `MGLRU generation ve victim secimi` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `MGLRU generation ve victim secimi` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/memory/mglru.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `MGLRU generation ve victim secimi` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `MGLRU generation ve victim secimi` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `MGLRU generation ve victim secimi` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `MGLRU generation ve victim secimi` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `MGLRU generation ve victim secimi` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `MGLRU generation ve victim secimi` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `MGLRU generation ve victim secimi` baglaminda `pick_victim` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `MGLRU generation ve victim secimi` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `MGLRU generation ve victim secimi` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/memory/mglru.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `MGLRU generation ve victim secimi` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `MGLRU generation ve victim secimi` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `MGLRU generation ve victim secimi` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `MGLRU generation ve victim secimi` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `MGLRU generation ve victim secimi` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `MGLRU generation ve victim secimi` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `MGLRU generation ve victim secimi` baglaminda `on_access` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `MGLRU generation ve victim secimi` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `MGLRU generation ve victim secimi` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/memory/mglru.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `MGLRU generation ve victim secimi` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `MGLRU generation ve victim secimi` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `MGLRU generation ve victim secimi` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `MGLRU generation ve victim secimi` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `MGLRU generation ve victim secimi` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `MGLRU generation ve victim secimi` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `MGLRU generation ve victim secimi` baglaminda `pick_victim` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `MGLRU generation ve victim secimi` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `MGLRU generation ve victim secimi` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/memory/mglru.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `MGLRU generation ve victim secimi` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `MGLRU generation ve victim secimi` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `MGLRU generation ve victim secimi` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `MGLRU generation ve victim secimi` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `MGLRU generation ve victim secimi` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `MGLRU generation ve victim secimi` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `MGLRU generation ve victim secimi` baglaminda `on_access` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `MGLRU generation ve victim secimi` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `MGLRU generation ve victim secimi` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/memory/mglru.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `MGLRU generation ve victim secimi` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `MGLRU generation ve victim secimi` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `MGLRU generation ve victim secimi` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `MGLRU generation ve victim secimi` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `MGLRU generation ve victim secimi` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `MGLRU generation ve victim secimi` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.


## Protokol 16 - ZSwap compression pipeline


- Denetim maddesi: `ZSwap compression pipeline` baglaminda `compress` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `ZSwap compression pipeline` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `ZSwap compression pipeline` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/memory/zswap.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `ZSwap compression pipeline` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `ZSwap compression pipeline` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `ZSwap compression pipeline` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `ZSwap compression pipeline` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `ZSwap compression pipeline` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `ZSwap compression pipeline` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `ZSwap compression pipeline` baglaminda `ZSWAP_DEFAULT_POOL_PERCENT` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `ZSwap compression pipeline` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `ZSwap compression pipeline` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/memory/zswap.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `ZSwap compression pipeline` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `ZSwap compression pipeline` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `ZSwap compression pipeline` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `ZSwap compression pipeline` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `ZSwap compression pipeline` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `ZSwap compression pipeline` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `ZSwap compression pipeline` baglaminda `compress` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `ZSwap compression pipeline` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `ZSwap compression pipeline` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/memory/zswap.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `ZSwap compression pipeline` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `ZSwap compression pipeline` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `ZSwap compression pipeline` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `ZSwap compression pipeline` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `ZSwap compression pipeline` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `ZSwap compression pipeline` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `ZSwap compression pipeline` baglaminda `ZSWAP_DEFAULT_POOL_PERCENT` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `ZSwap compression pipeline` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `ZSwap compression pipeline` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/memory/zswap.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `ZSwap compression pipeline` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `ZSwap compression pipeline` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `ZSwap compression pipeline` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `ZSwap compression pipeline` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `ZSwap compression pipeline` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `ZSwap compression pipeline` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `ZSwap compression pipeline` baglaminda `compress` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `ZSwap compression pipeline` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `ZSwap compression pipeline` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/memory/zswap.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `ZSwap compression pipeline` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `ZSwap compression pipeline` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `ZSwap compression pipeline` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `ZSwap compression pipeline` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `ZSwap compression pipeline` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `ZSwap compression pipeline` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.


## Protokol 17 - Lock-free io_uring publication boundaries


- Denetim maddesi: `Lock-free io_uring publication boundaries` baglaminda `push` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `Lock-free io_uring publication boundaries` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `Lock-free io_uring publication boundaries` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/posix/io_uring_ring.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `Lock-free io_uring publication boundaries` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `Lock-free io_uring publication boundaries` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `Lock-free io_uring publication boundaries` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `Lock-free io_uring publication boundaries` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `Lock-free io_uring publication boundaries` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `Lock-free io_uring publication boundaries` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `Lock-free io_uring publication boundaries` baglaminda `pop_batch` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `Lock-free io_uring publication boundaries` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `Lock-free io_uring publication boundaries` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/posix/io_uring_ring.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `Lock-free io_uring publication boundaries` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `Lock-free io_uring publication boundaries` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `Lock-free io_uring publication boundaries` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `Lock-free io_uring publication boundaries` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `Lock-free io_uring publication boundaries` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `Lock-free io_uring publication boundaries` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `Lock-free io_uring publication boundaries` baglaminda `push` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `Lock-free io_uring publication boundaries` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `Lock-free io_uring publication boundaries` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/posix/io_uring_ring.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `Lock-free io_uring publication boundaries` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `Lock-free io_uring publication boundaries` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `Lock-free io_uring publication boundaries` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `Lock-free io_uring publication boundaries` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `Lock-free io_uring publication boundaries` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `Lock-free io_uring publication boundaries` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `Lock-free io_uring publication boundaries` baglaminda `pop_batch` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `Lock-free io_uring publication boundaries` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `Lock-free io_uring publication boundaries` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/posix/io_uring_ring.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `Lock-free io_uring publication boundaries` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `Lock-free io_uring publication boundaries` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `Lock-free io_uring publication boundaries` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `Lock-free io_uring publication boundaries` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `Lock-free io_uring publication boundaries` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `Lock-free io_uring publication boundaries` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `Lock-free io_uring publication boundaries` baglaminda `push` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `Lock-free io_uring publication boundaries` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `Lock-free io_uring publication boundaries` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/posix/io_uring_ring.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `Lock-free io_uring publication boundaries` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `Lock-free io_uring publication boundaries` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `Lock-free io_uring publication boundaries` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `Lock-free io_uring publication boundaries` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `Lock-free io_uring publication boundaries` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `Lock-free io_uring publication boundaries` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.


## Protokol 18 - TLS 1.3 handshake ve key schedule


- Denetim maddesi: `TLS 1.3 handshake ve key schedule` baglaminda `derive_handshake_secret` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `TLS 1.3 handshake ve key schedule` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `TLS 1.3 handshake ve key schedule` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/net/tls.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `TLS 1.3 handshake ve key schedule` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `TLS 1.3 handshake ve key schedule` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `TLS 1.3 handshake ve key schedule` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `TLS 1.3 handshake ve key schedule` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `TLS 1.3 handshake ve key schedule` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `TLS 1.3 handshake ve key schedule` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `TLS 1.3 handshake ve key schedule` baglaminda `hkdf_expand_label` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `TLS 1.3 handshake ve key schedule` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `TLS 1.3 handshake ve key schedule` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/net/tls.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `TLS 1.3 handshake ve key schedule` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `TLS 1.3 handshake ve key schedule` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `TLS 1.3 handshake ve key schedule` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `TLS 1.3 handshake ve key schedule` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `TLS 1.3 handshake ve key schedule` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `TLS 1.3 handshake ve key schedule` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `TLS 1.3 handshake ve key schedule` baglaminda `derive_handshake_secret` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `TLS 1.3 handshake ve key schedule` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `TLS 1.3 handshake ve key schedule` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/net/tls.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `TLS 1.3 handshake ve key schedule` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `TLS 1.3 handshake ve key schedule` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `TLS 1.3 handshake ve key schedule` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `TLS 1.3 handshake ve key schedule` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `TLS 1.3 handshake ve key schedule` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `TLS 1.3 handshake ve key schedule` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `TLS 1.3 handshake ve key schedule` baglaminda `hkdf_expand_label` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `TLS 1.3 handshake ve key schedule` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `TLS 1.3 handshake ve key schedule` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/net/tls.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `TLS 1.3 handshake ve key schedule` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `TLS 1.3 handshake ve key schedule` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `TLS 1.3 handshake ve key schedule` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `TLS 1.3 handshake ve key schedule` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `TLS 1.3 handshake ve key schedule` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `TLS 1.3 handshake ve key schedule` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `TLS 1.3 handshake ve key schedule` baglaminda `derive_handshake_secret` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `TLS 1.3 handshake ve key schedule` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `TLS 1.3 handshake ve key schedule` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/net/tls.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `TLS 1.3 handshake ve key schedule` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `TLS 1.3 handshake ve key schedule` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `TLS 1.3 handshake ve key schedule` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `TLS 1.3 handshake ve key schedule` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `TLS 1.3 handshake ve key schedule` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `TLS 1.3 handshake ve key schedule` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.


## Protokol 19 - QUIC frame parser ve ACK guard


- Denetim maddesi: `QUIC frame parser ve ACK guard` baglaminda `encode_varint` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `QUIC frame parser ve ACK guard` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `QUIC frame parser ve ACK guard` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/net/quic.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `QUIC frame parser ve ACK guard` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `QUIC frame parser ve ACK guard` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `QUIC frame parser ve ACK guard` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `QUIC frame parser ve ACK guard` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `QUIC frame parser ve ACK guard` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `QUIC frame parser ve ACK guard` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `QUIC frame parser ve ACK guard` baglaminda `decode` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `QUIC frame parser ve ACK guard` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `QUIC frame parser ve ACK guard` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/net/quic.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `QUIC frame parser ve ACK guard` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `QUIC frame parser ve ACK guard` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `QUIC frame parser ve ACK guard` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `QUIC frame parser ve ACK guard` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `QUIC frame parser ve ACK guard` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `QUIC frame parser ve ACK guard` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `QUIC frame parser ve ACK guard` baglaminda `encode_varint` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `QUIC frame parser ve ACK guard` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `QUIC frame parser ve ACK guard` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/net/quic.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `QUIC frame parser ve ACK guard` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `QUIC frame parser ve ACK guard` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `QUIC frame parser ve ACK guard` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `QUIC frame parser ve ACK guard` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `QUIC frame parser ve ACK guard` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `QUIC frame parser ve ACK guard` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `QUIC frame parser ve ACK guard` baglaminda `decode` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `QUIC frame parser ve ACK guard` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `QUIC frame parser ve ACK guard` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/net/quic.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `QUIC frame parser ve ACK guard` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `QUIC frame parser ve ACK guard` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `QUIC frame parser ve ACK guard` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `QUIC frame parser ve ACK guard` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `QUIC frame parser ve ACK guard` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `QUIC frame parser ve ACK guard` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `QUIC frame parser ve ACK guard` baglaminda `encode_varint` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `QUIC frame parser ve ACK guard` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `QUIC frame parser ve ACK guard` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/net/quic.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `QUIC frame parser ve ACK guard` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `QUIC frame parser ve ACK guard` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `QUIC frame parser ve ACK guard` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `QUIC frame parser ve ACK guard` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `QUIC frame parser ve ACK guard` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `QUIC frame parser ve ACK guard` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.


## Protokol 20 - WireGuard handshake, nonce ve replay koruma


- Denetim maddesi: `WireGuard handshake, nonce ve replay koruma` baglaminda `initiate_handshake` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `WireGuard handshake, nonce ve replay koruma` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `WireGuard handshake, nonce ve replay koruma` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/net/wireguard.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `WireGuard handshake, nonce ve replay koruma` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `WireGuard handshake, nonce ve replay koruma` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `WireGuard handshake, nonce ve replay koruma` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `WireGuard handshake, nonce ve replay koruma` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `WireGuard handshake, nonce ve replay koruma` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `WireGuard handshake, nonce ve replay koruma` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `WireGuard handshake, nonce ve replay koruma` baglaminda `decrypt_packet` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `WireGuard handshake, nonce ve replay koruma` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `WireGuard handshake, nonce ve replay koruma` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/net/wireguard.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `WireGuard handshake, nonce ve replay koruma` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `WireGuard handshake, nonce ve replay koruma` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `WireGuard handshake, nonce ve replay koruma` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `WireGuard handshake, nonce ve replay koruma` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `WireGuard handshake, nonce ve replay koruma` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `WireGuard handshake, nonce ve replay koruma` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `WireGuard handshake, nonce ve replay koruma` baglaminda `initiate_handshake` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `WireGuard handshake, nonce ve replay koruma` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `WireGuard handshake, nonce ve replay koruma` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/net/wireguard.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `WireGuard handshake, nonce ve replay koruma` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `WireGuard handshake, nonce ve replay koruma` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `WireGuard handshake, nonce ve replay koruma` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `WireGuard handshake, nonce ve replay koruma` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `WireGuard handshake, nonce ve replay koruma` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `WireGuard handshake, nonce ve replay koruma` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `WireGuard handshake, nonce ve replay koruma` baglaminda `decrypt_packet` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `WireGuard handshake, nonce ve replay koruma` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `WireGuard handshake, nonce ve replay koruma` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/net/wireguard.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `WireGuard handshake, nonce ve replay koruma` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `WireGuard handshake, nonce ve replay koruma` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `WireGuard handshake, nonce ve replay koruma` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `WireGuard handshake, nonce ve replay koruma` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `WireGuard handshake, nonce ve replay koruma` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `WireGuard handshake, nonce ve replay koruma` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `WireGuard handshake, nonce ve replay koruma` baglaminda `initiate_handshake` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `WireGuard handshake, nonce ve replay koruma` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `WireGuard handshake, nonce ve replay koruma` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/net/wireguard.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `WireGuard handshake, nonce ve replay koruma` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `WireGuard handshake, nonce ve replay koruma` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `WireGuard handshake, nonce ve replay koruma` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `WireGuard handshake, nonce ve replay koruma` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `WireGuard handshake, nonce ve replay koruma` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `WireGuard handshake, nonce ve replay koruma` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.


## Protokol 21 - HPACK Huffman decode fail-closed modeli


- Denetim maddesi: `HPACK Huffman decode fail-closed modeli` baglaminda `decode_huffman` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `HPACK Huffman decode fail-closed modeli` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `HPACK Huffman decode fail-closed modeli` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/net/http2_huffman.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `HPACK Huffman decode fail-closed modeli` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `HPACK Huffman decode fail-closed modeli` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `HPACK Huffman decode fail-closed modeli` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `HPACK Huffman decode fail-closed modeli` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `HPACK Huffman decode fail-closed modeli` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `HPACK Huffman decode fail-closed modeli` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `HPACK Huffman decode fail-closed modeli` baglaminda `InvalidPadding` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `HPACK Huffman decode fail-closed modeli` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `HPACK Huffman decode fail-closed modeli` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/net/http2_huffman.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `HPACK Huffman decode fail-closed modeli` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `HPACK Huffman decode fail-closed modeli` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `HPACK Huffman decode fail-closed modeli` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `HPACK Huffman decode fail-closed modeli` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `HPACK Huffman decode fail-closed modeli` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `HPACK Huffman decode fail-closed modeli` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `HPACK Huffman decode fail-closed modeli` baglaminda `decode_huffman` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `HPACK Huffman decode fail-closed modeli` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `HPACK Huffman decode fail-closed modeli` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/net/http2_huffman.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `HPACK Huffman decode fail-closed modeli` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `HPACK Huffman decode fail-closed modeli` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `HPACK Huffman decode fail-closed modeli` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `HPACK Huffman decode fail-closed modeli` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `HPACK Huffman decode fail-closed modeli` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `HPACK Huffman decode fail-closed modeli` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `HPACK Huffman decode fail-closed modeli` baglaminda `InvalidPadding` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `HPACK Huffman decode fail-closed modeli` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `HPACK Huffman decode fail-closed modeli` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/net/http2_huffman.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `HPACK Huffman decode fail-closed modeli` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `HPACK Huffman decode fail-closed modeli` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `HPACK Huffman decode fail-closed modeli` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `HPACK Huffman decode fail-closed modeli` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `HPACK Huffman decode fail-closed modeli` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `HPACK Huffman decode fail-closed modeli` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
- Denetim maddesi: `HPACK Huffman decode fail-closed modeli` baglaminda `decode_huffman` fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir.
- Denetim maddesi: `HPACK Huffman decode fail-closed modeli` icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir.
- Denetim maddesi: `HPACK Huffman decode fail-closed modeli` alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir.
- Denetim maddesi: `src/net/http2_huffman.rs` dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir.
- Denetim maddesi: `HPACK Huffman decode fail-closed modeli` icin temel invariant ve ihlal semptomu beraber raporlanmalidir.
- Denetim maddesi: `HPACK Huffman decode fail-closed modeli` modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir.
- Denetim maddesi: `HPACK Huffman decode fail-closed modeli` icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir.
- Denetim maddesi: `HPACK Huffman decode fail-closed modeli` alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir.
- Denetim maddesi: `HPACK Huffman decode fail-closed modeli` ile bagli admission limiti nicel gerekceyle tanimlanmalidir.
- Denetim maddesi: `HPACK Huffman decode fail-closed modeli` kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir.
