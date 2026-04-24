# Cilt 1 Vaka ve Cozumler

Bu bolumde her topic icin operasyonel vaka setleri verilir. Amac, kodu gercek semptomla baglamak.

## Vaka Kumesi 01 - Boot, platform init ve erken dogruluk

### Vaka 001 - Boot, platform init ve erken dogruluk / senaryo 1

- Senaryo: `src/main.rs` icinde `init_platform_iommu` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Erken init sozlesmesi bozulursa semptom gec ve daginik gorulur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Fail-closed init, capability bazli acilis, adim bazli loglama.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 002 - Boot, platform init ve erken dogruluk / senaryo 2

- Senaryo: `src/main.rs` icinde `parse_swap_cmdline` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Erken init sozlesmesi bozulursa semptom gec ve daginik gorulur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Fail-closed init, capability bazli acilis, adim bazli loglama.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 003 - Boot, platform init ve erken dogruluk / senaryo 3

- Senaryo: `src/main.rs` icinde `serial_init` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Erken init sozlesmesi bozulursa semptom gec ve daginik gorulur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Fail-closed init, capability bazli acilis, adim bazli loglama.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 004 - Boot, platform init ve erken dogruluk / senaryo 4

- Senaryo: `src/main.rs` icinde `panic_handler` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Erken init sozlesmesi bozulursa semptom gec ve daginik gorulur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Fail-closed init, capability bazli acilis, adim bazli loglama.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 005 - Boot, platform init ve erken dogruluk / senaryo 5

- Senaryo: `src/main.rs` icinde `init_platform_iommu` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Erken init sozlesmesi bozulursa semptom gec ve daginik gorulur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Fail-closed init, capability bazli acilis, adim bazli loglama.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 006 - Boot, platform init ve erken dogruluk / senaryo 6

- Senaryo: `src/main.rs` icinde `parse_swap_cmdline` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Erken init sozlesmesi bozulursa semptom gec ve daginik gorulur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Fail-closed init, capability bazli acilis, adim bazli loglama.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 007 - Boot, platform init ve erken dogruluk / senaryo 7

- Senaryo: `src/main.rs` icinde `serial_init` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Erken init sozlesmesi bozulursa semptom gec ve daginik gorulur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Fail-closed init, capability bazli acilis, adim bazli loglama.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 008 - Boot, platform init ve erken dogruluk / senaryo 8

- Senaryo: `src/main.rs` icinde `panic_handler` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Erken init sozlesmesi bozulursa semptom gec ve daginik gorulur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Fail-closed init, capability bazli acilis, adim bazli loglama.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 009 - Boot, platform init ve erken dogruluk / senaryo 9

- Senaryo: `src/main.rs` icinde `init_platform_iommu` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Erken init sozlesmesi bozulursa semptom gec ve daginik gorulur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Fail-closed init, capability bazli acilis, adim bazli loglama.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 010 - Boot, platform init ve erken dogruluk / senaryo 10

- Senaryo: `src/main.rs` icinde `parse_swap_cmdline` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Erken init sozlesmesi bozulursa semptom gec ve daginik gorulur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Fail-closed init, capability bazli acilis, adim bazli loglama.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 011 - Boot, platform init ve erken dogruluk / senaryo 11

- Senaryo: `src/main.rs` icinde `serial_init` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Erken init sozlesmesi bozulursa semptom gec ve daginik gorulur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Fail-closed init, capability bazli acilis, adim bazli loglama.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 012 - Boot, platform init ve erken dogruluk / senaryo 12

- Senaryo: `src/main.rs` icinde `panic_handler` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Erken init sozlesmesi bozulursa semptom gec ve daginik gorulur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Fail-closed init, capability bazli acilis, adim bazli loglama.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 013 - Boot, platform init ve erken dogruluk / senaryo 13

- Senaryo: `src/main.rs` icinde `init_platform_iommu` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Erken init sozlesmesi bozulursa semptom gec ve daginik gorulur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Fail-closed init, capability bazli acilis, adim bazli loglama.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 014 - Boot, platform init ve erken dogruluk / senaryo 14

- Senaryo: `src/main.rs` icinde `parse_swap_cmdline` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Erken init sozlesmesi bozulursa semptom gec ve daginik gorulur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Fail-closed init, capability bazli acilis, adim bazli loglama.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 015 - Boot, platform init ve erken dogruluk / senaryo 15

- Senaryo: `src/main.rs` icinde `serial_init` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Erken init sozlesmesi bozulursa semptom gec ve daginik gorulur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Fail-closed init, capability bazli acilis, adim bazli loglama.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

---

## Vaka Kumesi 02 - Bootstrap frame allocator ve fiziksel aralik korumasi

### Vaka 016 - Bootstrap frame allocator ve fiziksel aralik korumasi / senaryo 1

- Senaryo: `src/memory/frame_allocator.rs` icinde `allocate_frame_internal` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Kernel image araligi korunmazsa self-corruption olusur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Kernel fiziksel araligi explicit hesaplanip tahsis disi tutulur.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 017 - Bootstrap frame allocator ve fiziksel aralik korumasi / senaryo 2

- Senaryo: `src/memory/frame_allocator.rs` icinde `allocate_contiguous` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Kernel image araligi korunmazsa self-corruption olusur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Kernel fiziksel araligi explicit hesaplanip tahsis disi tutulur.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 018 - Bootstrap frame allocator ve fiziksel aralik korumasi / senaryo 3

- Senaryo: `src/memory/frame_allocator.rs` icinde `overlaps_kernel` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Kernel image araligi korunmazsa self-corruption olusur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Kernel fiziksel araligi explicit hesaplanip tahsis disi tutulur.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 019 - Bootstrap frame allocator ve fiziksel aralik korumasi / senaryo 4

- Senaryo: `src/memory/frame_allocator.rs` icinde `kernel_phys_range` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Kernel image araligi korunmazsa self-corruption olusur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Kernel fiziksel araligi explicit hesaplanip tahsis disi tutulur.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 020 - Bootstrap frame allocator ve fiziksel aralik korumasi / senaryo 5

- Senaryo: `src/memory/frame_allocator.rs` icinde `allocate_frame_internal` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Kernel image araligi korunmazsa self-corruption olusur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Kernel fiziksel araligi explicit hesaplanip tahsis disi tutulur.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 021 - Bootstrap frame allocator ve fiziksel aralik korumasi / senaryo 6

- Senaryo: `src/memory/frame_allocator.rs` icinde `allocate_contiguous` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Kernel image araligi korunmazsa self-corruption olusur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Kernel fiziksel araligi explicit hesaplanip tahsis disi tutulur.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 022 - Bootstrap frame allocator ve fiziksel aralik korumasi / senaryo 7

- Senaryo: `src/memory/frame_allocator.rs` icinde `overlaps_kernel` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Kernel image araligi korunmazsa self-corruption olusur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Kernel fiziksel araligi explicit hesaplanip tahsis disi tutulur.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 023 - Bootstrap frame allocator ve fiziksel aralik korumasi / senaryo 8

- Senaryo: `src/memory/frame_allocator.rs` icinde `kernel_phys_range` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Kernel image araligi korunmazsa self-corruption olusur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Kernel fiziksel araligi explicit hesaplanip tahsis disi tutulur.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 024 - Bootstrap frame allocator ve fiziksel aralik korumasi / senaryo 9

- Senaryo: `src/memory/frame_allocator.rs` icinde `allocate_frame_internal` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Kernel image araligi korunmazsa self-corruption olusur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Kernel fiziksel araligi explicit hesaplanip tahsis disi tutulur.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 025 - Bootstrap frame allocator ve fiziksel aralik korumasi / senaryo 10

- Senaryo: `src/memory/frame_allocator.rs` icinde `allocate_contiguous` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Kernel image araligi korunmazsa self-corruption olusur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Kernel fiziksel araligi explicit hesaplanip tahsis disi tutulur.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 026 - Bootstrap frame allocator ve fiziksel aralik korumasi / senaryo 11

- Senaryo: `src/memory/frame_allocator.rs` icinde `overlaps_kernel` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Kernel image araligi korunmazsa self-corruption olusur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Kernel fiziksel araligi explicit hesaplanip tahsis disi tutulur.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 027 - Bootstrap frame allocator ve fiziksel aralik korumasi / senaryo 12

- Senaryo: `src/memory/frame_allocator.rs` icinde `kernel_phys_range` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Kernel image araligi korunmazsa self-corruption olusur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Kernel fiziksel araligi explicit hesaplanip tahsis disi tutulur.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 028 - Bootstrap frame allocator ve fiziksel aralik korumasi / senaryo 13

- Senaryo: `src/memory/frame_allocator.rs` icinde `allocate_frame_internal` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Kernel image araligi korunmazsa self-corruption olusur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Kernel fiziksel araligi explicit hesaplanip tahsis disi tutulur.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 029 - Bootstrap frame allocator ve fiziksel aralik korumasi / senaryo 14

- Senaryo: `src/memory/frame_allocator.rs` icinde `allocate_contiguous` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Kernel image araligi korunmazsa self-corruption olusur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Kernel fiziksel araligi explicit hesaplanip tahsis disi tutulur.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 030 - Bootstrap frame allocator ve fiziksel aralik korumasi / senaryo 15

- Senaryo: `src/memory/frame_allocator.rs` icinde `overlaps_kernel` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Kernel image araligi korunmazsa self-corruption olusur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Kernel fiziksel araligi explicit hesaplanip tahsis disi tutulur.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

---

## Vaka Kumesi 03 - SMP scheduler karar modeli

### Vaka 031 - SMP scheduler karar modeli / senaryo 1

- Senaryo: `src/task/scheduler.rs` icinde `choose_spawn_cpu` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Load skew artarsa tail latency patlar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Work stealing + queue telemetrisi + affinity filtreleri.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 032 - SMP scheduler karar modeli / senaryo 2

- Senaryo: `src/task/scheduler.rs` icinde `enqueue_boxed_task` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Load skew artarsa tail latency patlar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Work stealing + queue telemetrisi + affinity filtreleri.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 033 - SMP scheduler karar modeli / senaryo 3

- Senaryo: `src/task/scheduler.rs` icinde `publish_worker_load` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Load skew artarsa tail latency patlar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Work stealing + queue telemetrisi + affinity filtreleri.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 034 - SMP scheduler karar modeli / senaryo 4

- Senaryo: `src/task/scheduler.rs` icinde `update_cpu_count` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Load skew artarsa tail latency patlar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Work stealing + queue telemetrisi + affinity filtreleri.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 035 - SMP scheduler karar modeli / senaryo 5

