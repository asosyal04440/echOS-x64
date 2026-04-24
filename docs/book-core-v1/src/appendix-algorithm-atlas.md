# Ek A - echOS Algoritma Atlasi

Bu ek, Cilt 1 boyunca gecen algoritmalarin hizli referans ozetidir.

## Scheduler cekirdegi

| Algoritma | echOS dosyasi | Secim nedeni | Ana dezavantaj | Mitigasyon |
|---|---|---|---|---|
| RT FIFO/RR | `src/task/rt_scheduler.rs` | Gercek zamanli oncelik | starvation riski | RR dilimi ve bandwidth limiti |
| CFS | `src/task/cfs.rs` | adil CPU paylasimi | wakeup latency gerilimi | wakeup granularity |
| EEVDF | `src/task/eevdf.rs` | eligibility + deadline secimi | durum izleme karmasikligi | acik `lag/eligible/vd` alanlari |
| EDF/CBS | `src/task/deadline.rs` | deadline tabanli planlama | admission yanlis ayarlanabilir | `U=C/T` tabanli kontrol |
| Work stealing | `src/task/deque.rs` | SMP dengeleme | atomik ordering hatasi riski | CAS + fence sinirlari |
| Timing wheel | `src/task/timer.rs` | O(1) amortized uyandir | cascade mantigi karma olabilir | 4 level sabit tasarim |

## Bellek cekirdegi

| Algoritma | echOS dosyasi | Secim nedeni | Ana dezavantaj | Mitigasyon |
|---|---|---|---|---|
| Zone PMM fallback | `src/memory/fibonacci_pmm.rs` | DMA uyumlulugu | fallback baskiyi gizleyebilir | zone stats takibi |
| Fibonacci buddy | `src/memory/fibonacci_buddy.rs` | dusuk ic parcalanma | split/coalesce maliyeti | recursive coalesce kontrolu |
| TLSF | `src/allocator/tlsf.rs` | O(1) heap sinif secimi | metadata butunluk riski | canary + tracker |
| COW fault | `src/memory/mod.rs` | fork sonrasi tasarruf | write fault latency | refcount ve selective copy |
| THP | `src/memory/mod.rs` | TLB hit artisi | internal waste riski | eligibility ve rollback |
| MGLRU | `src/memory/mglru.rs` | hot/cold daha iyi ayrim | policy tuning zor | generation + refault mekanizmasi |
| ZSwap | `src/memory/zswap.rs` | diskten once RAM compression | CPU compression maliyeti | budget ve fallback |

## I/O ve ag cekirdegi

| Algoritma | echOS dosyasi | Secim nedeni | Ana dezavantaj | Mitigasyon |
|---|---|---|---|---|
| Lock-free io_uring ring | `src/posix/io_uring_ring.rs` | syscall ve lock maliyeti azaltma | ordering hatasi riski | `smp_wmb/rmb` disiplin |
| TLS 1.3 key schedule | `src/net/tls.rs` | modern guvenlik model | state machine karma | enum tabanli mesaj tipleri |
| QUIC frame/varint | `src/net/quic.rs` | dusuk RTT, cok akis | parser karmasikligi | ACK range limiti |
| WireGuard nonce replay guard | `src/net/wireguard.rs` | sade ve guvenli tunnel | nonce state hatasi riski | monotonic nonce kontrolu |
| HPACK Huffman decode | `src/net/http2_huffman.rs` | header sikistirma | padding/EOS corner case | fail-closed hata donusleri |
