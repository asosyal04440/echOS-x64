# Ölü Kod Raporu

Tarih: 2026-04-17

Kapsam: `src/`, `Cargo.toml`, runtime compatibility yüzeyleri, açık `#[allow(dead_code)]` adaları, `#[deprecated]` API'ler, `legacy`/`retired` işaretleri ve `cargo check --target x86_64-pc-windows-msvc --lib -q` çıktısı.

## Taarruz Planı

1. Sahiplik sınırı: Bu rapor kod silmez; canlı ABI, fallback, test oracle veya migration bridge olabilecek alanları ayrı sınıfa alır.
2. Eşzamanlılık seçimi: Runtime davranışına dokunulmadı; rapor sadece statik tarama ve derleme sinyali üretir.
3. ABI / donanım temas noktası: PE/Win32, NVMe, PCI, IPsec ve runtime-layer compatibility yolları doğrudan silme adayı değil; önce çağrı grafiği ve feature politikası doğrulanmalı.
4. Cache / hot-path stratejisi: Hot-path yorum kalıntıları ve compatibility branch'leri ayrı işaretlendi; temizleme yapılırsa performans karşılaştırması gerekir.
5. Doğrulama: `rg` ile mekanik tarama ve `cargo check` ile derleme doğrulandı. Crate seviyesinde `dead_code` kapalı olduğu için bu rapor kesin silme listesi değil, audit başlangıç listesidir.

## En Kritik Bulgular

### 1. Crate seviyesinde ölü kod uyarısı kapatılmış

Kanıt:

- `src/lib.rs:27` -> `#![allow(dead_code)]`
- `src/lib.rs:32..36` -> unused lint'leri crate genelinde yumuşatılmış
- `src/main.rs:16..17` -> bazı lint sınıfları crate girişinde bastırılmış

Etkisi: Rust derleyicisi normalde kullanılmayan private item'ları yakalayabilir; fakat `#![allow(dead_code)]` bunu susturuyor. Ayrıca çok sayıda `pub mod` dışa açık olduğu için reachability analizi daha da zayıflıyor.

Karar: Bu satır tek başına en büyük ölü kod körlüğüdür. Silme işi buradan başlamamalı; önce audit modu eklenmeli.

Önerilen kapı:

- `dead_code_audit` isimli bir cargo feature veya CI job aç.
- Audit job içinde crate seviyesindeki `allow(dead_code)` kaldırılmış gibi kontrol et.
- İlk hedef `warn(dead_code)`; doğrudan `deny(dead_code)` bu repo için fazla sert olur.

### 2. Açık `#[allow(dead_code)]` adaları

Bu dosyalarda item bazlı ölü kod bastırması var:

| Dosya | Kanıt | Risk | Öneri |
| --- | --- | --- | --- |
| `src/cpu/acpi.rs` | `:61`, `:305`, `:307`, `:310`, `:312`, `:314`, `:317` | ACPI tablo alanları spec gereği tutuluyor olabilir | Her item için "spec field mi, okunmayan model mi?" ayrımı yap |
| `src/drivers/apic.rs` | `:32`, `:36`, `:38`, `:42`, `:115` | APIC register sabitleri gelecekteki init yolu için tutuluyor olabilir | Register sabitiyse `register map` bölümüne taşı; kullanılmıyorsa kes |
| `src/drivers/ata.rs` | `:43`, `:76`, `:79` | Eski ATA yolu NVMe odaklı sistemde gereksiz kalmış olabilir | Boot/fallback storage politikası doğrulanmadan silme |
| `src/drivers/ps2.rs` | `:91` | PS/2 fallback input olabilir | USB HID boot input hazır değilse canlı fallback say |
| `src/ipc/channel.rs` | `:66` | IPC ABI evrimi içinde eski alan olabilir | Call-site ve serialization formatı kontrol edilmeli |
| `src/task/worker.rs` | `:141` | Scheduler/worker telemetry veya test hook kalmış olabilir | Test-only ise `#[cfg(test)]` veya explicit diagnostic feature'a taşı |
| `src/memory/mod.rs` | `:276` | MM alanı gelecekteki allocator/VM için tutuluyor olabilir | Kullanılmıyorsa VM contract doc'una ya da koda bağla |
| `src/crypto/ed25519.rs` | `:86`, `:96` | Sabit/zamanlama veya test helper olabilir | Kripto test corpus'u ile doğrulamadan kesme |
| `src/elf.rs` | `:55`, `:158`, `:160` | ELF loader type surface'i yarım kalmış olabilir | PE/ELF loader hedefi netleşene kadar "loader debt" olarak izle |
| `src/posix/windows_image.rs` | `:846`, `:893` | Windows image uyumluluğu için ayrılmış metadata olabilir | EGO/PE nucleus planıyla birlikte yeniden sınıflandır |