- Senaryo: `src/task/scheduler.rs` icinde `choose_spawn_cpu` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Load skew artarsa tail latency patlar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Work stealing + queue telemetrisi + affinity filtreleri.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 036 - SMP scheduler karar modeli / senaryo 6

- Senaryo: `src/task/scheduler.rs` icinde `enqueue_boxed_task` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Load skew artarsa tail latency patlar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Work stealing + queue telemetrisi + affinity filtreleri.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 037 - SMP scheduler karar modeli / senaryo 7

- Senaryo: `src/task/scheduler.rs` icinde `publish_worker_load` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Load skew artarsa tail latency patlar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Work stealing + queue telemetrisi + affinity filtreleri.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 038 - SMP scheduler karar modeli / senaryo 8

- Senaryo: `src/task/scheduler.rs` icinde `update_cpu_count` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Load skew artarsa tail latency patlar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Work stealing + queue telemetrisi + affinity filtreleri.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 039 - SMP scheduler karar modeli / senaryo 9

- Senaryo: `src/task/scheduler.rs` icinde `choose_spawn_cpu` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Load skew artarsa tail latency patlar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Work stealing + queue telemetrisi + affinity filtreleri.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 040 - SMP scheduler karar modeli / senaryo 10

- Senaryo: `src/task/scheduler.rs` icinde `enqueue_boxed_task` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Load skew artarsa tail latency patlar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Work stealing + queue telemetrisi + affinity filtreleri.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 041 - SMP scheduler karar modeli / senaryo 11

- Senaryo: `src/task/scheduler.rs` icinde `publish_worker_load` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Load skew artarsa tail latency patlar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Work stealing + queue telemetrisi + affinity filtreleri.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 042 - SMP scheduler karar modeli / senaryo 12

- Senaryo: `src/task/scheduler.rs` icinde `update_cpu_count` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Load skew artarsa tail latency patlar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Work stealing + queue telemetrisi + affinity filtreleri.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 043 - SMP scheduler karar modeli / senaryo 13

- Senaryo: `src/task/scheduler.rs` icinde `choose_spawn_cpu` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Load skew artarsa tail latency patlar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Work stealing + queue telemetrisi + affinity filtreleri.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 044 - SMP scheduler karar modeli / senaryo 14

- Senaryo: `src/task/scheduler.rs` icinde `enqueue_boxed_task` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Load skew artarsa tail latency patlar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Work stealing + queue telemetrisi + affinity filtreleri.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 045 - SMP scheduler karar modeli / senaryo 15

- Senaryo: `src/task/scheduler.rs` icinde `publish_worker_load` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Load skew artarsa tail latency patlar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Work stealing + queue telemetrisi + affinity filtreleri.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

---

## Vaka Kumesi 04 - RT scheduler: FIFO/RR ve runtime limiti

### Vaka 046 - RT scheduler: FIFO/RR ve runtime limiti / senaryo 1

- Senaryo: `src/task/rt_scheduler.rs` icinde `calculate_timeslice` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis policy secimi starvation ve jitter uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: RR dilimi ve RT bandwidth governance.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 047 - RT scheduler: FIFO/RR ve runtime limiti / senaryo 2

- Senaryo: `src/task/rt_scheduler.rs` icinde `enqueue` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis policy secimi starvation ve jitter uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: RR dilimi ve RT bandwidth governance.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 048 - RT scheduler: FIFO/RR ve runtime limiti / senaryo 3

- Senaryo: `src/task/rt_scheduler.rs` icinde `tick` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis policy secimi starvation ve jitter uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: RR dilimi ve RT bandwidth governance.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 049 - RT scheduler: FIFO/RR ve runtime limiti / senaryo 4

- Senaryo: `src/task/rt_scheduler.rs` icinde `set_sched_param` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis policy secimi starvation ve jitter uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: RR dilimi ve RT bandwidth governance.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 050 - RT scheduler: FIFO/RR ve runtime limiti / senaryo 5

- Senaryo: `src/task/rt_scheduler.rs` icinde `calculate_timeslice` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis policy secimi starvation ve jitter uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: RR dilimi ve RT bandwidth governance.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 051 - RT scheduler: FIFO/RR ve runtime limiti / senaryo 6

- Senaryo: `src/task/rt_scheduler.rs` icinde `enqueue` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis policy secimi starvation ve jitter uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: RR dilimi ve RT bandwidth governance.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 052 - RT scheduler: FIFO/RR ve runtime limiti / senaryo 7

- Senaryo: `src/task/rt_scheduler.rs` icinde `tick` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis policy secimi starvation ve jitter uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: RR dilimi ve RT bandwidth governance.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 053 - RT scheduler: FIFO/RR ve runtime limiti / senaryo 8

- Senaryo: `src/task/rt_scheduler.rs` icinde `set_sched_param` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis policy secimi starvation ve jitter uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: RR dilimi ve RT bandwidth governance.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 054 - RT scheduler: FIFO/RR ve runtime limiti / senaryo 9

- Senaryo: `src/task/rt_scheduler.rs` icinde `calculate_timeslice` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis policy secimi starvation ve jitter uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: RR dilimi ve RT bandwidth governance.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 055 - RT scheduler: FIFO/RR ve runtime limiti / senaryo 10

- Senaryo: `src/task/rt_scheduler.rs` icinde `enqueue` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis policy secimi starvation ve jitter uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: RR dilimi ve RT bandwidth governance.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 056 - RT scheduler: FIFO/RR ve runtime limiti / senaryo 11

- Senaryo: `src/task/rt_scheduler.rs` icinde `tick` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis policy secimi starvation ve jitter uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: RR dilimi ve RT bandwidth governance.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 057 - RT scheduler: FIFO/RR ve runtime limiti / senaryo 12

- Senaryo: `src/task/rt_scheduler.rs` icinde `set_sched_param` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis policy secimi starvation ve jitter uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: RR dilimi ve RT bandwidth governance.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 058 - RT scheduler: FIFO/RR ve runtime limiti / senaryo 13

- Senaryo: `src/task/rt_scheduler.rs` icinde `calculate_timeslice` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis policy secimi starvation ve jitter uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: RR dilimi ve RT bandwidth governance.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 059 - RT scheduler: FIFO/RR ve runtime limiti / senaryo 14

- Senaryo: `src/task/rt_scheduler.rs` icinde `enqueue` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis policy secimi starvation ve jitter uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: RR dilimi ve RT bandwidth governance.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 060 - RT scheduler: FIFO/RR ve runtime limiti / senaryo 15

- Senaryo: `src/task/rt_scheduler.rs` icinde `tick` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis policy secimi starvation ve jitter uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: RR dilimi ve RT bandwidth governance.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

---

## Vaka Kumesi 05 - CFS: vruntime adalet motoru

### Vaka 061 - CFS: vruntime adalet motoru / senaryo 1

- Senaryo: `src/task/cfs.rs` icinde `weight_to_vruntime` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Wakup-heavy yukte asiri preemption ve fairness gerilimi.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Wakeup granularity ve min_vruntime clamp.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 062 - CFS: vruntime adalet motoru / senaryo 2

- Senaryo: `src/task/cfs.rs` icinde `enqueue` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Wakup-heavy yukte asiri preemption ve fairness gerilimi.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Wakeup granularity ve min_vruntime clamp.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 063 - CFS: vruntime adalet motoru / senaryo 3

- Senaryo: `src/task/cfs.rs` icinde `pick_next` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Wakup-heavy yukte asiri preemption ve fairness gerilimi.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Wakeup granularity ve min_vruntime clamp.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 064 - CFS: vruntime adalet motoru / senaryo 4

- Senaryo: `src/task/cfs.rs` icinde `check_preempt_wakeup` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Wakup-heavy yukte asiri preemption ve fairness gerilimi.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Wakeup granularity ve min_vruntime clamp.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 065 - CFS: vruntime adalet motoru / senaryo 5

- Senaryo: `src/task/cfs.rs` icinde `weight_to_vruntime` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Wakup-heavy yukte asiri preemption ve fairness gerilimi.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Wakeup granularity ve min_vruntime clamp.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 066 - CFS: vruntime adalet motoru / senaryo 6

- Senaryo: `src/task/cfs.rs` icinde `enqueue` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Wakup-heavy yukte asiri preemption ve fairness gerilimi.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Wakeup granularity ve min_vruntime clamp.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 067 - CFS: vruntime adalet motoru / senaryo 7

- Senaryo: `src/task/cfs.rs` icinde `pick_next` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Wakup-heavy yukte asiri preemption ve fairness gerilimi.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Wakeup granularity ve min_vruntime clamp.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 068 - CFS: vruntime adalet motoru / senaryo 8

- Senaryo: `src/task/cfs.rs` icinde `check_preempt_wakeup` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Wakup-heavy yukte asiri preemption ve fairness gerilimi.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Wakeup granularity ve min_vruntime clamp.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 069 - CFS: vruntime adalet motoru / senaryo 9

- Senaryo: `src/task/cfs.rs` icinde `weight_to_vruntime` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Wakup-heavy yukte asiri preemption ve fairness gerilimi.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Wakeup granularity ve min_vruntime clamp.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 070 - CFS: vruntime adalet motoru / senaryo 10

- Senaryo: `src/task/cfs.rs` icinde `enqueue` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Wakup-heavy yukte asiri preemption ve fairness gerilimi.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Wakeup granularity ve min_vruntime clamp.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 071 - CFS: vruntime adalet motoru / senaryo 11

- Senaryo: `src/task/cfs.rs` icinde `pick_next` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Wakup-heavy yukte asiri preemption ve fairness gerilimi.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Wakeup granularity ve min_vruntime clamp.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 072 - CFS: vruntime adalet motoru / senaryo 12

- Senaryo: `src/task/cfs.rs` icinde `check_preempt_wakeup` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Wakup-heavy yukte asiri preemption ve fairness gerilimi.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Wakeup granularity ve min_vruntime clamp.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 073 - CFS: vruntime adalet motoru / senaryo 13

- Senaryo: `src/task/cfs.rs` icinde `weight_to_vruntime` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Wakup-heavy yukte asiri preemption ve fairness gerilimi.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Wakeup granularity ve min_vruntime clamp.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 074 - CFS: vruntime adalet motoru / senaryo 14

- Senaryo: `src/task/cfs.rs` icinde `enqueue` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Wakup-heavy yukte asiri preemption ve fairness gerilimi.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Wakeup granularity ve min_vruntime clamp.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 075 - CFS: vruntime adalet motoru / senaryo 15

- Senaryo: `src/task/cfs.rs` icinde `pick_next` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Wakup-heavy yukte asiri preemption ve fairness gerilimi.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Wakeup granularity ve min_vruntime clamp.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

---

## Vaka Kumesi 06 - EEVDF: eligible_vtime ve virtual deadline

