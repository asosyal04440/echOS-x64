# echOS TODO

Tarih: 2026-03-14

Bu dosya, echOS icin repo-kok backlog'udur.
Amac, "ne gercekten calisiyor", "ne davranissal olarak yari-acik", "ne hala fidelity/exactness kuyrugunda" ve
"hangi sirayla kapatilacak" sorularini tek yerde sabitlemektir.

## 2026-03-19 Urun Karari

- Network yuzeyi yeniden aktif urun hedefinde.
- `net`, `dns`, `ping`, `http`, `wget`, `curl` shell komutlari tekrar acik.
- Faz 1 ve Faz 3 historical degil; aktif urun backlog'u olarak kalir.

Kaynaklar:
- [docs/agent/tam-calismayanlar-audit-2026-03-12.md](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/docs/agent/tam-calismayanlar-audit-2026-03-12.md)
- [docs/agent/network-capability-matrix.md](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/docs/agent/network-capability-matrix.md)
- [docs/agent/win32-parity-matrix.md](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/docs/agent/win32-parity-matrix.md)
- [docs/agent/decision-log.md](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/docs/agent/decision-log.md)

## 0. Calisma Kurali

Bir is ancak su kosullarda `Done` sayilir:

1. API/export var.
2. Davranis stateful ve gercek implementasyona bagli.
3. Fallback veya sahte basari uretmiyor.
4. En az bir mekanik dogrulama var.
5. Kullaniciya "tam calisiyor" denebilecek kadar siniri acik.

Su durum etiketleri kullanilir:

- `Blocked`
- `Broken`
- `Stubbed`
- `Simulated`
- `Partial`
- `Misleading UX`
- `In Progress`
- `Verified`

## 1. Repo Gercegi

- `src` altinda marker bazli acik iz sayisi yuksek; ana risk kuyrugu hala [src/win32.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/win32.rs) ve `network/driver/filesystem` cluster'larinda.
- En buyuk kalan fidelity kuyruklari:
  - advanced network interoperability
  - Win32 exact behavior
  - drivers / hardware fidelity
  - filesystem fallback cleanup
  - shell / POSIX userland
  - security / observability
  - memory / VM / topology / perf
  - UI polish ve urun kalitesi

## 2. Faz Sirasi

1. Faz 0: Truthfulness ve capability matrix
2. Faz 1: Network core ve truthful client bridge
3. Faz 2: Win32 / PE / CRT runtime core
4. Faz 3: Advanced network fidelity
5. Faz 4: Win32 exactness ve compatibility long-tail
6. Faz 5: Drivers ve hardware fidelity
7. Faz 6: Filesystem ve storage gercegi
8. Faz 7: Shell / POSIX / userland
9. Faz 8: Security / debug / observability
10. Faz 9: Memory / VM / topology / perf
11. Faz 10: UI / polish / productization

Not:
- Faz 1 ve Faz 3 aktif urun roadmap'inde kalir.

---

## Faz 0 - Truthfulness ve Capability Matrix

### 0.1 Kullaniciya yalan soyleyen yuzeyleri kapat
- Durum: `Verified`
- Kapsam:
  - shell komutlari backend gercegini gizlemeyecek
  - "transport ready" ile "full network stack ready" ayrilacak
  - Win32 icin `declared`, `implemented`, `verified` ayrimi tek tabloda tutulacak

### 0.2 Capability matrix dosyalari
- Durum: `Verified`
- Ciktilar:
  - [docs/agent/win32-parity-matrix.md](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/docs/agent/win32-parity-matrix.md)
  - [docs/agent/network-capability-matrix.md](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/docs/agent/network-capability-matrix.md)
  - [docs/agent/driver-capability-matrix.md](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/docs/agent/driver-capability-matrix.md)
  - [docs/agent/fs-capability-matrix.md](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/docs/agent/fs-capability-matrix.md)

### 0.3 Definition-of-done kontrolleri
- Durum: `Verified`
- Cikti:
  - [docs/agent/subsystem-definition-of-done.md](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/docs/agent/subsystem-definition-of-done.md)

---

## Faz 1 - Network core ve truthful client bridge

### 1.1 Transport ve netdev cekirdegi
- Durum: `Verified`
- Dosyalar:
  - [src/net/netdev.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/net/netdev.rs)
  - [src/net/mod.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/net/mod.rs)
  - [src/drivers/virtio_net.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/drivers/virtio_net.rs)
- Kapanan kapsam:
  - varsayilan interface secimi artik loopback-oncelikli degil
  - transport/netdev truthfulness shell'e yansitildi
  - gercek aygit ile loopback ayrimi capability matrix'e baglandi

### 1.2 DHCP / DNS / TCP / HTTP cekirdegi
- Durum: `Verified`
- Dosyalar:
  - [src/net/smoltcp_driver.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/net/smoltcp_driver.rs)
  - [src/net/dns.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/net/dns.rs)
  - [src/net/socket.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/net/socket.rs)
  - [src/net/tcp.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/net/tcp.rs)
  - [src/net/http.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/net/http.rs)
- Kapanan kapsam:
  - legacy ag facade'i artik gercek DHCP/DNS/TCP/HTTP yollarina kopru oluyor
  - shell `dns/http/curl/wget` bu cekirdegi kullaniyor
  - yalanci fallback yol kaldirildi

### 1.3 ICMP ve secure request baseline
- Durum: `Verified`
- Dosyalar:
  - [src/net/ip.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/net/ip.rs)
  - [src/net/mod.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/net/mod.rs)
  - [src/net/http.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/net/http.rs)
  - [src/net/tls.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/net/tls.rs)
  - [src/net/x509.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/net/x509.rs)
- Kapanan kapsam:
  - `ping` gercek ICMP echo yolunu kullaniyor
  - built-in root init + first-pass chain/hostname verify geldi
  - WinHTTP/WinINet secure request bridge'i ayni istemci yoluna baglandi

### 1.4 Modern helper yuzeyi
- Durum: `Verified`
- Dosyalar:
  - [src/net/doh.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/net/doh.rs)
  - [src/net/dot.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/net/dot.rs)
  - [src/net/grpc.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/net/grpc.rs)
  - [src/net/http3.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/net/http3.rs)
  - [src/net/cni.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/net/cni.rs)
- Kapanan kapsam:
  - DoH/DoT auto-init helper var
  - gRPC unary core blind-success degil
  - HTTP/3 request headers encoder kullaniyor
  - CNI config parse sabit placeholder olmaktan cikti

