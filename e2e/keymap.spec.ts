import { expect, Page, test } from "@playwright/test";

/**
 * E2E for the Keymap settings section (B6.2): commands listed by group,
 * click-to-record rebinding, the reset pill, and the rebind actually routing
 * through the live dispatcher.
 *
 * Backend-free: settings persist optimistically (the settings_set invoke
 * rejects and only flashes), so all assertions run against live state.
 */

async function openKeymap(page: Page): Promise<void> {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await page.keyboard.press("Control+Shift+P");
  // wait for the palette to actually mount before typing — a blind type/Enter
  // races the first keystroke against bootstrap and silently runs nothing
  await expect(page.locator("app-command-palette input")).toBeVisible();
  await page.keyboard.type("settings");
  await page.keyboard.press("Enter");
  // the left nav is a <kj-list> of <kj-list-item>, not buttons
  await page.locator(".set-nav-item", { hasText: "Keymap" }).click();
}

test("keymap lists commands by group with their default bindings", async ({ page }) => {
  await openKeymap(page);
  // group headers mirror the registry groups
  await expect(page.getByText("Navigate", { exact: true })).toBeVisible();
  await expect(page.getByText("Workspace", { exact: true })).toBeVisible();
  // a known default binding renders on its row
  const saveRow = page.locator("app-set-row", { hasText: "Save All" });
  await expect(saveRow.locator(".set-kbd")).toHaveText("Ctrl+S");
});

test("recording a chord rebinds the command end-to-end", async ({ page }) => {
  await openKeymap(page);
  const saveRow = page.locator("app-set-row", { hasText: "Save All" });
  await saveRow.locator(".set-kbd").click();
  await expect(saveRow.locator(".set-kbd")).toHaveText("recording…");

  await page.keyboard.press("Control+Alt+s");
  await expect(saveRow.locator(".set-kbd")).toHaveText("Ctrl+Alt+S");

  // close the modal, then prove the DISPATCHER honors the new binding: with no
  // file open Save All is disabled, so the chord flashes "not available"
  await page.keyboard.press("Escape");
  await expect(page.locator("app-set-row").first()).toHaveCount(0);
  await page.keyboard.press("Control+Alt+s");
  await expect(page.getByText("Save All — not available here")).toBeVisible();
});

test("Escape cancels recording; the reset pill restores the default", async ({ page }) => {
  await openKeymap(page);
  const saveRow = page.locator("app-set-row", { hasText: "Save All" });

  await saveRow.locator(".set-kbd").click();
  await page.keyboard.press("Escape");
  await expect(saveRow.locator(".set-kbd")).toHaveText("Ctrl+S"); // unchanged
  await expect(page.locator("app-set-row").first()).toBeVisible(); // modal survived the Esc

  // rebind, then reset the override away
  await saveRow.locator(".set-kbd").click();
  await page.keyboard.press("Control+Alt+s");
  await expect(saveRow.locator(".set-kbd")).toHaveText("Ctrl+Alt+S");
  await saveRow.getByTitle(/reset/i).click();
  await expect(saveRow.locator(".set-kbd")).toHaveText("Ctrl+S");
});