### Vaka 076 - EEVDF: eligible_vtime ve virtual deadline / senaryo 1

- Senaryo: `src/task/eevdf.rs` icinde `update_runtime` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis slice ve lag dengesi wakeup davranisini bozar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Lag tabanli eligibility + deadline siralama.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 077 - EEVDF: eligible_vtime ve virtual deadline / senaryo 2

- Senaryo: `src/task/eevdf.rs` icinde `pick_next` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis slice ve lag dengesi wakeup davranisini bozar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Lag tabanli eligibility + deadline siralama.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 078 - EEVDF: eligible_vtime ve virtual deadline / senaryo 3

- Senaryo: `src/task/eevdf.rs` icinde `should_preempt` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis slice ve lag dengesi wakeup davranisini bozar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Lag tabanli eligibility + deadline siralama.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 079 - EEVDF: eligible_vtime ve virtual deadline / senaryo 4

- Senaryo: `src/task/eevdf.rs` icinde `stats` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis slice ve lag dengesi wakeup davranisini bozar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Lag tabanli eligibility + deadline siralama.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 080 - EEVDF: eligible_vtime ve virtual deadline / senaryo 5

- Senaryo: `src/task/eevdf.rs` icinde `update_runtime` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis slice ve lag dengesi wakeup davranisini bozar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Lag tabanli eligibility + deadline siralama.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 081 - EEVDF: eligible_vtime ve virtual deadline / senaryo 6

- Senaryo: `src/task/eevdf.rs` icinde `pick_next` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis slice ve lag dengesi wakeup davranisini bozar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Lag tabanli eligibility + deadline siralama.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 082 - EEVDF: eligible_vtime ve virtual deadline / senaryo 7

- Senaryo: `src/task/eevdf.rs` icinde `should_preempt` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis slice ve lag dengesi wakeup davranisini bozar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Lag tabanli eligibility + deadline siralama.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 083 - EEVDF: eligible_vtime ve virtual deadline / senaryo 8

- Senaryo: `src/task/eevdf.rs` icinde `stats` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis slice ve lag dengesi wakeup davranisini bozar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Lag tabanli eligibility + deadline siralama.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 084 - EEVDF: eligible_vtime ve virtual deadline / senaryo 9

- Senaryo: `src/task/eevdf.rs` icinde `update_runtime` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis slice ve lag dengesi wakeup davranisini bozar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Lag tabanli eligibility + deadline siralama.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 085 - EEVDF: eligible_vtime ve virtual deadline / senaryo 10

- Senaryo: `src/task/eevdf.rs` icinde `pick_next` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis slice ve lag dengesi wakeup davranisini bozar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Lag tabanli eligibility + deadline siralama.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 086 - EEVDF: eligible_vtime ve virtual deadline / senaryo 11

- Senaryo: `src/task/eevdf.rs` icinde `should_preempt` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis slice ve lag dengesi wakeup davranisini bozar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Lag tabanli eligibility + deadline siralama.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 087 - EEVDF: eligible_vtime ve virtual deadline / senaryo 12

- Senaryo: `src/task/eevdf.rs` icinde `stats` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis slice ve lag dengesi wakeup davranisini bozar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Lag tabanli eligibility + deadline siralama.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 088 - EEVDF: eligible_vtime ve virtual deadline / senaryo 13

- Senaryo: `src/task/eevdf.rs` icinde `update_runtime` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis slice ve lag dengesi wakeup davranisini bozar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Lag tabanli eligibility + deadline siralama.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 089 - EEVDF: eligible_vtime ve virtual deadline / senaryo 14

- Senaryo: `src/task/eevdf.rs` icinde `pick_next` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis slice ve lag dengesi wakeup davranisini bozar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Lag tabanli eligibility + deadline siralama.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 090 - EEVDF: eligible_vtime ve virtual deadline / senaryo 15

- Senaryo: `src/task/eevdf.rs` icinde `should_preempt` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis slice ve lag dengesi wakeup davranisini bozar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Lag tabanli eligibility + deadline siralama.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

---

## Vaka Kumesi 07 - Deadline scheduler: EDF/CBS admission ve replenish

### Vaka 091 - Deadline scheduler: EDF/CBS admission ve replenish / senaryo 1

- Senaryo: `src/task/deadline.rs` icinde `compute_bandwidth` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Admission ihlali deadline miss patlamasi uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Bandwidth limiti + periodik replenish + throttle.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 092 - Deadline scheduler: EDF/CBS admission ve replenish / senaryo 2

- Senaryo: `src/task/deadline.rs` icinde `check_replenishments` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Admission ihlali deadline miss patlamasi uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Bandwidth limiti + periodik replenish + throttle.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 093 - Deadline scheduler: EDF/CBS admission ve replenish / senaryo 3

- Senaryo: `src/task/deadline.rs` icinde `consume_runtime` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Admission ihlali deadline miss patlamasi uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Bandwidth limiti + periodik replenish + throttle.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 094 - Deadline scheduler: EDF/CBS admission ve replenish / senaryo 4

- Senaryo: `src/task/deadline.rs` icinde `enqueue` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Admission ihlali deadline miss patlamasi uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Bandwidth limiti + periodik replenish + throttle.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 095 - Deadline scheduler: EDF/CBS admission ve replenish / senaryo 5

- Senaryo: `src/task/deadline.rs` icinde `compute_bandwidth` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Admission ihlali deadline miss patlamasi uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Bandwidth limiti + periodik replenish + throttle.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 096 - Deadline scheduler: EDF/CBS admission ve replenish / senaryo 6

- Senaryo: `src/task/deadline.rs` icinde `check_replenishments` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Admission ihlali deadline miss patlamasi uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Bandwidth limiti + periodik replenish + throttle.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 097 - Deadline scheduler: EDF/CBS admission ve replenish / senaryo 7

- Senaryo: `src/task/deadline.rs` icinde `consume_runtime` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Admission ihlali deadline miss patlamasi uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Bandwidth limiti + periodik replenish + throttle.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 098 - Deadline scheduler: EDF/CBS admission ve replenish / senaryo 8

- Senaryo: `src/task/deadline.rs` icinde `enqueue` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Admission ihlali deadline miss patlamasi uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Bandwidth limiti + periodik replenish + throttle.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 099 - Deadline scheduler: EDF/CBS admission ve replenish / senaryo 9

- Senaryo: `src/task/deadline.rs` icinde `compute_bandwidth` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Admission ihlali deadline miss patlamasi uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Bandwidth limiti + periodik replenish + throttle.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 100 - Deadline scheduler: EDF/CBS admission ve replenish / senaryo 10

- Senaryo: `src/task/deadline.rs` icinde `check_replenishments` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Admission ihlali deadline miss patlamasi uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Bandwidth limiti + periodik replenish + throttle.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 101 - Deadline scheduler: EDF/CBS admission ve replenish / senaryo 11

- Senaryo: `src/task/deadline.rs` icinde `consume_runtime` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Admission ihlali deadline miss patlamasi uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Bandwidth limiti + periodik replenish + throttle.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 102 - Deadline scheduler: EDF/CBS admission ve replenish / senaryo 12

- Senaryo: `src/task/deadline.rs` icinde `enqueue` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Admission ihlali deadline miss patlamasi uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Bandwidth limiti + periodik replenish + throttle.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 103 - Deadline scheduler: EDF/CBS admission ve replenish / senaryo 13

- Senaryo: `src/task/deadline.rs` icinde `compute_bandwidth` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Admission ihlali deadline miss patlamasi uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Bandwidth limiti + periodik replenish + throttle.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 104 - Deadline scheduler: EDF/CBS admission ve replenish / senaryo 14

- Senaryo: `src/task/deadline.rs` icinde `check_replenishments` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Admission ihlali deadline miss patlamasi uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Bandwidth limiti + periodik replenish + throttle.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 105 - Deadline scheduler: EDF/CBS admission ve replenish / senaryo 15

- Senaryo: `src/task/deadline.rs` icinde `consume_runtime` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Admission ihlali deadline miss patlamasi uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Bandwidth limiti + periodik replenish + throttle.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

---

## Vaka Kumesi 08 - Chase-Lev deque: lock-free race analizi

### Vaka 106 - Chase-Lev deque: lock-free race analizi / senaryo 1

- Senaryo: `src/task/deque.rs` icinde `push` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Ordering bug'i sessiz veri bozulmasi yaratir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Acquire/Release/SeqCst sinirlarinin explicit kullanimi.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 107 - Chase-Lev deque: lock-free race analizi / senaryo 2

- Senaryo: `src/task/deque.rs` icinde `pop` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Ordering bug'i sessiz veri bozulmasi yaratir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Acquire/Release/SeqCst sinirlarinin explicit kullanimi.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 108 - Chase-Lev deque: lock-free race analizi / senaryo 3

- Senaryo: `src/task/deque.rs` icinde `steal` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Ordering bug'i sessiz veri bozulmasi yaratir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Acquire/Release/SeqCst sinirlarinin explicit kullanimi.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 109 - Chase-Lev deque: lock-free race analizi / senaryo 4

- Senaryo: `src/task/deque.rs` icinde `compare_exchange` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Ordering bug'i sessiz veri bozulmasi yaratir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Acquire/Release/SeqCst sinirlarinin explicit kullanimi.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 110 - Chase-Lev deque: lock-free race analizi / senaryo 5

- Senaryo: `src/task/deque.rs` icinde `push` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Ordering bug'i sessiz veri bozulmasi yaratir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Acquire/Release/SeqCst sinirlarinin explicit kullanimi.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 111 - Chase-Lev deque: lock-free race analizi / senaryo 6

- Senaryo: `src/task/deque.rs` icinde `pop` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Ordering bug'i sessiz veri bozulmasi yaratir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Acquire/Release/SeqCst sinirlarinin explicit kullanimi.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 112 - Chase-Lev deque: lock-free race analizi / senaryo 7

- Senaryo: `src/task/deque.rs` icinde `steal` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Ordering bug'i sessiz veri bozulmasi yaratir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Acquire/Release/SeqCst sinirlarinin explicit kullanimi.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 113 - Chase-Lev deque: lock-free race analizi / senaryo 8

- Senaryo: `src/task/deque.rs` icinde `compare_exchange` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Ordering bug'i sessiz veri bozulmasi yaratir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Acquire/Release/SeqCst sinirlarinin explicit kullanimi.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 114 - Chase-Lev deque: lock-free race analizi / senaryo 9

- Senaryo: `src/task/deque.rs` icinde `push` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Ordering bug'i sessiz veri bozulmasi yaratir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Acquire/Release/SeqCst sinirlarinin explicit kullanimi.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 115 - Chase-Lev deque: lock-free race analizi / senaryo 10