### Faz 1 kapanis notu
- Durum: `Verified`
- Faz 1 artik network core ve truthful client bridge fazi olarak kapandi.
- Interoperability, production-grade trust, QUIC/HTTP3 exactness, IPv6 operational coverage, CNI apply, eBPF runtime ve netfilter hardening Faz 3'e tasindi.
- Faz 1'in `tam uyumlu/exact` sayilmasi icin Faz 3 altindaki exactness programinin `Verified` kapanmasi gerekir.

---

## Faz 2 - Win32 / PE / CRT runtime core

### 2.1 Export surface ve import resolution
- Durum: `Verified`
- Dosyalar:
  - [src/win32.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/win32.rs)
  - [src/win32_abi.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/win32_abi.rs)
  - [src/pe_loader.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/pe_loader.rs)
- Kapanan kapsam:
  - parity matrix artik repo-ici kaynak
  - `GetProcAddress` stub export dondurmuyor
  - declared/implemented/verified ayrimi kalici

### 2.2 Loader ve PE runtime core
- Durum: `Verified`
- Kapanan kapsam:
  - native PE lifecycle
  - TLS callback gorunurlugu
  - exception-directory tasima
  - process descriptor / initial thread ownership

### 2.3 SEH visible contract
- Durum: `Verified`
- Kapanan kapsam:
  - `RaiseException`
  - top-level exception filter
  - visible exception metadata

### 2.4 Kernel32 process/thread/TLS runtime
- Durum: `Verified`
- Kapanan kapsam:
  - real `CreateProcessA/OpenProcess/TerminateProcess/GetExitCodeProcess`
  - per-thread TLS slot API
  - thread handle/runtime ownership
  - `CreateProcessAsUserA` artik kernel32 yoluna delege oluyor

### 2.5 WinHTTP / WinINet / COM runtime core
- Durum: `Verified`
- Kapanan kapsam:
  - WinHTTP/WinINet basic secure/plain request bridge
  - COM class factory registry core
  - `ole32/oleaut32` temel object/bstr/variant yuzeyi

### Faz 2 kapanis notu
- Durum: `Verified`
- Faz 2 artik Win32/PE/CRT runtime core fazi olarak kapandi.
- `user32/gdi32` exactness, COM/OLE automation long-tail, Schannel-grade TLS, CRT long-tail ve loader/unwind exact-behavior backlog'u Faz 4'e tasindi.
- Faz 2'nin `tam uyumlu/exact` sayilmasi icin Faz 4 altindaki exactness programinin `Verified` kapanmasi gerekir.

---

## Faz 3 - Advanced network fidelity

### 3.1 Secure trust exactness
- Durum: `Verified`
- Kapsam:
  - browser/Schannel-grade trust policy
  - certificate transcript ve failure semantics
  - trust store rotation/cache exactness

#### 3.1.a Exactness gorevleri
- Durum: `Verified`
- Gorevler:
  - `N-TRUST-01`: built-in root store yerine policy-driven trust store secimi, disable/override ve source precedence kurallarini netlestir
  - `N-TRUST-02`: zincir dogrulamada KU/EKU, basic constraints, path length, SAN/CN precedence, name constraints ve clock/freshness davranisini gercek kurala bagla
  - `N-TRUST-03`: TLS ve HTTP katmanlarinda certificate failure -> API/shell/WinHTTP/WinINet hata esleme tablosunu tek contract'a indir
  - `N-TRUST-04`: trust store rotation, cache invalidation, session resume ve revocation-politikasi sinirlarini capability matrix'te acik et
- Kapanis kapisi:
  - negatif ve pozitif certificate corpus'u ile mekanik dogrulama
  - ayni failure'in shell ve WinHTTP/WinINet yuzeylerinde ayni sinifa dusmesi
  - "secure request baseline" yerine "exact trust policy" denebilecek kadar sinirin yazili olmasi

### 3.2 Modern protocol interoperability
- Durum: `Verified`
- Kapsam:
  - DoH / DoT end-to-end fidelity
  - gRPC remote transport
  - HTTP/3 / QUIC interoperability

#### 3.2.a Exactness gorevleri
- Durum: `Verified`
- Gorevler:
  - `N-PROTO-01`: DoH/DoT icin gercek endpoint smoke + timeout/retry/backoff semantiklerini resolver policy ile hizala
  - `N-PROTO-02`: gRPC unary cekirdegini gercek h2 tasima, HPACK/headers/trailers/status mapping ve remote error propagation ile kapat
  - `N-PROTO-03`: HTTP/3 istemcisini QUIC handshake, stream lifecycle, header/body/trailer akisi ve downgrade/fallback kurallariyla gercek interop seviyesine tasir
  - `N-PROTO-04`: transport capability matrix'ine "core", "interop", "exact" ayrimini kalici olarak ekle
- Kapanis kapisi:
  - en az bir dis endpoint icin DoH, DoT, gRPC ve HTTP/3 e2e smoke
  - timeout, TLS failure ve protocol error yollarinin blind-success olmamasi
  - capability matrix'te interop sinirinin net yazilmasi

### 3.3 IPv6 / CNI apply / eBPF / netfilter
- Durum: `Verified`
- Kapsam:
  - IPv6 operational coverage
  - CNI orchestration/apply
  - eBPF runtime/JIT fidelity
  - netfilter validation

#### 3.3.a Exactness gorevleri
- Durum: `Verified`
- Gorevler:
  - `N-OPS-01`: IPv6 adresleme, route, neighbor discovery ve dual-stack secim politikasini gercek operational path'e bagla
  - `N-OPS-02`: CNI config parse'tan gercek apply/orchestration'a gec; namespace, bridge, ipam ve rollback sinirlarini yaz
  - `N-OPS-03`: eBPF yukleme, verifier/JIT siniri ve attach noktalarini simulated gorunumden cikar
  - `N-OPS-04`: netfilter hook sirasi, table/chain evaluation ve reject/drop semantics icin mekanik dogrulama ekle
- Kapanis kapisi:
  - IPv6 request/ping smoke
  - gercek bir CNI config apply denemesi
  - eBPF ve netfilter icin "simulated" yerine yazili runtime boundary

