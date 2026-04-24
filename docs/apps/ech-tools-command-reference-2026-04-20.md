# ech-tools Command Reference

Date: 2026-04-23

`ech-tools` is the echOS umbrella command for the permissive `sbase` and `ubase` command pool. The source pool contains 150 unique command candidates. The current shell bridge routes all 150 commands through echOS-owned shell behavior, driver/model calls, or explicit shell state records.

## Usage

```text
ech-tools
ech-tools help <command>
ech-tools <command> [arguments]
```

## Status Terms

| Status | Meaning |
| --- | --- |
| `shell-bridge` | The command is routed through an existing echOS shell implementation. |
| `adapter-pending` | Historical catalog state for commands whose echOS runtime adapter was not connected. Current catalog count: 0. |

## Tier 0 Shell-Bridge Commands

| Command | What it does |
| --- | --- |
| `basename` | Prints the last non-empty path component. |
| `cat` | Prints text file contents. |
| `cp` | Copies a file to a target path. |
| `dirname` | Prints the directory portion of a path. |
| `echo` | Prints arguments. |
| `false` | Returns an unsuccessful exit status. |
| `grep` | Selects lines containing a pattern. |
| `head` | Prints the first lines from input. |
| `ls` | Lists directory entries. |
| `mkdir` | Creates a directory. |
| `mv` | Moves or renames a path. |
| `printf` | Formats and prints arguments. |
| `pwd` | Prints the current directory. |
| `rm` | Removes a file path. |
| `sort` | Sorts input lines. |
| `tail` | Prints the last lines from input. |
| `tee` | Copies stdin to stdout and files. |
| `touch` | Creates a file or updates its timestamp path. |
| `tr` | Translates or deletes characters from stdin. |
| `true` | Returns a successful exit status. |
| `uniq` | Removes adjacent duplicate lines. |
| `wc` | Counts lines, words, and characters. |

## Tier 0 Bring-up Commands

| Command | What it does | Status |
| --- | --- | --- |
| `basename` | Prints the last non-empty path component. | `shell-bridge` |
| `cat` | Prints text file contents. | `shell-bridge` |
| `cp` | Copies a file to a target path. | `shell-bridge` |
| `dirname` | Prints the directory portion of a path. | `shell-bridge` |
| `echo` | Prints arguments. | `shell-bridge` |
| `false` | Returns an unsuccessful exit status. | `shell-bridge` |
| `grep` | Selects lines containing a pattern. | `shell-bridge` |
| `head` | Prints the first lines from input. | `shell-bridge` |
| `ls` | Lists directory entries. | `shell-bridge` |
| `mkdir` | Creates a directory. | `shell-bridge` |
| `mv` | Moves or renames a path. | `shell-bridge` |
| `printf` | Formats and prints arguments. | `shell-bridge` |
| `pwd` | Prints the current directory. | `shell-bridge` |
| `rm` | Removes a file path. | `shell-bridge` |
| `sort` | Sorts input lines. | `shell-bridge` |
| `tail` | Prints the last lines from input. | `shell-bridge` |
| `tee` | Copies stdin to stdout and files. | `shell-bridge` |
| `touch` | Creates a file or updates its timestamp path. | `shell-bridge` |
| `tr` | Translates or deletes characters from stdin. | `shell-bridge` |
| `true` | Returns a successful exit status. | `shell-bridge` |
| `uniq` | Removes adjacent duplicate lines. | `shell-bridge` |
| `wc` | Counts lines, words, and characters. | `shell-bridge` |

## Tier 1 and Tier 2 Bridge Coverage

The active bridge extends beyond Tier 0 into Tier 1 and Tier 2 commands with echOS-owned behavior:

- Tier 1 shell/text/filesystem commands: `clear`, `cmp`, `cut`, `date`, `env`, `find`, `hostname`, `ln`, `paste`, `readlink`, `rev`, `rmdir`, `seq`, `strings`, `test`, `uname`, `which`, `whoami`
- Tier 1 formatting/environment commands: `cal`, `comm`, `expand`, `expr`, `fold`, `getconf`, `join`, `link`, `logname`, `nl`, `printenv`, `sleep`, `time`, `tty`, `unexpand`, `unlink`
- Tier 2 system/state commands: `chmod`, `chown`, `df`, `free`, `id`, `kill`, `lsmod`, `lsusb`, `mount`, `mountpoint`, `pidof`, `ps`, `stat`, `truncate`, `umount`, `uptime`, `who`

## Final Bridge Additions

The final slice added root, swap, module, device, network, scheduling, login, halt, and namespace commands:

