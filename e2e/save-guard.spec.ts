import { expect, Page, test } from "@playwright/test";

/**
 * E2E for the unsaved-changes guards (B1.1 follow-ups):
 *  - closing a WORKSPACE tab with a dirty buffer raises the Save all /
 *    Discard / Cancel dialog instead of silently dropping edits;
 *  - the file-tab strip's right-click menu (Close / Close left / Close right /
 *    Close All) with icon-led items.
 *
 * Backend-free: content comes from a stubbed AgentDiff; dirty state is seeded
 * straight into the EditsStore through a live pane-node instance.
 */

const seedAgent = (id: string, name: string) => `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.agentActions["agentsStore"]["store"].upsert({
    id: "${id}", projectId: "p-e2e", tool: "claude", model: "m", name: "${name}",
    task: "", status: "idle", branch: "agent/${name}", worktree: "", base: "main",
    commits: 0, elapsed: 0, progress: 0, pending: [],
  });
})()`;

const stubDiff = () => `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.agentActions["agentsStore"].diff = () => Promise.resolve({ old: "", new: "hello\\n" });
})()`;

const dirty = (id: string, path: string) => `(() => {
  const pn = window.ng.getComponent(document.querySelector("app-pane-node"));
  pn["edits"].open("${id}", "${path}", "hello\\n");
  pn["edits"].update("${id}", "${path}", "hello, edited\\n");
})()`;

const ui = (expr: string) =>
  `window.ng.getComponent(document.querySelector("app-top-bar")).ui${expr}`;

async function openFiles(page: Page, id: string, paths: string[]): Promise<void> {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await page.evaluate(seedAgent(id, "e2e-guard"));
  await page.evaluate(stubDiff());
  for (const p of paths) await page.evaluate(ui(`.openFileInWorkspace("${id}", "${p}")`));
  await expect(page.locator(".file-tab")).toHaveCount(paths.length);
}

test("closing a workspace tab with unsaved edits raises the guard dialog", async ({ page }) => {
  await openFiles(page, "e2e-sg1", ["src/demo.ts"]);
  await page.evaluate(dirty("e2e-sg1", "src/demo.ts"));
  await expect(page.locator(".file-tab.dirty")).toHaveCount(1);

  // the workspace tab's × → dialog, not a silent close
  await page.locator("app-top-bar .tab-x").click();
  const card = page.locator(".tcg-card");
  await expect(card).toBeVisible();
  await expect(card).toContainText("src/demo.ts");

  await card.getByRole("button", { name: "Cancel" }).click();
  await expect(card).toHaveCount(0);
  await expect(page.locator("app-top-bar .tab-x")).toHaveCount(1); // tab survived

  await page.locator("app-top-bar .tab-x").click();
  await page.locator(".tcg-card").getByRole("button", { name: "Discard all" }).click();
  await expect(page.locator("app-top-bar .tab-x")).toHaveCount(0); // tab closed
});

test("Autosave is an opt-in setting under Agent defaults", async ({ page }) => {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await page.keyboard.press("Control+Shift+P");
  await page.keyboard.type("settings");
  await page.keyboard.press("Enter");
  await page.getByRole("button", { name: "Agent defaults" }).click();

  const row = page.locator("app-set-row", { hasText: "Autosave edits" });
  await expect(row).toBeVisible();
  await expect(row).toContainText("Ctrl+S still saves on demand");

  await row.locator("app-set-tgl").click();
  const on = await page.evaluate(
    `window.ng.getComponent(document.querySelector("app-top-bar")).settings.settings().autosave`,
  );
  expect(on).toBe(true);
});

test("file-tab right-click menu: icon-led items, Close All to the Right works", async ({ page }) => {
  await openFiles(page, "e2e-sg2", ["src/a.ts", "src/b.ts", "src/c.ts"]);

  await page.locator(".file-tab", { hasText: "a.ts" }).click({ button: "right" });
  const menu = page.locator(".menu-panel");
  await expect(menu).toBeVisible();
  const items = menu.locator(".menu-item");
  await expect(items).toHaveCount(4);
  await expect(items.nth(0)).toContainText("Close");
  await expect(items.nth(1)).toBeDisabled(); // nothing to the left of the first tab
  // every item leads with an icon
  await expect(menu.locator(".menu-item app-icon")).toHaveCount(4);

  await items.nth(2).click(); // Close All to the Right
  await expect(page.locator(".file-tab")).toHaveCount(1);
  await expect(page.locator(".file-tab")).toContainText("a.ts");
});
