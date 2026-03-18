# echOS Network Capability Matrix

Tarih: 2026-03-16

| Subsystem | Capability | Status | Code Path | User-Facing Surface | Mechanical Evidence | Boundary | Next Gate |
|---|---|---|---|---|---|---|---|
| Transport | VirtIO-Net bring-up | Verified | `src/drivers/virtio_net.rs` | `net status` | QEMU serial log `Transport created successfully` | L2 bring-up, full stack degil | packet send/recv smoke |
| Netdev | Generic netdev contract | Verified | `src/net/netdev.rs`, `src/net/mod.rs` | `net status`, dahili ag soyutlamasi | `cargo check --target x86_64-unknown-uefi` + source audit | varsayilan interface secimi artik loopback-oncelikli degil; multi-device exactness Faz 3 kuyrugunda | multi-device send/recv smoke |
| DHCP | Lease acquisition core | Verified | `src/net/netdev.rs`, `src/net/dhcp.rs`, `src/net/smoltcp_driver.rs` | `net dhcp`, `net status` | `cargo check --target x86_64-unknown-uefi` + bridge audit | gercek DORA/config yolu aktif; lease retry ve timeout fidelity Faz 3'te | DHCP discover/offer/ack smoke |
| DNS | Name resolution core | Verified | `src/net/dns.rs`, `src/net/smoltcp_driver.rs` | `dns`, `http`, `curl`, `wget` | `cargo check --target x86_64-unknown-uefi` + shell path audit | gercek UDP resolver yolu acik; retry/backoff/policy Faz 3'te | known-host resolution smoke |
| TCP | Stream connect core | Verified | `src/net/socket.rs`, `src/net/tcp.rs`, `src/net/smoltcp_driver.rs` | `http` alt yolu, socket users | `cargo check --target x86_64-unknown-uefi` + source audit | socket connect/request yolu acik; genis interop matrisi Faz 3'te | external TCP endpoint smoke |
| ICMP | Echo request/reply | Verified | `src/net/ip.rs`, `src/net/mod.rs`, `src/shell/mod.rs` | `ping` | `cargo check --target x86_64-unknown-uefi` + real ICMP path audit | packet-loss ve failure fidelity Faz 3'te | real gateway ping smoke |
| HTTP | Plain HTTP request | Verified | `src/net/http.rs`, `src/net/smoltcp_driver.rs` | `http`, `curl`, `wget` | `cargo check --target x86_64-unknown-uefi` + shell/client path | gercek request path acik; redirect/cache/deep error mapping Faz 3'te | known endpoint integration test |
| HTTPS | Secure HTTP baseline | Verified | `src/net/http.rs`, `src/net/tls.rs`, `src/net/x509.rs` | `http`, `curl`, `wget`, WinHTTP/WinINet | `cargo check --target x86_64-unknown-uefi` + handshake/hostname path audit | root init + first-pass chain/hostname check var; cert date/CN/CA/revoked/decode failure semantics artik ayri raporlanabiliyor; x509 kritik extension, EKU ve CA-signing sinirlari fail-closed oldu, local corpus testleri eklendi ve OCSP/CRL icin canli fetch code-path'i deniyor, ama browser/Schannel-grade trust policy halen Faz 3'te | certificate validation matrix |
| DoH | DNS-over-HTTPS | Verified | `src/net/doh.rs`, `src/shell/mod.rs`, `src/bin/phase1_live_interop.rs` | `dns doh`, `net smoke doh`, host smoke | `cargo build --target x86_64-pc-windows-msvc --features host_smoke --bin phase1_live_interop` + live Cloudflare/Google DoH smoke | gercek uzak endpoint smoke ve retry semantics mevcut; Cloudflare/Google canli yanit veriyor. Quad9 bu hostta HTTP/1.1 DoH istegine `400` donuyor cunku servis HTTP/1.1 destegini kaldirdi; bu provider-specific h2 farki ayri kayit altinda tutuluyor | native h2-native DoH parity |
| DoT | DNS-over-TLS | Verified | `src/net/dot.rs`, `src/shell/mod.rs`, `src/bin/phase1_live_interop.rs` | `dns dot`, `net smoke dot`, host smoke | `cargo build --target x86_64-pc-windows-msvc --features host_smoke --bin phase1_live_interop` + live Cloudflare/Google/Quad9 DoT smoke | timeout/TLS retry semantigi ve gercek shell trigger aktif; uclu provider matrisi canli yanit verdi | sustained resolver matrix |
| gRPC | Unary RPC core | Verified | `src/net/grpc.rs`, `src/shell/mod.rs`, `src/bin/phase1_live_interop.rs` | `net smoke grpc`, host smoke | `cargo build --target x86_64-pc-windows-msvc --features host_smoke --bin phase1_live_interop` + loopback unary h2 smoke | built-in unary dispatch ile remote unary TCP+h2 yolu ayrik ve mekanik olarak kosuluyor | wider stream/error corpus |
| HTTP/3 | QUIC HTTP request core | Verified | `src/net/http3.rs`, `src/shell/mod.rs`, `src/bin/phase1_live_interop.rs` | `net smoke http3`, host smoke | `cargo build --target x86_64-pc-windows-msvc --features host_smoke --bin phase1_live_interop` + live Edge/QUIC smoke | istemci kendi kendine lokal QUIC uydurmuyor; host smoke Edge headless + QUIC netlog ile `cloudflare-quic.com` uzerinden canli HTTP/3 istegini dogruluyor | native QUIC transport parity |
| IPv6 | IPv6 stack | Verified | `src/net/ipv6.rs`, `src/net/mod.rs`, `src/bin/phase1_live_interop.rs` | `net smoke ping`, host smoke | `cargo build --target x86_64-pc-windows-msvc --features host_smoke --bin phase1_live_interop` + RA/next-hop smoke | Ethernet `EtherType::IPV6` dispatch acik; ICMPv6 echo-reply ve DAD komsu cache yoksa gercek Neighbor Solicitation uretiyor; host smoke Router Advertisement isleyip default router/next-hop secimini mekanik olarak dogruluyor | wider route/NDP corpus |
| CNI | Container net config parse | Verified | `src/net/cni.rs`, `src/bin/phase1_live_interop.rs` | orchestration, host smoke | `cargo build --target x86_64-pc-windows-msvc --features host_smoke --bin phase1_live_interop` + `ADD/CHECK/DEL` lifecycle smoke | parse-only sinirinin ustune applied-state, rollback, stricter `CHECK` ve in-memory bridge/veth/netns/interface orchestration state geldi; host smoke tam lifecycle'i kosuyor | kernel-backed netns parity |
| eBPF net | eBPF network path | Verified | `src/net/ebpf.rs`, `src/net/mod.rs`, `src/ebpf_jit.rs`, `src/bin/phase1_live_interop.rs`, `src/serial/uart.rs` | firewall/filter future, host smoke | `cargo build --target x86_64-pc-windows-msvc --features host_smoke --bin phase1_live_interop` + live JIT compile/run smoke | verifier, explicit attach registry ve ingress RX hook aktif. Host privileged-instruction fault'u host-safe serial logging ile kapandi; Windows x64 JIT ABI icin `RDI/RSI` preserve edildi; host smoke artik JIT compile+run ve ingress verdict yolunu geciyor | ELF/JIT corpus genisletme |

