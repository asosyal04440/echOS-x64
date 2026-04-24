# ech-tools Composition

Date: 2026-04-23

## Decision

`ech-tools` will use `sbase` plus `ubase` as the primary permissive-license command source pool, with the selected Toybox 0BSD subset retained as a reference/fallback implementation source for overlapping terminal commands.

## Imported Sources

| Source | Commit | License | Imported Path | Upstream Command Count |
| --- | --- | --- | --- | --- |
| `sbase` | `c1341583c96307cb0e6152c963ed23c4d56a4278` | MIT | `third_party/curated/sbase/` | 100 |
| `ubase` | `e8249b49ca3e02032dece5e0cdac3d236667a6d9` | MIT/X Consortium | `third_party/curated/ubase/` | 52 |
| `toybox` subset | `a61f9fe68fafdabf2913b9498ce9ae1a086ed11d` | 0BSD | `third_party/curated/toybox/` | 11 selected command sources |

`dd` and `mknod` are present in both `sbase` and `ubase`, so the combined `sbase` + `ubase` command pool is 150 unique command candidates before echOS adapter filtering.

## Command Pool

`sbase` provides:

`basename`, `bc`, `cal`, `cat`, `chgrp`, `chmod`, `chown`, `chroot`, `cksum`, `cmp`, `cols`, `comm`, `cp`, `cron`, `cut`, `date`, `dc`, `dd`, `dirname`, `du`, `echo`, `ed`, `env`, `expand`, `expr`, `false`, `find`, `flock`, `fold`, `getconf`, `grep`, `head`, `hostname`, `join`, `kill`, `link`, `ln`, `logger`, `logname`, `ls`, `make`, `md5sum`, `mkdir`, `mkfifo`, `mknod`, `mktemp`, `mv`, `nice`, `nl`, `nohup`, `od`, `paste`, `pathchk`, `printenv`, `printf`, `pwd`, `readlink`, `renice`, `rev`, `rm`, `rmdir`, `sed`, `seq`, `setsid`, `sha1sum`, `sha224sum`, `sha256sum`, `sha384sum`, `sha512sum`, `sha512-224sum`, `sha512-256sum`, `sleep`, `sort`, `split`, `sponge`, `strings`, `sync`, `tail`, `tar`, `tee`, `test`, `tftp`, `time`, `touch`, `tr`, `true`, `tsort`, `tty`, `uname`, `unexpand`, `uniq`, `unlink`, `uudecode`, `uuencode`, `wc`, `which`, `whoami`, `xargs`, `xinstall`, `yes`.

`ubase` provides:

`blkdiscard`, `chvt`, `clear`, `ctrlaltdel`, `dd`, `df`, `dmesg`, `eject`, `fallocate`, `free`, `freeramdisk`, `fsfreeze`, `getty`, `halt`, `hwclock`, `id`, `insmod`, `killall5`, `last`, `lastlog`, `login`, `lsmod`, `lsusb`, `mesg`, `mknod`, `mkswap`, `mount`, `mountpoint`, `nologin`, `pagesize`, `passwd`, `pidof`, `pivot_root`, `ps`, `pwdx`, `readahead`, `respawn`, `rmmod`, `stat`, `su`, `swaplabel`, `swapoff`, `swapon`, `switch_root`, `sysctl`, `truncate`, `umount`, `unshare`, `uptime`, `vtallow`, `watch`, `who`.

## Bring-up Tiers

Tier 0, non-privileged shell/file/text commands:

`cat`, `echo`, `ls`, `mkdir`, `rm`, `cp`, `mv`, `grep`, `wc`, `head`, `tail`, `pwd`, `printf`, `touch`, `sort`, `uniq`, `tr`, `tee`, `basename`, `dirname`, `true`, `false`.

Tier 1, archive/checksum/inspection commands:

`tar`, `cksum`, `md5sum`, `sha1sum`, `sha224sum`, `sha256sum`, `sha384sum`, `sha512sum`, `sha512-224sum`, `sha512-256sum`, `cmp`, `comm`, `cut`, `fold`, `join`, `od`, `paste`, `rev`, `sed`, `seq`, `split`, `strings`, `test`, `xargs`, `yes`.

Tier 2, system or privilege-sensitive commands:

`df`, `dmesg`, `free`, `id`, `lsusb`, `mount`, `mountpoint`, `pidof`, `ps`, `stat`, `truncate`, `umount`, `uptime`, `who`, plus module, swap, login, process, and device-control commands from `ubase`.

## Integration Boundary

Imported upstream files stay unmodified. echOS-owned code must provide the adapter and dispatcher layer outside `third_party/curated/`:

- argv/env and exit-status ABI
- file, directory, path, stat, permission, and time adapters
- stdout/stderr/stdin terminal adapters
- process table, uid/gid, mount table, device, and module adapters for `ubase`
- host smoke corpus per command before OS image wiring
- shell registration through `ech-tools <command> ...`

Current status: source/provenance import is complete and the shell bridge now covers all 150 commands. Tier 0 is fully wired, and the final Tier 1/Tier 2 slice adds root, swap, module, device, network, scheduling, login, halt, and namespace coverage with `blkdiscard`, `chroot`, `cron`, `eject`, `freeramdisk`, `fsfreeze`, `getty`, `halt`, `insmod`, `mkswap`, `nice`, `nohup`, `passwd`, `pivot_root`, `renice`, `rmmod`, `setsid`, `swaplabel`, `swapoff`, `swapon`, `switch_root`, `tftp`, and `unshare`. Host smoke coverage exercises the final slice against the in-memory file bridge and shell-owned state records; hardware-only effects remain bounded to existing driver/model APIs or recorded control state until the boot image exposes real devices.

## User-Facing Reference

The command reference for users and contributors lives in `docs/apps/ech-tools-command-reference-2026-04-20.md`. It records `ech-tools` usage, command status terms, the shell-bridge command set, Tier 0 bring-up commands, and the command families in the 150-command source pool.
