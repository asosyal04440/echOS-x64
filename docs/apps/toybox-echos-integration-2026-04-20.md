# Toybox echOS Integration Slice

Date: 2026-04-20

## Source Boundary

- Upstream: `landley/toybox`
- Tag: `0.8.13`
- Commit: `a61f9fe68fafdabf2913b9498ce9ae1a086ed11d`
- License: `0BSD`
- Imported path: `third_party/curated/toybox/`
- Excluded path: upstream `kconfig/`

`kconfig/` is excluded because upstream marks that build configurator as GPLv2 build-only code. echOS imports the 0BSD runtime subset and will generate its own selected-command config/header layer.

## First Command Set

| Command | Upstream source | echOS first contract |
| --- | --- | --- |
| `cat` | `toys/posix/cat.c` | read one or more files and write bytes to stdout |
| `echo` | `toys/posix/echo.c` | print argv with `-n`, `-e`, and `-E` behavior pinned by tests |
| `ls` | `toys/posix/ls.c` | list VFS directories with stable sort and bounded metadata reads |
| `mkdir` | `toys/posix/mkdir.c` | create directories, including `-p` parent creation |
| `rm` | `toys/posix/rm.c` | remove files and directories through explicit recursive flag handling |
| `cp` | `toys/posix/cp.c` | copy files and directory trees through VFS reads/writes |
| `mv` | `toys/posix/cp.c` | rename when same mount supports it, copy/remove fallback only after tests |
| `grep` | `toys/posix/grep.c` | line-oriented search with bounded memory for file and stdin input |
| `wc` | `toys/posix/wc.c` | byte, line, word, and max-line counts |
| `head` | `toys/posix/head.c` | bounded prefix output by lines or bytes |
| `tail` | `toys/posix/tail.c` | bounded suffix output by lines or bytes; follow mode remains out of first contract |

## Adapter Surface

- File IO: `open`, `read`, `write`, `close`, `lseek`, `stat`, `fstat`, `rename`, `unlink`, `mkdir`, `rmdir`
- Directory IO: `opendir`, `readdir`, `closedir`, d_type fallback through `stat`
- Standard streams: fd `0`, `1`, `2` bound to shell stdin/stdout/stderr with deterministic error propagation
- Process shape: `argc`, `argv`, `environ`, current working directory, exit status
- Time and metadata: `mtime`, file type, mode bits, uid/gid fields reported through echOS VFS policy values
- Allocation: all heap use routed through echOS user/process allocator accounting; no kernel hot-path allocation
- Permissions: command-visible errors must preserve VFS denial causes without granting broader shell capability

## Validation Gate

- Host smoke corpus from imported upstream tests: `cat`, `cp`, `echo`, `grep`, `head`, `ls`, `mkdir`, `mv`, `rm`, `tail`, `wc`
- echOS-specific corpus: VFS mount root, nested directory, missing file, permission denial, stdout/stderr redirection, pipe input, large file, zero-length file, non-UTF-8 bytes
- Gate before shell wiring: every selected command returns deterministic stdout, stderr, and exit status for the host corpus and the echOS corpus

## Main Risk

Toybox assumes a Unix-like libc and process environment. The mitigation is an echOS-owned adapter layer and selected-command config instead of importing upstream `kconfig/` or linking the full command tree directly into the kernel.
