# echOS TODO

Tarih: 2026-03-14

Bu dosya, echOS icin repo-kok backlog'udur.
Amac, "ne gercekten calisiyor", "ne davranissal olarak yari-acik", "ne hala fidelity/exactness kuyrugunda" ve
"hangi sirayla kapatilacak" sorularini tek yerde sabitlemektir.

## 2026-03-19 Urun Karari

- Network yuzeyi yeniden aktif urun hedefinde.
- `net`, `dns`, `ping`, `http`, `wget`, `curl` shell komutlari tekrar acik.
- Faz 1 ve Faz 3 historical degil; aktif urun backlog'u olarak kalir.

## 2026-04-07 Truthfulness Refresh

- `target/x86_64-pc-windows-msvc/debug/echsdk.exe exactness strict` mevcut declared Win32 surface icin yesil; bu sinyal sonsuz Win32 ecosystem parity iddiasi olarak okunmayacak.
- Faz 1/Faz 2/Faz 3 kapanis notlari bundan sonra `Verified core` veya `Verified advanced fidelity` diliyle okunur; full parity ancak exactness kapilari ve yeni compatibility familyalariyla yeniden kazanilir.
- Nisan 2026'da eklenen loopback image mount lane, explicit seed-store/runtime curated package lane, curated-app commercial-safe packaging gate ve accessibility TTS/speech playback yuzeyleri backlog'a ayrik satirlar olarak eklendi.

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
- `Verified core`
- `Verified advanced fidelity`
- `Yapildi, gercek ortamda test edilmedi`

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
- Durum: `Verified core`
- Faz 1 artik network core ve truthful client bridge fazi olarak kapandi.
- Interoperability, production-grade trust, QUIC/HTTP3 exactness, IPv6 operational coverage, CNI apply, eBPF runtime ve netfilter hardening Faz 3'e tasindi.
- Faz 1'in `tam parity` sayilmasi icin Faz 3 altindaki advanced-fidelity programinin kapanmasi ve matrix disi compatibility ailelerinin yeniden acilmamasi gerekir.

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
- Durum: `Verified core`
- Faz 2 artik Win32/PE/CRT runtime core fazi olarak kapandi.
- `user32/gdi32` exactness, COM/OLE automation long-tail, Schannel-grade TLS, CRT long-tail ve loader/unwind exact-behavior backlog'u Faz 4'e tasindi.
- `echsdk exactness strict` mevcut declared surface icin yesil olsa da Faz 2 bunu sonsuz Win32 parity olarak yorumlamaz; Faz 4 ve browser compatibility programi acik kaldigi surece yalnizca bounded core closure sayilir.

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
- Durum: `Verified advanced fidelity`
- Faz 1/Faz 3 repo-visible supported scope'unda advanced-fidelity seviyesinde kapanmistir:
  - `N-TRUST-*`, `N-PROTO-*`, `N-OPS-*` gorevlerinin tamami `Verified`
  - network capability matrix'te Faz 1/Faz 3 yuzeylerinin hicbiri desteklenen scope icinde `Partial` sinir tasimiyor
  - shell, legacy facade ve WinHTTP/WinINet bridge ayni network gercegini raporluyor
  - strict source audit'te `src/net/netdev.rs`, `src/net/doh.rs`, `src/net/dot.rs`, `src/net/http.rs`, `src/net/http2.rs` ve `src/net/quic.rs` uzerindeki stale `stub` / `NotSupported` / `simplified` boundary'leri kapanmis durumda
  - HPACK Huffman-flagli string decode'u mekanik corpus ile kapali
  - not: bu closure matrix disindaki ancillary veya future-facing unsupported network surface'lerini degil, repo-visible supported scope'u kapsar; tam parity olarak okunmaz
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

#### 4.4.b PE Compatibility Analyzer + Execution Nucleus
- Durum: `In Progress`
- Amac:
  - V1 icin "her exe acilir" iddiasi degil, secilmis x86_64 PE fixture'lari icin dogru loader / VM / TLS / thread / env / minimal window-message loop execution substrate'i kurmak
  - EGO optimizer veya graphics/game compatibility baslatmadan once gozlemlenebilir ve fail-closed PE runtime cekirdegi elde etmek
  - basarisizliklari silent success yerine reason code, API adi, caller module, binary fingerprint, semantic class ve recommended next task ile raporlamak
- V1 yasaklari:
  - genel `.text` rewrite
  - call-site patching
  - function body rewrite
  - prologue/epilogue manipulation
  - unwind-aware binary rewriting
  - SEH scope ici kod degisimi
  - D3D11/D3D12 translation
  - graphics bridge
  - EGO optimization/cache/policy engine
  - full COM / registry / shell / services klonu
  - anti-cheat / DRM / launcher hedefleme
- V1 serbest alan:
  - offline PE parse/analyze
  - import/export/relocation/TLS/unwind/delay-load metadata okuma
  - binary fingerprint ve risk raporu
  - import-level prebinding plan
  - IAT seviyesinde guvenli loader fastpath tablosu
  - dar execution nucleus
  - explicit unsupported / fail-closed API reporting
  - EGO observe-only trace noktasi

##### 4.4.b.1 Vertical Slice 1 - PE Compatibility Analyzer
- Durum: `In Progress`
- Sinif: `Tooling only`
- Hedef dosyalar/moduller:
  - [tools/pe_analyzer/](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/tools/pe_analyzer/)
  - [tests/pe/](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/tests/pe/)
  - [docs/agent/pe-compatibility-nucleus.md](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/docs/agent/pe-compatibility-nucleus.md)