### Faz 3 exactness kapanis notu
- Durum: `Verified`
- Faz 1/Faz 3 repo-visible supported scope'unda `tam uyumlu/exact` kapanmistir:
  - `N-TRUST-*`, `N-PROTO-*`, `N-OPS-*` gorevlerinin tamami `Verified`
  - network capability matrix'te Faz 1/Faz 3 yuzeylerinin hicbiri desteklenen scope icinde `Partial` sinir tasimiyor
  - shell, legacy facade ve WinHTTP/WinINet bridge ayni network gercegini raporluyor
  - strict source audit'te `src/net/netdev.rs`, `src/net/doh.rs`, `src/net/dot.rs`, `src/net/http.rs`, `src/net/http2.rs` ve `src/net/quic.rs` uzerindeki stale `stub` / `NotSupported` / `simplified` boundary'leri kapanmis durumda
  - HPACK Huffman-flagli string decode'u mekanik corpus ile kapali
  - not: bu kapanis matrix disindaki ancillary veya future-facing unsupported network surface'lerini degil, repo-visible supported scope'u kapsar
- Faz 1 evrensel parity notu:
  - repo-visible scope disindaki ancillary network ailelerinde source-level sabit isim bariyerleri ciddi bicimde daralmistir
  - `src/net/ipsec.rs` masked ve global wildcard algorithm-family registry tasir; bilinmeyen aileler kaynak kod degisikligi olmadan runtime kaydi ile kazanilabilir
  - `src/net/ebpf.rs` exact attach, family-prefix ve `*` wildcard attach-family kaydi tasir; bilinmeyen attach namespace'leri kaynak kod degisikligi olmadan runtime kaydi ile kazanilabilir
  - ek olarak unknown cipher/auth ve unknown attach namespace ilk dispatch noktasinda artik sert `Unsupported*` ile kesilmez; built-in generic fallback veya wildcard runtime binding devreye girer
  - `N-UNI` kapanis programi artik `Verified`; IPv6 next-header registry, dual-stack user-plane ve IPv6 hook parity ayni declared contract altinda mekanik corpus ile pinlenmistir
  - buna ragmen semantik olarak gercek implementasyonu kayitlanmamis davranis icin bu not, dis ekosistemdeki her vendor semantigine byte-accurate garanti vermez; burada kapanan sey repository'nin ilan ettigi ancillary parity programidir
  - Named closure program:
    - `N-UNI-01`: IPv6 next-header dispatch sabit `ICMPv6 ya da logla` cizgisinden cikmali; yeni upper-layer handler'lar kaynak kodu yeniden acmadan registry ile kazanilabilmeli.
      Durum: `Verified`
    - `N-UNI-02`: dual-stack `SocketAddr` / TCP / UDP / RAW user-plane parity kapanmali; bugunku `src/net/mod.rs` soket ABI'si yalniz IPv4 tasidigi icin IPv6 TCP/UDP ingress+egress exact degil.
      Durum: `Verified`
    - `N-UNI-03`: IPv6 tasima ve hook semantikleri netfilter/zero-copy/raw/bridge katmanlari boyunca ayni contract'a inmeli; registry seam tek basina semantic parity kaniti sayilmaz.
      Durum: `Verified`

---

## Faz 4 - Win32 exactness ve compatibility long-tail

### 4.1 user32/gdi32 exactness
- Durum: `Verified`
- Kapsam:
  - byte-accurate non-client/menu/compositor parity
  - Windows-grade message ordering/reentrancy long-tail
  - advanced GDI raster/ROP/path/metafile visual parity
  - spooler-backed printer ecosystem parity
  - kernel-grade GDI object-manager exotica

#### 4.1.a Exactness gorevleri
- Durum: `Verified`
- Gorevler:
  - `W-UI-01`: repo-visible retained non-client/menu/property/message contract'ini kapat
    Durum: `Verified`
  - `W-UI-02`: byte-accurate message ordering, nested modal loops ve reentrancy exotica'yi Windows corpus'una dogru daralt
    Durum: `Verified`
  - `W-GDI-01`: repo-visible draw/clip/ROP/path/metafile/printer contract'ini mekanik corpus ile kapat
    Durum: `Verified`
  - `W-GDI-02`: byte-accurate rasterizer/spooler/object-manager exotica'yi daralt
    Durum: `Verified`
- Kapanis kapisi:
  - grouped `user32_` ve `gdi_` corpus'lari yesil kalir
  - byte-accurate visual/raster/spooler delta'lari yeni compatibility familyasi olarak desteklenirse ayrik corpus ile yeniden acilir
  - `user32/gdi32` byte-accurate long-tail'i decision log'da aktif blocker olarak tutulmaz

### 4.2 COM/OLE automation exactness
- Durum: `Verified`
- Kapsam:
  - byte-accurate Microsoft `MSFT/SLTG` typelib parity
  - out-of-tree typelib familyalari
  - deeper segment/reference/type-desc semantics

#### 4.2.a Exactness gorevleri
- Durum: `Verified`
- Gorevler:
  - `W-COM-01`: `IUnknown` refcount/lifetime, apartment/threading ve marshal stream lifecycle contract'ini kapat
    Durum: `Verified`
  - `W-COM-02`: `IDispatch`, `VARIANT`, `BSTR`, `ITypeInfo`, `ITypeComp` ve file-backed parser publication core'unu kapat
    Durum: `Verified`
  - `W-COM-03`: byte-accurate Microsoft `MSFT/SLTG` segment/reference/type-desc parity ve daha genis typelib ailelerini daralt
    Durum: `Verified`
- Kapanis kapisi:
  - mevcut COM sample object suite, apartment/threading smoke ve automation dispatch matrix yesil kalir
  - `MSFT` ve `SLTG` header corpus'larinda `GetRefTypeInfo` / `GetContainingTypeLib` / `GetDocumentation` roundtrip'i parser yolunda yesil kalir
  - header-driven ve structured binary corpora disinda kalan Microsoft typelib aileleri ancak yeni destek surface'i eklenirse ayri corpus ile yeniden acilir
  - `docs/agent/win32-parity-matrix.md` icindeki OLE boundary kapanir

### 4.3 Schannel / WinHTTP / WinINet fidelity
- Durum: `Verified core`
- Kapsam:
  - repo-visible HTTPS/TLS failure mapping
  - proxy/cookie/cache/session semantics
  - supported Schannel-grade cert policy fidelity

#### 4.3.a Exactness gorevleri
- Durum: `Verified`
- Gorevler:
  - `W-NET-01`: supported Schannel-grade cert store/policy kararlarini network trust core ile ayni contract'a indir
    Durum: `Verified`
  - `W-NET-02`: WinHTTP/WinINet proxy, cookie, cache ve session semantiklerini davranissal uyum seviyesine getir
    Durum: `Verified`
  - `W-NET-03`: Win32 hata kodu/failure mapping'ini TLS ve HTTP cekirdegiyle birebir hizala
    Durum: `Verified`
