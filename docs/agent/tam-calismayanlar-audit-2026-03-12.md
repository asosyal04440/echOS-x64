# echOS Tam Calismayanlar Audit

Tarih: 2026-03-12

Bu rapor `src` agaci ve son basarili QEMU serial logu uzerinden hazirlandi.
Amac, echOS icinde calismayan, sahte basari ureten, fallback ile ayakta duran
veya acikca `TODO/stub/placeholder/not implemented` olarak birakilmis yollarin
karar verilebilir bir dokumunu vermektir.

## Kisa Sonuc

- Repo marker taramasi `src` altinda 629 adet dogrudan `TODO/stub/placeholder/simulated/not implemented` izi verdi.
- `echos_gate -Paths src` su ek riskleri raporladi:
  - `std-leakage`: 3
  - `panic-sites`: 7
  - `handwave-lexicon`: 202
  - `atomic-ordering-review`: 2344
  - `alignment-review`: 2102
- En yuksek riskli yari-calisan kumeler:
  - network
  - win32 / pe loader
  - usb / virtio ffi ve bazi suruculer
  - btrfs / vfs / inotify
  - shell scripting ve shell UX
  - debug / package / spdk / topology

## Kanitli Eksikler

### 1. Network

| Subsystem | Durum | Kanit | Etkisi | Gercek sinir |
|---|---|---|---|---|
| `src/net/smoltcp_driver.rs` | `Stubbed` | `dhcp_configure()` icinde `TODO: smoltcp DHCP client kullan`; `dns_lookup()` sadece `localhost` ve `gateway`; `tcp_connect()` log olarak `TCP connect not implemented`; `http_get()` `HTTP not implemented` donuyor | Shell'de gorunen `net/dns/http` yuzeyi gercek L3/L4/L7 davranisini kanitlamiyor | Tasiyici var, ama DHCP/DNS/TCP/HTTP cekirdegi tam degil |
| `src/net/http.rs` | `Partial` | `HttpError::TlsNotSupported`; `current_url.is_https()` ise aninda `TlsNotSupported` | `http`, `curl`, `wget` yalnizca plain HTTP ile sinirli | HTTPS/TLS yok |
| `src/net/doh.rs` | `Stubbed` | `DohError::HttpsNotSupported // TLS/HTTPS henuz desteklenmiyor` | DNS-over-HTTPS yok | Sadece API iskeleti var |
| `src/net/dot.rs` | `Stubbed` | `DotError::TlsNotSupported // rustls/mbedtls entegrasyonu TODO` | DNS-over-TLS yok | TLS katmani eksik |
| `src/net/netdev.rs` | `Stubbed` | dosya acikca `stub` diyor; loopback init logu da `stub` | Ag aygiti soyutlamasi urun seviyesi degil | Netdev contract tam degil |
| `src/net/grpc.rs` | `Simulated` | `TODO: Send frames through Http2Connection`; `For now, just simulate success` | gRPC basarili gorunup gercekte cagrilmayabilir | HTTP/2 tasima eksik |
| `src/net/http3.rs` | `Partial` | `HPACK encoding` placeholder | HTTP/3 / header compression eksik | Tam wire uyumu yok |
| `src/net/ipv6.rs` | `Partial` | dosya icinde `taslak (placeholder)` ve gercek arayuze gonderim TODO | IPv6 stack var gibi gorunse de tam transmit yolu yok | Taslak duzey |
| `src/net/cni.rs` | `Stubbed` | `Parsing config (placeholder)` | CNI config parse gercek degil | Orkestrasyon/bridge tarafi yarim |
| `src/net/ebpf.rs` | `Simulated` | JIT, context access, trace print, ELF load placeholder | eBPF calisiyor izlenimi verebilir ama urun seviyesi degil | Gosterim/iskelet agirlikli |

### 2. Win32 / PE