- Gorevler:
  - `PE-A-01`: analyzer crate/tool iskeleti kur; runtime'a baglama yapma
    Durum: `In Progress`
    Kanit: `COMPILE_ONLY`
  - `PE-A-02`: DOS header, PE signature, COFF header, optional header ve data directories parse et
    Durum: `In Progress`
    Kanit: `HOST_FIXTURE`
  - `PE-A-03`: section table, entry point, image base, subsystem ve machine type raporu uret
    Durum: `In Progress`
    Kanit: `HOST_FIXTURE`
  - `PE-A-04`: import/export graph, dependency manifest ve forwarded export risklerini uret
    Durum: `In Progress`
    Kanit: `HOST_FIXTURE`
  - `PE-A-05`: relocation manifest ve ASLR/map gereksinimi raporu uret
    Durum: `In Progress`
    Kanit: `HOST_FIXTURE`
  - `PE-A-06`: TLS directory, TLS callbacks ve beklenen callback sirasini raporla
    Durum: `In Progress`
    Kanit: `HOST_FIXTURE`
  - `PE-A-07`: `.pdata/.xdata` unwind metadata summary uret; rewrite planlama yapma
    Durum: `In Progress`
    Kanit: `HOST_FIXTURE`
  - `PE-A-08`: delay-load imports ve thunk zincirlerini raporla
    Durum: `In Progress`
    Kanit: `HOST_FIXTURE`
  - `PE-A-09`: compiler fingerprinting, packer/obfuscation supheleri ve static risk classification uret
    Durum: `In Progress`
    Kanit: `HOST_FIXTURE`
  - `PE-A-10`: binary fingerprint, reject reason set, risk report, prebinding candidate list ve `uygun/riskli/reddet` karari uret
    Durum: `In Progress`
    Kanit: `HOST_FIXTURE`
- Kapanis kapisi:
  - PE_FIXTURE_001-008 analyzer tarafinda deterministic rapor uretir
  - malformed PE, packed/obfuscated suphe, unsupported machine ve guvensiz section shape optimistic accept almaz
  - loader'in tuketecegi manifest schema sabitlenir

##### 4.4.b.2 Vertical Slice 2 - Loader + VM + TLS
- Durum: `In Progress`
- Sinif: `Existing surface extension` + `New module required`
- Hedef dosyalar/moduller:
  - [src/pe_loader.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/pe_loader.rs)
  - [src/runtime_layer/pe_runtime.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/runtime_layer/pe_runtime.rs)
  - [src/win32_abi.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/win32_abi.rs)
- Gorevler:
  - `PE-L-01`: analyzer manifest'ini loader launch gate girdisi yap; fingerprint mismatch -> fail-closed
    Durum: `In Progress`
    Kanit: `HOST_FIXTURE`
  - `PE-L-02`: PE image mapping, section mapping ve RX/RW/R protection ayrimini runtime'da uygula
    Durum: `In Progress`
    Kanit: `ECHOS_RUNTIME`
  - `PE-L-03`: relocation table uygulamasini manifest ile dogrula; missing reloc durumunu typed reject'e bagla
    Durum: `In Progress`
    Kanit: `ECHOS_RUNTIME`
  - `PE-L-04`: normal import resolution'i user-callable veneer/IAT path'e bagla; silent zero yok
    Durum: `In Progress`
    Kanit: `ECHOS_RUNTIME`
  - `PE-L-05`: delay-load import metadata ve delay-IAT patch planini veneer path'e al
    Durum: `In Progress`
    Kanit: `HOST_FIXTURE` -> `ECHOS_RUNTIME`
  - `PE-L-06`: TLS callback order'i entrypoint oncesi dogrula; callback sirasi bozulursa launch deny
    Durum: `In Progress`
    Kanit: `ECHOS_RUNTIME`
  - `PE-L-07`: minimal VM semantics kur: reserve/commit/protect/free, image regions, stack guard, heap region, VirtualQuery-lite
    Durum: `In Progress`
    Kanit: `ECHOS_RUNTIME`
  - `PE-L-08`: VM optimizasyonlarini dogruluk bitene kadar kapali tut; RWX kolayciligi ve fake VirtualProtect yasak
    Durum: `In Progress`
    Kanit: `ECHOS_RUNTIME`
- Kapanis kapisi:
  - PE_FIXTURE_001 console hello / ExitProcess runtime'da biter
  - PE_FIXTURE_002 imports + relocation runtime'da biter
  - PE_FIXTURE_003 TLS callback order mekanik olarak dogrulanir
  - PE_FIXTURE_004 VirtualAlloc / VirtualProtect / VirtualFree stateful davranir

##### 4.4.b.3 Vertical Slice 3 - Runtime Nucleus + Win32 Surface Min
- Durum: `In Progress`
- Sinif: `Existing surface extension` + `New module required`
- Hedef dosyalar/moduller:
  - [src/runtime_layer/pe_runtime.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/runtime_layer/pe_runtime.rs)
  - [src/win32.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/win32.rs)
  - [src/win32_abi.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/win32_abi.rs)
  - [run_qemu.ps1](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/run_qemu.ps1)
- Gorevler:
  - `PE-N-01`: minimal PEB/TEB-lite kur; GS:[0x60] -> PEB, TLS slots, last-error, stack bounds ve client id dogru olsun
    Durum: `In Progress`
    Kanit: `ECHOS_RUNTIME`
  - `PE-N-02`: secondary thread bootstrap'i user-mapped TEB + user stack contract'a tasir
    Durum: `In Progress`
    Kanit: `ECHOS_RUNTIME`
  - `PE-N-03`: basic thread/env path'i kur: CreateThread, WaitForSingleObject, GetCurrentThreadId, SetLastError/GetLastError, TLS slot
    Durum: `In Progress`
    Kanit: `ECHOS_RUNTIME`
  - `PE-N-04`: basic file/path/env yuzeyi kur: cwd, command line, env block, minimal open/read path; unsupported path typed reason dondursun
    Durum: `In Progress`
    Kanit: `ECHOS_RUNTIME`
  - `PE-N-05`: heap basics: GetProcessHeap, HeapAlloc, HeapFree, process heap identity
    Durum: `In Progress`
    Kanit: `ECHOS_RUNTIME`
  - `PE-N-06`: timing/sync basics: Sleep, thread wait, event-lite; APC/IOCP v1 disi explicit unsupported
    Durum: `In Progress`
    Kanit: `ECHOS_RUNTIME`
  - `PE-N-07`: minimal window/message loop: RegisterClassEx, CreateWindowEx, DefWindowProc, GetMessage/PeekMessage/DispatchMessage, PostMessage, ShowWindow, UpdateWindow
    Durum: `In Progress`
    Kanit: `ECHOS_RUNTIME` -> `QEMU_BOOT`
  - `PE-N-08`: missing API fail-closed telemetry: reason code, API, caller module, fingerprint, semantic class, recommended task
    Durum: `In Progress`
    Kanit: `ECHOS_RUNTIME`
  - `PE-N-09`: x64 unwind/exception minimum: RtlLookupFunctionEntry, RtlVirtualUnwind, dispatcher visibility, unhandled exception -> typed terminate
    Durum: `In Progress`
    Kanit: `ECHOS_RUNTIME`
  - `PE-N-10`: packaged PE QEMU smoke: install -> launch -> CreateWindowEx marker -> close/exit marker
    Durum: `In Progress`
    Kanit: `QEMU_BOOT`
