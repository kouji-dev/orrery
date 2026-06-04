# Theming & Layout (Tweaks)

**Context.** A floating Tweaks panel exposes theme, accent palette, density, layout, and motion
controls. All settings are applied through CSS custom properties / `data-*` attributes on the
document root, so the whole UI re‑themes instantly.

Source: `tweaks/tweaks-panel.component.ts`, `orchestra.store.ts` (`tweaks`, `setTweak`),
`styles.css` (tokens), `index.html` (`data-theme`/`data-density`/`data-motion`).

## Tweaks panel

- [x] Floating launcher button (accent gradient) bottom‑right
- [x] Toggleable panel with sectioned controls
- [x] All controls write to the central tweaks state and apply live

## Theme

- [x] Dark / Light mode segmented control
- [x] Theme also toggleable from the top bar
- [x] Full dark + light token sets (bg, panels, hairlines, ink ramps, code add/del)
- [x] Accent palette swatches: Nebula / Plasma / Reactor / Ember
- [x] Accent applied as `--accent` / `--accent-2` (+ rgb) and flows into gradients, glows, dots

## Layout

- [x] Density: Compact / Regular / Comfy (drives row height, padding, font sizes)
- [x] Right‑panel show/hide toggle (workspace switches to a 2‑column grid when hidden)

## Orchestrator

- [x] Default agent visualization select (grid / kanban / graph / timeline)
- [x] Live‑motion toggle (disables animations/transitions globally via `data-motion=off`)

## Structural layout

- [x] App shell grid: 44px top bar / flexible body / 24px status bar, constrained to the viewport
- [x] Body grid: sidebar / center / right panel (right column drops when toggled off)
- [x] Footer pinned to the bottom; sidebar, center, and right panel each scroll internally
- [x] Background grid texture + dual accent glow vignette