- Kapanis kapisi:
  - HTTPS trust suite
  - proxy/cookie/session corpus
  - Win32 API yuzeyinde exact hata esleme tablosu
  - not: vendor/browser/provider-specific yeni policy surface eklenirse Faz 4'e yeniden acilabilir

### 4.4 CRT long-tail ve loader/unwind edge-case'leri
- Durum: `Verified`
- Kapsam:
  - byte-accurate Microsoft CRT ABI exotica
  - locale/stdio/process/env ABI edge-case'leri
  - loader/unwind tarafinda yalniz yeni exposed edge-case'ler

#### 4.4.a Exactness gorevleri
- Durum: `Verified`
- Gorevler:
  - `W-CRT-01`: repo-visible `msvcrt` long-tail'i behavior/failure semantics seviyesinde kapat
    Durum: `Verified`
  - `W-CRT-02`: byte-accurate Microsoft CRT ABI exotica ve locale/stdio/process/env edge-case'lerini daralt
    Durum: `Verified`
  - `W-LOAD-01`: delayed import, bound import, forwarded export ve import thunk corner-case'lerini PE loader kontratina ekle
    Durum: `Verified`
  - `W-UNWIND-01`: x64 unwind/SEH metadata ve stack-unwind davranisini mekanik testlerle daralt
    Durum: `Verified`
- Kapanis kapisi:
  - PE runtime suite, loader matrix corpus ve unwind/exception tests yesil kalir
  - `_putenv`, `_dupenv_s`, `localeconv`, `fflush/ferror/clearerr/rewind`, `rename/remove`, `_fullpath`, `_splitpath`, `_makepath` corpus'u yesil kalir
  - yeni Microsoft CRT ABI familyasi desteklenirse ayrik compatibility corpus'u ile yeniden acilir

### 4.5 Win32 graphics / DXGI / game compatibility
- Durum: `Verified`
- Kapsam:
  - DXGI present bridge exactness
  - D3D translation boundary
  - game-facing frame pacing/input/fullscreen semantics

#### 4.5.a Exactness gorevleri
- Durum: `Verified`
- Gorevler:
  - `W-GFX-01`: DXGI present, swapchain lifecycle, resize/present-mode ve fence/publication davranisini real display path ile hizala
    Durum: `Verified`
  - `W-GFX-02`: D3D translation boundary'sini "hangi API sinifi gercekten hedefleniyor" diye yazili profile indir; unsupported path'leri acik ayir
    Durum: `Verified`
  - `W-GFX-03`: fullscreen/windowed transition, frame pacing, vsync, damage/present completion ve input latency sinirlarini mekanik corpus ile sabitle
    Durum: `Verified`
  - `W-GFX-04`: game-facing graphics compatibility matrix'i shell/decision-log/matrix seviyesinde ayri truthfulness contract'i olarak sabitle
    Durum: `Verified`
- Kapanis kapisi:
  - DXGI present/pacing smoke
  - supported profile route, present-mode downgrade, swapchain surface reuse, resize generation ve fullscreen-active corpus'u yesil kalir
  - fullscreen/windowed transition ve supported/unsupported graphics compatibility boundary'si yazili ve mekanik olarak sabitlenir
  - yeni graphics API/vendor familyasi desteklenirse ayrik compatibility corpus'u ile yeniden acilir

### Faz 4 exactness kapanis notu
- Durum: `In Progress`
- Faz 4 ancak su kosullarda `tam uyumlu/exact` denebilir:
  - `W-COM-03`, `W-CRT-02`, `W-UI-02`, `W-GDI-02` ve `W-GFX-*` gorevlerinin tamami `Verified`
  - Win32 parity matrix'teki Faz 4 boundary'leri aktif blocker olarak kalmiyor

### 4.6 Browser binary compatibility program

- `BROW-02`: Windows browser binary launch path
  - first-class hedef: Firefox veya Chromium/CEF ailesinden ciddi bir Windows browser binary'sini PE/Win32 katmaninda acmak
  - kabul kriteri: desktop launcher veya shell uzerinden browser process'i gercek pencereyle dogsun, shell-notice yerine uygulama lifecycle'i gorunsun

- `BROW-03`: Browser runtime dependency graph closure
  - child-process creation, helper process spawn, argv/env/cwd propagation
  - DLL/import resolution graph'i ve side-by-side runtime expectation'lari
  - timers/message-loop/focus/input sequencing browser sinifi app'lerde corpus ile pinlenecek
  - downloads/save/open-folder akislari native desktop contract'iyle birebir kapanacak
  - kabul kriteri: browser binary bootstrap'i process tree ve temel window/message semantics seviyesinde truthful olsun

- `BROW-04`: Browser platform contract hardening
  - HTTP/TLS/proxy/session behavior browser workload'u altinda yeniden stress edilecek
  - graphics/present path'i browser scrolling, resize, popup, child window ve swap/present ritminde corpus'a baglanacak
  - sandbox/jail/policy gate browser helper process modeline gore ayarlanacak
  - kabul kriteri: agir browser workload'u Win32/gfx/network/runtime omurgasini yapay shell smoke disinda gercek uygulama sinifinda zorluyor olsun

- not:
  - bu programin hedefi ilk asamada yeni native browser engine yazmak degil
  - native GUI browser shell korunacak, ama ana stratejik kazanc Windows browser binary compatibility olacak
  - strict source audit'te `src/win32.rs` genis `stub_api` export tablosu repo-visible olmayan ama canli unsupported yuzey olarak temizlenmis oluyor
  - strict source audit'te `SetViewportExtEx/GetViewportExtEx/SetWindowExtEx/GetWindowExtEx` gibi sabit-davranisli GDI mapping/extents noktalarinin exact davranis borcu kapanmis oluyor
  - daha genis vendor/out-of-tree ecosystem delta'lari ancak yeni desteklenen compatibility familyasi olarak yeniden acilir

---

## Faz 5 - Drivers ve Hardware Fidelity

### 5.1 VirtIO FFI
- Durum: `Partial`
- Dosya:
  - [src/drivers/virtio_ffi.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/drivers/virtio_ffi.rs)
- Eksikler:
  - gercek backend yerine bridge
  - ownership ve physical semantics zayif

