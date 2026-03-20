# echOS Network Capability Matrix

Tarih: 2026-03-16

## Product Status

- 2026-03-19 itibariyla network yuzeyi aktif echOS urun hedefinde kalir.
- Shell `net`, `dns`, `ping`, `http`, `wget`, `curl` komutlari tekrar acik yuzeydir.
- Bu dosya aktif urun ve engineering capability matrix'i olarak kullanilir.

| Subsystem | Capability | Status | Code Path | User-Facing Surface | Mechanical Evidence | Boundary | Next Gate |
|---|---|---|---|---|---|---|---|
| Transport | VirtIO-Net bring-up | Verified | `src/drivers/virtio_net.rs` | `net status` | QEMU serial log `Transport created successfully` | L2 bring-up, full stack degil | packet send/recv smoke |
| Netdev | Generic netdev contract | Verified | `src/net/netdev.rs`, `src/net/mod.rs` | `net status`, dahili ag soyutlamasi | `cargo check --target x86_64-unknown-uefi` + source audit | varsayilan interface secimi artik loopback-oncelikli degil; multi-device exactness Faz 3 kuyrugunda | multi-device send/recv smoke |
| DHCP | Lease acquisition core | Verified | `src/net/netdev.rs`, `src/net/dhcp.rs`, `src/net/smoltcp_driver.rs` | `net dhcp`, `net status` | `cargo check --target x86_64-unknown-uefi` + bridge audit | gercek DORA/config yolu aktif; live shell girisleri runtime bootstrap ile DHCP veya slirp fallback'i tetikliyor; lease retry ve timeout fidelity Faz 3'te | DHCP discover/offer/ack smoke |
| DNS | Name resolution core | Verified | `src/net/dns.rs`, `src/net/smoltcp_driver.rs` | `dns`, `http`, `curl`, `wget` | `cargo check --target x86_64-unknown-uefi` + shell path audit | gercek UDP resolver yolu acik; shell/client entry'leri artik runtime network bootstrap olmadan DNS'e dusmuyor; retry/backoff/policy Faz 3'te | known-host resolution smoke |
| TCP | Stream connect core | Verified | `src/net/socket.rs`, `src/net/tcp.rs`, `src/net/smoltcp_driver.rs` | `http` alt yolu, socket users | `cargo check --target x86_64-unknown-uefi` + source audit | socket connect/request yolu acik; genis interop matrisi Faz 3'te | external TCP endpoint smoke |
| ICMP | Echo request/reply | Verified | `src/net/ip.rs`, `src/net/mod.rs`, `src/shell/mod.rs` | `ping` | `cargo check --target x86_64-unknown-uefi` + real ICMP path audit | packet-loss ve failure fidelity Faz 3'te | real gateway ping smoke |
| HTTP | Plain HTTP request | Verified | `src/net/http.rs`, `src/net/smoltcp_driver.rs` | `http`, `curl`, `wget` | `cargo check --target x86_64-unknown-uefi` + shell/client path | gercek request path acik; redirect/cache/deep error mapping Faz 3'te | known endpoint integration test |
| HTTPS | Secure HTTP baseline | Verified | `src/net/http.rs`, `src/net/tls.rs`, `src/net/x509.rs` | `http`, `curl`, `wget`, WinHTTP/WinINet | `cargo test --target x86_64-pc-windows-msvc --lib "net::x509::tests::"` + handshake/hostname path audit | certificate validation matrix artik negatif trust corpus'u, SAN/CN fallback siniri ve OCSP->CRL precedence ile mekanik olarak daraltildi; repo-visible trust matrix kapali | closed |
| DoH | DNS-over-HTTPS | Verified | `src/net/doh.rs`, `src/shell/mod.rs`, `src/bin/phase1_live_interop.rs` | `dns doh`, `net smoke doh`, host smoke | `cargo test --target x86_64-pc-windows-msvc --lib "net::doh::tests::"` + live Cloudflare/Google DoH smoke | native HTTP/2 DoH request/response yolu artik mevcut; provider h2 gereksinimi corpus ile ayrik dogrulaniyor ve h2-first/fallback contract'i belirgin | closed |
| DoT | DNS-over-TLS | Verified | `src/net/dot.rs`, `src/shell/mod.rs`, `src/bin/phase1_live_interop.rs` | `dns dot`, `net smoke dot`, host smoke | `cargo test --target x86_64-pc-windows-msvc --lib "net::dot::tests::"` + live Cloudflare/Google/Quad9 DoT smoke | sustained resolver matrix artik transient hata donuslerinde provider rotasyonunu ve fail-fast sinirini mekanik olarak dogruluyor | closed |
| gRPC | Unary + streaming RPC core | Verified | `src/net/grpc.rs`, `src/net/http2.rs`, `src/shell/mod.rs`, `src/bin/phase1_live_interop.rs` | `net smoke grpc`, host smoke | `cargo test --target x86_64-pc-windows-msvc --lib "net::grpc::tests::grpc_"` + loopback unary h2 smoke + remote server-streaming loopback corpus + local trailer/status | built-in unary dispatch ile remote unary TCP+h2 yolu ayrik; remote response path HTTP/2 trailers, `grpc-status` / `grpc-message`, HTTP `:status` ve `RST_STREAM` reason bilgisini ayri tasiyor; built-in server/client/bidi streaming core artik coklu gRPC frame akisini koruyor ve remote server-streaming loopback birden fazla mesaji trailer ile birlikte dogruluyor | broader remote service matrix |
| HTTP/3 | QUIC HTTP request core | Verified | `src/net/http3.rs`, `src/net/quic.rs`, `src/shell/mod.rs`, `src/bin/phase1_live_interop.rs` | `net smoke http3`, host smoke | `cargo test --target x86_64-pc-windows-msvc --lib "net::http3::tests::http3_"` + `cargo test --target x86_64-pc-windows-msvc --lib "net::http3::tests::native_connection_roundtrip_preserves_response_trailers" -- --exact` + `cargo test --target x86_64-pc-windows-msvc --lib "net::quic::tests::created_stream_starts_open_with_send_window" -- --exact` + live Edge/QUIC smoke | istemci kendi kendine lokal QUIC uydurmuyor; native transport registry established QUIC baglantisini istemciye rehydrate edebiliyor, stream send penceresi acik geliyor ve HEADERS/DATA/trailing HEADERS ayrimi korunuyor; istemci response surface'i artik trailer'lari da koruyabiliyor, ancak genis canli provider matrisi hala ayri corpus istiyor | broader live QUIC service matrix |
| IPv6 | IPv6 stack | Verified | `src/net/ipv6.rs`, `src/net/mod.rs`, `src/bin/phase1_live_interop.rs` | `net smoke ping`, host smoke | `cargo test --target x86_64-pc-windows-msvc --lib "net::ipv6::tests::"` + RA/next-hop smoke | wider route/NDP corpus artik multicast, neighbor-direct, link-local ve router-gc secimlerini mekanik olarak kapsiyor | closed |
| CNI | Container net config parse | Verified | `src/net/cni.rs`, `src/bin/phase1_live_interop.rs` | orchestration, host smoke | `cargo test --target x86_64-pc-windows-msvc --lib "net::cni::tests::"` + `ADD/CHECK/DEL` lifecycle smoke | orchestration state artik process-local manager'a degil global kernel-owned registry'ye oturuyor; ayrik `ADD/CHECK/DEL` komutlari ayni namespace/veth/ip state'ini goruyor | closed |
| eBPF net | eBPF network path | Verified | `src/net/ebpf.rs`, `src/net/mod.rs`, `src/ebpf_jit.rs`, `src/bin/phase1_live_interop.rs`, `src/serial/uart.rs` | firewall/filter future, host smoke | `cargo test --target x86_64-pc-windows-msvc --lib "net::ebpf::tests::"` + live JIT compile/run smoke | ELF section loader artik test corpus ile socket_filter section, JIT compile ve attach/run verdict zincirini mekanik olarak kanitliyor | closed |