- Senaryo: `src/task/deque.rs` icinde `pop` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Ordering bug'i sessiz veri bozulmasi yaratir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Acquire/Release/SeqCst sinirlarinin explicit kullanimi.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 116 - Chase-Lev deque: lock-free race analizi / senaryo 11

- Senaryo: `src/task/deque.rs` icinde `steal` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Ordering bug'i sessiz veri bozulmasi yaratir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Acquire/Release/SeqCst sinirlarinin explicit kullanimi.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 117 - Chase-Lev deque: lock-free race analizi / senaryo 12

- Senaryo: `src/task/deque.rs` icinde `compare_exchange` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Ordering bug'i sessiz veri bozulmasi yaratir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Acquire/Release/SeqCst sinirlarinin explicit kullanimi.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 118 - Chase-Lev deque: lock-free race analizi / senaryo 13

- Senaryo: `src/task/deque.rs` icinde `push` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Ordering bug'i sessiz veri bozulmasi yaratir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Acquire/Release/SeqCst sinirlarinin explicit kullanimi.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 119 - Chase-Lev deque: lock-free race analizi / senaryo 14

- Senaryo: `src/task/deque.rs` icinde `pop` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Ordering bug'i sessiz veri bozulmasi yaratir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Acquire/Release/SeqCst sinirlarinin explicit kullanimi.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 120 - Chase-Lev deque: lock-free race analizi / senaryo 15

- Senaryo: `src/task/deque.rs` icinde `steal` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Ordering bug'i sessiz veri bozulmasi yaratir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Acquire/Release/SeqCst sinirlarinin explicit kullanimi.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

---

## Vaka Kumesi 09 - Hiyerarsik timing wheel

### Vaka 121 - Hiyerarsik timing wheel / senaryo 1

- Senaryo: `src/task/timer.rs` icinde `schedule` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Cascade atlanirsa wakeup gecikmeleri birikir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Level wrap noktalarinda zorunlu cascade yolu.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 122 - Hiyerarsik timing wheel / senaryo 2

- Senaryo: `src/task/timer.rs` icinde `tick` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Cascade atlanirsa wakeup gecikmeleri birikir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Level wrap noktalarinda zorunlu cascade yolu.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 123 - Hiyerarsik timing wheel / senaryo 3

- Senaryo: `src/task/timer.rs` icinde `cascade` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Cascade atlanirsa wakeup gecikmeleri birikir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Level wrap noktalarinda zorunlu cascade yolu.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 124 - Hiyerarsik timing wheel / senaryo 4

- Senaryo: `src/task/timer.rs` icinde `WHEEL_SIZE` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Cascade atlanirsa wakeup gecikmeleri birikir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Level wrap noktalarinda zorunlu cascade yolu.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 125 - Hiyerarsik timing wheel / senaryo 5

- Senaryo: `src/task/timer.rs` icinde `schedule` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Cascade atlanirsa wakeup gecikmeleri birikir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Level wrap noktalarinda zorunlu cascade yolu.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 126 - Hiyerarsik timing wheel / senaryo 6

- Senaryo: `src/task/timer.rs` icinde `tick` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Cascade atlanirsa wakeup gecikmeleri birikir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Level wrap noktalarinda zorunlu cascade yolu.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 127 - Hiyerarsik timing wheel / senaryo 7

- Senaryo: `src/task/timer.rs` icinde `cascade` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Cascade atlanirsa wakeup gecikmeleri birikir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Level wrap noktalarinda zorunlu cascade yolu.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 128 - Hiyerarsik timing wheel / senaryo 8

- Senaryo: `src/task/timer.rs` icinde `WHEEL_SIZE` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Cascade atlanirsa wakeup gecikmeleri birikir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Level wrap noktalarinda zorunlu cascade yolu.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 129 - Hiyerarsik timing wheel / senaryo 9

- Senaryo: `src/task/timer.rs` icinde `schedule` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Cascade atlanirsa wakeup gecikmeleri birikir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Level wrap noktalarinda zorunlu cascade yolu.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 130 - Hiyerarsik timing wheel / senaryo 10

- Senaryo: `src/task/timer.rs` icinde `tick` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Cascade atlanirsa wakeup gecikmeleri birikir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Level wrap noktalarinda zorunlu cascade yolu.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 131 - Hiyerarsik timing wheel / senaryo 11

- Senaryo: `src/task/timer.rs` icinde `cascade` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Cascade atlanirsa wakeup gecikmeleri birikir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Level wrap noktalarinda zorunlu cascade yolu.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 132 - Hiyerarsik timing wheel / senaryo 12

- Senaryo: `src/task/timer.rs` icinde `WHEEL_SIZE` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Cascade atlanirsa wakeup gecikmeleri birikir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Level wrap noktalarinda zorunlu cascade yolu.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 133 - Hiyerarsik timing wheel / senaryo 13

- Senaryo: `src/task/timer.rs` icinde `schedule` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Cascade atlanirsa wakeup gecikmeleri birikir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Level wrap noktalarinda zorunlu cascade yolu.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 134 - Hiyerarsik timing wheel / senaryo 14

- Senaryo: `src/task/timer.rs` icinde `tick` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Cascade atlanirsa wakeup gecikmeleri birikir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Level wrap noktalarinda zorunlu cascade yolu.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 135 - Hiyerarsik timing wheel / senaryo 15

- Senaryo: `src/task/timer.rs` icinde `cascade` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Cascade atlanirsa wakeup gecikmeleri birikir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Level wrap noktalarinda zorunlu cascade yolu.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

---

## Vaka Kumesi 10 - Zone-aware PMM fallback mimarisi

### Vaka 136 - Zone-aware PMM fallback mimarisi / senaryo 1

- Senaryo: `src/memory/fibonacci_pmm.rs` icinde `fallback` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Sik fallback gizli kapasite krizini maskeler.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Zone telemetrisi ve reclaim tetigi.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 137 - Zone-aware PMM fallback mimarisi / senaryo 2

- Senaryo: `src/memory/fibonacci_pmm.rs` icinde `allocate_from_zone` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Sik fallback gizli kapasite krizini maskeler.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Zone telemetrisi ve reclaim tetigi.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 138 - Zone-aware PMM fallback mimarisi / senaryo 3

- Senaryo: `src/memory/fibonacci_pmm.rs` icinde `allocate_contiguous_from_zone` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Sik fallback gizli kapasite krizini maskeler.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Zone telemetrisi ve reclaim tetigi.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 139 - Zone-aware PMM fallback mimarisi / senaryo 4

- Senaryo: `src/memory/fibonacci_pmm.rs` icinde `zone_stats` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Sik fallback gizli kapasite krizini maskeler.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Zone telemetrisi ve reclaim tetigi.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 140 - Zone-aware PMM fallback mimarisi / senaryo 5

- Senaryo: `src/memory/fibonacci_pmm.rs` icinde `fallback` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Sik fallback gizli kapasite krizini maskeler.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Zone telemetrisi ve reclaim tetigi.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 141 - Zone-aware PMM fallback mimarisi / senaryo 6

- Senaryo: `src/memory/fibonacci_pmm.rs` icinde `allocate_from_zone` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Sik fallback gizli kapasite krizini maskeler.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Zone telemetrisi ve reclaim tetigi.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 142 - Zone-aware PMM fallback mimarisi / senaryo 7

- Senaryo: `src/memory/fibonacci_pmm.rs` icinde `allocate_contiguous_from_zone` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Sik fallback gizli kapasite krizini maskeler.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Zone telemetrisi ve reclaim tetigi.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 143 - Zone-aware PMM fallback mimarisi / senaryo 8

- Senaryo: `src/memory/fibonacci_pmm.rs` icinde `zone_stats` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Sik fallback gizli kapasite krizini maskeler.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Zone telemetrisi ve reclaim tetigi.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 144 - Zone-aware PMM fallback mimarisi / senaryo 9

- Senaryo: `src/memory/fibonacci_pmm.rs` icinde `fallback` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Sik fallback gizli kapasite krizini maskeler.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Zone telemetrisi ve reclaim tetigi.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 145 - Zone-aware PMM fallback mimarisi / senaryo 10

- Senaryo: `src/memory/fibonacci_pmm.rs` icinde `allocate_from_zone` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Sik fallback gizli kapasite krizini maskeler.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Zone telemetrisi ve reclaim tetigi.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 146 - Zone-aware PMM fallback mimarisi / senaryo 11

- Senaryo: `src/memory/fibonacci_pmm.rs` icinde `allocate_contiguous_from_zone` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Sik fallback gizli kapasite krizini maskeler.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Zone telemetrisi ve reclaim tetigi.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 147 - Zone-aware PMM fallback mimarisi / senaryo 12

- Senaryo: `src/memory/fibonacci_pmm.rs` icinde `zone_stats` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Sik fallback gizli kapasite krizini maskeler.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Zone telemetrisi ve reclaim tetigi.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 148 - Zone-aware PMM fallback mimarisi / senaryo 13

- Senaryo: `src/memory/fibonacci_pmm.rs` icinde `fallback` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Sik fallback gizli kapasite krizini maskeler.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Zone telemetrisi ve reclaim tetigi.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 149 - Zone-aware PMM fallback mimarisi / senaryo 14

- Senaryo: `src/memory/fibonacci_pmm.rs` icinde `allocate_from_zone` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Sik fallback gizli kapasite krizini maskeler.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Zone telemetrisi ve reclaim tetigi.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 150 - Zone-aware PMM fallback mimarisi / senaryo 15

- Senaryo: `src/memory/fibonacci_pmm.rs` icinde `allocate_contiguous_from_zone` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Sik fallback gizli kapasite krizini maskeler.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Zone telemetrisi ve reclaim tetigi.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

---

## Vaka Kumesi 11 - Fibonacci buddy split/coalesce

### Vaka 151 - Fibonacci buddy split/coalesce / senaryo 1

- Senaryo: `src/memory/fibonacci_buddy.rs` icinde `split_block` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis buddy hesabinda leak veya overlap olur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Adres bazli buddy aritmetigi + recursive coalesce.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 152 - Fibonacci buddy split/coalesce / senaryo 2

- Senaryo: `src/memory/fibonacci_buddy.rs` icinde `try_coalesce` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis buddy hesabinda leak veya overlap olur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Adres bazli buddy aritmetigi + recursive coalesce.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 153 - Fibonacci buddy split/coalesce / senaryo 3

- Senaryo: `src/memory/fibonacci_buddy.rs` icinde `find_buddy` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis buddy hesabinda leak veya overlap olur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Adres bazli buddy aritmetigi + recursive coalesce.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 154 - Fibonacci buddy split/coalesce / senaryo 4

