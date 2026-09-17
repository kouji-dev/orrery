import { expect, Page, test } from "@playwright/test";

/**
 * E2E for the graphite design system (design: app.html) after the preset and
 * accent-palette tweaks were removed: graphite IS the theme. Verifies the
 * fixed surface ramp in dark and light, and that the accent is the fixed
 * design token (no palette override is ever written onto <html>).
 */

async function ready(page: Page): Promise<void> {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
}

const readTokens = `(() => {
  const r = document.documentElement;
  const cs = getComputedStyle(r);
  return {
    theme: r.getAttribute("data-theme"),
    bg: cs.getPropertyValue("--bg").trim(),
    accent: cs.getPropertyValue("--accent").trim(),
    inlineAccent: r.style.getPropertyValue("--accent"),
    font: getComputedStyle(document.body).fontFamily,
  };
})()`;

test("graphite ramp + fixed accent in dark", async ({ page }) => {
  await ready(page);
  const t = await page.evaluate(readTokens);
  expect(t.theme).toBe("orrery");
  expect(t.bg).toBe("#121315"); // graphite, not the old nebula #090a0f
  expect(t.accent).toBe("#a855f7"); // the design token, straight from CSS
  expect(t.inlineAccent).toBe(""); // no tweak-service override on <html>
  expect(t.font).toContain("Inter"); // chrome is Inter, mono is code-only
});

test("light mode uses the paper ramp with its own accent", async ({ page }) => {
  await ready(page);
  await page.locator(".sb-chip", { hasText: "Tweaks" }).click();
  await page.locator(".seg button", { hasText: "light" }).click();
  await expect
    .poll(() => page.evaluate(() => document.documentElement.getAttribute("data-theme")))
    .toBe("orrery-light");
  const t = await page.evaluate(readTokens);
  expect(t.bg).toBe("#dedede"); // paper — the editor stays the one bright surface
  expect(t.accent).toBe("#8d4dcc"); // light re-tunes the accent for contrast
});