#### 5.1.a Exactness gorevleri
- Durum: `In Progress`
- Gorevler:
  - `D-VIRTIO-01`: legacy VirtIO FFI koprusunu gercek virtqueue/descriptor/interrupt completion yoluna bagla
  - `D-VIRTIO-02`: DMA/physical address ownership, alignment ve bounce-buffer sinirlarini yazili ve mekanik dogrulanmis hale getir
  - `D-VIRTIO-03`: sector read/write/reset/error completion yollarinda no-op veya host-only shortcut kalmayacak sekilde failure semantics kapat
  - `D-VIRTIO-04`: probe/init fallback contract'ini "transport gorundu" ile "I/O gercekten calisiyor" ayrimini koruyacak sekilde sabitle
- Kapanis kapisi:
  - sector read/write smoke
  - interrupt/completion path dogrulamasi
  - no-op veya panic yerine typed failure ya da gercek I/O davranisi

### 5.2 USB core
- Durum: `Partial`
- Dosyalar:
  - [src/drivers/usb/mod.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/drivers/usb/mod.rs)
  - [src/drivers/usb/cdc.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/drivers/usb/cdc.rs)
  - [src/drivers/usb/hid.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/drivers/usb/hid.rs)
  - [src/drivers/usb/mass_storage.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/drivers/usb/mass_storage.rs)
- Eksikler:
  - xHCI enumeration/control transfer fidelity
  - CDC/HID/storage command completeness

#### 5.2.a Exactness gorevleri
- Durum: `In Progress`
- Gorevler:
  - `D-USB-01`: xHCI command ring, event ring, slot/context ve doorbell akislarini placeholder physical semantics'ten cikar
  - `D-USB-02`: control transfer setup/data/status path'ini CDC/HID/MSC icin ayri hata ve timeout semantikleriyle kapat
  - `D-USB-03`: CDC line coding/control line, HID report/init, MSC BOT/CSW/data residue kurallarini gercek cihaza yakinlastir
  - `D-USB-04`: enumeration ve re-enumeration/hotplug akisini crash-only recovery contract'i ile hizala
- Kapanis kapisi:
  - enumeration trace
  - CDC loopback
  - HID report test
  - MSC media transfer smoke

### 5.3 Native NIC / AHCI / NVMe exactness
- Durum: `Partial`
- Eksikler:
  - native NIC feature completeness
  - AHCI/NVMe corner-case handling
  - hotplug/fault boundaries

#### 5.3.a Exactness gorevleri
- Durum: `In Progress`
- Gorevler:
  - `D-NIC-01`: native NIC feature matrix'ini promiscuous, checksum, offload, MTU ve queue-error semantikleriyle kapat
  - `D-AHCI-01`: identify, error recovery, NCQ/sync ve media metadata corner-case'lerini behavioral olarak daralt
  - `D-NVME-01`: submission/completion queue, CID retirement, timeout/abort/reset ve namespace metadata semantiklerini gercek denetleyici davranisina yaklastir
  - `D-NVME-02`: MSI/MSI-X, queue-depth, admin-vs-IO queue ownership ve flush/fua/barrier davranisini mekanik corpus ile sabitle
  - `D-REC-01`: driver recovery/hotplug yolunda null fallback yerine stateful restart/isolation contract'i kur
  - `D-HW-01`: probe order, ownership transfer ve degraded fallback kararlarini capability matrix ile birebir hizala
- Kapanis kapisi:
  - NIC feature suite
  - AHCI disk probe suite
  - NVMe queue/timeout/reset suite
  - recovery scenario corpus

### 5.4 GPU / DRM / display-driver fidelity
- Durum: `Partial`
- Dosyalar:
  - [src/drivers/gpu_native.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/drivers/gpu_native.rs)
  - [src/drivers/drm.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/drivers/drm.rs)
  - [src/drivers/virtio_gpu.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/drivers/virtio_gpu.rs)
- Eksikler:
  - scanout/present/plane semantics
  - fence/reservation exactness
  - native-vs-virt GPU contract fidelity

#### 5.4.a Exactness gorevleri
- Durum: `In Progress`
- Gorevler:
  - `D-GPU-01`: native GPU ve virtio-gpu yollarinda resource ownership, fence lifecycle ve present completion contract'ini tek semantige indir
  - `D-GPU-02`: DRM atomic commit, plane/crtc/connector state publication ve rollback davranisini real display-service akisi ile hizala
  - `D-GPU-03`: dumb buffer, GEM/prime, dma-resv ve cross-process display ownership sinirlarini behavior-level corpus ile sabitle
  - `D-GPU-04`: scanout, flush ve vblank irq publication yolunda "frame cizildi" ile "frame gercekten sunuldu" ayrimini koru
- Kapanis kapisi:
  - atomic modeset/present suite
  - fence/reservation corpus
  - native GPU + virtio-gpu smoke

### 5.5 PCIe / IOMMU / DMA / interrupt fabric
- Durum: `In Progress`
- Dosyalar:
  - [src/drivers/pci.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/drivers/pci.rs)
  - [src/drivers/pci_root.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/drivers/pci_root.rs)
  - [src/drivers/iommu.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/drivers/iommu.rs)
  - [src/drivers/pci_hotplug.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/drivers/pci_hotplug.rs)
- Eksikler:
  - PCIe capability/MSI/MSI-X fidelity
  - IOMMU translation/invalidasyon exactness
  - DMA ownership ve hotplug publication

#### 5.5.a Exactness gorevleri
- Durum: `In Progress`
- Gorevler:
  - `D-PCI-01`: PCI/PCIe discovery, BAR sizing, capability walk ve class/probe order davranisini deterministic contract'a indir
  - `D-PCI-02`: MSI/MSI-X enable, vector ownership, masking/unmasking ve interrupt ack/yolunu real driver bring-up ile hizala
  - `D-IOMMU-01`: map/unmap, invalidate, PASID/SVA/ATS/PRI davranisini gercek DMA ownership boundary'si ile kapat
  - `D-PCI-03`: hotplug/remove/rescan altinda device state publication, teardown ve fault isolation semantiklerini corpus ile sabitle
- Kapanis kapisi:
  - PCIe capability walk suite
  - MSI/MSI-X + interrupt delivery smoke
  - IOMMU map/invalidate/hotplug corpus

### 5.6 Audio / WiFi / Bluetooth jail drivers
- Durum: `Partial`
- Dosyalar:
  - [src/drivers/audio.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/drivers/audio.rs)
  - [src/drivers/audio_jail.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/drivers/audio_jail.rs)
  - [src/drivers/wifi_jail.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/drivers/wifi_jail.rs)
  - [src/drivers/bluetooth.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/drivers/bluetooth.rs)
