# echOS Win32 Parity Matrix

Tarih: 2026-03-17

Status anlami:

- `Declared`: API katalogda var.
- `Implemented`: `GetProcAddress` gercek fonksiyon cozluyor.
- `Verified`: mekanik dogrulama var.
- `Partial`: davranis veya edge-case kapsami eksik.

| Area | Capability | Status | Code Path | Mechanical Evidence | Boundary | Next Gate |
|---|---|---|---|---|---|---|
| Loader | Import/export/unwind visibility core | Verified | `src/win32.rs`, `src/win32_abi.rs`, `src/pe_loader.rs` | `cargo check --target x86_64-pc-windows-msvc --lib`; `cargo test --no-run --target x86_64-pc-windows-msvc --lib`; forwarded export + delay import wiring, bound-import timestamp match/mismatch IAT policy corpus'u, handler-data bridge'i, `UWOP_SAVE_XMM*` / `UWOP_PUSH_MACHFRAME` + XMM restore coverage'i, widened vector/context publication ve unwind transition telemetry (`last_branch*` / `last_exception*`) mevcut | deep x64 unwind full-context long-tail Faz 4'te | formalized loader matrix |
| Loader | native PE process lifecycle | Verified | `src/win32.rs`, `src\win32_abi.rs`, `src/pe_loader.rs` | `cargo check --target x86_64-unknown-uefi` + source audit | CreateProcess/OpenProcess/TerminateProcess/GetExitCodeProcess gercek handle/task yasam dongusune bagli; edge-case cleanup Faz 4'te | PE runtime suite |
| Kernel32 | basic file/process/thread | Verified | `src/win32.rs` | `cargo check --target x86_64-unknown-uefi` | wide API surface var; exact parity Faz 4'te | example PE runtime suite |
| TLS slots | `TlsAlloc/Set/Get/Free` | Verified | `src/win32.rs` | `cargo check --target x86_64-unknown-uefi` + unit assertions | APC/fiber ve deep runtime corners Faz 4'te | thread runtime tests |
| SEH visible contract | `RaiseException`, filters, exception directory visibility, `RtlLookupFunctionEntry` / `RtlVirtualUnwind` core | Verified | `src/win32.rs`, `src/win32_abi.rs`, `src/pe_loader.rs` | `cargo check --target x86_64-pc-windows-msvc --lib`; `cargo test --no-run --target x86_64-pc-windows-msvc --lib`; leaf-unwind, handler-data/exception-handler, runtime-function lookup, `UWOP_SAVE_XMM128(_FAR)` and machine-frame corpus'u, `vector_registers` mirror contract'i, nonvolatile context-pointer publication'i ve unwind transition telemetry (`last_branch*` / `last_exception*`) mevcut | deep x64 unwind full floating/context parity Faz 4'te | unwind/exception tests |
| WinHTTP / WinINet | request/session/query core | Verified | `src/win32.rs`, `src/net/http.rs`, `src/net/x509.rs` | `cargo check --target x86_64-pc-windows-msvc --lib`; `cargo test --no-run --target x86_64-pc-windows-msvc --lib`; session-cookie-cache-query corpus'una ek olarak protocol-aware proxy selection/bypass, loopback/local bypass, RFC1918 wildcard bypass, embedded proxy-credential parsing'i, HTTPS `CONNECT` tunnel path'i, `407` -> proxy-auth failure contract'i, multiline PAC block evaluation, `dnsResolve(host)` + boolean PAC predicates, IPv4 SAN hostname parity, RTC-backed cert time, pathLen chain gate'i ve CRL-signer hard-fail corpus'u mevcut | HTTPS bridge var; TLS cert date/CN/CA/revoked/decode failures artik ayri Win32 internet errors'ina mapleniyor; session timeout/proxy retention, header query, cookie jar ve GET cache stateful, HTTP+HTTPS proxy route behavioral oldu, `Proxy-Authorization` tunnel'e tasiniyor, `407` artik login-failure sinifina dusuyor, ama Schannel/browser-grade trust, PAC/WPAD ve full proxy policy parity Faz 4'te | HTTPS trust suite |
| COM registry | class factory register/get/create + apartment init core | Verified | `src/win32.rs` | `cargo check --target x86_64-pc-windows-msvc --lib`; apartment model/refcount + `CoGetApartmentType` corpus eklendi ama `cargo test --lib` repo-geneli eski kiriklarda bloklu | full COM/OLE/automation ecosystem Faz 4'te | COM sample object suite |
| OLE automation | `BSTR` / `VARIANT` / `IDispatch` dispatch helpers | Implemented + Partial | `src/win32.rs` | `cargo check --target x86_64-pc-windows-msvc --lib`; `cargo test --no-run --target x86_64-pc-windows-msvc --lib`; `DispGetIDsOfNames` / `DispInvoke` corpus'una ek olarak inter-thread COM marshal stream, `ITypeComp::Bind/BindType`, impl-type/DLL-entry query ve coclass `ITypeInfo::CreateInstance` wiring corpus'u var; `ECHOS-TLB1` text metadata'ya ek olarak binary payload'tan UTF-16/ASCII isim, GUID, version, LCID, help-context ve help-file cikarimi ile multi-type typelib yayinlanabiliyor; `ITypeInfo::GetNames`, `GetIDsOfNames`, `GetDocumentation`, `GetFuncDesc`, `GetVarDesc`, `ITypeLib::IsName`, `FindName`, `GetRefTypeInfo`, `GetDllEntry` ve `TYPEATTR.cFuncs/cVars/cImplTypes` artik retained metadata'ya bagli | `DispGetIDsOfNames` / `DispInvoke` artik gercek vtable dispatch yapar; apartment init/refcount ve `CoMarshalInterThreadInterfaceInStream` / `CoGetInterfaceAndReleaseStream` retained state'e bagli; `LoadTypeLib` file-backed typelib/typeinfo/typecomp nesnesi doner, ama tam Windows binary typelib parser + marshaling ecosystem long-tail acik | automation dispatch suite |
| user32 core window/message | retained window/message path | Implemented + Partial | `src/win32.rs` | `cargo check --target x86_64-pc-windows-msvc --lib`; `cargo test --no-run --target x86_64-pc-windows-msvc --lib`; in-tree menu/message corpus'una ek olarak `WM_NCCALCSIZE` / `WM_NCPAINT` non-client bounds corpus'u, clip-aware invalidation bandlari ve foreground/focus activation ordering (`WM_NCACTIVATE` / `WM_ACTIVATE` / `WM_KILLFOCUS` / `WM_SETFOCUS` / activation-driven `WM_NCPAINT`) corpus'u var | exact Windows ordering/parity ve non-client long-tail tamamen kapanmis degil | GUI sample app |
| user32 dialogs | modal/dialog/template support | Implemented + Partial | `src/win32.rs` | `cargo check --target x86_64-pc-windows-msvc --lib`; dialog navigation/default-button corpus eklendi ama `cargo test --lib` repo-geneli eski kiriklarda bloklu | full dialog manager edge-cases ve template long-tail yok | dialog corpus |
| user32 accelerators | keyboard modifiers aware | Implemented + Partial | `src/win32.rs` | `cargo check --target x86_64-pc-windows-msvc --lib`; modifier-aware accelerator corpus eklendi ama `cargo test --lib` repo-geneli eski kiriklarda bloklu | exact fidelity ve WM_SYSKEY edge-case long-tail yok | accelerator suite |
| gdi32 retained draw | pen/brush/font/region/DC state | Implemented + Partial | `src/win32.rs` | `cargo check --target x86_64-pc-windows-msvc --lib`; `cargo test --no-run --target x86_64-pc-windows-msvc --lib`; select/delete/save-restore corpus'una ek olarak polygon-backed path-to-region, exclusion clip, point-visibility ve clip-aware `BitBlt` / `PatBlt` / `StretchBlt` ROP corpus'u mevcut; destination brush/pattern artik `MERGECOPY` benzeri blit yollarina tasiniyor ve blit destination rect'leri retained invalidation map'e dusuyor | exact raster/ROP/path parity yok | GDI sample corpus |
| gdi32 metafile | replayable command subset | Implemented + Partial | `src/win32.rs` | `cargo check --target x86_64-pc-windows-msvc --lib`; print/metafile replay plumbing stateful kaldı, in-tree corpus printer queue üstünden dogrulaniyor ama `cargo test --lib` bloklu | full EMF/WMF semantics yok | command stream tests |
| printer DC | queued print-job compatibility | Implemented + Partial | `src/win32.rs` | `cargo check --target x86_64-pc-windows-msvc --lib`; `cargo test --no-run --target x86_64-pc-windows-msvc --lib`; committed print-job queue corpus mevcut | real Windows spooler yok | print job replay tests |
| GDI handles | retained metadata table | Implemented + Partial | `src/win32.rs` | `cargo check --target x86_64-pc-windows-msvc --lib`; `cargo test --no-run --target x86_64-pc-windows-msvc --lib`; selected-count/delete semantics corpus mevcut | kernel-grade object manager yok | lifetime/leak tests |
| CRT / msvcrt | stdio/env/time/parse core + broad API table | Implemented + Partial | `src/win32.rs` | `cargo check --target x86_64-pc-windows-msvc --lib`; `cargo test --no-run --target x86_64-pc-windows-msvc --lib`; in-tree unit corpus mevcut | `fopen/fread/fwrite/fseek/ftell/getenv/time/strftime/strtol/strtod` artik stateful; `scanf/system/locale/process-exit` long-tail acik | CRT compatibility matrix |