Karar: Bunlar en net ölü kod adaylarıdır, ama bazıları donanım/spec model alanı olabilir. İlk temizlikte sadece gerçekten hiçbir call-site, test, serialization veya register-map rolü olmayan item'lar kesilmeli.

### 3. Runtime-layer legacy API yüzeyi

Kanıt:

- `src/runtime_layer/bootstrap_api.rs:9`, `:13`, `:17`, `:21`, `:27`, `:33`
- `src/runtime_layer/runtime_api.rs:10`, `:16`, `:23`, `:30`, `:36`, `:40`, `:44`
- `src/runtime_layer/service_api.rs:9`, `:18`, `:22`, `:26`, `:30`, `:34`

Etkisi: Bu API'ler zaten `#[deprecated]` ile işaretlenmiş. Bu iyi sinyal: kod sahibi burada migration niyetini belirtmiş. Fakat deprecated demek ölü demek değildir; kullanıcıları bitmeden kesilirse runtime compatibility kırılır.

Karar: "Güçlü temizlik adayı, hemen silme adayı değil."

Güvenli temizlik sırası:

1. `rg` ile bu sembollerin çağrı noktalarını çıkar.
2. Çağrı kalmadıysa bir release boyunca compile-time uyarı bırak.
3. Sonra API'yi değil, önce re-export veya compatibility alias'ını kaldır.
4. En son gerçek implementasyonu sil.

### 4. NVMe dosyasında emekli edilmiş async yol izleri

Kanıt:

- `src/drivers/nvme.rs:2220..2263`
- `src/drivers/nvme.rs:2291..2294`
- `src/drivers/nvme.rs:2307..2311`
- `src/drivers/nvme.rs:2328..2343`

Gözlem: Aynı yorum çok kez tekrar ediyor: `legacy async path retired by SQ/CQ doorbell flow`.

Etkisi: Bu, canlı koddan çok migration mezar taşı gibi görünüyor. Hot-path dosyada bu kadar tekrar eden emeklilik yorumu okunabilirliği düşürür.

Karar: Yüksek güvenli temizlik adayı. Eğer yorumların koruduğu bir invariant yoksa tek bir karar notuna indirilmeli veya `docs/agent/decision-log.md` içinde tutulmalı.

Risk: NVMe Tier-1 path olduğu için yalnızca yorum temizliği bile diff review ile yapılmalı; SQ/CQ doorbell akışını değiştiren kod temizlikle aynı PR'a karışmamalı.

### 5. Security package legacy install/signature/manifest yolu

Kanıt:

- `src/security/package.rs:334` -> legacy package install fallback çağrısı
- `src/security/package.rs:462` -> `install_legacy_package`
- `src/security/package.rs:723` -> legacy signature verification çağrısı
- `src/security/package.rs:733` -> `verify_legacy_signature`
- `src/security/package.rs:750` -> `parse_legacy_manifest`

Etkisi: İsimler legacy diyor, ama kod hala install fallback hattına bağlı görünüyor. Bu yüzden ölü kod sınıfına alınamaz.

Karar: Canlı compatibility/fallback debt. Silme ancak yeni package manifest formatı her test corpus'unda eski yolu gereksiz kıldığında yapılmalı.

Güvenli kapı:

- Legacy paket formatı için kaç test kaldığını say.
- Runtime telemetry'de legacy fallback kullanımını ölç.
- Kullanım sıfırlandıktan sonra legacy verifier'ı önce feature gate altına al.

### 6. IPC service compatibility bridge

Kanıt:

- `src/ipc/service_ipc/compat.rs:76` -> `request_sync_legacy`
- `src/ipc/service_ipc/compat.rs:82` -> legacy çağrı `request_sync_compat(..., "request_sync_legacy")` üzerinden izleniyor

Etkisi: Bu alan ölü değil; migration metriği olan bir compatibility köprüsü. İsmi eski olabilir, ama sayaç/telemetry ile geçiş izliyorsa canlıdır.

Karar: Silme adayı değil. Önce `migrated_legacy_sync_clear()` benzeri metric kapısı ile sıfır kullanım kanıtı toplanmalı.

### 7. IPsec legacy weak crypto feature

Kanıt:

- `Cargo.toml:182` -> `ipsec_legacy_weak_crypto = []`
- `src/net/ipsec.rs` içinde bu feature'a bağlı kabul/red dalları var.

Etkisi: Bu bilinçli güvenlik feature gate'i gibi duruyor. Ölü kod değil; riskli legacy davranışı explicit flag ile izole edilmiş.

Karar: Silme adayı değil. Varsayılan kapalı kaldığı sürece kabul edilebilir. Eğer proje güvenlik zincirinde zayıf kriptoya hiç izin vermeyecekse ayrı güvenlik kararıyla tamamen kaldırılmalı.

### 8. Derleme uyarılarında ölü kodla ilişkili hijyen sinyalleri

`cargo check --target x86_64-pc-windows-msvc --lib -q` başarıyla bitti, fakat çok sayıda uyarı üretti. Ölü kod raporu açısından önemli örnekler:

- `src/interrupts/mod.rs:1210` -> unreachable statement; bu doğrudan ölü yürütme yolu adayıdır.
- `src/ml/mod.rs:11` ve `src/audio/mod.rs:12` -> `#![no_std]` crate root dışında etkisiz attribute; niyet var ama yürürlük yok.
- `src/elf.rs:475` -> public API private `ElfHeader64` tipine bağlı; loader yüzeyinde tasarım borcu.
- `src/fs/f2fs.rs:5175` -> public API private `F2fsContext` tipine bağlı; export sınırı borcu.
- Birden fazla `unused Result` uyarısı var; ölü kod değil ama hata yolunun sessiz düşmesi anlamına gelir.

Karar: Bunlar doğrudan "sil" listesi değildir. Fakat ölü kod temizliği başlamadan önce reachability ve API görünürlük sınırlarını netleştirir.

## Silinmemesi Gereken "Legacy" Alanlar

Aşağıdaki alanlar isim olarak eski görünüyor ama canlı rol taşıyor olabilir:

| Alan | Neden hemen silinmez |
| --- | --- |
| `src/security/package.rs` legacy package path | Kurulum fallback hattına bağlı |
| `src/ipc/service_ipc/compat.rs` legacy sync bridge | Migration telemetry ve compatibility ölçümü var |
| `src/net/ipsec.rs` weak crypto feature | Bilinçli feature gate; güvenlik politikası kararı ister |
| `src/drivers/ata.rs` | Boot/fallback storage politikası doğrulanmalı |
| `src/drivers/ps2.rs` | USB HID yokken input fallback olabilir |
| ACPI/APIC register sabitleri | Spec model alanları kullanılmasa bile dokümante register map olabilir |

## İlk Temizlik Backlog'u

1. `src/lib.rs` için `dead_code_audit` build modu tasarla.
   - Çıktı: audit modunda crate-level `dead_code` bastırması devre dışı kalsın.
   - Doğrulama: `cargo check --target x86_64-pc-windows-msvc --lib` uyarı listesi üretmeli.

2. `src/interrupts/mod.rs:1210` unreachable statement için kontrol akışını incele.
   - Çıktı: Ya unreachable kod kaldırılır ya da önceki `return` koşulu düzeltilir.
   - Doğrulama: unreachable warning kaybolmalı.

3. `src/drivers/nvme.rs` içindeki tekrar eden retired yorumları tek karar notuna indir.
   - Çıktı: Hot-path dosyada tekrar eden mezar taşı yorumu kalmasın.
   - Doğrulama: NVMe davranış diff'i sıfır olmalı; sadece yorum/doc değişmeli.

4. `runtime_layer` deprecated API çağrı grafiğini çıkar.
   - Çıktı: Her deprecated API için caller listesi.
   - Doğrulama: Çağrısı kalmayan API'ler ayrı "remove candidate" listesine girsin.

5. `#[allow(dead_code)]` item'larını tek tek sınıflandır.
   - Çıktı: `spec-field`, `fallback`, `test-only`, `remove-candidate`, `public-contract` etiketlerinden biri.
   - Doğrulama: Etiketsiz `allow(dead_code)` kalmasın.

6. `src/ml/mod.rs` ve `src/audio/mod.rs` içindeki etkisiz `#![no_std]` attribute'larını düzelt.
   - Çıktı: Ya kaldırılır ya da modül seviyesine uygun gerçek no_std sınırı tanımlanır.
   - Doğrulama: unused attribute warning kaybolmalı.

7. `src/elf.rs` public/private API sınırını düzelt.
   - Çıktı: `parse_dynamic_section` ya `pub(crate)` olur ya da `ElfHeader64` görünürlüğü bilinçli yapılır.
   - Doğrulama: private interface warning kaybolmalı.

