# Density-Driven CSS Variables Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make density mode (Compact/Regular/Comfy) change spacing, paddings, gaps, and control heights — not just font size — by routing every spacing/height/font value through density-driven global CSS variables.

**Architecture:** One `--density` multiplier is set per density on `<html>`. Spacing and height tokens derive from it via `calc(base * var(--density))`; font sizes are explicit per density (legibility floor); borders/pill-radius never scale. Every hardcoded `padding/margin/gap/height/font-size` px literal across `src/app` (~1,800) is swept to a token. A node scanner (run in vitest + CI) enforces "no literals left." A Playwright spec proves density now drives computed layout.

**Tech Stack:** Angular 20 (standalone, OnPush, inline `style="…"` + `styles:[]`), Tailwind v4 `@theme`, CSS custom properties, vitest (jsdom), Playwright (added here), Node ESM tooling.

**Spec:** `docs/superpowers/specs/2026-06-16-density-css-vars-design.md`

---

## File Structure

| File | Responsibility |
|---|---|
| `src/styles.css` | Token definitions (`--density`, spacing scale, control heights, type scale, `--hair`/`--r-pill`); atom classes (`.btn`, `.chip`, …) swept to tokens. |
| `src/app/ui/ui.store.ts` | Already applies `data-density` to `<html>`. No logic change — density values now live in CSS. Verify only. |
| `src/app/**/*.ts` (~40 components) | Literal→token sweep of inline `style="…"` + `styles:[]`. |
| `tools/density/check-tokens.mjs` | Scanner: finds disallowed `px` literals in target properties. CLI + exported `scanText`/`scanRepo`. |
| `src/density-tokens.spec.ts` | vitest: token structure test (Task 2) + scanner acceptance test (Task 19). |
| `tools/density/check-tokens.spec.ts` | vitest: scanner unit test against fixtures (Task 3). |
| `playwright.config.ts`, `e2e/density.spec.ts` | Functional E2E: density changes computed spacing/height. |
| `.github/workflows/test.yml` | CI: runs `pnpm test` (includes the scanner acceptance gate). |

## Reference: Snap Mapping Tables (used by every sweep task)

These tables are the single source of truth for the sweep. Snap rule = nearest scale step; **ties round down** toward the base unit.

**SPACING** — `padding*`, `margin*`, `gap`/`row-gap`/`column-gap` (literal px → token):

| px | token | px | token | px | token |
|----|-------|----|-------|----|-------|
| 2  | `--sp-1` | 9  | `--sp-4` | 15 | `--sp-7` |
| 3  | `--sp-1` | 10 | `--sp-5` | 16 | `--sp-7` |
| 4  | `--sp-2` | 11 | `--sp-5` | 18 | `--sp-7` |
| 5  | `--sp-2` | 12 | `--sp-6` | 20 | `--sp-8` |
| 6  | `--sp-3` | 13 | `--sp-6` | 22 | `--sp-8` |
| 7  | `--sp-3` | 14 | `--sp-6` | 24 | `--sp-9` |
| 8  | `--sp-4` | | | >24 | nearest `--sp-*` |

- `0` / `0px` → `0` (leave). Percentages / `auto` → leave.
- Negative (e.g. `-8px`) → `calc(var(--sp-4) * -1)`.

**HEIGHT** — `height`, `min-height` (literal px → token), context-sensitive:

| Role | px (typical) | token |
|---|---|---|
| Tiny decorative (dot, bar, rail) | ≤ 16 | use SPACING table (`5→--sp-2`, `2→--sp-1`, …) |
| Small control | 22 | `--ctl-h-sm` |
| Standard control / input | 26–28 | `--ctl-h` |
| List/tree row | 30 | `--row-h` |
| Large control | 32–34 | `--ctl-h-lg` |
| Top bar | 44 | `--topbar-h` |
| Status bar | 24 (in status-bar only) | `--statusbar-h` |
| Bespoke block, no token fits | >34 | add an entry to the scanner allowlist (stays fixed; document why) |

> `max-height` and `width` are **out of scope** (structural/scroll/panel sizing) — not swept, not flagged. Tokenize only opportunistically.

**FONT-SIZE** — `font-size` (literal px → token):

| px | token | px | token |
|----|-------|----|-------|
| 8, 8.5 | `--fs-3xs` | 13, 13.5 | `--fs-md` |
| 9, 9.5 | `--fs-2xs` | 14–16 | `--fs-lg` |
| 10, 10.5 | `--fs-xs` | 19, 21, 22 | `--fs-xl` |
| 11, 11.5 | `--fs-sm` | 24 | `--fs-2xl` |
| 12, 12.5 | `--fs-ui` | 36 | `--fs-display` |

