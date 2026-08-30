import { expect, test } from "@playwright/test";

/**
 * E2E for the v2 collapsed graph strip: history's one home, one click away.
 * The ~28px strip renders at the bottom of the center column whenever the
 * bottom tool window is closed; clicking it opens the Git Graph panel in its
 * place (and closing the dock brings it back).
 *
 * Backend-free.
 */

const seedAgent = (id: string, name: string) => `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.agentActions["agentsStore"]["store"].upsert({
    id: "${id}", projectId: "p-e2e", tool: "claude", model: "m", name: "${name}",
    task: "", status: "idle", branch: "agent/${name}", worktree: "", base: "main",
    commits: 3, elapsed: 0, progress: 0, pending: [],
  });
})()`;

const ui = (expr: string) =>
  `window.ng.getComponent(document.querySelector("app-top-bar")).ui${expr}`;

test("the strip shows the scoped branch and commits-ahead count", async ({ page }) => {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await page.evaluate(seedAgent("e2e-gs1", "strip"));
  await page.evaluate(ui(`.openAgent("e2e-gs1")`));
  const strip = page.locator("app-graph-strip");
  await expect(strip).toBeVisible();
  await expect(strip).toContainText("agent/strip");
  await expect(strip).toContainText("3 commits ahead");
  await expect(strip).toContainText("expand graph");
});

test("clicking the strip swaps it for the graph panel; closing brings it back", async ({ page }) => {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await page.evaluate(seedAgent("e2e-gs2", "swap"));
  await page.evaluate(ui(`.openAgent("e2e-gs2")`));

  await page.locator("app-graph-strip").click();
  await expect(page.locator("app-tool-window")).toBeVisible();
  await expect(page.locator("app-graph-strip")).toHaveCount(0);
  await expect(page.locator("app-commit-graph-panel")).toBeVisible();

  await page.locator("app-tool-window button[title='Hide tool window']").click();
  await expect(page.locator("app-tool-window")).toHaveCount(0);
  await expect(page.locator("app-graph-strip")).toBeVisible();
});

test("with no agent scoped, the strip still renders and invites scoping", async ({ page }) => {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  const strip = page.locator("app-graph-strip");
  await expect(strip).toBeVisible();
  await expect(strip).toContainText("select an agent to scope the graph");
});
