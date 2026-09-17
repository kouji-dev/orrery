import { expect, Page, test } from "@playwright/test";

/**
 * Design orrery-v2 sb-dev-slot: the floating FAB rail is gone — Tweaks and the
 * Dev console launch from bordered pill chips in the status bar's right
 * cluster, and each panel pops up ABOVE the footer instead of floating over
 * the workspace content. Backend-free: chips and panels render without Tauri.
 */

async function ready(page: Page): Promise<void> {
  await page.goto("/");
  await page.waitForSelector("app-status-bar");
}

test("the FAB rail is gone; Tweaks and Dev live in the status bar as pill chips", async ({ page }) => {
  await ready(page);
  await expect(page.locator(".fab")).toHaveCount(0);
  const tweaks = page.locator(".sb-chip", { hasText: "Tweaks" });
  const dev = page.locator(".sb-chip", { hasText: "Dev" });
  await expect(tweaks).toBeVisible();
  await expect(dev).toBeVisible();
  // the chips read as controls: bordered pill, not quiet footer text
  await expect(tweaks).toHaveCSS("border-radius", "999px");
});

test("the Tweaks chip toggles its panel; the chip's own click never insta-reopens", async ({ page }) => {
  await ready(page);
  const chip = page.locator(".sb-chip", { hasText: "Tweaks" });
  await chip.click();
  await expect(page.locator(".tweak-panel")).toBeVisible();
  await expect(chip).toHaveClass(/on/);
  // toggling from the chip must CLOSE (the light-dismiss ignores the chip's
  // mousedown — without that the dismiss+click pair reopened it immediately)
  await chip.click();
  await expect(page.locator(".tweak-panel")).toHaveCount(0);
});

test("the Dev chip toggles the console, anchored above the status bar", async ({ page }) => {
  await ready(page);
  const chip = page.locator(".sb-chip", { hasText: "Dev" });
  await chip.click();
  const panel = page.locator(".dvcon");
  await expect(panel).toBeVisible();
  // pops up from the footer: the panel's bottom edge sits above the status bar
  const [panelBox, footBox] = await Promise.all([
    panel.boundingBox(),
    page.locator("app-status-bar footer, app-status-bar > *").first().boundingBox(),
  ]);
  expect(panelBox!.y + panelBox!.height).toBeLessThanOrEqual(footBox!.y + 1);
  await chip.click();
  await expect(panel).toHaveCount(0);
});
