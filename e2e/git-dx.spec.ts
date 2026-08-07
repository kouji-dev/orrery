import { expect, Page, test } from "@playwright/test";

/**
 * E2E for the A4.1 dual-path git controls with the cost kill switch OFF
 * (src/app/cost/cost-flags.ts): the split buttons and AI variants keep
 * working, but NO token/$ chrome renders anywhere — no row estimates, no
 * inline estimate on aiOnly controls, no "spends tokens" phrasing.
 *
 * Backend-free: the buttons render without a Tauri backend.
 */

const seedAgent = (id: string, name: string) => `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.agentActions["agentsStore"]["store"].upsert({
    id: "${id}", projectId: "p-e2e", tool: "claude", model: "m", name: "${name}",
    task: "", status: "idle", branch: "agent/${name}", worktree: "", base: "main",
    commits: 0, elapsed: 0, progress: 0, pending: [],
  });
})()`;

const ui = (expr: string) =>
  `window.ng.getComponent(document.querySelector("app-top-bar")).ui${expr}`;

async function openGitTab(page: Page, id: string, name: string): Promise<void> {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await page.evaluate(seedAgent(id, name));
  await page.evaluate(ui(`.openAgent("${id}")`));
  await page.locator("app-right-panel").getByRole("button", { name: "Git" }).click();
}

test("git tab renders dual-path split buttons for commit / push / rebase / merge", async ({ page }) => {
  await openGitTab(page, "e2e-g1", "e2e-git");
  const buttons = page.locator("app-git-action-button");
  await expect(buttons).toHaveCount(4);

  // native-default guards: nothing to commit / push on a fresh agent
  await expect(page.getByRole("button", { name: /^Commit all/ })).toBeDisabled();
  await expect(page.getByRole("button", { name: /^Push to origin/ })).toBeDisabled();
});

test("merge dropdown offers the AI variant with NO cost chrome (kill switch off)", async ({ page }) => {
  await openGitTab(page, "e2e-g2", "e2e-merge");
  const merge = page.locator("app-git-action-button", { hasText: "Merge" });

  await merge.locator(".caret").click();
  const menu = merge.locator(".menu");
  await expect(menu).toBeVisible();
  await expect(menu).toContainText("AI path");
  await expect(menu).not.toContainText("spends tokens");

  // the variant row still works, but carries no token/$ estimate
  const row = menu.locator(".btn.row", { hasText: "Merge with AI" });
  await expect(row).toBeVisible();
  await expect(row.locator(".row-est")).toHaveCount(0);

  await expect(menu).toContainText("native path is instant and deterministic");
  await expect(menu).not.toContainText("tokens");

  await page.keyboard.press("Escape");
  await expect(menu).toHaveCount(0);
});

test("rebase is aiOnly until A3.4: usable, with no inline estimate while costs are off", async ({ page }) => {
  await openGitTab(page, "e2e-g3", "e2e-rebase");
  const rebase = page.locator("app-git-action-button", { hasText: "Rebase onto" });
  await expect(rebase.locator(".btn.main")).toBeVisible();
  await expect(rebase.locator(".est-inline")).toHaveCount(0);
});