- Senaryo: `src/memory/fibonacci_buddy.rs` icinde `utilization` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis buddy hesabinda leak veya overlap olur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Adres bazli buddy aritmetigi + recursive coalesce.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 155 - Fibonacci buddy split/coalesce / senaryo 5

- Senaryo: `src/memory/fibonacci_buddy.rs` icinde `split_block` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis buddy hesabinda leak veya overlap olur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Adres bazli buddy aritmetigi + recursive coalesce.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 156 - Fibonacci buddy split/coalesce / senaryo 6

- Senaryo: `src/memory/fibonacci_buddy.rs` icinde `try_coalesce` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis buddy hesabinda leak veya overlap olur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Adres bazli buddy aritmetigi + recursive coalesce.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 157 - Fibonacci buddy split/coalesce / senaryo 7

- Senaryo: `src/memory/fibonacci_buddy.rs` icinde `find_buddy` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis buddy hesabinda leak veya overlap olur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Adres bazli buddy aritmetigi + recursive coalesce.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 158 - Fibonacci buddy split/coalesce / senaryo 8

- Senaryo: `src/memory/fibonacci_buddy.rs` icinde `utilization` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis buddy hesabinda leak veya overlap olur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Adres bazli buddy aritmetigi + recursive coalesce.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 159 - Fibonacci buddy split/coalesce / senaryo 9

- Senaryo: `src/memory/fibonacci_buddy.rs` icinde `split_block` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis buddy hesabinda leak veya overlap olur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Adres bazli buddy aritmetigi + recursive coalesce.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 160 - Fibonacci buddy split/coalesce / senaryo 10

- Senaryo: `src/memory/fibonacci_buddy.rs` icinde `try_coalesce` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis buddy hesabinda leak veya overlap olur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Adres bazli buddy aritmetigi + recursive coalesce.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 161 - Fibonacci buddy split/coalesce / senaryo 11

- Senaryo: `src/memory/fibonacci_buddy.rs` icinde `find_buddy` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis buddy hesabinda leak veya overlap olur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Adres bazli buddy aritmetigi + recursive coalesce.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 162 - Fibonacci buddy split/coalesce / senaryo 12

- Senaryo: `src/memory/fibonacci_buddy.rs` icinde `utilization` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis buddy hesabinda leak veya overlap olur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Adres bazli buddy aritmetigi + recursive coalesce.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 163 - Fibonacci buddy split/coalesce / senaryo 13

- Senaryo: `src/memory/fibonacci_buddy.rs` icinde `split_block` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis buddy hesabinda leak veya overlap olur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Adres bazli buddy aritmetigi + recursive coalesce.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 164 - Fibonacci buddy split/coalesce / senaryo 14

- Senaryo: `src/memory/fibonacci_buddy.rs` icinde `try_coalesce` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis buddy hesabinda leak veya overlap olur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Adres bazli buddy aritmetigi + recursive coalesce.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 165 - Fibonacci buddy split/coalesce / senaryo 15

- Senaryo: `src/memory/fibonacci_buddy.rs` icinde `find_buddy` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis buddy hesabinda leak veya overlap olur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Adres bazli buddy aritmetigi + recursive coalesce.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

---

## Vaka Kumesi 12 - TLSF heap wrapper guvenligi

### Vaka 166 - TLSF heap wrapper guvenligi / senaryo 1

- Senaryo: `src/allocator/tlsf.rs` icinde `insert_free_region_ptr` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Heap metadata bozulmasi gec fark edilir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Canary, tracker, boundary guard ve erken heap ayrimi.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 167 - TLSF heap wrapper guvenligi / senaryo 2

- Senaryo: `src/allocator/tlsf.rs` icinde `alloc_from_main_heap` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Heap metadata bozulmasi gec fark edilir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Canary, tracker, boundary guard ve erken heap ayrimi.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 168 - TLSF heap wrapper guvenligi / senaryo 3

- Senaryo: `src/allocator/tlsf.rs` icinde `dealloc_to_main_heap` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Heap metadata bozulmasi gec fark edilir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Canary, tracker, boundary guard ve erken heap ayrimi.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 169 - TLSF heap wrapper guvenligi / senaryo 4

- Senaryo: `src/allocator/tlsf.rs` icinde `check_integrity` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Heap metadata bozulmasi gec fark edilir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Canary, tracker, boundary guard ve erken heap ayrimi.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 170 - TLSF heap wrapper guvenligi / senaryo 5

- Senaryo: `src/allocator/tlsf.rs` icinde `insert_free_region_ptr` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Heap metadata bozulmasi gec fark edilir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Canary, tracker, boundary guard ve erken heap ayrimi.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 171 - TLSF heap wrapper guvenligi / senaryo 6

- Senaryo: `src/allocator/tlsf.rs` icinde `alloc_from_main_heap` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Heap metadata bozulmasi gec fark edilir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Canary, tracker, boundary guard ve erken heap ayrimi.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 172 - TLSF heap wrapper guvenligi / senaryo 7

- Senaryo: `src/allocator/tlsf.rs` icinde `dealloc_to_main_heap` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Heap metadata bozulmasi gec fark edilir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Canary, tracker, boundary guard ve erken heap ayrimi.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 173 - TLSF heap wrapper guvenligi / senaryo 8

- Senaryo: `src/allocator/tlsf.rs` icinde `check_integrity` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Heap metadata bozulmasi gec fark edilir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Canary, tracker, boundary guard ve erken heap ayrimi.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 174 - TLSF heap wrapper guvenligi / senaryo 9

- Senaryo: `src/allocator/tlsf.rs` icinde `insert_free_region_ptr` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Heap metadata bozulmasi gec fark edilir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Canary, tracker, boundary guard ve erken heap ayrimi.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 175 - TLSF heap wrapper guvenligi / senaryo 10

- Senaryo: `src/allocator/tlsf.rs` icinde `alloc_from_main_heap` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Heap metadata bozulmasi gec fark edilir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Canary, tracker, boundary guard ve erken heap ayrimi.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 176 - TLSF heap wrapper guvenligi / senaryo 11

- Senaryo: `src/allocator/tlsf.rs` icinde `dealloc_to_main_heap` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Heap metadata bozulmasi gec fark edilir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Canary, tracker, boundary guard ve erken heap ayrimi.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 177 - TLSF heap wrapper guvenligi / senaryo 12

- Senaryo: `src/allocator/tlsf.rs` icinde `check_integrity` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Heap metadata bozulmasi gec fark edilir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Canary, tracker, boundary guard ve erken heap ayrimi.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 178 - TLSF heap wrapper guvenligi / senaryo 13

- Senaryo: `src/allocator/tlsf.rs` icinde `insert_free_region_ptr` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Heap metadata bozulmasi gec fark edilir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Canary, tracker, boundary guard ve erken heap ayrimi.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 179 - TLSF heap wrapper guvenligi / senaryo 14

- Senaryo: `src/allocator/tlsf.rs` icinde `alloc_from_main_heap` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Heap metadata bozulmasi gec fark edilir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Canary, tracker, boundary guard ve erken heap ayrimi.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 180 - TLSF heap wrapper guvenligi / senaryo 15

- Senaryo: `src/allocator/tlsf.rs` icinde `dealloc_to_main_heap` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Heap metadata bozulmasi gec fark edilir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Canary, tracker, boundary guard ve erken heap ayrimi.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

---

## Vaka Kumesi 13 - User page fault, COW ve THP karari

### Vaka 181 - User page fault, COW ve THP karari / senaryo 1

- Senaryo: `src/memory/mod.rs` icinde `handle_user_page_fault` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis fault ayrimi permission bypass veya crash uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Fail-closed fault ayrimi ve map flag sanitization.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 182 - User page fault, COW ve THP karari / senaryo 2

- Senaryo: `src/memory/mod.rs` icinde `handle_cow_fault` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis fault ayrimi permission bypass veya crash uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Fail-closed fault ayrimi ve map flag sanitization.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 183 - User page fault, COW ve THP karari / senaryo 3

- Senaryo: `src/memory/mod.rs` icinde `try_map_thp_anon` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis fault ayrimi permission bypass veya crash uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Fail-closed fault ayrimi ve map flag sanitization.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 184 - User page fault, COW ve THP karari / senaryo 4

- Senaryo: `src/memory/mod.rs` icinde `sanitize_user_map_flags` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis fault ayrimi permission bypass veya crash uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Fail-closed fault ayrimi ve map flag sanitization.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 185 - User page fault, COW ve THP karari / senaryo 5

- Senaryo: `src/memory/mod.rs` icinde `handle_user_page_fault` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis fault ayrimi permission bypass veya crash uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Fail-closed fault ayrimi ve map flag sanitization.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 186 - User page fault, COW ve THP karari / senaryo 6

- Senaryo: `src/memory/mod.rs` icinde `handle_cow_fault` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis fault ayrimi permission bypass veya crash uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Fail-closed fault ayrimi ve map flag sanitization.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 187 - User page fault, COW ve THP karari / senaryo 7

- Senaryo: `src/memory/mod.rs` icinde `try_map_thp_anon` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis fault ayrimi permission bypass veya crash uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Fail-closed fault ayrimi ve map flag sanitization.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 188 - User page fault, COW ve THP karari / senaryo 8

- Senaryo: `src/memory/mod.rs` icinde `sanitize_user_map_flags` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis fault ayrimi permission bypass veya crash uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Fail-closed fault ayrimi ve map flag sanitization.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 189 - User page fault, COW ve THP karari / senaryo 9

- Senaryo: `src/memory/mod.rs` icinde `handle_user_page_fault` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis fault ayrimi permission bypass veya crash uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Fail-closed fault ayrimi ve map flag sanitization.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 190 - User page fault, COW ve THP karari / senaryo 10

- Senaryo: `src/memory/mod.rs` icinde `handle_cow_fault` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis fault ayrimi permission bypass veya crash uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Fail-closed fault ayrimi ve map flag sanitization.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 191 - User page fault, COW ve THP karari / senaryo 11

- Senaryo: `src/memory/mod.rs` icinde `try_map_thp_anon` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis fault ayrimi permission bypass veya crash uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Fail-closed fault ayrimi ve map flag sanitization.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 192 - User page fault, COW ve THP karari / senaryo 12

- Senaryo: `src/memory/mod.rs` icinde `sanitize_user_map_flags` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis fault ayrimi permission bypass veya crash uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Fail-closed fault ayrimi ve map flag sanitization.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 193 - User page fault, COW ve THP karari / senaryo 13

- Senaryo: `src/memory/mod.rs` icinde `handle_user_page_fault` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis fault ayrimi permission bypass veya crash uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Fail-closed fault ayrimi ve map flag sanitization.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 194 - User page fault, COW ve THP karari / senaryo 14

