import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, it, expect } from "vitest";
import { scanRepo } from "../tools/density/check-tokens.mjs";

const css = readFileSync(resolve(__dirname, "styles.css"), "utf8");

describe("density token engine", () => {
  it("defines both knobs in :root and both density blocks", () => {
    expect(css).toMatch(/:root[^}]*--density:\s*1\b/s);
    expect(css).toMatch(/:root[^}]*--fs-scale:\s*1\b/s);
    expect(css).toMatch(/\[data-density="compact"\][^}]*--density:\s*0\.85/s);
    expect(css).toMatch(/\[data-density="compact"\][^}]*--fs-scale:\s*0\.875/s);
    expect(css).toMatch(/\[data-density="comfy"\][^}]*--density:\s*1\.18/s);
    expect(css).toMatch(/\[data-density="comfy"\][^}]*--fs-scale:\s*1\.125/s);
  });

  it("derives every spacing + height token from --density, snapped to whole px", () => {
    for (const t of ["--sp-1", "--sp-4", "--sp-9", "--ctl-h", "--row-h", "--topbar-h", "--statusbar-h"]) {
      const re = new RegExp(t + String.raw`:\s*round\(calc\([^;]*var\(--density\)[^;]*\),\s*1px\)`);
      expect(css, `${t} must be round(calc(... var(--density)), 1px)`).toMatch(re);
    }
  });

  it("derives the type ramp from --fs-scale, snapped to half px", () => {
    for (const t of ["--fs-3xs", "--fs-2xs", "--fs-xs", "--fs-sm", "--fs-md", "--fs-lg", "--fs-xl"]) {
      const re = new RegExp(t + String.raw`:\s*round\(calc\([^;]*var\(--fs-scale\)[^;]*\),\s*0\.5px\)`);
      expect(css, `${t} must be round(calc(... var(--fs-scale)), 0.5px)`).toMatch(re);
    }
  });

  it("anchors the ramp on --fs-sm at 16px, with --fs-ui/--fs-tree as aliases", () => {
    // One reference size, three names: --fs-sm is what 165 call sites use, and
    // --fs-ui (the <body> default) / --fs-tree must not drift away from it.
    expect(css).toMatch(/--fs-sm:\s*round\(calc\(16px \* var\(--fs-scale\)\)/);
    expect(css).toMatch(/--fs-ui:\s*var\(--fs-sm\)/);
    expect(css).toMatch(/--fs-tree:\s*var\(--fs-sm\)/);
  });

  it("exposes code-surface metrics for Monaco + xterm", () => {
    expect(css).toMatch(/--fs-code:\s*round\(calc\([^;]*var\(--fs-scale\)/);
    expect(css).toMatch(/--lh-code:\s*round\(calc\([^;]*var\(--fs-scale\)/);
  });

  it("defines semantic type roles on top of the ramp", () => {
    // Call sites should name a ROLE, not a step. The rebase is what proved this
    // necessary: --fs-sm meant "small" at 11.5px and now means "primary" at
    // 16px, so every call site that picked it under the old meaning silently
    // changed intent. Roles cannot drift that way.
    for (const role of ["--fs-body", "--fs-label", "--fs-meta", "--fs-badge"]) {
      expect(css, `${role} must be defined`).toMatch(
        new RegExp(role + String.raw`:\s*var\(--fs-`),
      );
    }
  });

  it("sizes icons in em so they track adjacent type", () => {
    // The 129 hardcoded [px] bindings these replaced were frozen snapshots of a
    // ~1em relationship. px froze while the ramp grew 1.28-1.37x; em does not.
    for (const step of ["xs", "sm", "md", "lg", "xl"]) {
      expect(css, `--ico-${step} must be em-based`).toMatch(
        new RegExp(String.raw`--ico-` + step + String.raw`:\s*[\d.]+em`),
      );
    }
    expect(css).toMatch(/app-icon svg\s*\{[^}]*var\(--ico,/);
  });

  it("keeps pill radius non-scaling", () => {
    expect(css).toMatch(/--r-pill:\s*999px/);
  });
});

describe("no hardcoded spacing/font literals", () => {
  it("src/app + styles.css are fully tokenized", () => {
    const { total, byFile } = scanRepo();
    expect(total, JSON.stringify(byFile, null, 2)).toBe(0);
  });
});