- Kapanis kapisi:
  - PE_FIXTURE_005 thread create / wait / TLS slot yesil
  - PE_FIXTURE_006 minimal window + message loop QEMU'da yesil
  - PE_FIXTURE_007 controlled exception / unwind metadata inspection yesil
  - PE_FIXTURE_008 missing API fail-closed report yesil
  - `COMPILE_ONLY` tek basina davranis kaniti sayilmaz

##### 4.4.b.4 V1 fixture ailesi
- Durum: `In Progress`
- Sinif: `Test fixture only`
- Fixture listesi:
  - `PE_FIXTURE_001`: console hello / ExitProcess
    Kanit: `ECHOS_RUNTIME`
  - `PE_FIXTURE_002`: imports + relocation
    Kanit: `ECHOS_RUNTIME`
  - `PE_FIXTURE_003`: TLS callback order
    Kanit: `ECHOS_RUNTIME`
  - `PE_FIXTURE_004`: VirtualAlloc / VirtualProtect / VirtualFree
    Kanit: `ECHOS_RUNTIME`
  - `PE_FIXTURE_005`: thread create / wait / TLS slot
    Kanit: `ECHOS_RUNTIME`
  - `PE_FIXTURE_006`: minimal window + message loop
    Kanit: `QEMU_BOOT`
  - `PE_FIXTURE_007`: controlled exception / unwind metadata inspection
    Kanit: `ECHOS_RUNTIME`
  - `PE_FIXTURE_008`: missing API fail-closed test
    Kanit: `ECHOS_RUNTIME`

##### 4.4.b.5 V1 acceptance ve telemetry contract'i
- Durum: `In Progress`
- Zorunlu reason code aileleri:
  - `PeMalformedHeader`
  - `PeUnsupportedMachine`
  - `PePackedOrObfuscated`
  - `PeWritableExecutableSection`
  - `PeMissingRelocations`
  - `PeUnsupportedImport`
  - `PeTlsUnsupportedShape`
  - `PeUnwindMetadataInvalid`
  - `PeUnwindUnsupportedOpcode`
  - `PeDispatcherContractViolation`
  - `PeDelayImportUnsupportedTarget`
  - `PrebindFingerprintMismatch`
  - `PrebindImportMissing`
  - `PrebindDllGraphMismatch`
  - `Win32ApiUnsupported`
- Runtime telemetry minimum alanlari:
  - binary fingerprint
  - module/API name
  - caller module
  - semantic class
  - loader phase
  - reason code
  - recommended next implementation task
- Kapanis kapisi:
  - unsupported API silent success uretmez
  - analyzer decision log, import resolution log, TLS callback trace, VM mapping trace ve missing API report en az fixture yolunda gorulur
  - EGO bu fazda observe-only kalir; optimization/cache/policy yok

##### 4.4.b.6 V1 sonrasi tasinacaklar
- Durum: `Future phase`
- V1 sonrasi:
  - `PE-P2-01`: COM-lite, yalniz V1 fixture'lari COM talep ederse read-only/minimal profile olarak acilacak
  - `PE-P2-02`: registry-lite, yalniz hedef binary zorunlu kiliyorsa read-only profile olarak acilacak
  - `PE-P2-03`: Raw Input ve daha genis input stack
  - `PE-P2-04`: SendMessage reentrancy, nested modal loops ve advanced USER32 message ordering
  - `PE-P2-05`: broad msvcrt/kernelbase/kernel32 long-tail
  - `PE-P2-06`: SxS manifests, activation context, installer/updater flows
  - `PE-P2-07`: browser-class helper process lifecycle ve sandbox hardening
  - `PE-P2-08`: DXGI/D3D11/D3D12, graphics bridge ve frame pacing
  - `PE-P2-09`: 32-bit WoW64
  - `PE-P2-10`: anti-cheat/DRM/protected-process familyalari
  - `PE-P2-11`: EGO optimization/cache/policy engine; ancak V1 analyzer+nucleus telemetry guvenilir olduktan sonra
- V1 sonrasi icin red line:
  - V1 bitmeden `.text` rewrite, call-site patching, D3D translation veya EGO guarded fast path acilmayacak

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
- Faz 4 su an tam parity/exact closure sayilamaz:
  - `W-COM-03`, `W-CRT-02`, `W-UI-02` ve `W-GDI-02` repo-visible bounded corpus icin guclu coverage tasisa da byte-accurate ecosystem long-tail kapanmis degil
  - `W-GFX-*` tarafinda [src/ecosystem.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/ecosystem.rs) artik synthetic completion uydurmaz, `queued-without-display-feedback` sonucunu acikca yayimlar ve native GPU/display lane varsa `dxgi=>display-native/dxgi-native-present` route'una cikabilir; buna ragmen D3D11/D3D12 ile fallback DXGI halen translation-profile tabanlidir ve gercek present completion / fullscreen / pacing / input-latency parity'si kapanmis degildir
  - `BROW-*` tarafinda [src/runtime_layer/runtime_spawn.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/runtime_layer/runtime_spawn.rs), [src/runtime_layer/runtime_state.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/runtime_layer/runtime_state.rs) ve [src/ironshim_app.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/ironshim_app.rs) browser-class shell PE launch'lar icin structured runtime graph publication, broker-child helper runtime attach'i ve helper-role aware bridge policy tasir; buna ragmen executable helper-process lifecycle, sandbox hardening ve full browser platform contract closure'u yok
  - `target/x86_64-pc-windows-msvc/debug/echsdk.exe exactness strict` artik browser/gfx long-tail aciklarini mekanik blocker olarak gorur; yesil gecse bile Faz 4 long-tail parity'sinin tumunu tek basina ispat etmez
