# echOS Hypr-Pro Figma Polish Handoff

## Goal

Build a product-level shell design in Figma for echOS that preserves the current retained scene architecture while replacing the prototype-looking shell composition with a calmer, more coherent Hypr-Pro visual system.

The first implementation target is not new functionality. It is visual productization of existing shell surfaces:

- top bar
- dock / task strip
- launcher / command palette
- workspace overview
- quick settings
- notifications
- lock screen
- terminal window chrome

## Repo-First Audit

### What Feels Prototype

- `top bar` reads like a status strip plus debug dashboard instead of a product shell header.
- `task strip` is visually over-segmented; workspace controls, app cards, and shortcut hints compete at the same hierarchy level.
- `launcher` is copy-heavy and panel-heavy. It explains the shell instead of feeling like a tool.
- `quick settings` is a thin row list without grouping, density control, or system-at-a-glance hierarchy.
- `overview` is still a stage rail list, not a real workspace overview with spatial understanding.
- `notifications` and `dialogs` resemble generic data panels more than system surfaces.
- `lock screen` is functionally fine but not yet premium; it lacks a strong focal center and atmospheric depth.

### What Breaks Product Feel

- Too many bordered rectangles with equal emphasis.
- Accent lines are overused as separators instead of being reserved for focus or priority.
- Text hierarchy is too flat: labels, metadata, commands, and state badges often share the same visual weight.
- Negative space is underused; most surfaces are dense and filled rather than intentionally composed.
- Shell vocabulary is inconsistent and tool-like:
  - `Desktop Launcher`
  - `Workspace Overview`
  - `Notification Surface`
  - `Dialog Broker`
- Several surfaces are list-first instead of object-first, which makes the shell feel administrative instead of spatial.

### What Stays

- The dark graphite / aqua / azure / gold family in [src/gui/theme.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/gui/theme.rs) is a solid base.
- The clean desktop startup direction is correct.
- Workspace-driven interaction remains the right shell model.
- Terminal as the first reference window is the right product anchor.
- Existing shell topology in [src/gfx/velvet_glove.rs](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/gfx/velvet_glove.rs) is already separated enough to redesign surface by surface.

## Figma File Structure

Create one new Figma file:

- `echOS Hypr-Pro Product Shell`

Pages:

1. `00 Audit`
2. `01 Tokens`
3. `02 Desktop`
4. `03 Window Chrome`
5. `04 Launcher + Palette`
6. `05 Overview + Workspaces`
7. `06 Quick Settings + Notifications`
8. `07 Motion`
9. `08 Handoff`

## Page Requirements

### 00 Audit

Frames:

- `Current Shell Problems`
- `Keep / Fix / Remove`
- `Product Direction`

Annotate:

- hierarchy failures
- spacing failures
- state ambiguity
- overlay clutter
- chrome inconsistency

### 01 Tokens

Create variables and token tables for:

- `color.surface.desktop`
- `color.surface.panel`
- `color.surface.overlay`
- `color.surface.window`
- `color.surface.titlebar.active`
- `color.surface.titlebar.inactive`
- `color.text.primary`
- `color.text.secondary`
- `color.text.tertiary`
- `color.accent.primary`
- `color.accent.secondary`
- `color.accent.warning`
- `color.accent.error`
- `color.border.subtle`
- `color.border.focus`
- `radius.sm`
- `radius.md`
- `radius.lg`
- `space.4`
- `space.8`
- `space.12`
- `space.16`
- `space.24`
- `motion.hover`
- `motion.focus`
- `motion.overlay`
- `motion.workspace`

Typography styles:

- `Display / Shell Hero`
- `Title / Panel`
- `Title / Window`
- `Body / Primary`
- `Body / Secondary`
- `Meta / System`
- `Mono / Terminal`

### 02 Desktop

Frames:

- `Idle Desktop`
- `Desktop With One Terminal`
- `Desktop With Two Windows`

Rules:

- launcher closed by default
- command palette closed by default
- overview closed by default
- quick settings closed by default
- desktop must feel intentional, not empty