- Senaryo: `src/memory/mod.rs` icinde `handle_cow_fault` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis fault ayrimi permission bypass veya crash uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Fail-closed fault ayrimi ve map flag sanitization.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 195 - User page fault, COW ve THP karari / senaryo 15

- Senaryo: `src/memory/mod.rs` icinde `try_map_thp_anon` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis fault ayrimi permission bypass veya crash uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Fail-closed fault ayrimi ve map flag sanitization.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

---

## Vaka Kumesi 14 - Reclaim daemon, writeback budget ve pressure

### Vaka 196 - Reclaim daemon, writeback budget ve pressure / senaryo 1

- Senaryo: `src/memory/mod.rs` icinde `memory_reclaim_daemon` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: rho > 1 kalirsa writeback kuyrugu patlar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Budget tabanli writeback ve pressure sinyali.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 197 - Reclaim daemon, writeback budget ve pressure / senaryo 2

- Senaryo: `src/memory/mod.rs` icinde `reclaim_pages_global` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: rho > 1 kalirsa writeback kuyrugu patlar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Budget tabanli writeback ve pressure sinyali.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 198 - Reclaim daemon, writeback budget ve pressure / senaryo 3

- Senaryo: `src/memory/mod.rs` icinde `process_writeback_budget` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: rho > 1 kalirsa writeback kuyrugu patlar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Budget tabanli writeback ve pressure sinyali.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 199 - Reclaim daemon, writeback budget ve pressure / senaryo 4

- Senaryo: `src/memory/mod.rs` icinde `start_reclaim_daemon` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: rho > 1 kalirsa writeback kuyrugu patlar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Budget tabanli writeback ve pressure sinyali.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 200 - Reclaim daemon, writeback budget ve pressure / senaryo 5

- Senaryo: `src/memory/mod.rs` icinde `memory_reclaim_daemon` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: rho > 1 kalirsa writeback kuyrugu patlar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Budget tabanli writeback ve pressure sinyali.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 201 - Reclaim daemon, writeback budget ve pressure / senaryo 6

- Senaryo: `src/memory/mod.rs` icinde `reclaim_pages_global` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: rho > 1 kalirsa writeback kuyrugu patlar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Budget tabanli writeback ve pressure sinyali.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 202 - Reclaim daemon, writeback budget ve pressure / senaryo 7

- Senaryo: `src/memory/mod.rs` icinde `process_writeback_budget` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: rho > 1 kalirsa writeback kuyrugu patlar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Budget tabanli writeback ve pressure sinyali.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 203 - Reclaim daemon, writeback budget ve pressure / senaryo 8

- Senaryo: `src/memory/mod.rs` icinde `start_reclaim_daemon` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: rho > 1 kalirsa writeback kuyrugu patlar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Budget tabanli writeback ve pressure sinyali.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 204 - Reclaim daemon, writeback budget ve pressure / senaryo 9

- Senaryo: `src/memory/mod.rs` icinde `memory_reclaim_daemon` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: rho > 1 kalirsa writeback kuyrugu patlar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Budget tabanli writeback ve pressure sinyali.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 205 - Reclaim daemon, writeback budget ve pressure / senaryo 10

- Senaryo: `src/memory/mod.rs` icinde `reclaim_pages_global` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: rho > 1 kalirsa writeback kuyrugu patlar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Budget tabanli writeback ve pressure sinyali.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 206 - Reclaim daemon, writeback budget ve pressure / senaryo 11

- Senaryo: `src/memory/mod.rs` icinde `process_writeback_budget` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: rho > 1 kalirsa writeback kuyrugu patlar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Budget tabanli writeback ve pressure sinyali.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 207 - Reclaim daemon, writeback budget ve pressure / senaryo 12

- Senaryo: `src/memory/mod.rs` icinde `start_reclaim_daemon` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: rho > 1 kalirsa writeback kuyrugu patlar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Budget tabanli writeback ve pressure sinyali.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 208 - Reclaim daemon, writeback budget ve pressure / senaryo 13

- Senaryo: `src/memory/mod.rs` icinde `memory_reclaim_daemon` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: rho > 1 kalirsa writeback kuyrugu patlar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Budget tabanli writeback ve pressure sinyali.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 209 - Reclaim daemon, writeback budget ve pressure / senaryo 14

- Senaryo: `src/memory/mod.rs` icinde `reclaim_pages_global` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: rho > 1 kalirsa writeback kuyrugu patlar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Budget tabanli writeback ve pressure sinyali.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 210 - Reclaim daemon, writeback budget ve pressure / senaryo 15

- Senaryo: `src/memory/mod.rs` icinde `process_writeback_budget` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: rho > 1 kalirsa writeback kuyrugu patlar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Budget tabanli writeback ve pressure sinyali.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

---

## Vaka Kumesi 15 - MGLRU generation ve victim secimi

### Vaka 211 - MGLRU generation ve victim secimi / senaryo 1

- Senaryo: `src/memory/mglru.rs` icinde `on_access` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis aging policy refault dalgasi uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Generation + access_count + refault promotion.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 212 - MGLRU generation ve victim secimi / senaryo 2

- Senaryo: `src/memory/mglru.rs` icinde `age_tick` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis aging policy refault dalgasi uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Generation + access_count + refault promotion.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 213 - MGLRU generation ve victim secimi / senaryo 3

- Senaryo: `src/memory/mglru.rs` icinde `pick_victim` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis aging policy refault dalgasi uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Generation + access_count + refault promotion.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 214 - MGLRU generation ve victim secimi / senaryo 4

- Senaryo: `src/memory/mglru.rs` icinde `record_refault` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis aging policy refault dalgasi uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Generation + access_count + refault promotion.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 215 - MGLRU generation ve victim secimi / senaryo 5

- Senaryo: `src/memory/mglru.rs` icinde `on_access` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis aging policy refault dalgasi uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Generation + access_count + refault promotion.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 216 - MGLRU generation ve victim secimi / senaryo 6

- Senaryo: `src/memory/mglru.rs` icinde `age_tick` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis aging policy refault dalgasi uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Generation + access_count + refault promotion.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 217 - MGLRU generation ve victim secimi / senaryo 7

- Senaryo: `src/memory/mglru.rs` icinde `pick_victim` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis aging policy refault dalgasi uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Generation + access_count + refault promotion.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 218 - MGLRU generation ve victim secimi / senaryo 8

- Senaryo: `src/memory/mglru.rs` icinde `record_refault` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis aging policy refault dalgasi uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Generation + access_count + refault promotion.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 219 - MGLRU generation ve victim secimi / senaryo 9

- Senaryo: `src/memory/mglru.rs` icinde `on_access` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis aging policy refault dalgasi uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Generation + access_count + refault promotion.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 220 - MGLRU generation ve victim secimi / senaryo 10

- Senaryo: `src/memory/mglru.rs` icinde `age_tick` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis aging policy refault dalgasi uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Generation + access_count + refault promotion.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 221 - MGLRU generation ve victim secimi / senaryo 11

- Senaryo: `src/memory/mglru.rs` icinde `pick_victim` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis aging policy refault dalgasi uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Generation + access_count + refault promotion.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 222 - MGLRU generation ve victim secimi / senaryo 12

- Senaryo: `src/memory/mglru.rs` icinde `record_refault` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis aging policy refault dalgasi uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Generation + access_count + refault promotion.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 223 - MGLRU generation ve victim secimi / senaryo 13

- Senaryo: `src/memory/mglru.rs` icinde `on_access` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis aging policy refault dalgasi uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Generation + access_count + refault promotion.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 224 - MGLRU generation ve victim secimi / senaryo 14

- Senaryo: `src/memory/mglru.rs` icinde `age_tick` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis aging policy refault dalgasi uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Generation + access_count + refault promotion.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 225 - MGLRU generation ve victim secimi / senaryo 15

- Senaryo: `src/memory/mglru.rs` icinde `pick_victim` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis aging policy refault dalgasi uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Generation + access_count + refault promotion.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

---

## Vaka Kumesi 16 - ZSwap compression pipeline

### Vaka 226 - ZSwap compression pipeline / senaryo 1

- Senaryo: `src/memory/zswap.rs` icinde `compress` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis algoritma secimi CPU'yu bogar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Pool limiti, compressor secimi ve fallback yolu.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 227 - ZSwap compression pipeline / senaryo 2

- Senaryo: `src/memory/zswap.rs` icinde `decompress` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis algoritma secimi CPU'yu bogar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Pool limiti, compressor secimi ve fallback yolu.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 228 - ZSwap compression pipeline / senaryo 3

- Senaryo: `src/memory/zswap.rs` icinde `ZSWAP_DEFAULT_POOL_PERCENT` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis algoritma secimi CPU'yu bogar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Pool limiti, compressor secimi ve fallback yolu.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 229 - ZSwap compression pipeline / senaryo 4

- Senaryo: `src/memory/zswap.rs` icinde `Compressor` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis algoritma secimi CPU'yu bogar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Pool limiti, compressor secimi ve fallback yolu.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 230 - ZSwap compression pipeline / senaryo 5

- Senaryo: `src/memory/zswap.rs` icinde `compress` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis algoritma secimi CPU'yu bogar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Pool limiti, compressor secimi ve fallback yolu.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 231 - ZSwap compression pipeline / senaryo 6

- Senaryo: `src/memory/zswap.rs` icinde `decompress` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis algoritma secimi CPU'yu bogar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Pool limiti, compressor secimi ve fallback yolu.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 232 - ZSwap compression pipeline / senaryo 7

- Senaryo: `src/memory/zswap.rs` icinde `ZSWAP_DEFAULT_POOL_PERCENT` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis algoritma secimi CPU'yu bogar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Pool limiti, compressor secimi ve fallback yolu.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 233 - ZSwap compression pipeline / senaryo 8

- Senaryo: `src/memory/zswap.rs` icinde `Compressor` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis algoritma secimi CPU'yu bogar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Pool limiti, compressor secimi ve fallback yolu.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 234 - ZSwap compression pipeline / senaryo 9

- Senaryo: `src/memory/zswap.rs` icinde `compress` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis algoritma secimi CPU'yu bogar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Pool limiti, compressor secimi ve fallback yolu.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 235 - ZSwap compression pipeline / senaryo 10

- Senaryo: `src/memory/zswap.rs` icinde `decompress` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis algoritma secimi CPU'yu bogar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Pool limiti, compressor secimi ve fallback yolu.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 236 - ZSwap compression pipeline / senaryo 11

- Senaryo: `src/memory/zswap.rs` icinde `ZSWAP_DEFAULT_POOL_PERCENT` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis algoritma secimi CPU'yu bogar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Pool limiti, compressor secimi ve fallback yolu.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 237 - ZSwap compression pipeline / senaryo 12