- Acik boundary:
  - [src/win32.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/win32.rs) icindeki `WIN32_EXACTNESS_BOUNDARIES` artik browser runtime graph, DXGI completion truthfulness ve translation-profile boundary'lerini fail-closed olarak tasir
  - daha genis vendor/out-of-tree ecosystem delta'lari veya yeni compatibility familyalari yine ayri backlog/corpus ile acilmalidir

### 4.6 Browser binary compatibility expansion

- Durum: `In Progress`
- Kapsam:
  - browser-class Windows binary compatibility Phase 4 icindeki en buyuk acik compatibility programidir
  - [docs/agent/curated-app-compatibility-matrix-2026-04-08.md](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/docs/agent/curated-app-compatibility-matrix-2026-04-08.md) browser-class shell ailelerini bugunku curated contract'ta explicit unsupported boundary olarak yazar; buna ragmen Phase 4 parity hedefi acisindan bu lane acik kalir

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
  - bugunku kodda browser lane'in yeni truth surface'i alias resolution/probe uzerine structured runtime graph publication ekler; brokered launch state artik reserved/preflight-blocked/spawned asamalarini, primary import blocker'i, projected helper role topology'sini, broker-child helper runtime attach zincirini ve image/cwd/download roots gibi workflow contract'ini tasir
  - [src/runtime.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/runtime.rs), [src/runtime_layer/runtime_spawn.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/runtime_layer/runtime_spawn.rs), [src/runtime_layer/runtime_state.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/runtime_layer/runtime_state.rs) ve [src/ironshim_app.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/ironshim_app.rs) browser-class PE yolunda artik unresolved import graph'ini yalniz mesaj olarak degil, process-broker kaydi ve bridge launch envelope icinde de raporlar; ready helper role'leri icin broker child tree acilir, runtime registry'ye attach edilir ve helper-role aware policy ile kisitlanir, fakat helper-process executable spawn/lifecycle, sandbox, graphics, downloads-save/open-folder ve session closure henuz yok
  - [src/gui/launch_pipeline.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/gui/launch_pipeline.rs) `ShellOwnedExternal` event-loop descriptor'u yayinlar ama bu tek basina browser runtime parity kaniti degildir
- Kapanis kapisi:
  - Firefox/Chromium/CEF ailesinden en az bir binary icin gercek process tree + child helper spawn + pencere/message-loop smoke
  - download/save/open-folder ve proxy/TLS/session davranisinin browser workload'u altinda corpus ile pinlenmesi
  - launcher/probe/diagnostic seviyesinden gercek runtime compatibility seviyesine gecildigini gosteren mekanik ve saha smoke'u

### 4.7 V1 application floor / release confidence base
- Durum: `In Progress`
- Amac:
  - v1 guveni "sonsuz Win32 parity" ile degil, ilk gun is gorecek temel uygulama tabani ile kur
  - release kapisini browser-class veya game-facing parity yerine stabil native desktop omurgasi ve dar bir utility compatibility ailesi uzerinden tanimla
  - Faz 4 compatibility programini product truth surface ile hizala: hangi uygulama aileleri v1 release guveni icinde, hangileri browser/gfx long-tail backlog'unda kalacak acik yaz
- V1 first-party floor:
  - `terminal`: PTY, clipboard, open/save/pick-folder, command routing ve shell ergonomisi stabil olacak
  - `files`: mounted storage, path navigation, open-with, drag/drop benzeri temel dosya akislari ve typed failure propagation stabil olacak
  - `editor`: metin acma/duzenleme/kaydetme, dosya association ve clipboard akisi stabil olacak
  - `settings`: desktop/session/policy toggles ve package/update girisleri tek truth surface'te calisacak
  - `seed catalog / package surface`: install/update/remove/retry durumlari shell + UI + control-plane boyunca ayni typed state modelini koruyacak
  - `native web shell`: bounded "hafif web erisimi" hedefiyle kalacak; browser-class binary parity v1 cikis kapisi olmayacak
- V1 compatibility floor:
  - hedef aile: shell-owned PE text/console ve kucuk utility uygulamalar
  - hedef siniflar: editor/navigator, terminal workspace, text-first observability/git/markdown, search/viewer, bounded API client
  - v1 cikis kapisi disi: browser-class GUI shell'ler, Electron/CEF, agir desktop GUI stack'leri, installer/updater binary'leri, service/driver/extensibility akislari, game-facing graphics parity
  - [docs/agent/curated-app-compatibility-matrix-2026-04-08.md](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/docs/agent/curated-app-compatibility-matrix-2026-04-08.md) v1-in / v1-out ailelerini Faz 4 truth surface'inin product-facing matrisi olarak tasimaya devam edecek
- Exactness gorevleri:
  - `V1-APP-01`: first-party `terminal/files/editor/settings` lane'lerini tek release-floor matrisiyle sabitle
    Durum: `In Progress`
  - `V1-APP-02`: package/update/seed catalog yuzeyinde "kurulu mu / guncel mi / retryable mi / quarantine mi" state'lerini user-visible ve typed halde koru
    Durum: `In Progress`
  - `V1-APP-03`: curated compatibility hedefini shell-owned utility ailesiyle sinirla; browser/game parity'yi v1 release blocker'i olmaktan cikar
    Durum: `In Progress`
  - `V1-APP-04`: first-party floor icin saha smoke listesi uret: cold boot -> terminal -> files -> editor -> settings -> package install/update -> bounded web shell
    Durum: `In Progress`