| Command | What it does |
| --- | --- |
| `blkdiscard` | Zeroes a byte range in a file-backed block surface. |
| `chroot` | Runs a command with `ECHOS_ROOT` and `PWD` rebound for the shell scope. |
| `cron` | Reads crontab lines and runs bounded scheduled command entries once. |
| `eject` | Detaches loopback media when present or records removable media as offline. |
| `freeramdisk` | Detaches a loopback/ramdisk registration. |
| `fsfreeze` | Records frozen/thawed mount state for the shell control plane. |
| `getty` | Selects a tty and starts a login session. |
| `halt` | Arms the init shutdown path, with host/test execution kept non-destructive. |
| `insmod` | Loads a module image into the shell module registry. |
| `mkswap` | Writes an `ECHOSSWAP1` header and records swap metadata. |
| `nice` | Runs a nested command with `ECHOS_NICE` set. |
| `nohup` | Runs a nested command and writes output to `nohup.out`. |
| `passwd` | Updates the local user password hash. |
| `pivot_root` | Rebinds the shell root and records `put_old`. |
| `renice` | Updates recorded process nice values. |
| `rmmod` | Removes a shell module registry entry. |
| `setsid` | Runs a nested command with a new shell session id. |
| `swaplabel` | Reads or writes recorded swap labels. |
| `swapoff` | Marks a recorded swap area disabled. |
| `swapon` | Validates and enables an `ECHOSSWAP1` swap area. |
| `switch_root` | Rebinds the shell root and runs an init command. |
| `tftp` | Performs local file transfer or emits a TFTP RRQ/WRQ UDP datagram. |
| `unshare` | Runs a nested command with recorded namespace flags. |

## Previous Bridge Additions

The previous slice added archive, editor, build, terminal-session, login-state, and control commands:

| Command | What it does |
| --- | --- |
| `chvt` | Selects the active virtual terminal tracked by the shell bridge. |
| `ctrlaltdel` | Reads or sets the reboot-mode policy bit. |
| `dmesg` | Reads kernel log lines from the kmsg lane. |
| `ed` | Applies batch-oriented line editor commands to text files. |
| `flock` | Acquires or releases advisory file locks through the file-lock lane. |
| `killall5` | Signals process-table entries except the active shell task. |
| `last` | Prints recorded login/session history. |
| `lastlog` | Prints last-login records. |
| `login` | Updates the shell login identity and home directory. |
| `make` | Runs Makefile target recipes through the shell executor. |
| `mesg` | Reads or changes terminal write permission state. |
| `mknod` | Creates FIFO nodes and reports unsupported device-node requests explicitly. |
| `nologin` | Reports the configured no-login denial message. |
| `pwdx` | Prints process working-directory ownership. |
| `respawn` | Re-runs a command for a bounded repeat count. |
| `su` | Switches shell login identity through the same local identity lane as `login`. |
| `sysctl` | Reads and writes selected kernel and terminal policy keys. |
| `tar` | Creates, lists, and extracts uncompressed ustar-style archives. |
| `vtallow` | Reads or changes virtual-terminal switching permission. |
| `watch` | Re-runs a command on a bounded watch interval. |

The previous slice added calculator, byte-copy, stream-edit, uuencode, FIFO, RTC, readahead, and install-style file commands:

| Command | What it does |
| --- | --- |
| `bc` | Evaluates arithmetic expressions through the shell arithmetic evaluator. |
| `chgrp` | Updates the group owner recorded for a path. |
| `cols` | Prints the active terminal column count. |
| `dc` | Evaluates reverse-polish arithmetic expressions. |
| `dd` | Copies bytes between files with block-size and count controls. |
| `fallocate` | Ensures a file has the requested byte length. |
| `hwclock` | Prints the current RTC-backed wall-clock value. |
| `mkfifo` | Creates a FIFO through the POSIX FIFO manager. |
| `readahead` | Warms file contents through the file read path. |
| `sed` | Applies `s/old/new/[g]` substitutions to text input. |
| `uudecode` | Decodes uuencoded transfer data. |
| `uuencode` | Encodes files or stdin into uuencode transfer data. |
| `xinstall` | Copies a file into place with install-style path handling. |
| `yes` | Emits a bounded repeat stream of text. |

The earlier checksum slice added checksum, temp-path, inspection, buffering, and topo/argv commands:

| Command | What it does |
| --- | --- |
| `cksum` | Computes POSIX CRC checksums and byte counts. |
| `du` | Reports total file or directory byte usage. |
| `logger` | Writes a message into the kernel log lane. |
| `md5sum` | Computes an MD5 digest. |
| `mktemp` | Creates a unique temporary path and file. |
| `od` | Dumps bytes in octal rows. |
| `pagesize` | Prints the active page size. |
| `pathchk` | Validates literal path length and component bounds. |
| `sha1sum` | Computes a SHA-1 digest. |
| `sha224sum` | Computes a SHA-224 digest. |
| `sha256sum` | Computes a SHA-256 digest. |
| `sha384sum` | Computes a SHA-384 digest. |
| `sha512sum` | Computes a SHA-512 digest. |
| `sha512-224sum` | Computes a SHA-512/224 digest. |
| `sha512-256sum` | Computes a SHA-512/256 digest. |
| `split` | Splits line-oriented input into chunk files. |
| `sponge` | Buffers stdin and writes only after reads finish. |
| `sync` | Flushes filesystem state in kernel mode or host smoke. |
| `tsort` | Performs a topological sort over dependency pairs. |
| `xargs` | Appends stdin tokens to a command line and executes it. |