- Senaryo: `src/memory/zswap.rs` icinde `Compressor` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis algoritma secimi CPU'yu bogar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Pool limiti, compressor secimi ve fallback yolu.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 238 - ZSwap compression pipeline / senaryo 13

- Senaryo: `src/memory/zswap.rs` icinde `compress` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis algoritma secimi CPU'yu bogar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Pool limiti, compressor secimi ve fallback yolu.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 239 - ZSwap compression pipeline / senaryo 14

- Senaryo: `src/memory/zswap.rs` icinde `decompress` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis algoritma secimi CPU'yu bogar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Pool limiti, compressor secimi ve fallback yolu.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 240 - ZSwap compression pipeline / senaryo 15

- Senaryo: `src/memory/zswap.rs` icinde `ZSWAP_DEFAULT_POOL_PERCENT` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Yanlis algoritma secimi CPU'yu bogar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Pool limiti, compressor secimi ve fallback yolu.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

---

## Vaka Kumesi 17 - Lock-free io_uring publication boundaries

### Vaka 241 - Lock-free io_uring publication boundaries / senaryo 1

- Senaryo: `src/posix/io_uring_ring.rs` icinde `push` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Tail erken publish edilirse stale read olur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: smp_wmb/smp_rmb ve Acquire/Release disiplini.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 242 - Lock-free io_uring publication boundaries / senaryo 2

- Senaryo: `src/posix/io_uring_ring.rs` icinde `pop` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Tail erken publish edilirse stale read olur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: smp_wmb/smp_rmb ve Acquire/Release disiplini.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 243 - Lock-free io_uring publication boundaries / senaryo 3

- Senaryo: `src/posix/io_uring_ring.rs` icinde `pop_batch` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Tail erken publish edilirse stale read olur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: smp_wmb/smp_rmb ve Acquire/Release disiplini.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 244 - Lock-free io_uring publication boundaries / senaryo 4

- Senaryo: `src/posix/io_uring_ring.rs` icinde `process_submissions` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Tail erken publish edilirse stale read olur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: smp_wmb/smp_rmb ve Acquire/Release disiplini.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 245 - Lock-free io_uring publication boundaries / senaryo 5

- Senaryo: `src/posix/io_uring_ring.rs` icinde `push` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Tail erken publish edilirse stale read olur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: smp_wmb/smp_rmb ve Acquire/Release disiplini.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 246 - Lock-free io_uring publication boundaries / senaryo 6

- Senaryo: `src/posix/io_uring_ring.rs` icinde `pop` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Tail erken publish edilirse stale read olur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: smp_wmb/smp_rmb ve Acquire/Release disiplini.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 247 - Lock-free io_uring publication boundaries / senaryo 7

- Senaryo: `src/posix/io_uring_ring.rs` icinde `pop_batch` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Tail erken publish edilirse stale read olur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: smp_wmb/smp_rmb ve Acquire/Release disiplini.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 248 - Lock-free io_uring publication boundaries / senaryo 8

- Senaryo: `src/posix/io_uring_ring.rs` icinde `process_submissions` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Tail erken publish edilirse stale read olur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: smp_wmb/smp_rmb ve Acquire/Release disiplini.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 249 - Lock-free io_uring publication boundaries / senaryo 9

- Senaryo: `src/posix/io_uring_ring.rs` icinde `push` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Tail erken publish edilirse stale read olur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: smp_wmb/smp_rmb ve Acquire/Release disiplini.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 250 - Lock-free io_uring publication boundaries / senaryo 10

- Senaryo: `src/posix/io_uring_ring.rs` icinde `pop` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Tail erken publish edilirse stale read olur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: smp_wmb/smp_rmb ve Acquire/Release disiplini.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 251 - Lock-free io_uring publication boundaries / senaryo 11

- Senaryo: `src/posix/io_uring_ring.rs` icinde `pop_batch` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Tail erken publish edilirse stale read olur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: smp_wmb/smp_rmb ve Acquire/Release disiplini.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 252 - Lock-free io_uring publication boundaries / senaryo 12

- Senaryo: `src/posix/io_uring_ring.rs` icinde `process_submissions` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Tail erken publish edilirse stale read olur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: smp_wmb/smp_rmb ve Acquire/Release disiplini.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 253 - Lock-free io_uring publication boundaries / senaryo 13

- Senaryo: `src/posix/io_uring_ring.rs` icinde `push` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Tail erken publish edilirse stale read olur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: smp_wmb/smp_rmb ve Acquire/Release disiplini.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 254 - Lock-free io_uring publication boundaries / senaryo 14

- Senaryo: `src/posix/io_uring_ring.rs` icinde `pop` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Tail erken publish edilirse stale read olur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: smp_wmb/smp_rmb ve Acquire/Release disiplini.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 255 - Lock-free io_uring publication boundaries / senaryo 15

- Senaryo: `src/posix/io_uring_ring.rs` icinde `pop_batch` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Tail erken publish edilirse stale read olur.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: smp_wmb/smp_rmb ve Acquire/Release disiplini.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

---

## Vaka Kumesi 18 - TLS 1.3 handshake ve key schedule

### Vaka 256 - TLS 1.3 handshake ve key schedule / senaryo 1

- Senaryo: `src/net/tls.rs` icinde `derive_handshake_secret` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: State gecisi veya transcript hatasi guven modeli kirar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Tipli handshake state ve explicit key schedule adimlari.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 257 - TLS 1.3 handshake ve key schedule / senaryo 2

- Senaryo: `src/net/tls.rs` icinde `derive_master_secret` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: State gecisi veya transcript hatasi guven modeli kirar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Tipli handshake state ve explicit key schedule adimlari.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 258 - TLS 1.3 handshake ve key schedule / senaryo 3

- Senaryo: `src/net/tls.rs` icinde `hkdf_expand_label` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: State gecisi veya transcript hatasi guven modeli kirar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Tipli handshake state ve explicit key schedule adimlari.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 259 - TLS 1.3 handshake ve key schedule / senaryo 4

- Senaryo: `src/net/tls.rs` icinde `process_server_hello` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: State gecisi veya transcript hatasi guven modeli kirar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Tipli handshake state ve explicit key schedule adimlari.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 260 - TLS 1.3 handshake ve key schedule / senaryo 5

- Senaryo: `src/net/tls.rs` icinde `derive_handshake_secret` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: State gecisi veya transcript hatasi guven modeli kirar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Tipli handshake state ve explicit key schedule adimlari.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 261 - TLS 1.3 handshake ve key schedule / senaryo 6

- Senaryo: `src/net/tls.rs` icinde `derive_master_secret` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: State gecisi veya transcript hatasi guven modeli kirar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Tipli handshake state ve explicit key schedule adimlari.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 262 - TLS 1.3 handshake ve key schedule / senaryo 7

- Senaryo: `src/net/tls.rs` icinde `hkdf_expand_label` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: State gecisi veya transcript hatasi guven modeli kirar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Tipli handshake state ve explicit key schedule adimlari.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 263 - TLS 1.3 handshake ve key schedule / senaryo 8

- Senaryo: `src/net/tls.rs` icinde `process_server_hello` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: State gecisi veya transcript hatasi guven modeli kirar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Tipli handshake state ve explicit key schedule adimlari.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 264 - TLS 1.3 handshake ve key schedule / senaryo 9

- Senaryo: `src/net/tls.rs` icinde `derive_handshake_secret` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: State gecisi veya transcript hatasi guven modeli kirar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Tipli handshake state ve explicit key schedule adimlari.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 265 - TLS 1.3 handshake ve key schedule / senaryo 10

- Senaryo: `src/net/tls.rs` icinde `derive_master_secret` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: State gecisi veya transcript hatasi guven modeli kirar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Tipli handshake state ve explicit key schedule adimlari.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 266 - TLS 1.3 handshake ve key schedule / senaryo 11

- Senaryo: `src/net/tls.rs` icinde `hkdf_expand_label` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: State gecisi veya transcript hatasi guven modeli kirar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Tipli handshake state ve explicit key schedule adimlari.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 267 - TLS 1.3 handshake ve key schedule / senaryo 12

- Senaryo: `src/net/tls.rs` icinde `process_server_hello` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: State gecisi veya transcript hatasi guven modeli kirar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Tipli handshake state ve explicit key schedule adimlari.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 268 - TLS 1.3 handshake ve key schedule / senaryo 13

- Senaryo: `src/net/tls.rs` icinde `derive_handshake_secret` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: State gecisi veya transcript hatasi guven modeli kirar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Tipli handshake state ve explicit key schedule adimlari.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 269 - TLS 1.3 handshake ve key schedule / senaryo 14

- Senaryo: `src/net/tls.rs` icinde `derive_master_secret` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: State gecisi veya transcript hatasi guven modeli kirar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Tipli handshake state ve explicit key schedule adimlari.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 270 - TLS 1.3 handshake ve key schedule / senaryo 15

- Senaryo: `src/net/tls.rs` icinde `hkdf_expand_label` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: State gecisi veya transcript hatasi guven modeli kirar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Tipli handshake state ve explicit key schedule adimlari.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

---

## Vaka Kumesi 19 - QUIC frame parser ve ACK guard

### Vaka 271 - QUIC frame parser ve ACK guard / senaryo 1

- Senaryo: `src/net/quic.rs` icinde `encode_varint` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Parser limitsizligi memory amplification yapar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: ACK range limiti ve frame decode guardlari.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 272 - QUIC frame parser ve ACK guard / senaryo 2

- Senaryo: `src/net/quic.rs` icinde `decode_varint` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Parser limitsizligi memory amplification yapar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: ACK range limiti ve frame decode guardlari.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 273 - QUIC frame parser ve ACK guard / senaryo 3

- Senaryo: `src/net/quic.rs` icinde `decode` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Parser limitsizligi memory amplification yapar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: ACK range limiti ve frame decode guardlari.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 274 - QUIC frame parser ve ACK guard / senaryo 4

- Senaryo: `src/net/quic.rs` icinde `MAX_ACK_RANGES` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Parser limitsizligi memory amplification yapar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: ACK range limiti ve frame decode guardlari.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 275 - QUIC frame parser ve ACK guard / senaryo 5

- Senaryo: `src/net/quic.rs` icinde `encode_varint` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Parser limitsizligi memory amplification yapar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: ACK range limiti ve frame decode guardlari.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 276 - QUIC frame parser ve ACK guard / senaryo 6

- Senaryo: `src/net/quic.rs` icinde `decode_varint` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Parser limitsizligi memory amplification yapar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: ACK range limiti ve frame decode guardlari.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 277 - QUIC frame parser ve ACK guard / senaryo 7

