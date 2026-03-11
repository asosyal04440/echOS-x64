# ADR-006: Trait Freeze ve API Stabilizasyonu

**Tarih**: 2025-06  
**Durum**: Kabul Edildi  
**Kararı Veren**: Çekirdek Mimarisi  

## Bağlam

echOS, H22 itibarıyla 51 modül, 75+ shell komutu ve yüzlerce public API içerir.
Modüller arası bağımlılıklar artarak değişikliklerin yan etkilerini tahmin etmeyi
zorlaştırmaktadır. API stabilizasyonu olmadan üçüncü taraf sürücüler veya
kullanıcı alanı uygulamaları güvenli şekilde geliştirilemez.

## Karar

### Dondurulmuş (Frozen) Trait'ler

Aşağıdaki trait'ler **v1.0'dan itibaren değiştirilemez**:

| Trait | Modül | Açıklama |
|-------|-------|----------|
| `AsyncBlockDevice` | `drivers::async_traits` | Asenkron blok cihaz arayüzü |
| `AsyncNetDevice` | `drivers::async_traits` | Asenkron ağ cihaz arayüzü |
| `AsyncGpuDevice` | `drivers::async_traits` | Asenkron GPU cihaz arayüzü |
| `BlockDevice` | `drivers::block` | Senkron blok cihaz trait'i |
| `FileSystem` | `fs::vfs` | VFS dosya sistemi trait'i |
| `Allocator` (buddy/TLSF) | `allocator` | Bellek ayırıcı arayüzü |

### Stabilite Garantileri

1. **Frozen trait'lere metot eklenemez** (default impl hariç)  
2. **Frozen trait'lerin mevcut metot imzaları değiştirilemez**  
3. **Yeni trait'ler frozen trait'i genişletebilir** (extension trait pattern)  
4. **Struct alanları `#[non_exhaustive]` ile korunur**  

### Sürüm Politikası

- **Major (x.0.0)**: Frozen trait kırıcı değişiklik (sadece zorunlu güvenlik fix)
- **Minor (0.x.0)**: Yeni modül/özellik (geriye uyumlu)
- **Patch (0.0.x)**: Bug fix

### Syscall ABI Stabilizasyonu

| Syscall Nr | İsim | Stabilite |
|-----------|------|-----------|
| 0-63 | POSIX core (read, write, open, ...) | **Frozen** |
| 64-150 | POSIX extended (socket, mmap, ...) | **Frozen** |
| 151-299 | Linux compat (clone3, io_uring, ...) | **Stable** |
| 300-399 | echOS extensions | **Unstable** |
| 400+ | Debug/internal | **Unstable** |

## Sonuçlar

**Olumlu:**
- Üçüncü taraf sürücüler güven ile geliştirilebilir
- Geriye uyumluluk sözü
- Modüller arası bağımlılıklar açık/belirgin

**Olumsuz:**
- Hatalı API tasarımları düzeltmek zorlaşır
- Extension trait pattern ekstra karmaşıklık getirir

## İlgili Dosyalar
- `src/drivers/async_traits.rs` — Frozen async trait'ler
- `src/drivers/block.rs` — Frozen block device trait
- `src/fs/vfs.rs` — Frozen VFS trait
- `src/syscall.rs` — Syscall numaraları