> Where an existing usage already says `var(--fs-tree)` (tree rows), keep it — `--fs-tree` is retained. Generic 12.5px UI text → `--fs-ui`.

## Sweep Procedure (applies to Tasks 4–17)

For the target file(s):

1. Find every `padding*`, `margin*`, `gap`/`row-gap`/`column-gap`, `height`/`min-height`, `font-size` declaration in inline `style="…"` attributes and `styles:[ ` … ` ]` blocks.
2. Replace each px literal using the mapping tables above. Multi-value shorthands convert each value (`padding:6px 10px 7px` → `padding:var(--sp-3) var(--sp-5) var(--sp-3)`).
3. Do **not** restructure markup, rename classes, or extract components. Pure value substitution. (Opportunistic: if a component already duplicates an existing atom like `.btn` exactly, you may switch to the class — but never invent new component APIs.)
4. Leave `1px` borders, `999px`, `0`, percentages, `auto`, `max-height`, and `width` untouched.
5. Verify the file/dir is clean: `node tools/density/check-tokens.mjs <paths>` → `0 violations`.
6. Commit.

Throughout the sweep `pnpm test` stays green — the scanner's **acceptance** assertion is added only in Task 19. Use the CLI for per-batch progress.

---

### Task 1: Token foundation in `styles.css`

**Files:**
- Modify: `src/styles.css:71-89` (replace the density block) and `src/styles.css:66-69` (radii — add `--hair`/`--r-pill`)

- [ ] **Step 1: Replace the density token block**

Replace lines 71-89 (the `/* density */` block in `:root`, plus the `[data-density="compact"]` and `[data-density="comfy"]` blocks) with:

```css
  /* radii (fixed — not density-scaled) */
  --r-sm: 5px;
  --r-md: 8px;
  --r-lg: 12px;
  --r-pill: 999px;
  --hair: 1px;

  /* ---- DENSITY ENGINE ---- */
  /* single multiplier; spacing + heights derive from it */
  --density: 1;

  /* spacing scale (padding / margin / gap) */
  --sp-1: calc(2px  * var(--density));
  --sp-2: calc(4px  * var(--density));
  --sp-3: calc(6px  * var(--density));
  --sp-4: calc(8px  * var(--density));
  --sp-5: calc(10px * var(--density));
  --sp-6: calc(12px * var(--density));
  --sp-7: calc(16px * var(--density));
  --sp-8: calc(20px * var(--density));
  --sp-9: calc(24px * var(--density));

  /* control + structural heights */
  --ctl-h-sm: calc(22px * var(--density));
  --ctl-h:    calc(28px * var(--density));
  --ctl-h-lg: calc(34px * var(--density));
  --row-h:    calc(30px * var(--density));   /* kept name; now derived */
  --topbar-h:    calc(44px * var(--density));
  --statusbar-h: calc(24px * var(--density));

  /* back-compat alias (old --pad == 12px) */
  --pad: var(--sp-6);

  /* type scale — explicit per density (regular values here) */
  --fs-3xs: 9px;
  --fs-2xs: 9.5px;
  --fs-xs:  10.5px;
  --fs-sm:  11.5px;
  --fs-ui:  12.5px;   /* kept */
  --fs-tree: 12.5px;  /* kept */
  --fs-md:  13.5px;
  --fs-lg:  15px;
  --fs-xl:  21px;
  --fs-2xl: 24px;
  --fs-display: 36px;
}

[data-density="compact"] {
  --density: 0.85;
  --fs-3xs: 8.5px;
  --fs-2xs: 9px;
  --fs-xs:  10px;
  --fs-sm:  11px;
  --fs-ui:  11.5px;
  --fs-tree: 11.5px;
  --fs-md:  12.5px;
  --fs-lg:  14px;
  --fs-xl:  19px;
  --fs-2xl: 22px;
  --fs-display: 32px;
}

[data-density="comfy"] {
  --density: 1.18;
  --fs-3xs: 9.5px;
  --fs-2xs: 10.5px;
  --fs-xs:  11.5px;
  --fs-sm:  12.5px;
  --fs-ui:  13.5px;
  --fs-tree: 13px;
  --fs-md:  14.5px;
  --fs-lg:  16.5px;
  --fs-xl:  23px;
  --fs-2xl: 26px;
  --fs-display: 40px;
}
```

