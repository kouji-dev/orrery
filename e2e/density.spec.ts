import { test, expect } from "@playwright/test";

/**
 * Injects probe elements and returns computed metrics for a given density.
 *
 * Two separate elements avoid the box-sizing conflict (padding > width):
 *   padEl: measures padding from var(--sp-4) and height from var(--row-h)
 *   widthEl: measures width from var(--sp-4) with no padding, content-box sizing
 *
 * A .dot uses width=height=var(--sp-4) — if --sp-4 = round(8px*density) at every
 * level, both dims track the same token and the dot stays square across densities.
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

  // --sp-4 = round(8px * density) — whole pixels, never 6.8/9.44
  expect(compact.pad).toBe(Math.round(8 * 0.85)); // 7
  expect(regular.pad).toBe(8);
  expect(comfy.pad).toBe(Math.round(8 * 1.18)); // 9

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
    const expected = Math.round(8 * multiplier);

    // var(--sp-4) width must equal round(8px * density)
    expect(m.w, `density=${d}: --sp-4 width must equal round(8*density)`).toBe(
      expected
    );

    // var(--sp-4) height must equal width (same token → square stays square)
    expect(
      m.dotH,
      `density=${d}: height must equal width (same token, proves square)`
    ).toBeCloseTo(m.w, 1);
  }
});

/** Reads the resolved value of a set of custom properties at a given density. */
const tokens = (density: string, names: string[]) =>
  `(() => {
    document.documentElement.setAttribute("data-density", "${density}");
    const cs = getComputedStyle(document.documentElement);
    const out = {};
    for (const n of ${JSON.stringify(names)}) out[n] = parseFloat(cs.getPropertyValue(n));
    return out;
  })()`;

const SPACING = [
  "--sp-1","--sp-2","--sp-3","--sp-4","--sp-5","--sp-6",
  "--sp-7","--sp-8","--sp-9","--sp-10","--sp-11",
  "--ctl-h-sm","--ctl-h","--ctl-h-lg","--row-h","--topbar-h","--statusbar-h",
];

test("every spacing + height token lands on a whole pixel", async ({ page }) => {
  await page.goto("/");
  // This is what round() buys us. Before it, compact --row-h was 25.5px and
  // --ctl-h 23.8px: fractional heights blur hairlines and desync the rows from
  // the SVG/canvas surfaces (commit graph lanes) that mirror them in JS.
  for (const d of ["compact", "regular", "comfy"]) {
    const t = await page.evaluate(tokens(d, SPACING));
    for (const name of SPACING) {
      expect(t[name], `density=${d}: ${name} = ${t[name]} must be an integer`).toBe(
        Math.round(t[name]),
      );
      expect(t[name], `density=${d}: ${name} must be positive`).toBeGreaterThan(0);
    }
  }
});

test("--fs-sm is the 16px reference, with --fs-ui/--fs-tree aliased to it", async ({ page }) => {
  await page.goto("/");
  const names = ["--fs-sm", "--fs-ui", "--fs-tree"];

  const regular = await page.evaluate(tokens("regular", names));
  expect(regular["--fs-sm"]).toBe(16);
  // Aliases, not near-duplicates — they must not be able to drift apart.
  expect(regular["--fs-ui"]).toBe(regular["--fs-sm"]);
  expect(regular["--fs-tree"]).toBe(regular["--fs-sm"]);

  const compact = await page.evaluate(tokens("compact", names));
  const comfy = await page.evaluate(tokens("comfy", names));
  expect(compact["--fs-sm"]).toBe(14);   // 16 * 0.875
  expect(comfy["--fs-sm"]).toBe(18);     // 16 * 1.125
  for (const d of [compact, comfy]) {
    expect(d["--fs-ui"]).toBe(d["--fs-sm"]);
    expect(d["--fs-tree"]).toBe(d["--fs-sm"]);
  }
});

test("the type ramp stays ordered and above a 10px floor at every density", async ({ page }) => {
  await page.goto("/");
  // Ascending order. The old ramp put --fs-sm (11.5) BELOW --fs-ui (12.5) while
  // 165 call sites used sm — the ordering is what keeps the names meaningful.
  const ramp = ["--fs-3xs","--fs-2xs","--fs-xs","--fs-sm","--fs-md","--fs-lg","--fs-xl","--fs-2xl"];
  for (const d of ["compact", "regular", "comfy"]) {
    const t = await page.evaluate(tokens(d, ramp));
    for (let i = 1; i < ramp.length; i++) {
      expect(
        t[ramp[i]],
        `density=${d}: ${ramp[i]} (${t[ramp[i]]}) must be >= ${ramp[i - 1]} (${t[ramp[i - 1]]})`,
      ).toBeGreaterThanOrEqual(t[ramp[i - 1]]);
    }
    // Nothing in the app may render below 10px, even at compact.
    expect(t["--fs-3xs"], `density=${d}: smallest type must clear 10px`).toBeGreaterThanOrEqual(10);
  }
});