## Truthfulness Rules

- `Declared` asla `working parity` anlamina gelmez.
- `Implemented` asla `Windows'tan farksiz` anlamina gelmez.
- `Verified` ancak siniri ile birlikte yazilir.

## Current High-Risk Gaps

1. `user32/gdi32` exact behavior long-tail
2. `CRT long-tail`
3. `Schannel/browser-grade HTTPS/TLS`
4. `COM/OLE type-info exactness`
5. deep unwind full-context exactness

## Phase Boundary

- Faz 2 kapandi: loader/export/import core, process/thread/TLS runtime core, visible SEH contract, WinHTTP/WinINet bridge ve COM class-factory core artik gercek runtime state'e bagli.
- Exact `user32/gdi32` davranis, COM/OLE automation long-tail, Schannel/browser-grade TLS, CRT long-tail ve loader/unwind exactness kuyrugu Faz 4'e tasindi.

## Exactness Exit Criteria

Faz 2'nin `tam uyumlu/exact` sayilmasi icin su kapilarin kapanmasi gerekir:

1. `user32/gdi32`
   - parity matrix'teki GUI/GDI satirlari `Partial` olarak kalmaz
   - message ordering, non-client behavior, GDI object lifetime ve printer/metafile semantics mekanik corpus ile sinanir
2. COM/OLE automation
   - `IUnknown`, `IDispatch`, apartment/threading ve automation dispatch davranisi stateful ve mekanik dogrulanmis olur
   - `LoadTypeLib` binary typelib ekosistemini ve `ITypeInfo` metadata publication'ini yalniz text/binary string harvest ile degil, gercek parser ile tasir; mevcut file-driven help-context/help-file publication bu kapanisa ara basamak
3. WinHTTP / WinINet / Schannel
   - TLS/certificate failure mapping, proxy/cookie/cache/session semantics exact uyum seviyesine gelir
   - RTC-backed cert time, revocation hard-fail, `CONNECT` auth, RFC1918/local bypass, multiline PAC/WPAD, hostname IP-SAN parity ve Schannel/browser-grade trust policy ayni contract'a iner
4. CRT / loader / unwind
   - `msvcrt` long-tail `Partial` olmaktan cikar
   - delayed import, forwarded export ve deep x64 unwind full-context corpus'u kapanir

Bu kosullar kapanmadan Faz 2 yalnizca `Verified core`, exact degil.