### 03 Window Chrome

Reference window: `Terminal`

Frames:

- `Focused Terminal`
- `Inactive Terminal`
- `Floating Utility Window`
- `Modal Window`

Specify:

- titlebar height
- title alignment
- action button layout
- focused border behavior
- inactive contrast
- shadow strength by elevation

### 04 Launcher + Palette

Split the current launcher into two product surfaces:

- `App Launcher`
- `Command Palette`

Rules:

- launcher is app-centric, grid or compact cards
- palette is command-centric, keyboard-first, denser
- remove explanatory shell copy from the core interaction zone

### 05 Overview + Workspaces

Replace the current stage rail feel with a real overview.

Frames:

- `Overview Grid`
- `Overview With Active Workspace`
- `Overview With Scratchpad Hint`

Rules:

- workspaces should read spatially
- each workspace card shows:
  - name / index
  - layout
  - window count
  - active state

### 06 Quick Settings + Notifications

Frames:

- `Quick Settings Default`
- `Quick Settings Expanded`
- `Notifications Feed`
- `Single High-Priority Notification`

Rules:

- quick settings grouped by category
- notifications should feel lighter than dialogs
- state badges should rely on hierarchy and spacing first, accent second

### 07 Motion

Required specs:

- `overlay enter`
- `overlay exit`
- `workspace switch`
- `focus shift`
- `window open`
- `window close`

Default durations:

- focus: `120–160 ms`
- overlay: `140–180 ms`
- workspace switch: `180 ms`
- window open/close: `180–220 ms`

### 08 Handoff

For every primary surface:

- final frame
- annotation frame
- spacing callouts
- token callouts
- motion references

Preferred node names:

- `Shell/TopBar/Default`
- `Shell/Dock/Default`
- `Shell/Launcher/Default`
- `Shell/Palette/Results`
- `Shell/Overview/Default`
- `Shell/QuickSettings/Default`
- `Shell/Notifications/List`
- `Window/Terminal/Focused`

## Surface-by-Surface Design Directives

### Top Bar

- Remove dashboard feeling.
- Keep left brand, center command affordance, right system cluster.
- Reduce segmentation lines.

### Dock / Task Strip

- Treat as a dock-first strip, not a panel full of independent widgets.
- Workspace chips and app presence should not fight.
- Shortcut legend must be de-emphasized.

### Launcher

- Stop explaining the shell.
- Show apps and recent intent, not a management console.

### Overview

- Must feel like a workspace lens, not a list widget.

### Quick Settings

- Group by function:
  - appearance
  - sound
  - notifications
  - power/session

### Notifications

- Prioritize recency and severity.

### Lock Screen

- Make one strong visual focal point.
- Authentication area should feel premium, not debug-like.

## Figma MCP Deliverables Already Created

- [echOS Shell State Flow](https://www.figma.com/online-whiteboard/create-diagram/92efab49-0538-4868-af4f-eb7d860cc8ef?utm_source=other&utm_content=edit_in_figjam&oai_id=&request_id=9e46e28b-a3d5-4932-835e-5ebf48fc7803)
- [echOS Overlay Priority Matrix](https://www.figma.com/online-whiteboard/create-diagram/ae96aafb-939f-4889-b602-2c35d817bd4d?utm_source=other&utm_content=edit_in_figjam&oai_id=&request_id=e839eee3-fe43-4848-a379-200e7a194850)

## Next MCP Pass

When we switch from polish to implementation, the next pass must use:

- `get_design_context`
- `get_screenshot`
- `get_metadata` only if node scope is too large
- `get_variable_defs`

Implementation mapping rules:

- prefer repo theme tokens over ad-hoc colors
- shell surfaces map to retained scene producers, not framebuffer paint helpers
- terminal window chrome is the first code parity target

## Acceptance Criteria

- Clean desktop looks premium with no launcher visible.
- Top bar, dock, window chrome, and overlays share one visual language.
- Overview, quick settings, and notifications are visually distinct but related.
- Figma nodes are named and grouped so an implementer can fetch exact states with MCP.