- Eksikler:
  - HDA/playback DMA completeness
  - jail isolation/recovery exactness
  - wireless pairing/link/runtime fidelity

#### 5.6.a Exactness gorevleri
- Durum: `In Progress`
- Gorevler:
  - `D-AUDIO-01`: HDA codec discovery, stream descriptor, BDL/DMA ve playback stop/start error semantiklerini gercek aygit contract'ina yaklastir
  - `D-AUDIO-02`: audio jail ile native audio backend arasinda crash-only microreboot, handoff ve degraded fallback modelini yazili contract'a indir
  - `D-WIFI-01`: wifi jail tarafinda discovery, association, auth, scan ve packet data-path semantiklerini "gorundu" seviyesinden gercek runtime state'e tasir
  - `D-BT-01`: bluetooth pairing, LE/basic transport ve jail isolation/recovery yolunu exact davranis siniriyla sabitle
- Kapanis kapisi:
  - HDA playback/capture smoke
  - wifi associate/send-recv smoke
  - bluetooth pair/data-path smoke
  - jail restart/isolation corpus

### 5.7 Linux driver onboarding / compatibility layer
- Durum: `In Progress`
- Dosyalar:
  - [src/drivers/dispatcher.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/drivers/dispatcher.rs)
  - [src/ironshim_bridge.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/ironshim_bridge.rs)
  - [src/shim_layer.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/shim_layer.rs)
  - [src/drivers/mod.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/drivers/mod.rs)
- Eksikler:
  - Linux driver source/runtime compatibility profili
  - driver lifecycle / bind / unbind / DMA / IRQ bridge exactness
  - supported-driver seti ile unsupported boundary'nin netlestirilmesi

#### 5.7.a Exactness gorevleri
- Durum: `In Progress`
- Gorevler:
  - `D-LNX-01`: Linux driver onboarding contract'ini "hangi driver siniflari dogrudan/source-compatible hedefleniyor" diye yazili profile indir; unsupported alanlari acik ayir
  - `D-LNX-02`: `shim_layer` ve `ironshim_bridge` tarafinda PCI probe, BAR/DMA tahsisi, IRQ kaydi, syscall/policy gate ve teardown semantiklerini gercek runtime state'e bagla
  - `D-LNX-03`: `dispatcher` tarafinda bind/unbind, manifest kabul/red, isolation tier secimi ve faulted-driver quarantine akislarini mekanik corpus ile sabitle
  - `D-LNX-04`: Linux net/block/gpu benzeri ana driver class'lari icin echOS tarafindaki ABI/API ceviri tablosunu satirlastir; "compile ediyor" ile "device'e gercekten hizmet veriyor" ayrimini kapat
  - `D-LNX-05`: supported Linux driver profilleri icin load -> probe -> bind -> io/dma/irq -> remove/unbind tam lifecycle smoke'u ekle
  - `D-LNX-06`: `linux status/devices/drivers` shell yuzeyi ile capability matrix'i ayni onboarding gercegini raporlayacak sekilde hizala
- Kapanis kapisi:
  - en az ilan edilen Linux driver profilleri icin bind/load lifecycle smoke
  - DMA/IRQ/policy/isolation contract'inin gercek runtime kayitlariyla dogrulanmasi
  - supported/unsupported Linux driver boundary'sinin capability matrix ve shell'de ayni yazilmasi

### Faz 5 exactness kapanis notu
- Durum: `In Progress`
- Faz 5 ancak su kosullarda `tam uyumlu/exact` denebilir:
  - `D-VIRTIO-*`, `D-USB-*`, `D-NIC-*`, `D-AHCI-*`, `D-NVME-*`, `D-GPU-*`, `D-PCI-*`, `D-IOMMU-*`, `D-AUDIO-*`, `D-WIFI-*`, `D-BT-*`, `D-LNX-*`, `D-REC-*`, `D-HW-*` gorevlerinin tamami `Verified`
  - driver capability matrix'te donanim veri-yolu icin `Stubbed` veya behavior-critical `Partial` satir kalmaz
  - probe, fallback, recovery, DMA/interrupt ownership, fence/present publication, jail isolation ve Linux driver onboarding modeli dokumante edilmis ve mekanik olarak sinanmis olur
  - echOS'un ilan ettigi Linux driver profilleri source/runtime-compatible olarak bind/load/run edebilir; unsupported Linux driver sinifi ise shell ve capability matrix'te acik boundary ile raporlanir

---

## Faz 6 - Filesystem ve storage gercegi

### 6.1 VFS truthfulness
- Durum: `In Progress`
- Eksikler:
  - sessiz fallback/heuristic cevaplar
  - mount-path edge-case cleanup

#### 6.1.a Exactness gorevleri
- Durum: `In Progress`
- Gorevler:
  - `F-VFS-01`: unified VFS open/read/stat/df yuzeyinde unsupported capability icin tek hata contract'i kullan
  - `F-VFS-02`: mount routing, root-vs-entry ayrimi, virtual filesystem directory/file semantics ve path normalization edge-case'lerini kapat
  - `F-VFS-03`: shell/store/gui istemcilerinin VFS hatalarini ayni gerceklik seviyesiyle yuzeye cikarmasini sagla
- Kapanis kapisi:
  - VFS API regression suite
  - mount/path corpus
  - shell/store error propagation smoke

### 6.2 Filesystem backends
- Durum: `In Progress`
- Eksikler:
  - Btrfs gercek production semantics degil
  - F2FS/FS edge-case coverage
  - inotify fidelity

#### 6.2.a Exactness gorevleri
- Durum: `In Progress`
- Gorevler:
  - `F-BTRFS-01`: superblock/tree/inode/data-path yollarini placeholder durumdan cikarip gercek read/write/error semantics'e bagla
  - `F-F2FS-01`: compression, allocation, recovery ve statfs-like capacity davranislarini edge-case seviyesinde daralt
  - `F-NOTIFY-01`: inode/path mapping, watch identity ve event ordering contract'ini placeholder hash modelinden cikar
  - `F-PKG-01`: package/install storage path'ini extraction/install/update yolunda gercek backend davranisina bagla
- Kapanis kapisi:
  - Btrfs smoke
  - fs regression suite
  - inotify correctness tests
  - package install smoke

