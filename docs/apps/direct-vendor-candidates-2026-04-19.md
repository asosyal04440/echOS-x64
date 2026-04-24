# Direct Vendor Candidate Matrix

Date: 2026-04-19

Goal: choose ready-made permissive-license components that can be embedded into echOS without creating a large new application-development front.

## Selection Rule

Accept only components that satisfy all of these:

- Permissive license: BSD, MIT, ISC, 0BSD, zlib, public domain, or Apache-2.0 with notice discipline.
- Source available from official upstream repository or release.
- Low dependency count.
- Small enough to audit or isolate.
- Useful as a real tool seed, not just a demo.

Reject or defer:

- GPL/LGPL/AGPL for base image embedding.
- Unknown or missing license.
- Large GUI applications tied to X11/Wayland/GTK/Qt.
- Components that force POSIX breadth before echOS has the needed runtime surface.

## Imported Now

| Need | Component | License | Why It Fits | echOS Use |
| --- | --- | --- | --- | --- |
| Text editor | `antirez/kilo` | BSD-2-Clause | Single C file, no curses, VT100 escape based | Seed `ech-edit` |
| Image decode/viewer core | `nothings/stb` | Public domain or MIT | Single-header image decode/write/resize | Seed `ech-view` decode path |
| Image format | `phoboslab/qoi` | MIT | Single-file C/C++ library, simple format | Native fast image asset format |
| Compression | `richgel999/miniz` | MIT | Source/header pair, zlib/deflate/ZIP scope | Seed `ech-compress` |
| JSON tooling | `zserge/jsmn` | MIT | Header-only, C89, no libc dependency, no dynamic allocation | Manifest/telemetry parser seed |
| TOML tooling | `cktan/tomlc17` | MIT | `tomlc17.c/h` direct include, C99-compatible claim | Host/user config parser seed |
| Font/format utility | `nothings/stb` expanded set | Public domain or MIT | Added TrueType rasterizer, fast sprintf, textedit core, and Vorbis source under existing pin | GUI font fallback, C utility formatting, editor widget core, Ogg Vorbis decode source |
| Audio codecs | `mackron/dr_libs` | MIT-0 or Unlicense | Single-header WAV/FLAC/MP3 codec sources | Future audio decode tooling after corpus tests |
| Package archives | `rxi/microtar` | MIT | Small ANSI C tar reader/writer | Package archive tooling seed |
| INI config | `rxi/ini` | MIT | Tiny ANSI C parser | Host/user config parser seed |
| Shell line editing | `antirez/linenoise` | BSD-2-Clause | Small readline alternative with history/completion/hints | Shell/REPL input editing after tty/fd adapter |
| Embedded database | `sqlite.org` amalgamation | Public domain | Official `sqlite3.c`/`sqlite3.h` release with tiny embed boundary and no copyleft obligations | Seed package metadata, settings, and structured app state |
| Scripting runtime | `lua.org` Lua 5.4.8 | MIT | Official ANSI C source tree with allocator hook boundary and optional stdlib pruning | Seed shell scripting, config evaluation, and plugin/runtime work |

Imported location: `third_party/curated/`

## Terminal Tools Decision

Preferred: `sbase` + `ubase`

- Licenses: `sbase` is MIT; `ubase` is MIT/X Consortium.
- Fit: broad, small Unix-style command source sets with simple upstream structure and permissive licenses.
- Import status: full upstream repository contents, excluding `.git`, imported under `third_party/curated/sbase/` and `third_party/curated/ubase/`.
- Pins: `sbase` commit `c1341583c96307cb0e6152c963ed23c4d56a4278`; `ubase` commit `e8249b49ca3e02032dece5e0cdac3d236667a6d9`.
- Command count: `sbase` exposes 100 commands, `ubase` exposes 52 commands, and `dd` plus `mknod` overlap, giving 150 unique command candidates for `ech-tools`.
- Runtime status: source/provenance is imported; echOS libc/POSIX adapter, command dispatch policy, and per-command smoke wiring remain the next integration slice.

Reference/fallback: `toybox`

- License: 0BSD.
- Upstream states Toybox is under Zero-Clause BSD.
- Fit: terminal tools as a single multi-call utility family and implementation reference for selected commands.
- Import status: selected 0BSD runtime subset imported under `third_party/curated/toybox/` at tag `0.8.13`, commit `a61f9fe68fafdabf2913b9498ce9ae1a086ed11d`.
- Import boundary: upstream `kconfig/` is GPLv2 build-only code and is excluded from the curated tree. The selected import covers shared `lib/`, `main.c`, `toys.h`, first command sources, and matching upstream tests.
- First command scope: `cat`, `echo`, `ls`, `mkdir`, `rm`, `cp`, `mv`, `grep`, `wc`, `head`, and `tail`.
- Runtime status: source/provenance is imported; echOS libc/POSIX adapter and command smoke wiring remain the next integration slice.

Rejected for base embedding:

- BusyBox: GPL family, wrong default for echOS base image embedding.
- Full desktop image viewers: most pull X11/Wayland/toolkit dependencies or GPL-heavy trees.
- libmagic/file as first file-type tool: license is permissive enough in common packaging, but the magic database and build/runtime shape are too large for the first embed pass.

## First App Mapping

| echOS Program | Vendored Seed | First Useful Scope |
| --- | --- | --- |
| `ech-edit` | `kilo` | open/save/search text file over terminal |
| `ech-view` | `stb` + `qoi` | decode PNG/JPEG/BMP/QOI into framebuffer buffer |
| `ech-compress` | `miniz` | list/extract/create ZIP and deflate streams |
| `ech-json` | `jsmn` | validate JSON and print token tree |
| `ech-config` | `tomlc17` | parse TOML config and report keys |
| `ech-db` | `sqlite` | embed SQLite with an echOS VFS/allocator boundary |
| `ech-lua` | `lua` | run Lua scripts with an echOS-owned host module surface |
| `ech-tools` | `sbase` + `ubase`, with Toybox reference subset | 150 unique command candidates before adapter filtering |

## Next Integration Slice

1. Create an echOS-owned `ech-tools` dispatcher that selects `sbase` implementations for core file/text/shell commands and `ubase` implementations for privileged/system commands.
2. Add an echOS adapter boundary for file IO, argv/env, stdout/stderr, time, directory iteration, permissions, process state, mount state, devices, and exit status.
3. Start with the non-privileged bring-up tier: `cat`, `echo`, `ls`, `mkdir`, `rm`, `cp`, `mv`, `grep`, `wc`, `head`, `tail`, `pwd`, `printf`, `touch`, `sort`, `uniq`, `tr`, `tee`, `basename`, `dirname`, `true`, and `false`.
4. Add host smoke tests for each selected command before wiring into the OS image.
5. Wire `ech-tools <cmd> ...` into the shell only after adapter tests pass.