- Kapanis kapisi:
  - cold boot sonrasi first-party floor uygulamalari aciliyor, temel akislari typed failure olmadan tamamliyor
  - package/update/seed catalog state'i shell, UI ve service boundary'lerinde ayni sonucu veriyor
  - curated app matrix v1'in hedef utility ailesini ve v1 disi aileleri acik yaziyor
  - browser-class / DXGI / game-facing parity backlog'u acik kalirken v1 release iddiasi yine de durust kaliyor

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
- Durum: `Verified`
- Acik boundary:
  - XFS halen explicit unsupported boundary ile fail-closed; bu artık parity blocker degil, capability matrix'te acik yazili supported-boundary
  - wider field/appliance smoke repo-visible exactness kapanisindan ayri izlenmeli

#### 6.1.a Exactness gorevleri
- Durum: `Verified`
- Gorevler:
  - `F-VFS-01`: unified VFS open/read/stat/df yuzeyinde unsupported capability icin tek hata contract'i kullan
    Durum: `Verified`
  - `F-VFS-02`: mount routing, root-vs-entry ayrimi, virtual filesystem directory/file semantics ve path normalization edge-case'lerini kapat
    Durum: `Verified`
  - `F-VFS-03`: shell/store/gui istemcilerinin VFS hatalarini ayni gerceklik seviyesiyle yuzeye cikarmasini sagla
    Durum: `Verified`
- Kapanis kapisi:
  - `fs::vfs_unified::tests::mount_resolution_normalizes_separators_and_respects_boundaries`
  - `fs::vfs_unified::tests::xfs_unwired_capabilities_share_one_contract_surface`
  - `fs::vfs_unified::tests::missing_mount_contract_is_shared_across_open_read_and_list`
  - `services::ech_store::tests::store_read_and_metadata_errors_preserve_vfs_contract`
  - `services::ech_store::tests::store_directory_errors_preserve_vfs_contract`
  - `gui::client::tests::gui_store_helpers_preserve_exact_store_errors`
  - `gui::client::tests::gui_shell_file_access_helper_preserves_exact_shell_errors`

### 6.2 Filesystem backends
- Durum: `Verified`
- Acik boundary:
  - wider mounted field smoke/appliance corpus coverage ileri validation lane olarak acik, fakat repo-visible backend/event contract blocker degil
  - unsupported backend ve unsupported on-disk shape'lar capability matrix'te explicit fail-closed boundary olarak kalmali
  - F2FS file-encryption helper lane exact backend gelene kadar explicit `NotSupported` boundary ile fail-closed kalmali; bu unsupported boundary, repo-visible exactness ile celişmez

#### 6.2.a Exactness gorevleri
- Durum: `Verified`
- Gorevler:
  - `F-BTRFS-01`: superblock/tree/inode/data-path yollarini placeholder durumdan cikarip gercek read/write/error semantics'e bagla
    Durum: `Verified`
  - `F-F2FS-01`: compression, allocation, recovery, inode identity, exact-sized read ve overwrite-truncate davranislarini edge-case seviyesinde daralt
    Durum: `Verified`
  - `F-NOTIFY-01`: inode/path mapping, watch identity ve event ordering contract'ini placeholder hash modelinden cikar
    Durum: `Verified`
  - `F-PKG-01`: package/install storage path'ini extraction/install/update yolunda gercek backend davranisina bagla
    Durum: `Verified`
- Kapanis kapisi:
  - `fs::vfs_unified::tests::btrfs_mounted_backend_supports_open_read_list_and_df`
  - `fs::vfs_unified::tests::btrfs_mount_rejects_multi_device_image`
  - `fs::vfs_unified::tests::btrfs_mount_rejects_compressed_extent_image`
  - `fs::vfs_unified::tests::f2fs_vfs_info_preserves_real_inode_identity`
  - `fs::vfs_unified::tests::f2fs_exact_read_len_tracks_full_file_size`
  - `fs::inotify::tests::namespace_aware_targets_do_not_collapse_same_inode_across_mounts`
  - `fs::inotify::tests::dispatch_targets_only_matching_namespace_watchers`
  - `fs::inotify::tests::same_namespace_different_inodes_do_not_collapse`
  - `fs::inotify::tests::move_events_keep_cookie_and_order_for_parent_watchers`
  - `fs::inotify::tests::delete_and_move_self_events_target_only_watched_inode`
  - `fs::inotify::tests::store_style_file_sequences_preserve_parent_and_self_ordering`
  - `fs::inotify::tests::store_style_directory_sequences_preserve_isdir_flag`
  - `services::ech_store::tests::truncate_after_overwrite_only_shrinks_when_needed`
  - `fs::f2fs::tests::incompressible_lz4_payload_falls_back_to_raw_copy_contract`
  - `fs::f2fs::tests::encryption_helpers_fail_closed_without_exact_backend`
  - `security::seed_store::tests::mounted_seed_roots_and_loop_images_cover_install_and_update_lifecycle`

### 6.3 Loopback image mount ve seed-store storage lane
- Durum: `Verified`
- Dosyalar:
  - [src/drivers/loopback.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/drivers/loopback.rs)
  - [src/fs/fat.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/fs/fat.rs)
  - [src/fs/mount.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/fs/mount.rs)
  - [src/fs/vfs_unified.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/fs/vfs_unified.rs)
  - [src/security/seed_store.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/security/seed_store.rs)
- Kapanan kapsam:
  - genel loopback block-device lane artik `attach/list/flush/detach/mount/umount` yuzeyiyle shell'e acik
  - FAT32 loopback block read/write ve VFS mount yolu mekanik testlerle pinli
  - ext4/ntfs loop mount resident full-image snapshot zorunlulugundan cikarilip block-backed storage contract'ina indirildi
  - seed-store discovery artik explicit seed mount/source contract'i ile deny-by-default calisiyor
