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

  it("keeps pill radius non-scaling", () => {
    expect(css).toMatch(/--r-pill:\s*999px/);
  });

  it("redefines the type scale in all three densities", () => {
    for (const block of [/:root/, /\[data-density="compact"\]/, /\[data-density="comfy"\]/]) {
      const seg = css.slice(css.search(block));
      expect(seg).toMatch(/--fs-ui:\s*\d/);
    }
  });
});