| Subsystem | Durum | Kanit | Etkisi | Gercek sinir |
|---|---|---|---|---|
| `src/win32.rs` | `Partial` | Gercek islenen bazi API'ler var (`VirtualAlloc`, `ReadFile`, `WriteFile`, `CreateProcessA`, `TerminateProcess`) ama genel fallback `_ => stub_api` | Win32 yuzeyi kismen gercek, buyuk bolumu stub | Genis API kapsami yok |
| `src/win32.rs` | `Stubbed` | satir 7397+ bolgesinde cok buyuk sayida `kernel32/gdi32/advapi32/shell32/msvcrt` sembolu `stub_api`'ye mapleniyor | Windows binary uyumlulugu ciddi bicimde yari-sahte | Bazi demo path'ler disinda genis Win32 ABI yok |
| `src/pe_loader.rs` | `Partial` | `stub_api` donen import'lar basarisiz kabul ediliyor | PE yukleme var ama import resolution gercek kapsama dayanmiyor | Win32 surface eksikligi PE yolunu sinirliyor |
| `src/win32_abi.rs` | `Partial` | `stub_api` ile iliskili kontrol yollarina bagli | ABI katmani tam degil | Import resolution guvenilir degil |

### 3. Drivers

| Subsystem | Durum | Kanit | Etkisi | Gercek sinir |
|---|---|---|---|---|
| `src/drivers/virtio_ffi.rs` | `Stubbed` | dosya acikca `C tabanli surucu stublara baglayan kopru`; `virt_to_phys_c` panic edebiliyor; gercek C arka uc yok | VirtIO block FFI yolu urun seviyesi degil | C backend eksik |
| `src/drivers/usb/mod.rs` | `Simulated` | command ring physical address icin `placeholder`, slot id `simulated`, transfer ring phys addr `simulated` | xHCI/USB enumeration gercek donanim davranisina tam dayanmiyor | Core USB bring-up yarim |
| `src/drivers/usb/cdc.rs` | `Partial` | `SET_LINE_CODING`, `SET_CONTROL_LINE_STATE`, `USB bulk out` TODO | USB serial control/data yolu eksik | TX/RX tam degil |
| `src/drivers/usb/hid.rs` | `Partial` | `Kontrol aktarimi gonder (TODO: gercek uygulama)` | HID init/feature path eksik olabilir | Kontrol transferi tamam degil |
| `src/drivers/usb/mass_storage.rs` | `Partial` | kontrol aktarimi ve 1 byte oku TODO | USB mass storage tam degil | BOT/CSW gibi yollarda acik bosluk var |
| `src/drivers/nic_native.rs` | `Partial` | promiscuous mode TODO | NIC ozellik seti eksik | Feature completeness yok |
| `src/drivers/ahci.rs` | `Partial` | IDENTIFY command ile ogren TODO | AHCI metadata/identify tam degil | Drive info yolu eksik |

### 4. Filesystem

| Subsystem | Durum | Kanit | Etkisi | Gercek sinir |
|---|---|---|---|---|
| `src/btrfs.rs` | `Stubbed` | superblock read/write, root tree load, inode load placeholder loglari | Btrfs gercek dosya sistemi olarak guvenilir degil | Struct dolumu var, disk gercegi yok |
| `src/fs/vfs_unified.rs` | `Partial` | fallback stub kapasite degerleri; diger FS'ler icin `Ok(Vec::new())` | VFS unified info/okuma yollari yaniltici olabilir | FS turune gore bos veya heuristik donuyor |
| `src/fs/inotify.rs` | `Partial` | inode path'ten `placeholder` hash ile uretiliyor | Gercek inode takibi yerine sahte kimlik kullaniliyor | File watch semantics zayif |
| `src/fs/f2fs.rs` | `Partial` | `LZO not implemented, use LZ4` | Bazi compression kombinasyonlari eksik | Tam format kapsami yok |

### 5. Shell

| Subsystem | Durum | Kanit | Etkisi | Gercek sinir |
|---|---|---|---|---|
| `src/shell/mod.rs` | `Misleading UX` | `net status` tasiyici hazir gosterebilir ama DHCP fallback; `dns` kisitli; `ping` hala gercek ICMP degil | Kullanici agin tam calistigini sanabilir | Shell komutlari cekirdek ag gerceginden daha parlak gorunebilir |
| `src/shell/mod.rs` | `Partial` | RTC tarih, gercek memory info, gercek disk info TODO | Sistem komutlari tam telemetri vermiyor | Cikti kismi |
| `src/shell/mod.rs` | `Stubbed` | `append: TODO - f2fs append destegi gerekli` | Shell file append eksik | Dosya islemleri tam degil |
| `src/shell/mod.rs` | `Partial` | pipeline redirect yorumunda stdout append/stdin TODO | Pipe/redirect yuzeyi eksik | Shell redirection tam degil |
| `src/shell/scripting.rs` | `Stubbed` | `Command` icin `TODO: gercek komut calistirma`; `CommandSub` TODO | Script engine komut calistirma ve command substitution tarafinda yari yolda | Tam shell scripting yok |
| `src/shell/advanced.rs` | `Partial` | PATH'teki executable'lari ekle TODO | Completion/discovery eksik | PATH resolution tam degil |

