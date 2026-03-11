# echOS Architecture Decision Records (ADR)

Bu dizin, echOS çekirdeğinin tasarım kararlarını ve gerekçelerini belgelemek için
Mimari Karar Kayıtlarını (ADR) içerir.

## ADR Listesi

| No | Başlık | Tarih | Durum |
|----|--------|-------|-------|
| 001 | [İki Katmanlı Sürücü Kast Sistemi](001-two-tier-driver-caste.md) | 2025-01 | Kabul Edildi |
| 002 | [Lock-Free SPSC Ring Buffer](002-lock-free-spsc-ring.md) | 2025-01 | Kabul Edildi |
| 003 | [io_uring Kernel I/O Arayüzü](003-io-uring-interface.md) | 2025-02 | Kabul Edildi |
| 004 | [UEFI Boot + HHDM](004-uefi-boot-hhdm.md) | 2025-01 | Kabul Edildi |
| 005 | [KASLR + Manifest Signing](005-kaslr-manifest-signing.md) | 2025-06 | Kabul Edildi |
| 006 | [Trait Freeze ve API Stabilizasyonu](006-trait-freeze-api.md) | 2025-06 | Kabul Edildi |

## ADR Formatı

Her ADR şu bölümleri içerir:
- **Durum**: Önerilen / Kabul Edildi / Reddedildi / Kullanım Dışı
- **Bağlam**: Problemi tanımlayan arka plan
- **Karar**: Ne yapılmasına karar verildi
- **Sonuçlar**: Kararın etkileri
