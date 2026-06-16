# Density-Driven CSS Variables — Design

**Date:** 2026-06-16
**Status:** Approved (brainstorm), pending implementation plan

## Problem

Density mode (Compact / Regular / Comfy) currently changes almost nothing
visible besides font size. `data-density` on `<html>` swaps only four tokens —
`--row-h`, `--pad`, `--fs-ui`, `--fs-tree` — while the rest of the UI's spacing,
gaps, paddings, control heights, and small font sizes are **hardcoded px values
inline** across the component tree. A histogram of `src/app` + `styles.css`
shows ~1,800 hardcoded `px` literals spread over ~40 components (settings-modal
330, dev-panel 233, ticket-page 95, …). Because spacing is baked into inline
`style="…"` attributes, swapping density tokens has no effect on layout density.

**Goal:** changing density should change spacing, paddings, heights, and gaps —
not just font size. Achieve this by making *every* spacing/height/font value
reference a global theme CSS variable that responds to density.

## Decisions (locked during brainstorm)

1. **Token model — single density factor + `calc()`.** One `--density`
   multiplier is set explicitly per density (the anchor rows). Spacing and
   height tokens are *derived* from it via `calc(base * var(--density))`, so
   density flows automatically into anything built on the scale. Font sizes are
   set explicitly per density (they don't scale linearly). Borders and pill
   radius never scale.
2. **Migration — comprehensive literal→token sweep.** Zero hardcoded
   padding/margin/gap/height/font-size literals may remain in `src/app`. Every
   such value references a global token. Component extraction is *opportunistic*
   (route obvious repeaters through existing `.btn`/`.chip`-style atoms when a
   component already uses near-identical inline spacing) but is **not** the goal
   and introduces no new component APIs.
3. **Scale — snap to a clean scale.** Irregular existing values (3,5,7,9,11,13…)
   snap to the nearest step on a tight semantic scale. Sub-pixel/±1–2px shifts at
   regular density are an accepted cost in exchange for a small, reasoned
   vocabulary.
4. **Corner radii stay fixed** (not density-scaled) so cards don't look warped.
5. **Density anchors:** `0.85 / 1.00 / 1.18` (compact / regular / comfy).

## Token architecture

Replace the four ad-hoc density tokens with one anchor per density plus four
derived token families.

```css
:root                  { --density: 1.00; }   /* regular */
[data-density=compact] { --density: 0.85; }
[data-density=comfy]   { --density: 1.18; }
```

| Family | Response to density | Example |
|---|---|---|
| **Spacing** (padding/margin/gap) | `calc(base × --density)` | `--sp-4: calc(8px * var(--density))` |
| **Heights/controls** (rows, inputs, bars, buttons) | `calc(base × --density)` | `--ctl-h: calc(28px * var(--density))` |
| **Font sizes** | explicit per density | `--fs-ui: 12.5 / 11.5 / 13.5` |
| **Borders & pill radius** | never scale | `--hair: 1px`, `--r-pill: 999px` |

## Token vocabulary

Snapped to the real value histogram (heaviest spacing values: 5, 6, 8, 9, 4, 2,
10, 11, 12, 7px). `1px` (244×) is almost entirely borders; `999px` is pill
radius — neither scales.

**Spacing scale** — ~9 steps, each `calc(base * var(--density))`:

```
--sp-1: 2px    --sp-2: 4px    --sp-3: 6px    --sp-4: 8px
--sp-5: 10px   --sp-6: 12px   --sp-7: 16px   --sp-8: 20px   --sp-9: 24px
```

Snap rule: nearest step; **ties round down** toward the base unit. `5→4`,
`7→6`, `9→8`, `11→10`, `13→12`, `15→16`, `18→16`. Drift ≤2px, regular density
only. The exact snap table is
finalized in the implementation plan; where a high-frequency value would drift
>1px and is actually a *component size* (e.g. icon `size=14`), it is handled by
the component's size prop, not a spacing token.

**Control heights** — `calc(base * var(--density))`:

```
--ctl-h-sm: 22px   --ctl-h: 28px   --ctl-h-lg: 34px
--row-h: 30px (kept)   --topbar-h: 44px   --statusbar-h: 24px
```

**Type scale** — explicit per density (compact / regular / comfy):

```
--fs-2xs  9.5 / 9 / 10        --fs-xs  10.5 / 10 / 11
--fs-sm   11.5 / 11 / 12      --fs-ui  12.5 / 11.5 / 13.5 (kept)
--fs-tree (kept)             --fs-md  13.5 / 12.5 / 14.5
```

**Non-scaling:** `1px` borders (left literal — `--hair` is already the hairline
*color*, not a width), `--r-pill: 999px`, plus existing `--r-sm/md/lg` (kept,
fixed).

## Migration sweep

Target: zero hardcoded padding/margin/gap/height/font-size literals in
`src/app`. The only literals allowed are `1px` borders and `999px` pill radius
(`999px` also available as `--r-pill`).

**Per-component, mechanical:**

1. In each inline `style="…"` and `styles:[]` block, replace each
   spacing/height/font literal with its snapped token
   (`gap:7px → gap:var(--sp-4)`, `font-size:9.5px → font-size:var(--fs-2xs)`).
2. Leave layout structure untouched — no markup restructuring, no component
   extraction. Keeps each diff reviewable and regression risk low.
3. Order by blast radius, biggest offenders last: shared atoms + small
   components first (agent-row, status-bar, top-bar), settings-modal (330) and
   dev-panel (233) last.

**Opportunistic extraction:** route obvious repeaters (button, field/input, row)
through existing token-driven CSS atoms when a component already uses
near-identical inline spacing. Never the goal; no new component API.

**Guardrail against backsliding:** add a stylelint rule (or a small custom check
wired into the existing test/CI) that fails on raw `px` in spacing/font
properties within `src/app`, with an allowlist for `1px` and `999px`. Makes "no
hardcoded values" enforceable, not a one-time cleanup.

## Verification

1. **Visual regression across all 3 densities.** A Playwright spec loads the
   app, switches `data-density` to compact/regular/comfy, and screenshots key
   surfaces (shell, sidebar, settings-modal, ticket-page). Regular-density
   baselines must stay within snap tolerance (≤2px); compact/comfy must visibly
   differ in spacing — proving density drives layout, not just font size.
2. **Token-coverage test.** The stylelint/no-literal check, run in CI, is the
   objective proof that everything relies on CSS vars.
3. **Manual smoke** in `tauri dev`: toggle density in the Tweaks panel; confirm
   rows, padding, and heights all shift live.

## Touch points

- `src/styles.css` — token definitions (`:root`, `[data-density=*]` blocks),
  Tailwind `@theme` mapping for any new tokens used by utilities.
- `src/app/ui/ui.store.ts` — already applies `data-density`; density anchor
  values now live in CSS, so no logic change expected (verify).
- ~40 components under `src/app/**` — literal→token sweep.
- CI/test config — stylelint no-literal rule + Playwright density-regression
  spec.

## Out of scope

- Restructuring component markup or extracting a full atom/primitive component
  library.
- Changing the set of density modes or adding new tweaks.
- Theme/accent/motion token systems (unchanged).