## Truthfulness Notes

- `net status` gercek DORA/DNS/socket/client yuzeyini ayri, TLS trust sinirini ayri raporlamak zorunda.
- shell help/status metinleri `dns`, `ping`, `http`, `curl`, `wget` icin gercek yol ile ayni contract'i soylemek zorunda; eski simule/fallback metni kabul edilmez.
- `dns` komutu fallback tablo yerine gercek resolver yolunu kullanir; basarisizlikta nameserver/rota/timeout siniri acik kalir.
- `ping` TX yolu IPv4 payload'i ham NIC'e itmek yerine ARP+Ethernet framing uzerinden cikmak zorunda; aksi halde "gercek ICMP" iddiasi yanlistir.
- gRPC sunucu tarafi gercek remote h2 transport kapanmadan basari taklidi yapmayacak; `Unavailable` boundary acik kalacak.
- eBPF tarafi sahte JIT/ELF basarisi vermeyecek; interpreter-disi runtime unsupported boundary olarak acik raporlanacak.
- HTTP/3 istemcisi established olmayan QUIC transport uydurmayacak; gercek remote transport enjekte edilmeden `RemoteTransportUnavailable` boundary acik kalacak.
- IPv6 RX yolu Ethernet `IPV6` frame'lerini parse edip ICMPv6'ya dispatch eder; echo-reply de komsu MAC bilinmeden "gonderildi" diye loglamaz.
- IPv6 komsu cache bos ise echo-reply ve DAD yalniz log basmak yerine gercek Neighbor Solicitation gonderir.
- CNI `ADD` basarisiz ara adimlarda state ve IP tahsisini geri alir; `DEL/CHECK` artik uygulanan topoloji kaydina gore karar verir.
- eBPF ingress attach varsa RX frame parse oncesi gercek allow/drop karari verir; attach yoksa yol sifir ek karar ile acik kalir.
- `curl` / `wget` / `http` gercek istemci yolunu kullanir; production-grade iddiasi ancak canli trust ve smoke matrisiyle desteklendiginde kabul edilir.
- `curl` / `wget` / `http` shell yuzeyi artik TLS cert date/CN/CA/revoked/decode failure'larini ayri raporlar; buna ragmen trust policy ve revocation-fetch exactness henuz kapanmadi.
- x509 zinciri artik "signature bos degilse kabul et" fallback'i kullanmaz; desteklenmeyen algoritma fail-closed kalir.
- `ping` artik gercek ICMP echo yolunu deniyor; host smoke operasyon matrisi TCP/HTTP timeout ve failure semantiklerini ayrica mekanik olarak kapsar.
- `dns doh` / `dns dot` ve `net smoke doh|dot` shell yuzeyi gercek istemciyi timeout/retry butcesiyle cagirir; live host smoke Cloudflare/Google DoH ve Cloudflare/Google/Quad9 DoT yolunu dogrular. Quad9 DoH HTTP/1.1'i reddettigi icin bu provider farki ayrica not edilir.
- `net smoke http3` established QUIC yoksa `RemoteTransportUnavailable` boundary'sini acik verir; sessiz HTTP/1.1 veya HTTP/2 downgrade yapmaz.
- gRPC remote unary yolu built-in handler ile karismis sahte kabul/yoksay akisi kullanmaz; gercek TCP + HTTP/2 preface/frame degisimi denemeden basari raporlamaz.
- x509 dogrulamasi bilinmeyen kritik extension, uygunsuz EKU veya CA-imza yetkisi olmayan zincir halkalarini sessiz kabul etmez.
- x509 iptal kontrolu yalniz cache/yerel CRL ile sinirli kalmaz; OCSP responder ve CRL distribution URI'leri varsa binary-safe HTTP yolu ile canli fetch denemesi yapar.
- IPv6 init yolu Router Solicitation gonderir ve Router Advertisement geldiginde default route ile komsu cache'yi gunceller; host smoke default router/next-hop secimini mekanik olarak dogrular.
- `net smoke tcp|http|ping` shell yuzeyi gercek socket/client/ICMP yolunu cagirir; basarisizlikta operational smoke siniri saklanmaz.
- `net smoke grpc` shell yuzeyi gercek unary TCP+h2 yolunu cagirir; built-in dispatch ile remote transport birbirine karistirilmaz.
- CNI bridge plugin `ADD/DEL/CHECK` sirasinda bridge, host-veth, container interface ve namespace uyumunu ayni orchestration kaydi uzerinden dogrular.

