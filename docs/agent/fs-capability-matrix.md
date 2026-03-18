# echOS Filesystem Capability Matrix

Tarih: 2026-03-12

| FS Area | Capability | Status | Code Path | Mechanical Evidence | Boundary | Next Gate |
|---|---|---|---|---|---|---|
| F2FS | primary writable store path | Partial | `src/fs/f2fs.rs` | build + desktop usage | compression/format coverage eksik | fs regression suite |
| Btrfs | mount/read/write semantics | Stubbed | `src/btrfs.rs` | source audit | placeholder superblock/root/inode paths | real btrfs smoke |
| Unified VFS | cross-fs abstraction | Partial | `src/fs/vfs_unified.rs` | `cargo check --target x86_64-pc-windows-msvc --lib` | unsupported paths explicit error; F2FS'ye ek olarak mounted ext4/fat32/ntfs image backend'lerinde gercek open/read/list var; XFS/Btrfs henuz unwired | backend-by-backend read/open wiring |
| inotify | file watch semantics | Partial | `src/fs/inotify.rs` | `cargo check --target x86_64-pc-windows-msvc --lib` | watch identity artik VFS-backed inode resolution kullaniyor; cross-fs inode namespace ve event ordering exactligi henuz acik | watch correctness tests |
| Package/install storage path | payload extraction/install | Partial | `src/security/package.rs` | `cargo check --target x86_64-pc-windows-msvc --lib` | install path artik framed payload + path validation kullaniyor; store `read/list/stat` mounted backend'leri unified VFS uzerinden goruyor; `update` remote repo olmadan explicit error donuyor | package install smoke |

## Truthfulness Rules

- Empty or heuristic result, basarili operation gibi sunulmayacak.
- Her filesystem command veya API unsupported capability'yi acikca belirtecek.
- `df` benzeri kapasite raporlari, olculemeyen backend icin tahmini kapasite uretmeyecek.

## Exactness Exit Criteria

Faz 6'nin `tam uyumlu/exact` sayilmasi icin su kapilarin kapanmasi gerekir:

1. Unified VFS contract
   - open/read/stat/df yuzeylerinde unsupported capability tek hata contract'i ile raporlanir
   - shell/store/gui istemcileri ayni capability/error sinirini gorur
2. Backend semantics
   - Btrfs, F2FS ve diger wired backend'lerde silent fallback, placeholder metadata veya bos-success yolu kalmaz
   - capacity, mount ve error semantics'i mekanik regression suite ile sinanir
3. Event and package fidelity
   - inotify watch identity/event ordering placeholder hash modelinden cikar
   - package/install storage path'i extraction/install/update yolunda gercek backend davranisina baglanir

Bu kosullar kapanmadan Faz 6 yalnizca `truthful/partial`, exact degil.
