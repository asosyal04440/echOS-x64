# Curated App Commercial License Audit

## Summary

This audit covers the curated third-party application set that echOS currently packages through [C:\Users\Bahadir\Desktop\dersler_ve_projeler\echOS\scripts\package_curated_apps.ps1](C:\Users\Bahadir\Desktop\dersler_ve_projeler\echOS\scripts\package_curated_apps.ps1).

Goal: fail closed on GPL/AGPL-class bundled applications so the curated app lane does not silently block future commercial distribution work.

## Result

No currently curated packaged application was found to require removal under the "no GPL/AGPL bundled apps" rule.

Current allowlisted set:

| App | Upstream | License |
| --- | --- | --- |
| Helix | [helix-editor/helix](https://github.com/helix-editor/helix) | MPL-2.0 |
| Yazi | [sxyazi/yazi](https://github.com/sxyazi/yazi) | MIT |
| Zellij | [zellij-org/zellij](https://github.com/zellij-org/zellij) | MIT |
| bottom | [ClementTsang/bottom](https://github.com/ClementTsang/bottom) | MIT |
| GitUI | [gitui-org/gitui](https://github.com/gitui-org/gitui) | MIT |
| Glow | [charmbracelet/glow](https://github.com/charmbracelet/glow) | MIT |
| ripgrep | [BurntSushi/ripgrep](https://github.com/BurntSushi/ripgrep) | Unlicense OR MIT |
| fd | [sharkdp/fd](https://github.com/sharkdp/fd) | Apache-2.0 OR MIT |
| bat | [sharkdp/bat](https://github.com/sharkdp/bat) | Apache-2.0 OR MIT |
| Posting | [darrenburns/posting](https://github.com/darrenburns/posting) | Apache-2.0 |

## Enforcement

The packaging script now embeds a commercial-safe allowlist and fails closed when a curated app declares a disallowed or unreviewed license.

Allowed licenses in the packaging lane:

- `MIT`
- `Apache-2.0`
- `MIT OR Apache-2.0`
- `Apache-2.0 OR MIT`
- `MPL-2.0`
- `BSD-3-Clause`
- `BSD-2-Clause`
- `ISC`
- `Zlib`
- `MIT OR Unlicense`
- `Unlicense OR MIT`

Disallowed by policy in this lane:

- `GPL-*`
- `AGPL-*`
- `LGPL-*` unless separately reviewed for dynamic-linking/export constraints
- any unknown or missing license value

## Important Boundary

This audit only covers curated bundled third-party applications.

It does **not** solve the workspace root licensing problem.

The repository currently contains an AGPL license file at [C:\Users\Bahadir\Desktop\dersler_ve_projeler\echOS\LICENSE](C:\Users\Bahadir\Desktop\dersler_ve_projeler\echOS\LICENSE). If echOS is intended to become commercially distributable under a non-copyleft product model, the repository's own licensing posture must be addressed separately. No curated app cleanup can override that.