- Acik boundary:
  - xfs/btrfs loop mount supported degil ve fail-closed kalmali
  - runtime curated source availability mounted `/seed` veya loop-image surface'lerine bagli

#### 6.3.a Exactness gorevleri
- Durum: `Verified`
- Gorevler:
  - `F-LOOP-01`: ext4/ntfs loop mount yolunu resident full-image snapshot'tan block-backed storage contract'ina indir
    Durum: `Verified`
  - `F-LOOP-02`: xfs/btrfs loop mount icin explicit unsupported boundary'yi koru veya wired backend kazan
    Durum: `Verified`
  - `F-SEED-01`: seed partition + loop-image + mounted seed roots arasinda retry/update/install corpus'unu stateful lifecycle seviyesinde genislet
    Durum: `Verified`
- Kapanis kapisi:
  - `drivers::loopback::tests::loopback_block_reads_and_writes_roundtrip`
  - `drivers::loopback::tests::loopback_mounts_fat32_image_into_vfs`
  - `drivers::loopback::tests::loopback_mounts_ntfs_image_into_vfs`
  - `security::seed_store::tests::explicit_seed_mount_detection_is_deny_by_default`
  - large image ve seeded-install smoke'u

### Faz 6 exactness kapanis notu
- Durum: `Verified`
- Faz 6 ancak su kosullarda `tam uyumlu/exact` denebilir:
  - `F-VFS-*`, `F-BTRFS-*`, `F-F2FS-*`, `F-NOTIFY-*`, `F-PKG-*`, `F-LOOP-*` ve `F-SEED-*` gorevlerinin tamami `Verified`
  - filesystem capability matrix'te unsupported backend veya unsupported feature lane'leri acik yazili kalir, wired backend'lerde ise silent heuristic veya bos-success yolu kalmaz
  - VFS ve backend kapasite/error/mount semantics'leri shell/store/gui tarafinda ayni contract ile gorunur
  - repository-visible coverage su an bu kosullari sagliyor; auditte yakalanan F2FS inode/read/truncate ve compression-header tutarsizliklari mekanik corpus ile kapanmis, encryption lane ise gercek backend gelene kadar explicit fail-closed unsupported boundary olarak kayda alinmistir
  - daha genis appliance/field smoke gelecekteki validation lane olarak ayri izlenmeli, mevcut exactness blocker'i degil

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
- Durum: `Verified`
- Dosyalar:
  - [src/gfx/velvet_glove.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/gfx/velvet_glove.rs)
  - [src/gfx/shell_invalidation.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/gfx/shell_invalidation.rs)
  - [src/gui/theme.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/gui/theme.rs)
- Kapanan kapsam:
  - shell top bar retained scene commit yoluna tasindi; product chrome artik tam-frame raster fallback'e yaslanmiyor
  - shell row/layout ritmi ve spacing policy'si regression corpus ile pinlendi
  - magnifier capture budget ve viewport contract'i mode-bazli bounded smoke ile sabitlendi
- Acik boundary:
  - bu closure publish sunucusu + echOS yüklü demo PC gerektiren saha update/distribution smoke'unu kapsamaz

### 10.2 Product polish
- Durum: `Verified`
- Dosyalar:
  - [src/gfx/velvet_glove.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/gfx/velvet_glove.rs)
  - [src/gui/theme.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/gui/theme.rs)
  - [src/gui/client.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/gui/client.rs)
- Kapanan kapsam:
  - shell grammar/runtime truth surface locale ve speech state ile ayni UX contract'inda toplandi
  - theme spacing/radius ladder'lari ve screen-policy resolution mekanik corpus ile sabitlendi
  - final UX quality pass'te shell top bar, row rhythm, magnifier ve speech status yuzeyleri ayni product token setiyle hizalandi
- Acik boundary:
  - curated update/distribution saha smoke'u ayri kapanis kapisi olarak 10.4 altinda kalir

### 10.2.a Accessibility captions / magnifier / speech playback
- Durum: `Verified core`
- Dosyalar:
  - [src/services/ech_shell.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/services/ech_shell.rs)
  - [src/gui/client.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/gui/client.rs)
  - [src/gfx/velvet_glove.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/gfx/velvet_glove.rs)
  - [src/audio/tts.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/audio/tts.rs)
  - [assets/tts/voices/README.md](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/assets/tts/voices/README.md)
- Kapanan kapsam:
  - shell-owned accessibility event/caption ve magnifier lanes artik first-class runtime surface
  - local voice assets parse edilip bounded PCM16 speech clip uretiliyor
  - speech playback deadline'i shell/client tick omurgasina bagli; blind timer degil
- Acik boundary:
  - bu yol tam `espeak-ng` parity degil; daha dar source-filter synthesizer
  - audible output fidelity halen `EchAudio` ve downstream device entegrasyonuna bagli

#### 10.2.b Accessibility exactness gorevleri
- Durum: `Verified`
- Gorevler:
  - `UI-A11Y-01`: speech output lane'ini audio backend error/recovery semantics'i ile typed contract'a indir
    Durum: `Verified`
  - `UI-A11Y-02`: magnifier fullscreen/lens/docked modlari icin perf budget ve dirty-region davranisini mekanik smoke ile sabitle
    Durum: `Verified`
  - `UI-A11Y-03`: voice catalog/language expansion'i shell truth surface'ini bozmadan fail-closed secim modeliyle genislet
    Durum: `Verified`
