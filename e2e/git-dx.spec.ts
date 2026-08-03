import { expect, Page, test } from "@playwright/test";

/**
 * E2E for the A4.1 dual-path git controls: every AI variant carries its
 * token/$ estimate ON the row (never hover-only), the dropdown discloses
 * "AI path · spends tokens", and aiOnly controls (rebase, until A3.4 lands
 * a native path) show the estimate inline on the button itself.
 *
 * Backend-free: estimates come from the frontend EstimateService heuristic +
 * the default rate table, so they render without a Tauri backend.
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

test("merge dropdown discloses the AI price on the row before it can run", async ({ page }) => {
  await openGitTab(page, "e2e-g2", "e2e-merge");
  const merge = page.locator("app-git-action-button", { hasText: "Merge" });

  await merge.locator(".caret").click();
  const menu = merge.locator(".menu");
  await expect(menu).toBeVisible();
  await expect(menu).toContainText("AI path · spends tokens");

  // A4.1: the estimate is ON the row — tokens and dollars visible without hover
  const row = menu.locator(".btn.row", { hasText: "Merge with AI" });
  await expect(row).toBeVisible();
  await expect(row.locator(".row-est")).toContainText("tok");
  await expect(row.locator(".row-est")).toContainText("$");

  await expect(menu).toContainText("native path is instant, deterministic and costs 0 tokens");

  await page.keyboard.press("Escape");
  await expect(menu).toHaveCount(0);
});

test("rebase is aiOnly until A3.4: estimate shown inline on the primary button", async ({ page }) => {
  await openGitTab(page, "e2e-g3", "e2e-rebase");
  const rebase = page.locator("app-git-action-button", { hasText: "Rebase onto" });
  const inline = rebase.locator(".est-inline");
  await expect(inline).toBeVisible();
  await expect(inline).toContainText("tok");
  await expect(inline).toContainText("$");
});
