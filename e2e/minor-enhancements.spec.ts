import { expect, Page, test } from "@playwright/test";

/**
 * E2E for the projects-folder setting UI and the delete-worktree confirm flow.
 *
 * The browser build has no Tauri backend (every invoke rejects), so agents are
 * seeded straight into the entity store through Angular's dev-mode `window.ng`
 * component API. That also makes the backend-failure branch of the delete flow
 * the reachable one: confirming surfaces the "delete failed" toast.
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

async function agentCount(page: Page): Promise<number> {
  return page.evaluate(
    `window.ng.getComponent(document.querySelector("app-top-bar")).runtime.agents().length`,
  );
}

/** The shell renders after a startup loading screen — wait for it. */
async function ready(page: Page): Promise<void> {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
}

test("settings has a Projects folder row defaulting to OS default", async ({ page }) => {
  await ready(page);
  await page.evaluate(
    `window.ng.getComponent(document.querySelector("app-shell")).settings.openModal("agent")`,
  );
  const row = page.locator("app-set-row", { hasText: "Projects folder" });
  await expect(row).toBeVisible();
  await expect(row).toContainText("OS default");
  await expect(row.getByRole("button", { name: "Browse" })).toBeVisible();
});

test("delete worktree asks for confirmation and cancel keeps everything", async ({ page }) => {
  await ready(page);
  await page.evaluate(seedAgent("e2e-a1", "e2e-agent"));
  await page.evaluate(ui(`.openAgent("e2e-a1")`));
  await page.evaluate(ui(`.openDeleteWorktree("e2e-a1")`));

  const modal = page.locator("app-delete-worktree-modal");
  await expect(modal).toContainText("Delete worktree?");
  await expect(modal).toContainText("e2e-agent");
  await expect(modal).toContainText("agent/e2e-agent");

  await modal.getByRole("button", { name: "Cancel" }).click();
  await expect(modal).toHaveCount(0);
  expect(await agentCount(page)).toBe(1); // nothing deleted
  // its tab is still open (an agent tab exists beside orchestrator/backlog)
  expect(await page.evaluate(ui(`.tabs().length`))).toBe(3);
});

test("confirming delete closes the agent's tab and runs the remove", async ({ page }) => {
  await ready(page);
  await page.evaluate(seedAgent("e2e-a2", "e2e-doomed"));
  await page.evaluate(ui(`.openAgent("e2e-a2")`));
  await page.evaluate(ui(`.openDeleteWorktree("e2e-a2")`));

  const modal = page.locator("app-delete-worktree-modal");
  await modal.getByRole("button", { name: "Delete worktree" }).click();
  await expect(modal).toHaveCount(0);

  // tabs for the agent close before the backend call
  expect(await page.evaluate(ui(`.tabs().length`))).toBe(2);
  // no backend in the browser build → the remove rejects and the failure
  // branch keeps the agent listed + flashes the error toast
  await expect(page.getByText("delete failed")).toBeVisible();
  expect(await agentCount(page)).toBe(1);
});

test("confirm modal for an unknown agent closes itself", async ({ page }) => {
  await ready(page);
  await page.evaluate(ui(`.openDeleteWorktree("no-such-agent")`));
  await expect(page.locator("app-delete-worktree-modal")).toHaveCount(0);
});