## Truthfulness Notes

- `net status` gercek DORA/DNS/socket/client yuzeyini ayri, TLS trust sinirini ayri raporlamak zorunda.
- shell help/status metinleri `dns`, `ping`, `http`, `curl`, `wget` icin gercek yol ile ayni contract'i soylemek zorunda; eski simule/fallback metni kabul edilmez.
- `dns` komutu fallback tablo yerine gercek resolver yolunu kullanir; basarisizlikta nameserver/rota/timeout siniri acik kalir.
- shell `dns` / `ping` / `http` girisleri gercek resolver yoluna dusmeden once runtime DHCP veya QEMU slirp fallback ile `{ip, gateway, dns}` uclusunu bootstrap etmek zorunda; aksi halde "gercek resolver denendi" mesaji yalana doner.
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
- `dns doh` / `dns dot` ve `net smoke doh|dot` shell yuzeyi gercek istemciyi timeout/retry butcesiyle cagirir; native h2 DoH request/response corpus'u ve sustained DoT provider rotasyonu mekanik olarak green durumdadir. Quad9 DoH HTTP/1.1'i reddettigi icin h2-first contract ayri corpus ile sabitlenir.
- `net smoke http3` established QUIC yoksa `RemoteTransportUnavailable` boundary'sini acik verir; sessiz HTTP/1.1 veya HTTP/2 downgrade yapmaz.
- gRPC remote unary yolu built-in handler ile karismis sahte kabul/yoksay akisi kullanmaz; gercek TCP + HTTP/2 preface/frame degisimi denemeden basari raporlamaz.
- gRPC response parser'i initial HEADERS ile trailing HEADERS'i birbirine ezmez; `grpc-status` / `grpc-message` trailer'da geldiyse unary sonucu buna gore kapatir ve `RST_STREAM` reason bilgisini `Unavailable` altina gizlemez.
- gRPC built-in streaming yollari tek body icinde gelen birden fazla length-delimited mesaji tek yanita ezmez; server/client/bidi davranisi ayrik surface olarak kalir.
- x509 dogrulamasi bilinmeyen kritik extension, uygunsuz EKU veya CA-imza yetkisi olmayan zincir halkalarini sessiz kabul etmez.
- x509 iptal kontrolu yalniz cache/yerel CRL ile sinirli kalmaz; OCSP responder ve CRL distribution URI'leri varsa binary-safe HTTP yolu ile canli fetch denemesi yapar.
- IPv6 init yolu Router Solicitation gonderir ve Router Advertisement geldiginde default route ile komsu cache'yi gunceller; host smoke default router/next-hop secimini mekanik olarak dogrular.
- `net smoke tcp|http|ping` shell yuzeyi gercek socket/client/ICMP yolunu cagirir; basarisizlikta operational smoke siniri saklanmaz.
- `net smoke grpc` shell yuzeyi gercek unary TCP+h2 yolunu cagirir; built-in dispatch ile remote transport birbirine karistirilmaz.
- gRPC remote server-streaming corpus'u host loopback uzerinde birden fazla framed mesaji ve trailer status'unu tek unary yuze ezmeden dogrular.
- HTTP/3 QPACK dynamic indexed header decode'u trailing HEADERS'i statik tabloyla karistirip dusurmez; native registry host corpus'unda established QUIC handle'i gercek istemci baglamina tasir.
- HTTP/3 istemci yuzeyi artik `status + headers + trailers + body` yanitini koruyabilir; native roundtrip corpus'u trailing HEADERS'in istemci baglaminda da kaybolmadigini dogrular.
- CNI bridge plugin `ADD/DEL/CHECK` sirasinda bridge, host-veth, container interface ve namespace uyumunu ayni orchestration kaydi uzerinden dogrular.
- IPv6 next-hop secimi artik multicast MAC map, neighbor cache tercihleri, link-local refusal ve expired router gc davranisini ayni corpus icinde korur.
- CNI komutlari artik process-local manager reset'ine bagli degil; global kernel-owned manager ayni ADD/CHECK/DEL akisini ayri invokelarda surdurur.
- eBPF ELF loader socket-filter section'ini JIT compile edip attach/run verdict zincirine tasiyan corpus ile korunur.

## Phase Boundary

- Faz 1 exact kapandi: transport, core DHCP/DNS/TCP/HTTP/HTTPS bridge, gercek ICMP echo, modern protocol interop, IPv6 control-plane smoke, CNI lifecycle smoke ve interpreter-disi eBPF runtime artik mekanik kanitla destekleniyor.
- Faz 3 exact kapandi: HTTPS trust matrix, native h2 DoH parity, sustained DoT resolver matrix, wider IPv6 route/NDP corpus, kernel-owned CNI netns parity ve eBPF ELF/JIT corpus mekanik kanitla destekleniyor.

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