test("code surfaces expose density-scaled metrics to Monaco and xterm", async ({ page }) => {
  await page.goto("/");
  const names = ["--fs-code", "--lh-code"];
  const compact = await page.evaluate(tokens("compact", names));
  const regular = await page.evaluate(tokens("regular", names));
  const comfy = await page.evaluate(tokens("comfy", names));

  // Monaco and xterm read these as JS numbers — they cannot inherit font-size,
  // so if these stop scaling the editor and terminal silently stop tracking density.
  for (const t of [compact, regular, comfy]) {
    expect(t["--fs-code"]).toBeGreaterThan(0);
    // Leading must clear the glyph size or lines overlap.
    expect(t["--lh-code"]).toBeGreaterThan(t["--fs-code"]);
    expect(t["--lh-code"], "line height must sit on the device grid").toBe(
      Math.round(t["--lh-code"]),
    );
  }
  expect(compact["--fs-code"]).toBeLessThan(regular["--fs-code"]);
  expect(regular["--fs-code"]).toBeLessThan(comfy["--fs-code"]);
});

/**
 * kouji-ui integration. Orrery consumes @kouji-ui/themes for the token layer,
 * so these assert the seam between the two systems rather than either alone.
 *
 * Note the two read strategies. --kj-density / --kj-type-scale / --kj-ctl-h-* /
 * --kj-row-h are @property-registered, so they have a computed value and can be
 * read straight off the element. --kj-space-* / --kj-text-* are deliberately NOT
 * registered (registration makes invalid-at-computed-value-time apply, which
 * would silently discard a consumer override), so getPropertyValue hands back
 * their raw token stream and they must be measured through a real property on a
 * probe element instead.
 */
test("kouji tokens resolve and track Orrery's density switch", async ({ page }) => {
  await page.goto("/");

  const kj = (density: string) =>
    `(() => {
      const r = document.documentElement;
      r.setAttribute("data-density", "${density}");
      const cs = getComputedStyle(r);
      const n = (p) => parseFloat(cs.getPropertyValue(p));

      // Unregistered tokens have no computed value — measure them as real
      // lengths on a probe rather than parsing the token stream.
      const probe = document.createElement("div");
      probe.style.cssText =
        "position:absolute;visibility:hidden;box-sizing:content-box;padding:0;" +
        "margin-left:var(--kj-space-md);margin-right:var(--kj-space-6);" +
        "font-size:var(--kj-text-base);width:var(--kj-text-code)";
      document.body.appendChild(probe);
      const ps = getComputedStyle(probe);
      const measured = {
        spaceMd: parseFloat(ps.marginLeft),
        space6: parseFloat(ps.marginRight),
        textBase: parseFloat(ps.fontSize),
        textCode: parseFloat(ps.width),
      };
      probe.remove();

      return {
        density: n("--kj-density"),
        typeScale: n("--kj-type-scale"),
        ctlMd: n("--kj-ctl-h-md"),
        rowH: n("--kj-row-h"),
        bgBody: cs.getPropertyValue("--kj-bg-body").trim(),
        ...measured,
      };
    })()`;

  const compact = await page.evaluate(kj("compact"));
  const regular = await page.evaluate(kj("regular"));
  const comfy = await page.evaluate(kj("comfy"));

  // The theme applied at all, i.e. [data-theme="orrery"] matched.
  expect(regular.bgBody).toBe("#121315");

  // Orrery emits compact | regular | comfy. `regular` is NOT one of kouji's
  // selector values (`standard` is) — it deliberately matches nothing and falls
  // through to the :root defaults, which are exactly density 1.
  expect(regular.density).toBe(1);
  expect(regular.typeScale).toBe(1);
  expect(compact.density).toBeCloseTo(0.85, 5);
  expect(comfy.density).toBeCloseTo(1.18, 5);

  for (const [name, d] of [["compact", compact], ["regular", regular], ["comfy", comfy]] as const) {
    // Guard against a vacuous pass: NaN === NaN under Object.is, so an
    // unresolved token would otherwise satisfy the equality checks below.
    for (const [k, v] of Object.entries(d)) {
      if (k === "bgBody") continue;
      expect(Number.isFinite(v as number), `${name}: ${k} must resolve to a number`).toBe(true);
    }
    // The t-shirt names re-point at the numeric ladder, so they must agree.
    expect(d.spaceMd, `${name}: --kj-space-md must equal --kj-space-6`).toBe(d.space6);
    expect(d.rowH).toBe(d.ctlMd);
    // Every derived length lands on a whole pixel.
    expect(d.ctlMd).toBe(Math.round(d.ctlMd));
    expect(d.spaceMd).toBe(Math.round(d.spaceMd));
  }

  // Unchanged at density 1 — the re-pointing must not have moved any value.
  expect(regular.spaceMd).toBe(12);
  expect(regular.ctlMd).toBe(36);
  expect(regular.textBase).toBe(16);

  // …and everything scales in the right direction.
  expect(compact.ctlMd).toBeLessThan(regular.ctlMd);
  expect(regular.ctlMd).toBeLessThan(comfy.ctlMd);
  expect(compact.textCode).toBeLessThan(comfy.textCode);
});
