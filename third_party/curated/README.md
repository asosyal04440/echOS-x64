# echOS Curated Third-Party Sources

This directory contains small permissive-license sources selected for direct echOS adaptation.

## Rules

- Keep each upstream source in its own subdirectory with the upstream `LICENSE`.
- Record the exact upstream commit in `VENDOR_MANIFEST.toml`.
- Do not edit upstream files in place unless the manifest records the mutation.
- Put echOS adapters, shims, and build glue outside the imported upstream files.
- Do not add GPL, LGPL, AGPL, unknown-license, or source-unavailable components here.

## Current Imports

| Directory | Upstream | License | Intended echOS Role |
| --- | --- | --- | --- |
| `kilo/` | `antirez/kilo` | BSD-2-Clause | `ech-edit` terminal text editor seed |
| `jsmn/` | `zserge/jsmn` | MIT | JSON manifest/token parser |
| `miniz/` | `richgel999/miniz` | MIT | zlib/deflate/ZIP compression core |
| `stb/` | `nothings/stb` | Public domain or MIT | Image decode/write/resize source for viewer tooling |
| `qoi/` | `phoboslab/qoi` | MIT | QOI image format decode/encode and conversion tooling |
| `tomlc17/` | `cktan/tomlc17` | MIT | TOML config parser for host/user tooling |
| `dr_libs/` | `mackron/dr_libs` | MIT-0 or Unlicense | WAV, FLAC, and MP3 codec source for audio tooling |
| `microtar/` | `rxi/microtar` | MIT | Minimal tar archive read/write source for package tooling |
| `ini/` | `rxi/ini` | MIT | Small INI parser source for configuration tooling |
| `linenoise/` | `antirez/linenoise` | BSD-2-Clause | Shell/REPL line editing, history, completion, and hints |
| `sqlite/` | `sqlite.org` | Public domain | Embedded SQL database core and CLI seed for package/state tooling |
| `lua/` | `lua.org` | MIT | Embeddable scripting language source tree for shell and extension runtime work |
| `toybox/` | `landley/toybox` | 0BSD | Selected terminal utility source subset for `ech-tools` |
| `sbase/` | `git.suckless.org/sbase` | MIT | Primary portable core command source set for `ech-tools` |
| `ubase/` | `git.suckless.org/ubase` | MIT/X Consortium | Primary system utility source set for `ech-tools` |

`ech-tools` is now sourced primarily from `sbase/` and `ubase/`: 100 core commands plus 52 system commands, with `dd` and `mknod` duplicated between both trees, for 150 unique command candidates before echOS adapter filtering. Toybox remains imported as a selected 0BSD reference/fallback subset, not as the full upstream repository. The upstream Toybox `kconfig/` build configurator is GPLv2 build-only code and is intentionally excluded from `third_party/curated/`.

`sqlite/` carries the official SQLite amalgamation release (`sqlite3.c`, `sqlite3.h`, `sqlite3ext.h`, `shell.c`) plus the official copyright page as the local license record. `lua/` carries the official Lua 5.4.8 source tree (`src/` and `doc/`), where `doc/readme.html` includes the upstream MIT license text. In both cases, echOS-owned allocator, VFS, stdlib-pruning, and runtime glue must stay outside the imported upstream files unless `VENDOR_MANIFEST.toml` records a deliberate mutation.
