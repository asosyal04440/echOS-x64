# echOS UI 10/10 Release Gate (Hybrid Titan, UI-Only)

## Scope
- UI/shell only: boot, login/lock, desktop shell surfaces, window chrome, halo bar, dock/task strip, quick settings, notifications, command palette, stage rail.
- Kernel, drivers, KMS/compositor internals are out of this gate.

## Matrix
- Resolutions: `1920x1080`, `2560x1440`, `3840x2160`.
- Themes: `Dark`, `Light`.
- Density profile: `Desktop` and `Compact` (where width triggers compact).

## Visual Golden Set
- Required golden scenes:
1. Boot splash (emblem + progress beam).
2. Login/lock (idle + wrong-password shake frame).
3. Desktop idle (halo bar + dock/task strip + wallpaper depth).
4. Active/inactive window chrome pair.
5. Quick settings open.
6. Command palette open (query empty + filtered list).
7. Stage rail open with active set.
8. Notifications surface with mixed severity entries.
- Pass rule: no geometry drift outside density scaling; no token bypass (hardcoded palette) in touched surfaces.

## Interaction Gate
- Hover/press/focus timing:
1. Hover preset `120ms`.
2. Press preset `70ms`.
3. Focus preset `160ms`.
4. Launch/minimize preset `220ms`.
- Keyboard paths:
1. `Super+Space` toggles command palette.
2. `Super+,` toggles quick settings.
3. `Super+\`` cycles stage sets.
- Pointer hit targets:
1. Window chrome controls: minimum `44x28`.
2. Quick settings rows: minimum `44x28`.
3. Top bar action controls must not clip at halo height.

## Accessibility Gate
- Contrast/readability:
1. Primary text on panel surfaces remains readable in both dark/light.
2. Muted/secondary text auto-falls back when luma contrast is insufficient.
- Focus visibility:
1. Selected row and active stage set must preserve visible border/accent in both themes.
- Keyboard navigation:
1. Palette selection (`j/k`, arrows, enter, esc) deterministic with filtered lists.

## Performance Gate (UI)
- During shell interactions (panel/palette open-close, stage set switch):
1. No visible jitter spikes in normal flow.
2. No repeated full-surface repaint caused by stale dirty flags.
3. No pointer hit-lag regression after layout/contrast updates.

## Fail Conditions
- Any hardcoded shell color path bypassing theme tokens in newly touched surfaces.
- Any control below `44x28` logical target where pointer interaction is expected.
- Theme parity regression (light mode layout drift vs dark).
- Shortcut mismatch or dead action in command palette/stage rail.
