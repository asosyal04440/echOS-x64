# Curated App Compatibility Matrix

Date: 2026-04-08

Scope: repo-visible truth table for `TODO.md` `DIST-03`.

Primary sources:

- [scripts/package_curated_apps.ps1](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/scripts/package_curated_apps.ps1)
- [src/gfx/velvet_glove_registry.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/gfx/velvet_glove_registry.rs)
- [src/security/package.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/security/package.rs)
- [src/runtime.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/runtime.rs)

## Contract summary

Curated third-party apps in echOS are currently packaged as signed `.bhd` bundles with:

- `runtime = "pe"`
- `presentation = "shell-owned"`
- bounded shell-facing capabilities (`fs.read`, dialogs, notifications, clipboard)
- packaged install / verify / remove flow through the package/control-plane contract

That means the supported boundary is not "all Windows apps". The supported boundary is the narrower class of packaged PE applications that behave like shell-owned console/text tools and do not require privileged Windows desktop integration.

## Supported family boundary

| Family | Status | Why it is inside the contract | Current curated examples |
| --- | --- | --- | --- |
| Console/TUI editor and navigator apps | Supported | Shell-owned PE package, bounded window contract, no kernel-mode or desktop-compositor ownership requirement | Helix, Yazi |
| Terminal workspace / multiplexer apps | Supported | Same shell-owned PE lane; expected to operate through text/terminal semantics rather than native Windows desktop shell APIs | Zellij |
| Text-first observability / Git / Markdown tools | Supported | Package/runtime contract already treats them as packaged PE payloads with bounded filesystem/dialog/clipboard surface | bottom, GitUI, Glow |
| Search / file-discovery / viewer utilities | Supported | Single-process PE tools with text-centric I/O and no installer/service side effects | ripgrep, fd, bat |
| API/HTTP client in shell-owned packaging lane | Conditionally supported | Runtime family fits the same shell-owned PE contract, but host packaging currently remains best-effort in the scripted producer path | Posting |

## Unsupported family boundary

| Family | Status | Why it is outside the contract |
| --- | --- | --- |
| Native Win32 desktop GUI stacks (`Explorer`-class, WPF, WinUI, Qt, Electron, browser-class shells) | Unsupported | Current curated manifest lane is `presentation = "shell-owned"` and does not promise full desktop-shell, compositor, COM, or browser-engine parity |
| Installer/updater binaries that self-mutate system state | Unsupported | Install/update authority is intentionally centralized behind `PackageRegistry` and `UpdateInstaller`; self-managed third-party installers violate that control-plane ownership |
| Kernel drivers, services, shell extensions, COM registration flows | Unsupported | These require privileged OS integration, persistent service registration, or kernel/device ownership outside the packaged app contract |
| Apps that depend on admin elevation, raw device access, or custom driver stacks | Unsupported | Curated package manifests expose bounded user-facing capabilities, not arbitrary privileged execution |
| Linux-native ELF/WSL userland shipped as curated app family | Unsupported in this matrix | This matrix covers the current curated producer path, which packages PE payloads; ELF support exists elsewhere in the platform but is not the current curated-app distribution contract |

## Curated app truth table

| App | Family | Status | Boundary note |
| --- | --- | --- | --- |
| Helix | Console/TUI editor | Supported | Fits packaged shell-owned PE contract |
| Yazi | Console/TUI file manager | Supported | Fits packaged shell-owned PE contract |
| Zellij | Terminal multiplexer | Supported | Fits packaged shell-owned PE contract |
| bottom | Text system monitor | Supported | Fits packaged shell-owned PE contract |
| GitUI | Text Git client | Supported | Fits packaged shell-owned PE contract |
| Glow | Markdown viewer | Supported | Fits packaged shell-owned PE contract |
| ripgrep | Search utility | Supported | Fits packaged shell-owned PE contract |
| fd | File discovery utility | Supported | Fits packaged shell-owned PE contract |
| bat | Text viewer | Supported | Fits packaged shell-owned PE contract |
| Posting | API client | Conditionally supported | Runtime family is inside the boundary, but producer script may skip packaging if source install fails |

## V1 release stance

The v1 product floor is intentionally narrower than "general Windows compatibility".

- v1 confidence comes first from first-party desktop apps: terminal, files, editor, settings, package/seed catalog, and the bounded native web shell
- curated third-party compatibility exists to strengthen that story with shell-owned utility apps, not to replace the first-party floor
- browser-class binaries, Electron/CEF stacks, installer/updater families, services/drivers, and game-facing graphics parity are explicitly outside the v1 release gate even when they remain long-tail roadmap work elsewhere

For v1, this matrix therefore means:

- release-positive: shell-owned PE text/console and small utility families that complement the first-party floor
- release-negative: treating browser-class GUI compatibility or DXGI/game parity as prerequisites for saying the OS already feels usable on day one

## Decision

`DIST-03` is closed for the repository's current productization scope by explicitly publishing the family boundary above:

- supported: packaged, signed, shell-owned PE text/console applications
- unsupported: full native Windows desktop shells, installers, services, drivers, and privileged integration families

This matrix does not claim universal PE compatibility. It defines the honest commercial/product boundary for the curated app lane that the repository currently produces.