8. `src/fs/f2fs.rs` public/private API sınırını düzelt.
   - Çıktı: `read_block_cached` görünürlüğü ve `F2fsContext` görünürlüğü aynı contract'a çekilir.
   - Doğrulama: private interface warning kaybolmalı.

9. Security package legacy yoluna kullanım telemetry'si ekle veya mevcut telemetry'yi raporla.
   - Çıktı: Legacy manifest kaç kez kullanılıyor görülsün.
   - Doğrulama: test corpus'unda legacy fallback kullanımı sayısal raporlanmalı.

10. IPC legacy sync köprüsü için migration gate raporu üret.
    - Çıktı: `request_sync_legacy` kullanım sayacı release raporunda görünmeli.
    - Doğrulama: kullanım sıfırlanmadan silme yapılmamalı.

## Karar Matrisi

| Başlık | Durum | Gerekçe | Risk | Getiri |
| --- | --- | --- | --- | --- |
| Crate-level `allow(dead_code)` | Sonra kaldır; şimdi audit modu aç | Ani kaldırma çok geniş uyarı fırtınası üretir | CI gürültüsü | Gerçek ölü kod görünür olur |
| Item-level `#[allow(dead_code)]` | Şimdi sınıflandır | Sinyal doğrudan | Spec alanı yanlış kesilebilir | Temiz reachability |
| Runtime deprecated API | Araştır, sonra kes | Migration niyeti açık | Compatibility kırılması | Runtime yüzeyi küçülür |
| NVMe retired yorum blokları | Şimdi dokümantasyon temizliği | Kod davranışı değiştirmeden sadeleşir | Tier-1 diff karışırsa tehlikeli | Hot-path okunabilirliği |
| Security legacy package path | Şimdilik kesme | Fallback aktif görünüyor | Paket install kırılır | Uzun vadede güvenlik yüzeyi daralır |
| IPC legacy sync bridge | Şimdilik kesme | Telemetry/migration rolü var | Eski servisler kırılır | Migration bittiğinde API sadeleşir |
| IPsec weak crypto feature | Güvenlik kararıyla değerlendir | Feature gate bilinçli | Zayıf kripto policy sızıntısı | Policy netleşir |
| Unreachable code warning | Şimdi düzelt | Derleyici doğrudan gösteriyor | Yanlış kontrol akışı saklanıyor olabilir | Daha dürüst boot/interrupt init |
| Unused doc comments | Düşük öncelik | Davranışsal ölü kod değil | Gürültü | Uyarı bütçesi temizlenir |
| Private interface warnings | Şimdi düzelt | Public contract bulanık | API kullanıcısı kırılabilir | Modül sınırları netleşir |

## Claim Ledger

Proven:

- `cargo check --target x86_64-pc-windows-msvc --lib -q` exit code 0 ile tamamlandı.
- `src/lib.rs` crate seviyesinde `dead_code` ve unused sınıfı uyarıları bastırıyor.
- Yukarıdaki dosyalarda açık `#[allow(dead_code)]`, `#[deprecated]`, `legacy` veya `retired` sinyalleri bulundu.

Tested:

- `rg` ile `#[allow(dead_code)]`, `#[deprecated]`, `legacy`, `retired` ve ilgili feature gate tarandı.
- `cargo check` ile mevcut çalışma ağacının library target derlemesi kontrol edildi.

Inferred:

- NVMe'deki tekrar eden retired yorumları davranışsal koddan çok migration kalıntısı gibi görünüyor; kesin karar için ilgili fonksiyonların diff review'i gerekir.
- Security package legacy yolu canlı fallback gibi görünüyor; kesin kaldırma kararı için test corpus ve runtime telemetry gerekir.
- Runtime-layer deprecated API'ler temizlik adayıdır; çağrı grafiği sıfırlanmadan silinmemelidir.

## Önerilen Ana Hüküm

echOS'ta ölü kod temizliği önce `src/lib.rs` içindeki crate-level körlüğü audit moduna alarak başlamalı; hemen ardından `#[allow(dead_code)]` adaları sınıflandırılmalı, `runtime_layer` deprecated yüzeyi çağrı grafiğiyle daraltılmalı, NVMe retired yorum kalıntıları davranışsız doc temizliği olarak ayrılmalı, fakat security package legacy fallback, IPC compatibility bridge, IPsec weak crypto feature, ATA/PS2 fallback ve ACPI/APIC register map alanları kullanım kanıtı olmadan silinmemelidir.