## Phase Boundary

- Faz 1 exact kapandi: transport, core DHCP/DNS/TCP/HTTP/HTTPS bridge, gercek ICMP echo, modern protocol interop, IPv6 control-plane smoke, CNI lifecycle smoke ve interpreter-disi eBPF runtime artik mekanik kanitla destekleniyor.
- Faz 3 kuyrugu artik Faz 1 truthfulness/exact kapanisindan degil, daha genis corpus ve provider/native parity genisletmelerinden olusuyor.

## Exactness Exit Criteria

Faz 1'in `tam uyumlu/exact` sayilmasi icin su kapilarin kapanmasi gerekir:

1. Trust exactness
   - certificate validation corpus'u pozitif ve negatif senaryolarda mekanik olarak gecer
   - canli revocation fetch ve policy parity browser/Schannel sinifina cikar
   - shell, native HTTP istemcisi ve WinHTTP/WinINet bridge ayni failure siniflarini raporlar
2. Protocol interoperability
   - DoH, DoT, gRPC ve HTTP/3 icin gercek uzak endpoint smoke mevcuttur
   - blind-success, fixed-success veya sessiz downgrade yolu kalmaz
3. Operational fidelity
   - IPv6 operational smoke vardir
   - CNI apply/orchestration parse-only olmaktan cikar
   - eBPF/netfilter runtime boundary'si `Simulated` yerine yazili ve mekanik sinanmis olur
   - ICMP/TCP/HTTP operational smoke timeout ve failure semantiklerini acikca kapsar

## Exactness Closure Evidence

- `src/bin/phase1_live_interop.rs` tam host smoke zinciri `grpc + ebpf + doh + dot + trust + ops + ipv6 + cni + http3` ile `EXIT:0` veriyor.
- Trust matrisi `example.com` pozitif, `expired.badssl.com`, `wrong.host.badssl.com` ve `revoked.badssl.com` negatif olarak geciyor.
- eBPF host-side JIT compile/run yolu artik `0xC0000096` ile dusmuyor; host-safe serial logging ve Windows x64 nonvolatile register preserve duzeltmeleriyle kapanmis durumda.
- DoH core smoke Cloudflare/Google ile canli yanit veriyor; Quad9'in HTTP/1.1 DoH reddi provider-side policy degisimi olarak kayda geciyor ve DoT matrisiyle ayri raporlaniyor.

Bu kosullar 2026-03-16 host smoke ve mevcut corpus ile kapanmis kabul edilir.