> Note: the original `--r-sm/md/lg` block at lines 66-69 is now folded into the snippet above — delete the old standalone radii block so radii are defined once.

- [ ] **Step 2: Build to confirm CSS is valid**

Run: `pnpm build`
Expected: build succeeds (Angular + Tailwind compile, no CSS parse error).

- [ ] **Step 3: Commit**

```bash
git add src/styles.css
git commit -m "feat(density): add density-factor token engine (spacing/height/type scale)"
```

---

### Task 2: Token structure test

**Files:**
- Create: `src/density-tokens.spec.ts`

- [ ] **Step 1: Write the test**

```ts
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, it, expect } from "vitest";

const css = readFileSync(resolve(__dirname, "styles.css"), "utf8");

describe("density token engine", () => {
  it("defines --density in :root and both density blocks", () => {
    expect(css).toMatch(/:root[^}]*--density:\s*1\b/s);
    expect(css).toMatch(/\[data-density="compact"\][^}]*--density:\s*0\.85/s);
    expect(css).toMatch(/\[data-density="comfy"\][^}]*--density:\s*1\.18/s);
  });

  it("derives every spacing + height token from --density via calc()", () => {
    for (const t of ["--sp-1","--sp-4","--sp-9","--ctl-h","--row-h","--topbar-h","--statusbar-h"]) {
      const re = new RegExp(`${t}:\\s*calc\\([^;]*var\\(--density\\)`);
      expect(css, `${t} must be calc(... var(--density))`).toMatch(re);
    }
  });

  it("keeps borders + pill radius non-scaling", () => {
    expect(css).toMatch(/--hair:\s*1px/);
    expect(css).toMatch(/--r-pill:\s*999px/);
  });

  it("redefines the type scale in all three densities", () => {
    for (const block of [/:root/, /\[data-density="compact"\]/, /\[data-density="comfy"\]/]) {
      const seg = css.slice(css.search(block));
      expect(seg).toMatch(/--fs-ui:\s*\d/);
    }
  });
});
```

- [ ] **Step 2: Run it**

Run: `pnpm test density-tokens`
Expected: PASS (4 tests).

- [ ] **Step 3: Commit**

```bash
git add src/density-tokens.spec.ts
git commit -m "test(density): assert token engine structure"
```

---

### Task 3: Literal scanner tool + unit test

**Files:**
- Create: `tools/density/check-tokens.mjs`
- Create: `tools/density/check-tokens.spec.ts`

- [ ] **Step 1: Write the scanner**

```js
// tools/density/check-tokens.mjs
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, resolve } from "node:path";

// Properties whose px literals MUST be tokens. max-height/width are out of scope.
const TARGET = [
  "padding","padding-top","padding-right","padding-bottom","padding-left",
  "padding-inline","padding-inline-start","padding-inline-end",
  "padding-block","padding-block-start","padding-block-end",
  "margin","margin-top","margin-right","margin-bottom","margin-left",
  "margin-inline","margin-inline-start","margin-inline-end",
  "margin-block","margin-block-start","margin-block-end",
  "gap","row-gap","column-gap","height","min-height","font-size",
];
const PROPS = TARGET.slice().sort((a, b) => b.length - a.length).join("|");
const RE = new RegExp(`(?<![\\w-])(${PROPS})\\s*:\\s*([^;"'\`}]*)`, "g");
const PX = /-?\d*\.?\d+px/g;
const ALLOW_PX = new Set(["0px", "1px", "999px"]);

// Documented exceptions: { file: <substring>, value: <substring> }. Bespoke
// structural heights that intentionally stay fixed go here (populated in sweep).
export const ALLOWLIST = [];

export function scanText(text, file = "") {
  const out = [];
  for (const m of text.matchAll(RE)) {
    const [, prop, rawVal] = m;
    const val = rawVal.trim();
    const pxs = (val.match(PX) || []).map((s) => s.replace("-", ""));
    const bad = pxs.filter((p) => !ALLOW_PX.has(p));
    if (!bad.length) continue;
    if (ALLOWLIST.some((a) => file.includes(a.file) && val.includes(a.value))) continue;
    out.push(`${prop}: ${val}`);
  }
  return out;
}

function walk(p, acc = []) {
  const s = statSync(p);
  if (s.isDirectory()) {
    for (const e of readdirSync(p)) walk(join(p, e), acc);
  } else if (/\.(ts|css)$/.test(p) && !/\.spec\.ts$/.test(p)) {
    acc.push(p);
  }
  return acc;
}

const ROOT = resolve(import.meta.dirname, "../..");
export function defaultFiles() {
  return [resolve(ROOT, "src/styles.css"), ...walk(resolve(ROOT, "src/app"))];
}

export function scanRepo(files = defaultFiles()) {
  const byFile = {};
  let total = 0;
  for (const f of files) {
    const v = scanText(readFileSync(f, "utf8"), f);
    if (v.length) { byFile[f] = v; total += v.length; }
  }
  return { total, byFile };
}

// CLI: `node check-tokens.mjs [paths...]`
if (process.argv[1] && process.argv[1].endsWith("check-tokens.mjs")) {
  const args = process.argv.slice(2);
  const files = args.length ? args.flatMap((a) => walk(resolve(a))) : defaultFiles();
  const { total, byFile } = scanRepo(files);
  for (const [f, vs] of Object.entries(byFile)) {
    console.log(`\n${f}  (${vs.length})`);
    for (const v of vs.slice(0, 50)) console.log(`  ${v}`);
  }
  console.log(`\n${total} violations`);
  process.exit(total ? 1 : 0);
}
```

- [ ] **Step 2: Write the scanner unit test**

```ts
import { describe, it, expect } from "vitest";
import { scanText } from "./check-tokens.mjs";

describe("scanText", () => {
  it("flags px literals in target properties", () => {
    expect(scanText('style="padding:6px 10px 7px;gap:7px"')).toEqual([
      "padding: 6px 10px 7px",
      "gap: 7px",
    ]);
    expect(scanText("font-size:9.5px")).toEqual(["font-size: 9.5px"]);
  });

  it("allows tokens, 0, 1px borders, 999px, percentages", () => {
    expect(scanText("padding:var(--sp-3) var(--sp-5)")).toEqual([]);
    expect(scanText("height:1px")).toEqual([]);
    expect(scanText("border-radius:999px")).toEqual([]);
    expect(scanText("margin:0;width:100%")).toEqual([]);
  });

  it("ignores out-of-scope properties (max-height, width, line-height, border)", () => {
    expect(scanText("max-height:320px;width:252px;line-height:20px;border:1px solid")).toEqual([]);
  });

  it("does not match custom-property definitions", () => {
    expect(scanText("--row-h: calc(30px * var(--density))")).toEqual([]);
  });

  it("flags negative margins", () => {
    expect(scanText("margin-left:-8px")).toEqual(["margin-left: -8px"]);
  });
});
```

- [ ] **Step 3: Add npm script, register the test dir, run tests**

Add to `package.json` `scripts`: `"check:tokens": "node tools/density/check-tokens.mjs"`.

The vitest `include` currently misses `tools/`. Edit `vitest.config.ts` so `include` is:
```ts
    include: ['src/**/*.spec.ts', 'scripts/**/*.spec.ts', 'tools/**/*.spec.ts'],
```

Run: `pnpm test check-tokens`
Expected: PASS (5 tests).

Run: `pnpm check:tokens`
Expected: prints a large baseline count (~1,800) and exits 1 — confirms the scanner sees the unswept literals.

- [ ] **Step 4: Commit**

```bash
git add tools/density/check-tokens.mjs tools/density/check-tokens.spec.ts package.json vitest.config.ts
git commit -m "feat(density): literal scanner + unit tests"
```

---

### Task 4: Sweep `styles.css` atoms/utilities

**Files:**
- Modify: `src/styles.css` (atom classes: `.btn`, `.chip`, `.kbd`, `.surface`, `.activity`, `.meter`, `.osel`, `.field-label`, scrollbars, etc.)

- [ ] **Step 1: Apply the Sweep Procedure** to all CSS rule bodies in `styles.css` (NOT the `:root`/`[data-density]` token definitions — those are correct). Convert `padding/margin/gap/height/min-height/font-size` px literals to tokens per the mapping tables.

- [ ] **Step 2: Verify**

Run: `node tools/density/check-tokens.mjs src/styles.css`
Expected: `0 violations`.

- [ ] **Step 3: Build + commit**

Run: `pnpm build` → succeeds.
```bash
git add src/styles.css
git commit -m "refactor(density): tokens in styles.css atoms"
```

---

### Tasks 5–16: Component sweep (small → large)

For each task: **apply the Sweep Procedure** to all `*.ts` in the listed path(s), verify `node tools/density/check-tokens.mjs <path>` → `0 violations`, run `pnpm build`, then commit with the given message. Order is smallest-blast-radius first so the procedure is validated cheaply before the large files.

- [ ] **Task 5 — trivial dirs.** Paths: `src/app/shell`, `src/app/context-menu`, `src/app/loading`.
  Commit: `refactor(density): tokens in shell/context-menu/loading`
- [ ] **Task 6 — status bar.** Path: `src/app/status-bar`. (Use `--statusbar-h` for the 24px bar height.)
  Commit: `refactor(density): tokens in status-bar`
- [ ] **Task 7 — tweaks.** Path: `src/app/tweaks`.
  Commit: `refactor(density): tokens in tweaks panel`
- [ ] **Task 8 — top bar.** Path: `src/app/top-bar`. (Use `--topbar-h` for the 44px bar.)
  Commit: `refactor(density): tokens in top-bar`
- [ ] **Task 9 — shared atoms/components.** Path: `src/app/shared`. (Includes `rich-editor`, icon, status-dot, tool-badge, version-badge. Leave icon `[px]`/`size` inputs alone — they aren't px strings.)
  Commit: `refactor(density): tokens in shared components`
- [ ] **Task 10 — notifications.** Path: `src/app/notifications`.
  Commit: `refactor(density): tokens in notifications`
- [ ] **Task 11 — overview.** Path: `src/app/overview`. (Note `graph-view`/`timeline-view` may use px in SVG geometry attributes like `r=`/`cx=` — those are NOT CSS properties and are not flagged; leave them.)
  Commit: `refactor(density): tokens in overview`
- [ ] **Task 12 — sidebar.** Path: `src/app/sidebar` (agent-row, project-group, compact-rail, sidebar).
  Commit: `refactor(density): tokens in sidebar`
- [ ] **Task 13 — right panel.** Path: `src/app/right-panel` (git-tab, commit-feed, file-tree, right-panel).
  Commit: `refactor(density): tokens in right-panel`
- [ ] **Task 14 — backlog.** Path: `src/app/backlog` (ticket-page, ticket-card, backlog, agent-strip).
  Commit: `refactor(density): tokens in backlog`
- [ ] **Task 15 — dev tools.** Path: `src/app/dev-tools` (dev-panel, 233 literals).
  Commit: `refactor(density): tokens in dev-tools`
- [ ] **Task 16 — workspace.** Path: `src/app/workspace` (pane-node, diff-view, file-view, terminal).
  Commit: `refactor(density): tokens in workspace`

---

### Task 17: Sweep modals (largest)

Split because `settings-modal.component.ts` alone has ~330 literals.

- [ ] **Step 1: Sweep `settings-modal.component.ts`.** Apply the Sweep Procedure to `src/app/modals/settings-modal.component.ts`.
  Verify: `node tools/density/check-tokens.mjs src/app/modals/settings-modal.component.ts` → `0`.
  Commit: `refactor(density): tokens in settings-modal`
- [ ] **Step 2: Sweep remaining modals.** Apply to the rest of `src/app/modals` (spawn-modal, add-project-modal, runtime-row, and any others).
  Verify: `node tools/density/check-tokens.mjs src/app/modals` → `0`.
  Commit: `refactor(density): tokens in remaining modals`
- [ ] **Step 3: Full-repo verify.**
  Run: `node tools/density/check-tokens.mjs`
  Expected: `0 violations` (entire `src/styles.css` + `src/app`). If any remain, sweep them.

---

### Task 18: Density functional E2E (Playwright)

**Files:**
- Create: `playwright.config.ts`
- Create: `e2e/density.spec.ts`
- Modify: `package.json` (devDep + scripts)

- [ ] **Step 1: Install Playwright**

```bash
pnpm add -D @playwright/test
pnpm exec playwright install chromium
```

- [ ] **Step 2: Config (runs against `ng serve`)**

```ts
// playwright.config.ts
import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "e2e",
  timeout: 60_000,
  use: { baseURL: "http://localhost:4200" },
  webServer: {
    command: "pnpm start",
    url: "http://localhost:4200",
    reuseExistingServer: true,
    timeout: 120_000,
  },
});
```

- [ ] **Step 3: Spec — density drives computed spacing/height**

```ts
// e2e/density.spec.ts
import { test, expect } from "@playwright/test";

const probe = (density: string) =>
  `(() => {
    document.documentElement.setAttribute("data-density", "${density}");
    const el = document.createElement("div");
    el.style.padding = "var(--sp-4)";
    el.style.height = "var(--row-h)";
    document.body.appendChild(el);
    const cs = getComputedStyle(el);
    const r = { pad: parseFloat(cs.paddingTop), h: parseFloat(cs.height) };
    el.remove();
    return r;
  })()`;

test("density scales spacing and height", async ({ page }) => {
  await page.goto("/");
  const compact = await page.evaluate(probe("compact"));
  const regular = await page.evaluate(probe("regular"));
  const comfy = await page.evaluate(probe("comfy"));

  // --sp-4 = 8px * density ; --row-h = 30px * density
  expect(compact.pad).toBeCloseTo(8 * 0.85, 1);
  expect(regular.pad).toBeCloseTo(8 * 1.0, 1);
  expect(comfy.pad).toBeCloseTo(8 * 1.18, 1);

  expect(compact.h).toBeLessThan(regular.h);
  expect(regular.h).toBeLessThan(comfy.h);
});
```

- [ ] **Step 4: Run it**

Add to `package.json` `scripts`: `"e2e": "playwright test"`.
Run: `pnpm e2e`
Expected: PASS — proves density changes real computed layout, not just font size.

- [ ] **Step 5: Commit**

```bash
git add playwright.config.ts e2e/density.spec.ts package.json pnpm-lock.yaml
git commit -m "test(density): e2e proving density drives spacing + height"
```

---

### Task 19: Enforce scanner (acceptance gate) + CI

**Files:**
- Modify: `src/density-tokens.spec.ts` (add acceptance test)
- Create: `.github/workflows/test.yml`

- [ ] **Step 1: Add the acceptance assertion**

Append to `src/density-tokens.spec.ts`:

```ts
import { scanRepo } from "../tools/density/check-tokens.mjs";

describe("no hardcoded spacing/font literals", () => {
  it("src/app + styles.css are fully tokenized", () => {
    const { total, byFile } = scanRepo();
    expect(total, JSON.stringify(byFile, null, 2)).toBe(0);
  });
});
```

- [ ] **Step 2: Run full suite**

Run: `pnpm test`
Expected: PASS — including the acceptance test (0 violations).

- [ ] **Step 3: CI workflow**

```yaml
# .github/workflows/test.yml
name: test
on:
  push: { branches: [main] }
  pull_request:
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: pnpm/action-setup@v4
        with: { version: 11 }
      - uses: actions/setup-node@v4
        with: { node-version: 22, cache: pnpm }
      - run: pnpm install --frozen-lockfile
      - run: pnpm test
```

- [ ] **Step 4: Commit**

```bash
git add src/density-tokens.spec.ts .github/workflows/test.yml
git commit -m "test(density): enforce zero literals in CI"
```

---

### Task 20: Manual smoke + docs

- [ ] **Step 1: Manual smoke.** Run `pnpm dev` (tauri), open Tweaks panel, toggle Compact/Regular/Comfy. Confirm row heights, paddings, gaps, and control heights all shift live (not just font size). Spot-check sidebar, settings-modal, ticket-page.

- [ ] **Step 2: Update design-system doc.** In `docs/design-system.md`, update the density line (currently "`--row-h`, `--pad`, font sizes") to describe the `--density`-factor engine (spacing scale, control heights, type scale).
  Commit: `docs: document density token engine`

---

## Self-Review Notes

- **Spec coverage:** density factor + calc (Task 1) ✓; explicit type scale (Task 1) ✓; non-scaling borders/pill (Task 1) ✓; snapped scale (Reference tables) ✓; zero-literal sweep (Tasks 4–17) ✓; guardrail (Tasks 3, 19) ✓; 3-density verification (Task 18) ✓; manual smoke + docs (Task 20) ✓.
- **Scope refinement (deliberate):** scanner targets `padding/margin/gap/height/min-height/font-size`; `max-height`/`width` are out of scope (structural). Bespoke heights >34px use the documented `ALLOWLIST`. Recorded in the spec's "heights" intent.
- **Type consistency:** token names used in sweep tables (`--sp-1..9`, `--ctl-h-sm/--ctl-h/--ctl-h-lg`, `--row-h`, `--topbar-h`, `--statusbar-h`, `--fs-3xs..--fs-display`, `--hair`, `--r-pill`) all match the definitions in Task 1. Scanner exports (`scanText`, `scanRepo`, `defaultFiles`, `ALLOWLIST`) match their usages in Tasks 3 and 19.
