# ADR-001: İki Katmanlı Sürücü Kast Sistemi (Two-Tier Driver Caste System)

**Tarih**: 2025-01  
**Durum**: Kabul Edildi  
**Kararı Veren**: Çekirdek Mimarisi  

## Bağlam

Geleneksel monolitik çekirdeklerde tüm sürücüler aynı güven seviyesinde (Ring 0)
çalışır. Bir hatalı sürücü tüm sistemi çökertebilir. Mikro çekirdekler bunu
kullanıcı-alanı sürücülerle çözer ama performans kaybı ciddidir.

echOS, kritik I/O yolu (NVMe, NIC, GPU) için **sıfır soyutlama maliyetine**
ihtiyaç duyarken, ikincil sürücüler (USB, WiFi, Audio, Bluetooth) için
**çökme izolasyonu** gerektirir.

## Karar

İki katmanlı bir sürücü modeli benimsiyoruz:

### TIER 1 — Native Lock-Free Drivers
- **Kapsam**: NVMe, Ethernet NIC, GPU
- **Çalışma Modeli**: Doğrudan MMIO, async trait, sıfır Mutex
- **Güven**: Tam güvenilir (çekirdek alanında)
- **Performans**: Mikrosaniye altı latency
- **Doğrulama**: `grep -r "Mutex" nvme.rs nic_native.rs gpu_native.rs` → **0 sonuç**

### TIER 2 — Jail Sandbox Drivers
- **Kapsam**: USB, WiFi, Audio, Bluetooth, I2C
- **Çalışma Modeli**: SPSC ring buffer üzerinden izole worker thread
- **Güven**: Güvenilmez (crash izolasyonu var)
- **Performans**: ~1000-5000 cycle overhead (kabul edilebilir)
- **Test**: 1000 jail crash → core kernel sağlam

### Sınıflandırma
```
PCI class/subclass → tier.rs::classify_device() → Tier1 | Tier2
```
Runtime override ile dispatcher.rs üzerinden tier değiştirilebilir.

## Sonuçlar

**Olumlu:**
- Kritik I/O yolu için bare-metal performans
- İkincil sürücü crashleri sistemi çökertmez
- Her katman için özelleştirilmiş test stratejisi

**Olumsuz:**
- İki ayrı sürücü API'si bakım yükü
- TIER 1 sürücüleri çok dikkatli yazılmalı (crash = kernel panic)
- SPSC ring overhead bazı kullanım durumlarında hissedilebilir

## İlgili Dosyalar
- `src/drivers/tier.rs` — PCI class sınıflandırıcı
- `src/drivers/dispatcher.rs` — Otomatik tier yönlendirme
- `src/drivers/jail_ring.rs` — SPSC ring buffer
- `src/drivers/jail.rs` — Jail worker thread