- Kapanis kapisi:
  - `audio::tts::tests::builtin_voice_catalog_parses_real_assets`
  - `audio::tts::tests::synthesized_speech_produces_bounded_pcm`
  - `audio::tts::tests::voice_catalog_and_selection_are_locale_aware_and_fail_closed`
  - `services::ech_audio::tests::audio_service_rejects_invalid_channel_and_empty_payload_with_typed_errors`
  - `services::ech_audio::tests::audio_service_fails_closed_when_stream_queue_is_saturated`
  - `services::ech_shell::tests::speech_output_status_tracks_locale_voice_catalog_and_preference`
  - `services::ech_shell::tests::speech_output_failure_transitions_distinguish_retryable_audio_and_fail_closed_voice_errors`
  - `gfx::velvet_glove::tests::magnifier_capture_budget_and_viewport_stay_bounded_per_mode`
  - real audio playback + captions alignment smoke'u halen saha/cihaz entegrasyonuna bagli ayrik boundary olarak kalir

#### 10.2.c Engine / polish / accessibility exactness kapanis notu
- Durum: `Verified`
- `10.1` artik [src/gfx/velvet_glove.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/gfx/velvet_glove.rs) ve [src/gfx/shell_invalidation.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/gfx/shell_invalidation.rs) uzerinden retained product chrome contract'ina sahiptir:
  - top bar scene-backed retained commit ile tam pencere damage truth'unu ayni compositor yolundan gecirir
  - shell row geometry ve vertical rhythm regression testleri product spacing exactness'ini pinler
  - magnifier capture/viewport budget'i mode bazli bounded smoke ile fail-open olmadan korunur
- `10.2` artik [src/gui/theme.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/gui/theme.rs), [src/gfx/velvet_glove.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/gfx/velvet_glove.rs) ve [src/gui/client.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/gui/client.rs) uzerinden product polish closure'una sahiptir:
  - theme token ladder ve screen policy çözümü mekanik testlerle monotonic/consistent kalir
  - client/shell locale ve speech status UX surface'i ayni typed contract'tan beslenir
  - shell chrome/artboard spacing'i retained scene ve theme policy ile ayrismadan calisir
- `10.2.b` artik [src/services/ech_audio.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/services/ech_audio.rs), [src/audio/tts.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/audio/tts.rs), [src/services/ech_shell.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/services/ech_shell.rs) ve [src/gui/client.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/gui/client.rs) uzerinden repo-visible exactness closure'una sahiptir:
  - audio backend retryable/fail-closed hata semantiklari typed `AudioError` ve `SpeechOutputError` modeliyle yayinlanir
  - voice catalog locale-aware ve fail-closed secim modeliyle shell truth surface'ine baglidir
  - screen reader speech lane retry backoff, preferred/resolved voice durumu ve locale state'i ile user-visible olarak raporlanir
- Mekanik kanit:
  - `audio::tts::tests::voice_catalog_and_selection_are_locale_aware_and_fail_closed`
  - `services::ech_audio::tests::audio_service_rejects_invalid_channel_and_empty_payload_with_typed_errors`
  - `services::ech_audio::tests::audio_service_fails_closed_when_stream_queue_is_saturated`
  - `services::ech_shell::tests::speech_output_status_tracks_locale_voice_catalog_and_preference`
  - `services::ech_shell::tests::speech_output_failure_transitions_distinguish_retryable_audio_and_fail_closed_voice_errors`
  - `gfx::velvet_glove::tests::top_bar_scene_marks_full_window_damage_for_retained_commit`
  - `gfx::velvet_glove::tests::shell_row_layouts_preserve_even_vertical_rhythm`
  - `gfx::velvet_glove::tests::magnifier_capture_budget_and_viewport_stay_bounded_per_mode`
  - `gui::theme::tests::theme_spacing_and_radii_ladders_are_monotonic`
  - `gui::theme::tests::theme_resolution_and_layout_profile_follow_screen_policy`

### 10.3 Application Model / Core System Services
- Durum: `Verified`
- Kapsam:
  - app identity / package registry
  - brokered process + MMU isolation contract
  - state serialization / suspend-resume
  - unified control-plane service model

#### 10.3.a Exactness gorevleri
- Durum: `Verified`
- Gorevler:
  - `AM-1`: `AppIdentity` ve `PackageRegistry` contract'ini built-in app, installed package ve external image resolution'u ayni truth surface'te birlestirecek sekilde yaz ve uygula
    Durum: `Verified`
  - `AM-2`: `ProcessBroker`u tek privileged spawn authority yap; capability token publication, child-process tree ownership ve launch-time policy gate'i ayni omurgada birlestir
    Durum: `Verified`
  - `AM-3`: app-facing `WarmSuspend` / `ColdResume` state contract'ini yaz; `prepare_suspend`, `export_state`, `import_state`, `resume` seam'lerini en az bir stateful app sinifinda corpus ile dogrula
    Durum: `Verified`
  - `AM-4`: process launch'i VM/MMU contract'ina bagla; per-app address space, broker-mapped IPC region ve revoke/teardown semantics'ini ABI personality'den bagimsiz tek izolasyon policy'sine indir
    Durum: `Verified`
  - `AM-5`: text manifest'i runtime hot path'ten cikar; source manifest -> compiled binary manifest pipeline'i ve deterministic `no_std` parse contract'ini kur
    Durum: `Verified`
  - `CS-1`: `NetworkBroker`, `PackageRegistry` ve install/update/remove akisini privileged service boundary olarak satirlastir
    Durum: `Verified`
  - `CS-2`: core services icin crash/restart/rebind ve typed deny/error reason modelini yazili contract haline getir
    Durum: `Verified`
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

#### 10.3.c AM-3 / AM-4 / AM-5 / CS-1 / CS-2 kapanis notu
- Durum: `Verified`
- `AM-3` artik [src/runtime_supervisor.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/runtime_supervisor.rs), [src/bin/echsdk.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/bin/echsdk.rs) ve [src/gfx/velvet_glove/session_runtime.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/gfx/velvet_glove/session_runtime.rs) uzerinden repo-visible suspend/resume contract'ina sahip:
  - compiled manifest state contract'i `WarmSuspend` / `ColdResume` olarak binary schema'da tasinir
  - stateful app ornegi `export_state` / `import_state` / `resume` seam'lerini host uretilen SDK orneginde gercekler
  - runtime supervisor resume bundle header'i generation, digest ve relative payload contract'i ile crash-safe store semantigine baglidir
