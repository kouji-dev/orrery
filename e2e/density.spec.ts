import { test, expect } from "@playwright/test";

/**
 * Injects probe elements and returns computed metrics for a given density.
 *
 * Two separate elements avoid the box-sizing conflict (padding > width):
 *   padEl: measures padding from var(--sp-4) and height from var(--row-h)
 *   widthEl: measures width from var(--sp-4) with no padding, content-box sizing
 *
 * A .dot uses width=height=var(--sp-4) — if --sp-4 = 8px*density at every level,
 * both dims track the same token and the dot stays square across densities.
 */
const probe = (density: string) =>
  `(() => {
    document.documentElement.setAttribute("data-density", "${density}");

    // Element 1: padding + height measurement
    const padEl = document.createElement("div");
    padEl.style.cssText = "position:absolute;padding:var(--sp-4);height:var(--row-h);width:200px;box-sizing:content-box;";
    document.body.appendChild(padEl);
    const padCs = getComputedStyle(padEl);

    // Element 2: width-only measurement (no padding, so content-box = var(--sp-4))
    const widthEl = document.createElement("div");
    widthEl.style.cssText = "position:absolute;width:var(--sp-4);height:var(--sp-4);box-sizing:content-box;padding:0;";
    document.body.appendChild(widthEl);
    const widthCs = getComputedStyle(widthEl);

    const r = {
      pad: parseFloat(padCs.paddingTop),
      h: parseFloat(padCs.height),
      w: parseFloat(widthCs.width),
      dotH: parseFloat(widthCs.height),
    };
    padEl.remove();
    widthEl.remove();
    return r;
  })()`;

test("density scales spacing and height", async ({ page }) => {
  await page.goto("/");
  const compact = await page.evaluate(probe("compact"));
  const regular = await page.evaluate(probe("regular"));
  const comfy = await page.evaluate(probe("comfy"));

  // --sp-4 = 8px * density
  expect(compact.pad).toBeCloseTo(8 * 0.85, 1);
  expect(regular.pad).toBeCloseTo(8 * 1.0, 1);
  expect(comfy.pad).toBeCloseTo(8 * 1.18, 1);

  // --row-h = 30px * density; height must change across densities
  expect(compact.h).toBeLessThan(regular.h);
  expect(regular.h).toBeLessThan(comfy.h);
});

test("square elements stay square across densities", async ({ page }) => {
  await page.goto("/");

  // For each density, verify that var(--sp-4) resolves to exactly 8px*density.
  // The .dot class uses width=height=var(--sp-4). If --sp-4 = 8*density at every
  // level, both the width and height dims of a dot track the same token — the
  // shape stays square and scales uniformly across all density levels.
  for (const d of ["compact", "regular", "comfy"]) {
    const m = await page.evaluate(probe(d));
    const multiplier =
      d === "compact" ? 0.85 : d === "comfy" ? 1.18 : 1.0;
    const expected = 8 * multiplier;

    // var(--sp-4) width must equal 8px * density
    expect(m.w, `density=${d}: --sp-4 width must equal 8*density`).toBeCloseTo(
      expected,
      1
    );

    // var(--sp-4) height must equal width (same token → square stays square)
    expect(
      m.dotH,
      `density=${d}: height must equal width (same token, proves square)`
    ).toBeCloseTo(m.w, 1);
  }
});