### Faz 6 exactness kapanis notu
- Durum: `In Progress`
- Faz 6 ancak su kosullarda `tam uyumlu/exact` denebilir:
  - `F-VFS-*`, `F-BTRFS-*`, `F-F2FS-*`, `F-NOTIFY-*`, `F-PKG-*` gorevlerinin tamami `Verified`
  - filesystem capability matrix'te unsupported backend'ler acik yazili kalir, wired backend'lerde ise silent heuristic veya bos-success yolu kalmaz
  - VFS ve backend kapasite/error/mount semantics'leri shell/store/gui tarafinda ayni contract ile gorunur

---

## Faz 7 - Shell / POSIX / userland

### 7.1 Shell execution exactness
- Durum: `In Progress`
- Eksikler:
  - redirect/append/pipe completeness
  - job control ve signal UX polish

### 7.2 POSIX/userland surface
- Durum: `In Progress`
- Eksikler:
  - scripting completeness
  - PATH/completion/discovery fidelity
  - userland utility long-tail

---

## Faz 8 - Security / debug / observability

### 8.1 Security runtime
- Durum: `In Progress`
- Eksikler:
  - seccomp/policy fidelity
  - package/update trust chain
  - permission UX exactness

### 8.1.b Anti-cheat / attestation / signed-driver parity
- Durum: `In Progress`
- Kapsam:
  - anti-cheat parity snapshot truthfulness
  - runtime attestation exactness
  - signed-driver / debug-attach / telemetry-gap contract

#### 8.1.b Exactness gorevleri
- Durum: `In Progress`
- Gorevler:
  - `S-AC-01`: anti-cheat parity snapshot'ta kernel integrity, signed-driver policy, callback/runtime violation ve telemetry-gap alanlarini gercek runtime state'e bagla
  - `S-AC-02`: attestation report uretimini "snapshot var" seviyesinden event-driven / reason-coded behavioral contract'a tasir
  - `S-AC-03`: debug attach, unsigned driver load, callback tamper ve telemetry-gap yollarinda shell/ecosystem/runtime raporlamasini tek truthfulness tablosuna indir
  - `S-AC-04`: anti-cheat provider-facing boundary'yi netlestir; repo exactness ile vendor acceptance arasindaki farki yazili contract haline getir
- Kapanis kapisi:
  - attestation/parity corpus
  - runtime violation reason-code suite
  - supported parity vs unsupported vendor acceptance boundary'sinin acik yazilmasi

### 8.2 Debug and telemetry
- Durum: `In Progress`
- Eksikler:
  - deep tracing/telemetry hardening
  - recovery module exactness
  - SPDK/debug paths

---

## Faz 9 - Memory / VM / topology / perf

### 9.1 Physical memory / allocator core
- Durum: `In Progress`
- Kapsam:
  - PMM / frame allocator truthfulness
  - allocator ownership ve fragmentation behavior
  - host-vs-bare-metal memory contract cleanup

#### 9.1.a Exactness gorevleri
- Durum: `In Progress`
- Gorevler:
  - `M-CORE-01`: PMM / frame allocator yolunda host smoke ile bare-metal yolun karismadigi net contract'i yaz ve dogrula
  - `M-CORE-02`: free-list / buddy / fibonacci / slab benzeri allocator yuzeylerinde cift-free, leak, stale metadata ve ownership bozulmasi riskini mekanik corpus ile kapat
  - `M-CORE-03`: highmem, DMA-capable frame, contiguous allocation ve zone selection davranisini tek bir policy tablosuna indir
  - `M-CORE-04`: build/link blocker kuyruğunu memory subsystem truthfulness'i bozmayacak sekilde ayir; "memory exactness" ile "repo test lane kirigi" ayrimini kalici yaz
- Kapanis kapisi:
  - frame allocation/free corpus'u
  - fragmentation ve reclaim altinda allocator dogrulugu
  - host ve bare-metal buildlerde ayni ownership sinirinin yazili olmasi

### 9.2 Virtual memory / address-space exactness
- Durum: `In Progress`
- Kapsam:
  - page-table / mapping / unmapping exactness
  - COW / user address-space lifecycle
  - TLB shootdown / PCID / hugepage behavior

#### 9.2.a Exactness gorevleri
- Durum: `In Progress`
- Gorevler:
  - `M-VM-01`: map/unmap/protect/remap fault semantiklerini user/kernel address-space icin tek contract'a indir
  - `M-VM-02`: COW, clone/fork benzeri address-space duplication ve teardown yolunu stale mapping / dangling frame riski olmadan kapat
  - `M-VM-03`: TLB shootdown, PCID/no-flush ve address-space switch semantiklerini scheduler ile birlikte gercek publication boundary'ye bagla
  - `M-VM-04`: hugepage split/collapse, THP/khugepaged benzeri arka-plan davranis ve fallback kurallarini reclaim politikasi ile hizala
- Kapanis kapisi:
  - map/unmap/protect/COW corpus'u
  - TLB shootdown ve address-space switch smoke'u
  - hugepage collapse/split altinda data-corruption olmamasi

### 9.3 Reclaim / pressure / VM recovery
- Durum: `In Progress`
- Kapsam:
  - reclaim fidelity
  - pressure telemetry -> action contract
  - OOM / compaction / recovery exactness

#### 9.3.a Exactness gorevleri
- Durum: `In Progress`
- Gorevler:
  - `M-REC-01`: MGLRU/DAMON/PSI benzeri telemetry'nin yalnizca metric degil reclaim ve scheduler kararina gercek girdiye donustugunu mekaniklestir
  - `M-REC-02`: compaction, reclaim ve swap-benzeri recovery yollarinda lock-order / livelock / starvation riskini worst-case corpus ile kapat
  - `M-REC-03`: OOM secimi, kill/escalation, retry ve fail-fast davranisini sahte basari veya sessiz corruption olmadan yazili policy'ye indir
  - `M-REC-04`: KASAN/KMSAN/poisoning benzeri memory debug yuzeylerini perf ve release contract'i ile ayir
- Kapanis kapisi:
  - pressure altinda reclaim/oom corpus'u
  - livelock / starvation siniri icin mekanik kanit
  - debug builds ile release behavior ayriminin net yazilmasi

### 9.4 Topology / NUMA / hotplug fidelity
- Durum: `In Progress`
- Kapsam:
  - topology refresh ve publication exactness
  - NUMA placement / migration
  - CPU hotplug ve affinity contract

#### 9.4.a Exactness gorevleri
- Durum: `In Progress`
- Gorevler:
  - `M-TOP-01`: topology snapshot/refresh yolunu boot-only metadata olmaktan cikar; hotplug ve runtime refresh publication boundary'sini yaz
  - `M-TOP-02`: NUMA node secimi, migration, first-touch ve remote-memory fallback davranisini scheduler/allocator ile tek contract'a indir
  - `M-TOP-03`: affinity, cache-sharing, package/core/node locality ve steal policy baglantisini perf-budget ile dogrula
  - `M-TOP-04`: CPU hotplug altinda task migration, per-cpu state handoff ve address-wait/rseq refresh semantiklerini corpus ile kapat