- `AM-4` artik [src/runtime.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/runtime.rs), [src/ipc/service_ipc.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/ipc/service_ipc.rs) ve [src/ipc/service_ipc/runtime_bridge.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/ipc/service_ipc/runtime_bridge.rs) uzerinden tek izolasyon/policy omurgasina baglidir:
  - packaged Native/PE/ELF launch ayni broker ticket + capability token + address space handle modelinden gecer
  - broker-mapped IPC endpoint generation/rebind semantics'i ABI personality'den bagimsiz ayni service bus contract'inda tutulur
  - revoke/stale-generation/service-unavailable yollarinin hepsi typed service error olarak yayinlanir
- `AM-5` artik [sdk/echos-manifest/src/lib.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/sdk/echos-manifest/src/lib.rs), [src/bin/echsdk.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/bin/echsdk.rs) ve [src/security/package.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/security/package.rs) uzerinden kapanir:
  - text source manifest build/sign asamasinda `CompiledAppManifest` binary'sine donusturulur
  - runtime hot path binary manifest decode eder; text TOML parse'i launch hot path'inde kalmaz
  - deterministic payload hash ve manifest digest contract'i package verify/install zincirinde sabittir
- `CS-1` artik [src/runtime_layer/service_control.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/runtime_layer/service_control.rs), [src/update.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/update.rs), [src/shell/cmd_pkg.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/shell/cmd_pkg.rs) ve [src/ipc/service_ipc.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/ipc/service_ipc.rs) uzerinden repo-visible olarak kapanir:
  - `NetworkBroker` privileged remote fetch authority olarak service directory'de first-class endpoint'tir
  - package install/remove/verify/list/info/search shell yolu `PackageRegistry` service boundary disina cikmaz
  - update inspect/apply/status shell yolu typed `UpdateInstaller` service contract'ina baglidir
- `CS-2` artik [src/ipc/service_ipc/transport.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/ipc/service_ipc/transport.rs), [src/runtime_layer/service_control.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/runtime_layer/service_control.rs) ve [src/posix/service_bridge.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/posix/service_bridge.rs) uzerinden typed deny/restart/rebind modeline sahiptir:
  - `EndpointRestarted`, `ServiceUnavailable`, `RightsDenied`, `WrongService`, `WrongResponseKind` ve benzeri nedenler sessiz degil typed olarak doner
  - package/update/network control-plane servisleri user-visible typed error payload'lari yayinlar
  - service handle generation mismatch ve restart sonrasi rebind gereksinimi mekanik olarak ayrilir
- Mekanik kanit:
  - `ipc::service_ipc::tests::package_registry_service_runs_install_verify_remove_through_control_plane`
  - `ipc::service_ipc::tests::network_broker_returns_typed_invalid_url_error`
  - `runtime::tests::package_registry_entries_unify_built_in_and_installed_truth_surface`
  - `update::tests::signed_index_roundtrips_and_plans`

### 10.4 Seed catalog / curated app distribution / packaging gate
- Durum: `Yapildi, gercek ortamda test edilmedi`
- Dosyalar:
  - [src/security/seed_store.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/security/seed_store.rs)
  - [src/security/package.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/security/package.rs)
  - [scripts/package_curated_apps.ps1](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/scripts/package_curated_apps.ps1)
  - [docs/agent/curated-app-commercial-license-audit-2026-04-07.md](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/docs/agent/curated-app-commercial-license-audit-2026-04-07.md)
  - [docs/agent/curated-app-compatibility-matrix-2026-04-08.md](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/docs/agent/curated-app-compatibility-matrix-2026-04-08.md)
- Kapanan kapsam:
  - curated bundle lane artik `.bhd` + `curated-seed.img` uretebiliyor
  - packaging script GPL/AGPL benzeri disallowed lisanslari fail-closed reddediyor
  - runtime seed catalog mounted seed root ve loop-image kaynagini tek truth surface'te birlestiriyor
- Acik boundary:
  - her curated binary icin runtime application compatibility garanti edilmis degil
  - host-side packaging smoke mevcut audit turunda yeniden kosulmadi
  - curated third-party app lane temizlense de repo-root AGPL posture ayri konu olarak acik
  - publish sunucusu + echOS yüklü demo PC ile saha smoke'u henuz yok

#### 10.4.a Distribution exactness gorevleri
- Durum: `Yapildi, gercek ortamda test edilmedi`
- Gorevler:
  - `DIST-01`: host packaging lane'ini reproducible smoke ile bundle-download -> sign -> seed-image zincirinde mekaniklestir
    Durum: `Yapildi, gercek ortamda test edilmedi`
  - `DIST-02`: seeded app install/update/remove lifecycle'ini UI + shell + runtime tarafinda tek typed state modeline indir
    Durum: `Yapildi, gercek ortamda test edilmedi`
  - `DIST-03`: curated binary compatibility matrisi icin supported/unsupported app family boundary'sini ayri truth table olarak yayinla
    Durum: `Verified`
  - `DIST-04`: engineering PC -> signed update-index -> echOS installer zinciri tamamlandi; `echsdk update publish|inspect`, `UpdateInstaller` service ve `pkg update inspect|apply|status` shell yolu ayni fail-closed planner'a baglandi
    Durum: `Yapildi, gercek ortamda test edilmedi`
  - `DIST-05`: reboot-gerektiren service bundle artifact'lari artik shared live store yerine hedef slotun `/config/update/slot-stage/...` alanina journal ile stage ediliyor; ilk saglikli boot bu journal'i commit edip sonra `mark_boot_success()` yapiyor
    Durum: `Yapildi, gercek ortamda test edilmedi`
- Kapanis kapisi:
  - host curated packaging smoke
  - seeded update/retry/install corpus'u
  - supported curated app profile matrix'i
  - signed update index + staged reboot/rollback corpus'u
  - publish sunucusu + demo PC ile gercek saha publish/pull/apply smoke'u
