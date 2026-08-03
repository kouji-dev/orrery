import { expect, Page, test } from "@playwright/test";

/**
 * E2E for the A7.7 process tree + A0.7 emit telemetry surfaces in the dev
 * console, and the A0.6 status-bar memory split.
 *
 * Backend-free: `process_tree` / `telemetry_emits` invokes reject in the
 * browser build, so the specs assert the panel chrome (tabs, privacy footer,
 * empty states) rather than live data — the data path is covered by the Rust
 * unit tests.
 */

async function ready(page: Page): Promise<void> {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
}

test("dev console FAB opens the panel with Processes and Emits tabs", async ({ page }) => {
  await ready(page);
  await page.locator(".dvc-fab").click();
  const panel = page.locator(".dvcon");
  await expect(panel).toBeVisible();

  for (const label of ["Perf", "Agents", "Projects", "Resources", "Processes", "Emits"]) {
    await expect(panel.locator(".dvc-tab", { hasText: label })).toBeVisible();
  }
});

test("Processes tab activates without a backend (empty tree, no crash)", async ({ page }) => {
  await ready(page);
  await page.locator(".dvc-fab").click();
  const tab = page.locator(".dvc-tab", { hasText: "Processes" });
  await tab.click();
  await expect(tab).toHaveClass(/on/);
  await expect(page.locator(".dvcon")).toBeVisible(); // still alive after the failed poll
});

test("Emits tab carries the telemetry privacy contract in its footer", async ({ page }) => {
  await ready(page);
  await page.locator(".dvc-fab").click();
  await page.locator(".dvc-tab", { hasText: "Emits" }).click();
  // A0.7 non-negotiable, stated in the UI itself
  await expect(page.locator(".dvcon")).toContainText(
    "names · keys · byte counts only — payload contents are never written",
  );
});

test("status bar shows the A0.6 memory split base (Orrery + CPU)", async ({ page }) => {
  await ready(page);
  const bar = page.locator("app-status-bar");
  await expect(bar).toContainText("CPU");
  await expect(bar).toContainText("Orrery");
  // no backend → no agent samples → the agents figure stays hidden (honest split)
  await expect(bar).not.toContainText("· agents");
});