### 6. Debug / Security / Storage / Topology

| Subsystem | Durum | Kanit | Etkisi | Gercek sinir |
|---|---|---|---|---|
| `src/debug/mod.rs` | `Stubbed` | dosya acikca butun fonksiyonlar `stub`; Ring3/VM/IRQ testleri sadece log yaziyor | Test/hata ayiklama altyapisi calisiyor gibi gorunse de derin degil | Smoke-level log var, gercek test yok |
| `src/security/package.rs` | `Partial` | remote package update TODO; tar.gz extract TODO | Paket yonetimi imza dogrulasa bile dagitim/kurulum yolu eksik | Uzak depo ve payload extract yarim |
| `src/spdk.rs` | `Simulated` | PCI config, TCP read/write, RDMA read/write `simulate` | SPDK/NVMe-oF performans iddialari gercek donanim yolu degil | Modelleme/simulasyon agirlikli |
| `src/topology.rs` | `Partial` | `redetect_topology()` `NotImplemented` donuyor | Hotplug/topology refresh eksik | Ilk detect var, yeniden algilama yok |

## Yaniltici Calisiyor Gorunenler

### Network UX

- `src/shell/mod.rs`
  - `net status`: VirtIO-Net transport init ile "hazir" diyebilir, ama bu sadece tasiyici seviyesini kanitlar
  - `net dhcp`: gercek DHCP lease yerine `10.0.2.15/10.0.2.2/10.0.2.3` fallback uygular
  - `dns`: artik daha durust ama tam DNS istemcisi degil
  - `ping`: gercek ICMP yolu yok; daha once sahte basari uretiyordu, halen tam kanit araci degil
- `src/net/http.rs`
  - plain HTTP istemci var, ama HTTPS/TLS yok; kullanici `curl/wget/http` yuzeyini tam web stack sanabilir

### Win32 UX

- `src/win32.rs`
  - bazi gercek API implementasyonlari oldugu icin sistem daha tam gorunuyor
  - fakat import surface'in buyuk kismi `stub_api`
  - sonuc: PE dosyasi yuklenebilir ama anlamli runtime uyumlulugu garanti degil

### VFS / FS UX

- `src/fs/vfs_unified.rs`
  - unsupported veya diger FS'ler icin bos veri veya heuristik kapasite donduruyor
  - sonuc: "okundu" gibi gorunen bilgi aslinda conservative fallback olabilir

### Debug/Test UX

- `src/debug/mod.rs`
  - self-check ve stress test isimleri var
  - ama cogu yalnizca serial log stub
  - sonuc: test kapsami oldugundan daha guclu gorunebilir

## Yuksek Riskli Kismi Sistemler

### Network

- Risk: transport init var, ama host-level HTTP/TCP/TLS gercegi parcalanmis
- Sonuc: "ag var" ile "ag stack urun seviyesi" birbirine karisiyor
- En kritik dosyalar:
  - `src/net/smoltcp_driver.rs`
  - `src/net/http.rs`
  - `src/net/doh.rs`
  - `src/net/dot.rs`
  - `src/net/netdev.rs`

### Win32 / PE

- Risk: birkac API gercek oldugu icin kapsama oldugundan buyuk saniliyor
- Sonuc: binary uyumluluk beklentisi kolayca yanlis kuruluyor
- En kritik dosyalar:
  - `src/win32.rs`
  - `src/win32_abi.rs`
  - `src/pe_loader.rs`

### USB / VirtIO FFI

- Risk: simulated slot id, placeholder physical address, stub FFI
- Sonuc: donanim bring-up mantigi var gibi gorunse de urun seviyesi degil
- En kritik dosyalar:
  - `src/drivers/usb/mod.rs`
  - `src/drivers/usb/cdc.rs`
  - `src/drivers/usb/hid.rs`
  - `src/drivers/usb/mass_storage.rs`
  - `src/drivers/virtio_ffi.rs`