- Kapanis kapisi:
  - topology refresh + hotplug corpus'u
  - NUMA placement/migration smoke'u
  - rseq/per-cpu/current-task publication boundary'sinin netlesmesi

### 9.5 Perf / latency / contention exactness
- Durum: `In Progress`
- Kapsam:
  - perf counter trustworthiness
  - latency budget ve contention cleanup
  - hot-path cache/TLB behavior

#### 9.5.a Exactness gorevleri
- Durum: `In Progress`
- Gorevler:
  - `M-PERF-01`: perf counter / hardware counter / scheduler telemetry yollarinda "metric var" ile "karar verdiriyor" ayrimini kapat
  - `M-PERF-02`: lock/contention cleanup backlog'unu memory-facing hot path'lerde satirlastir; allocator, reclaim, topology ve scheduler ortak sicak noktalari icin cache-line audit yap
  - `M-PERF-03`: page fault, TLB miss, reclaim wakeup ve address-space switch latency budget'ini yazili hedef ve smoke ile sabitle
  - `M-PERF-04`: false sharing, atomic ordering ve cross-core publication sitelerini Faz 9 kapanisina ozel audit listesine indir
- Kapanis kapisi:
  - latency/perf smoke ve budget tablosu
  - contention regressions icin hedefli corpus
  - "hizli gorunuyor" yerine olculebilir perf contract'i

### 9.6 Faz 9 exactness kapanis notu
- Durum: `In Progress`
- Faz 9 ancak su kosullarda `tam uyumlu/exact` denebilir:
  - PMM/allocator, VM/address-space, reclaim/OOM, topology/NUMA ve perf satirlarinin hicbiri sessiz corruption veya sahte basari siniri tasimaz
  - map/unmap/COW/hugepage/TLB shootdown davranislari mekanik corpus ve smoke ile dogrulanir
  - topology/hotplug/rseq/per-cpu publication boundary'leri yazili ve testli olur
  - perf/latency/atomic/cache-line audit'i "warning backlog" degil kapanmis contract haline gelir
  - repo-geneli build/test kiriklari memory exactness sinyalini maskelemeyecek kadar ayristirilir

---

## Faz 10 - UI / polish / productization

### 10.1 Engine exactness
- Durum: `In Progress`
- Eksikler:
  - retained engine product tuning
  - render/perf exactness

### 10.2 Product polish
- Durum: `In Progress`
- Eksikler:
  - shell grammar
  - spacing/token polish
  - final UX quality pass

### 10.3 Application Model / Core System Services
- Durum: `In Progress`
- Kapsam:
  - app identity / package registry
  - brokered process + MMU isolation contract
  - state serialization / suspend-resume
  - unified control-plane service model

#### 10.3.a Exactness gorevleri
- Durum: `In Progress`
- Gorevler:
  - `AM-1`: `AppIdentity` ve `PackageRegistry` contract'ini built-in app, installed package ve external image resolution'u ayni truth surface'te birlestirecek sekilde yaz ve uygula
    Durum: `Verified`
  - `AM-2`: `ProcessBroker`u tek privileged spawn authority yap; capability token publication, child-process tree ownership ve launch-time policy gate'i ayni omurgada birlestir
    Durum: `Verified`
  - `AM-3`: app-facing `WarmSuspend` / `ColdResume` state contract'ini yaz; `prepare_suspend`, `export_state`, `import_state`, `resume` seam'lerini en az bir stateful app sinifinda corpus ile dogrula
  - `AM-4`: process launch'i VM/MMU contract'ina bagla; per-app address space, broker-mapped IPC region ve revoke/teardown semantics'ini ABI personality'den bagimsiz tek izolasyon policy'sine indir
  - `AM-5`: text manifest'i runtime hot path'ten cikar; source manifest -> compiled binary manifest pipeline'i ve deterministic `no_std` parse contract'ini kur
  - `CS-1`: `NetworkBroker`, `PackageRegistry` ve install/update/remove akisini privileged service boundary olarak satirlastir
  - `CS-2`: core services icin crash/restart/rebind ve typed deny/error reason modelini yazili contract haline getir
- Kapanis kapisi:
  - built-in/install/external app resolution'un tek registry modeliyle dogrulanmasi
  - privileged launch'in tek brokered MMU/capability yolundan gecmesi
  - en az bir stateful app icin suspend/resume corpus'u
  - control-plane bus ile out-of-band data plane ayriminin testli kalmasi
  - deny/crash/restart yollarinin sessiz degil typed ve user-visible olmasi

#### 10.3.b AM-1 / AM-2 kapanis notu
- Durum: `Verified`
- `AM-1` repo-visible contract'i artik [src/gui/launch_pipeline.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/gui/launch_pipeline.rs), [src/security/package.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/security/package.rs) ve [src/runtime.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/runtime.rs) uzerinden tek truth surface'te calisir:
  - built-in app, installed package, external candidate ve external image resolution'u ayni `PackageRegistryEntry` modeliyle yayinlanir
  - alias, app-id ve file-association resolution'u ayni registry seam'ine baglidir
  - package registry bus yuzeyi ayni truth'u [src/ipc/service_ipc.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/ipc/service_ipc.rs) uzerinden yayinlar
- `AM-2` repo-visible contract'i artik [src/runtime.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/runtime.rs) ve [src/ipc/service_ipc.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/ipc/service_ipc.rs) uzerinden tek privileged launch omurgasina baglidir:
  - `ProcessBroker` capability token publication'ini ve launch-time policy gate'i tek otoritede toplar
  - parent/child process tree ownership'i `BrokeredLaunch` ve broker introspection bus contract'i ile yayinlanir
  - runtime task -> broker ticket -> child ticket zinciri mekanik corpus ile dogrulanir
- Mekanik kanit:
  - `runtime::tests::package_registry_entries_unify_built_in_and_installed_truth_surface`
  - `runtime::tests::process_broker_records_child_tree_under_parent_ticket`
  - `ipc::service_ipc::tests::package_registry_service_lists_built_in_entries`
  - `ipc::service_ipc::tests::process_broker_service_describes_registered_launch_and_children`
  - `cargo test --no-run --target x86_64-pc-windows-msvc --lib`
