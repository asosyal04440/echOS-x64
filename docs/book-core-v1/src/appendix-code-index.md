# Ek B - Kod Referans Indeksi

Bu indeks, kitapta anlatilan konularin echOS dosya referanslarini tek yerde toplar.

## 1) Boot ve platform init

- `src/main.rs`
- `src/memory/frame_allocator.rs`

## 2) Scheduler cekirdegi

- `src/task/mod.rs`
- `src/task/scheduler.rs`
- `src/task/rt_scheduler.rs`
- `src/task/cfs.rs`
- `src/task/eevdf.rs`
- `src/task/deadline.rs`
- `src/task/deque.rs`
- `src/task/timer.rs`

## 3) Bellek cekirdegi

- `src/memory/mod.rs`
- `src/memory/fibonacci_pmm.rs`
- `src/memory/fibonacci_buddy.rs`
- `src/allocator/tlsf.rs`
- `src/memory/mglru.rs`
- `src/memory/zswap.rs`

## 4) I/O ve lock-free ring

- `src/posix/io_uring_ring.rs`
- `src/net/io_uring.rs`

## 5) Ag core guvenlik protokolleri

- `src/net/tls.rs`
- `src/net/quic.rs`
- `src/net/wireguard.rs`
- `src/net/http2_huffman.rs`

## 6) Yardimci mimari baglam dosyalari

- `README.tr.md`
- `README.md`
- `src/lib.rs`

---

## Konu -> Dosya hizli esleme tablosu

| Konu | Dosya |
|---|---|
| CPU secim ve spawn | `src/task/scheduler.rs` |
| RT oncelik ve RR dilimi | `src/task/rt_scheduler.rs` |
| Vruntime formulu | `src/task/cfs.rs` |
| EEVDF `lag/eligible/vd` | `src/task/eevdf.rs` |
| EDF `U=C/T` admission | `src/task/deadline.rs` |
| Chase-Lev CAS yarisi | `src/task/deque.rs` |
| Timing wheel cascade | `src/task/timer.rs` |
| Zone fallback | `src/memory/fibonacci_pmm.rs` |
| Buddy split/coalesce | `src/memory/fibonacci_buddy.rs` |
| TLSF wrapper guvenligi | `src/allocator/tlsf.rs` |
| COW fault yolu | `src/memory/mod.rs` |
| THP map denemesi | `src/memory/mod.rs` |
| MGLRU victim secimi | `src/memory/mglru.rs` |
| Writeback budget | `src/memory/mod.rs` |
| ZSwap compressor yolu | `src/memory/zswap.rs` |
| SQ/CQ lock-free ordering | `src/posix/io_uring_ring.rs` |
| TLS key schedule | `src/net/tls.rs` |
| QUIC ACK guard | `src/net/quic.rs` |
| WireGuard replay guard | `src/net/wireguard.rs` |
| HPACK decode hatalari | `src/net/http2_huffman.rs` |