### Filesystem

- Risk: Btrfs ve unified VFS fallback yollari sessizce heuristik veya bos donuyor
- Sonuc: storage feature matrix gercekte oldugundan daha genis algilanabilir
- En kritik dosyalar:
  - `src/btrfs.rs`
  - `src/fs/vfs_unified.rs`
  - `src/fs/inotify.rs`

### Shell / Scripting

- Risk: shell komut kapsami UX tarafinda fazla iddiali
- Sonuc: kullanici urun seviyesinde POSIX/network davranisi bekleyebilir
- En kritik dosyalar:
  - `src/shell/mod.rs`
  - `src/shell/scripting.rs`
  - `src/shell/advanced.rs`

## Dogrulanmis Calisan Taban

Asagidaki maddeler "tam sistem hazir" anlamina gelmez; yalnizca belirtilen kapsamda
mekanik olarak dogrulanmis yol olduklarini gosterir.

| Subsystem | Durum | Kanit | Etkisi | Gercek sinir |
|---|---|---|---|---|
| QEMU boot zinciri | `Verified Working` | `logs/serial_20260312_004357.log` icinde `[BOOTCTRL] stage=desktop-ready` ve `[BOOTCTRL] success` | Sistem QEMU appliance olarak boot ediyor | Bu, tum subsistemlerin urun seviyesi oldugu anlamina gelmez |
| VirtIO-Net transport init | `Verified Working` | ayni logda `[VIRTIO-NET] Transport created successfully` | VirtIO-Net tasiyici bring-up gercekten denenmis | Ust seviye TCP/DNS/HTTP completeness ayri konu |
| SMP/AP startup | `Verified Working` | ayni logda AP1/AP2/AP3 startup ve `4/4 CPUs online` | SMP startup path en az bu QEMU profilinde ayaga kalkiyor | Scheduler/topology hotplug completeness ayri konu |

## Onceliklendirilmis Duzeltme Sirasi

1. Network truthfulness ve gercek stack
   - `smoltcp_driver` stub'larini ya gerceklestir ya da shell yuzeyini daha da daralt
2. Win32 truthfulness
   - `stub_api` haritasini capability matrix ile belgeleyip user-facing claims'i daralt
3. USB / virtio ffi
   - simulated fiziksel adres ve slot id yollarini gercek donanim sozlesmesine cek
4. Filesystem
   - Btrfs / unified VFS fallback'lerini sessiz heuristic olmaktan cikar
5. Shell scripting
   - command execution ve command substitution'u gercek shell runtime ile bagla
6. Debug/security/spdk
   - stub test/remote package/SPDK simulate yollarini capability bayraklariyla acik et

## Appendix A — Audit Kapsaminda Mutlaka Izlenmesi Gereken Dosyalar

### Network
- `src/net/smoltcp_driver.rs`
- `src/net/http.rs`
- `src/net/doh.rs`
- `src/net/dot.rs`
- `src/net/netdev.rs`
- `src/net/grpc.rs`
- `src/net/http3.rs`
- `src/net/ipv6.rs`
- `src/net/cni.rs`
- `src/net/ebpf.rs`
- `src/net/netfilter.rs`

### Win32 / PE
- `src/win32.rs`
- `src/win32_abi.rs`
- `src/pe_loader.rs`

### Drivers
- `src/drivers/virtio_ffi.rs`
- `src/drivers/usb/mod.rs`
- `src/drivers/usb/cdc.rs`
- `src/drivers/usb/hid.rs`
- `src/drivers/usb/mass_storage.rs`
- `src/drivers/nic_native.rs`
- `src/drivers/ahci.rs`

### Filesystem
- `src/btrfs.rs`
- `src/fs/vfs_unified.rs`
- `src/fs/inotify.rs`
- `src/fs/f2fs.rs`

### Shell
- `src/shell/mod.rs`
- `src/shell/scripting.rs`
- `src/shell/advanced.rs`

### Debug / Security / Other
- `src/debug/mod.rs`
- `src/security/package.rs`
- `src/spdk.rs`
- `src/topology.rs`
