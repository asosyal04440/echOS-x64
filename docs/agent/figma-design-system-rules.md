# echOS Figma Design System Rules

## Platform Context

- Product: bare-metal Rust OS shell
- Runtime: retained scene graph
- Rendering: `RenderObject` pipeline, CPU-first, GPU-upgradeable
- Styling source of truth: [src/gui/theme.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/gui/theme.rs)
- Shell composition source of truth: [src/gfx/velvet_glove.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/gfx/velvet_glove.rs)

## Translation Rules

- Treat Figma as product intent, not literal frontend code.
- Map Figma surfaces to echOS scene producers, not HTML/CSS concepts.
- Prefer token reuse over hardcoded per-node styling.
- Use one token family per semantic purpose:
  - surfaces
  - text
  - accents
  - borders
  - shadows
  - radii
  - spacing
  - motion

## Component Mapping

- `Top bar`, `dock`, `launcher`, `overview`, `quick settings`, `notifications`, `lock screen` map to shell scene builders.
- `Window chrome` maps to display/chrome rendering contracts, not widget-local hacks.
- `Terminal` is the reference product window.
- Avoid creating visual states that imply functionality the current shell does not have.

## Token Rules

- Use graphite/mist/accent palette as the base family.
- Do not introduce purple-biased default themes.
- Accent is for focus, status, and controlled emphasis only.
- Borders should not define every surface.
- Spacing should create hierarchy before color does.

## Motion Rules

- Motion tokens are required for:
  - overlay enter/exit
  - workspace transition
  - focus change
  - window open/close
- Motion must feel structural, not decorative.

## Node Hygiene

- Name nodes by product role and state.
- Separate each shell surface into a clear frame/state pair.
- Keep tokenized variables attached where possible.
- Avoid anonymous groups and duplicated unnamed frames.

## Handoff Rules

- Every implementation-target node should have:
  - screenshotable state
  - stable name
  - known tokens
  - clear size and spacing
  - no ambiguous hidden alternates

- Preferred first implementation targets:
  1. `Window/Terminal/Focused`
  2. `Shell/TopBar/Default`
  3. `Shell/Dock/Default`
  4. `Shell/QuickSettings/Default`
  5. `Shell/Overview/Default`
