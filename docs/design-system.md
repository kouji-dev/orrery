# Design System

**Context.** The "futuristic IntelliJ" aesthetic is built on a CSS‑variable token system, a
mono‑forward type pairing, an SVG icon set, and a small library of reusable Angular components.

Source: `styles.css` (tokens + atoms), `utils.ts` (icons, status meta, helpers),
`shared/*` (components), `index.html` (fonts).

## Tokens

- [x] Theme‑switched color tokens (`--bg`, `--panel*`, `--hair*`, `--ink*`, `--accent*`)
- [x] Status colors (running/blocked/waiting/done/idle) as tokens
- [x] Code add/del background + ink tokens for diffs
- [x] Density tokens (`--row-h`, `--pad`, font sizes) driven by `data-density`
- [x] Radii, glow, and shadow tokens
- [x] Tokens mapped into Tailwind v4 `@theme` so utilities stay themeable
- [x] `color-mix(oklch)` used for tinted/translucent surfaces

## Typography & texture

- [x] Display font: Space Grotesk; mono font: JetBrains Mono
- [x] Utility classes: `.disp`, `.mono`, `.tnum`, `.up`, `.grad-ink`
- [x] Background grid texture + accent glow layers
- [x] Custom slim scrollbars

## Icon set

- [x] Single stroke‑based SVG icon component driven by a named path map (50+ glyphs)
- [x] Sizes (sm/md/lg or explicit px) and inheritable color

## Shared components

- [x] `app-icon` — named SVG icon
- [x] `app-button` — ghost / ghost‑hair / primary variants with optional leading icon
- [x] `app-select` — styled native select (string or {value,label} options)
- [x] `app-status-dot` — animated per‑status dot
- [x] `app-status-pill` — status chip (filled + outline)
- [x] `app-tool-badge` — agent‑tool monogram (claude/codex/cursor/gemini)
- [x] `app-ring` — circular progress ring

## Atoms (CSS)

- [x] `.btn` (+ `.primary`, `.ghost-hair`), `.chip`, `.kbd`
- [x] `.surface` card, `.divider` / `.vdiv`
- [x] `.activity` bar, `.meter` bar, `.dot` states, `.caret`
- [x] `.osel` native‑select skin, `.field-label`

## Architecture notes

- [x] Standalone, OnPush components throughout
- [x] Central signals store (`OrchestraStore`) holds all state + actions
- [x] ~35 components split by domain under `src/app/orchestra/`
- [x] Wrapper hosts use `display:contents` so layout/scroll chains stay intact