The earlier slice added the formatting/environment bridge:

| Command | What it does |
| --- | --- |
| `cal` | Prints a monthly calendar. |
| `comm` | Compares sorted line streams and groups left/right/common rows. |
| `expand` | Converts tabs to spaces. |
| `expr` | Evaluates shell expressions. |
| `fold` | Wraps long lines to a fixed width. |
| `getconf` | Prints selected system configuration values. |
| `join` | Joins matching first-column rows from two sorted files. |
| `link` | Creates a hard link through the shell bridge or host smoke adapter. |
| `logname` | Prints the current login name. |
| `nl` | Numbers input lines. |
| `printenv` | Prints all environment variables or selected names. |
| `sleep` | Waits for the requested number of seconds. |
| `time` | Runs a command and prints elapsed wall time. |
| `tty` | Prints the current terminal path. |
| `unexpand` | Converts spaces back into tabs at tab stops. |
| `unlink` | Removes a single path through the shell bridge or host smoke adapter. |

## Command Families

| Family | Commands | Purpose |
| --- | --- | --- |
| Files and directories | `basename`, `cat`, `chgrp`, `chmod`, `chown`, `chroot`, `cmp`, `cp`, `dirname`, `du`, `find`, `ln`, `ls`, `mkdir`, `mkfifo`, `mknod`, `mktemp`, `mv`, `pathchk`, `pwd`, `readlink`, `rm`, `rmdir`, `stat`, `touch`, `truncate`, `unlink`, `xinstall` | Inspect, create, move, remove, and describe filesystem entries. |
| Text processing | `cols`, `comm`, `cut`, `ed`, `expand`, `fold`, `grep`, `head`, `join`, `nl`, `paste`, `rev`, `sed`, `sort`, `split`, `sponge`, `strings`, `tail`, `tee`, `tr`, `tsort`, `unexpand`, `uniq`, `wc` | Transform streams and line-oriented text. |
| Shell and environment | `echo`, `env`, `expr`, `false`, `getconf`, `link`, `logname`, `nice`, `nohup`, `printenv`, `printf`, `seq`, `setsid`, `sleep`, `test`, `time`, `true`, `tty`, `uname`, `which`, `whoami`, `xargs`, `yes` | Provide shell glue, process launch helpers, and script-friendly primitives. |
| Checksums and archives | `cksum`, `md5sum`, `sha1sum`, `sha224sum`, `sha256sum`, `sha384sum`, `sha512-224sum`, `sha512-256sum`, `sha512sum`, `tar`, `uudecode`, `uuencode` | Verify data, package files, and encode/decode transfer formats. |
| Calculators and build tools | `bc`, `cal`, `date`, `dc`, `make` | Compute, inspect calendars/time, and run build rules. |
| System and device control | `blkdiscard`, `chvt`, `clear`, `ctrlaltdel`, `dd`, `df`, `dmesg`, `eject`, `fallocate`, `free`, `freeramdisk`, `fsfreeze`, `getty`, `halt`, `hwclock`, `id`, `insmod`, `kill`, `killall5`, `last`, `lastlog`, `login`, `lsmod`, `lsusb`, `mesg`, `mkswap`, `mount`, `mountpoint`, `nologin`, `pagesize`, `passwd`, `pidof`, `pivot_root`, `ps`, `pwdx`, `readahead`, `renice`, `respawn`, `rmmod`, `su`, `swaplabel`, `swapoff`, `swapon`, `switch_root`, `sync`, `sysctl`, `tftp`, `umount`, `unshare`, `uptime`, `vtallow`, `watch`, `who` | Query or change kernel, process, device, terminal, swap, mount, and login state. |

## Full Per-Command Help

The in-kernel catalog has one-line help and usage text for all 150 commands. Use:

```text
ech-tools help ls
ech-tools help mount
ech-tools help sha256sum
```

## Current State

Tier 0 is fully routed, and the shell bridge now covers 150 commands across the full catalog. `true`/`false`, `test`, `chroot`, `cron`, `flock`, `halt`, `make`, `mountpoint`, `nice`, `nohup`, `nologin`, `pidof`, `printenv`, `respawn`, `setsid`, `switch_root`, `time`, `unshare`, `watch`, and `xargs` preserve explicit shell exit ownership so `&&` and `||` observe command status directly instead of guessing from stdout. Hardware-only side effects remain bounded to existing driver/model calls or shell-owned state records until a boot image exposes real devices.