- Senaryo: `src/net/quic.rs` icinde `decode` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Parser limitsizligi memory amplification yapar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: ACK range limiti ve frame decode guardlari.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 278 - QUIC frame parser ve ACK guard / senaryo 8

- Senaryo: `src/net/quic.rs` icinde `MAX_ACK_RANGES` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Parser limitsizligi memory amplification yapar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: ACK range limiti ve frame decode guardlari.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 279 - QUIC frame parser ve ACK guard / senaryo 9

- Senaryo: `src/net/quic.rs` icinde `encode_varint` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Parser limitsizligi memory amplification yapar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: ACK range limiti ve frame decode guardlari.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 280 - QUIC frame parser ve ACK guard / senaryo 10

- Senaryo: `src/net/quic.rs` icinde `decode_varint` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Parser limitsizligi memory amplification yapar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: ACK range limiti ve frame decode guardlari.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 281 - QUIC frame parser ve ACK guard / senaryo 11

- Senaryo: `src/net/quic.rs` icinde `decode` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Parser limitsizligi memory amplification yapar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: ACK range limiti ve frame decode guardlari.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 282 - QUIC frame parser ve ACK guard / senaryo 12

- Senaryo: `src/net/quic.rs` icinde `MAX_ACK_RANGES` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Parser limitsizligi memory amplification yapar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: ACK range limiti ve frame decode guardlari.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 283 - QUIC frame parser ve ACK guard / senaryo 13

- Senaryo: `src/net/quic.rs` icinde `encode_varint` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Parser limitsizligi memory amplification yapar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: ACK range limiti ve frame decode guardlari.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 284 - QUIC frame parser ve ACK guard / senaryo 14

- Senaryo: `src/net/quic.rs` icinde `decode_varint` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Parser limitsizligi memory amplification yapar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: ACK range limiti ve frame decode guardlari.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 285 - QUIC frame parser ve ACK guard / senaryo 15

- Senaryo: `src/net/quic.rs` icinde `decode` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Parser limitsizligi memory amplification yapar.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: ACK range limiti ve frame decode guardlari.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

---

## Vaka Kumesi 20 - WireGuard handshake, nonce ve replay koruma

### Vaka 286 - WireGuard handshake, nonce ve replay koruma / senaryo 1

- Senaryo: `src/net/wireguard.rs` icinde `initiate_handshake` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Nonce tekrarinda replay kabul riski.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Monoton nonce kontrolu ve session state.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 287 - WireGuard handshake, nonce ve replay koruma / senaryo 2

- Senaryo: `src/net/wireguard.rs` icinde `encrypt_packet` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Nonce tekrarinda replay kabul riski.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Monoton nonce kontrolu ve session state.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 288 - WireGuard handshake, nonce ve replay koruma / senaryo 3

- Senaryo: `src/net/wireguard.rs` icinde `decrypt_packet` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Nonce tekrarinda replay kabul riski.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Monoton nonce kontrolu ve session state.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 289 - WireGuard handshake, nonce ve replay koruma / senaryo 4

- Senaryo: `src/net/wireguard.rs` icinde `is_allowed_ip` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Nonce tekrarinda replay kabul riski.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Monoton nonce kontrolu ve session state.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 290 - WireGuard handshake, nonce ve replay koruma / senaryo 5

- Senaryo: `src/net/wireguard.rs` icinde `initiate_handshake` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Nonce tekrarinda replay kabul riski.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Monoton nonce kontrolu ve session state.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 291 - WireGuard handshake, nonce ve replay koruma / senaryo 6

- Senaryo: `src/net/wireguard.rs` icinde `encrypt_packet` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Nonce tekrarinda replay kabul riski.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Monoton nonce kontrolu ve session state.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 292 - WireGuard handshake, nonce ve replay koruma / senaryo 7

- Senaryo: `src/net/wireguard.rs` icinde `decrypt_packet` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Nonce tekrarinda replay kabul riski.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Monoton nonce kontrolu ve session state.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 293 - WireGuard handshake, nonce ve replay koruma / senaryo 8

- Senaryo: `src/net/wireguard.rs` icinde `is_allowed_ip` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Nonce tekrarinda replay kabul riski.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Monoton nonce kontrolu ve session state.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 294 - WireGuard handshake, nonce ve replay koruma / senaryo 9

- Senaryo: `src/net/wireguard.rs` icinde `initiate_handshake` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Nonce tekrarinda replay kabul riski.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Monoton nonce kontrolu ve session state.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 295 - WireGuard handshake, nonce ve replay koruma / senaryo 10

- Senaryo: `src/net/wireguard.rs` icinde `encrypt_packet` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Nonce tekrarinda replay kabul riski.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Monoton nonce kontrolu ve session state.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 296 - WireGuard handshake, nonce ve replay koruma / senaryo 11

- Senaryo: `src/net/wireguard.rs` icinde `decrypt_packet` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Nonce tekrarinda replay kabul riski.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Monoton nonce kontrolu ve session state.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 297 - WireGuard handshake, nonce ve replay koruma / senaryo 12

- Senaryo: `src/net/wireguard.rs` icinde `is_allowed_ip` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Nonce tekrarinda replay kabul riski.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Monoton nonce kontrolu ve session state.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 298 - WireGuard handshake, nonce ve replay koruma / senaryo 13

- Senaryo: `src/net/wireguard.rs` icinde `initiate_handshake` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Nonce tekrarinda replay kabul riski.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Monoton nonce kontrolu ve session state.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 299 - WireGuard handshake, nonce ve replay koruma / senaryo 14

- Senaryo: `src/net/wireguard.rs` icinde `encrypt_packet` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Nonce tekrarinda replay kabul riski.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Monoton nonce kontrolu ve session state.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 300 - WireGuard handshake, nonce ve replay koruma / senaryo 15

- Senaryo: `src/net/wireguard.rs` icinde `decrypt_packet` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: Nonce tekrarinda replay kabul riski.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: Monoton nonce kontrolu ve session state.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

---

## Vaka Kumesi 21 - HPACK Huffman decode fail-closed modeli

### Vaka 301 - HPACK Huffman decode fail-closed modeli / senaryo 1

- Senaryo: `src/net/http2_huffman.rs` icinde `decode_huffman` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: EOS/padding hatalari parser acigi uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: InvalidPadding ve EosInString ile fail-closed cikis.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 302 - HPACK Huffman decode fail-closed modeli / senaryo 2

- Senaryo: `src/net/http2_huffman.rs` icinde `BitIterator` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: EOS/padding hatalari parser acigi uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: InvalidPadding ve EosInString ile fail-closed cikis.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 303 - HPACK Huffman decode fail-closed modeli / senaryo 3

- Senaryo: `src/net/http2_huffman.rs` icinde `InvalidPadding` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: EOS/padding hatalari parser acigi uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: InvalidPadding ve EosInString ile fail-closed cikis.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 304 - HPACK Huffman decode fail-closed modeli / senaryo 4

- Senaryo: `src/net/http2_huffman.rs` icinde `EosInString` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: EOS/padding hatalari parser acigi uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: InvalidPadding ve EosInString ile fail-closed cikis.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 305 - HPACK Huffman decode fail-closed modeli / senaryo 5

- Senaryo: `src/net/http2_huffman.rs` icinde `decode_huffman` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: EOS/padding hatalari parser acigi uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: InvalidPadding ve EosInString ile fail-closed cikis.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 306 - HPACK Huffman decode fail-closed modeli / senaryo 6

- Senaryo: `src/net/http2_huffman.rs` icinde `BitIterator` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: EOS/padding hatalari parser acigi uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: InvalidPadding ve EosInString ile fail-closed cikis.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 307 - HPACK Huffman decode fail-closed modeli / senaryo 7

- Senaryo: `src/net/http2_huffman.rs` icinde `InvalidPadding` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: EOS/padding hatalari parser acigi uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: InvalidPadding ve EosInString ile fail-closed cikis.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 308 - HPACK Huffman decode fail-closed modeli / senaryo 8

- Senaryo: `src/net/http2_huffman.rs` icinde `EosInString` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: EOS/padding hatalari parser acigi uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: InvalidPadding ve EosInString ile fail-closed cikis.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 309 - HPACK Huffman decode fail-closed modeli / senaryo 9

- Senaryo: `src/net/http2_huffman.rs` icinde `decode_huffman` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: EOS/padding hatalari parser acigi uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: InvalidPadding ve EosInString ile fail-closed cikis.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 310 - HPACK Huffman decode fail-closed modeli / senaryo 10

- Senaryo: `src/net/http2_huffman.rs` icinde `BitIterator` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: EOS/padding hatalari parser acigi uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: InvalidPadding ve EosInString ile fail-closed cikis.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 311 - HPACK Huffman decode fail-closed modeli / senaryo 11

- Senaryo: `src/net/http2_huffman.rs` icinde `InvalidPadding` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: EOS/padding hatalari parser acigi uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: InvalidPadding ve EosInString ile fail-closed cikis.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 312 - HPACK Huffman decode fail-closed modeli / senaryo 12

- Senaryo: `src/net/http2_huffman.rs` icinde `EosInString` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: EOS/padding hatalari parser acigi uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: InvalidPadding ve EosInString ile fail-closed cikis.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 313 - HPACK Huffman decode fail-closed modeli / senaryo 13

- Senaryo: `src/net/http2_huffman.rs` icinde `decode_huffman` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: EOS/padding hatalari parser acigi uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: InvalidPadding ve EosInString ile fail-closed cikis.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 314 - HPACK Huffman decode fail-closed modeli / senaryo 14

- Senaryo: `src/net/http2_huffman.rs` icinde `BitIterator` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: EOS/padding hatalari parser acigi uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: InvalidPadding ve EosInString ile fail-closed cikis.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

### Vaka 315 - HPACK Huffman decode fail-closed modeli / senaryo 15

- Senaryo: `src/net/http2_huffman.rs` icinde `InvalidPadding` etrafinda yuksek yuk altinda anlik performans dususu goruluyor.
- Belirti: Ortalama metrik iyi ama p99/p999 gecikmede sert artis var.
- Muhtemel kok neden: EOS/padding hatalari parser acigi uretir.
- Inceleme adimi 1: ilgili state degiskenlerini ve atomik publication sinirlarini cikar.
- Inceleme adimi 2: basarili ve basarisiz yol akisini iki ayri tabloya dok.
- Inceleme adimi 3: pressure/queue/load metriklerini zaman ekseninde eslestir.
- Cozum yaklasimi: InvalidPadding ve EosInString ile fail-closed cikis.
- Dogrulama: Ayni workload ile A/B karsilastirmasi yap, p99 ve hata sayacini raporla.
- Son not: Cozumun yan etkisini (CPU maliyeti, bellek maliyeti, kod karmasikligi) ayrica yaz.

---
