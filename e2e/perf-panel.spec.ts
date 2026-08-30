import { expect, Page, test } from "@playwright/test";

/**
 * E2E for the merged Resources tab (gauges + A7.7 process tree under one
 * "Orrery App" root) + A0.7 emit telemetry surfaces in the dev console, and
 * the agents-only status-bar readout.
 *
 * Backend-free: `process_tree` / `telemetry_emits` invokes reject in the
 * browser build, so the specs assert the panel chrome (tabs, privacy footer,
 * empty states) rather than live data — the data path is covered by the Rust
 * unit tests and the mergeRoots vitest specs.
 */

async function ready(page: Page): Promise<void> {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
}

test("the status-bar Dev chip opens the panel with the merged tab set (no Processes tab)", async ({ page }) => {
  await ready(page);
  await page.locator(".sb-chip", { hasText: "Dev" }).click();
  const panel = page.locator(".dvcon");
  await expect(panel).toBeVisible();

  for (const label of ["Perf", "Agents", "Projects", "Resources", "Emits"]) {
    await expect(panel.locator(".dvc-tab", { hasText: label })).toBeVisible();
  }
  await expect(panel.locator(".dvc-tab", { hasText: "Processes" })).toHaveCount(0);
});

test("Resources tab activates without a backend (empty tree state, no crash)", async ({ page }) => {
  await ready(page);
  await page.locator(".sb-chip", { hasText: "Dev" }).click();
  const tab = page.locator(".dvc-tab", { hasText: "Resources" });
  await tab.click();
  // the dev-console tabs are <kj-button [kjPressed]> now — active state is
  // aria-pressed on the inner button, not an `.on` class on the host
  await expect(tab.locator("button")).toHaveAttribute("aria-pressed", "true");
  await expect(page.locator(".dvcon")).toBeVisible(); // still alive after the failed poll
  await expect(page.locator(".dvcon")).toContainText("No process tree yet");
});

test("Emits tab carries the telemetry privacy contract in its footer", async ({ page }) => {
  await ready(page);
  await page.locator(".sb-chip", { hasText: "Dev" }).click();
  await page.locator(".dvc-tab", { hasText: "Emits" }).click();
  // A0.7 non-negotiable, stated in the UI itself
  await expect(page.locator(".dvcon")).toContainText(
    "names · keys · byte counts only — payload contents are never written",
  );
});

test("status bar readout is agents-only and stays hidden with no agent samples", async ({ page }) => {
  await ready(page);
  const bar = page.locator("app-status-bar");
  // the deep-link gauge button is always there…
  await expect(bar.locator(".gauge")).toBeVisible();
  // …but with no backend (no agent subtree samples) the readout text is hidden,
  // and the old CPU/Orrery split is gone for good
  await expect(bar.locator(".gauge")).not.toContainText("agents");
  await expect(bar).not.toContainText("Orrery");
  await expect(bar).not.toContainText("CPU");
});
