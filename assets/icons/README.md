# echOS Icon Assets

This directory contains the curated icon asset set for echOS UI, shell, installer, package, security, and diagnostics surfaces.

## Policy

- Primary source: Lucide icons from the official GitHub repository.
- License: ISC, copied into `licenses/lucide-LICENSE.txt`.
- Import scope: selected SVG files only, not the full upstream icon set.
- Runtime rule: kernel and hot-path code must not depend on SVG parsing. SVG files are source assets; boot/runtime consumers should use generated bitmap, atlas, or vector-cache outputs.
- Provenance rule: every imported icon must be listed in `ICONS_MANIFEST.toml`.
- Forbidden sources: random icon mirror sites, unknown-license assets, GPL/LGPL icon packs, and untracked web downloads.

## Layout

```text
assets/icons/
  ICONS_MANIFEST.toml
  README.md
  .lucide-source-commit
  licenses/
    lucide-LICENSE.txt
  source/
    lucide/
      *.svg
  generated/
    README.md
    16/
    24/
    32/
    64/
```

## Update Procedure

1. Resolve the upstream Lucide commit that will be imported.
2. Download only the selected `icons/*.svg` files from the official repository at that commit.
3. Refresh `licenses/lucide-LICENSE.txt` from the same commit.
4. Update `.lucide-source-commit` and `ICONS_MANIFEST.toml`.
5. Validate every SVG parses as XML and contains no script/event payload.
6. Generate runtime-specific raster or atlas outputs under `generated/`; do not edit generated outputs by hand.

## Initial Core Set

The first set intentionally stays small: power, settings, terminal, security, storage, input, network, diagnostics, package, and display primitives. This keeps the OS visual language coherent without vendoring thousands of unused assets.
